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
use sqex_proto::channel::{
    EVENT_ADDED, EVENT_CREATED, EVENT_DEMOTED, EVENT_JOINED, EVENT_LEFT, EVENT_PROMOTED,
    EVENT_REMOVED, EVENT_RENAMED, EVENT_REPLICATE, EVENT_RETENTION, EVENT_ROTATED,
    EVENT_UNREPLICATE, Role, Visibility,
};
use sqex_proto::events::Event;
use sqex_proto::message::{
    CALL_ANSWERED, CALL_CANCELLED, CALL_DECLINED, CALL_FAILED, CALL_MISSED, MEDIA_AUDIO,
    RING_ACCEPTED, RING_DECLINED, RING_ENDED, RING_RINGING,
};
use sqex_proto::timeline::{Timeline, Verdict};
use sqnr_core::{PubKey, SoftwareSigner};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use sigil_net::Dial;

/// How long a call rings before a reader derives that it was missed.
///
/// The same 45 seconds the terminal client uses, so the two agree about when a
/// call stopped ringing. This is not only how long a phone rings: with no
/// `CallEnd` entry, **every** reader derives `CALL_MISSED` from it, so two
/// clients disagreeing here would disagree about what happened.
pub const RING_SECS: u16 = 45;

/// The same, as a duration, for whatever has to outlast a ring.
///
/// A call still connecting when this has passed is a call nobody answered:
/// every reader derives `CALL_MISSED` at this point, so waiting longer means
/// holding a microphone open for a call the protocol has already given up on.
pub const RING_WINDOW: std::time::Duration = std::time::Duration::from_secs(RING_SECS as u64);

/// How often the client is driven.
///
/// The same cadence `sqex-chat` uses. It is not a poll of the exchange —
/// SIP-30 pushes what changed — but the dial, the subscription and the event
/// queue all need a turn of the handle, and 700 ms is short enough that typing
/// never waits behind it.
const TICK_MS: u64 = 700;

/// How long the loop waits when there is nothing outstanding.
///
/// **The timer is a backstop now, not the clock.** SIP-30's events say what
/// moved and the stream knocks the moment one arrives, so this is only what
/// catches what the stream never mentioned: a subscription that died quietly, a
/// reconnect, the periodic rebuild of the list. Measured with two clients idle
/// in one conversation, at 700ms it was still the largest source of traffic
/// either of them made.
///
/// Anything the loop is in the middle of -- a link that is down and being
/// redialled, a note that has to disappear on time, channels an event named,
/// pictures still to fetch -- puts it back to [`TICK_MS`] until that is done.
const QUIET_MS: u64 = 5_000;

/// How many messages a conversation opens on, and how many more each time
/// somebody asks for earlier ones.
///
/// # Why the transcript is paged at all
///
/// Opening a channel built a [`Line`] for **every message it had ever
/// carried** — its text, its stub, its reactions, its attachments — and did it
/// again on every poll, and the interface then cloned the whole vector twice
/// per frame. On a small conversation that is free. On a public channel a few
/// thousand messages deep it is seconds of work to show a screenful, repeated
/// forever, and it was the first thing anybody noticed about a busy room.
///
/// Nothing is dropped: the store still holds every entry and the fold still
/// folds all of them, so search, replies and receipts are unchanged. This is
/// only how much of it gets turned into something drawable at once.
pub const PAGE: usize = 50;

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
    /// Emoji, how many sent it, and whether we are one of them.
    pub reactions: Vec<(String, usize, bool)>,
    /// What this replies to: who said it and a stub of what they said.
    ///
    /// The author and the words, not the sequence number — "↳ 57" names a
    /// number nobody has memorised.
    pub reply_to: Option<(String, String)>,
    /// How far one of ours is known to have got. `None` on anybody else's.
    pub receipt: Option<Receipt>,
    /// Files this message carries.
    pub attachments: Vec<Attached>,
    /// What SIP-31 concluded about it. SIP-31 **requires** a fork be
    /// surfaced, and a message with nothing wrong says nothing.
    pub standing: Standing,
}

/// Something that happened *to* the conversation rather than in it.
///
/// The exchange writes and signs an entry for every membership and metadata
/// change — SIP-16 defines twelve — and they belong in the transcript in the
/// order everybody sees them: whether somebody was in the room when a thing
/// was said is not a detail.
///
/// Plain data, like [`Line`]: the words are built here, where names resolve,
/// so the widget draws a string and understands no wire format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Happened {
    pub seq: u64,
    pub at: u64,
    /// What happened, in words, with people already named.
    pub said: String,
    /// The keys the words name, so a name is never the only thing on offer.
    pub actor: PubKey,
    pub subject: PubKey,
    /// Anything a reader would otherwise have to know. `None` for the events
    /// that mean exactly what they say.
    pub caveat: Option<&'static str>,
}

/// What SIP-31 verification concluded about one message.
///
/// # Why three of these and not a warning triangle
///
/// SIP-31 requires a fork be surfaced, and it is the **only** one of these
/// that is evidence: two entries by one device at one chain position cannot
/// happen without that device signing twice or somebody replaying. A gap is
/// ordinary — pruning, a retention window, and joining a channel without its
/// history all produce one — so a client that drew the two alike would cry
/// wolf on every channel that keeps anything for a fixed time, and the cry
/// that matters would be lost in it.
///
/// *Unattributed* is a third thing again: the signature verifies and nobody
/// can say whose key it is, because no SIP-20 credential could be obtained to
/// bind the signing device to the account the entry names. A mapping somebody
/// asserts and evidence somebody can check are different things.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Standing {
    /// Signed by the device it names, following that device's chain.
    #[default]
    Sound,
    /// A gap in the chain. Ordinary, and reported rather than hidden.
    Gap,
    /// Two entries at one chain position. Evidence.
    Fork,
    /// It was signed, and whose key signed it is unknown.
    Unattributed,
}

impl Standing {
    /// What to say about it, or nothing when there is nothing to say.
    pub fn word(self) -> Option<&'static str> {
        match self {
            Standing::Sound => None,
            Standing::Gap => Some("gap"),
            Standing::Fork => Some("forked"),
            Standing::Unattributed => Some("unattributed"),
        }
    }

    /// The long form: what it means, and whether to be alarmed.
    pub fn means(self) -> Option<&'static str> {
        match self {
            Standing::Sound => None,
            Standing::Gap => Some(
                "The chain skips here. Ordinary — messages expire, and joining a channel \
                 does not bring its history. It is not evidence of anything.",
            ),
            Standing::Fork => Some(
                "Two messages signed at the same position in one device's chain. This \
                 cannot happen without that device signing twice or somebody replaying, \
                 and it is the one thing here that is evidence.",
            ),
            Standing::Unattributed => Some(
                "The signature is good and nothing proves whose key it is: no credential \
                 could be got binding the signing device to the account named.",
            ),
        }
    }
}

/// How long a confirmation stays on screen.
///
/// Five seconds: long enough to read a sentence, short enough to be gone
/// before it becomes furniture. **Every note, not one of them.** A
/// confirmation that stays until something else happens to replace it is a
/// claim about the present that stopped being true minutes ago.
pub const NOTE_SECS: u64 = 5;

/// A confirmation of something just done, and when it was said.
///
/// The time is carried with the words rather than beside them, because the two
/// have to move together: a note whose clock was set somewhere else is a note
/// that expires at the wrong moment or never.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub said: String,
    /// Unix seconds. See [`NOTE_SECS`].
    pub at: u64,
}

/// How far a message is known to have got.
///
/// **Under-claiming is the only safe direction.** `Read` means *everybody* in
/// the conversation is known to have read it, so in a group it waits for the
/// last of them. An account that opted out of receipts reports no reading at
/// all — the exchange withholds their reading, not their existence — and must
/// never be counted as having read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receipt {
    /// The exchange has it.
    Sent,
    /// Everybody has fetched it.
    Delivered,
    /// Everybody has read it.
    Read,
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
    ///
    /// `None` until the exchange has said. A conversation restored from this
    /// machine's own copy knows it is a group, because that is in the store;
    /// whether it is *public* is not, and neither guess may be made on
    /// somebody's behalf. Drawn as neither until the answer arrives.
    pub public: Option<bool>,
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
    /// The exchange this session actually reached.
    ///
    /// Reported rather than configured: a session is started against a *name*
    /// — a domain, or nothing at all for the default — and what that resolves
    /// to is not known until it is dialled. It is the key a receipt verifies
    /// under, so an interface shows it rather than the name it was asked for.
    pub exchange: Option<PubKey>,
    /// The domain that key was discovered at, when it was discovered at one.
    ///
    /// **What the default exchange is called.** The default has no name in the
    /// roster — it is whatever this identity's own SIP-38 handle and
    /// `~/.sqnr/config` resolve to — so the switcher labelled it with a
    /// truncated public key, which is unreadable and says nothing about where
    /// it is. This is the same value `Chat::handle` composes `name@domain`
    /// from, so the two cannot disagree. `None` when the connection was made
    /// to an address, which has no domain to report.
    pub domain: Option<String>,
    /// The exchange this session could not take the store lock for.
    ///
    /// Two interactive clients on one account at one exchange would disagree
    /// about the next SIP-17 counter, so the second is refused. **Usually that
    /// second client is sigil itself**: an identity whose default exchange
    /// resolves to the same place as one of its added ones has two sessions
    /// for one pair, and the store cannot tell them apart. Reported as the key
    /// rather than as a sentence so the interface can look at its own other
    /// sessions and say which of the two it is.
    pub locked_out: Option<PubKey>,
    /// Up, retrying, or gone. Drawn with the *word* beside the colour: a
    /// colour on its own is not a message.
    pub link: LinkState,
    pub trouble: Option<String>,
    pub conversations: Vec<Summary>,
    /// Which conversation is on screen, and what is in it.
    pub open: Option<[u8; 32]>,
    pub lines: Vec<Line>,
    /// What happened to the conversation, in the same sequence space as
    /// `lines` so the two interleave.
    pub events: Vec<Happened>,
    /// Whether the exchange has answered about the conversation on screen
    /// yet, this session.
    ///
    /// What is drawn before that is this machine's own copy, which is the only
    /// copy that can ever be read anyway -- opening an epoch key spends the
    /// prekey it was sealed against. It is worth drawing at once, and it is
    /// not the whole story, so a conversation with nothing in it yet says it
    /// is still asking rather than that there is nothing here.
    pub loading: bool,
    /// Whether the conversation list has been fetched at least once.
    ///
    /// An empty list means two entirely different things -- "you have no
    /// conversations" and "we have not asked yet" -- and the first one was
    /// being said during the second, on every launch.
    pub synced: bool,
    /// How many messages there are before the first one in `lines`.
    ///
    /// Zero means the transcript is whole. Anything else is what the reader
    /// gets offered when they reach the top; see [`PAGE`].
    pub earlier: usize,
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
    pub note: Option<Note>,
    /// Who is in the open conversation.
    pub members: Vec<Member>,
    /// Whether we may rename, invite, remove and rotate here.
    pub i_am_admin: bool,
    /// The open conversation's topic, when it has one.
    pub topic: String,
    /// Every device registered to this account.
    ///
    /// **First class, not buried.** An epoch key arrives sealed against a
    /// one-time prekey and opening it spends the prekey, so the copy on this
    /// disk is the only one that will ever exist — and a linked device is the
    /// only backup of it there can be. Losing this store with no second device
    /// loses those conversations permanently, for everybody in them.
    pub devices: Vec<Linked>,
    /// Whether this client still acts for its account.
    ///
    /// `None` when it was never linked: an account with no registered device
    /// *is* its own device and there is nothing to check. `Some(false)` means
    /// revoked — otherwise learned only by being refused as a stranger to
    /// every conversation it can see.
    pub linked: Option<bool>,
    /// A credential just written, for another device to register with.
    pub credential: Option<String>,
    /// Who we are blocking.
    pub blocked: Vec<PubKey>,
    /// What a local search turned up.
    pub hits: Vec<Hit>,
    /// A message search has been run, so an empty `hits` means "nothing
    /// matched" rather than "nobody has looked".
    pub searched_messages: bool,
    /// Calls ringing right now, in **any** conversation.
    ///
    /// Not only the one on screen: a call is the thing that most needs to
    /// reach somebody who is looking elsewhere.
    pub ringing: Vec<Ring>,
    /// The first message that was unread when this conversation was opened.
    ///
    /// **Frozen on entry.** Reading advances the read mark, so a divider that
    /// tracked it would disappear exactly when somebody wanted to see where
    /// they had got to.
    pub divider: Option<u64>,
    /// How many there were, for the divider's label. Frozen with it.
    pub unread_on_open: usize,
}

/// One message a local search turned up.
///
/// **Local only.** The exchange holds ciphertext and could not search it if it
/// wanted to, so this covers what this client has fetched and opened and
/// nothing else — which is a real limit and not a bug, and the interface says
/// so rather than presenting an empty result as "nothing was ever said".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub channel: [u8; 32],
    pub seq: u64,
    /// What the conversation it was found in is called.
    pub label: String,
    pub text: String,
    pub at: u64,
}

/// One device registered to this account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    pub device: PubKey,
    pub added: u64,
    /// When its credential expires. A registration expires with it, and there
    /// is deliberately no second lifetime.
    pub not_after: u64,
    /// This one — the client you are looking at.
    pub is_this_one: bool,
}

/// A file carried by a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    /// Image, video, voice note, or file. **Taken from the kind, never from
    /// the mime type**, which is the sender's claim and nothing more: SIP-18
    /// says a receiver must not dispatch on it beyond choosing how to display,
    /// must never execute a blob, and must never hand one to a handler chosen
    /// by that string.
    pub kind: u8,
    /// What it is, in words: `[image 1920x1080, 2.1 MB]`.
    pub described: String,
    pub size: u64,
    /// The thumbnail the sender put in, if any. Drawn while the blob is
    /// fetched, and the only thing shown at all until it is.
    ///
    /// **Shared, not owned.** A picture used to be copied into every published
    /// state and again into every clone of one -- and the interface clones the
    /// whole state four or five times a frame. A two-megabyte photograph on
    /// screen was ten megabytes of memcpy per frame before anything was drawn.
    pub preview: std::sync::Arc<[u8]>,
    /// The exchange was asked for it and would not give it.
    ///
    /// **Not the same as "not yet".** A blob past its retention window is gone
    /// and asking again on every tick will not bring it back, so the fetch is
    /// not retried — which left a picture that had failed and one that had not
    /// been reached yet looking identical, and neither of them saying anything.
    pub missing: bool,
    /// The whole file, once it has been fetched and opened. Shared for the
    /// reason `preview` is.
    ///
    /// Held here rather than fetched by the view: a view runs sixty times a
    /// second and must never be where a download starts.
    pub bytes: Option<std::sync::Arc<[u8]>>,
    /// A name for the blob, stable across passes, so the interface can key a
    /// texture on it.
    pub id: String,
}

/// A call, as the interface needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ring {
    pub channel: [u8; 32],
    /// The `seq` of the invitation. Everything about a call is keyed on it.
    pub seq: u64,
    /// Who rang.
    pub from: PubKey,
    /// Ours, so it is drawn as calling rather than as ringing.
    pub mine: bool,
    /// The SIP-13 room secret carried by the invitation. **This is the whole
    /// of what joining needs**, which is also why the invitation is a bearer
    /// capability: anybody who can read the entry can join the call.
    pub secret: [u8; 32],
    /// Somebody has said they are taking it.
    ///
    /// **A second notion of what a call is doing, deliberately.** The log is
    /// the only authority on what *happened* — SIP-36 forbids deriving that
    /// from a signal — but it structurally cannot say what is *happening*,
    /// because answering posts no entry at all. Without somewhere to put this,
    /// a client shows an answered call as still ringing, then derives
    /// **missed** of a call two people are talking on.
    pub answered: bool,
    /// What the conversation this rang in is called.
    pub label: String,
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
    /// Entries whose signature did not verify.
    ///
    /// **Never shown as messages** — the whole point is that nobody vouched
    /// for them — but counted and said, because something arrived claiming to
    /// be from somebody in this conversation and was not.
    pub forged: usize,
}

impl Trouble {
    pub fn is_clear(&self) -> bool {
        *self == Trouble::default()
    }
}

/// [`Link`] without a dependency on the chat crate, and `Default`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LinkState {
    /// Dialling, and **the default**.
    ///
    /// It used to default to `Up`, which meant a session that had not
    /// connected yet — and a `ChatState::default()` standing in for a session
    /// that does not exist at all — showed a green light and the word
    /// *connected*. A fallback that looks like a real value destroys the
    /// distinction it was standing in for; here it claimed the one thing the
    /// indicator exists to report.
    #[default]
    Connecting,
    Up,
    Retrying,
    /// Down through the whole backoff ramp. Still trying.
    Gone,
}

impl LinkState {
    pub fn word(self) -> &'static str {
        match self {
            LinkState::Connecting => "connecting…",
            LinkState::Up => "connected",
            LinkState::Retrying => "reconnecting…",
            LinkState::Gone => "offline",
        }
    }
}

#[cfg(test)]
mod link_tests {
    use super::*;

    /// A state nobody has filled in does not claim to be connected.
    ///
    /// `ChatState::default()` stands in for a session that has not connected
    /// yet, and used to stand in for one that does not exist at all. With `Up`
    /// as the default both drew a green light and the word *connected* — the
    /// one thing that indicator exists to report, asserted by a value that
    /// means "nothing has been reported".
    #[test]
    fn an_unfilled_state_does_not_claim_the_link_is_up() {
        assert_ne!(LinkState::default(), LinkState::Up);
        assert_eq!(ChatState::default().link, LinkState::Connecting);
        assert_eq!(LinkState::default().word(), "connecting…");
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
    /// Ask again for every file the exchange refused.
    Refetch,
    /// Build another [`PAGE`] of the open conversation's history.
    ///
    /// Asked for by the transcript when somebody reaches the top of it. The
    /// entries are already held; this is only how many of them get turned into
    /// something drawable.
    Earlier,
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
    /// Claim a SIP-38 name at this exchange.
    ///
    /// Not the same thing as [`Cmd::SetProfile`] and easily confused with it.
    /// A profile name is what somebody says about themselves and nobody
    /// attests; a SIP-38 name is bound at the exchange, resolves to exactly
    /// one account, and is what makes `name@domain` work.
    ClaimName(String),
    /// Give up a SIP-38 name this account holds.
    ReleaseName(String),
    /// Present a credential another device of this account wrote, and become
    /// one of its devices (SIP-20/22).
    ///
    /// The **new** device runs this, on its own connection: the credential
    /// names the delegate, and the exchange checks that against who is asking
    /// — so one somebody found is one they cannot use.
    RegisterSelf(String),

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

    // ---- doing things to a message -------------------------------------
    /// Add or take back an emoji. Keyed on `(account, target, emoji)` by the
    /// fold, so sending one twice is a no-op and taking back one that was
    /// never sent is ordinary — which is what lets this be sent without first
    /// knowing what we already sent.
    React {
        target: u64,
        emoji: String,
    },
    /// A message that names another.
    Reply {
        target: u64,
        text: String,
    },
    /// Rewrite one of ours. Enforced at the **reader**: only from the account
    /// that posted it and only inside the edit window. The client checks too,
    /// so it can say an edit will be ignored rather than send one that
    /// silently is.
    Edit {
        target: u64,
        text: String,
    },
    /// Remove a message's body. The entry stays, and the gap is the record.
    Redact(u64),
    /// Say we are typing, or have stopped. Best effort, ephemeral and
    /// forgeable, like every signal.
    Typing(bool),

    // ---- calls (SIP-36) -------------------------------------------------
    /// Ring everybody in the open conversation.
    Call,
    /// Take a call. Signals that we have, so the caller stops seeing "ringing"
    /// — answering posts **no entry**, so a caller watching only the log would
    /// go on ringing and then derive *missed* of a call being spoken on.
    Answer {
        channel: [u8; 32],
        seq: u64,
    },
    /// Refuse one. Signals it *and* writes the durable record, because a
    /// signal is not an account of what happened.
    Decline {
        channel: [u8; 32],
        seq: u64,
    },
    /// End one that is up, or cancel one that never connected.
    Hangup {
        channel: [u8; 32],
        seq: u64,
        seconds: u32,
    },

    // ---- files (SIP-18) -------------------------------------------------
    /// Seal a file, upload it, and post a message carrying the reference.
    SendFile(std::path::PathBuf),
    /// Fetch a file and write it out.
    SaveFile {
        seq: u64,
        index: usize,
        to: std::path::PathBuf,
    },
    /// Set the open channel's picture, or clear it.
    SetChannelAvatar(Option<std::path::PathBuf>),

    // ---- devices (SIP-20/22) --------------------------------------------
    /// Re-read the device list.
    Devices,
    /// Write a credential for another device to register itself with.
    ///
    /// The credential names both keys in the clear to whoever holds it, and it
    /// is **evidence, not authority**: it says which account vouches for a
    /// key, and entitles that key to nothing on its own.
    LinkDevice {
        device: PubKey,
        days: u64,
    },
    /// Withdraw a device.
    ///
    /// **The revocation outlives the credential**, and has to: everything
    /// needed to register is on the stolen machine, so deleting the mapping
    /// alone would be undone by one request. A found device comes back only
    /// with a credential the account signed *after* the revocation — the one
    /// thing that was never on it.
    RevokeDevice(PubKey),
    /// Hand the open channel's key to our own other devices.
    ResealToSiblings,
    /// Ask an exchange that does not admit us to let us in (SIP-24).
    RequestAdmission(String),

    // ---- people, names, and the rest -------------------------------------
    /// Block somebody, or stop blocking them.
    ///
    /// **Refused invisibly.** The exchange answers on the blocker's behalf, so
    /// a blocked person is told nothing — but it is *inferable* from a
    /// delivery cursor that stops moving, and the interface must not claim
    /// otherwise.
    SetBlocked {
        who: PubKey,
        blocked: bool,
    },
    /// Re-read who we are blocking.
    Blocked,
    /// Open a conversation with somebody named `name@domain` (SIP-38).
    OpenByName(String),
    /// Attach an existing file to another conversation and post the reference.
    ///
    /// **By reference: the bytes stay where they are.** The exchange stores one
    /// copy however many conversations point at it, and the blob dies with its
    /// last attachment. Attaching before posting, because a message naming a
    /// blob the destination has no claim on is one its readers cannot fetch.
    Forward {
        seq: u64,
        index: usize,
        to: [u8; 32],
    },
    /// Search what this client holds. **Local only** — the exchange cannot
    /// read a sealed entry, so it could not search one if it wanted to.
    Search(String),
    /// SIP-35: let another exchange carry a copy of this channel, or stop it.
    Replicate {
        exchange: PubKey,
        on: bool,
    },
}

pub struct ChatHandle {
    state: watch::Receiver<ChatState>,
    cmds: mpsc::UnboundedSender<Cmd>,
    task: JoinHandle<()>,
    /// The connection this session holds, for a call or a console to use.
    /// See [`ChatHandle::connection`].
    ///
    /// Beside the state rather than in it: `ChatState` is cloned on every read
    /// and compared to decide whether the window needs repainting, and a
    /// connection is neither cloneable in that sense nor comparable in any.
    holds: sigil_net::Held,
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

    /// The connection this session holds, for a call or a console to use.
    ///
    /// A call used to dial its own, which cost a handshake at the moment
    /// somebody pressed the button and cost bandwidth for as long as it lasted:
    /// the exchange writes a relayed datagram to **every** connection an
    /// identity holds, so every audio frame was also written to this one, where
    /// nothing reads it.
    ///
    /// A slot rather than a connection — see [`sigil_net::Held`]. This session
    /// owns what is in it: it dials, holds and redials, and rewrites the slot
    /// as it does, so a borrower always reads the connection that exists rather
    /// than the one that did. Empty while the link is down.
    pub fn connection(&self) -> sigil_net::Held {
        self.holds.clone()
    }

    /// One question about the state, answered without copying the rest of it.
    ///
    /// # Why these exist
    ///
    /// `state()` clones everything: every line, its text, its reactions, its
    /// attachments. Three callers walk **every session** on **every pass** to
    /// read a single field each -- what is ringing, how much is unread, which
    /// exchange this one is on -- and each of those walks was a full clone per
    /// session per frame. Reading one field off the borrow costs nothing and
    /// says what it wants.
    pub fn ringing(&self) -> Vec<Ring> {
        self.state.borrow().ringing.clone()
    }

    /// How much is waiting here, across every conversation.
    pub fn unread(&self) -> usize {
        self.state
            .borrow()
            .conversations
            .iter()
            .map(|c| c.unread)
            .sum()
    }

    /// Which exchange this session is talking to, once it knows.
    pub fn exchange(&self) -> Option<PubKey> {
        self.state.borrow().exchange
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
    start_every(
        dial,
        signer,
        store_at,
        wake,
        std::time::Duration::from_millis(QUIET_MS),
    )
}

/// The same, with the backstop at a chosen interval.
///
/// **A timer, not a path.** Everything runs exactly as it does in the
/// application; only how often the loop gives up waiting and asks anyway
/// changes. A test that wants to know whether something arrived *because the
/// exchange said so* has to be able to tell that apart from arriving because
/// the timer came round -- and at 700ms the two are indistinguishable, which
/// is how the first version of that test passed with the knock taken out.
pub fn start_every(
    dial: impl Into<Dial>,
    signer: SoftwareSigner,
    store_at: Option<std::path::PathBuf>,
    wake: impl Fn() + Send + Sync + 'static,
    every: std::time::Duration,
) -> ChatHandle {
    let (state_tx, state_rx) = watch::channel(ChatState::default());
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let wake = Arc::new(wake);
    let dial = dial.into();
    let holds = sigil_net::Held::empty();
    let held = holds.clone();

    let task = tokio::spawn(async move {
        if let Err(e) = run(
            dial,
            signer,
            store_at,
            Wires {
                state: state_tx.clone(),
                cmds: cmd_rx,
                wake: wake.clone(),
                holds: held,
            },
            every,
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
        holds,
    }
}

/// Everything the session speaks to the outside through.
///
/// One parameter rather than four, because they arrive together, are made
/// together in `start_every`, and none of them means anything without the
/// others.
struct Wires {
    /// What the interface reads.
    state: watch::Sender<ChatState>,
    /// What it asks for.
    cmds: mpsc::UnboundedReceiver<Cmd>,
    /// How it is told to look again.
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The connection, for a call or a console to use. See
    /// [`ChatHandle::connection`].
    holds: sigil_net::Held,
}

async fn run(
    dial: Dial,
    signer: SoftwareSigner,
    store_at: Option<std::path::PathBuf>,
    wires: Wires,
    every: std::time::Duration,
) -> Result<(), String> {
    use sqnr_core::Signer;
    let Wires {
        state,
        mut cmds,
        wake,
        holds,
    } = wires;
    let seed = signer.seed();
    let me = PubKey::new(signer.public());

    let path = match store_at {
        Some(p) => p,
        None => store::store_path(&me).map_err(|e| e.to_string())?,
    };
    // Resolved before the lock, because the lock is per account **and
    // exchange**: what it protects is the SIP-17 counter, and the store keeps
    // one of those per pair. One lock per account would refuse a second
    // exchange over a conflict that does not exist.
    let endpoint = match &dial {
        Dial::At(e) => *e,
        Dial::Discover(layers) => {
            let mut silent = sqex_voice::engine::Silent;
            sqex_voice::engine::resolve(&layers[..], &mut silent).await?
        }
        // A session **owns** its connection: it is the thing that dials, holds
        // and redials it, and lends it to calls (`ChatHandle::connection`).
        // Handing it one to run on would invert that, and there would be
        // nothing left to say who redials.
        Dial::On(_) => {
            return Err("a chat session opens its own connection".to_string());
        }
    };
    // Held for the life of the session. Two interactive clients on one account
    // at one exchange would disagree about the next message counter, and
    // reusing one costs the confidentiality of two messages.
    let _lock = store::lock(&path, &endpoint.server).map_err(|e| {
        // Which exchange, so the interface can check whether the client
        // already holding it is one of its own. The message stands on its own
        // for the case where it is not.
        state.send_modify(|s| {
            s.exchange = Some(endpoint.server);
            s.locked_out = Some(endpoint.server);
        });
        format!("another client is already using this account at this exchange: {e}")
    })?;
    let store = Store::open(&seed, Some(&path)).map_err(|e| e.to_string())?;
    let client =
        sqnr::Client::connect_as(endpoint.address, endpoint.server.as_bytes(), &seed).await?;
    let mut chat = Chat::new(client, seed, me, endpoint.server, store);
    // The exchange's domain, for showing SIP-38 handles as `name@domain`.
    //
    // **Nothing set this.** `Chat::handle` needs a domain to compose one, so
    // it returned `None` for every account including our own — and a name
    // claimed at this exchange still read as unregistered afterwards, with
    // the claim having actually worked. Read off the same layers the
    // connection was made from, and only when they name a domain: an address
    // is not one, and `name@203.0.113.1` is not a handle.
    let domain = match &dial {
        Dial::Discover(layers) => sigil_net::domain_of(layers),
        // No domain to show: reached by a literal host and key, or — refused
        // above — on somebody else's connection.
        Dial::At(_) | Dial::On(_) => None,
    };
    chat.set_domain(domain.clone());
    // So a lost connection can be rebuilt without restarting the session.
    chat.dials(endpoint.address, endpoint.server.as_bytes().to_owned());

    state.send_modify(|s| {
        s.me = Some(me);
        s.exchange = Some(endpoint.server);
        s.domain = domain;
    });

    // **Somewhere for the exchange to knock.** The tick is a backstop now, not
    // the clock: an event says which conversation moved, and waiting 700ms to
    // hear it is 700ms of a message sitting in a queue that has already
    // crossed the world.
    let knock: sqex_chat::events::Wake = Arc::new(tokio::sync::Notify::new());
    chat.wake_on_events(knock.clone());

    let mut desk = Desk::default();
    // **Before the exchange is asked anything.** Everything below this point
    // is a round trip -- prekeys, then the list, then a fetch per channel --
    // and none of it is needed to draw what this machine already holds. The
    // local copy is also the only copy that can ever be read: opening an
    // epoch key spends the prekey it was sealed against, so the disc is not a
    // cache of the exchange's data, it is the data.
    sync_local(&mut chat, &mut desk, me);
    let _ = publish(&chat, &state, &desk, me);
    (wake)();

    chat.top_up_prekeys().await.map_err(|e| e.to_string())?;
    // The backstop's own clock. `sleep` rather than `interval`, because how
    // long to wait is decided each time round from what is outstanding.
    let busy = std::cmp::min(every, std::time::Duration::from_millis(TICK_MS));
    let mut ticked = tokio::time::Instant::now();
    // Set when a pass left work it could not finish -- a picture still to
    // fetch, chiefly -- so the next pass comes round at once rather than in
    // five seconds' time.
    let mut more = false;
    // Where a parked fetch hands back what it found. See [`Parked`].
    let (arrived_tx, mut arrived_rx) = mpsc::unbounded_channel::<Arrived>();
    let mut parked: Option<Parked> = None;
    // What was last handed out to be called on. See `ChatHandle::connection`.
    // `Retrying` rather than `Up`, so the first pass writes the connection this
    // session has just made instead of thinking it already had.
    let mut lent = Link::Retrying;

    loop {
        // **The connection, for whoever else wants to reach this exchange as
        // this identity.** Written when the link changes rather than every
        // pass: a redial makes a new connection, and what was lent before it
        // is closed.
        if chat.link() != lent {
            lent = chat.link();
            holds.set(chat.connection().map(|c| (c, endpoint)));
        }
        // **What is outstanding decides the wait.** A link being redialled
        // advances a slice per pass and would take minutes at the quiet
        // interval; a note has to disappear five seconds after it appeared,
        // not ten; a channel an event named is one somebody is waiting to see;
        // and "typing..." has to go out when somebody stops, which is the one
        // thing only asking can find (see `still_live`) -- at the quiet
        // interval it would linger five seconds after they had gone.
        let quick = more
            || chat.link() != Link::Up
            || !desk.dirty.is_empty()
            || desk.restructure
            || still_live(&desk).is_some()
            || state.borrow().note.is_some();
        let want = if quick { busy } else { every };
        let until = want.saturating_sub(ticked.elapsed());

        // **A fetch left waiting on the conversation being read.** See
        // [`Parked`]: it costs one request, answers in one trip the moment
        // anything is said, and is dropped the instant this loop has something
        // else to do -- including somebody typing, whose *stopping* only a
        // second ask can find.
        let wanted = (chat.link() == Link::Up && !quick)
            .then_some(desk.open)
            .flatten();
        if parked.as_ref().map(|p| p.channel) != wanted {
            unpark(&mut parked);
        }
        if let Some(channel) = wanted
            && parked.is_none()
            && let Some(watch) = chat.watch(&channel, sqex_proto::channel::MAX_WAIT)
        {
            let answer = arrived_tx.clone();
            let task = tokio::spawn(async move {
                let _ = match watch.arrived().await {
                    Ok(got) => answer.send(Arrived::Entries(Box::new(got))),
                    // Not acted on here: this task has no client to lower a
                    // link on. The loop asks the ordinary way instead, which
                    // does.
                    Err(_) => answer.send(Arrived::Trouble(channel)),
                };
            });
            parked = Some(Parked { channel, task });
        }

        tokio::select! {
            // Commands first and unconditionally. Typing must never wait behind
            // the network, which is the discipline `sqex-chat`'s own loop keeps
            // by handling keys before anything else.
            Some(cmd) = cmds.recv() => {
                apply(&mut chat, cmd, &state, &mut desk).await;
                (wake)();
            }
            // **What was already being waited for.** A parked fetch is a
            // question asked before there was an answer, so this is the whole
            // round trip: the entry is in what came back, not a hint that one
            // exists.
            Some(arrived) = arrived_rx.recv() => {
                // Answered, so the task is over whatever it said.
                parked = None;
                match arrived {
                    Arrived::Entries(got) => {
                        if absorb(&mut chat, &state, &mut desk, me, *got).await {
                            (wake)();
                        }
                    }
                    Arrived::Trouble(channel) => {
                        desk.dirty.insert(channel);
                    }
                }
            }
            // **What the exchange has to say, the moment it says it.**
            //
            // The events themselves arrive on their own QUIC stream and are
            // queued by a task that never waits for this loop; all this does is
            // stop the queue sitting there until the next tick. An event is a
            // hint -- "this channel moved" -- so the answer to one is the same
            // sweep the tick does, and no more.
            _ = knock.notified() => {
                for event in chat.take_events() {
                    desk.note(event, me);
                }
                if desk.restructure
                    && let Ok(()) = sync_channels(&mut chat, &mut desk).await
                {
                    desk.restructure = false;
                    desk.synced = true;
                    desk.synced_at = std::time::Instant::now();
                }
                if refresh(&mut chat, &state, &mut desk, me, &mut cmds).await {
                    (wake)();
                }
                // A message with pictures in it arrives as one event; without
                // this they would come in one per backstop, five seconds
                // apart, however fast the exchange was.
                more = fetch_files(&mut chat, &state, &mut desk, &mut cmds).await;
            }
            _ = tokio::time::sleep(until) => {
                ticked = tokio::time::Instant::now();
                chat.keep_alive().await;
                // **A fresh subscription carries nothing from before it was
                // made.** SIP-30's stream has no replay, so the list and every
                // conversation in it are stale on the far side of one -- which
                // is what `Chat::subscribe` means by "a caller must reconcile
                // after this returns". `Ok(true)` is a stream that was just
                // opened, so a reconnection reconciles and a tick that finds
                // one already open does nothing.
                if chat.link() == Link::Up
                    && !chat.subscribed()
                    && matches!(chat.subscribe().await, Ok(true))
                {
                    desk.restructure = true;
                    desk.dirty.extend(desk.channels.keys().copied());
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
                // A confirmation is about something just done, so it stops
                // being true. Cleared here rather than left for whatever
                // happens next to overwrite -- see `NOTE_SECS`.
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                // Checked before modifying: `send_modify` reports a change
                // whether or not anything changed, and a watch that always
                // says so wakes the interface on every tick for nothing.
                let mut moved = false;
                if state
                    .borrow()
                    .note
                    .as_ref()
                    .is_some_and(|n| now.saturating_sub(n.at) >= NOTE_SECS)
                {
                    state.send_modify(|s| s.note = None);
                    moved = true;
                }
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
                            desk.synced = true;
                            desk.synced_at = std::time::Instant::now();
                        }
                        Err(e) => {
                            state.send_modify(|s| s.trouble = Some(e));
                            moved = true;
                        }
                    }
                }
                learn_names(&mut chat, &mut desk).await;
                // **The conversation before the pictures in it.** A refresh is
                // what somebody is waiting for; a blob is what they will be
                // looking at in a moment. Measured the other way round, one
                // tick spent two and a half seconds on pictures before asking
                // whether anything had been said.
                moved |= refresh(&mut chat, &state, &mut desk, me, &mut cmds).await;
                more = fetch_files(&mut chat, &state, &mut desk, &mut cmds).await;
                // **Only when something moved.** eframe is reactive: with
                // nothing asking for a repaint it sleeps. This wake used to
                // fire on every tick regardless, which held the window at 1.4
                // frames a second for ever -- each of them cloning the state
                // several times and laying out every message in view -- for an
                // account where nothing at all was happening.
                if moved {
                    (wake)();
                }
            }
        }
    }
}

/// One conversation, as this client knows it.
struct Known {
    /// The other party, for a direct message. `None` for a group or a public
    /// channel.
    peer: Option<PubKey>,
    /// Anybody may find and join it, and nothing in it is encrypted. `None`
    /// until the exchange has said which; see [`Summary::public`].
    public: Option<bool>,
    /// More than two people.
    group: bool,
    label: String,
    /// Who may redact and rename. From the exchange, remembered so that a
    /// client starting offline folds its own history correctly.
    admins: Vec<PubKey>,
    /// Everybody in it, with the role the **exchange** attests.
    members: Vec<Member>,
    /// Where everybody's cursor is, as of the last time we asked.
    ///
    /// Fetched only for the conversation on screen: it is another round trip,
    /// and nobody is reading a receipt in a channel they are not looking at.
    marks: Vec<sqex_proto::channel::Mark>,
    timeline: Timeline,
    /// How many messages we had last time, so a new one can be counted unread
    /// without diffing two timelines.
    seen: usize,
    /// How many of this channel's messages the interface has asked for.
    ///
    /// Grows by [`PAGE`] each time somebody reaches the top and asks for
    /// earlier ones, and is reset when the conversation is opened afresh: a
    /// channel somebody scrolled a long way back into last week should not
    /// cost that again today.
    wanted: usize,
    /// The newest thing said here. What the list sorts on.
    last_at: u64,
    unread: usize,
    /// The read mark this client has already written to the exchange.
    ///
    /// **A cursor is only worth writing when it has moved.** It was written on
    /// every tick while a conversation was open; the exchange stores it
    /// unconditionally and publishes a `Cursor` event to every other member
    /// whether or not the value changed, and each of those marks the channel
    /// dirty at the other end, which fetches, which writes its own cursor.
    /// Two people with the same conversation open kept that going between them
    /// at 1.4 rounds a second, for as long as both windows were open, with
    /// nobody typing.
    told: u64,
    /// They have published no prekeys, so nothing can be sealed to them yet.
    waiting: bool,
    typing: bool,
    /// Whether the exchange has been asked about this channel yet, this
    /// session. What is on screen before that is this machine's own copy,
    /// which is worth showing at once and is not the whole story.
    fetched: bool,
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
    /// Channels whose read marks somebody has moved, so the receipts beside
    /// our own messages are worth asking about again.
    cursors_moved: HashSet<[u8; 32]>,
    /// The conversation list itself needs rebuilding from the exchange.
    restructure: bool,
    /// Whether the exchange has ever answered about the list, this session.
    /// What is on screen before that came off this machine's own disc.
    synced: bool,
    /// Accounts whose profile an event says has moved on.
    restale: HashSet<PubKey>,
    /// Calls somebody has said they are taking, by (channel, invitation).
    ///
    /// Ephemeral and never written: answering posts no entry, so this is the
    /// only place the fact lives. It drives what is on screen and must never
    /// be allowed to contradict the log.
    answered: HashSet<([u8; 32], u64)>,
    /// Files fetched and opened, by blob.
    ///
    /// Images in the conversation on screen are fetched **here**, on the
    /// session, and never by a view: a view runs sixty times a second and is
    /// the last place a download should start. Bounded by kind and by size —
    /// a hundred-megabyte video is not something to pull because somebody
    /// scrolled past it.
    files: HashMap<[u8; 32], std::sync::Arc<[u8]>>,
    /// The order they were fetched in, for [`to_put_down`].
    fetched: Vec<[u8; 32]>,
    /// Blobs we tried and could not get, so a broken one is not retried on
    /// every pass for as long as the conversation is open.
    unfetchable: HashSet<[u8; 32]>,
    /// When the list was last rebuilt. See [`BACKSTOP`].
    ///
    /// A backstop, not the mechanism. Events are what make this responsive,
    /// but a subscription can drop and reconnect with a gap in it, and a
    /// conversation list that is only ever event-driven would then be wrong
    /// until something else happened to change it -- which, for somebody who
    /// has been added to a channel and told about it nowhere else, is never.
    ///
    /// **A time rather than a count of ticks**, because a tick is no longer a
    /// fixed length: the loop waits on the exchange and falls back to a timer
    /// whose interval depends on whether anything is outstanding.
    synced_at: std::time::Instant,
}

impl Default for Desk {
    fn default() -> Self {
        Desk {
            channels: HashMap::new(),
            open: None,
            dirty: HashSet::new(),
            cursors_moved: HashSet::new(),
            restale: HashSet::new(),
            answered: HashSet::new(),
            files: HashMap::new(),
            fetched: Vec::new(),
            unfetchable: HashSet::new(),
            // The first tick has nothing yet, so it rebuilds.
            restructure: true,
            synced: false,
            synced_at: std::time::Instant::now(),
        }
    }
}

/// How long the conversation list can be wrong before the backstop fixes it.
///
/// Exposed so a test can say what it is pointed at. A test that waits *longer*
/// than this proves only that the backstop works — it passes whether or not
/// SIP-30's events are being acted on at all, which is how the stranger test
/// passed at 28.9 seconds while the event path was broken.
///
/// **Counted in time, not in ticks.** A tick is no longer a fixed length: the
/// loop waits on what the exchange has to say and falls back to a timer whose
/// interval depends on whether anything is outstanding, so counting forty of
/// them would mean anything between half a minute and three.
pub const BACKSTOP: std::time::Duration = std::time::Duration::from_secs(28);

impl Desk {
    fn note(&mut self, event: Event, me: PubKey) {
        match event {
            Event::Channel { channel, .. } | Event::Signal { channel } => {
                self.dirty.insert(channel);
            }
            // Somebody's read mark moved. Ours moving is not news; theirs is,
            // once receipts are drawn.
            // Somebody read something. The receipts beside our own messages
            // are the only thing that changes, so this asks for the marks and
            // not for the entries.
            Event::Cursor { channel } => {
                self.cursors_moved.insert(channel);
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
            // SIP-36: a call is ringing. The event names the channel and the
            // invitation and carries nothing else, so the entry is fetched to
            // learn the rest — which is the point of it being an event rather
            // than a message.
            //
            // **Redundant with `Channel`, on purpose.** An invitation is also
            // an ordinary entry, so posting one emits both kinds, and the test
            // for this passes with either arm removed and fails only with both
            // — which is how it was measured rather than assumed. Keeping this
            // arm is what lets a client tell a phone ringing from somebody
            // typing without fetching to find out, and it is the kind that
            // carries SIP-21's blocking rules.
            //
            // It is also what the mailbox ring listener was a stopgap for.
            // That was written when no such kind existed, swept the mailbox
            // every two seconds, and cost about a thousand requests a day
            // finding nothing. It is gone.
            Event::Ringing { channel, .. } => {
                self.dirty.insert(channel);
            }
            Event::Admission | Event::Heartbeat | Event::CrossCall { .. } | Event::Unknown(_) => {}
        }
    }

    fn age(&mut self) {
        if self.synced_at.elapsed() >= BACKSTOP {
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
                public: Some(public),
                group,
                label: String::new(),
                admins: Vec::new(),
                members: Vec::new(),
                marks: Vec::new(),
                timeline,
                seen,
                wanted: PAGE,
                last_at,
                unread: 0,
                told: 0,
                waiting: false,
                typing: false,
                fetched: false,
                trouble: Trouble::default(),
            }
        });
        entry.peer = peer.map(|(a, _)| a);
        entry.public = Some(public);
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
            public: Some(false),
            group: false,
            label: if c.label.is_empty() {
                c.account.to_string()
            } else {
                c.label.clone()
            },
            admins: vec![me, c.account],
            members: Vec::new(),
            marks: Vec::new(),
            timeline: Timeline::default(),
            seen: 0,
            wanted: PAGE,
            unread: 0,
            // Nothing has happened here yet, so it sorts below anything that
            // has rather than claiming a time it does not have.
            last_at: 0,
            told: 0,
            waiting: false,
            typing: false,
            fetched: false,
            trouble: Trouble::default(),
        });
    }

    // Left, removed, or closed. Dropped from the list rather than left on it
    // as a conversation nothing can be sent to.
    //
    // **And forgotten on the disc**, or it comes back: the list is folded from
    // the store before the exchange is asked anything, so a channel the
    // exchange no longer lists reappeared on every launch and vanished a
    // second later. Two conversations called "general", one of them ten days
    // stale, is what that looked like.
    //
    // Only the row that makes it a conversation: `forget_channel` clears
    // `channel_meta` and leaves the messages, the keys and the cursor alone.
    // And only against a *complete* answer -- `mine` pages internally and
    // either returns all of it or fails, and this function returns before here
    // when it fails.
    for gone in desk.channels.keys().filter(|c| !present.contains(*c)) {
        let _ = chat.store().forget_channel(gone);
    }
    desk.channels.retain(|c, _| present.contains(c));
    desk.dirty.retain(|c| present.contains(c));
    ask_about_unfetched(desk);
    Ok(())
}

/// Everything the exchange has never answered about is asked about once.
///
/// **Events say what changed while somebody was listening.** They say nothing
/// about what changed before that: SIP-30's stream carries what happens from
/// the moment it is opened and has no replay, so every entry posted while this
/// client was shut down -- or in the second between its connection coming up
/// and its subscription being made -- is news nothing will ever mention again.
/// Without this the only thing that fetched such a conversation was somebody
/// opening it, so an overnight message left the list showing yesterday and the
/// unread count showing nothing.
///
/// A stranger's first message is the sharp end of it: the `Membership` and
/// `Channel` events that announce it are both published in the moment the
/// conversation is created, which is before the person being written to has
/// heard of the channel at all. The row arrives from `mine()`; this is what
/// then reads it.
///
/// Cheap by construction: `fetched` is set by the first answered poll, so this
/// hands over each conversation once per session, and `this_tick` spreads them
/// a few per pass.
fn ask_about_unfetched(desk: &mut Desk) {
    let asking: Vec<[u8; 32]> = desk
        .channels
        .iter()
        .filter(|(_, k)| !k.fetched)
        .map(|(c, _)| *c)
        .collect();
    desk.dirty.extend(asking);
}

/// Everything this machine already knows, before the exchange is asked.
///
/// # Why this exists at all
///
/// The conversation list came from `mine()` and every transcript from a
/// `fetch`, so nothing was on screen until the connection was up, the prekeys
/// topped up, the list fetched and the open channel polled -- four round trips
/// before the first word appeared, on a client whose disc already held every
/// one of those words. A public channel with a few hundred messages therefore
/// "took a long time to load" while its whole history sat in a file.
///
/// What the store cannot say is which of its groups are **public**: it keeps
/// `kind` as group-or-not, and nothing else. That is why `public` is an
/// `Option` and stays `None` here -- see [`Summary::public`]. A direct message
/// is the exception, being never public and derivable: its identifier is
/// derived from the two accounts, so the other member of a two-member channel
/// that derives back to itself is the peer.
fn sync_local(chat: &mut Chat, desk: &mut Desk, me: PubKey) {
    let Ok(channels) = chat.store().channels() else {
        return;
    };
    for (channel, group, label, admins) in channels {
        let timeline = chat.history(&channel, &admins).unwrap_or_default();
        let last_at = timeline.messages().last().map(|m| m.posted).unwrap_or(0);
        let seen = timeline.messages().count();
        let peer = (!group)
            .then(|| admins.iter().copied().find(|a| *a != me))
            .flatten()
            .filter(|other| chat.dm_with(other) == channel);
        desk.channels.entry(channel).or_insert(Known {
            peer,
            // Not `false`. Drawing a public channel as private claims its
            // contents are sealed, and drawing a private group as public
            // claims the opposite; neither is a guess to make on somebody's
            // behalf, and the answer is one round trip away.
            public: if group { None } else { Some(false) },
            group,
            label,
            // Remembered from the last time the exchange said so, which is
            // what the fold above just used.
            admins,
            members: Vec::new(),
            marks: Vec::new(),
            timeline,
            seen,
            wanted: PAGE,
            last_at,
            unread: 0,
            told: 0,
            waiting: false,
            typing: false,
            fetched: false,
            trouble: Trouble::default(),
        });
    }
    // What the disc holds is what was true when this client last ran. See
    // `ask_about_unfetched`: everything here is asked about once, so a
    // conversation that moved overnight says so without being opened.
    ask_about_unfetched(desk);
}

/// How many conversations one tick will ask the exchange about.
///
/// Every one of them is a round trip -- measured at 100-250ms to a real
/// exchange -- and they are made one after another, so a tick that polls
/// everything the events named takes seconds. During that the task is inside
/// `refresh` and cannot take a command, which is what made switching
/// conversations slow: the click waited for the whole sweep.
///
/// The rest stay dirty and are asked about on the next tick. Nothing is lost
/// by being late; the events say what changed, and they keep saying it.
const PER_TICK: usize = 4;

/// Which conversations this tick asks about, and which wait for the next one.
///
/// The open one first **when it is being asked about at all**: it is the one
/// somebody is looking at, and everything else in the sweep is somewhere they
/// are not. It is no longer added to the sweep by being open -- see
/// [`refresh`] -- so this orders what it is given rather than growing it. The
/// rest are bounded by [`PER_TICK`] and keep their turn rather than losing it:
/// being late costs nothing, since the events that named them keep naming them
/// until they are read.
///
/// A free function over plain data because the ordering is the part worth
/// testing: a sweep that starves the conversation on screen is one where new
/// messages arrive everywhere except where somebody is reading.
fn this_tick(open: Option<[u8; 32]>, dirty: Vec<[u8; 32]>) -> (Vec<[u8; 32]>, Vec<[u8; 32]>) {
    let mut now: Vec<[u8; 32]> = dirty;
    if let Some(open) = open
        && now.contains(&open)
    {
        now.retain(|c| *c != open);
        now.insert(0, open);
    }
    let later = if now.len() > PER_TICK {
        now.split_off(PER_TICK)
    } else {
        Vec::new()
    };
    (now, later)
}

/// The conversation whose state will change with nothing to announce it.
///
/// Everything else is event-driven now (see [`refresh`]), which works because
/// every *beginning* is an event: an entry, a read mark, a membership, a ring,
/// and a signal all produce one. Endings mostly are too — a call that is
/// declined or hung up writes an entry, and a ring that nobody answers
/// acquires `CALL_MISSED` from its own window with no help from anybody.
///
/// The exception is somebody who **stops typing**. SIP-19's signal is a "now":
/// the exchange relays it, stores it nowhere, and lets it lapse in silence.
/// There is no event for a lapse, so a client that stopped asking would latch
/// the indicator on and leave a conversation nobody had touched for an hour
/// saying somebody was writing in it.
///
/// Bounded by the state itself: the signals stop, the next fetch carries none,
/// and this stops naming it. Nothing here can hold the loop at its busy
/// interval for longer than somebody is actually typing.
///
/// **Answering a call is not in this list, and was**: taking a call posts no
/// entry, so `Conversation::accepted` is the only thing that ever says so, and
/// that reads like a state only a poll can find. It is not — `ring_state`
/// signals, and a signal is an event. Measured, not reasoned about: the test
/// that a caller learns their call was taken passes with this clause removed,
/// which is why it is not here.
fn still_live(desk: &Desk) -> Option<[u8; 32]> {
    desk.open
        .filter(|open| desk.channels.get(open).is_some_and(|k| k.typing))
}

/// Anything the reader has asked for, before the next round trip.
///
/// **Between the round trips, not after them.** A conversation opens from the
/// disc and needs no network at all, so the only reason a switch waited was
/// that the task was in the middle of a sweep it could not be interrupted in.
async fn attend(
    chat: &mut Chat,
    state: &watch::Sender<ChatState>,
    desk: &mut Desk,
    cmds: &mut mpsc::UnboundedReceiver<Cmd>,
) {
    while let Ok(cmd) = cmds.try_recv() {
        apply(chat, cmd, state, desk).await;
    }
}

/// A fetch left waiting at the exchange for the conversation on screen.
///
/// # Why one is worth holding
///
/// `/channel/fetch` takes a wait: the exchange holds the request open and
/// answers the moment an entry or a signal arrives. Everything else here is
/// event-driven, and an event is a *hint* -- "this conversation moved" -- which
/// costs a fetch to act on. This is that fetch, sent before there was anything
/// to fetch, so a message reaches the screen in one trip instead of two. On a
/// real exchange that is a round trip saved on the one conversation somebody is
/// actually looking at.
///
/// # When one is held
///
/// Only while the loop has nothing else to do, and only on the open
/// conversation. Anything outstanding -- a channel an event named, a
/// reconnection, a picture still to fetch, somebody typing -- drops it, because
/// all of those are about to fetch anyway and a second answer is a wasted
/// request. It is dropped on switching conversations for the same reason.
///
/// **Nothing is lost by dropping one.** The cursor only ever moves forward
/// (`Store::set_since` takes a `MAX`), and SIP-17's counters refuse a second
/// decryption of an entry already read, so an answer that arrives late is
/// absorbed harmlessly or not at all.
struct Parked {
    channel: [u8; 32],
    task: tokio::task::JoinHandle<()>,
}

/// What a [`Parked`] fetch came back with.
enum Arrived {
    /// Entries, signals, or both. Boxed because everything else in this enum is
    /// two words and a fetch's answer is a buffer off the wire.
    Entries(Box<sqex_chat::Fetched>),
    /// It could not be asked, or the exchange refused. The channel is asked
    /// about the ordinary way, which is the path that knows what to do about a
    /// link that has gone.
    Trouble([u8; 32]),
}

/// Stop waiting for an answer nobody wants any more.
fn unpark(parked: &mut Option<Parked>) {
    if let Some(p) = parked.take() {
        p.task.abort();
    }
}

/// Fold what a parked fetch brought back, exactly as the sweep would have.
///
/// The opening, the counters and the store are all `Chat`'s, and this is where
/// they happen -- the parked half never touched them.
async fn absorb(
    chat: &mut Chat,
    state: &watch::Sender<ChatState>,
    desk: &mut Desk,
    me: PubKey,
    got: sqex_chat::Fetched,
) -> bool {
    let channel = got.channel();
    let Some(known) = desk.channels.get_mut(&channel) else {
        return false;
    };
    let mut timeline = std::mem::take(&mut known.timeline);
    let absorbed = chat.absorb(&mut timeline, got).await;
    let open = desk.open;
    let Some(known) = desk.channels.get_mut(&channel) else {
        return false;
    };
    let accepted = match absorbed {
        Ok(conversation) => took(chat.store(), known, channel, open, conversation),
        Err(_) => {
            known.timeline = timeline;
            // Whatever went wrong, the ordinary path reports it and decides
            // what it means for the connection.
            desk.dirty.insert(channel);
            return false;
        }
    };
    if let Some(seq) = accepted {
        desk.answered.insert((channel, seq));
    }
    publish(chat, state, desk, me)
}

/// Take everything a fetch turned up into what the interface reads.
///
/// One copy, because there are two ways in: the sweep, which fetches what an
/// event named, and a parked fetch that was already waiting when it happened
/// (see [`Parked`]). Both hand back the same `Conversation`, and a second copy
/// of this is a second place for the unread count or the trouble flags to be
/// got subtly differently.
///
/// Returns the sequence number of a call somebody has taken, which the caller
/// records: `desk` is borrowed for the conversation being written.
fn took(
    store: &sqex_chat::Store,
    known: &mut Known,
    channel: [u8; 32],
    open: Option<[u8; 32]>,
    conversation: sqex_chat::Conversation,
) -> Option<u64> {
    known.timeline = conversation.timeline;
    known.typing = conversation.typing;
    known.waiting = false;
    // Answered for. Until this, what is on screen came off the disc and the
    // interface says it is still asking.
    known.fetched = true;
    known.trouble = Trouble {
        unreadable: conversation.unreadable.len(),
        gap: conversation.gap,
        restarted: conversation.restarted,
        no_key: conversation.no_key,
        lost: conversation.lost,
        forged: known.timeline.forged().len(),
    };
    if !conversation.admins.is_empty() {
        known.admins = conversation.admins;
    }
    let after = known.timeline.messages().count();
    // Counted against what we had rather than against a read mark, so a
    // message arriving in a conversation nobody is looking at is counted once,
    // when it arrives.
    if after > known.seen && open != Some(channel) {
        known.unread += after - known.seen;
    }
    known.seen = after;
    if let Some(newest) = known.timeline.messages().last().map(|m| m.posted) {
        known.last_at = known.last_at.max(newest);
    }
    // A group's name lives in a sealed entry, so it is only known once the log
    // has been read -- and it changes when an admin renames it.
    let named = known.timeline.name.clone();
    if known.peer.is_none() && !named.is_empty() && named != known.label {
        known.label = named.clone();
        let _ = store.set_label(&channel, &named);
    }
    // Somebody said they are taking a call. The only way to learn it:
    // answering posts no entry, so without this the caller goes on showing
    // "ringing" and then derives *missed* of a call that is up and being
    // spoken on.
    conversation.accepted
}

/// Fetch what has changed and publish the result.
async fn refresh(
    chat: &mut Chat,
    state: &watch::Sender<ChatState>,
    desk: &mut Desk,
    me: PubKey,
    cmds: &mut mpsc::UnboundedReceiver<Cmd>,
) -> bool {
    // **Only what something said had changed.** The open conversation used to
    // be fetched every pass, on the grounds that it is the one somebody is
    // looking at -- which cost a round trip per client per tick, for ever,
    // with nobody saying anything. Every kind of news in it arrives as a
    // SIP-30 event: an entry, a typing signal, a read mark, a ring, a
    // membership. A stream that stops carrying them is not mistaken for a
    // quiet one either: it heartbeats, and `Chat::take_events` drops one that
    // has gone silent, after which the loop resubscribes and reconciles
    // everything.
    //
    // What has no event is somebody *stopping* typing, so the conversation
    // that says they are is asked about until it stops -- see `still_live`.
    desk.dirty.extend(still_live(desk));
    let (to_poll, later) = this_tick(desk.open, desk.dirty.drain().collect());
    desk.dirty.extend(later);

    // Collected rather than written straight into `desk`, which is borrowed
    // mutably for the channel being polled. Same for what is open.
    let mut accepted: Vec<([u8; 32], u64)> = Vec::new();
    let open = desk.open;

    for channel in to_poll {
        attend(chat, state, desk, cmds).await;
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
                if let Some(seq) = took(chat.store(), known, channel, open, conversation) {
                    accepted.push((channel, seq));
                }
            }
            Err(_) => {
                known.timeline = timeline;
            }
        }
    }

    desk.answered.extend(accepted);

    // Reading it is what clears it, and advancing the exchange's mark is what
    // makes "where was I" survive closing the client.
    if let Some(open) = desk.open
        && let Some(known) = desk.channels.get_mut(&open)
    {
        known.unread = 0;
        // **Only when it has moved.** See `Known::told`: writing an unchanged
        // cursor tells every other member that something happened, and what
        // they do about it is fetch and write their own.
        if let Some(last) = known.timeline.messages().last().map(|m| m.seq)
            && last > known.told
        {
            known.told = last;
            let _ = chat.mark_read(&open, last).await;
        }
    }

    // Everybody else's cursor, for the conversation on screen only. Another
    // round trip, and nobody is reading a receipt in a channel they are not
    // looking at.
    //
    // **Reciprocity is enforced at the exchange**: opting out of receipts
    // withholds others' reading from you, and never withholds `delivered`,
    // which the exchange observes whether or not anybody consents to report
    // it. So a mark of `read: 0` beside a real `delivered` is somebody who
    // opted out, not somebody who has not read -- and `receipt_for`
    // under-claims on exactly that.
    // **Asked for when somebody's cursor moved**, which is what the `Cursor`
    // event says, and when a conversation is opened. It was another round trip
    // on every tick to be told the same numbers as the tick before.
    if let Some(open) = desk.open
        && desk.cursors_moved.remove(&open)
        && let Ok(marks) = chat.marks(&open).await
        && let Some(known) = desk.channels.get_mut(&open)
    {
        known.marks = marks;
    }

    publish(chat, state, desk, me)
}

/// Re-read who we are blocking.
async fn refresh_blocked(chat: &mut Chat, state: &watch::Sender<ChatState>) {
    match chat.blocked().await {
        Ok(blocked) => state.send_modify(|s| s.blocked = blocked),
        Err(e) => trouble(state, e),
    }
}

/// Re-read who this account's devices are, and whether we are still one.
async fn refresh_devices(chat: &mut Chat, state: &watch::Sender<ChatState>) {
    let this = chat.device();
    let devices = match chat.my_devices().await {
        Ok(devices) => devices
            .into_iter()
            .map(|d| Linked {
                device: d.device,
                added: d.added,
                not_after: d.not_after,
                is_this_one: d.device == this,
            })
            .collect(),
        Err(e) => {
            trouble(state, e);
            return;
        }
    };
    let linked = chat.still_linked().await.ok().flatten();
    state.send_modify(|s| {
        s.devices = devices;
        s.linked = linked;
    });
}

/// The longest edge of a thumbnail, in pixels.
///
/// It rides inside the message, which is capped, so this has to stay small
/// enough that a photograph does not push the post over the limit on its own.
const THUMBNAIL_EDGE: u32 = 96;

/// A small picture of an image file, to carry inside the message.
///
/// `None` for anything that will not decode. A missing thumbnail is ordinary —
/// SIP-18 makes the field optional and every reader has to cope with an empty
/// one — so a file that cannot be previewed is still sent.
fn thumbnail(path: &std::path::Path) -> Option<Vec<u8>> {
    let image = image::ImageReader::open(path).ok()?.decode().ok()?;
    let small = image.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE);
    let mut out = std::io::Cursor::new(Vec::new());
    // PNG rather than the source format: a thumbnail of a JPEG is small enough
    // that the difference does not matter, and one encoder is one thing that
    // can go wrong.
    small.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// The largest file fetched without being asked for.
///
/// An image is worth pulling so a conversation reads as a conversation; a
/// video is not, and neither is a large photograph on a metered connection.
/// Everything above this waits to be asked for, which is what the Save control
/// is.
const AUTO_FETCH_MAX: u64 = 4 * 1024 * 1024;

/// How much of somebody's pictures this session keeps in hand.
///
/// **Fetched files used to be kept for ever.** Scrolling back through a channel
/// with pictures in it grew the process without limit -- four megabytes at a
/// time, three times over between the session, egui's cache and the texture --
/// and nothing ever put any of it down.
///
/// Sixty-four megabytes is a few dozen photographs, which is more than a
/// reader is looking at and less than a machine will notice. What goes is what
/// was fetched longest ago, because pictures are fetched as somebody scrolls
/// and the oldest are the furthest from where they now are; what stays,
/// whatever its age, is anything in the conversation on screen.
const HOLD_BYTES: usize = 64 * 1024 * 1024;

/// Which files to put down, now that `held` has grown past `budget`.
///
/// Returns them oldest-first, and never returns one that is `on_screen` --
/// evicting what somebody is looking at would fetch it again immediately, and
/// the picture would blink.
///
/// A free function over plain data because the policy is the part worth
/// testing, and a cache that quietly keeps everything looks exactly like one
/// that is working.
fn to_put_down(
    order: &[[u8; 32]],
    size: impl Fn(&[u8; 32]) -> usize,
    on_screen: &HashSet<[u8; 32]>,
    budget: usize,
) -> Vec<[u8; 32]> {
    let mut total: usize = order.iter().map(&size).sum();
    let mut go = Vec::new();
    for blob in order {
        if total <= budget {
            break;
        }
        if on_screen.contains(blob) {
            continue;
        }
        total -= size(blob);
        go.push(*blob);
    }
    go
}

/// Fetch the images in the conversation on screen.
///
/// Bounded three ways — kind, size, and one attempt per blob — because this
/// runs on a tick. `download` verifies the blob's name against the ciphertext
/// **before decrypting**, so what arrives is what was named or nothing.
/// Fetch one picture, and say whether more are waiting.
async fn fetch_files(
    chat: &mut Chat,
    state: &watch::Sender<ChatState>,
    desk: &mut Desk,
    cmds: &mut mpsc::UnboundedReceiver<Cmd>,
) -> bool {
    let Some(open) = desk.open else { return false };
    let Some(known) = desk.channels.get(&open) else {
        return false;
    };
    let wanted: Vec<sqex_proto::blob::Attachment> = known
        .timeline
        .messages()
        .flat_map(|m| m.post.attachments())
        .filter(|a| a.effective_kind() == sqex_proto::blob::KIND_IMAGE)
        .filter(|a| a.size <= AUTO_FETCH_MAX)
        .filter(|a| !desk.files.contains_key(&a.blob) && !desk.unfetchable.contains(&a.blob))
        .cloned()
        .collect();

    // **One a pass.** A picture takes as long as it takes -- one of them was
    // measured at two and a half seconds -- and the whole of that is time the
    // task cannot answer anybody in. The caller comes straight back round for
    // the next one, so ten pictures is ten passes and not ten backstops.
    let waiting = wanted.len();
    let Some(a) = wanted.into_iter().next() else {
        return false;
    };
    // And not at all while somebody is waiting for something. A reader who has
    // moved on should not be behind a picture for a conversation that is
    // already on the disc.
    attend(chat, state, desk, cmds).await;
    if !cmds.is_empty() {
        return true;
    }
    match chat.download(&a).await {
        Ok(bytes) => {
            if desk.files.insert(a.blob, bytes.into()).is_none() {
                desk.fetched.push(a.blob);
            }
            put_down_what_is_not_wanted(desk);
        }
        // Remembered as a failure rather than retried every pass. A blob that
        // has passed its retention window is gone, and asking again four times
        // a second will not bring it back.
        Err(_) => {
            desk.unfetchable.insert(a.blob);
        }
    }
    waiting > 1
}

/// Keep the held files inside [`HOLD_BYTES`], sparing the conversation on
/// screen.
fn put_down_what_is_not_wanted(desk: &mut Desk) {
    let on_screen: HashSet<[u8; 32]> = desk
        .open
        .and_then(|c| desk.channels.get(&c))
        .map(|k| {
            k.timeline
                .messages()
                .flat_map(|m| m.post.attachments())
                .map(|a| a.blob)
                .collect()
        })
        .unwrap_or_default();
    let go = to_put_down(
        &desk.fetched,
        |b| desk.files.get(b).map(|f| f.len()).unwrap_or(0),
        &on_screen,
        HOLD_BYTES,
    );
    for blob in go {
        desk.files.remove(&blob);
        desk.fetched.retain(|b| *b != blob);
    }
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
fn publish(chat: &Chat, state: &watch::Sender<ChatState>, desk: &Desk, me: PubKey) -> bool {
    // Whether what follows is the whole story or only this machine's copy of
    // it. Both are worth drawing; only one of them means "there is nothing
    // here", and saying that during the other is how a conversation somebody
    // has been having for a month greets them as empty.
    let loading = desk
        .open
        .and_then(|c| desk.channels.get(&c))
        .map(|k| !k.fetched)
        .unwrap_or(desk.open.is_some());
    let synced = desk.synced;
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
    let lines: Vec<Line> = open
        .map(|(_, k)| {
            // A stub of what each message says, so a reply can name it. Built
            // once rather than searched per reply: a conversation full of
            // replies would otherwise be quadratic in its own length.
            // `broken` is a list rather than a field on the message: the
            // fold keeps it separate so that a client which ignores it still
            // shows the message, which is right for a gap and would be wrong
            // for a fork. Turned into a lookup once, not searched per line.
            let standing: HashMap<u64, Standing> = k
                .timeline
                .broken()
                .iter()
                .map(|(seq, verdict)| {
                    (
                        *seq,
                        match verdict {
                            Verdict::Fork => Standing::Fork,
                            Verdict::Unattributed => Standing::Unattributed,
                            // `Gap` is what is left; `Valid` never reaches
                            // this list and `Forged` never becomes a message.
                            _ => Standing::Gap,
                        },
                    )
                })
                .collect();
            // The **last** `wanted` of them, which is where a conversation is
            // read from. Everything before that stays in the fold and in the
            // store; this only bounds how much is turned into something
            // drawable at once. See [`PAGE`].
            let total = k.timeline.messages().count();
            let window = total.saturating_sub(k.wanted);

            // A stub of what each message says, so a reply can name it. Only
            // for what a reply in the window actually points at, and looked up
            // in the fold rather than built for every message that ever
            // existed -- which is precisely the work the window exists to
            // avoid doing.
            let stubs: HashMap<u64, (PubKey, String)> = k
                .timeline
                .messages()
                .skip(window)
                .filter_map(|m| m.post.reply_to())
                .filter_map(|target| k.timeline.get(target))
                .map(|m| {
                    let said = if m.redacted {
                        "deleted".to_string()
                    } else {
                        stub(m.post.body_text().unwrap_or_default())
                    };
                    (m.seq, (m.account, said))
                })
                .collect();
            k.timeline
                .messages()
                .skip(window)
                .map(|m| Line {
                    seq: m.seq,
                    who: m.account,
                    name: people.get(&m.account).and_then(|p| p.name.clone()),
                    mine: m.account == me,
                    at: m.posted,
                    text: m.post.body_text().unwrap_or_default().to_string(),
                    redacted: m.redacted,
                    edited: m.edited.is_some(),
                    reactions: m
                        .reactions
                        .iter()
                        .map(|(emoji, who)| (emoji.clone(), who.len(), who.contains(&me)))
                        .collect(),
                    reply_to: m.post.reply_to().and_then(|target| {
                        stubs.get(&target).map(|(account, said)| {
                            let named = people
                                .get(account)
                                .and_then(|p| p.name.clone())
                                .unwrap_or_else(|| short(account));
                            (named, said.clone())
                        })
                    }),
                    // Only ever on our own. On somebody else's it would be a
                    // claim about our own reading, shown back to us.
                    receipt: (m.account == me).then(|| receipt_for(k, m.seq, &me)),
                    attachments: m
                        .post
                        .attachments()
                        .map(|a| Attached {
                            // From the kind, never the mime: that is the
                            // sender's claim, and SIP-18 forbids dispatching
                            // on it beyond choosing how to display.
                            kind: a.effective_kind(),
                            described: sqex_chat::attach::describe(a),
                            size: a.size,
                            preview: a.preview.as_slice().into(),
                            bytes: desk.files.get(&a.blob).cloned(),
                            // Asked for and refused, as against not reached
                            // yet. The two look the same on screen otherwise,
                            // and only one of them is worth waiting for.
                            missing: desk.unfetchable.contains(&a.blob),
                            id: bs58::encode(a.blob).into_string(),
                        })
                        .collect(),
                    standing: standing.get(&m.seq).copied().unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // How many are behind the window, so the reader can be offered them.
    let earlier = open
        .map(|(_, k)| k.timeline.messages().count().saturating_sub(k.wanted))
        .unwrap_or(0);

    // What happened *to* the channel, from the exchange's own signed entries.
    let events: Vec<Happened> = open
        .map(|(_, k)| {
            let named = |who: &PubKey| {
                people
                    .get(who)
                    .and_then(|p| p.name.clone())
                    .unwrap_or_else(|| short(who))
            };
            // In a direct message the membership *is* the channel: it is made
            // when you first write, both of you are added, and the key is
            // rotated -- three lines of machinery at the head of every
            // conversation, before a word has been said.
            //
            // So they are left out of a DM and kept everywhere else. What
            // stays in a DM is everything that tells somebody something they
            // did not already know by opening it: a removal, a retention
            // change, and above all replication -- another operator holding a
            // copy of your one-to-one conversation is not housekeeping.
            let plumbing = [
                EVENT_CREATED,
                EVENT_ADDED,
                EVENT_JOINED,
                EVENT_ROTATED,
                EVENT_PROMOTED,
                EVENT_DEMOTED,
                EVENT_RENAMED,
            ];
            let dm = k.peer.is_some();
            // From the same point as the messages. An event above the first
            // message drawn would sit at the top of the transcript describing
            // something that happened before anything on screen.
            // Only while there is something behind the window. With the whole
            // conversation on screen this cut every event that came *before*
            // the first message -- which is all of them in a new channel,
            // where the exchange writes the creation and the invitations
            // before anybody has said a word.
            let behind = k.timeline.messages().count().saturating_sub(k.wanted);
            let first = if behind == 0 {
                0
            } else {
                k.timeline
                    .messages()
                    .nth(behind)
                    .map(|m| m.seq)
                    .unwrap_or(0)
            };
            k.timeline
                .events()
                .filter(|h| h.seq >= first)
                .filter(|h| !(dm && plumbing.contains(&h.what.event)))
                .map(|h| {
                    let (actor, subject) = (h.what.actor, h.what.subject);
                    let (who, them) = (named(&actor), named(&subject));
                    // The exchange's own record put into words. `you` rather
                    // than your own name: reading "Ada added Ada" about
                    // yourself is a puzzle nobody should have to solve.
                    let me_or = |k: &PubKey, name: &str| {
                        if *k == me {
                            "you".to_string()
                        } else {
                            name.to_string()
                        }
                    };
                    let a = me_or(&actor, &who);
                    let b = me_or(&subject, &them);
                    let (said, caveat) = match h.what.event {
                        EVENT_CREATED => (format!("{a} made this channel"), None),
                        EVENT_ADDED => (format!("{a} added {b}"), None),
                        EVENT_REMOVED => (
                            format!("{a} removed {b}"),
                            // Said here because it is the one thing a reader
                            // would get wrong: removal rotates the key, and it
                            // does not take back what they already hold.
                            Some(
                                "The key was rotated. Everything they were given before \
                                 stays readable to them.",
                            ),
                        ),
                        EVENT_LEFT => (format!("{b} left"), None),
                        EVENT_JOINED => (format!("{b} joined"), None),
                        EVENT_PROMOTED => (format!("{a} made {b} an admin"), None),
                        EVENT_DEMOTED => (format!("{a} took {b}'s admin away"), None),
                        EVENT_ROTATED => (
                            format!("{a} rotated the key"),
                            Some("Messages from before it are still readable to whoever held the old one."),
                        ),
                        EVENT_RETENTION => (
                            format!("{a} changed how long messages are kept"),
                            // Narrowing retention is a deletion, applied at
                            // once, and it is the sort of thing somebody
                            // learns afterwards if nobody says it.
                            Some("Narrowing it deletes what falls outside, immediately."),
                        ),
                        EVENT_RENAMED => (format!("{a} changed the name or topic"), None),
                        // The subject of these two is an **exchange**, not a
                        // person, so it is never resolved as one.
                        EVENT_REPLICATE => (
                            format!("{a} let {} hold a copy of this channel", short(&subject)),
                            Some("Another operator now receives everything posted here."),
                        ),
                        EVENT_UNREPLICATE => (
                            // Never "recalled". SIP-35 is explicit that this
                            // is the end of a subscription and takes nothing
                            // back, and an interface that implied otherwise
                            // would be telling somebody they are safe.
                            format!("{a} stopped {} receiving this channel", short(&subject)),
                            Some("What it already holds stays where it is. Nothing is recalled."),
                        ),
                        // Unreachable: an event this version does not know
                        // decodes to nothing at all and never reaches here.
                        _ => (format!("{a} did something this version does not know"), None),
                    };
                    Happened {
                        seq: h.seq,
                        at: h.posted,
                        said,
                        actor,
                        subject,
                        caveat,
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    // A call that happened is a thing that happened here.
    //
    // `Timeline` folds a SIP-36 invitation into a `CallRecord` rather than a
    // message, so without this a call leaves the transcript with no trace at
    // all -- somebody scrolling back sees a silence where a conversation was.
    // One still ringing is left out: the banner has it, and it is not history
    // yet.
    let mut events = events;
    if let Some((_, k)) = open {
        let named = |who: &PubKey| {
            people
                .get(who)
                .and_then(|p| p.name.clone())
                .unwrap_or_else(|| short(who))
        };
        for call in k.timeline.calls() {
            let Some(outcome) = call.outcome(now) else {
                continue;
            };
            let mine = call.account == me;
            let them = named(&call.account);
            let said = match outcome {
                CALL_ANSWERED => {
                    let secs = call.ended.map(|(_, d, _)| d).unwrap_or(0);
                    let length = match secs {
                        0 => String::new(),
                        s if s < 60 => format!(", {s}s"),
                        s => format!(", {}m {}s", s / 60, s % 60),
                    };
                    if mine {
                        format!("You called{length}")
                    } else {
                        format!("{them} called{length}")
                    }
                }
                // Missed is **derived**, not recorded: a caller whose client
                // died posts no ending, and a reader that waited for one would
                // show the call ringing for ever.
                CALL_MISSED if mine => "You called, no answer".to_string(),
                CALL_MISSED => format!("Missed call from {them}"),
                CALL_DECLINED if mine => "Your call was declined".to_string(),
                CALL_DECLINED => format!("{them} declined"),
                CALL_CANCELLED if mine => "You cancelled the call".to_string(),
                CALL_CANCELLED => format!("{them} cancelled the call"),
                CALL_FAILED => "The call did not connect".to_string(),
                _ => format!("A call from {them}"),
            };
            events.push(Happened {
                seq: call.seq,
                at: call.posted,
                said,
                actor: call.account,
                subject: call.account,
                caveat: None,
            });
        }
        // Back into the exchange's own order: the calls were appended and the
        // transcript walks this list against the messages by sequence number.
        events.sort_by_key(|e| e.seq);
    }

    // Calls ringing anywhere, not only in the conversation on screen.
    //
    // Derived from the **log** and not from a signal: SIP-36 is explicit that
    // a durable outcome must not come from one, and `CallRecord::outcome`
    // derives `CALL_MISSED` once the ring window passes — so a call whose
    // caller crashed stops ringing on its own rather than for ever. The one
    // thing taken from a signal is `answered`, because answering writes no
    // entry and the log therefore cannot say it.
    let mut ringing: Vec<Ring> = Vec::new();
    for (channel, known) in &desk.channels {
        for call in known.timeline.calls() {
            if call.outcome(now).is_some() {
                continue;
            }
            ringing.push(Ring {
                channel: *channel,
                seq: call.seq,
                from: call.account,
                mine: call.account == me,
                secret: call.secret,
                answered: desk.answered.contains(&(*channel, call.seq)),
                label: known.label.clone(),
            });
        }
    }
    ringing.sort_by_key(|r| r.seq);

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

    // **Published only where it differs, and it says whether it did.**
    //
    // `send_modify` reports a change whether or not anything changed, and the
    // tick used to wake the interface unconditionally afterwards -- so an
    // account with nothing happening repainted the window 1.4 times a second
    // for ever, and each of those frames cloned the state several times over
    // and laid out every message in view. A field-by-field comparison is
    // cheaper than one frame of that by a wide margin.
    //
    // The caller wakes the interface when this returns true, and only then.
    state.send_if_modified(|s| {
        let mut moved = false;
        macro_rules! set {
            ($field:ident, $value:expr) => {
                if s.$field != $value {
                    s.$field = $value;
                    moved = true;
                }
            };
        }
        set!(link, link);
        set!(conversations, summaries);
        set!(lines, lines);
        set!(events, events);
        set!(earlier, earlier);
        set!(loading, loading);
        set!(synced, synced);
        set!(typing, typing);
        set!(trouble_with, trouble);
        set!(people, people);
        set!(mine, mine);
        set!(members, members);
        set!(i_am_admin, i_am_admin);
        set!(topic, topic);
        set!(ringing, ringing);
        moved
    })
}

/// How far one of our own messages is known to have got.
///
/// **Under-claims deliberately.** `Read` requires that *every* other member is
/// known to have read it, so one person who has not — or who has opted out of
/// receipts, and therefore reports no reading at all — holds the whole message
/// at `Delivered`. Claiming more than is known is the one direction that
/// cannot be corrected: somebody acts on "they have read it" and nothing ever
/// says otherwise.
fn receipt_for(known: &Known, seq: u64, me: &PubKey) -> Receipt {
    let others: Vec<&sqex_proto::channel::Mark> =
        known.marks.iter().filter(|m| m.account != *me).collect();
    if others.is_empty() {
        // Nobody else to have received it, or we have not asked yet. Either
        // way we know only that the exchange took it.
        return Receipt::Sent;
    }
    if others.iter().all(|m| m.read >= seq) {
        Receipt::Read
    } else if others.iter().all(|m| m.delivered >= seq) {
        Receipt::Delivered
    } else {
        Receipt::Sent
    }
}

/// A few words of a message, to name it in a reply.
fn stub(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 48 {
        let cut: String = flat.chars().take(47).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

/// The first characters of a key, where a whole one will not fit.
fn short(key: &PubKey) -> String {
    key.to_string().chars().take(8).collect()
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
                    // A direct message, and a direct message is never public.
                    public: Some(false),
                    group: false,
                    label: peer.to_string(),
                    admins: vec![chat.me, peer],
                    members: Vec::new(),
                    marks: Vec::new(),
                    timeline: Timeline::default(),
                    seen: 0,
                    wanted: PAGE,
                    last_at: 0,
                    unread: 0,
                    told: 0,
                    waiting,
                    typing: false,
                    fetched: false,
                    trouble: Trouble::default(),
                });
                if let Some(k) = desk.channels.get_mut(&channel) {
                    k.waiting = waiting;
                }
                open(desk, state, channel);
            }
            Err(e) => state.send_modify(|s| s.trouble = Some(e.to_string())),
        },
        Cmd::Show(channel) => {
            open(desk, state, channel);
            // **At once, from the disc.** `open` clears the transcript and
            // marks the channel for the next poll, and the poll is a round
            // trip: until this, opening a conversation showed an empty pane
            // for as long as the exchange took to answer -- on every open,
            // including the one sigil does for you on the way in. The history
            // is already folded and sitting in `desk`.
            let me = chat.me;
            let _ = publish(chat, state, desk, me);
        }
        Cmd::Refetch => {
            // Everything that failed, not one file: a fetch fails for reasons
            // that are rarely about the one blob — the link was down, the key
            // had not arrived — and a reader asking again means "try the lot".
            desk.unfetchable.clear();
        }
        Cmd::Earlier => {
            if let Some(channel) = desk.open
                && let Some(known) = desk.channels.get_mut(&channel)
            {
                known.wanted += PAGE;
                desk.dirty.insert(channel);
            }
        }
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
        Cmd::ClaimName(name) => {
            // The exchange's own outcome, in words. A refusal here is an
            // answer and not a fault: whether self-claim is offered at all is
            // the operator's policy, and "somebody else has it" and "not
            // here, ask an administrator" want opposite things from whoever
            // asked.
            match chat.claim_name(&name).await {
                Ok(outcome) => {
                    let said = match outcome {
                        sqex_proto::name::CLAIM_GRANTED => {
                            // Read back rather than assumed: the handle shown
                            // everywhere else comes from the exchange, and a
                            // client that wrote its own would be showing a
                            // name nobody else can see.
                            desk.restale.insert(chat.me);
                            format!("{name} is yours here.")
                        }
                        sqex_proto::name::CLAIM_TAKEN => {
                            format!("{name} is already somebody else's.")
                        }
                        sqex_proto::name::CLAIM_CLOSED => {
                            "This exchange assigns names itself. Ask its operator.".to_string()
                        }
                        sqex_proto::name::CLAIM_AT_CAPACITY => {
                            "You hold as many names here as this exchange allows.".to_string()
                        }
                        sqex_proto::name::CLAIM_RATE_LIMITED => {
                            "Too many claims just now. Try again shortly.".to_string()
                        }
                        sqex_proto::name::CLAIM_FULL => {
                            "This exchange is not taking any more names.".to_string()
                        }
                        other => format!(
                            "The exchange answered {other}, which this                                           version does not know."
                        ),
                    };
                    note(state, said);
                }
                Err(e) => state.send_modify(|s| s.trouble = Some(e.to_string())),
            }
        }
        Cmd::ReleaseName(name) => match chat.release_name(&name).await {
            Ok(()) => {
                // Read back rather than assumed, like the claim: the handle
                // shown everywhere else is the exchange's, and a client that
                // cleared its own would be hiding a name still resolving.
                desk.restale.insert(chat.me);
                note(state, format!("{name} is nobody's here now."));
            }
            Err(e) => trouble(state, e),
        },
        Cmd::Reconnect => chat.reconnect_now(),

        Cmd::React { target, emoji } => {
            let Some(channel) = desk.open else { return };
            // Whether this adds or takes back is decided from what we can see:
            // the fold keys reactions on (account, target, emoji), so asking
            // for the opposite of what is there is always the right request.
            let ours = desk
                .channels
                .get(&channel)
                .and_then(|k| k.timeline.messages().find(|m| m.seq == target))
                .map(|m| {
                    m.reactions
                        .get(&emoji)
                        .is_some_and(|who| who.contains(&chat.me))
                })
                .unwrap_or(false);
            match chat.react(&channel, target, &emoji, !ours).await {
                Ok(_) => desk.dirty.insert(channel),
                Err(e) => {
                    trouble(state, e);
                    false
                }
            };
        }
        Cmd::Reply { target, text } => {
            let Some(channel) = desk.open else { return };
            match chat.reply(&channel, target, &text).await {
                Ok(_) => desk.dirty.insert(channel),
                Err(e) => {
                    trouble(state, e);
                    false
                }
            };
        }
        Cmd::Edit { target, text } => {
            let Some(channel) = desk.open else { return };
            let post = sqex_proto::message::Post {
                parts: vec![sqex_proto::message::Part::Text(text)],
                ..Default::default()
            };
            match chat.edit(&channel, target, post).await {
                Ok(_) => desk.dirty.insert(channel),
                Err(e) => {
                    trouble(state, e);
                    false
                }
            };
        }
        Cmd::Redact(target) => {
            let Some(channel) = desk.open else { return };
            match chat.redact(&channel, target).await {
                Ok(redacted) => {
                    desk.dirty.insert(channel);
                    // A file the message carried may outlive it: the reference
                    // is detached, but somebody who already opened it holds the
                    // bytes and the key. Said rather than implied, because
                    // "deleted" reads as gone.
                    if !redacted.left_behind.is_empty() {
                        note(
                            state,
                            format!(
                                "Deleted. {} file(s) it carried could not be detached and may \
                                 still be reachable by anybody who already opened them.",
                                redacted.left_behind.len()
                            ),
                        );
                    } else {
                        note(state, "Deleted.".into());
                    }
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Typing(on) => {
            let Some(channel) = desk.open else { return };
            chat.typing(&channel, on).await;
        }

        Cmd::SetBlocked { who, blocked } => match chat.set_block(&who, blocked).await {
            Ok(()) => {
                refresh_blocked(chat, state).await;
                note(
                    state,
                    if blocked {
                        "Blocked. They are told nothing — though somebody watching their \
                         own delivery mark stop moving could work it out."
                            .into()
                    } else {
                        "Unblocked.".to_string()
                    },
                );
            }
            Err(e) => trouble(state, e),
        },
        Cmd::Blocked => refresh_blocked(chat, state).await,
        Cmd::OpenByName(name) => match chat.resolve_name(&name).await {
            Ok(who) => {
                Box::pin(apply(chat, Cmd::OpenDm(who), state, desk)).await;
            }
            Err(e) => trouble(state, e),
        },
        Cmd::Forward { seq, index, to } => {
            let Some(channel) = desk.open else { return };
            if to == channel {
                return trouble(state, "that is the conversation it is already in");
            }
            let Some(attachment) = desk
                .channels
                .get(&channel)
                .and_then(|k| k.timeline.messages().find(|m| m.seq == seq))
                .and_then(|m| m.post.attachments().nth(index).cloned())
            else {
                return trouble(state, "that file is no longer in the conversation");
            };
            // Attach first, then post. A message naming a blob the destination
            // has no claim on is a message its readers cannot fetch.
            if let Err(e) = chat.attach(&to, &attachment.blob).await {
                return trouble(state, e);
            }
            let post = sqex_proto::message::Post {
                parts: vec![sqex_proto::message::Part::Attachment(attachment)],
                ..Default::default()
            };
            match chat.send_post(&to, post).await {
                Ok(_) => {
                    desk.dirty.insert(to);
                    // Said plainly, because it is the consequence people miss:
                    // the key travels inside the message.
                    note(
                        state,
                        "Forwarded. Whoever is there can now open the file.".into(),
                    );
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Search(query) => {
            let needle = query.trim().to_lowercase();
            let mut hits = Vec::new();
            if !needle.is_empty() {
                for (channel, known) in &desk.channels {
                    for m in known.timeline.messages() {
                        let text = m.post.body_text().unwrap_or_default();
                        if m.redacted || !text.to_lowercase().contains(&needle) {
                            continue;
                        }
                        hits.push(Hit {
                            channel: *channel,
                            seq: m.seq,
                            label: known.label.clone(),
                            text: text.to_string(),
                            at: m.posted,
                        });
                    }
                }
                // Newest first: a search for a word said often wants the last
                // time, not the first.
                hits.sort_by_key(|h| std::cmp::Reverse(h.at));
            }
            state.send_modify(|s| {
                s.hits = hits;
                s.searched_messages = true;
            });
        }
        Cmd::Replicate { exchange, on } => {
            let Some(channel) = desk.open else { return };
            match chat.replicate(&channel, &exchange, on).await {
                Ok(()) => note(
                    state,
                    if on {
                        "That exchange may now carry a copy of this conversation. It cannot \
                         read it, and this cannot be taken back for what it already has."
                            .into()
                    } else {
                        "Withdrawn. It keeps whatever it already pulled.".to_string()
                    },
                ),
                Err(e) => trouble(state, e),
            }
        }

        Cmd::Devices => refresh_devices(chat, state).await,
        Cmd::LinkDevice { device, days } => {
            match chat.issue_credential(&device, days * 24 * 60 * 60) {
                Ok(credential) => {
                    let encoded = bs58::encode(credential.encode()).into_string();
                    state.send_modify(|s| s.credential = Some(encoded));
                    note(
                        state,
                        "Give this to the other device. It names both keys in the clear, \
                         so hand it over the way you would a key."
                            .into(),
                    );
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::RegisterSelf(encoded) => {
            let decoded = bs58::decode(encoded.trim())
                .into_vec()
                .map_err(|e| format!("that is not a credential: {e}"))
                .and_then(|raw| {
                    sqex_proto::credential::Credential::decode(&raw)
                        .map_err(|e| format!("that is not a credential: {e}"))
                });
            match decoded {
                Err(e) => trouble(state, e),
                Ok(credential) => match chat.register_self(&credential).await {
                    Ok(()) => {
                        refresh_devices(chat, state).await;
                        note(
                            state,
                            "This device acts for the account now. It holds no epoch keys \
                             yet — the other device has to hand them over before anything \
                             already said can be read here."
                                .into(),
                        );
                    }
                    Err(e) => trouble(state, e),
                },
            }
        }
        Cmd::RevokeDevice(device) => match chat.revoke_device(&device).await {
            Ok(()) => {
                refresh_devices(chat, state).await;
                note(
                    state,
                    "Revoked. Rotate the key in any conversation that device could read: \
                     it keeps everything it was already given."
                        .into(),
                );
            }
            Err(e) => trouble(state, e),
        },
        Cmd::ResealToSiblings => {
            let Some(channel) = desk.open else { return };
            match chat.reseal_to_siblings(&channel).await {
                Ok(0) => note(
                    state,
                    "Your other devices already hold this conversation's key.".into(),
                ),
                Ok(n) => note(state, format!("Handed the key to {n} of your devices.")),
                Err(e) => trouble(state, e),
            }
        }
        Cmd::RequestAdmission(label) => match chat.request_admission(&label).await {
            // The reply is identical whatever happens, on purpose: a route
            // that answered differently would be an oracle for whether an
            // account is admitted. So this reports that it was *sent*, and
            // claims nothing about what came of it.
            Ok(()) => note(
                state,
                "Asked. The exchange answers the same either way, so there is nothing \
                 here to watch — an administrator has to decide."
                    .into(),
            ),
            Err(e) => trouble(state, e),
        },

        Cmd::SendFile(path) => {
            let Some(channel) = desk.open else { return };
            // **Asked, never assumed.** SIP-18 says a client discovers the
            // chunk size from the exchange, and a client that guessed 256 KiB
            // against one on the uniform 64 KiB cap fails its first Put with
            // nothing explaining why.
            let limits = match chat.blob_limits().await {
                Ok(l) => l,
                Err(e) => return trouble(state, e),
            };
            let prepared = match chat.prepare_file(&path, limits.chunk as usize) {
                Ok(p) => p,
                Err(e) => return trouble(state, e),
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            note(state, format!("Sending {name}…"));
            let mut attachment = match chat.upload(&channel, &prepared).await {
                Ok(a) => a,
                Err(e) => return trouble(state, e),
            };
            // The field SIP-18 has always had and the terminal client always
            // left empty: "rendering one means decoding the image, and a
            // terminal client has nothing to show it on. The field exists for
            // a client that does." This is that client.
            //
            // It travels **inside the sealed message**, so it is no more
            // visible to the exchange than the picture is — and it is what a
            // reader sees before the blob has been fetched, or instead of it
            // when the blob is too big to fetch unasked.
            if attachment.effective_kind() == sqex_proto::blob::KIND_IMAGE {
                attachment.preview = thumbnail(&path).unwrap_or_default();
            }
            let post = sqex_proto::message::Post {
                parts: vec![sqex_proto::message::Part::Attachment(attachment)],
                ..Default::default()
            };
            match chat.send_post(&channel, post).await {
                Ok(_) => {
                    desk.dirty.insert(channel);
                    note(state, format!("Sent {name}."));
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::SaveFile { seq, index, to } => {
            let Some(channel) = desk.open else { return };
            let Some(attachment) = desk
                .channels
                .get(&channel)
                .and_then(|k| k.timeline.messages().find(|m| m.seq == seq))
                .and_then(|m| m.post.attachments().nth(index).cloned())
            else {
                return trouble(state, "that file is no longer in the conversation");
            };
            match chat.download(&attachment).await {
                Ok(bytes) => match std::fs::write(&to, &bytes) {
                    Ok(()) => note(state, format!("Saved to {}", to.display())),
                    Err(e) => trouble(state, format!("could not write {}: {e}", to.display())),
                },
                // A missing chunk is reported as retention rather than as a
                // failure, because that is usually what it is.
                Err(e) => trouble(state, e),
            }
        }
        Cmd::SetChannelAvatar(path) => {
            let Some(channel) = desk.open else { return };
            let attachment = match path {
                None => None,
                Some(path) => {
                    let limits = match chat.blob_limits().await {
                        Ok(l) => l,
                        Err(e) => return trouble(state, e),
                    };
                    let prepared = match chat.prepare_file(&path, limits.chunk as usize) {
                        Ok(p) => p,
                        Err(e) => return trouble(state, e),
                    };
                    match chat.upload(&channel, &prepared).await {
                        Ok(a) => Some(a),
                        Err(e) => return trouble(state, e),
                    }
                }
            };
            match chat.set_avatar(&channel, attachment).await {
                Ok(_) => desk.dirty.insert(channel),
                Err(e) => {
                    trouble(state, e);
                    false
                }
            };
        }

        Cmd::Call => {
            let Some(channel) = desk.open else { return };
            match chat.call(&channel, MEDIA_AUDIO, RING_SECS).await {
                Ok((posted, _secret)) => {
                    // The signal says it is ringing *now*; the entry is what
                    // says it happened. Both, because neither does the other's
                    // job.
                    chat.ring_state(&channel, posted.seq, RING_RINGING).await;
                    desk.dirty.insert(channel);
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Answer { channel, seq } => {
            // Signalled, not written: SIP-36 is right that a durable outcome
            // must not be derived from a signal, and taking a call is not an
            // outcome. The entry comes when it ends.
            chat.ring_state(&channel, seq, RING_ACCEPTED).await;
            desk.answered.insert((channel, seq));
            desk.dirty.insert(channel);
        }
        Cmd::Decline { channel, seq } => {
            chat.ring_state(&channel, seq, RING_DECLINED).await;
            // And the durable record, which the signal is not. A caller who
            // was not listening at that instant still learns it was refused.
            if let Err(e) = chat.end_call(&channel, seq, CALL_DECLINED, 0).await {
                trouble(state, e);
            }
            desk.dirty.insert(channel);
        }
        Cmd::Hangup {
            channel,
            seq,
            seconds,
        } => {
            chat.ring_state(&channel, seq, RING_ENDED).await;
            // Answered if anybody got as far as speaking, cancelled if the
            // caller gave up first. They are different facts about the call
            // and the log is where the difference survives.
            let outcome = if desk.answered.contains(&(channel, seq)) {
                CALL_ANSWERED
            } else {
                CALL_CANCELLED
            };
            if let Err(e) = chat.end_call(&channel, seq, outcome, seconds).await {
                trouble(state, e);
            }
            desk.answered.remove(&(channel, seq));
            desk.dirty.insert(channel);
        }

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
                    // The same reason as a channel the exchange stops listing:
                    // the list is folded from the store on the way in, so a
                    // conversation somebody left would be waiting for them at
                    // the next launch. Its messages stay where they are.
                    let _ = chat.store().forget_channel(&channel);
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
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    state.send_modify(|s| s.note = Some(Note { said, at }));
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
    // Where everybody else has got to, once, on arriving.
    desk.cursors_moved.insert(channel);
    // Back to one page. A channel somebody scrolled a long way into last week
    // should not cost that again today -- and **at least the unread run**,
    // because the divider marks where they stopped and a divider above the
    // first message drawn is a mark pointing off the top of the screen.
    if let Some(known) = desk.channels.get_mut(&channel) {
        known.wanted = PAGE.max(known.unread + 1);
    }
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

#[cfg(test)]
mod tick_tests {
    use super::{PER_TICK, this_tick};

    fn channels(n: u8) -> Vec<[u8; 32]> {
        (0..n).map(|i| [i; 32]).collect()
    }

    /// The conversation on screen is asked about first, however much else is
    /// waiting. Anything less and a busy account leaves the one being read
    /// until last -- or, past the bound, until some later tick.
    #[test]
    fn the_open_conversation_goes_first() {
        let (now, _) = this_tick(Some([9; 32]), channels(20));
        assert_eq!(now[0], [9; 32]);
    }

    /// Even when the events have already named it: once, not twice.
    #[test]
    fn the_open_conversation_is_asked_about_once() {
        let mut dirty = channels(3);
        dirty.push([9; 32]);
        let (now, later) = this_tick(Some([9; 32]), dirty);
        assert_eq!(now.iter().filter(|c| **c == [9; 32]).count(), 1);
        assert!(!later.contains(&[9; 32]));
    }

    /// A sweep is bounded, and what it leaves it hands back.
    ///
    /// Each of these is a round trip -- 100 to 250ms to a real exchange -- and
    /// they are made one after another, so an unbounded sweep is seconds
    /// during which nothing else can happen.
    #[test]
    fn a_tick_is_bounded_and_loses_nothing() {
        let all = channels(20);
        let (now, later) = this_tick(None, all.clone());
        assert_eq!(now.len(), PER_TICK);
        assert_eq!(now.len() + later.len(), all.len());
        for c in &all {
            assert!(
                now.contains(c) || later.contains(c),
                "a conversation was dropped rather than left for later"
            );
        }
    }

    /// Nothing to do is nothing to do.
    #[test]
    fn a_quiet_tick_asks_about_nothing() {
        let (now, later) = this_tick(None, Vec::new());
        assert!(now.is_empty() && later.is_empty());
    }
}

#[cfg(test)]
mod holding_tests {
    use super::{HOLD_BYTES, to_put_down};
    use std::collections::HashSet;

    fn blob(i: u8) -> [u8; 32] {
        [i; 32]
    }

    /// Past the budget, the oldest go, and only enough of them.
    #[test]
    fn the_oldest_are_put_down_and_no_more_than_needed() {
        let order: Vec<[u8; 32]> = (0..5).map(blob).collect();
        // Five of two megabytes is ten against a budget of six, so two go and
        // the third is not touched: six is inside six.
        let go = to_put_down(
            &order,
            |_| 2 * 1024 * 1024,
            &HashSet::new(),
            6 * 1024 * 1024,
        );
        assert_eq!(go, vec![blob(0), blob(1)]);
    }

    /// Nothing on screen is put down, whatever its age.
    ///
    /// Evicting a picture somebody is looking at fetches it again immediately,
    /// and the picture blinks -- so the cache would be doing work to save
    /// nothing.
    #[test]
    fn what_is_being_looked_at_stays() {
        let order: Vec<[u8; 32]> = (0..5).map(blob).collect();
        let on_screen: HashSet<[u8; 32]> = [blob(0), blob(1)].into_iter().collect();
        let go = to_put_down(&order, |_| 2 * 1024 * 1024, &on_screen, 6 * 1024 * 1024);
        assert_eq!(go, vec![blob(2), blob(3)]);
        assert!(go.iter().all(|b| !on_screen.contains(b)));
    }

    /// Inside the budget, nothing is put down at all.
    #[test]
    fn a_cache_that_fits_is_left_alone() {
        let order: Vec<[u8; 32]> = (0..3).map(blob).collect();
        assert!(to_put_down(&order, |_| 1024, &HashSet::new(), HOLD_BYTES).is_empty());
    }

    /// A budget smaller than what is on screen puts down everything else and
    /// then stops -- it does not loop, and it does not give up and clear.
    #[test]
    fn a_budget_smaller_than_the_screen_is_survivable() {
        let order: Vec<[u8; 32]> = (0..3).map(blob).collect();
        let on_screen: HashSet<[u8; 32]> = [blob(0)].into_iter().collect();
        let go = to_put_down(&order, |_| 1024, &on_screen, 0);
        assert_eq!(go, vec![blob(1), blob(2)]);
    }
}
