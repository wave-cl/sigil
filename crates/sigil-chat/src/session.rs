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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use sqex_chat::client::{Chat, Link};
use sqex_chat::store::{self, Store};
use sqex_proto::channel::{Role, Visibility};
use sqex_proto::events::Event;
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
    /// Their display name, if a profile has been seen for them.
    ///
    /// Self-declared and attested by nobody, so it is drawn as ordinary text
    /// with the key always reachable beside it (SIP-21). `None` falls back to
    /// the key, which is never wrong.
    pub name: Option<String>,
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
    /// The last thing said, for the list. `None` when nothing has been.
    pub preview: Option<String>,
    /// When that was, for the list's time column.
    pub at: Option<u64>,
    /// Anybody may find and join it, and **nothing in it is encrypted** —
    /// everyone who may join would hold any key it used, so encrypting would
    /// look end-to-end and not be. A reader has to see this before they type.
    pub public: bool,
    /// More than two people.
    pub group: bool,
    /// Somebody in it is typing.
    pub typing: bool,
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
    /// What is wrong with the open conversation, if anything.
    pub trouble_with: Trouble,
    /// Everybody we can put a name to, keyed by account.
    ///
    /// One map for the whole interface rather than a lookup per view, so there
    /// is a single answer to "what is this person called" and no view can
    /// quietly disagree with another.
    pub people: HashMap<PubKey, Person>,
    /// Our own profile, for the pane that edits it.
    pub mine: Person,
    /// What the directory last turned up.
    pub found: Vec<Found>,
    /// A search has been run, so an empty `found` means "nothing matched"
    /// rather than "nobody has looked".
    pub searched: bool,
    /// A confirmation of something just done.
    ///
    /// **Separate from `trouble`**, which is about the state of the
    /// conversation and is rebuilt by every refresh. Merged, every
    /// confirmation would be on screen for less than a tick.
    pub note: Option<String>,
    /// Who is in the open conversation.
    pub members: Vec<Member>,
    /// Whether we may rename, invite, remove and rotate here.
    pub i_am_admin: bool,
    /// The open conversation's topic, when it has one.
    pub topic: String,
    /// The first message that was unread when this conversation was opened.
    ///
    /// **Frozen on entry.** Reading advances the read mark, so a divider that
    /// tracked it would disappear exactly when somebody wanted to see where
    /// they had got to.
    pub divider: Option<u64>,
    /// How many there were, for the divider's label. Frozen with it.
    pub unread_on_open: usize,
}

/// A public channel the directory turned up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub channel: [u8; 32],
    /// Which incarnation the directory is showing. Carried because a joiner
    /// has to sign against it and cannot ask `Info`, which requires the
    /// membership they are trying to acquire.
    pub instance: [u8; 32],
    pub name: String,
    pub topic: String,
    pub members: u16,
}

/// Who is in a conversation, and what they may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub account: PubKey,
    /// May redact, rename, invite and mint a new epoch. **Attested by the
    /// exchange**, unlike a SIP-21 title, which is why it may be shown as a
    /// role and a title may not.
    pub admin: bool,
}

/// How somebody can be named.
///
/// **None of this is attested.** A SIP-21 profile is self-declared: the
/// exchange stores it and vouches for none of it. A handle is bound at the
/// exchange, so it says more, but it still is not the person. The key is the
/// only thing that identifies somebody, which is why every view that shows a
/// name keeps the key one gesture away.
///
/// A profile that is **withheld, absent, or blocked answers identically** by
/// design (SIP-4's rule, and SIP-21 keeps it). So `None` here means "we cannot
/// name them", never "they have nothing" and never "they blocked you" — the
/// interface must not invent a distinction the protocol refuses to make.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Person {
    /// Self-declared display name.
    pub name: Option<String>,
    /// Self-declared standing. **Never rendered as a badge, in channel-role
    /// styling, or beside a verification mark** — SIP-21 makes those MUSTs,
    /// because a title asserts authority directly and "Exchange
    /// Administrator" does the social engineering by itself.
    pub title: Option<String>,
    /// `name@domain`, bound at the exchange (SIP-38).
    pub handle: Option<String>,
}

impl Person {
    /// What to call them, falling back to the key, which is never wrong.
    pub fn label(&self, key: &PubKey) -> String {
        self.name
            .clone()
            .or_else(|| self.handle.clone())
            .unwrap_or_else(|| key.to_string())
    }

    pub fn is_empty(&self) -> bool {
        *self == Person::default()
    }
}

/// What is wrong with a conversation, as against what is in it.
///
/// **These are not interchangeable and a client must not collapse them.** Each
/// says something different about what to do, and the two that look alike are
/// the two that matter most:
///
/// - `unreadable` is an entry whose key **may still arrive**. Wait.
/// - `lost` is an entry under a superseded epoch. It is **gone**, and telling
///   somebody to wait for it wastes their time forever.
/// - `gap` is history that fell outside the retention window. It was never
///   ours to have.
/// - `forked` is SIP-31 evidence that the exchange signed two histories for one
///   position. It **must** be surfaced, and must never be shown as a `gap` —
///   one is ordinary and the other is misconduct.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trouble {
    /// Entries held and not opened. A key for them may still come.
    pub unreadable: usize,
    /// Entries under an epoch we will never hold a key for. Gone.
    pub lost: usize,
    /// We were away longer than the retention window; there is history that
    /// can never be filled in. Shown as a gap, never as the whole conversation.
    pub gap: bool,
    /// The channel was destroyed and recreated under the same identifier, so
    /// what came before is unrelated to what follows (SIP-16).
    pub restarted: bool,
    /// The epoch in force, when we hold no key for it: SIP-17's *stranded*
    /// member, who can fetch every entry and open none of them.
    pub no_key: Option<u32>,
}

impl Trouble {
    pub fn is_clear(&self) -> bool {
        *self == Trouble::default()
    }
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
    /// Put the open conversation away. What "back" means in a single pane.
    Close,
    /// Publish a display name and title (SIP-21). Empty clears them.
    SetProfile {
        name: String,
        title: String,
    },

    // ---- making conversations ------------------------------------------
    /// A private group. Its name is a sealed entry, not the exchange's.
    NewGroup(String),
    /// A public channel. Anybody may find and join it, and **nothing in it is
    /// encrypted** — everyone who may join would hold any key it used.
    NewPublic {
        name: String,
        topic: String,
    },
    /// Search the public directory. Empty lists everything (SIP-16).
    Find(String),
    /// Join a public channel found in the directory.
    ///
    /// The incarnation comes from the directory row and has to: SIP-31 binds
    /// it into the signature, and `Info` — the other place it appears —
    /// requires the membership this call is asking for.
    Join {
        channel: [u8; 32],
        instance: [u8; 32],
    },

    // ---- running one ---------------------------------------------------
    /// Add somebody, and seal them the **current** epoch key, which grants them
    /// the history. Rotating instead would deny it — a different decision, and
    /// not one to make on somebody's behalf without saying so.
    Invite(PubKey),
    /// Remove somebody. **This rotates**: a removed member keeps every key they
    /// were ever given, so without a new epoch they could go on reading from
    /// the exchange's own copy.
    Kick(PubKey),
    /// Make somebody an admin, or stop them being one.
    Grant {
        who: PubKey,
        admin: bool,
    },
    /// Mint a new epoch for everybody present.
    Rotate,
    /// Leave. For a direct message this removes only us — leaving a
    /// conversation must not delete the other person's copy.
    Leave,
    /// **Destroy the channel for everybody.** Not the same as `Close`, which
    /// only puts it away on this screen.
    Destroy,
    SetName(String),
    SetTopic(String),
    /// Retention window and an optional entry cap. **Narrowing is a deletion**,
    /// applied at once, not a policy that takes effect later.
    SetRetention {
        secs: u32,
        max_entries: u32,
    },
}

pub struct ChatHandle {
    state: watch::Receiver<ChatState>,
    cmds: mpsc::UnboundedSender<Cmd>,
    task: JoinHandle<()>,
}

/// A session that has been told to stop but may not have finished stopping.
///
/// **The store's `flock` is released when the task's future is dropped, not
/// when `abort` is called.** So an identity that was just closed cannot be
/// reopened straight away: the new session would ask for a lock the old one
/// still holds and be refused with `StoreError::InUse`, which reads as
/// "another client is already using this account" and names sigil itself.
///
/// Holding one of these and waiting for [`Closing::is_finished`] is how a
/// switch waits without blocking the interface to do it.
pub struct Closing {
    task: JoinHandle<()>,
}

impl Closing {
    /// Whether the task is really gone, and the store lock with it.
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
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

    /// Stop, and hand back something that says when the store lock is free.
    ///
    /// Prefer this to [`stop`](Self::stop) anywhere the account might be
    /// reopened — see [`Closing`].
    #[must_use = "the store lock is held until the closing task finishes"]
    pub fn close(self) -> Closing {
        self.task.abort();
        Closing { task: self.task }
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
            sqex_voice::engine::resolve(&layers[..], &mut silent).await?
        }
    };
    let client =
        sqnr::Client::connect_as(endpoint.address, endpoint.server.as_bytes(), &seed).await?;
    let mut chat = Chat::new(client, seed, me, endpoint.server, store);
    // So a lost connection can be rebuilt without restarting the session.
    chat.dials(endpoint.address, endpoint.server.as_bytes().to_owned());
    chat.top_up_prekeys().await.map_err(|e| e.to_string())?;

    state.send_modify(|s| s.me = Some(me));
    (wake)();

    let mut desk = Desk::default();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(TICK_MS));

    loop {
        tokio::select! {
            // Commands first and unconditionally. Typing must never wait behind
            // the network, which is the discipline `sqex-chat`'s own loop keeps
            // by handling keys before anything else.
            Some(cmd) = cmds.recv() => {
                apply(&mut chat, cmd, &state, &mut desk).await;
                (wake)();
            }
            _ = tick.tick() => {
                chat.keep_alive().await;
                if chat.link() == Link::Up && !chat.subscribed() {
                    let _ = chat.subscribe().await;
                }
                // **SIP-30's events say which channels moved**, and that is
                // what decides where to look. This used to drain and discard
                // them and poll only whatever was on screen, so a message
                // arriving in any other conversation was invisible until
                // somebody happened to open it -- the unread count could not
                // have worked, and did not.
                for event in chat.take_events() {
                    desk.note(event, me);
                }
                desk.age();
                if desk.restructure {
                    // **Cleared only on success.** Clearing it first meant a
                    // rebuild that failed -- which is what every rebuild does
                    // before the connection is up -- was not tried again until
                    // the backstop came round half a minute later. For that
                    // half minute the conversation list was empty and nothing
                    // said why.
                    match sync_channels(&mut chat, &mut desk).await {
                        Ok(()) => {
                            desk.restructure = false;
                            desk.since_sync = 0;
                        }
                        Err(e) => state.send_modify(|s| s.trouble = Some(e)),
                    }
                }
                learn_names(&mut chat, &mut desk).await;
                refresh(&mut chat, &state, &mut desk, me).await;
                (wake)();
            }
        }
    }
}

/// One conversation, as this client knows it.
struct Known {
    /// The other party, for a direct message. `None` for a group or a public
    /// channel.
    peer: Option<PubKey>,
    /// Anybody may find and join it, and nothing in it is encrypted.
    public: bool,
    /// More than two people.
    group: bool,
    label: String,
    /// Who may redact and rename. From the exchange, remembered so that a
    /// client starting offline folds its own history correctly.
    admins: Vec<PubKey>,
    /// Everybody in it, with the role the **exchange** attests.
    members: Vec<Member>,
    timeline: Timeline,
    /// How many messages we had last time, so a new one can be counted unread
    /// without diffing two timelines.
    seen: usize,
    /// The newest thing said here. What the list sorts on.
    last_at: u64,
    unread: usize,
    /// They have published no prekeys, so nothing can be sealed to them yet.
    waiting: bool,
    typing: bool,
    trouble: Trouble,
}

impl Known {
    fn summary(&self, channel: [u8; 32], me: &PubKey) -> Summary {
        Summary {
            channel,
            peer: self.peer,
            label: self.label.clone(),
            unread: self.unread,
            waiting: self.waiting,
            preview: self.preview(me),
            at: (self.last_at > 0).then_some(self.last_at),
            public: self.public,
            group: self.group,
            typing: self.typing,
        }
    }

    /// The newest thing said here, whoever said it.
    ///
    /// A redaction shows as the gap it is rather than being skipped, or the
    /// list would claim the conversation ended at an older message.
    fn preview(&self, me: &PubKey) -> Option<String> {
        let m = self.timeline.messages().last()?;
        let said = if m.redacted {
            "message deleted".to_string()
        } else {
            m.post
                .body_text()
                .map(str::to_string)
                .unwrap_or_else(|| "a file".to_string())
        };
        // In a group half of what the line is worth is *who*: the row already
        // names the channel, so "lol" on its own says nothing about whether it
        // is worth opening. A direct message needs no prefix -- the row is
        // the person.
        if self.peer.is_some() {
            return Some(said);
        }
        let who = if m.account == *me {
            "you".to_string()
        } else {
            let key = m.account.to_string();
            key.chars().take(8).collect()
        };
        Some(format!("{who}: {said}"))
    }
}

/// The session's own view of the world, kept between ticks.
struct Desk {
    channels: HashMap<[u8; 32], Known>,
    /// Which conversation is on screen. Held here as well as in the published
    /// state so the task can read it without borrowing the watch channel.
    open: Option<[u8; 32]>,
    /// Channels an event says have changed. Only these are fetched.
    dirty: HashSet<[u8; 32]>,
    /// The conversation list itself needs rebuilding from the exchange.
    restructure: bool,
    /// Accounts whose profile an event says has moved on.
    restale: HashSet<PubKey>,
    /// Ticks since the list was last rebuilt.
    ///
    /// A backstop, not the mechanism. Events are what make this responsive,
    /// but a subscription can drop and reconnect with a gap in it, and a
    /// conversation list that is only ever event-driven would then be wrong
    /// until something else happened to change it -- which, for somebody who
    /// has been added to a channel and told about it nowhere else, is never.
    since_sync: u32,
}

impl Default for Desk {
    fn default() -> Self {
        Desk {
            channels: HashMap::new(),
            open: None,
            dirty: HashSet::new(),
            restale: HashSet::new(),
            // The first tick has nothing yet, so it rebuilds.
            restructure: true,
            since_sync: 0,
        }
    }
}

/// How long the conversation list can be wrong before the backstop fixes it.
///
/// Exposed so a test can say what it is pointed at. A test that waits *longer*
/// than this proves only that the backstop works — it passes whether or not
/// SIP-30's events are being acted on at all, which is how the stranger test
/// passed at 28.9 seconds while the event path was broken.
pub const BACKSTOP: std::time::Duration =
    std::time::Duration::from_millis(TICK_MS * Desk::RESYNC_TICKS as u64);

impl Desk {
    /// How often the list is rebuilt regardless of events. See `since_sync`.
    const RESYNC_TICKS: u32 = 40;

    fn note(&mut self, event: Event, me: PubKey) {
        match event {
            Event::Channel { channel, .. } | Event::Signal { channel } => {
                self.dirty.insert(channel);
            }
            // Somebody's read mark moved. Ours moving is not news; theirs is,
            // once receipts are drawn.
            Event::Cursor { channel } => {
                self.dirty.insert(channel);
            }
            Event::Membership {
                channel, account, ..
            } => {
                self.dirty.insert(channel);
                // **A channel we have never heard of is a new conversation**,
                // whoever the event names. This is how one somebody else
                // started arrives, and getting the condition wrong costs
                // exactly that: `create` publishes the event with `account`
                // set to the *creator*, so the person being invited receives
                // one naming somebody else. Keying only on `account == me`
                // meant a stranger's first message did not appear until the
                // periodic rebuild came round half a minute later -- which
                // looked like it worked, because eventually it did.
                if account == me || !self.channels.contains_key(&channel) {
                    self.restructure = true;
                }
            }
            // We fell behind and events were dropped. Nothing local can be
            // trusted to be current, so read everything again.
            Event::Resync => {
                self.restructure = true;
                self.dirty.extend(self.channels.keys().copied());
            }
            // A profile changed. Refetching is the *only* way to know: a
            // cached name that has moved on looks exactly like a correct one,
            // so this must ignore the cache rather than merely revalidate it.
            Event::Profile { account } => {
                self.restale.insert(account);
            }
            Event::Admission
            | Event::Heartbeat
            | Event::Ringing { .. }
            | Event::CrossCall { .. }
            | Event::Unknown(_) => {}
        }
    }

    fn age(&mut self) {
        self.since_sync = self.since_sync.saturating_add(1);
        if self.since_sync >= Self::RESYNC_TICKS {
            self.restructure = true;
        }
    }
}

/// Reconcile what the exchange says we are in with what we hold.
///
/// Groups come from here and only from here: a group's identifier is random
/// rather than derived, so there is nothing to compute and nothing to guess.
/// Direct messages are matched against the contact list so a conversation keeps
/// the name its contact was given -- and a direct message from **somebody not
/// in it** still appears, which is the whole reason this cannot be built from
/// the contact list alone.
async fn sync_channels(chat: &mut Chat, desk: &mut Desk) -> Result<(), String> {
    let contacts = chat.store().contacts().map_err(|e| e.to_string())?;
    let known = chat.store().channels().map_err(|e| e.to_string())?;
    let mine = chat.mine().await.map_err(|e| e.to_string())?;
    let me = chat.me;

    let mut present: HashSet<[u8; 32]> = HashSet::new();

    for m in mine {
        present.insert(m.channel);
        let peer = contacts
            .iter()
            .find(|c| chat.dm_with(&c.account) == m.channel)
            .map(|c| (c.account, c.label.clone()));
        let remembered = known.iter().find(|k| k.0 == m.channel);
        let public = m.visibility == Visibility::Public;

        // The exchange is authoritative about who administers a channel; the
        // store is what makes that survive being offline.
        let (admins, given_name, roster) = match chat.info(&m.channel).await {
            Ok(info) => (
                info.members
                    .iter()
                    .filter(|mem| mem.role == Role::Admin)
                    .map(|mem| mem.account)
                    .collect::<Vec<_>>(),
                info.name,
                info.members
                    .iter()
                    .map(|mem| Member {
                        account: mem.account,
                        // Attested by the exchange. This is the one role that
                        // may be drawn as a role; a SIP-21 title may not.
                        admin: mem.role == Role::Admin,
                    })
                    .collect::<Vec<_>>(),
            ),
            Err(_) => (
                remembered.map(|k| k.3.clone()).unwrap_or_default(),
                String::new(),
                Vec::new(),
            ),
        };
        let members: Vec<PubKey> = roster.iter().map(|m| m.account).collect();

        // A direct message with somebody who is not a contact: the identifier
        // derives from the two accounts, so proving it is one means finding the
        // other member and checking the derivation.
        let peer = peer.or_else(|| {
            let other = members.iter().find(|a| **a != me)?;
            (chat.dm_with(other) == m.channel).then(|| (*other, other.to_string()))
        });

        let label = match &peer {
            Some((account, l)) if !l.is_empty() => {
                if l == &account.to_string() {
                    // An unnamed contact's label is only its key repeated.
                    account.to_string()
                } else {
                    l.clone()
                }
            }
            Some((account, _)) => account.to_string(),
            // A public channel's name is held by the exchange in the clear --
            // that is what the directory searches -- so it is known before a
            // single entry is read. A group's is a sealed entry and is not, so
            // until the log is read it goes by its identifier.
            None if public && !given_name.is_empty() => given_name,
            None => remembered
                .map(|k| k.2.clone())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| format!("group {}", hex8(&m.channel))),
        };

        let group = peer.is_none();
        let _ = chat.store().put_channel(&m.channel, group, &label, &admins);

        let entry = desk.channels.entry(m.channel).or_insert_with(|| {
            // Folded from the store, so history is on screen before the
            // exchange has answered anything -- and stays there when it never
            // does. The local copy is the only one that can be read anyway:
            // opening an epoch key spends the prekey it was sealed against.
            let timeline = chat.history(&m.channel, &admins).unwrap_or_default();
            let last_at = timeline.messages().last().map(|m| m.posted).unwrap_or(0);
            let seen = timeline.messages().count();
            Known {
                peer: None,
                public,
                group,
                label: String::new(),
                admins: Vec::new(),
                members: Vec::new(),
                timeline,
                seen,
                last_at,
                unread: 0,
                waiting: false,
                typing: false,
                trouble: Trouble::default(),
            }
        });
        entry.peer = peer.map(|(a, _)| a);
        entry.public = public;
        entry.group = group;
        entry.label = label;
        entry.admins = admins;
        if !roster.is_empty() {
            entry.members = roster;
        }
        if entry.last_at == 0 {
            entry.last_at = m.joined;
        }
    }

    // A contact we have never exchanged anything with is not a membership yet,
    // so it will not come back from `mine` -- but somebody who added them
    // expects a row they can write into.
    for c in &contacts {
        let channel = chat.dm_with(&c.account);
        present.insert(channel);
        desk.channels.entry(channel).or_insert_with(|| Known {
            peer: Some(c.account),
            public: false,
            group: false,
            label: if c.label.is_empty() {
                c.account.to_string()
            } else {
                c.label.clone()
            },
            admins: vec![me, c.account],
            members: Vec::new(),
            timeline: Timeline::default(),
            seen: 0,
            unread: 0,
            // Nothing has happened here yet, so it sorts below anything that
            // has rather than claiming a time it does not have.
            last_at: 0,
            waiting: false,
            typing: false,
            trouble: Trouble::default(),
        });
    }

    // Left, removed, or closed. Dropped from the list rather than left on it
    // as a conversation nothing can be sent to.
    desk.channels.retain(|c, _| present.contains(c));
    desk.dirty.retain(|c| present.contains(c));
    Ok(())
}

/// Fetch what has changed and publish the result.
async fn refresh(chat: &mut Chat, state: &watch::Sender<ChatState>, desk: &mut Desk, me: PubKey) {
    // The open conversation is always fetched: it is the one somebody is
    // looking at, and a signal there (typing) has no event of its own until it
    // is delivered.
    let mut to_poll: Vec<[u8; 32]> = desk.dirty.drain().collect();
    if let Some(open) = desk.open
        && !to_poll.contains(&open)
    {
        to_poll.push(open);
    }

    for channel in to_poll {
        let Some(known) = desk.channels.get_mut(&channel) else {
            continue;
        };
        let mut timeline = std::mem::take(&mut known.timeline);
        let polled = chat.poll(&channel, &mut timeline, 0).await;
        let Some(known) = desk.channels.get_mut(&channel) else {
            continue;
        };
        match polled {
            Ok(conversation) => {
                known.timeline = conversation.timeline;
                known.typing = conversation.typing;
                known.waiting = false;
                known.trouble = Trouble {
                    unreadable: conversation.unreadable.len(),
                    gap: conversation.gap,
                    restarted: conversation.restarted,
                    no_key: conversation.no_key,
                    lost: conversation.lost,
                };
                if !conversation.admins.is_empty() {
                    known.admins = conversation.admins;
                }
                let after = known.timeline.messages().count();
                // Counted against what we had rather than against a read mark,
                // so a message arriving in a conversation nobody is looking at
                // is counted once, when it arrives.
                if after > known.seen && desk.open != Some(channel) {
                    known.unread += after - known.seen;
                }
                known.seen = after;
                if let Some(newest) = known.timeline.messages().last().map(|m| m.posted) {
                    known.last_at = known.last_at.max(newest);
                }
                // A group's name lives in a sealed entry, so it is only known
                // once the log has been read -- and it changes when an admin
                // renames it.
                let named = known.timeline.name.clone();
                if known.peer.is_none() && !named.is_empty() && named != known.label {
                    known.label = named.clone();
                    let _ = chat.store().set_label(&channel, &named);
                }
            }
            Err(_) => {
                known.timeline = timeline;
            }
        }
    }

    // Reading it is what clears it, and advancing the exchange's mark is what
    // makes "where was I" survive closing the client.
    if let Some(open) = desk.open
        && let Some(known) = desk.channels.get_mut(&open)
    {
        known.unread = 0;
        if let Some(last) = known.timeline.messages().last().map(|m| m.seq) {
            let _ = chat.mark_read(&open, last).await;
        }
    }

    publish(chat, state, desk, me);
}

/// Learn what everybody on screen is called.
///
/// Two fetches with different meanings, and they must not be merged:
/// `refresh_profiles` fills in people we cannot name yet and is cheap because
/// it honours the cache; `refetch_profiles` ignores the cache and is the only
/// correct answer to a SIP-30 `Profile` event, since a name that has moved on
/// looks exactly like one that has not.
async fn learn_names(chat: &mut Chat, desk: &mut Desk) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let stale: Vec<PubKey> = desk.restale.drain().collect();
    if !stale.is_empty() {
        let _ = chat.refetch_profiles(&stale, now).await;
    }

    // Everybody a view could name: the other party in each direct message, and
    // whoever has said anything in the conversation on screen. Not every member
    // of every channel -- that is a request per person per rebuild for names
    // nobody is looking at.
    let mut want: Vec<PubKey> = Vec::new();
    let mut add = |a: PubKey| {
        if a != chat.me && !want.contains(&a) {
            want.push(a);
        }
    };
    for known in desk.channels.values() {
        if let Some(peer) = known.peer {
            add(peer);
        }
    }
    if let Some(open) = desk.open
        && let Some(known) = desk.channels.get(&open)
    {
        for m in known.timeline.messages() {
            add(m.account);
        }
    }
    if !want.is_empty() {
        let _ = chat.refresh_profiles(&want, now).await;
    }
}

/// Everything we can say about who somebody is, read back out of the store.
fn people_of(chat: &Chat, desk: &Desk) -> HashMap<PubKey, Person> {
    let mut out = HashMap::new();
    let look = |account: PubKey, out: &mut HashMap<PubKey, Person>| {
        out.entry(account).or_insert_with(|| Person {
            name: chat.display_name(&account),
            title: chat.title_of(&account),
            handle: chat.handle(&account),
        });
    };
    for known in desk.channels.values() {
        if let Some(peer) = known.peer {
            look(peer, &mut out);
        }
    }
    if let Some(open) = desk.open
        && let Some(known) = desk.channels.get(&open)
    {
        let authors: Vec<PubKey> = known.timeline.messages().map(|m| m.account).collect();
        for a in authors {
            look(a, &mut out);
        }
    }
    out
}

/// Build what the interface draws from what the task holds.
fn publish(chat: &Chat, state: &watch::Sender<ChatState>, desk: &Desk, me: PubKey) {
    let people = people_of(chat, desk);
    let mine = Person {
        name: chat.display_name(&me),
        title: chat.title_of(&me),
        handle: chat.handle(&me),
    };
    // Most recent first, the way every chat client orders a conversation list.
    // `mine()` hands them back in join order, which says nothing about where
    // anything is happening.
    //
    // Selection is by channel and not by position, so reordering under
    // somebody's cursor moves the row and not the reader.
    let mut summaries: Vec<Summary> = desk
        .channels
        .iter()
        .map(|(c, k)| {
            let mut summary = k.summary(*c, &me);
            // A direct message's row names a *person*, so a published name
            // wins over the local label -- which is only what we happened to
            // call them, and is often just their key repeated back.
            if let Some(peer) = k.peer
                && let Some(named) = people.get(&peer).and_then(|p| p.name.clone())
            {
                summary.label = named;
            }
            summary
        })
        .collect();
    summaries.sort_by(|a, b| {
        b.at.unwrap_or(0)
            .cmp(&a.at.unwrap_or(0))
            // A stable tie-break, or two conversations with the same time would
            // swap places on every redraw.
            .then_with(|| a.channel.cmp(&b.channel))
    });

    let open = desk
        .open
        .and_then(|c| desk.channels.get(&c).map(|k| (c, k)));
    let lines = open
        .map(|(_, k)| {
            k.timeline
                .messages()
                .map(|m| Line {
                    seq: m.seq,
                    who: m.account,
                    name: people.get(&m.account).and_then(|p| p.name.clone()),
                    mine: m.account == me,
                    at: m.posted,
                    text: m.post.body_text().unwrap_or_default().to_string(),
                    redacted: m.redacted,
                    edited: m.edited.is_some(),
                })
                .collect()
        })
        .unwrap_or_default();

    let typing = open.map(|(_, k)| k.typing).unwrap_or(false);
    let trouble = open.map(|(_, k)| k.trouble.clone()).unwrap_or_default();
    let members = open.map(|(_, k)| k.members.clone()).unwrap_or_default();
    // What the **exchange** attests, not what anybody says about themselves.
    // This is the one place a role may be drawn as a role.
    let i_am_admin = members.iter().any(|m| m.account == me && m.admin);
    let topic = open
        .map(|(_, k)| k.timeline.topic.clone())
        .unwrap_or_default();
    let link = LinkState::from(chat.link());

    state.send_modify(|s| {
        s.link = link;
        s.conversations = summaries;
        s.lines = lines;
        s.typing = typing;
        s.trouble_with = trouble;
        s.people = people;
        s.mine = mine;
        s.members = members;
        s.i_am_admin = i_am_admin;
        s.topic = topic;
    });
}

/// The first eight hex characters of an identifier, for a channel with no name.
fn hex8(id: &[u8; 32]) -> String {
    id.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

async fn apply(chat: &mut Chat, cmd: Cmd, state: &watch::Sender<ChatState>, desk: &mut Desk) {
    match cmd {
        Cmd::OpenDm(peer) => match chat.open_dm(&peer).await {
            Ok(channel) => {
                // Opening a conversation and minting its key are separate, and
                // have to be: a direct message can be opened with somebody who
                // has never run a client, but SIP-23 forbids sealing a key to a
                // device with no prekeys. That is a conversation waiting to
                // start, not a failure to open one.
                let waiting = chat.ensure_epoch(&channel).await.is_err();
                // A brand new conversation is not in the list until the next
                // rebuild, so it is put there now rather than opening onto
                // nothing.
                desk.restructure = true;
                desk.channels.entry(channel).or_insert_with(|| Known {
                    peer: Some(peer),
                    public: false,
                    group: false,
                    label: peer.to_string(),
                    admins: vec![chat.me, peer],
                    members: Vec::new(),
                    timeline: Timeline::default(),
                    seen: 0,
                    last_at: 0,
                    unread: 0,
                    waiting,
                    typing: false,
                    trouble: Trouble::default(),
                });
                if let Some(k) = desk.channels.get_mut(&channel) {
                    k.waiting = waiting;
                }
                open(desk, state, channel);
            }
            Err(e) => state.send_modify(|s| s.trouble = Some(e.to_string())),
        },
        Cmd::Show(channel) => open(desk, state, channel),
        Cmd::Close => {
            desk.open = None;
            state.send_modify(|s| {
                s.open = None;
                s.lines.clear();
                s.divider = None;
                s.unread_on_open = 0;
            });
        }
        Cmd::Send(text) => {
            let Some(channel) = desk.open else { return };
            if let Err(e) = chat.send(&channel, &text).await {
                // The text is not thrown away here; the interface keeps it in
                // the composer, because retyping a message the program lost is
                // the worst thing a chat client can do to somebody.
                state.send_modify(|s| s.trouble = Some(e.to_string()));
            } else {
                desk.dirty.insert(channel);
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
            desk.restructure = true;
        }
        Cmd::SetProfile { name, title } => {
            let profile = sqex_proto::profile::Profile {
                name,
                title,
                ..Default::default()
            };
            match chat.set_profile(profile).await {
                // Read it straight back rather than assuming: the exchange
                // holds the record and a published profile is what everybody
                // else will see, not what we asked for.
                Ok(()) => desk.restale.insert(chat.me),
                Err(e) => {
                    state.send_modify(|s| s.trouble = Some(e.to_string()));
                    false
                }
            };
        }
        Cmd::Reconnect => chat.reconnect_now(),

        Cmd::NewGroup(name) => match chat.create_group(&name, &[]).await {
            Ok(channel) => {
                desk.restructure = true;
                open(desk, state, channel);
                note(state, format!("Created {name}. Invite somebody to it."));
            }
            Err(e) => trouble(state, e),
        },
        Cmd::NewPublic { name, topic } => match chat.create_public(&name, &topic).await {
            Ok(channel) => {
                desk.restructure = true;
                open(desk, state, channel);
                note(
                    state,
                    format!(
                        "Created {name}. Anybody may find and join it, and nothing in it is encrypted."
                    ),
                );
            }
            Err(e) => trouble(state, e),
        },
        Cmd::Find(query) => match chat.find(&query, 0).await {
            Ok(listing) => {
                let found = listing
                    .channels
                    .into_iter()
                    .map(|c| Found {
                        channel: c.channel,
                        instance: c.instance,
                        name: c.name,
                        topic: c.topic,
                        members: c.members,
                    })
                    .collect();
                state.send_modify(|s| {
                    s.found = found;
                    s.searched = true;
                });
            }
            Err(e) => trouble(state, e),
        },
        Cmd::Join { channel, instance } => match chat.join(&channel, instance).await {
            Ok(()) => {
                desk.restructure = true;
                open(desk, state, channel);
            }
            Err(e) => trouble(state, e),
        },

        Cmd::Invite(who) => {
            let Some(channel) = desk.open else { return };
            match chat.invite(&channel, &who).await {
                Ok(()) => {
                    desk.dirty.insert(channel);
                    // SIP-17: a device with no prekeys cannot be sealed to, and
                    // one holding no envelope fetches every entry and opens
                    // none -- which is indistinguishable from not reading. So
                    // it is said here rather than left to be discovered.
                    match chat.stranded(&channel).await {
                        Ok(absent) if !absent.devices.is_empty() => note(
                            state,
                            format!(
                                "Invited. {} of their devices still hold no key for this \
                                 conversation and cannot read it yet.",
                                absent.devices.len()
                            ),
                        ),
                        _ => note(state, "Invited.".into()),
                    }
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Kick(who) => {
            let Some(channel) = desk.open else { return };
            match chat.remove(&channel, &who).await {
                Ok(()) => {
                    desk.dirty.insert(channel);
                    note(
                        state,
                        "Removed, and the key rotated: what follows is not theirs.".into(),
                    );
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Grant { who, admin } => {
            let Some(channel) = desk.open else { return };
            let role = if admin { Role::Admin } else { Role::Member };
            match chat.grant(&channel, &who, role).await {
                Ok(()) => desk.dirty.insert(channel),
                Err(e) => {
                    trouble(state, e);
                    false
                }
            };
        }
        Cmd::Rotate => {
            let Some(channel) = desk.open else { return };
            match chat.rotate(&channel).await {
                Ok(epoch) => note(state, format!("New key minted (epoch {epoch}).")),
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Leave => {
            let Some(channel) = desk.open else { return };
            match chat.leave(&channel).await {
                Ok(()) => {
                    desk.channels.remove(&channel);
                    desk.restructure = true;
                    close(desk, state);
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Destroy => {
            let Some(channel) = desk.open else { return };
            match chat.close(&channel).await {
                Ok(()) => {
                    // Forgotten locally only after the exchange has confirmed
                    // it: dropping our copy first would leave somebody with no
                    // conversation and no channel either, if the call failed.
                    let _ = chat.store().forget_channel(&channel);
                    desk.channels.remove(&channel);
                    desk.restructure = true;
                    close(desk, state);
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::SetName(name) => {
            let Some(channel) = desk.open else { return };
            match chat.set_name(&channel, &name).await {
                Ok(_) => desk.dirty.insert(channel),
                Err(e) => {
                    trouble(state, e);
                    false
                }
            };
        }
        Cmd::SetTopic(topic) => {
            let Some(channel) = desk.open else { return };
            match chat.set_topic(&channel, &topic).await {
                Ok(_) => desk.dirty.insert(channel),
                Err(e) => {
                    trouble(state, e);
                    false
                }
            };
        }
        Cmd::SetRetention { secs, max_entries } => {
            let Some(channel) = desk.open else { return };
            match chat.set_retention(&channel, secs, max_entries).await {
                Ok(()) => {
                    desk.dirty.insert(channel);
                    note(
                        state,
                        "Retention set. Anything already outside the window is gone.".into(),
                    );
                }
                Err(e) => trouble(state, e),
            }
        }
    }
}

/// A confirmation of something that was just done.
///
/// **Separate from `trouble`, and deliberately.** A note is about an action and
/// a trouble is about a state, and the state is rebuilt by every refresh — so
/// keeping the two in one field puts every confirmation on screen for less than
/// a tick, which is to say it is never read.
fn note(state: &watch::Sender<ChatState>, said: String) {
    state.send_modify(|s| s.note = Some(said));
}

fn trouble(state: &watch::Sender<ChatState>, e: impl std::fmt::Display) {
    state.send_modify(|s| s.trouble = Some(e.to_string()));
}

/// Put the open conversation away, without touching it at the exchange.
fn close(desk: &mut Desk, state: &watch::Sender<ChatState>) {
    desk.open = None;
    state.send_modify(|s| {
        s.open = None;
        s.lines.clear();
        s.divider = None;
        s.unread_on_open = 0;
    });
}

/// Put a conversation on screen, taking the unread divider as it goes.
fn open(desk: &mut Desk, state: &watch::Sender<ChatState>, channel: [u8; 32]) {
    desk.open = Some(channel);
    desk.dirty.insert(channel);
    // The divider is taken **here**, on entry, and then left alone. Reading
    // advances the read mark, so one recomputed each refresh would disappear
    // the moment somebody looked at the thing it was marking.
    let known = desk.channels.get(&channel);
    let unread = known.map(|k| k.unread).unwrap_or(0);
    let divider = (unread > 0)
        .then(|| {
            known.and_then(|k| {
                let messages: Vec<u64> = k.timeline.messages().map(|m| m.seq).collect();
                messages
                    .len()
                    .checked_sub(unread)
                    .and_then(|i| messages.get(i).copied())
            })
        })
        .flatten();
    state.send_modify(|s| {
        s.open = Some(channel);
        s.lines.clear();
        s.unread_on_open = unread;
        s.divider = divider;
    });
}
