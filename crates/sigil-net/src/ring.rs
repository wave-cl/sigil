//! Listening for somebody to call you.
//!
//! `sqex_voice::ring` defines what a ring *is*. This keeps one running: a task
//! that sweeps the mailbox on its own connection, so a ring arrives whether or
//! not there is a call in progress and whether or not the window is on screen.
//!
//! # It polls, and that is a stopgap
//!
//! The mailbox has to be *asked*. SIP-30's event stream — which chat already
//! holds open — has kinds for channels, cursors, membership, profiles and
//! admission, and **none for mail**, so nothing pushes a ring. Until it does,
//! this sweeps.
//!
//! The cost is exactly what SIP-30 exists to remove. `sqexd` counts requests
//! "because nothing else could say how much this exchange is being asked", and
//! a polling client costs requests proportional to how long it has been
//! running rather than to what has happened. One idle client at
//! [`SWEEP`](Self::SWEEP) is about a thousand requests a day that mostly find
//! nothing.
//!
//! Replacing it is one function: [`RingListener::sweep`] becomes a read from a
//! subscription. Everything else — the connection, the block list, the
//! delivery to the interface — stays. That seam is deliberate, and it is not
//! wrapped in a trait with one implementation, because an abstraction invented
//! before its second case usually fits neither.

use std::sync::Arc;
use std::time::Duration;

use sqex_voice::engine::{self, Endpoint, Report, Silent};
use sqex_voice::ring::{self, Ring};
use sqnr_core::{PubKey, SoftwareSigner};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::call::Dial;

/// A ring that has arrived and not yet been answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Incoming {
    pub from: PubKey,
    /// When the exchange received it.
    pub at: u64,
}

impl From<Ring> for Incoming {
    fn from(r: Ring) -> Self {
        Incoming {
            from: r.from,
            at: r.at,
        }
    }
}

/// Whether anybody is listening, and why not if not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListenerState {
    /// Rings are being collected. **When this is false, calls do not arrive**,
    /// and an interface must say so rather than looking idle: a phone that has
    /// quietly stopped ringing is worse than one that is obviously broken.
    pub listening: bool,
    /// Why it stopped, if it did.
    pub trouble: Option<String>,
}

pub struct RingListener {
    state: watch::Receiver<ListenerState>,
    rings: mpsc::UnboundedReceiver<Incoming>,
    blocked: watch::Sender<Vec<PubKey>>,
    task: JoinHandle<()>,
}

impl RingListener {
    /// How often the mailbox is swept.
    ///
    /// Two seconds is the longest anyone should wait to be told the phone is
    /// ringing, and it is already the exchange's own room heartbeat, so it does
    /// not introduce a new rhythm. Whatever this is set to is the worst-case
    /// delay somebody experiences before their phone rings.
    pub const SWEEP: Duration = Duration::from_secs(2);

    /// How long to wait before trying again after the connection fails.
    ///
    /// Deliberately not a backoff ramp: while this is down no call can arrive
    /// at all, so backing off to a minute would mean quietly missing calls for
    /// a minute. Chat can afford patience because its messages wait; a ring
    /// does not wait, it expires.
    const RETRY: Duration = Duration::from_secs(5);

    pub fn state(&self) -> ListenerState {
        self.state.borrow().clone()
    }

    /// Everything that has rung since the last drain.
    pub fn drain(&mut self) -> Vec<Incoming> {
        let mut out = Vec::new();
        while let Ok(r) = self.rings.try_recv() {
            out.push(r);
        }
        out
    }

    /// Replace the set of people who do not ring.
    ///
    /// Takes effect on the next sweep. One list, shared with whatever else
    /// blocks people — there should be a single answer to "make them stop".
    pub fn set_blocked(&self, blocked: Vec<PubKey>) {
        let _ = self.blocked.send(blocked);
    }

    pub fn stop(&self) {
        self.task.abort();
    }

    /// Await the next change in whether rings are arriving.
    pub async fn changed(&mut self) -> Result<(), String> {
        self.state
            .changed()
            .await
            .map_err(|_| "the listener stopped".to_string())
    }
}

/// Start listening.
///
/// Holds a connection of its own rather than borrowing a call's, because it has
/// to be listening precisely when there is no call. `sqexd` is explicit that an
/// identity may hold several connections at once, so this is a supported thing
/// to do rather than a trick.
pub fn listen(
    dial: impl Into<Dial>,
    signer: SoftwareSigner,
    wake: impl Fn() + Send + Sync + 'static,
) -> RingListener {
    let (state_tx, state_rx) = watch::channel(ListenerState::default());
    let (rings_tx, rings_rx) = mpsc::unbounded_channel();
    let (blocked_tx, blocked_rx) = watch::channel(Vec::new());
    let wake = Arc::new(wake);
    let dial = dial.into();

    let task = tokio::spawn(async move {
        run(dial, signer, state_tx, rings_tx, blocked_rx, wake).await;
    });

    RingListener {
        state: state_rx,
        rings: rings_rx,
        blocked: blocked_tx,
        task,
    }
}

async fn run(
    dial: Dial,
    signer: SoftwareSigner,
    state: watch::Sender<ListenerState>,
    rings: mpsc::UnboundedSender<Incoming>,
    blocked: watch::Receiver<Vec<PubKey>>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    let seed = signer.seed();

    // Resolved once. A listener that re-resolved every sweep would do a DNSSEC
    // lookup every two seconds for as long as the program is open.
    let endpoint = match resolve_once(&dial).await {
        Ok(e) => e,
        Err(e) => {
            state.send_replace(ListenerState {
                listening: false,
                trouble: Some(e),
            });
            (wake)();
            return;
        }
    };

    loop {
        let mut client = match engine::connect(endpoint, &signer, &mut Silent).await {
            Ok(c) => c,
            Err(e) => {
                state.send_replace(ListenerState {
                    listening: false,
                    trouble: Some(format!("not listening for calls: {e}")),
                });
                (wake)();
                tokio::time::sleep(RingListener::RETRY).await;
                continue;
            }
        };
        state.send_replace(ListenerState {
            listening: true,
            trouble: None,
        });
        (wake)();

        // Sweep until the connection fails, then build a new one. A failure
        // here is not fatal to the program -- it is a phone that has to be
        // picked up again.
        loop {
            tokio::time::sleep(RingListener::SWEEP).await;
            let blocked_now = blocked.borrow().clone();
            match ring::collect(&mut client, &seed, &blocked_now).await {
                Ok(found) => {
                    if !found.is_empty() {
                        for r in found {
                            let _ = rings.send(r.into());
                        }
                        (wake)();
                    }
                }
                Err(e) => {
                    state.send_replace(ListenerState {
                        listening: false,
                        trouble: Some(format!("not listening for calls: {e}")),
                    });
                    (wake)();
                    break;
                }
            }
        }
        tokio::time::sleep(RingListener::RETRY).await;
    }
}

async fn resolve_once(dial: &Dial) -> Result<Endpoint, String> {
    match dial {
        Dial::At(e) => Ok(*e),
        Dial::Discover(layers) => {
            let mut silent = Silent;
            engine::resolve(layers, &mut silent as &mut dyn Report).await
        }
    }
}
