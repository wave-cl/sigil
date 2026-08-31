//! Running a chat client on a task, and telling the interface what it holds.
//!
//! `sqex_chat::Chat` is the protocol — publishing prekeys, distributing epoch
//! keys, posting and fetching — and it is already free of any interface. What
//! it is not free of is *time*: it wants driving, on a cadence, forever. This
//! does that driving on a task and hands the interface a snapshot.
//!
//! # The store is the conversation
//!
//! An epoch key arrives sealed against a single-use prekey, and opening it
//! spends that prekey. Asking the exchange for the same envelope tomorrow
//! returns the same bytes and they will not open. So the copy on disk is the
//! only copy that will ever exist, and losing it loses the conversation
//! permanently — for everyone in it, not only for us.
//!
//! Two things follow, and both are load-bearing rather than tidiness:
//!
//! - **One interactive client per account.** The store is `flock`ed for the
//!   life of the session. Two clients would each keep their own idea of the
//!   next SIP-17 message counter, and reusing a counter costs the
//!   confidentiality of two messages. sigil therefore refuses to start beside a
//!   running `sqex-chat`, and says which.
//! - **A linked second device is the only backup.** Nothing here can make one,
//!   but the interface should not let somebody discover that after a disk
//!   failure.

use std::collections::HashMap;
use std::sync::Arc;

use sqex_chat::client::{Chat, Link};
use sqex_chat::store::{self, Store};
use sqex_proto::timeline::Timeline;
use sqnr_core::{PubKey, SoftwareSigner};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use sigil_net::Dial;

/// How often the client is driven.
///
/// The same cadence `sqex-chat` uses. It is not a poll of the exchange —
/// SIP-30 pushes what changed — but the dial, the subscription and the event
/// queue all need a turn of the handle, and 700 ms is short enough that typing
/// never waits behind it.
const TICK_MS: u64 = 700;

/// One message, as the interface should draw it.
///
/// Plain data on purpose: the widgets take this rather than
/// `sqex_proto::timeline::Message`, so drawing a conversation does not require
/// understanding the wire format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub seq: u64,
    pub who: PubKey,
    /// Ours, so it can be drawn on the other side.
    pub mine: bool,
    pub at: u64,
    pub text: String,
    /// Shown as a gap rather than removed: the tombstone is the record.
    pub redacted: bool,
    /// Presenting an edit as though it were the original hides that the text
    /// changed after it was read.
    pub edited: bool,
}

/// One conversation in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub channel: [u8; 32],
    /// The other party, for a direct message.
    pub peer: Option<PubKey>,
    pub label: String,
    pub unread: usize,
    /// They have published no prekeys, so nothing can be sealed to them yet.
    /// A conversation waiting to start, **not** a failure to open one — the
    /// difference is what somebody sees on the screen.
    pub waiting: bool,
}

/// Everything the interface needs to draw chat.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatState {
    pub me: Option<PubKey>,
    /// Up, retrying, or gone. Drawn with the *word* beside the colour: a
    /// colour on its own is not a message.
    pub link: LinkState,
    pub trouble: Option<String>,
    pub conversations: Vec<Summary>,
    /// Which conversation is on screen, and what is in it.
    pub open: Option<[u8; 32]>,
    pub lines: Vec<Line>,
    /// Somebody is typing in the open conversation (SIP-19's only signal).
    pub typing: bool,
    /// Entries held under a superseded epoch, gone for good. Said out loud
    /// rather than silently missing.
    pub lost: usize,
}

/// [`Link`] without a dependency on the chat crate, and `Default`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LinkState {
    #[default]
    Up,
    Retrying,
    /// Down through the whole backoff ramp. Still trying.
    Gone,
}

impl LinkState {
    pub fn word(self) -> &'static str {
        match self {
            LinkState::Up => "connected",
            LinkState::Retrying => "reconnecting…",
            LinkState::Gone => "offline",
        }
    }
}

impl From<Link> for LinkState {
    fn from(l: Link) -> Self {
        match l {
            Link::Up => LinkState::Up,
            Link::Retrying => LinkState::Retrying,
            Link::Gone => LinkState::Gone,
        }
    }
}

/// What the interface asks the task to do.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Open a direct message with somebody, creating it if need be.
    OpenDm(PubKey),
    /// Show an existing conversation.
    Show([u8; 32]),
    /// Post to whatever is open.
    Send(String),
    /// Remember somebody, so they appear in the list before they write.
    AddContact(PubKey, String),
    /// Redial now, whatever the backoff had planned.
    Reconnect,
}

pub struct ChatHandle {
    state: watch::Receiver<ChatState>,
    cmds: mpsc::UnboundedSender<Cmd>,
    task: JoinHandle<()>,
}

impl ChatHandle {
    pub fn state(&self) -> ChatState {
        self.state.borrow().clone()
    }

    /// Ask the task to do something. Never blocks, and never fails visibly: a
    /// dropped task means the session is over, which the state already says.
    pub fn send(&self, cmd: Cmd) {
        let _ = self.cmds.send(cmd);
    }

    pub fn stop(&self) {
        self.task.abort();
    }

    pub async fn changed(&mut self) -> Result<(), String> {
        self.state
            .changed()
            .await
            .map_err(|_| "the chat session ended".to_string())
    }
}

/// Start a chat session.
///
/// `store_at` overrides where the database lives; `None` uses the real
/// `~/.sqex/chat`. Tests must always pass one — the real store is somebody's
/// only copy of their conversations.
pub fn start(
    dial: impl Into<Dial>,
    signer: SoftwareSigner,
    store_at: Option<std::path::PathBuf>,
    wake: impl Fn() + Send + Sync + 'static,
) -> ChatHandle {
    let (state_tx, state_rx) = watch::channel(ChatState::default());
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let wake = Arc::new(wake);
    let dial = dial.into();

    let task = tokio::spawn(async move {
        if let Err(e) = run(
            dial,
            signer,
            store_at,
            state_tx.clone(),
            cmd_rx,
            wake.clone(),
        )
        .await
        {
            state_tx.send_modify(|s| s.trouble = Some(e));
            (wake)();
        }
    });

    ChatHandle {
        state: state_rx,
        cmds: cmd_tx,
        task,
    }
}

async fn run(
    dial: Dial,
    signer: SoftwareSigner,
    store_at: Option<std::path::PathBuf>,
    state: watch::Sender<ChatState>,
    mut cmds: mpsc::UnboundedReceiver<Cmd>,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> Result<(), String> {
    use sqnr_core::Signer;
    let seed = signer.seed();
    let me = PubKey::new(signer.public());

    let path = match store_at {
        Some(p) => p,
        None => store::store_path(&me).map_err(|e| e.to_string())?,
    };
    // Held for the life of the session. See the module note: two interactive
    // clients would disagree about the next message counter, and reusing one
    // costs the confidentiality of two messages.
    let _lock = store::lock(&path)
        .map_err(|e| format!("another client is already using this account's store: {e}"))?;
    let store = Store::open(&seed, Some(&path)).map_err(|e| e.to_string())?;

    let endpoint = match &dial {
        Dial::At(e) => *e,
        Dial::Discover(layers) => {
            let mut silent = sqex_voice::engine::Silent;
            sqex_voice::engine::resolve(layers, &mut silent).await?
        }
    };
    let client =
        sqnr::Client::connect_as(endpoint.address, endpoint.server.as_bytes(), &seed).await?;
    let mut chat = Chat::new(client, seed, me, endpoint.server, store);
    // So a lost connection can be rebuilt without restarting the session.
    chat.dials(
        endpoint.address,
        endpoint.server.as_bytes().to_owned(),
    );
    chat.top_up_prekeys().await.map_err(|e| e.to_string())?;

    state.send_modify(|s| s.me = Some(me));
    (wake)();

    let mut timelines: HashMap<[u8; 32], Timeline> = HashMap::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(TICK_MS));

    loop {
        tokio::select! {
            // Commands first and unconditionally. Typing must never wait behind
            // the network, which is the discipline `sqex-chat`'s own loop keeps
            // by handling keys before anything else.
            Some(cmd) = cmds.recv() => {
                apply(&mut chat, cmd, &state, &mut timelines).await;
                (wake)();
            }
            _ = tick.tick() => {
                chat.keep_alive().await;
                if chat.link() == Link::Up && !chat.subscribed() {
                    let _ = chat.subscribe().await;
                }
                // The events say only *what changed*; the fetch below is what
                // reads it. Draining them here keeps the queue from growing.
                let _ = chat.take_events();
                refresh(&mut chat, &state, &mut timelines).await;
                (wake)();
            }
        }
    }
}

async fn apply(
    chat: &mut Chat,
    cmd: Cmd,
    state: &watch::Sender<ChatState>,
    timelines: &mut HashMap<[u8; 32], Timeline>,
) {
    match cmd {
        Cmd::OpenDm(peer) => match chat.open_dm(&peer).await {
            Ok(channel) => {
                // Opening a conversation and minting its key are separate, and
                // have to be: a direct message can be opened with somebody who
                // has never run a client, but SIP-23 forbids sealing a key to a
                // device with no prekeys. That is a conversation waiting to
                // start, not a failure to open one.
                let waiting = chat.ensure_epoch(&channel).await.is_err();
                state.send_modify(|s| {
                    s.open = Some(channel);
                    s.lines.clear();
                    if let Some(c) = s.conversations.iter_mut().find(|c| c.channel == channel) {
                        c.waiting = waiting;
                    }
                });
                timelines.entry(channel).or_default();
            }
            Err(e) => state.send_modify(|s| s.trouble = Some(e.to_string())),
        },
        Cmd::Show(channel) => {
            state.send_modify(|s| {
                s.open = Some(channel);
                s.lines.clear();
            });
            timelines.entry(channel).or_default();
        }
        Cmd::Send(text) => {
            let open = state.borrow().open;
            let Some(channel) = open else { return };
            if let Err(e) = chat.send(&channel, &text).await {
                // The text is not thrown away here; the interface keeps it in
                // the composer, because retyping a message the program lost is
                // the worst thing a chat client can do to somebody.
                state.send_modify(|s| s.trouble = Some(e.to_string()));
            }
        }
        Cmd::AddContact(who, label) => {
            // Contacts are the store's, not the protocol's: adding somebody is
            // a note to ourselves that they exist, and involves the exchange
            // not at all.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = chat.store().add_contact(&who, &label, now);
        }
        Cmd::Reconnect => chat.reconnect_now(),
    }
}

/// Refetch what is on screen and rebuild the conversation list.
async fn refresh(
    chat: &mut Chat,
    state: &watch::Sender<ChatState>,
    timelines: &mut HashMap<[u8; 32], Timeline>,
) {
    let me = state.borrow().me;
    let open = state.borrow().open;

    let mut summaries = Vec::new();
    if let Ok(contacts) = chat.store().contacts() {
        for contact in contacts {
            let channel = chat.dm_with(&contact.account);
            summaries.push(Summary {
                channel,
                peer: Some(contact.account),
                label: if contact.label.is_empty() {
                    contact.account.to_string()
                } else {
                    contact.label.clone()
                },
                unread: 0,
                waiting: false,
            });
        }
    }

    let mut lines = Vec::new();
    let mut typing = false;
    let mut lost = 0;
    if let Some(channel) = open {
        let timeline = timelines.entry(channel).or_default();
        // `wait_secs: 0` -- a long poll here would hold the tick open and make
        // every command wait behind it.
        if let Ok(conversation) = chat.poll(&channel, timeline, 0).await {
            typing = conversation.typing;
            lost = conversation.lost;
            lines = conversation
                .timeline
                .messages()
                .map(|m| Line {
                    seq: m.seq,
                    who: m.account,
                    mine: Some(m.account) == me,
                    at: m.posted,
                    text: m.post.body_text().unwrap_or_default().to_string(),
                    redacted: m.redacted,
                    edited: m.edited.is_some(),
                })
                .collect();
            // Reading advances the read mark, which is what makes "where was I"
            // survive closing the client.
            if let Some(last) = lines.last().map(|l| l.seq) {
                let _ = chat.mark_read(&channel, last).await;
            }
        }
    }

    let link = LinkState::from(chat.link());
    state.send_modify(|s| {
        s.link = link;
        s.conversations = summaries;
        s.lines = lines;
        s.typing = typing;
        s.lost = lost;
    });
}
