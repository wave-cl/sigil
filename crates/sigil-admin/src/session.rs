//! The task that talks to an exchange as an administrator.
//!
//! # What is different about this one
//!
//! Every other connection sigil holds acts for a person in a conversation.
//! This one acts on the exchange itself, and the difference shows in three
//! places:
//!
//! - **Everything that changes anything is signed.** A SIP-10 transaction is
//!   bound to a fresh server nonce, so a captured signature cannot be applied
//!   twice, and a dropped connection is retried by signing again rather than by
//!   resending.
//! - **The signing happens after a human has seen what will be signed.**
//!   `sign_and_submit` takes an `on_review` callback, but it is called *during*
//!   signing and cannot ask anything — it reports. So the confirmation is the
//!   interface's job, before the transaction is built at all.
//! - **A batch is atomic.** The exchange applies all of it or none, so what is
//!   confirmed is the batch and not the operations one at a time.

use std::sync::Arc;

use sigil_net::Dial;
use sqex_proto::Op;
use sqnr::signer::Backend;
use sqnr_core::{PubKey, SoftwareSigner};
use tokio::sync::{mpsc, watch};

/// How often the console checks the exchange is still answering.
const TICK_MS: u64 = 2_000;

/// What the console asks for.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Re-read `/health` and `/status`, which need no signature.
    Probe,
    /// Sign and submit a batch. **Already confirmed** by the time it gets here.
    Submit(Vec<Op>),
}

/// One answer from the exchange, kept so the operator can read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// What was asked, in the operator's words.
    pub asked: String,
    /// What came back, pretty-printed, or the refusal.
    pub said: String,
    pub refused: bool,
}

/// Everything the console draws.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AdminState {
    /// Who we are signing as.
    pub admin: Option<PubKey>,
    /// The exchange we are administering.
    pub exchange: Option<PubKey>,
    /// Whether it answered `/health`.
    ///
    /// Unsigned and unauthenticated, so it says the exchange is *up* and
    /// nothing about whether we may administer it. Those are different
    /// questions and an operator wants both.
    pub healthy: Option<bool>,
    /// Its own numbers, from `/status`.
    pub status: Option<String>,
    /// What has been asked and answered, newest first.
    pub answers: Vec<Answer>,
    /// Why the console cannot work, if it cannot.
    pub trouble: Option<String>,
    /// A submission is in flight. Nothing else may be sent meanwhile: two
    /// batches racing for one nonce is one wasted signature and one confusing
    /// refusal.
    pub busy: bool,
}

pub struct AdminHandle {
    state: watch::Receiver<AdminState>,
    cmds: mpsc::UnboundedSender<Cmd>,
    /// Dropping this is what stops the session: the loop selects on it, and a
    /// closed channel is the signal to leave.
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl AdminHandle {
    pub fn state(&self) -> AdminState {
        self.state.borrow().clone()
    }

    pub fn send(&self, cmd: Cmd) {
        let _ = self.cmds.send(cmd);
    }

    pub fn stop(&mut self) {
        self.stop.take();
    }
}

/// Start an administrative session.
///
/// # Why this gets a thread of its own
///
/// `sqnr::flow::sign_and_submit` takes its review and touch callbacks as
/// `&dyn Fn(..)` with no `Sync` bound, and holds them across an await. A future
/// containing one is therefore not `Send`, and `tokio::spawn` will not take it.
///
/// The alternative was to inline the challenge/sign/submit sequence here, and
/// that is the wrong trade by a long way: the nonce binding is the whole
/// authority protocol — the card only ever signs a transaction bound to a fresh
/// server nonce, which is what stops a captured signature being applied twice.
/// A second copy of that is a second place for it to be subtly wrong. So the
/// session runs on a current-thread runtime on a thread of its own, and the
/// shared flow stays shared.
pub fn start(
    dial: impl Into<Dial>,
    signer: SoftwareSigner,
    wake: impl Fn() + Send + Sync + 'static,
) -> AdminHandle {
    let (state_tx, state_rx) = watch::channel(AdminState::default());
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let wake = Arc::new(wake);
    let dial = dial.into();

    std::thread::Builder::new()
        .name("sigil-admin".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    state_tx.send_modify(|s| s.trouble = Some(e.to_string()));
                    (wake)();
                    return;
                }
            };
            runtime.block_on(async move {
                let running = run(dial, signer, state_tx.clone(), cmd_rx, wake.clone());
                tokio::select! {
                    outcome = running => {
                        if let Err(e) = outcome {
                            state_tx.send_modify(|s| s.trouble = Some(e));
                            (wake)();
                        }
                    }
                    // The handle was dropped or `stop` called.
                    _ = stop_rx => {}
                }
            });
        })
        .expect("spawning a thread for the admin session");

    AdminHandle {
        state: state_rx,
        cmds: cmd_tx,
        stop: Some(stop_tx),
    }
}

async fn run(
    dial: Dial,
    signer: SoftwareSigner,
    state: watch::Sender<AdminState>,
    mut cmds: mpsc::UnboundedReceiver<Cmd>,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> Result<(), String> {
    use sqnr_core::Signer;
    let seed = signer.seed();
    let me = PubKey::new(signer.public());

    let endpoint = match &dial {
        Dial::At(e) => *e,
        Dial::Discover(layers) => {
            let mut silent = sqex_voice::engine::Silent;
            sqex_voice::engine::resolve(&layers[..], &mut silent).await?
        }
    };
    let mut client =
        sqnr::Client::connect_as(endpoint.address, endpoint.server.as_bytes(), &seed).await?;
    // A software identity only. A YubiKey signs and never releases a seed, so
    // it cannot be a transport key — the card would sign the transaction and
    // there would be no connection to send it over. Supporting one means a
    // second identity for the connection, which is a decision rather than a
    // detail.
    let backend = Backend::software(signer);

    state.send_modify(|s| {
        s.admin = Some(me);
        s.exchange = Some(endpoint.server);
    });
    (wake)();

    let mut tick = tokio::time::interval(std::time::Duration::from_millis(TICK_MS));
    loop {
        tokio::select! {
            Some(cmd) = cmds.recv() => {
                apply(&mut client, &backend, endpoint.server, cmd, &state).await;
                (wake)();
            }
            _ = tick.tick() => {
                probe(&mut client, &state).await;
                (wake)();
            }
        }
    }
}

/// `/health` and `/status`: the two questions that need no signature.
async fn probe(client: &mut sqnr::Client, state: &watch::Sender<AdminState>) {
    let healthy = matches!(client.get("/health").await, Ok((200, _)));
    let status = match client.get("/status").await {
        Ok((200, body)) => Some(pretty(&body)),
        _ => None,
    };
    state.send_modify(|s| {
        s.healthy = Some(healthy);
        s.status = status;
    });
}

async fn apply(
    client: &mut sqnr::Client,
    backend: &Backend,
    server: PubKey,
    cmd: Cmd,
    state: &watch::Sender<AdminState>,
) {
    match cmd {
        Cmd::Probe => probe(client, state).await,
        Cmd::Submit(ops) => {
            if ops.is_empty() {
                return;
            }
            let asked = ops
                .iter()
                .map(|op| op.to_operation().summary)
                .collect::<Vec<_>>()
                .join("; ");
            state.send_modify(|s| s.busy = true);

            let operations: Vec<sqnr_core::Operation> =
                ops.iter().map(|op| op.to_operation()).collect();
            // `on_review` reports and cannot ask: it runs inside the signing,
            // after the nonce is fetched. The confirmation already happened, in
            // the interface, before this was sent.
            let review = |txn: &sqnr_core::Transaction| {
                tracing::info!("signing {} operation(s)", txn.ops.len());
            };
            let touch = || tracing::info!("touch the card to sign");
            let result =
                sqnr::flow::sign_and_submit(client, backend, server, operations, &review, &touch)
                    .await;

            let answer = match result {
                Ok(value) => Answer {
                    asked,
                    said: serde_json::to_string_pretty(&value)
                        .unwrap_or_else(|_| value.to_string()),
                    refused: false,
                },
                Err(e) => Answer {
                    asked,
                    said: e,
                    refused: true,
                },
            };
            state.send_modify(|s| {
                s.busy = false;
                s.answers.insert(0, answer);
                s.answers.truncate(50);
            });
        }
    }
}

/// A JSON body, laid out for reading.
fn pretty(body: &[u8]) -> String {
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
        // Not JSON. Shown as it came rather than guessed at.
        Err(_) => String::from_utf8_lossy(body).into_owned(),
    }
}
