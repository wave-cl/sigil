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

use sqex_chat::client::{Chat, HomeSaid, Link};
use sqex_chat::store::{self, Store};
use sqex_proto::channel::{
    EVENT_ADDED, EVENT_CREATED, EVENT_DEMOTED, EVENT_JOINED, EVENT_LEFT, EVENT_MUTED,
    EVENT_PROMOTED, EVENT_REMOVED, EVENT_RENAMED, EVENT_REPLICATE, EVENT_RETENTION, EVENT_ROTATED,
    EVENT_SUCCEEDED, EVENT_UNMUTED, EVENT_UNREPLICATE, Role, Visibility,
};
use sqex_proto::events::Event;
use sqex_proto::message::{
    CALL_ANSWERED, CALL_CANCELLED, CALL_DECLINED, CALL_FAILED, CALL_MISSED, MEDIA_AUDIO,
    MEDIA_DIRECT, Part, RING_ACCEPTED, RING_DECLINED, RING_ENDED, RING_RINGING,
};
use sqex_proto::timeline::{Timeline, Verdict};
use sqnr_core::{PubKey, SoftwareSigner};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use sigil_net::{Dial, Endpoint};

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
    /// SIP-43: the exchange the sender says they posted this through, when
    /// that is not where the conversation lives; named by this machine's
    /// pin store, or by the head of its key.
    pub via: Option<String>,
    /// The emoji on this message, and who sent each: named here, where the
    /// people are known, rather than in the interface, which has only keys.
    pub reactions: Vec<sigil_ui::Reaction>,
    /// What this replies to.
    pub reply_to: Option<Quoted>,
    /// How far one of ours is known to have got. `None` on anybody else's.
    pub receipt: Option<Receipt>,
    /// Files this message carries.
    pub attachments: Vec<Attached>,
    /// What SIP-31 concluded about it. SIP-31 **requires** a fork be
    /// surfaced, and a message with nothing wrong says nothing.
    pub standing: Standing,
    /// Who it mentions, from the SIP-19 parts and not from the text: the
    /// part carries a key, and the name is what *this* client calls it.
    pub mentions: Vec<Mentioned>,
    /// One of those keys is ours.
    pub me_mentioned: bool,
    /// SIP-53 §Posting again: when the poster says they first said this,
    /// where a move stranded it and they posted it again.
    ///
    /// **Not what orders the log.** The entry's own `posted` stays `at`
    /// for everything structural -- the day it falls under, whether it
    /// groups with the one above -- because a time that runs backwards in
    /// a transcript is a transcript lying about order. This is what the
    /// message's own clock reads, which is what SIP-53 asks for. Ignored
    /// where it is later than the entry, as the spec says: nothing was
    /// first said after it was posted.
    pub said: Option<u64>,
    /// This message belongs to an **earlier copy** of the conversation
    /// (SIP-60 §The client keeps what it read), not to the one that is
    /// live. Its channel no longer exists, so there is nothing to reply
    /// to, react to, edit or take down -- and its sequence numbers mean
    /// nothing beside the live ones.
    pub earlier: bool,
}

impl Line {
    /// This message as a reply to it would quote it: who, a line of what,
    /// and the first picture's thumbnail. What the composer shows above the
    /// box while the reply is written, so it is the quote the reply will
    /// carry and not an approximation of one.
    pub fn quoted(&self) -> Quoted {
        let said = if self.redacted {
            "deleted".to_string()
        } else if !self.text.is_empty() {
            stub(&self.text)
        } else {
            only_files(self.attachments.iter().map(|a| a.kind))
        };
        let preview = (!self.redacted)
            .then(|| {
                self.attachments.iter().find(|a| {
                    (a.kind == sqex_proto::blob::KIND_IMAGE
                        || a.kind == sqex_proto::blob::KIND_VIDEO)
                        && !a.preview.is_empty()
                })
            })
            .flatten()
            .map(|a| Thumb {
                id: a.id.clone(),
                bytes: a.preview.clone(),
            });
        Quoted {
            seq: self.seq,
            who: self.name.clone().unwrap_or_else(|| short(&self.who)),
            said,
            preview,
        }
    }
}

/// What a reply quotes, as the fold finds it: who, a line of what, and the
/// thumbnail of the first picture if there is one.
type Stub = (PubKey, String, Option<Thumb>);

/// Somebody a message mentions: the key the part carries, and what to call
/// it -- ours to decide, and drawn with the key beside it (SIP-21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mentioned {
    pub key: PubKey,
    pub label: String,
}

/// A message from somebody else, arrived while this session was up: what
/// the interface announces. Derived from the log every pass, like [`Ring`],
/// so there is nothing to store and nothing to clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    pub channel: [u8; 32],
    pub seq: u64,
    pub from: PubKey,
    /// What we call them.
    pub from_label: String,
    /// What we call the conversation.
    pub conversation: String,
    pub public: bool,
    /// A conversation with one person, where the conversation *is* them.
    pub direct: bool,
    /// What they said, shortened -- or what they sent, for files alone.
    pub said: String,
    /// The conversation is the one on screen.
    pub in_open: bool,
    /// It names us.
    pub mentions_me: bool,
}

/// An arrival that names us. The same record; the name is what it was
/// called when only mentions were said out loud.
pub type Mention = Arrival;

/// Whether an event is a call, and whether anybody took it.
///
/// A call is the one thing in a transcript a reader may still act on --
/// ring back -- so it gets a clock beside it that a membership change does
/// not, and the one that went unanswered is the only one coloured. A
/// missed call used to read in the same muted grey as "Ada added Bram",
/// and in a column of them there was nothing to catch the eye.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// Answered, declined, cancelled, or placed from here and not taken.
    /// Something a reader may want the time of, but nothing is owed.
    Was,
    /// Somebody rang this account and nobody here answered.
    Missed,
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
    /// When this event is a call, which kind. `None` for everything else:
    /// a membership change wants no clock beside it.
    pub call: Option<Call>,
}

/// What a message replies to, as the reply shows it.
///
/// The author and the words are what is *drawn* -- "↳ 57" names a number
/// nobody has memorised. The sequence number is what a **click** goes to: the
/// quote is the way back to the message it quotes, and the transcript needs
/// its place in the channel to get there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quoted {
    /// The quoted message's place in the channel.
    pub seq: u64,
    /// Who said it, named.
    pub who: String,
    /// A line of what they said -- or, for a message that is only files,
    /// what it carries.
    pub said: String,
    /// The thumbnail of the first picture or clip it carries, if any: a
    /// quote of a picture shows the picture.
    pub preview: Option<Thumb>,
}

/// A thumbnail and the blob it is of.
///
/// **The id goes with the bytes** because the interface registers a picture
/// under a name, and egui keeps the first bytes given for a name: named by
/// the quoted message's number alone, message 12's picture in one
/// conversation was drawn on message 12's quote in every other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumb {
    pub id: String,
    pub bytes: std::sync::Arc<[u8]>,
}

impl Quoted {
    /// A reply to a message this reader does not hold -- before it joined,
    /// or pruned. Quoted as what it is rather than silently unthreaded:
    /// the reply *is* an answer to something, and pressing the quote goes
    /// looking for it.
    pub fn unheld(seq: u64) -> Quoted {
        Quoted {
            seq,
            who: String::new(),
            said: "an earlier message".to_string(),
            preview: None,
        }
    }
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
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

/// A message as the composer hands it over.
///
/// `mentions` are keys and nothing else: SIP-19's `Mention` part carries no
/// name, so whatever the sender typed beside the `@` is text like any other
/// and the reader calls the key what the reader calls it. `edit` rewrites
/// the message at that sequence number instead of posting a new one; an
/// edit with a `reply` keeps the reply.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Draft {
    pub text: String,
    pub reply: Option<u64>,
    pub edit: Option<u64>,
    pub mentions: Vec<PubKey>,
    /// Files to carry, uploaded on the way: pictures, clips, anything. No
    /// more than the wire's four in one message; the composer stops at
    /// that.
    pub files: Vec<std::path::PathBuf>,
    /// On a rewrite: which of the original's files stay on it, by blob id.
    /// A rewrite is a whole post, so one left out here is taken off the
    /// message. The composer always fills this from what it showed.
    pub keep: Vec<String>,
    /// The composer's own number for this draft, answered in
    /// [`ChatState::posted`] so the composer can tell which of its sends
    /// failed and put that one back. Zero for a draft nobody is waiting on.
    pub token: u64,
}

/// Whether a message posted at `posted` can still be rewritten at `now`.
///
/// SIP-19 gives a rewrite a day: past that, every reader -- the sender's own
/// included -- drops it on the floor. The interface offers Edit by this, and
/// the session refuses by it, so a rewrite that cannot land is never sent.
pub fn rewritable(posted: u64, now: u64) -> bool {
    now.saturating_sub(posted) <= sqex_proto::message::EDIT_WINDOW
}

/// What became of the last [`Cmd::Post`]: which draft, and why it did not
/// go, if it did not.
///
/// **The composer's, not the reader's.** `trouble` says what is wrong with
/// the conversation and is rebuilt by every refresh; this says what happened
/// to one message somebody wrote, and stays until the next one is sent, so
/// the interface can put the words back in the box rather than lose them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posted {
    pub token: u64,
    pub trouble: Option<String>,
}

impl Draft {
    /// Just words.
    pub fn text(text: impl Into<String>) -> Draft {
        Draft {
            text: text.into(),
            ..Default::default()
        }
    }

    /// The SIP-19 parts, in the order a reader expects them: the text, the
    /// files as `attachments` (already uploaded), the reply if any, then
    /// each mention once, no more than the wire allows. A message that is
    /// only files carries no text part: an empty one is not a message.
    pub fn parts(&self) -> Vec<Part> {
        self.parts_with(Vec::new())
    }

    pub fn parts_with(&self, attachments: Vec<sqex_proto::blob::Attachment>) -> Vec<Part> {
        let mut parts = Vec::new();
        if !self.text.is_empty() || attachments.is_empty() {
            parts.push(Part::Text(self.text.clone()));
        }
        parts.extend(attachments.into_iter().map(Part::Attachment));
        if let Some(target) = self.reply {
            parts.push(Part::Reply(target));
        }
        let mut seen = std::collections::HashSet::new();
        parts.extend(
            self.mentions
                .iter()
                .filter(|k| seen.insert(**k))
                .take(sqex_proto::message::MAX_MENTIONS)
                .map(|k| Part::Mention(*k)),
        );
        parts
    }
}

/// One conversation in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub channel: [u8; 32],
    /// The other party, for a direct message.
    pub peer: Option<PubKey>,
    pub label: String,
    pub unread: usize,
    /// How many of the unread mention us.
    pub mentioned: usize,
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
    /// SIP-16: the channel's picture, as the bytes that draw it.
    ///
    /// **The preview, not the blob.** An attachment carries a small copy
    /// inline -- the same bytes a message thumbnail is drawn from -- so a
    /// picture costs no fetch and no second lifetime rule. At the size a
    /// row draws a mark, the preview is the whole of what is needed.
    ///
    /// Only a room has one. A direct message draws the other party, whose
    /// picture is their profile's (SIP-21) and not this.
    pub avatar: Option<Vec<u8>>,
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
    /// SIP-85: the home this connection is carried through, when it is --
    /// the domain the roster named, for the switcher to say "via" by. The
    /// exchange sees the home's address; and no introduction (SIP-25) may be
    /// asked for on this connection, which is what a call checks here.
    pub carried: Option<String>,
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
    /// SIP-59: this exchange told the session, on start, that the account
    /// lives at another one -- the key and its domain -- and hands its
    /// services off there. The session is **parked**: nothing more is asked
    /// of this exchange, and the interface starts it again on a long clock
    /// (an account can move back), not every three seconds. The first
    /// version restarted such a session every three seconds and minted
    /// sixty-five prekeys each time, for a week (2026-09-22).
    pub moved_to: Option<(PubKey, String)>,
    /// Whether [`ChatState::devices`] is the exchange's answer or just a
    /// vector nobody has filled yet.
    ///
    /// **An empty list and an unanswered one looked identical**, and the
    /// card drew its "nothing else is linked" warning on `len() <= 1` --
    /// which is exactly as true of a fetch that failed as of an account
    /// with one device. A refusal, an exchange that could not be reached,
    /// a card opened a moment too early: each of them rendered as a
    /// confident claim about the account.
    pub devices_known: bool,
    /// Why a join from the public-channels pane was refused, if one was.
    ///
    /// **Its own field, because the pane drew the general `trouble`.** That
    /// field is set by *any* failing command and cleared by none, so a call
    /// that could not be placed hours earlier was still being drawn in red
    /// above a directory listing that had just answered perfectly -- where
    /// it reads as "this pane is not connected", which it was not.
    pub join_trouble: Option<String>,
    /// SIP-24: this exchange does not admit this account, named as this
    /// client reaches it.
    ///
    /// An exchange running a whitelist refuses every gated route with
    /// `NotWhitelisted`, so the session cannot do anything at all -- and
    /// the one route left open is `/admission/request`, which is the way
    /// in. Without this the refusal arrived as an ordinary error string and
    /// retried for ever, and the whole of SIP-24 had nothing that asked.
    pub not_admitted: Option<String>,
    /// Why the device list could not be fetched, if it could not.
    ///
    /// Its own field rather than the general `trouble`, because this card
    /// has to choose between three sentences -- asking, could not ask, and
    /// nothing else is linked -- and the general one cannot tell it which.
    pub devices_trouble: Option<String>,
    /// SIP-23: one-time prekeys this exchange still holds for **this**
    /// device, as it counted them at the last catch-up.
    ///
    /// `None` until an exchange has said, which is not the same fact as
    /// zero: zero is a drained pool, and not knowing is a client that has
    /// not asked yet. The number arrived on every catch-up from the day
    /// catch-up existed and went straight into a log line, so a pool that
    /// had run dry was invisible here while being perfectly visible to the
    /// exchange serving the fallback in its place.
    pub prekeys: Option<u16>,
    /// How many conversations this session has folded from the disc.
    ///
    /// A conversation is folded when something needs it -- opened, polled,
    /// searched -- and listing them needs none. That is the whole of the
    /// difference between a window that draws at once and one that reads
    /// every message on the disc first, and nothing else about it is
    /// observable, so the number is published rather than logged.
    pub folds: usize,
    /// Up, retrying, or gone. Drawn with the *word* beside the colour: a
    /// colour on its own is not a message.
    pub link: LinkState,
    pub trouble: Option<String>,
    /// What became of the last message sent from the composer; see
    /// [`Posted`].
    pub posted: Option<Posted>,
    pub conversations: Vec<Summary>,
    /// Which conversation is on screen, and what is in it.
    pub open: Option<[u8; 32]>,
    pub lines: Vec<Line>,
    /// SIP-59: where the open direct message's peer's account lives, when
    /// this exchange knows. **Not the domain their handle is registered
    /// at** — a name is bound per exchange and somebody may hold one here
    /// as an alias while living somewhere else, which is exactly the case
    /// that decides whether a call needs SIP-39's bridge.
    pub peer_home: Option<(PubKey, String)>,
    /// SIP-53 §Posting again: this client's own posts that a move stranded
    /// in the open conversation, oldest first. Offered above the composer,
    /// one at a time.
    pub stranded: Vec<Stranded>,
    /// Earlier copies of the open conversation, oldest first, each whole
    /// (SIP-60 §The client keeps what it read). Drawn above `lines` with a
    /// divider, and never merged into them.
    pub copies: Vec<Vec<Line>>,
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
    /// Whether the people you talk to are there, by SIP-4 beacon: the
    /// other party of every direct message and the members of the open
    /// conversation. Absent is not known.
    pub presence: HashMap<PubKey, crate::presence::Presence>,
    /// The exchanges this one federates with (SIP-39 §The peer directory): each by key and,
    /// where recorded, the domain it is reached by. A hint; sigil reaches
    /// one only by discovering its domain and refusing a different key.
    pub peers: Vec<(PubKey, String)>,
    /// The keys this person verified (SIP-41): their safety words were
    /// compared with their owner. By key, with when. From this machine's
    /// store and nowhere else.
    pub verified: HashMap<PubKey, u64>,
    /// SIP-27: who else has said, at this exchange, that they compared safety
    /// words with a key -- the statements sigil itself lodges with `Attest`,
    /// read back for whoever is being verified. By subject, the issuers whose
    /// statements verify and are still standing, this identity's own left
    /// out. **Others' word, to be read and not acted on**: the dialog says so.
    pub attested: HashMap<PubKey, Vec<PubKey>>,
    /// SIP-44: accounts the registry says were succeeded, and by whom --
    /// asked for the other party when a direct message opens. A transcript
    /// says so only where the move was written into a channel both are in;
    /// the registry is what knows for a contact who moved while nothing was
    /// said. `None` is the registry's answer that they were not.
    pub succeeded: HashMap<PubKey, Option<PubKey>>,
    /// SIP-45: what came of leaving the wake endpoint with this exchange --
    /// "registered", "forgotten", "not offered here", or the refusal. A
    /// readout, since a phone that quietly failed to register would go
    /// unwoken with nothing to say why.
    pub wake: Option<String>,
    /// Whether we may rename, invite, remove and rotate here.
    pub i_am_admin: bool,
    /// SIP-56: the open conversation's reports, as last loaded (admins).
    pub reports: Vec<Report>,
    /// SIP-56: reports the exchange has announced since the reports were
    /// last read here, for a badge; cleared by a load.
    pub reports_pending: usize,
    /// The open conversation's topic, when it has one.
    pub topic: String,
    /// SIP-43: where the open conversation lives, when that is not the
    /// exchange this session is connected to -- the origin's key and, when
    /// the operator recorded one, the domain it is reached by. Posts made
    /// here are carried there and ordered there.
    pub home: Option<(PubKey, String)>,
    /// Every device registered to this account.
    ///
    /// **First class, not buried.** An epoch key arrives sealed against a
    /// one-time prekey and opening it spends the prekey, so the copy on this
    /// disk is the only one that will ever exist — and a linked device is the
    /// only backup of it there can be. Losing this store with no second device
    /// loses those conversations permanently, for everybody in them.
    pub devices: Vec<Linked>,
    /// SIP-48: what the exchange holds of this account's sealed backup, and
    /// whether this store has the key to write one. `None` until asked.
    pub backup: Option<Backup>,
    /// SIP-44: what has been arranged for the day this key is lost, and what
    /// was just written. `None` until asked.
    pub succession: Option<Succession>,
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
    /// SIP-39: a call another exchange carried here, ringing now.
    ///
    /// Not a [`Ring`]: there is no conversation, no invitation and no room
    /// secret, only who is calling and the bridge our exchange is holding
    /// for them. Answering opens a session back toward the caller here and
    /// our exchange matches it to the bridge; declining tells them so.
    /// One at a time, because one is what a bridge can carry, and the
    /// interface has one place to ring.
    pub cross_ring: Option<CrossRing>,
    /// Calls that have ended, by conversation and ring: the other side's
    /// hangup is an entry in the channel, and a call this window is still
    /// in that the channel says is over is one to leave. Without this the
    /// window stayed "in a call" the other side had left, until the path
    /// itself timed out.
    pub over: Vec<([u8; 32], u64)>,
    /// Messages mentioning us that arrived while this session was up, in
    /// every conversation. See [`Mention`].
    /// Every message from somebody else that arrived live; the ones that
    /// name us are the mentions.
    pub arrivals: Vec<Arrival>,
    /// Messages from others this device did not hold when the session
    /// started -- live or not. See `Known::held_from`.
    pub unseen: Vec<Arrival>,
    /// The first message that was unread when this conversation was opened.
    ///
    /// **Frozen on entry.** Reading advances the read mark, so a divider that
    /// tracked it would disappear exactly when somebody wanted to see where
    /// they had got to.
    pub divider: Option<u64>,
    /// How many there were, for the divider's label. Frozen with it.
    pub unread_on_open: usize,
}

impl ChatState {
    /// Whether the conversation on screen is a direct message.
    ///
    /// **One answer, because five copies of it disagreed.** A direct
    /// message is a conversation whose identifier stands for two accounts,
    /// which is what `peer` records. The fold below was written out
    /// independently at every place that needed it, and the places that
    /// needed it are the places that decide whether a *room's* rules apply
    /// -- naming, moderation, who may take a message down. One of them
    /// reached for `i_am_admin` instead and offered one party of a direct
    /// message a moderator's powers over the other, on the strength of a
    /// comment claiming direct messages have no admins. They have two.
    pub fn open_is_direct(&self) -> bool {
        self.conversations
            .iter()
            .find(|c| Some(c.channel) == self.open)
            .is_some_and(|c| c.peer.is_some())
    }
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
    /// Who said it, as the transcript would name them; "You" for our own.
    pub who: String,
    pub text: String,
    /// Where in `text` the first match is, so the result can show that part
    /// of the message and mark the word. Always a real span: the same
    /// comparison decides the hit and places this.
    pub found: std::ops::Range<usize>,
    pub at: u64,
}

/// One device registered to this account.
/// SIP-48: the account's backup as the exchange and this store see it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Backup {
    /// This store holds a backup key, so it can write one.
    pub has_key: bool,
    /// What the exchange holds, when it holds anything.
    pub held: Option<HeldBackup>,
    pub used: u64,
    pub quota: u64,
    /// The 24 words, while shown. Cleared by `Cmd::HideBackupKey`; never
    /// kept in the state longer than the person asked to see them.
    pub words: Option<Vec<String>>,
}

/// SIP-44: what is arranged for the day this key is lost.
///
/// Two ways, both signed by the account while it still can: a **will** names
/// a successor key, presented by that key when this one is gone; **guardians**
/// are people named with a number of them it takes, who each sign for the
/// successor later. The exchange holds the guardians' policy so they and the
/// successor can find it when the account cannot be asked; a will is held by
/// nobody but whoever the account gave it to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Succession {
    /// Whether this client *is* the account, and so may write a will or name
    /// guardians. A linked device holds the account's credential and not its
    /// seed, and a will it signed would be its own.
    pub is_account: bool,
    /// The policy lodged for this account at the exchange, if any: how many
    /// guardians it takes, and who they are.
    pub lodged: Option<(u8, Vec<PubKey>)>,
    /// A will just written, base58, while shown. Kept apart from the
    /// successor's secret; together they are the account.
    pub will: Option<String>,
    /// A vouch just signed as a guardian, base58, while shown.
    pub vouch: Option<String>,
}

/// SIP-48: one backup, as the exchange describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldBackup {
    pub generation: u64,
    pub written: u64,
    pub device: PubKey,
}

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

/// SIP-53 §Posting again: one of this client's own posts that a move
/// stranded, waiting to be sent again or let go.
///
/// **Offered, never resent.** "Each stranded post is offered to the person,
/// in the order it was first posted, and sent again only on their say: it
/// is a new entry, and the person may have said it since, or no longer mean
/// it." So this is a question the interface asks, one at a time, and not a
/// queue it drains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stranded {
    /// The stranded entry's sequence number, in the losing regime. The
    /// handle for sending it again or letting it go, and nothing else: it
    /// numbers a position in an ordering the members did not choose.
    pub seq: u64,
    /// When it was first posted -- what travels with it as SIP-53's `Said`
    /// if it is sent again.
    pub posted: u64,
    pub text: String,
    /// How many files went with it, so the offer can say what it is when
    /// there are no words to show.
    pub files: usize,
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
    /// How long, for a video or a voice note; what the sender said.
    pub duration_ms: Option<u64>,
    /// Width and height, for a picture or a video; what the sender said.
    pub shape: Option<(u32, u32)>,
    /// SIP-18: a voice note's waveform, one level per bar, **so it draws
    /// before any audio is fetched**. SIP-15's scale: half a decibel below
    /// full scale per unit, 255 for digital silence -- the same numbers a
    /// live call's meter uses, which is why the spec reuses them.
    pub waveform: std::sync::Arc<[u8]>,
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
    /// Too big to fetch unasked, and nobody has asked. What is on screen is
    /// the thumbnail, and it stays the thumbnail until somebody presses Fetch
    /// -- which the picture had better say, because "fetching" over a fetch
    /// that will never start is a lie a reader waits on.
    pub held: bool,
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

/// How long a SIP-39 ring is shown for, nobody having answered or refused
/// it. See [`ChatState::cross_ring`].
const CROSS_RING_FOR: std::time::Duration = std::time::Duration::from_secs(60);

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
    /// The caller said it will ask for a SIP-25 introduction (SIP-36's
    /// `MEDIA_DIRECT`), so a willing callee asks too.
    pub direct: bool,
    /// The other person, when this is a direct message -- who a direct
    /// connection would be made to. `None` in a group, which is relayed.
    pub peer: Option<PubKey>,
}

/// SIP-39: somebody at another exchange is calling, and this is what their
/// exchange told ours. See [`ChatState::cross_ring`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossRing {
    /// What our exchange is holding the call open under, for the answer or
    /// the refusal.
    pub bridge: [u8; 16],
    /// Who is calling, as their exchange named them.
    pub caller: PubKey,
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
    /// SIP-16 §Federated directory: where the room lives, as the exchange
    /// names it; empty when it lives at the exchange that answered.
    pub domain: String,
    /// Whether the exchange that answered holds it (its own, or a copy) --
    /// joinable from here -- or only lists it, to be joined at `domain`.
    pub here: bool,
}

/// Who is in a conversation, and what they may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub account: PubKey,
    /// May redact, rename, invite and mint a new epoch. **Attested by the
    /// exchange**, unlike a SIP-21 title, which is why it may be shown as a
    /// role and a title may not.
    pub admin: bool,
    /// SIP-56: muted by an admin -- reads, and may not write. Read off the
    /// exchange's own signed mute/unmute entries in the transcript, since the
    /// roster carries no such flag; a mute older than what has been fetched
    /// is not seen here, and the exchange refuses the posts regardless.
    pub muted: bool,
}

/// SIP-56: one report, as the exchange holds it for the admins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub id: u64,
    pub reporter: PubKey,
    /// The entry reported, or 0 for the room itself.
    pub target: u64,
    pub reason: &'static str,
    pub at: u64,
    /// Stored in the clear at the exchange.
    pub note: String,
}

/// SIP-56's four reasons, as the wire numbers them.
pub const REASONS: [(u8, &str); 4] = [(1, "spam"), (2, "harassment"), (3, "illegal"), (4, "other")];

fn reason_word(reason: u8) -> &'static str {
    REASONS
        .iter()
        .find(|(n, _)| *n == reason)
        .map(|(_, w)| *w)
        .unwrap_or("other")
}

/// How somebody can be named.
///
/// **None of this is attested.** A SIP-21 profile is self-declared: the
/// exchange stores it and vouches for none of it. A handle is bound at the
/// exchange, so it says more, but it still is not the person. The key is the
/// only thing that identifies somebody, which is why every view that shows a
/// name keeps the key one gesture away.
///
/// A picture somebody published, and a hash of it (SIP-21).
///
/// **Bytes, but shared and hashed**, because of where this rides: `Person`
/// lives in `ChatState`, and `ChatApp::state_of` clones the whole state
/// several times per render pass. An `Arc` makes that clone a refcount bump
/// instead of every avatar in the account memcpy'd at 60 Hz.
///
/// The hash is what a texture cache compares, so a picture that *changed*
/// is redecoded and the same picture never is -- keyed by account alone, a
/// cache shows the old face for as long as the window is open, and keyed by
/// the bytes it compares kilobytes every frame.
///
/// Carries SIP-21's warning: a picture is chosen by its subject and is
/// evidence of nothing. Two accounts may publish the same face.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Face {
    pub hash: u64,
    pub bytes: std::sync::Arc<Vec<u8>>,
}

impl Face {
    fn of(bytes: Vec<u8>) -> Option<Face> {
        if bytes.is_empty() {
            return None;
        }
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        Some(Face {
            hash: h.finish(),
            bytes: std::sync::Arc::new(bytes),
        })
    }
}

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
    /// The picture they published, undecoded. `None` is "no picture" and
    /// "no profile yet", which draw the same thing: the identicon.
    pub picture: Option<Face>,
}

impl Person {
    /// What they are called, when anything calls them anything.
    ///
    /// `None` is **not** "they have no name". A withheld profile, an absent
    /// one and a blocked one answer identically by design, so this says only
    /// that we cannot name them.
    pub fn named(&self) -> Option<String> {
        self.name.clone().or_else(|| self.handle.clone())
    }

    /// What to call them, falling back to the key, which is never wrong.
    ///
    /// **Shortened**, which it did not used to be. A whole base58 key is 41 to
    /// 44 characters, and where a name goes that is a row of noise wide enough
    /// to push the time and the unread count off the end of it -- which is
    /// what the conversation list, the ring banner and the identity header all
    /// did for anybody who had not published a name.
    ///
    /// A short key rather than "unknown", because it is *true* and because two
    /// people nobody can name still have to be told apart. It is not enough to
    /// identify somebody on its own: the whole key is a click away in Members,
    /// which every conversation's header opens.
    pub fn label(&self, key: &PubKey) -> String {
        self.named().unwrap_or_else(|| short(key))
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
    /// Those of the unreadable this identity may delete: its own, and all of
    /// them if it administers the channel -- which is the exchange's rule for
    /// a redaction, so an offer here is one the exchange will honour.
    ///
    /// **Why an unreadable message needs deleting at all.** One that will
    /// never open is not always waiting for a key. Two pictures were posted
    /// to a public channel with previews over SIP-18's cap, and every client
    /// refused them, for ever, as "not opened yet". The one thing to do with
    /// such a message is take it down, and the only party who can is its
    /// author -- who is exactly who is looking at the notice.
    pub redactable: Vec<u64>,
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
    /// SIP-43 §The heads by position: the exchange's record of what *this
    /// device* wrote here disagrees with this device's own.
    ///
    /// Not a lag: the exchange is asked about the one position this device
    /// last signed, and this is set only when it holds a different head for
    /// that same position. Either this store has been rolled back -- a
    /// restore, a copy of a file, two machines sharing one device key --
    /// or something was written here under this key that this machine did
    /// not write.
    pub chain_apart: bool,
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
mod draft_tests {
    use super::*;

    fn k(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    /// A rewrite lands for a day and not a second longer -- the same edge
    /// the reader's fold uses, so what is offered is what will be taken.
    #[test]
    fn a_rewrite_is_possible_for_exactly_the_window() {
        let day = sqex_proto::message::EDIT_WINDOW;
        assert!(rewritable(1_000, 1_000));
        assert!(rewritable(1_000, 1_000 + day));
        assert!(!rewritable(1_000, 1_000 + day + 1));
        assert!(rewritable(2_000, 1_000), "a clock behind the exchange's");
    }

    /// Text first, the reply if any, then each mention once: the order a
    /// reader expects, and no more mentions than the wire allows.
    #[test]
    fn a_drafts_parts_are_text_reply_then_each_mention_once() {
        let plain = Draft::text("hi");
        assert!(matches!(plain.parts().as_slice(), [Part::Text(t)] if t == "hi"));

        let full = Draft {
            text: "@Ada @Bram look".into(),
            reply: Some(7),
            edit: None,
            mentions: vec![k(1), k(2), k(1)],
            files: Vec::new(),
            keep: Vec::new(),
            token: 0,
        };
        let parts = full.parts();
        assert!(matches!(&parts[0], Part::Text(t) if t == "@Ada @Bram look"));
        assert!(matches!(parts[1], Part::Reply(7)));
        assert!(matches!(parts[2], Part::Mention(m) if m == k(1)));
        assert!(matches!(parts[3], Part::Mention(m) if m == k(2)));
        assert_eq!(parts.len(), 4, "a key picked twice is one part");

        let many = Draft {
            mentions: (0..40).map(k).collect(),
            ..Draft::text("all of you")
        };
        assert_eq!(
            many.parts().len(),
            1 + sqex_proto::message::MAX_MENTIONS,
            "no more than the wire allows"
        );
        // And a post of those parts is one the wire accepts.
        let post = sqex_proto::message::Post {
            parts: many.parts(),
            ..Default::default()
        };
        post.validate().expect("a valid post");
    }

    /// Files go after the words and before the rest; a message that is only
    /// files carries no empty text part, and one with words and files
    /// carries both.
    #[test]
    fn a_drafts_files_follow_its_words_and_stand_alone_without_them() {
        let one = sqex_proto::blob::Attachment {
            kind: sqex_proto::blob::KIND_IMAGE,
            blob: [1u8; 32],
            key: [2u8; 32],
            size: 3,
            chunks: 1,
            mime: "image/png".into(),
            meta: Vec::new(),
            preview: Vec::new(),
        };
        let silent = Draft::default().parts_with(vec![one.clone(), one.clone()]);
        assert_eq!(silent.len(), 2, "no empty text part: {silent:?}");
        assert!(silent.iter().all(|p| matches!(p, Part::Attachment(_))));
        let spoken = Draft {
            reply: Some(3),
            mentions: vec![k(1)],
            ..Draft::text("look")
        }
        .parts_with(vec![one]);
        assert!(matches!(&spoken[0], Part::Text(t) if t == "look"));
        assert!(matches!(spoken[1], Part::Attachment(_)));
        assert!(matches!(spoken[2], Part::Reply(3)));
        assert!(matches!(spoken[3], Part::Mention(_)));
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
    /// Fetch one file that was too big to fetch unasked: the one at `index`
    /// on the message at `seq` in the open conversation.
    Fetch {
        seq: u64,
        index: usize,
    },
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
    /// SIP-59: where a peer's account lives, asked once and cached.
    ///
    /// **Its own command, not part of opening.** It was a round trip inside
    /// `Show`'s handler, which runs on the session's own loop on the way
    /// in — so opening a conversation waited on a lookup that opening a
    /// conversation does not need. Whether a call to them wants SIP-39's
    /// bridge is not urgent; the chat list is.
    PeerHome(PubKey),
    /// SIP-17: ask for an epoch key this device was not sent.
    ///
    /// Asks the exchange for an envelope sealed to this device, and where
    /// none has been left and this account may mint one, mints the next
    /// epoch and seals it to everybody present. In a direct message both
    /// parties may, which is what makes this the way out of being
    /// stranded there.
    AskForKey,
    /// SIP-53 §Posting again: send one of this client's stranded posts
    /// again, as a new entry saying when it was first said.
    PostAgain(u64),
    /// SIP-53 §Posting again: let a stranded post go unsent.
    ForgetStranded(u64),
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
    /// SIP-56: mute somebody -- they read, and may not write -- or unmute
    /// them. An admin's signed entry, like a removal, without the rotation.
    Mute {
        who: PubKey,
        on: bool,
    },
    /// SIP-56: report an entry (`target`, or 0 for the room) of the open
    /// conversation to its admins. The note is stored in the clear at the
    /// exchange; the admins see who reported, nobody else does.
    Report {
        target: u64,
        reason: u8,
        note: String,
    },
    /// SIP-56: read the open conversation's reports (admins).
    LoadReports,
    /// SIP-56: dismiss a report by id (admins).
    Dismiss(u64),
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
    /// What the composer sends: text, and what goes with it -- a reply, an
    /// edit, the people it mentions. One shape for every message the
    /// composer makes, because the parts a message carries are one list
    /// (SIP-19) and a variant per combination was a variant per
    /// combination.
    Post(Draft),
    /// Remove a message's body. The entry stays, and the gap is the record.
    Redact(u64),
    /// Say we are typing, or have stopped. Best effort, ephemeral and
    /// forgeable, like every signal.
    Typing(bool),

    // ---- calls (SIP-36) -------------------------------------------------
    /// Ring everybody in the open conversation. `direct`: say in the
    /// invitation that this side will ask the exchange for a SIP-25
    /// introduction once answered, so the call can go straight between the
    /// two people (SIP-36's `MEDIA_DIRECT`).
    Call {
        direct: bool,
    },
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
    /// SIP-44: what is arranged for this account's succession. Asked with
    /// `BackupStatus`, which is the same screen.
    SuccessionStatus,
    /// SIP-44 §The will: sign that `successor` may take this account.
    WriteWill(PubKey),
    /// SIP-44 §Guardians: name them and how many it takes, and lodge it.
    NameGuardians {
        threshold: u8,
        guardians: Vec<PubKey>,
    },
    /// SIP-44 §Guardians, as one of them: sign that `successor` succeeds
    /// `account`.
    Vouch {
        account: PubKey,
        successor: PubKey,
    },
    /// SIP-44 §The successor: take an account. `proof` is what was pasted --
    /// a will, base58; or an account's key and then the guardians' vouches,
    /// base58, one per line. Which it is is decided by shape.
    Succeed(String),
    /// SIP-44 §The handover: change this account's key while the old one is
    /// still held. A new key is made here, the will is signed under the old
    /// one, and a credential is issued from the new key for every device
    /// this account has — presented together, so the exchange carries the
    /// names, the conversations and the devices across in one step.
    ///
    /// Not undoable: after it, the old key is not the account.
    HandOver,
    /// Put away a will or a vouch that was shown.
    HideSuccession,
    /// SIP-39: the cross-exchange ring was answered or refused. Both happen
    /// on the interface's own connection -- the answer is a call of its
    /// own, the refusal a post to the bridge -- so all the session has to
    /// do is stop ringing.
    CrossRingHandled,
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
    /// SIP-48: what the exchange holds of this account's backup, and whether
    /// this store can write one.
    BackupStatus,
    /// SIP-48: show the backup key as 24 words, making one if there is none.
    BackupKey,
    /// Take the words off the state.
    HideBackupKey,
    /// SIP-48: write the store to the exchange now, sealed under the key.
    BackupNow,
    /// SIP-48: take the account's backup into this store, with the words.
    /// Merges: nothing here is removed.
    Restore {
        words: String,
        /// The identity file: the exchange restored from is recorded beside
        /// it as the home (SIP-60), since a backup lives at the home.
        identity: Option<std::path::PathBuf>,
    },
    /// SIP-48: release what the exchange holds. What is here stays.
    DropBackup,
    /// Write a credential for another device to register itself with.
    ///
    /// The credential names both keys in the clear to whoever holds it, and it
    /// is **evidence, not authority**: it says which account vouches for a
    /// key, and entitles that key to nothing on its own.
    LinkDevice {
        device: PubKey,
        days: u64,
    },
    /// SIP-47 §Pairing, step 3, from the phone's side: another device of
    /// `owner` registered this one and showed where to go. This client finds
    /// itself in the account's list, with the credential that device
    /// presented, and only then treats the account as its own. `owner` is a
    /// key, or a SIP-38 name resolved at this exchange.
    ClaimAccount(String),
    /// Withdraw a device.
    ///
    /// **The revocation outlives the credential**, and has to: everything
    /// needed to register is on the stolen machine, so deleting the mapping
    /// alone would be undone by one request. A found device comes back only
    /// with a credential the account signed *after* the revocation — the one
    /// thing that was never on it.
    RevokeDevice(PubKey),
    /// SIP-22: sign *this* client out, locally. The exchange records it on
    /// its own authority — a device holds no account key, so there is no
    /// artifact to repeat. For a device you have lost, revoke it instead.
    SignOutDevice(PubKey),
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
    /// SIP-60: open a conversation with somebody at **another** exchange,
    /// `label@domain` (a key or a SIP-38 name at that domain). Only from the
    /// home session: reaching out is carried by the home after this
    /// identity's Move, which is presented here on the first reach if it is
    /// not already on record -- and never from an added exchange, whose
    /// session must not become the home. `identity` is the identity file,
    /// so the home is recorded beside it (`<identity>.home`, SIP-60 §When a
    /// client presents a Move unasked) when the Move is presented.
    OpenRemote {
        target: String,
        identity: Option<std::path::PathBuf>,
    },
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
    /// Nobody is at this machine, or somebody is again. Beaten to the
    /// exchange at once (SIP-4's away bit), so the people you talk to see
    /// it without waiting for the next beat.
    Away(bool),
    /// This person compared the safety words with the owner of this key
    /// and they matched (SIP-41). A fact they established; kept here.
    Verify(PubKey),
    /// Take the mark back. Theirs to take; nobody else's.
    Unverify(PubKey),
    /// Say, to the exchange, that the words were compared: a SIP-27 claim
    /// others may read and must not act on. Never sent without asking.
    Attest(PubKey),
    /// SIP-27: read who has said that of `who`, for the dialog that offers
    /// to say it too.
    Attested(PubKey),
    /// SIP-44: ask the registry whether `who` was succeeded, and by whom.
    SuccessionOf(PubKey),
    /// SIP-45: the platform's wake endpoint and how long the exchange may
    /// keep it, or `None` to forget the one registered. Told to the exchange
    /// now if the link is up, and again on every connect.
    WakeEndpoint(Option<(String, u32)>),
    /// Open a conversation with one of its messages in the window: what a
    /// search result does. [`Show`](Cmd::Show) opens on the last page and a
    /// hit can be anywhere before it; this widens the window so the message
    /// is drawn, with half a page above it, in one hop rather than a page at
    /// a time. Never narrows a conversation that is already open wider.
    ShowAt {
        channel: [u8; 32],
        seq: u64,
    },
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

    /// The exchange this session reached, and the home carrying it —
    /// **without cloning the state around them**.
    ///
    /// `state()` copies the whole of what the interface reads: the open
    /// conversation's lines, everybody's profile, the conversation list.
    /// The title strip wanted two fields out of it *per exchange row, per
    /// frame*, and paid for all of it each time.
    pub fn where_it_is(&self) -> (Option<PubKey>, Option<String>) {
        let state = self.state.borrow();
        (state.exchange, state.carried.clone())
    }

    /// The same, plus the domain: what the title strip calls this
    /// exchange, again without copying what the strip is not drawing.
    pub fn how_it_is_named(&self) -> (Option<String>, Option<PubKey>) {
        let state = self.state.borrow();
        (state.domain.clone(), state.exchange)
    }

    /// The state's own channel, to be read without this handle: what the
    /// announcer holds, so a ring can be said from a session's wake with no
    /// frame to read it in.
    pub fn watch(&self) -> watch::Receiver<ChatState> {
        self.state.clone()
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

    /// SIP-39: the cross-exchange call ringing, if one is. The same shape as
    /// [`ringing`](Self::ringing), for the same reason.
    pub fn cross_ring(&self) -> Option<CrossRing> {
        self.state.borrow().cross_ring.clone()
    }

    /// Messages mentioning us that arrived while this session was up. The
    /// same shape as [`ringing`](Self::ringing), for the same reason.
    pub fn mentions(&self) -> Vec<Mention> {
        self.state
            .borrow()
            .arrivals
            .iter()
            .filter(|a| a.mentions_me)
            .cloned()
            .collect()
    }

    /// Every message from somebody else that arrived live.
    pub fn arrivals(&self) -> Vec<Arrival> {
        self.state.borrow().arrivals.clone()
    }

    /// Every message from somebody else that this device did not hold when
    /// the session started, whether it arrived live or while the device was
    /// away. What a woken phone has to say.
    pub fn unseen(&self) -> Vec<Arrival> {
        self.state.borrow().unseen.clone()
    }

    /// How much is waiting here, across every conversation.
    pub fn unread(&self) -> usize {
        self.unread_but(|_| false)
    }

    /// How much is waiting here, leaving out the conversations `muted`
    /// says to: their own row still counts, the icon does not.
    pub fn unread_but(&self, muted: impl Fn(&[u8; 32]) -> bool) -> usize {
        self.state
            .borrow()
            .conversations
            .iter()
            .filter(|c| !muted(&c.channel))
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

    /// Whether this session is over.
    ///
    /// A session ends by returning an error — a store that could not be
    /// opened, prekeys that could not be published — and the task then stops.
    /// Nothing about the handle says so, and the interface went on holding it
    /// as though an identity were connected. See `ChatApp::reconcile`, which
    /// starts it again.
    pub fn stopped(&self) -> bool {
        self.task.is_finished()
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

/// An `At` or `Discover` dial, resolved to where it points. The two other
/// shapes are refused here: a session **owns** its connection -- it is the
/// thing that dials, holds and redials it, and lends it to calls
/// (`ChatHandle::connection`) -- so running on somebody else's (`On`) would
/// invert that, and a tunnel inside a tunnel (`Via` within `Via`) is not
/// specified (SIP-85 §Rationale).
/// The exchange's domain, for showing SIP-38 handles as `name@domain`.
///
/// **Nothing set this.** `Chat::handle` needs a domain to compose one, so
/// it returned `None` for every account including our own — and a name
/// claimed at this exchange still read as unregistered afterwards, with
/// the claim having actually worked. Read off the same layers the
/// connection was made from, and only when they name a domain: an address
/// is not one, and `name@203.0.113.1` is not a handle.
fn domain_of_dial(dial: &Dial) -> Option<String> {
    match dial {
        Dial::Discover(layers) => sigil_net::domain_of(layers),
        // SIP-85: the name the home was asked to find the target by is the
        // target's domain, whichever way the target itself was given.
        Dial::Via { target_domain, .. } => Some(target_domain.clone()),
        // No domain to show: reached by a literal host and key, or — refused
        // above — on somebody else's connection.
        Dial::At(_) | Dial::On(_) => None,
    }
}

/// The exchange's key, where it is known without the network: given
/// literally, or pinned in `known_servers` for the domain to be discovered
/// (SIP-33 -- the pin is what every later contact is held to anyway).
/// `None` for a first contact, which has nothing on the disc to draw.
fn pinned_key(dial: &Dial) -> Option<PubKey> {
    match dial {
        Dial::At(e) => Some(e.server),
        Dial::Discover(layers) => {
            let first = layers
                .iter()
                .find(|l| l.server.is_some() || l.host.is_some())?;
            if let Some(k) = &first.key {
                return k.parse().ok();
            }
            let domain = first.server.as_deref()?;
            sigil_net::pinned_key_of(domain)
        }
        Dial::Via { target, .. } => pinned_key(target),
        Dial::On(_) => None,
    }
}

async fn resolve_plain(dial: &Dial) -> Result<Endpoint, String> {
    match dial {
        Dial::At(e) => Ok(*e),
        Dial::Discover(layers) => {
            let mut silent = sqex_voice::engine::Silent;
            sqex_voice::engine::resolve(&layers[..], &mut silent).await
        }
        Dial::On(_) => Err("a chat session opens its own connection".to_string()),
        Dial::Via { .. } => {
            Err("a tunnel through a tunnel is not something this opens".to_string())
        }
    }
}

/// SIP-85: what to call the home in the interface -- its domain when the
/// dial names one, else its address.
fn chat_home_label(dial: &Dial) -> String {
    match dial {
        Dial::Via { home, .. } => match home.as_ref() {
            Dial::Discover(layers) => {
                sigil_net::domain_of(layers).unwrap_or_else(|| "your home".into())
            }
            Dial::At(e) => e.address.to_string(),
            _ => "your home".into(),
        },
        _ => "your home".into(),
    }
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
    // SIP-85: through the home, the target is resolved for its key and the
    // home for its address, and what is dialled is the tunnel's loopback
    // socket with the target's key pinned. The lock below is by the
    // **target's** key -- it is the target's SIP-17 counter -- as it would be
    // directly.
    // **The disc first.** Where the exchange's key is already known without
    // asking anybody -- pinned in `known_servers`, or given literally -- the
    // store is locked, opened and drawn from before a single packet is sent:
    // the conversation list and the open conversation come from this
    // machine, and the exchange adds to them when it answers. Until this,
    // a window showed nothing until DNS, a tunnel and a handshake had all
    // completed, which on a slow morning was the whole of what the person
    // saw (2026-09-22). A first contact has no pin and nothing on the disc,
    // and takes the old order.
    let domain = domain_of_dial(&dial);
    let mut desk = Desk {
        seed,
        ..Desk::default()
    };
    let early = match pinned_key(&dial) {
        Some(key) => match store::lock(&path, &key) {
            Ok(lock) => match Store::open(&seed, Some(&path)) {
                Ok(mut store) => {
                    let _ = store.scope_to(&key);
                    let acting = store.account().ok().flatten().unwrap_or(me);
                    let offline = Offline {
                        store: &store,
                        me: acting,
                        domain: domain.clone(),
                    };
                    sync_local(&offline, &mut desk, acting);
                    state.send_modify(|s| {
                        s.me = Some(acting);
                        s.exchange = Some(key);
                        s.domain = domain.clone();
                    });
                    let _ = publish(&offline, &state, &desk, acting);
                    (wake)();
                    Some((key, lock, store))
                }
                Err(_) => None,
            },
            // Locked by another client: reported below, by the same path
            // that always reported it.
            Err(_) => None,
        },
        None => None,
    };
    let (endpoint, carried) = match &dial {
        Dial::Via {
            home,
            target,
            target_domain,
        } => {
            let target = resolve_plain(target).await?;
            let home = resolve_plain(home).await?;
            if home.server == target.server {
                return Err(format!(
                    "{target_domain} is your home; a home carries connections to other exchanges"
                ));
            }
            let carrier = sigil_net::carry(home, &seed, &target.server, target_domain).await?;
            (
                Endpoint {
                    address: carrier.local_addr(),
                    server: target.server,
                },
                Some((home, carrier)),
            )
        }
        other => (resolve_plain(other).await?, None),
    };
    // Held for the life of the session. Two interactive clients on one account
    // at one exchange would disagree about the next message counter, and
    // reusing one costs the confidentiality of two messages.
    // What the disc gave us stands if the exchange resolved to the key it
    // was pinned under; a pin that moved (SIP-40) is a different exchange,
    // and the lock and the store are taken again under the real key.
    let (_lock, store) = match early {
        Some((key, lock, store)) if key == endpoint.server => (lock, store),
        _ => {
            desk = Desk {
                seed,
                ..Desk::default()
            };
            let lock = store::lock(&path, &endpoint.server).map_err(|e| {
                // Which exchange, so the interface can check whether the
                // client already holding it is one of its own. The message
                // stands on its own for the case where it is not.
                state.send_modify(|s| {
                    s.exchange = Some(endpoint.server);
                    s.locked_out = Some(endpoint.server);
                });
                format!("another client is already using this account at this exchange: {e}")
            })?;
            let store = Store::open(&seed, Some(&path)).map_err(|e| e.to_string())?;
            (lock, store)
        }
    };
    // **Opening a conversation does not wait for the handshake.** While the
    // exchange is dialled, a Show is answered from the disc -- the fold is
    // already in `desk` -- and anything that needs the exchange is kept
    // until there is one. A click that waited behind a slow handshake was
    // a click that seemed to do nothing.
    let mut deferred: Vec<Cmd> = Vec::new();
    let client = {
        let connect = sqnr::Client::connect_as(endpoint.address, endpoint.server.as_bytes(), &seed);
        tokio::pin!(connect);
        let acting = store.account().ok().flatten().unwrap_or(me);
        loop {
            tokio::select! {
                connected = &mut connect => break connected?,
                cmd = cmds.recv() => match cmd {
                    Some(Cmd::Show(channel)) => {
                        // Borrowed for the open and the publish only: a
                        // `&Store` held across the await above would make
                        // this task un-spawnable.
                        let offline = Offline {
                            store: &store,
                            me: acting,
                            domain: domain.clone(),
                        };
                        open(&offline, &mut desk, &state, channel);
                        let _ = publish(&offline, &state, &desk, acting);
                        (wake)();
                    }
                    // **Answered from the disc, like `Show`.** A search
                    // reads words this machine already holds and asks the
                    // exchange nothing, so deferring it until a connection
                    // exists would make a reader wait for a round trip that
                    // has nothing to do with their question -- and, at an
                    // exchange that never answers, wait for ever.
                    Some(Cmd::Search(query)) => {
                        let offline = Offline {
                            store: &store,
                            me: acting,
                            domain: domain.clone(),
                        };
                        search_local(&offline, &mut desk, &state, acting, &query);
                        // Searching folds whatever it had not folded yet, so
                        // what the list says about those conversations has
                        // just changed.
                        let _ = publish(&offline, &state, &desk, acting);
                        (wake)();
                    }
                    Some(Cmd::Earlier) => {
                        let offline = Offline {
                            store: &store,
                            me: acting,
                            domain: domain.clone(),
                        };
                        reach_earlier(&offline, &mut desk);
                        let _ = publish(&offline, &state, &desk, acting);
                        (wake)();
                    }
                    Some(Cmd::Close) => {
                        desk.open = None;
                        state.send_modify(|s| {
                            s.open = None;
                            s.lines.clear();
                            s.copies.clear();
                            s.stranded.clear();
                            s.divider = None;
                            s.unread_on_open = 0;
                        });
                        (wake)();
                    }
                    Some(other) => deferred.push(other),
                    // The handle is gone: nobody is listening.
                    None => return Ok(()),
                },
            }
        }
    };
    let mut chat = Chat::new(client, seed, me, endpoint.server, store);
    // **The account, which is not always the device.** `me` above is this
    // client's own key: what it seals under, publishes prekeys for and counts
    // messages with. The account it *acts for* is the same key until the
    // device is linked to one, and from then on it is the account's, which
    // `Chat` reads from the store.
    //
    // Everything below is relative to the account -- the display name, the
    // handle, whether a message is one's own, which admin is somebody else --
    // and this took the device's key and never looked again. A linked device
    // drew itself as its own key, under which there is no name and no handle,
    // and read its account's other device as a stranger. It can also change
    // while the session runs (a credential presented here), so `acting_as`
    // below follows it rather than this being read once.
    let device = me;
    let mut me = chat.me;
    // SIP-85: the tunnel is the session's for as long as it lives, and a
    // reconnect that finds it closed opens another before it dials.
    let carried_by = match (&dial, carried) {
        (Dial::Via { target_domain, .. }, Some((home, carrier))) => {
            let home_label = chat_home_label(&dial);
            chat.via(
                (home.address, *home.server.as_bytes()),
                *endpoint.server.as_bytes(),
                target_domain.clone(),
                carrier,
            );
            Some(home_label)
        }
        _ => None,
    };
    chat.set_domain(domain.clone());
    // So a lost connection can be rebuilt without restarting the session.
    chat.dials(endpoint.address, endpoint.server.as_bytes().to_owned());

    state.send_modify(|s| {
        s.me = Some(me);
        s.exchange = Some(endpoint.server);
        s.domain = domain;
        s.carried = carried_by;
    });

    // **Somewhere for the exchange to knock.** The tick is a backstop now, not
    // the clock: an event says which conversation moved, and waiting 700ms to
    // hear it is 700ms of a message sitting in a queue that has already
    // crossed the world.
    let knock: sqex_chat::events::Wake = Arc::new(tokio::sync::Notify::new());
    chat.wake_on_events(knock.clone());

    // **Before the exchange is asked anything.** Everything below this point
    // is a round trip -- prekeys, then the list, then a fetch per channel --
    // and none of it is needed to draw what this machine already holds. The
    // local copy is also the only copy that can ever be read: opening an
    // epoch key spends the prekey it was sealed against, so the disc is not a
    // cache of the exchange's data, it is the data.
    sync_local(&chat, &mut desk, me);
    let _ = publish(&chat, &state, &desk, me);
    (wake)();

    // **SIP-44 §The handover: whose device this is, as the registry has it.**
    // A device can be handed from one account to another while it is away --
    // a succession, or simply being given to somebody else -- and the
    // exchange's registry is the one party that knows. The client asks once a
    // connection, and where the answer is not the account this store names,
    // it follows: the store's account, its credential and its direct message
    // aliases all move, as the presenting device's did.
    //
    // Cheap in the ordinary case, which is why it is unconditional: a device
    // that is its own account is told its own key back and stops there, and
    // an exchange too old to know the route answers 404, which stands for
    // "the store is right".
    match chat.follow_account().await {
        // Two shapes, and they read nothing like each other to whoever is
        // holding the phone. The registry answering this device its own key
        // back is a registration that is gone -- revoked, or expired -- and
        // the device is its own account again, which is what every key is
        // until a registration says otherwise (SIP-22).
        Ok(Some(now)) if now == device => {
            tracing::info!("the registration is gone; this device is its own account again");
            note(
                &state,
                "This device is its own account again: the account it acted for no \
                 longer lists it. What was said in its conversations stays on this \
                 machine and cannot be added to."
                    .into(),
            );
        }
        Ok(Some(now)) => {
            tracing::info!(account = %now, "the registry moved this device to another account");
            note(
                &state,
                format!(
                    "This device now acts for {now}. The exchange's registry says it was \
                     handed over; what is held here followed it."
                ),
            );
        }
        Ok(None) => {}
        // Not fatal and not silent. The store's account stands, which is what
        // every other operation this pass is about to use.
        Err(e) => tracing::warn!(error = %e, "could not ask whose device this is"),
    }
    if acting_as(&chat, &state, &mut me) {
        desk.restructure = true;
        sync_local(&chat, &mut desk, me);
        let _ = publish(&chat, &state, &desk, me);
        (wake)();
    }
    match chat.top_up_prekeys().await {
        Ok(()) => {}
        // SIP-59: not ours to use any more. Said, and parked -- the task
        // ends without an error, and `reconcile` leaves a parked session
        // alone for `PARKED_RETRY`. What the disc held was published above,
        // so the conversations stay readable.
        Err(sqex_chat::client::ChatError::Moved(key, domain_there)) => {
            let there = match (&key, domain_there.is_empty()) {
                (Some(k), true) => k.to_string(),
                (_, false) => domain_there.clone(),
                (None, true) => "another exchange".to_string(),
            };
            let here = chat
                .domain()
                .map(str::to_string)
                .unwrap_or_else(|| endpoint.server.to_string());
            state.send_modify(|s| {
                s.moved_to = key.map(|k| (k, domain_there.clone()));
                s.link = LinkState::Gone;
                s.trouble = Some(format!(
                    "this identity lives at {there}; {here} hands its services off there"
                ));
            });
            (wake)();
            return Ok(());
        }
        // **Unverified against a real refusal (2026-09-24).** Pointing this
        // client at an exchange whose whitelist excludes it did *not* reach
        // here: the session sat in `Retrying` indefinitely, because the
        // library retries the connection internally and reports only
        // `Link::Retrying` -- no code, no reason. So this arm fires for an
        // exchange that admits the *connection* and refuses the account's
        // routes, which is what `server.admitted` does and why
        // `/admission/request` is exempt from it; it does not fire when the
        // connection itself never completes. Telling those apart needs the
        // library to surface why a dial failed, which it does not.
        //
        // **SIP-24: not admitted here.** An exchange with a whitelist
        // refuses every gated route with this code, so nothing else this
        // session would try can succeed -- retrying is the wrong shape.
        // Parked like the moved case above, and said with the one thing
        // that can be done about it, because `/admission/request` is open
        // exactly when everything else is not.
        Err(sqex_chat::client::ChatError::Refused(_, ref r))
            if r.code == sqex_proto::refusal::Code::NotWhitelisted =>
        {
            let here = chat
                .domain()
                .map(str::to_string)
                .unwrap_or_else(|| endpoint.server.to_string());
            state.send_modify(|s| {
                s.link = LinkState::Gone;
                s.not_admitted = Some(here.clone());
                s.trouble = Some(format!("{here} does not admit this account"));
            });
            (wake)();
            return Ok(());
        }
        Err(e) => return Err(e.to_string()),
    }
    // SIP-47 §Catching up in one round trip: everything that moved while this client was away, in one
    // round trip, before the sweep asks channel by channel. On a desktop it
    // is a faster start; on a phone woken for seconds it is the difference
    // between a window that finishes and one that does not.
    catch_up(&mut chat, &state, &mut desk, me).await;
    // What was asked for while the exchange was being dialled, now that
    // there is one to ask.
    for cmd in std::mem::take(&mut deferred) {
        apply(&mut chat, cmd, &state, &mut desk).await;
    }
    (wake)();
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
        // Before anything is drawn or published this pass: a command handled
        // last pass may have linked this device to an account.
        if acting_as(&chat, &state, &mut me) {
            desk.restructure = true;
            desk.dirty.extend(desk.channels.keys().copied());
            sync_local(&chat, &mut desk, me);
            let _ = publish(&chat, &state, &desk, me);
            (wake)();
        }
        // **The connection, for whoever else wants to reach this exchange as
        // this identity.** Written when the link changes rather than every
        // pass: a redial makes a new connection, and what was lent before it
        // is closed.
        if chat.link() != lent {
            lent = chat.link();
            holds.set(chat.connection().map(|c| (c, endpoint)));
            // A new connection may be to a redeployed exchange: ask again.
            desk.peers_read = false;
        }
        // **Pictures still waiting come first, and at once.** The fetch just
        // done was the pacing; a wait here on top of it -- seven hundred
        // milliseconds a picture, and a refresh round trip with each -- was
        // most of what a reader waited for on the second sight of a
        // conversation. Commands are still answered between them: the fetch
        // attends to them before it asks the exchange for anything.
        if more {
            let did = fetch_files(&mut chat, &state, &mut desk, &mut cmds).await;
            more = did.more;
            if did.landed && publish(&chat, &state, &desk, me) {
                (wake)();
            }
            continue;
        }
        // Nothing left to fetch: one clip's first frame, from what is
        // already on the disc. After the fetches, because a picture on its
        // way matters more than a sharper thumbnail for one that is here.
        if still_for_a_clip(&mut chat, &mut desk).await && publish(&chat, &state, &desk, me) {
            (wake)();
        }
        // **What is outstanding decides the wait.** A link being redialled
        // advances a slice per pass and would take minutes at the quiet
        // interval; a note has to disappear five seconds after it appeared,
        // not ten; a channel an event named is one somebody is waiting to see;
        // and "typing..." has to go out when somebody stops, which is the one
        // thing only asking can find (see `still_live`) -- at the quiet
        // interval it would linger five seconds after they had gone.
        let quick = chat.link() != Link::Up
            || !desk.dirty.is_empty()
            || desk.restructure
            || still_live(&desk).is_some()
            || desk.siblings.live.is_some()
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
                // A picture just asked for, or just sent, is fetched now and
                // not on the next backstop: that was five seconds of
                // thumbnail after pressing Fetch, and after sending one.
                {
                    let did = fetch_files(&mut chat, &state, &mut desk, &mut cmds).await;
                    more = did.more;
                    if did.landed && publish(&chat, &state, &desk, me) {
                        (wake)();
                    }
                }
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
                        // The same as the knock: a message with a picture
                        // in it is one round trip, and the picture is the
                        // next -- not the next backstop. Measured at eight
                        // to ten seconds from message to picture before
                        // this, of which the fetch itself was under two.
                        {
                    let did = fetch_files(&mut chat, &state, &mut desk, &mut cmds).await;
                    more = did.more;
                    if did.landed && publish(&chat, &state, &desk, me) {
                        (wake)();
                    }
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
                refresh_rosters(&mut chat, &mut desk).await;
                if refresh(&mut chat, &state, &mut desk, me, &mut cmds).await {
                    (wake)();
                }
                // A message with pictures in it arrives as one event; without
                // this they would come in one per backstop, five seconds
                // apart, however fast the exchange was.
                {
                    let did = fetch_files(&mut chat, &state, &mut desk, &mut cmds).await;
                    more = did.more;
                    if did.landed && publish(&chat, &state, &desk, me) {
                        (wake)();
                    }
                }
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
                // SIP-45, on every connect: a registration is idempotent
                // and cheap, and a phone that forgot would go quiet at the
                // end of the last ttl, silently. `wake_told` is per link:
                // a redial is a new connection the exchange keys nothing to.
                if chat.link() != Link::Up {
                    desk.wake_told = false;
                } else if !desk.wake_told && desk.wake.is_some() {
                    tell_wake(&mut chat, &state, &mut desk).await;
                }
                // SIP-48: keep the backup up to date, for an account that
                // has one.
                keep_the_backup_fresh(&mut chat, &state, &mut desk).await;
                // SIP-43: and whether what this device wrote here is what
                // the exchange has for it.
                if check_the_chain(&mut chat, &mut desk).await {
                    desk.restructure = true;
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
                // SIP-4: say we are here, and ask about the people we
                // talk to. Both on the tick and neither in the way of it:
                // a beat is one request every half minute, and the asking
                // is spread a few people at a time.
                if chat.link() == Link::Up {
                    if !desk.peers_read {
                        desk.peers_read = true;
                        moved |= read_peers(&mut chat, &mut desk).await;
                    }
                    if desk
                        .beat_at
                        .is_none_or(|at| at.elapsed().as_secs() >= u64::from(crate::presence::BEAT_SECS))
                    {
                        beat(&mut chat, &mut desk).await;
                    }
                    moved |= read_presence(&mut chat, &mut desk, me).await;
                    // SIP-42: history between this account's devices. An
                    // open kept toward each sibling, and a sync run a step
                    // a tick with whichever has turned up.
                    moved |= sync_siblings(&mut chat, &state, &mut desk).await;
                }
                // **The conversation before the pictures in it.** A refresh is
                // what somebody is waiting for; a blob is what they will be
                // looking at in a moment. Measured the other way round, one
                // tick spent two and a half seconds on pictures before asking
                // whether anything had been said.
                moved |= refresh(&mut chat, &state, &mut desk, me, &mut cmds).await;
                {
                    let did = fetch_files(&mut chat, &state, &mut desk, &mut cmds).await;
                    more = did.more;
                    if did.landed && publish(&chat, &state, &desk, me) {
                        (wake)();
                    }
                }
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
    /// The conversation, folded -- **or a short tail of it**, while `folded`
    /// is false.
    timeline: Timeline,
    /// Whether `timeline` is the whole conversation.
    ///
    /// False for one this session has not needed yet: what it holds then is
    /// the last few rows, plus the name, topic and picture the store
    /// remembered -- the one line a list draws, and nothing else. **This is
    /// not a cache flag.** Opening an epoch key spends the prekey it was
    /// sealed against, so the fold of this machine's own rows is the only
    /// reading of the conversation there will ever be; what is deferred is
    /// *when* it happens, never whether. Everything that draws a transcript,
    /// counts a conversation, searches it or polls it goes through
    /// [`ensure_folded`] first -- a tail handed to `poll` would be appended
    /// to above the cursor and leave a hole where the middle was.
    folded: bool,
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
    /// How many of the unread mention us.
    mentioned: usize,
    /// The newest sequence number this channel had when the exchange first
    /// answered for it this session -- everything after it arrived live.
    /// What was there already is history, and history is not announced.
    live_from: Option<u64>,
    /// The newest message this **device's store** held when the session
    /// started, or 0 for a channel it did not hold at all. Everything above
    /// it is new to this device, whether it arrived live or while the device
    /// was away -- which is a different question from `live_from`'s, and the
    /// one a phone woken by SIP-45 asks: not "what happened while I
    /// watched" but "what did I miss". A desktop that was closed overnight
    /// could ask it too.
    held_from: u64,
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
            mentioned: self.mentioned,
            waiting: self.waiting,
            // A direct message is drawn as the person in it, so a channel
            // picture on one would be a second mark for the same party.
            avatar: self
                .peer
                .is_none()
                .then(|| {
                    self.timeline
                        .avatar
                        .as_ref()
                        .map(|a| a.preview.clone())
                        .filter(|b| !b.is_empty())
                })
                .flatten(),
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

/// Why a blob could not be fetched.
///
/// # Why this is not a set of ids
///
/// It was, and every failure was final: the fetch's `Err(_)` arm put the
/// blob in a set and nothing took it out until the reader pressed Refetch or
/// the session restarted. The comment said what it was for -- "a blob past
/// its retention window is gone, and asking again four times a second will
/// not bring it back" -- and that is true of a blob that is *gone*. It is not
/// true of a radio that dropped for a second, which is the ordinary condition
/// of a phone, and both arrived here as the same `Err`.
///
/// So a failed fetch asks `/blob/head` (SIP-18) what actually happened. The
/// exchange answering `found: false` is the only thing that makes a file
/// missing; everything else is worth trying again, after a pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unfetched {
    /// The exchange says it does not hold this blob. Nothing will bring it
    /// back, and the row says so.
    Gone,
    /// The fetch failed and the blob is still there, or the exchange could
    /// not be asked. Tried again once [`RETRY_AFTER`] has passed.
    Later(std::time::Instant),
}

/// How long to leave a blob alone after a fetch that failed for a reason
/// that was not the blob.
///
/// Long enough that a link which is down does not turn into a fetch on every
/// pass -- which is what the original set was for, and the part of it worth
/// keeping. Short enough that a reader looking at a picture does not have to
/// press anything for it to arrive once the radio comes back.
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(5);

/// What a failed fetch means, given what `/blob/head` said about the blob.
///
/// Its own function because it is the whole decision, and the case that
/// matters -- a transient failure on a blob the exchange still holds -- is
/// one a test cannot easily produce against a real exchange. Here it can be
/// asked directly.
fn after_a_failed_fetch(head: Option<bool>, now: std::time::Instant) -> Unfetched {
    match head {
        // The exchange was asked and says it does not have it.
        Some(false) => Unfetched::Gone,
        // It does have it, so the fetch failed for some other reason; or it
        // could not be asked at all, which is itself a link that is down.
        // Neither says the file is gone.
        Some(true) | None => Unfetched::Later(now),
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
    /// SIP-56: reports announced since the admin last read them.
    reports_pending: usize,
    /// SIP-23: one-time prekeys the exchange still holds for this device,
    /// as of the last catch-up. `None` until one has said.
    prekeys: Option<u16>,
    /// How many conversations have been folded this session; see
    /// [`ensure_folded`] and [`ChatState::folds`].
    folds: usize,
    /// SIP-39: a cross-exchange call ringing, and when it began, so one
    /// nobody answers stops ringing on its own.
    cross_ring: Option<(CrossRing, std::time::Instant)>,
    /// Channels whose roster changed by somebody else's act -- a join, a
    /// removal -- and is re-read on its own, without rebuilding everything.
    roster_dirty: HashSet<[u8; 32]>,
    /// Channels whose read marks somebody has moved, so the receipts beside
    /// our own messages are worth asking about again.
    cursors_moved: HashSet<[u8; 32]>,
    /// The conversation list itself needs rebuilding from the exchange.
    restructure: bool,
    /// Whether the exchange has ever answered about the list, this session.
    /// What is on screen before that came off this machine's own disc.
    synced: bool,
    /// SIP-4: when this client last beat, whether it said away, and
    /// whether the exchange took the away bit at all -- one from before
    /// it refuses the bit, and is beaten to plainly from then on.
    beat_at: Option<std::time::Instant>,
    away: bool,
    beats_plainly: bool,
    /// When each person was last asked about, so the asking is spread.
    asked_at: HashMap<PubKey, std::time::Instant>,
    /// What the asking found.
    presence: HashMap<PubKey, crate::presence::Presence>,
    /// SIP-39 §The peer directory: what this exchange federates with, read once per connection.
    peers: Vec<(PubKey, String)>,
    peers_read: bool,
    /// SIP-45: the endpoint the platform offered, to be registered on every
    /// connect (SIP-47 §Connecting, step 2), or `Some(None)` to forget the
    /// one registered. `wake_told` is whether this connection was told.
    wake: Option<Option<(String, u32)>>,
    wake_told: bool,
    /// SIP-42: this account's other devices, the open kept toward each,
    /// and the sync running with one when it has turned up.
    siblings: crate::siblings::Siblings,
    /// The identity's seed, for what is signed from here (a SIP-27 claim).
    /// The chat client holds the same bytes; this is not a second secret,
    /// it is the same one where a command can reach it.
    seed: [u8; 32],
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
    /// A clip's own first frame, by blob: a JPEG of a few tens of
    /// kilobytes, decoded once from the blob on this device's disc and
    /// carried as the attachment's preview.
    ///
    /// **Because the sender's is 96 pixels.** SIP-18 caps a preview at
    /// eight kilobytes so that it can ride inside the message, and a phone
    /// draws a clip in a bubble over seven hundred device pixels. The
    /// bytes it is made from are dropped again at once: a clip is not
    /// fetched for being scrolled past ([`fetch_files`]) and this does not
    /// change that -- it only reads what is already here.
    stills: HashMap<[u8; 32], std::sync::Arc<[u8]>>,
    /// Blobs whose first frame would not decode, so it is not tried again.
    no_still: HashSet<[u8; 32]>,
    /// The order they were fetched in, for [`to_put_down`].
    fetched: Vec<[u8; 32]>,
    /// Conversations whose chain has been asked about this session.
    chain_checked: HashSet<[u8; 32]>,
    /// When this session began, so the first backup is not a launch.
    started: std::time::Instant,
    /// When the backup was last considered. See [`keep_the_backup_fresh`].
    backup_tried: Option<std::time::Instant>,
    /// Blobs a fetch has failed on, and whether the failure was final.
    ///
    /// See [`Unfetched`]: a file the exchange no longer holds is different from
    /// one a radio dropped, and until this told them apart every fetch that
    /// failed for any reason was remembered as "gone" for the rest of the
    /// session.
    unfetchable: HashMap<[u8; 32], Unfetched>,
    /// Files over [`AUTO_FETCH_MAX`] the reader pressed Fetch on. A file
    /// that is already on the disc -- this session's own upload, or one
    /// fetched last time -- needs no entry here: the store is asked directly,
    /// because what the cap guards is the *network*.
    wanted: HashSet<[u8; 32]>,
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
    /// SIP-59: where each peer's account lives, asked once. A round trip,
    /// and the answer only changes when somebody moves home.
    peer_homes: HashMap<PubKey, (PubKey, String)>,
}

impl Default for Desk {
    fn default() -> Self {
        Desk {
            channels: HashMap::new(),
            peer_homes: HashMap::new(),
            open: None,
            dirty: HashSet::new(),
            reports_pending: 0,
            prekeys: None,
            folds: 0,
            cross_ring: None,
            roster_dirty: HashSet::new(),
            cursors_moved: HashSet::new(),
            restale: HashSet::new(),
            answered: HashSet::new(),
            files: HashMap::new(),
            chain_checked: HashSet::new(),
            started: std::time::Instant::now(),
            backup_tried: None,
            stills: HashMap::new(),
            no_still: HashSet::new(),
            fetched: Vec::new(),
            unfetchable: HashMap::new(),
            wanted: HashSet::new(),
            // The first tick has nothing yet, so it rebuilds.
            restructure: true,
            synced: false,
            synced_at: std::time::Instant::now(),
            beat_at: None,
            away: false,
            beats_plainly: false,
            asked_at: HashMap::new(),
            presence: HashMap::new(),
            peers: Vec::new(),
            peers_read: false,
            wake: None,
            wake_told: false,
            siblings: crate::siblings::Siblings::default(),
            seed: [0; 32],
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
                } else {
                    // **Somebody else joined or left a room we know.** The
                    // roster is what the members view and its admin's
                    // buttons draw from, and it used to wait for the
                    // periodic rebuild -- half a minute in which a new
                    // member could not be muted or removed because they
                    // were not on the list.
                    self.roster_dirty.insert(channel);
                }
            }
            // We fell behind and events were dropped. Nothing local can be
            // trusted to be current, so read everything again.
            Event::Resync => {
                self.restructure = true;
                self.dirty.extend(self.channels.keys().copied());
            }
            // SIP-56: a member reported an entry here, said to the admins.
            // Nothing in sigil shows a report yet; the channel is re-read so
            // whatever the exchange now serves about it is what is drawn.
            Event::Reported { channel } => {
                self.dirty.insert(channel);
                self.reports_pending += 1;
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
            // SIP-39: another exchange carried a call here and ours is
            // holding a bridge for it. Nothing else arrives about it -- no
            // entry, no channel -- so this is the whole of what the
            // interface has to ring with. A second while the first still
            // rings replaces it: a bridge is for one call, and the caller
            // whose bridge lapsed is told by their own exchange.
            Event::CrossCall { bridge, caller } => {
                self.cross_ring = Some((CrossRing { bridge, caller }, std::time::Instant::now()));
            }
            Event::Admission | Event::Heartbeat | Event::Unknown(_) => {}
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
        let remembered = known.iter().find(|k| k.channel == m.channel);
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
                        // Filled from the transcript when the state is drawn.
                        muted: false,
                    })
                    .collect::<Vec<_>>(),
            ),
            Err(_) => (
                remembered.map(|k| k.admins.clone()).unwrap_or_default(),
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
            // Nothing for a label: they are not a contact, so nobody here has
            // ever said what to call them.
            (chat.dm_with(other) == m.channel).then(|| (*other, String::new()))
        });

        let label = match &peer {
            // A direct message: whatever this machine was told to call them,
            // and **nothing** when it was told nothing.
            //
            // Both arms used to answer with the key, under a comment saying
            // exactly that -- "an unnamed contact's label is only its key
            // repeated". That is a placeholder wearing the clothes of a real
            // value: nothing downstream can tell "they are called 3Kj9…" from
            // "we have nothing to call them", so the conversation list drew
            // forty-four characters of base58 where a name goes, and so did
            // the ring banner and the search results.
            //
            // What to show instead is decided in `publish`, which is the only
            // place that knows whether a profile has arrived since.
            Some((account, l)) if !l.is_empty() && l != &account.to_string() => l.clone(),
            Some(_) => String::new(),
            // A public channel's name is held by the exchange in the clear --
            // that is what the directory searches -- so it is known before a
            // single entry is read. A group's is a sealed entry and is not, so
            // until the log is read it goes by its identifier.
            None if public && !given_name.is_empty() => given_name,
            None => remembered
                .map(|k| k.label.clone())
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| format!("group {}", hex8(&m.channel))),
        };

        let group = peer.is_none();
        // The exchange has just said which kind it is, so it is written down:
        // the next start draws the right mark without asking anybody. This is
        // also what settles a row from before the store recorded it.
        let _ = chat
            .store()
            .put_channel(&m.channel, group, Some(public), &label, &admins);

        let entry = desk.channels.entry(m.channel).or_insert_with(|| {
            // A channel the exchange named that `sync_local` did not find on
            // the disc. What this machine holds of it is drawn at once -- the
            // local copy is the only one that can ever be read, since opening
            // an epoch key spends the prekey it was sealed against -- but the
            // *list* needs one line of it, so the fold waits for whatever
            // needs the rest. The poll this channel is about to get is one of
            // those, and folds it.
            let timeline = chat
                .store()
                .history_tail(&m.channel, &admins, PREVIEW_ROWS)
                .unwrap_or_default();
            let last_at = timeline.messages().last().map(|m| m.posted).unwrap_or(0);
            Known {
                peer: None,
                public: Some(public),
                group,
                label: String::new(),
                admins: Vec::new(),
                members: Vec::new(),
                marks: Vec::new(),
                timeline,
                folded: false,
                seen: 0,
                wanted: PAGE,
                last_at,
                unread: 0,
                mentioned: 0,
                live_from: None,
                held_from: 0,
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
        // SIP-43: a conversation that lives at another exchange is named
        // with where it lives -- `general@trunk.exchange` -- so two rooms
        // called the same thing on two exchanges read as two rooms. The
        // store keeps the bare name; the home is learned afresh each session.
        entry.label = match chat.homed_elsewhere(&m.channel) {
            Some(home) if entry.peer.is_none() && !label.is_empty() => {
                format!("{label}@{}", at_home(home))
            }
            _ => label,
        };
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
            // Empty when nobody has said what to call them; see the match in
            // `sync_channels` for why that is not their key.
            label: c.label.clone(),
            admins: vec![me, c.account],
            members: Vec::new(),
            marks: Vec::new(),
            timeline: Timeline::default(),
            // A contact nothing has ever been exchanged with: there is
            // nothing on the disc to fold, and the one query that finds that
            // out waits until somebody opens the conversation.
            folded: false,
            seen: 0,
            wanted: PAGE,
            unread: 0,
            mentioned: 0,
            live_from: None,
            held_from: 0,
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
/// The store now also says which of its groups are **public** (sqex-chat
/// v0.47), so the mark on a row is right from the first frame. It did not: it
/// kept `kind` as group-or-not, so every group came back with `public: None`
/// and was drawn with no mark at all until the exchange answered -- a wait,
/// on every start, for a fact that never changes.
///
/// `None` has not gone away and must not: a row written before v0.47 recorded
/// no answer, and reading it as private would tell somebody their words are
/// sealed when anybody may read them. Those rows stay unmarked for one sweep,
/// which is what writes the answer down. See [`Summary::public`].
///
/// A direct message is derivable as well as never public: its identifier comes
/// from the two accounts, so the other member of a two-member channel that
/// derives back to itself is the peer.
/// Follow the account this client acts for, when it changes under the session.
///
/// It changes twice: when a credential is presented here (`RegisterSelf`,
/// `ClaimAccount`), and when the registry moves this device to another
/// account (SIP-44 §The handover), which `Chat::follow_account` acts on. Both
/// happen while the loop is running, so the loop asks rather than remembers.
///
/// Answers whether it moved, which is a reason to redraw everything: every
/// summary, name and "is this mine" was computed against the old key.
fn acting_as(chat: &Chat, state: &watch::Sender<ChatState>, me: &mut PubKey) -> bool {
    if chat.me == *me {
        return false;
    }
    *me = chat.me;
    state.send_modify(|s| s.me = Some(*me));
    true
}

/// What drawing needs from a client, whether or not it is connected.
///
/// Everything here reads the store -- names, handles, the fold of a channel
/// -- and `Chat` answers from its store; [`Offline`] answers from a store
/// alone, before there is a connection, so a window draws what the disc
/// holds while the exchange is still being found (2026-09-22: the freeze
/// on launch was a window with nothing to draw until the handshake).
pub(crate) trait Local {
    fn store(&self) -> &Store;
    fn display_name(&self, account: &PubKey) -> Option<String>;
    fn title_of(&self, account: &PubKey) -> Option<String>;
    /// The picture an account published, as published (SIP-21 sends it
    /// inline with the profile, so this is a store read and not a round
    /// trip). Read once per state rebuild, never per frame: the accessor's
    /// own comment says it reads the database every time it is asked.
    fn avatar_of(&self, account: &PubKey) -> Option<Vec<u8>>;
    fn handle(&self, account: &PubKey) -> Option<String>;
    fn history(&self, channel: &[u8; 32], admins: &[PubKey]) -> Option<Timeline>;
    /// SIP-60 §The client keeps what it read: earlier incarnations of this
    /// channel that this client read, oldest first.
    fn earlier(&self, channel: &[u8; 32], admins: &[PubKey]) -> Vec<Timeline>;
    /// SIP-53 §Posting again: this client's own posts a move stranded,
    /// oldest first.
    fn stranded_posts(&self, channel: &[u8; 32]) -> Vec<Stranded>;
    fn dm_with(&self, them: &PubKey) -> [u8; 32];
    fn homed_elsewhere(&self, channel: &[u8; 32]) -> Option<(PubKey, String)>;
    fn link(&self) -> LinkState;
}

impl Local for Chat {
    fn store(&self) -> &Store {
        Chat::store(self)
    }
    fn display_name(&self, account: &PubKey) -> Option<String> {
        Chat::display_name(self, account)
    }
    fn title_of(&self, account: &PubKey) -> Option<String> {
        Chat::title_of(self, account)
    }
    fn avatar_of(&self, account: &PubKey) -> Option<Vec<u8>> {
        Chat::avatar_of(self, account)
    }
    fn handle(&self, account: &PubKey) -> Option<String> {
        Chat::handle(self, account)
    }
    fn history(&self, channel: &[u8; 32], admins: &[PubKey]) -> Option<Timeline> {
        Chat::history(self, channel, admins).ok()
    }
    fn earlier(&self, channel: &[u8; 32], admins: &[PubKey]) -> Vec<Timeline> {
        Chat::earlier(self, channel, admins).unwrap_or_default()
    }
    fn stranded_posts(&self, channel: &[u8; 32]) -> Vec<Stranded> {
        Chat::stranded_posts(self, channel)
            .unwrap_or_default()
            .into_iter()
            .map(|(seq, posted, post)| Stranded {
                seq,
                posted,
                text: post.body_text().unwrap_or_default().to_string(),
                files: post.attachments().count(),
            })
            .collect()
    }
    fn dm_with(&self, them: &PubKey) -> [u8; 32] {
        Chat::dm_with(self, them)
    }
    fn homed_elsewhere(&self, channel: &[u8; 32]) -> Option<(PubKey, String)> {
        Chat::homed_elsewhere(self, channel).map(|h| (h.origin, h.domain.clone()))
    }
    fn link(&self) -> LinkState {
        LinkState::from(Chat::link(self))
    }
}

/// A store and nothing else: the disc, drawn before the exchange answers.
struct Offline<'a> {
    store: &'a Store,
    me: PubKey,
    domain: Option<String>,
}

impl Local for Offline<'_> {
    fn store(&self) -> &Store {
        self.store
    }
    fn display_name(&self, account: &PubKey) -> Option<String> {
        let (name, _, _) = self.store.profile(account).ok().flatten()?;
        (!name.is_empty()).then_some(name)
    }
    fn title_of(&self, account: &PubKey) -> Option<String> {
        let (_, title, _) = self.store.profile(account).ok().flatten()?;
        (!title.is_empty()).then_some(title)
    }
    fn avatar_of(&self, account: &PubKey) -> Option<Vec<u8>> {
        self.store.avatar(account).ok().flatten()
    }
    fn handle(&self, account: &PubKey) -> Option<String> {
        let (name, _) = self.store.handle(account).ok().flatten()?;
        let domain = self.domain.as_deref()?;
        (!name.is_empty()).then(|| format!("{name}@{domain}"))
    }
    fn history(&self, channel: &[u8; 32], admins: &[PubKey]) -> Option<Timeline> {
        self.store.history(channel, admins).ok()
    }
    /// **Not from a store alone.** Folding an earlier copy opens its
    /// entries under the keys of the incarnation they belong to, which is
    /// `Chat`'s work and not the store's. Nothing is lost by waiting: this
    /// view lasts until the session is up, and the copies appear with it.
    fn earlier(&self, _channel: &[u8; 32], _admins: &[PubKey]) -> Vec<Timeline> {
        Vec::new()
    }
    /// Nothing is offered before the session is up either: sending one
    /// again needs the exchange, and an offer that cannot be taken is worse
    /// than none.
    fn stranded_posts(&self, _channel: &[u8; 32]) -> Vec<Stranded> {
        Vec::new()
    }
    fn dm_with(&self, them: &PubKey) -> [u8; 32] {
        self.store.dm_with(&self.me, them)
    }
    fn homed_elsewhere(&self, _channel: &[u8; 32]) -> Option<(PubKey, String)> {
        None
    }
    fn link(&self) -> LinkState {
        LinkState::Connecting
    }
}

/// How many stored rows a conversation's preview line is folded from.
///
/// Rows, not messages, so that a conversation whose last words are buried
/// under reactions still shows a line. Twenty is enough for that and small
/// enough that a list of a hundred conversations opens two thousand bodies
/// rather than every body on the disc.
pub const PREVIEW_ROWS: usize = 20;

fn sync_local(chat: &impl Local, desk: &mut Desk, me: PubKey) {
    let Ok(channels) = chat.store().channels() else {
        return;
    };
    for known in channels {
        let sqex_chat::Channel {
            channel,
            group,
            public,
            label,
            admins,
            topic,
            avatar,
            meta_seq,
        } = known;
        // **The list, and not the conversations in it.** What a row draws is
        // a name, a time and a line; folding a channel to arrive at those
        // reads every row it holds and opens every sealed body, for
        // conversations nobody is going to open. The store answers the first
        // two outright and the third from its last few rows.
        let newest = chat.store().newest_message(&channel).ok().flatten();
        let mut timeline = chat
            .store()
            .history_tail(&channel, &admins, PREVIEW_ROWS)
            .unwrap_or_default();
        // The room's own name, topic and picture as the last fold read them.
        // Seeded only when the tail read no metadata entry of its own: a
        // twenty-row tail almost never contains the entry that named the
        // room, and a list drawn without them would lose every channel
        // picture on the first frame of every launch.
        if timeline.metadata_seq() == 0 && meta_seq > 0 {
            timeline.topic = topic;
            timeline.avatar = avatar;
        }
        let last_at = timeline
            .messages()
            .last()
            .map(|m| m.posted)
            .or(newest.map(|(_, posted)| posted))
            .unwrap_or(0);
        // The newest *row*, not the newest message: everything above it is
        // new to this device, and a row this store already holds is not.
        // Taking the last message instead would leave the reactions and
        // system entries above it looking like arrivals.
        let held_from = newest.map_or(0, |(seq, _)| seq);
        // Nothing is folded yet, so nothing has been seen yet -- and nothing
        // reads this until `ensure_folded` sets it from the fold, which is
        // the same number the eager fold had here.
        let seen = 0;
        let peer = (!group)
            .then(|| admins.iter().copied().find(|a| *a != me))
            .flatten()
            .filter(|other| chat.dm_with(other) == channel);
        desk.channels.entry(channel).or_insert(Known {
            peer,
            // Whatever the store recorded, and `None` when it recorded
            // nothing. Never `false` as a stand-in: drawing a public channel
            // as private claims its contents are sealed, and drawing a private
            // group as public claims the opposite. Neither is a guess to make
            // on somebody's behalf.
            public,
            group,
            label,
            // Remembered from the last time the exchange said so, which is
            // what a fold of this channel will need.
            admins,
            members: Vec::new(),
            marks: Vec::new(),
            timeline,
            folded: false,
            seen,
            wanted: PAGE,
            last_at,
            unread: 0,
            mentioned: 0,
            live_from: None,
            held_from,
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

/// Fold a conversation whole, if this session has not already.
///
/// **The one place a conversation stops being a preview and becomes itself.**
/// A fold is not free -- every row read, every sealed body opened -- and it is
/// also not optional for anything that reads more than the last line: the
/// store's rows are the only copy of this conversation there is. So it
/// happens on the first thing that needs it (opening, searching, scrolling
/// back) and on every poll, because `poll` appends above the cursor of the
/// timeline it is handed and would leave a hole in one that began at the
/// twentieth row from the end.
///
/// Returns whether it folded, which is only of interest to the count.
fn ensure_folded(chat: &impl Local, desk: &mut Desk, channel: &[u8; 32]) -> bool {
    let Some(known) = desk.channels.get_mut(channel) else {
        return false;
    };
    if known.folded {
        return false;
    }
    let mut timeline = chat.history(channel, &known.admins).unwrap_or_default();
    // **A fold that read no metadata entry knows nothing that could replace
    // what was remembered**, which is the rule the store follows when it
    // writes the row and the rule the list follows when it reads it. Without
    // this, a room whose naming entry has since passed out of retention would
    // lose its topic and picture the moment somebody opened it -- the fold
    // finding nothing would be read as the room having nothing.
    if timeline.metadata_seq() == 0 {
        timeline.topic = std::mem::take(&mut known.timeline.topic);
        timeline.avatar = known.timeline.avatar.take();
    }
    // What the disc held, counted as the eager fold counted it at start: the
    // floor an arrival is counted unread against.
    known.seen = timeline.messages().count();
    if let Some(newest) = timeline.messages().last().map(|m| m.posted) {
        known.last_at = known.last_at.max(newest);
    }
    known.timeline = timeline;
    known.folded = true;
    desk.folds += 1;
    true
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
        Ok(conversation) => took(chat.store(), chat.me, known, channel, open, conversation),
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
    me: PubKey,
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
    if !known.fetched {
        known.live_from = Some(known.timeline.messages().last().map_or(0, |m| m.seq));
    }
    known.fetched = true;
    // Which of the unreadable this identity may take down. The fold says
    // which entries; the store says who wrote each, which the fold does not
    // carry for an entry it could not make sense of. Only when there are any,
    // because it is a walk over the channel's rows.
    //
    // **By the fold's list, not by what the store could open.** An entry is
    // unreadable in two ways: sealed under a key we lack, which the store
    // holds unopened; or well formed at the transport and not understood by
    // `Body::decode`, which the store holds *opened* -- a public channel's
    // entry is plaintext and goes in as it came. The first version of this
    // looked only for the former, and offered nothing for exactly the case
    // this exists for.
    let redactable = if conversation.unreadable.is_empty() {
        Vec::new()
    } else {
        let admin = known.admins.contains(&me);
        let unopened: Vec<(u64, PubKey)> = store
            .messages(&channel)
            .unwrap_or_default()
            .into_iter()
            .filter(|(seq, ..)| conversation.unreadable.contains(seq))
            .map(|(seq, account, ..)| (seq, account))
            .collect();
        redactable(&unopened, me, admin)
    };
    known.trouble = Trouble {
        unreadable: conversation.unreadable.len(),
        redactable,
        gap: conversation.gap,
        restarted: conversation.restarted,
        no_key: conversation.no_key,
        lost: conversation.lost,
        forged: known.timeline.forged().len(),
        // Kept across a restructure: it is asked once a session and this
        // runs on every change to the conversation.
        chain_apart: known.trouble.chain_apart,
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
        known.mentioned += known
            .timeline
            .messages()
            .skip(known.seen)
            .filter(|m| m.account != me && m.post.mentions().any(|k| *k == me))
            .count();
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
/// SIP-45: leave the wake endpoint with the exchange, or take it back, on
/// the connection this session holds. The outcome is published, in words.
async fn tell_wake(chat: &mut Chat, state: &watch::Sender<ChatState>, desk: &mut Desk) {
    let Some(wanted) = desk.wake.clone() else {
        return;
    };
    let Some(mut client) = chat.connection() else {
        return;
    };
    let (route, body, done) = match &wanted {
        Some((url, ttl)) => (
            "/wake/register",
            sqex_proto::wake::Register {
                ttl: (*ttl).clamp(1, sqex_proto::wake::MAX_TTL),
                endpoint: url.clone(),
            }
            .encode(),
            "registered",
        ),
        None => ("/wake/forget", sqex_proto::wake::forget(), "forgotten"),
    };
    let said = match client.post(route, body).await {
        Ok((200, _)) => done.to_string(),
        // SIP-45 is additive: an exchange from before it answers not_found,
        // and the phone keeps its stream open as long as the platform lets
        // it, which is what it did.
        Ok((404, _)) => "not offered here".to_string(),
        Ok((code, body)) => match sqex_proto::refusal::Refusal::decode(&body) {
            Ok(r) => format!("refused: {r} ({code})"),
            Err(_) => format!("refused: status {code}"),
        },
        Err(e) => format!("failed: {e}"),
    };
    tracing::info!(route, %said, "wake endpoint");
    desk.wake_told = true;
    state.send_modify(|s| s.wake = Some(said));
}

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
        // **Whole, or not polled at all.** `poll` appends above the cursor of
        // the timeline it is handed, so handing it a preview tail would leave
        // a hole where the middle of the conversation was. With no link there
        // is nothing to append -- `post_within` refuses while offline -- and
        // folding a conversation for a request that cannot be made is work
        // nobody asked for, so it waits for the link.
        if chat.link() != Link::Up {
            continue;
        }
        ensure_folded(&*chat, desk, &channel);
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
                if let Some(seq) = took(chat.store(), chat.me, known, channel, open, conversation) {
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
        known.mentioned = 0;
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
/// SIP-43: what a channel that lives elsewhere is said to live at -- the
/// domain the operator recorded, or the head of the origin's key.
fn at_home(home: &sqex_proto::channel::Home) -> String {
    if home.domain.is_empty() {
        let key = home.origin.to_string();
        format!("{}…", &key[..key.len().min(8)])
    } else {
        home.domain.clone()
    }
}

/// Bytes of entries and envelopes one catch-up asks for. Half of SIP-47 §Limits'
/// ceiling: what a phone can absorb in a window, and more than a night's
/// worth of most conversations.
const CATCHUP_BUDGET: u32 = 512 * 1024;

/// SIP-47 §Catching up in one round trip, once per session: name every channel the store holds, with where
/// it got to, and absorb what came back exactly as a poll's answer is
/// absorbed -- the same `Chat::absorb`, the same bookkeeping in `took`.
///
/// A channel answered whole leaves the sweep's dirty set, so the first
/// pass does not fetch it a second time; one answered with `more` stays,
/// and the sweep collects the rest. An exchange from before sqex 0.70.0 (SIP-47 §Catching up in one round trip) refuses
/// the route and nothing changes: the sweep runs as it always did.
async fn catch_up(chat: &mut Chat, state: &watch::Sender<ChatState>, desk: &mut Desk, me: PubKey) {
    let named = match chat.named_for_catchup() {
        Ok(named) if !named.is_empty() => named,
        _ => return,
    };
    let answer = match chat.catchup(&named, CATCHUP_BUDGET).await {
        Ok(answer) => answer,
        Err(sqex_chat::client::ChatError::NoChatHere(_)) => {
            tracing::debug!("the exchange does not catch up (before SIP-47); polling instead");
            return;
        }
        Err(e) => {
            tracing::warn!("catch-up failed, polling instead: {e}");
            return;
        }
    };
    let open = desk.open;
    let mut accepted: Vec<([u8; 32], u64)> = Vec::new();
    let mut whole = 0usize;
    for caught in answer.caught {
        let Some(fetched) = caught.fetched else {
            continue;
        };
        // What `refresh` does before a poll, for the same reason: `absorb`
        // appends to the timeline it is handed.
        ensure_folded(&*chat, desk, &caught.channel);
        let Some(known) = desk.channels.get_mut(&caught.channel) else {
            continue;
        };
        let mut timeline = std::mem::take(&mut known.timeline);
        let absorbed = chat.absorb(&mut timeline, fetched).await;
        let Some(known) = desk.channels.get_mut(&caught.channel) else {
            continue;
        };
        match absorbed {
            Ok(conversation) => {
                if let Some(seq) = took(
                    chat.store(),
                    chat.me,
                    known,
                    caught.channel,
                    open,
                    conversation,
                ) {
                    accepted.push((caught.channel, seq));
                }
                if !caught.more {
                    desk.dirty.remove(&caught.channel);
                    whole += 1;
                }
            }
            Err(_) => {
                known.timeline = timeline;
            }
        }
    }
    tracing::info!(
        named = named.len(),
        whole,
        unnamed = answer.unnamed.len(),
        prekeys = answer.prekeys,
        "caught up in one round trip"
    );
    // The count the exchange just gave us, kept rather than only logged: a
    // pool that has run dry is a fact about this device somebody can act on,
    // and a log line is not somewhere anybody looks.
    desk.prekeys = Some(answer.prekeys);
    desk.answered.extend(accepted);
    let _ = publish(chat, state, desk, me);
}

/// Re-read the roster of each channel somebody else's membership event
/// named, as `sync_channels` reads it, and nothing else of the channel.
async fn refresh_rosters(chat: &mut Chat, desk: &mut Desk) {
    let stale: Vec<[u8; 32]> = desk.roster_dirty.drain().collect();
    for channel in stale {
        let Ok(info) = chat.info(&channel).await else {
            continue;
        };
        let Some(k) = desk.channels.get_mut(&channel) else {
            continue;
        };
        k.admins = info
            .members
            .iter()
            .filter(|mem| mem.role == Role::Admin)
            .map(|mem| mem.account)
            .collect();
        k.members = info
            .members
            .iter()
            .map(|mem| Member {
                account: mem.account,
                admin: mem.role == Role::Admin,
                muted: false,
            })
            .collect();
        desk.dirty.insert(channel);
    }
}

async fn refresh_blocked(chat: &mut Chat, state: &watch::Sender<ChatState>) {
    match chat.blocked().await {
        Ok(blocked) => state.send_modify(|s| s.blocked = blocked),
        Err(e) => trouble(state, e),
    }
}

/// SIP-42, one tick's worth: step the sync that is running, or open toward
/// the siblings that are due. Returns whether something on screen moved.
async fn sync_siblings(chat: &mut Chat, state: &watch::Sender<ChatState>, desk: &mut Desk) -> bool {
    use sqex_chat::sync::Phase;
    let now = std::time::Instant::now();
    // A sync under way: one step, and the reckoning when it ends.
    if let Some(live) = desk.siblings.live.as_mut() {
        let more = match live.sync.step(chat, &mut live.link).await {
            Ok(more) => more,
            Err(e) => {
                tracing::warn!(peer = %live.peer, "sync with the other device ended: {e}");
                false
            }
        };
        if more {
            return false;
        }
        let Some(mut over) = desk.siblings.over(now) else {
            return false;
        };
        over.link.close().await;
        let p = &over.sync.progress;
        match over.sync.phase() {
            Phase::Finished => {
                tracing::info!(
                    peer = %over.peer,
                    took = p.entries_in,
                    keys = p.keys_in,
                    files = p.blobs_in,
                    gave = p.entries_out,
                    "synced with the other device"
                );
                // What arrived is in the store, below the cursor, where no
                // poll will find it: the conversations it touched are folded
                // again from the disc, as they were on the way in. Read on
                // the other device, so not unread here.
                for channel in &p.channels_in {
                    if let Some(known) = desk.channels.get_mut(channel) {
                        let admins = if known.admins.is_empty() {
                            chat.store()
                                .channels()
                                .ok()
                                .and_then(|all| all.into_iter().find(|c| c.channel == *channel))
                                .map(|c| c.admins)
                                .unwrap_or_default()
                        } else {
                            known.admins.clone()
                        };
                        known.timeline = chat.history(channel, &admins).unwrap_or_default();
                        // Folded here whether or not anything had folded it
                        // before: a sibling's entries land *below* the poll
                        // cursor, which is why this re-folds rather than
                        // polls.
                        known.folded = true;
                        known.seen = known.timeline.messages().count();
                        known.last_at = known
                            .timeline
                            .messages()
                            .last()
                            .map(|m| m.posted)
                            .unwrap_or(known.last_at);
                    }
                    desk.dirty.insert(*channel);
                }
                desk.restructure = true;
                if let Some(said) = crate::siblings::said(p) {
                    note(state, said);
                    return true;
                }
            }
            _ => tracing::warn!(
                peer = %over.peer,
                "sync with the other device did not finish: {}",
                over.sync.why.as_deref().unwrap_or("no reason given")
            ),
        }
        return false;
    }
    // Nothing running: who the siblings are, then an open toward each due.
    if desk.siblings.list_due(now) {
        let this = chat.device();
        match chat.my_devices().await {
            Ok(devices) => {
                let siblings = devices
                    .into_iter()
                    .map(|d| d.device)
                    .filter(|d| *d != this)
                    .collect();
                desk.siblings.listed(siblings, now);
            }
            Err(e) => tracing::debug!("could not list this account's devices: {e}"),
        }
    }
    for (sibling, ephemeral) in desk.siblings.to_open(now) {
        match chat.meet_sibling(&ephemeral, &sibling).await {
            Ok(Some((link, session))) => {
                tracing::info!(peer = %sibling, "met the other device; syncing history");
                let sync = sqex_chat::sync::Sync::new(session, sibling);
                desk.siblings.met(sibling, sync, link, now);
                break;
            }
            Ok(None) => {}
            Err(e) => {
                tracing::debug!(peer = %sibling, "could not open toward the other device: {e}")
            }
        }
    }
    false
}

/// Re-read who this account's devices are, and whether we are still one.
async fn refresh_devices(chat: &mut Chat, state: &watch::Sender<ChatState>, desk: &mut Desk) {
    // Whoever the siblings are now, SIP-42 opens toward them next tick.
    desk.siblings.relist();
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
            // Said on the card as well as in the general place: a list that
            // stays empty because nobody answered must not read as a list
            // that is empty, and "asking..." for ever is its own untruth.
            let why = e.to_string();
            trouble(state, e);
            state.send_modify(|s| s.devices_trouble = Some(why));
            return;
        }
    };
    let linked = chat.still_linked().await.ok().flatten();
    state.send_modify(|s| {
        s.devices = devices;
        s.devices_known = true;
        s.devices_trouble = None;
        s.linked = linked;
    });
}

/// The longest edge of a thumbnail, in pixels.
///
/// It rides inside the message, which is capped, so this has to stay small
/// enough that a photograph does not push the post over the limit on its own.
///
/// **As large as the cap allows, not as small as it is sure to.** At 96 it
/// always fitted and always looked it: a phone draws a clip's thumbnail over
/// seven hundred device pixels, and 96 of them stretched that far is the blur
/// somebody sees before they press play. `thumbnail_of` starts here and steps
/// down until one fits, so a picture that compresses well keeps this size and
/// a busy one still gets something.
const THUMBNAIL_EDGE: u32 = 192;

/// A small picture of an image file, to carry inside the message.
///
/// `None` for anything that will not decode. A missing thumbnail is ordinary —
/// SIP-18 makes the field optional and every reader has to cope with an empty
/// one — so a file that cannot be previewed is still sent.
///
/// # Sized to the limit, and what happened before it was
///
/// SIP-18 caps a preview at [`MAX_PREVIEW`] and says, in so many words, that
/// **a client sizes previews to what it is actually sending**. This did not:
/// it made a 96-pixel PNG and sent whatever that came to. For a photograph
/// that is a few kilobytes; for a busy picture -- a dithered GIF frame, a
/// screenshot full of text -- a lossless PNG of 96×96 is twelve. Every reader
/// refused the whole post, and reported it as "not opened yet, its key may
/// still arrive" -- in a public channel, where nothing has a key. Two
/// pictures sat in `general` as two unreadable messages for everybody.
///
/// So it is tried at falling cost until one fits: PNG first, because it keeps
/// transparency and a smooth picture is small anyway; then JPEG, which is what
/// a busy picture compresses under; then smaller. A picture that fits none of
/// them goes without, which the protocol allows and a reader survives.
/// SIP-18's `meta` for a picture or a video: width and height as u16 big
/// endian, then for a video its length in milliseconds as u32.
/// Upload one file to the conversation and describe it: the attachment a
/// message carries, with the shape and the thumbnail a reader draws before
/// -- or instead of -- fetching it.
async fn attach_file(
    chat: &mut Chat,
    state: &watch::Sender<ChatState>,
    channel: &[u8; 32],
    path: &std::path::Path,
) -> Result<sqex_proto::blob::Attachment, String> {
    // **Asked, never assumed.** SIP-18 says a client discovers the chunk
    // size from the exchange, and a client that guessed 256 KiB against one
    // on the uniform 64 KiB cap fails its first Put with nothing explaining
    // why.
    let limits = chat.blob_limits().await.map_err(|e| e.to_string())?;
    let prepared = chat
        .prepare_file(path, limits.chunk as usize)
        .map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    note(state, format!("Sending {name}…"));
    let mut attachment = chat
        .upload(channel, &prepared)
        .await
        .map_err(|e| e.to_string())?;
    // The field SIP-18 has always had and the terminal client always left
    // empty: "rendering one means decoding the image, and a terminal client
    // has nothing to show it on. The field exists for a client that does."
    // This is that client.
    //
    // It travels **inside the sealed message**, so it is no more visible to
    // the exchange than the picture is -- and it is what a reader sees
    // before the blob has been fetched, or instead of it when the blob is
    // too big to fetch unasked.
    if let Some((meta, preview)) = preview_of(path, attachment.effective_kind()) {
        attachment.meta = meta;
        attachment.preview = preview;
    }
    Ok(attachment)
}

/// A file's shape (and a clip's length) as SIP-18 meta, and its thumbnail,
/// by decoding it -- a picture whole, a clip's first frame the same way it
/// will be played. `None` for a kind that has no picture to show.
pub fn preview_of(path: &std::path::Path, kind: u8) -> Option<(Vec<u8>, Vec<u8>)> {
    match kind {
        sqex_proto::blob::KIND_IMAGE => {
            let image = image::ImageReader::open(path).ok()?.decode().ok()?;
            Some((
                shape_meta(image.width(), image.height(), None),
                thumbnail_of(&image).unwrap_or_default(),
            ))
        }
        sqex_proto::blob::KIND_VIDEO => {
            let bytes = std::fs::read(path).ok()?;
            let (described, first) = sigil_video::still(bytes.into()).ok()?;
            Some((
                shape_meta(
                    described.width,
                    described.height,
                    Some(described.duration_ms),
                ),
                thumbnail_of(&frame_image(&first)).unwrap_or_default(),
            ))
        }
        // **A voice note has no picture and is not exempt.** SIP-18 gives
        // this kind a meta of its own -- `duration_ms | bars | level ×
        // bars` -- and says why: "A voice note's `bars` are its waveform,
        // so it draws before any audio is fetched." A note sent without it
        // is a row of nothing at the far end until somebody fetches it.
        sqex_proto::blob::KIND_VOICE => {
            let bytes = std::fs::read(path).ok()?;
            let decoded = sigil_video::note::decode(&bytes).ok()?;
            Some((voice_meta(&decoded), Vec::new()))
        }
        _ => None,
    }
}

/// SIP-18's meta for a voice note: `duration_ms: u32 | bars: u8 | level ×
/// bars`, the levels in **SIP-15's scale**.
///
/// The scale is not a coincidence and not ours to choose: the spec reuses
/// a live call's so that "one function in a client renders both a live
/// call and a voice note" -- so this measures with the same
/// [`sqex_voice::media::Comfort`] a call's meter does rather than
/// re-deriving half-decibels here.
fn voice_meta(decoded: &sigil_video::note::Decoded) -> Vec<u8> {
    /// Enough to read as speech at a phone's bubble width and small enough
    /// that the meta is a rounding error against the entry cap: 48 bars is
    /// 53 bytes.
    const BARS: usize = 48;
    let channels = decoded.channels.max(1);
    let frames = decoded.samples.len() / channels;
    let bars = BARS.min(frames.max(1));
    let mut meta = Vec::with_capacity(5 + bars);
    meta.extend_from_slice(&(decoded.duration_ms().min(u32::MAX as u64) as u32).to_be_bytes());
    meta.push(bars as u8);
    let mut window: Vec<f32> = Vec::new();
    for i in 0..bars {
        let from = i * frames / bars;
        let to = (((i + 1) * frames / bars).max(from + 1)).min(frames);
        window.clear();
        // Averaged across channels rather than taking the first: SIP-18
        // says a note SHOULD be mono, and a sender that ignored that must
        // not have half its sound silently dropped from the picture of it.
        window.extend((from..to).map(|f| {
            let at = f * channels;
            decoded.samples[at..at + channels].iter().sum::<f32>() / channels as f32
        }));
        meta.push(sqex_voice::media::Comfort::measure(&window).level);
    }
    meta
}

/// Read SIP-18's voice meta back: how long the note runs, and its bars.
///
/// Through sqex-proto's own accessors rather than a second parser here, so
/// a note drawn in the composer before it is sent is drawn from the same
/// bytes, read the same way, as the note the far end will draw. `None`
/// when the meta is too short to hold either -- a length with no bars is a
/// note that was measured and has nothing to show.
pub fn voice_facts(meta: Vec<u8>) -> Option<(u64, Vec<u8>)> {
    let a = sqex_proto::blob::Attachment {
        kind: sqex_proto::blob::KIND_VOICE,
        blob: [0; 32],
        key: [0; 32],
        size: 0,
        chunks: 0,
        mime: String::new(),
        meta,
        preview: Vec::new(),
    };
    Some((a.duration_ms()? as u64, a.waveform()?.to_vec()))
}

fn shape_meta(width: u32, height: u32, duration_ms: Option<u64>) -> Vec<u8> {
    let mut m = Vec::with_capacity(8);
    m.extend_from_slice(&(width.min(u16::MAX as u32) as u16).to_be_bytes());
    m.extend_from_slice(&(height.min(u16::MAX as u32) as u16).to_be_bytes());
    if let Some(ms) = duration_ms {
        m.extend_from_slice(&(ms.min(u32::MAX as u64) as u32).to_be_bytes());
    }
    m
}

/// A decoded frame as the image crate sees it, so the same thumbnail code
/// serves pictures and videos.
fn frame_image(frame: &egui::ColorImage) -> image::DynamicImage {
    let [w, h] = frame.size;
    let rgba: Vec<u8> = frame.pixels.iter().flat_map(|p| p.to_array()).collect();
    image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .map(image::DynamicImage::ImageRgba8)
        .unwrap_or_default()
}

/// [`thumbnail`] from a decoded image, so it can be tested on one built in
/// memory rather than on a file.
fn thumbnail_of(image: &image::DynamicImage) -> Option<Vec<u8>> {
    use image::ImageFormat;
    // Lossless and with alpha first; then lossy, then smaller and lossy. A
    // JPEG has no alpha, so it is encoded from the colour channels alone --
    // the encoder refuses RGBA outright rather than dropping the channel.
    // Lossless and with alpha first *for a picture that has any* -- a JPEG
    // has none, and the encoder refuses RGBA outright rather than dropping
    // the channel -- and then the ladder, biggest first. A picture with
    // nothing transparent skips the PNG: at this size it is several times
    // the cap, and its only purpose is the alpha.
    let alpha = image.color().has_alpha();
    let ladder = [
        (THUMBNAIL_EDGE, ImageFormat::Jpeg, Some(72)),
        (THUMBNAIL_EDGE * 2 / 3, ImageFormat::Jpeg, Some(72)),
        (THUMBNAIL_EDGE / 2, ImageFormat::Jpeg, Some(75)),
        (THUMBNAIL_EDGE / 3, ImageFormat::Jpeg, Some(70)),
        (THUMBNAIL_EDGE / 4, ImageFormat::Jpeg, Some(60)),
    ];
    let attempts: Vec<(u32, ImageFormat, Option<u8>)> = alpha
        .then_some((THUMBNAIL_EDGE / 2, ImageFormat::Png, None))
        .into_iter()
        .chain(ladder)
        .collect();
    for (edge, format, quality) in attempts {
        let small = image.thumbnail(edge, edge);
        let mut out = std::io::Cursor::new(Vec::new());
        let written = match (format, quality) {
            (ImageFormat::Jpeg, Some(q)) => {
                let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q);
                small.to_rgb8().write_with_encoder(encoder).is_ok()
            }
            _ => small.write_to(&mut out, format).is_ok(),
        };
        let bytes = out.into_inner();
        if written && bytes.len() <= MAX_PREVIEW {
            return Some(bytes);
        }
    }
    None
}

/// SIP-18's cap on a preview, re-said here so the sender and the reader agree
/// by construction: the reader refuses anything over it.
const MAX_PREVIEW: usize = sqex_proto::blob::MAX_PREVIEW;

#[cfg(test)]
mod thumbnail_tests {
    use super::{MAX_PREVIEW, thumbnail_of};

    /// A picture that does not compress: every pixel different, no runs.
    /// A lossless 96×96 of this is well over the cap, which is what the two
    /// pictures in `general` were.
    fn noise() -> image::DynamicImage {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(400, 300, |_, _| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let b = seed.to_le_bytes();
            image::Rgba([b[0], b[1], b[2], 255])
        }))
    }

    /// The preview of a busy picture fits the protocol's cap.
    #[test]
    fn a_busy_picture_gets_a_preview_that_fits() {
        let preview = thumbnail_of(&noise()).expect("some preview of it");
        assert!(
            preview.len() <= MAX_PREVIEW,
            "the preview is {} bytes against a cap of {MAX_PREVIEW}, and every \
             reader will refuse the whole message",
            preview.len()
        );
    }

    /// And the PNG alone would not have: this is the case that was shipped.
    #[test]
    fn the_png_alone_is_over_the_cap_for_a_busy_picture() {
        let small = noise().thumbnail(super::THUMBNAIL_EDGE, super::THUMBNAIL_EDGE);
        let mut out = std::io::Cursor::new(Vec::new());
        small.write_to(&mut out, image::ImageFormat::Png).unwrap();
        assert!(
            out.into_inner().len() > MAX_PREVIEW,
            "the fixture is not busy enough to reproduce the fault"
        );
    }

    /// A smooth picture keeps its lossless preview, transparency and all.
    #[test]
    fn a_smooth_picture_keeps_a_png_preview() {
        let smooth =
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(400, 300, |x, _| {
                image::Rgba([x as u8, 40, 90, 128])
            }));
        let preview = thumbnail_of(&smooth).expect("a preview");
        assert!(preview.len() <= MAX_PREVIEW);
        assert!(
            preview.starts_with(&[0x89, b'P', b'N', b'G']),
            "a smooth picture should not have needed to give up PNG"
        );
    }
}

/// The largest file fetched without being asked for.
///
/// An image is worth pulling so a conversation reads as a conversation; a
/// video is not, and neither is a large photograph on a metered connection.
/// Everything above this waits to be asked for -- the picture offers Fetch,
/// and Save always fetches.
///
/// **Twenty-five, not four.** A gif is a picture that moves, and an ordinary
/// one is four to eight megabytes; at four the thumbnail of every one of them
/// sat under a caption that said "fetching" for ever. Twenty-five is the
/// user's number, and is under what the store keeps (`BLOB_KEEP_MAX`), so a
/// picture fetched unasked is a picture fetched once.
///
/// The cap is about the network, so it does not apply to a file that is
/// already on the disc: this session's own upload, or one fetched last time.
const AUTO_FETCH_MAX: u64 = 25 * 1024 * 1024;

/// Whether a picture is worth fetching without being asked: small enough, or
/// wanted anyway, or already here.
fn fetch_unasked(size: u64, wanted: bool, on_disc: bool) -> bool {
    size <= AUTO_FETCH_MAX || wanted || on_disc
}

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
///
/// Raised from sixty-four when videos joined the pictures: one is forty
/// megabytes, and two of them in a budget of sixty-four put every picture
/// in the conversation down.
const HOLD_BYTES: usize = 256 * 1024 * 1024;

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
/// What a pass of [`fetch_files`] did.
struct Fetching {
    /// Pictures still waiting, so the next pass should come round at once.
    more: bool,
    /// A picture arrived, so the state wants publishing **now**. It used to
    /// wait for the next refresh to notice -- five seconds, on the backstop,
    /// for the last picture in a pass -- after a fetch of a third of one.
    landed: bool,
}

async fn fetch_files(
    chat: &mut Chat,
    state: &watch::Sender<ChatState>,
    desk: &mut Desk,
    cmds: &mut mpsc::UnboundedReceiver<Cmd>,
) -> Fetching {
    let none = Fetching {
        more: false,
        landed: false,
    };
    let Some(open) = desk.open else { return none };
    let Some(known) = desk.channels.get(&open) else {
        return none;
    };
    // Each with whether it is already on the disc, which decides both
    // whether it is fetched unasked and how many of them one pass takes.
    let wanted: Vec<(sqex_proto::blob::Attachment, bool)> = known
        .timeline
        .messages()
        .flat_map(|m| m.post.attachments())
        .filter(|a| !desk.files.contains_key(&a.blob) && !held_back(desk, &a.blob))
        .map(|a| (a, chat.store().has_blob(&a.blob).unwrap_or(false)))
        .filter(|(a, on_disc)| match a.effective_kind() {
            sqex_proto::blob::KIND_IMAGE => {
                fetch_unasked(a.size, desk.wanted.contains(&a.blob), *on_disc)
            }
            // **A video only when play is pressed.** Never for its size, and
            // not for being on the disc either: forty megabytes read and
            // opened into memory for a conversation somebody scrolled past
            // is not a picture on the way, and the press is what starts it.
            // This filter used to admit images only, and a pressed video
            // said "fetching" for ever.
            sqex_proto::blob::KIND_VIDEO => desk.wanted.contains(&a.blob),
            _ => false,
        })
        .map(|(a, on_disc)| (a.clone(), on_disc))
        .collect();
    if wanted.is_empty() {
        return none;
    }

    // **Everything on the disc, then one from the exchange.**
    //
    // A picture already here costs a read and an open -- milliseconds --
    // and there is nothing to wait behind, so a conversation opened for the
    // second time gets all of its pictures in one pass rather than one per
    // pass with a wait between. One from the network a pass, because that
    // one takes as long as it takes -- one was measured at two and a half
    // seconds -- and the whole of that is time the task cannot answer
    // anybody in; the caller comes straight back round for the next.
    let mut landed = false;
    let mut land = |desk: &mut Desk, blob: [u8; 32], bytes: Vec<u8>| {
        if desk.files.insert(blob, bytes.into()).is_none() {
            desk.fetched.push(blob);
        }
        put_down_what_is_not_wanted(desk);
        landed = true;
    };
    let mut from_the_exchange = Vec::new();
    for (a, on_disc) in wanted {
        if !on_disc {
            from_the_exchange.push(a);
            continue;
        }
        // Named as on the disc and not readable after all: the store has
        // already put it down, so the next pass asks the exchange.
        if let Ok(bytes) = chat.download(&a).await {
            land(desk, a.blob, bytes);
        }
    }
    let waiting = from_the_exchange.len();
    let Some(a) = from_the_exchange.into_iter().next() else {
        return Fetching {
            more: false,
            landed,
        };
    };
    // And not at all while somebody is waiting for something. A reader who has
    // moved on should not be behind a picture for a conversation that is
    // already on the disc.
    attend(chat, state, desk, cmds).await;
    if !cmds.is_empty() {
        return Fetching { more: true, landed };
    }
    match chat.download(&a).await {
        Ok(bytes) => land(desk, a.blob, bytes),
        // **Ask what actually happened.** A fetch that failed because the
        // blob is gone and one that failed because the link blinked arrive
        // here as the same `Err`, and treating them alike meant a picture
        // lost to one dropped packet read as "no longer at the exchange" for
        // the rest of the session. `/blob/head` is the difference (SIP-18).
        Err(_) => {
            let head = chat.head(&a.blob).await.ok().map(|h| h.found);
            let trouble = after_a_failed_fetch(head, std::time::Instant::now());
            desk.unfetchable.insert(a.blob, trouble);
        }
    }
    Fetching {
        more: waiting > 1,
        landed,
    }
}

/// A first frame for one clip that is already on this device, as a picture
/// the interface can draw instead of the sender's thumbnail.
///
/// **Read, decoded, and put down again.** A video is not fetched for being
/// scrolled past -- forty megabytes for a clip somebody passed is not a
/// picture on the way -- and this does not change that: it only opens what
/// the store already holds, keeps a JPEG of a few tens of kilobytes, and
/// drops the rest. One a pass, and never the same one twice; the decode is
/// a blocking job, so it goes where blocking jobs go.
///
/// Returns whether anything was made, which is a reason to publish.
async fn still_for_a_clip(chat: &mut Chat, desk: &mut Desk) -> bool {
    let Some(open) = desk.open else { return false };
    let Some(known) = desk.channels.get(&open) else {
        return false;
    };
    let want = known
        .timeline
        .messages()
        .flat_map(|m| m.post.attachments())
        .find(|a| {
            a.effective_kind() == sqex_proto::blob::KIND_VIDEO
                && !desk.stills.contains_key(&a.blob)
                && !desk.no_still.contains(&a.blob)
                && chat.store().has_blob(&a.blob).unwrap_or(false)
        })
        .cloned();
    let Some(a) = want else { return false };
    let blob = a.blob;
    let Ok(bytes) = chat.download(&a).await else {
        // On the disc a moment ago and not readable now: the store has put
        // it down. Nothing is wrong, and nothing is to be done about it.
        desk.no_still.insert(blob);
        return false;
    };
    let made = tokio::task::spawn_blocking(move || still_jpeg(&bytes))
        .await
        .ok()
        .flatten();
    match made {
        Some(jpeg) => {
            desk.stills.insert(blob, jpeg.into());
            true
        }
        None => {
            desk.no_still.insert(blob);
            false
        }
    }
}

/// A clip's first frame as a JPEG, at the size a bubble draws one.
///
/// Nine hundred and sixty pixels on the long edge: the width a picture is
/// drawn at on the widest pane sigil has, doubled for what a phone's screen
/// has per point. `None` for anything that will not decode.
fn still_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    const EDGE: u32 = 960;
    let (_, frame) = sigil_video::still(bytes.into()).ok()?;
    let [w, h] = [frame.width() as u32, frame.height() as u32];
    if w == 0 || h == 0 {
        return None;
    }
    let mut rgb = Vec::with_capacity(w as usize * h as usize * 3);
    for pixel in frame.pixels.iter() {
        rgb.extend_from_slice(&[pixel.r(), pixel.g(), pixel.b()]);
    }
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, rgb)?);
    let small = image.thumbnail(EDGE, EDGE);
    let mut out = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 80);
    small.to_rgb8().write_with_encoder(encoder).ok()?;
    Some(out.into_inner())
}

/// Whether the backup at the exchange is old enough to write again.
///
/// A free function over plain data, because *when* is the part that can be
/// got wrong and a live exchange is the one thing a test cannot arrange.
/// Nothing held at all -- a key made and never used -- is due at once: that
/// is an account that meant to have a backup and has none.
fn backup_due(generation: u64, written: u64, now: u64) -> bool {
    generation == 0 || now.saturating_sub(written) >= BACKUP_EVERY.as_secs()
}

/// How often a backup is written again, for an account that has one.
///
/// A day. The backup is incremental -- what has not changed is kept, not
/// uploaded again -- so this costs what was said since, and a phone that is
/// lost loses at most a day.
const BACKUP_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// How long after the session starts the first one is considered, so a
/// launch is not a backup and the link has settled.
const BACKUP_AFTER_START: std::time::Duration = std::time::Duration::from_secs(120);

/// SIP-48: write the backup again when the one at the exchange is a day old.
///
/// **Only for an account that has already chosen to have one.** The key is
/// the 24 words somebody wrote down; without one there is nothing to write
/// with, and making one unasked would be making a secret on somebody's
/// behalf. With one, a backup nobody ever writes again is a backup of the
/// day it was made -- which is the shape almost every lost phone is in.
///
/// Quiet: it says nothing on success, because this is not something anybody
/// asked for just now, and the Devices card shows the generation and the
/// time. A failure is a log line; the next day comes round.
async fn keep_the_backup_fresh(chat: &mut Chat, state: &watch::Sender<ChatState>, desk: &mut Desk) {
    if chat.link() != Link::Up {
        return;
    }
    if desk.started.elapsed() < BACKUP_AFTER_START {
        return;
    }
    if let Some(when) = desk.backup_tried
        && when.elapsed() < BACKUP_EVERY
    {
        return;
    }
    let Ok(Some(key)) = chat.backup_key() else {
        return;
    };
    // What the exchange holds now: the one fact that decides whether this is
    // due, and it is a request, so it is made once a day and not every tick.
    desk.backup_tried = Some(std::time::Instant::now());
    let me = chat.me;
    let held = match chat.backup_held(&me).await {
        Ok(held) => held,
        Err(e) => {
            tracing::debug!("the backup's state could not be read: {e}");
            return;
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if !backup_due(held.generation, held.written, now) {
        return;
    }
    match chat.backup(&key).await {
        Ok(done) => {
            tracing::info!(
                "backup kept up to date: generation {}, {} uploaded, {} kept",
                done.generation,
                done.uploaded,
                done.kept
            );
            backup_status(chat, state).await;
        }
        Err(e) => tracing::warn!("the backup could not be written: {e}"),
    }
}

/// SIP-43 §The heads by position: whether the exchange's record of this
/// device's chain disagrees with this device's own.
///
/// `next` and `head` are this store's chain -- the position it would sign
/// next, and the head after the last thing it signed -- and `heads` is what
/// the exchange holds for this device by position.
///
/// **Only a shared position counts.** The exchange being *behind* is the
/// ordinary case a second after posting, and a device that has signed
/// nothing has nothing to disagree about. The one thing that means
/// something is the same position with a different head.
///
/// A free function over plain data: the convention -- the exchange's
/// positions are the position of an entry, this store's `next` is the one
/// after it -- is the part that can be got wrong, and an exchange is the
/// one thing a test cannot arrange.
fn chain_apart(next: u64, head: [u8; 32], heads: &[(u64, [u8; 32])]) -> bool {
    let Some(last) = next.checked_sub(1) else {
        return false;
    };
    heads
        .iter()
        .any(|(at, theirs)| *at == last && *theirs != head)
}

/// Ask the exchange what it holds for this device in the open conversation,
/// once per conversation per session, and say so when it disagrees.
///
/// Once: the answer only changes when this device signs something, and what
/// this is for -- a store that has been rolled back, or a key used on two
/// machines -- is a state, not an event. The cost is one request the first
/// time a conversation is opened with the link up.
async fn check_the_chain(chat: &mut Chat, desk: &mut Desk) -> bool {
    if chat.link() != Link::Up {
        return false;
    }
    let Some(channel) = desk.open else {
        return false;
    };
    if !desk.chain_checked.insert(channel) {
        return false;
    }
    let Ok((next, head)) = chat.store().chain(&channel) else {
        return false;
    };
    let Some(from) = next.checked_sub(1) else {
        return false;
    };
    match chat.chain_heads(&channel, from).await {
        Ok(heads) if chain_apart(next, head, &heads) => {
            tracing::warn!(
                "the exchange holds a different head at position {from} for this device"
            );
            if let Some(known) = desk.channels.get_mut(&channel) {
                known.trouble.chain_apart = true;
            }
            true
        }
        Ok(_) => false,
        Err(e) => {
            // An exchange that predates the route answers nothing, which
            // `chain_heads` already turns into an empty list; anything else
            // is a link that blinked, and the next session asks again.
            tracing::debug!("the chain could not be read: {e}");
            desk.chain_checked.remove(&channel);
            false
        }
    }
}

/// Whether a blob is being left alone: gone for good, or waiting out a retry.
fn held_back(desk: &Desk, blob: &[u8; 32]) -> bool {
    match desk.unfetchable.get(blob) {
        Some(Unfetched::Gone) => true,
        Some(Unfetched::Later(when)) => when.elapsed() < RETRY_AFTER,
        None => false,
    }
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
    let want = wanted_names(desk, chat.me);
    if !want.is_empty() {
        let _ = chat.refresh_profiles(&want, now).await;
    }
}

/// Whose name is worth asking about.
///
/// **A set, not a list searched for every message.** This was a `Vec` with a
/// `contains` per entry, so the conversation on screen cost a scan of everybody
/// already found for each of its messages -- five hundred messages against
/// fifty speakers is twenty-five thousand comparisons, on every pass, to
/// produce a list of fifty.
///
/// A free function over plain data because the rule is the part worth testing:
/// everybody a view could name -- the other party in each direct message, and
/// whoever has said anything in the conversation on screen -- and nobody twice.
/// Not every member of every channel: that is a request per person per rebuild
/// for names nobody is looking at.
fn wanted_names(desk: &Desk, me: PubKey) -> Vec<PubKey> {
    let mut want: HashSet<PubKey> = HashSet::new();
    for known in desk.channels.values() {
        if let Some(peer) = known.peer {
            want.insert(peer);
        }
    }
    if let Some(open) = desk.open
        && let Some(known) = desk.channels.get(&open)
    {
        want.extend(known.timeline.messages().map(|m| m.account));
        // And everybody in it, said anything or not: the mention picker
        // offers the room's members by name, and a member who has never
        // spoken is exactly the one you are trying to get to speak. Bounded
        // by the roster, and only the room on screen.
        want.extend(known.members.iter().map(|m| m.account));
    }
    want.remove(&me);
    want.into_iter().collect()
}

/// Everything we can say about who somebody is, read back out of the store.
/// Another page of the open conversation, from what this machine holds.
///
/// Like [`search_local`], this asks the exchange nothing: the entries are on
/// the disc and the window over them is a number. One copy of it, because
/// both the ordinary loop and the one that runs while the exchange is being
/// dialled answer it -- and a reader at an exchange that never answers is
/// exactly the reader who has time to scroll.
fn reach_earlier(chat: &impl Local, desk: &mut Desk) {
    let Some(channel) = desk.open else {
        return;
    };
    ensure_folded(chat, desk, &channel);
    if let Some(known) = desk.channels.get_mut(&channel) {
        known.wanted += PAGE;
        desk.dirty.insert(channel);
    }
}

/// Search every word this machine holds.
///
/// **Nothing here asks the exchange anything.** The words are on the disc --
/// they have to be, since opening an epoch key spends the prekey it was
/// sealed against -- so this is answered whether or not there is a
/// connection, and it is one of the two things served while one is still
/// being made (see the `Cmd::Show` arm in `connect_as`). A reader who opens
/// the window and types into the search box before DNS has answered gets
/// their answer.
fn search_local(
    chat: &impl Local,
    desk: &mut Desk,
    state: &watch::Sender<ChatState>,
    me: PubKey,
    query: &str,
) {
    let needle = query.trim().to_lowercase();
    let mut hits = Vec::new();
    if !needle.is_empty() {
        // **Everything this machine holds, not everything it has
        // looked at.** A conversation nobody has opened this session
        // holds a preview line and not its words, and a search that
        // skipped it would answer "nothing" about something sitting
        // on the disc. The sweep folds them all within a few ticks of
        // the link coming up; this is what makes that true for a
        // client that never connects.
        let unfolded: Vec<[u8; 32]> = desk
            .channels
            .iter()
            .filter(|(_, k)| !k.folded)
            .map(|(c, _)| *c)
            .collect();
        for channel in unfolded {
            ensure_folded(chat, desk, &channel);
        }
        // A hit names the conversation it was found in, and a direct
        // message's name is a person's -- so it is resolved the same
        // way the list resolves it, or a search would be the one place
        // still showing a whole key.
        let people = people_of(chat, desk);
        for (channel, known) in &desk.channels {
            for m in known.timeline.messages() {
                if m.redacted {
                    continue;
                }
                let text = m.post.body_text().unwrap_or_default();
                let Some(found) = find_ignoring_case(text, &needle) else {
                    continue;
                };
                hits.push(Hit {
                    channel: *channel,
                    seq: m.seq,
                    label: match known.peer {
                        Some(peer) => name_for(&people, &peer, &known.label),
                        None => known.label.clone(),
                    },
                    // Named as the transcript names them, and ours
                    // as the reader's own: "You" is what a bubble on
                    // the right-hand side says without a name.
                    who: if m.account == me {
                        "You".to_string()
                    } else {
                        people
                            .get(&m.account)
                            .and_then(|p| p.name.clone())
                            .unwrap_or_else(|| short(&m.account))
                    },
                    text: text.to_string(),
                    found,
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

fn people_of(chat: &impl Local, desk: &Desk) -> HashMap<PubKey, Person> {
    let mut out = HashMap::new();
    let look = |account: PubKey, out: &mut HashMap<PubKey, Person>| {
        out.entry(account).or_insert_with(|| Person {
            name: chat.display_name(&account),
            title: chat.title_of(&account),
            handle: chat.handle(&account),
            picture: chat.avatar_of(&account).and_then(Face::of),
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
        // Straight from the fold. This collected every author into a `Vec`
        // first -- one allocation the length of the conversation, per pass,
        // for a lookup that already dedupes.
        for m in known.timeline.messages() {
            look(m.account, &mut out);
        }
        for m in &known.members {
            look(m.account, &mut out);
        }
    }
    out
}

/// Build what the interface draws from what the task holds.
/// SIP-39 §The peer directory: ask the exchange what it federates with. An exchange from
/// before the directory answers 404, which is an empty directory here.
async fn read_peers(chat: &mut Chat, desk: &mut Desk) -> bool {
    let Some(mut client) = chat.connection() else {
        return false;
    };
    let peers = match client.get("/exchange/peers").await {
        Ok((200, body)) => sqex_proto::exchange::Peers::decode(&body)
            .map(|p| {
                p.peers
                    .into_iter()
                    .map(|e| (e.key, e.domain))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    if desk.peers != peers {
        desk.peers = peers;
        true
    } else {
        false
    }
}

/// Beat (SIP-4): this identity is here, or here and away. On the
/// session's own connection, which carries the identity (SIP-3), so the
/// exchange records it against us without anything signed.
///
/// An exchange from before the away bit refuses a beat that carries it
/// as reserved; that one is beaten to again without it and from then on
/// plainly, and the people reading it see active or absent only.
async fn beat(chat: &mut Chat, desk: &mut Desk) {
    let Some(mut client) = chat.connection() else {
        return;
    };
    let away = desk.away && !desk.beats_plainly;
    let beat = sqex_proto::beacon::Beat {
        interval_secs: crate::presence::BEAT_SECS,
        withhold: false,
        away,
    };
    desk.beat_at = Some(std::time::Instant::now());
    match client.post("/beacon/beat", beat.encode()).await {
        Ok((400, _)) if away => {
            desk.beats_plainly = true;
            let plain = sqex_proto::beacon::Beat {
                away: false,
                ..beat
            };
            let _ = client.post("/beacon/beat", plain.encode()).await;
        }
        _ => {}
    }
}

/// Ask about a few of the people we talk to (SIP-4 read), and remember
/// what the exchange said. Returns whether anything changed.
async fn read_presence(chat: &mut Chat, desk: &mut Desk, me: PubKey) -> bool {
    let dm_peers = desk.channels.values().filter_map(|k| k.peer);
    let open_members = desk
        .open
        .and_then(|c| desk.channels.get(&c))
        .map(|k| k.members.iter().map(|m| m.account).collect::<Vec<_>>())
        .unwrap_or_default();
    let wanted = crate::presence::wanted(Some(me), dm_peers, open_members);
    let now = std::time::Instant::now();
    let due = crate::presence::due(&wanted, &desk.asked_at, now);
    if due.is_empty() {
        return false;
    }
    let Some(mut client) = chat.connection() else {
        return false;
    };
    let mut moved = false;
    for key in due {
        desk.asked_at.insert(key, now);
        let read = sqex_proto::beacon::Read { key };
        let Ok((200, body)) = client.post("/beacon/read", read.encode()).await else {
            continue;
        };
        let Ok(reply) = sqex_proto::beacon::Reply::decode(&body) else {
            continue;
        };
        let found = crate::presence::Presence::of(&reply);
        if desk.presence.insert(key, found) != Some(found) {
            moved = true;
        }
    }
    // Somebody no longer talked to is no longer asked about, or read.
    desk.presence.retain(|k, _| wanted.contains(k));
    desk.asked_at.retain(|k, _| wanted.contains(k));
    moved
}

/// A key this person verified whose handle, as this client knows it, is
/// `name` -- and is not `resolved`. The one case a name must not be
/// followed (SIP-41).
fn verified_holder_of(chat: &Chat, name: &str, resolved: &PubKey) -> Option<PubKey> {
    let verified = chat
        .store()
        .verified()
        .unwrap_or_default()
        .into_iter()
        .map(|(k, _)| k);
    verified_holder(name, resolved, verified, |k| chat.handle(k))
}

/// The rule, over plain data: among the keys verified, one that is not
/// what the name resolved to today and whose handle is that name.
pub fn verified_holder(
    name: &str,
    resolved: &PubKey,
    verified: impl IntoIterator<Item = PubKey>,
    handle_of: impl Fn(&PubKey) -> Option<String>,
) -> Option<PubKey> {
    let wanted = name.trim().to_lowercase();
    verified
        .into_iter()
        .filter(|k| k != resolved)
        .find(|k| handle_of(k).is_some_and(|h| h.to_lowercase() == wanted))
}

/// One timeline's messages as the interface draws them.
///
/// **Two callers, and they are not the same conversation.** The live one is
/// windowed to what somebody has asked for and carries this device's read
/// receipts. An earlier copy (SIP-60 §The client keeps what it read) is
/// whole, has no receipts -- the channel it belongs to does not exist any
/// more, so nobody's cursor is in it -- and is marked `earlier`, which is
/// what stops the interface offering to reply to a message that can never
/// be replied to.
#[allow(clippy::too_many_arguments)]
fn lines_of(
    timeline: &Timeline,
    window: usize,
    receipts: Option<&Known>,
    earlier: bool,
    me: PubKey,
    people: &HashMap<PubKey, Person>,
    desk: &Desk,
    chat: &impl Local,
) -> Vec<Line> {
    // A stub of what each message says, so a reply can name it. Built
    // once rather than searched per reply: a conversation full of
    // replies would otherwise be quadratic in its own length.
    // `broken` is a list rather than a field on the message: the
    // fold keeps it separate so that a client which ignores it still
    // shows the message, which is right for a gap and would be wrong
    // for a fork. Turned into a lookup once, not searched per line.
    let standing: HashMap<u64, Standing> = timeline
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

    // A stub of what each message says, so a reply can name it. Only
    // for what a reply in the window actually points at, and looked up
    // in the fold rather than built for every message that ever
    // existed -- which is precisely the work the window exists to
    // avoid doing.
    let stubs: HashMap<u64, Stub> = timeline
        .messages()
        .skip(window)
        .filter_map(|m| m.post.reply_to())
        .filter_map(|target| timeline.get(target))
        .map(|m| {
            let pictures: Vec<_> = m
                .post
                .attachments()
                .filter(|a| {
                    let kind = a.effective_kind();
                    kind == sqex_proto::blob::KIND_IMAGE || kind == sqex_proto::blob::KIND_VIDEO
                })
                .collect();
            let words = m.post.body_text().unwrap_or_default();
            let said = if m.redacted {
                "deleted".to_string()
            } else if !words.is_empty() {
                stub(words)
            } else {
                only_files(m.post.attachments().map(|a| a.effective_kind()))
            };
            let preview = (!m.redacted)
                .then(|| pictures.first())
                .flatten()
                .filter(|a| !a.preview.is_empty())
                .map(|a| Thumb {
                    id: bs58::encode(a.blob).into_string(),
                    bytes: a.preview.as_slice().into(),
                });
            (m.seq, (m.account, said, preview))
        })
        .collect();
    timeline
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
            said: m.post.said().filter(|first| *first <= m.posted),
            via: m.post.via().map(|k| Chat::via_name(&k)),
            reactions: m
                .reactions
                .iter()
                .map(|(emoji, who)| {
                    // Oneself first and by the word, the way one is
                    // named anywhere a list includes you.
                    let ours = who.contains(&me);
                    let mut named: Vec<String> = who
                        .iter()
                        .filter(|k| **k != me)
                        .map(|k| name_for(people, k, ""))
                        .collect();
                    named.sort();
                    if ours {
                        named.insert(0, "You".to_string());
                    }
                    sigil_ui::Reaction {
                        emoji: emoji.clone(),
                        who: named,
                        ours,
                    }
                })
                .collect(),
            reply_to: m.post.reply_to().map(|target| {
                stubs
                    .get(&target)
                    .map(|(account, said, preview)| Quoted {
                        seq: target,
                        who: people
                            .get(account)
                            .and_then(|p| p.name.clone())
                            .unwrap_or_else(|| short(account)),
                        said: said.clone(),
                        preview: preview.clone(),
                    })
                    .unwrap_or_else(|| Quoted::unheld(target))
            }),
            // Only ever on our own. On somebody else's it would be a
            // claim about our own reading, shown back to us.
            receipt: (m.account == me)
                .then(|| receipts.map(|k| receipt_for(k, m.seq, &me)))
                .flatten(),
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
                    duration_ms: a.duration_ms().map(u64::from),
                    shape: a.dimensions().map(|(w, h)| (u32::from(w), u32::from(h))),
                    waveform: a.waveform().unwrap_or_default().into(),
                    // A clip's own first frame where this device has
                    // decoded one; the sender's 96-pixel thumbnail
                    // otherwise. See `still_for_a_clip`.
                    preview: desk
                        .stills
                        .get(&a.blob)
                        .cloned()
                        .unwrap_or_else(|| a.preview.as_slice().into()),
                    bytes: desk.files.get(&a.blob).cloned(),
                    // Asked for and refused, as against not reached
                    // yet. The two look the same on screen otherwise,
                    // and only one of them is worth waiting for.
                    // Only a blob the exchange says it does not hold.
                    // One waiting out a retry is not missing; it is
                    // on its way, and saying otherwise is the fault
                    // this told apart.
                    missing: matches!(desk.unfetchable.get(&a.blob), Some(Unfetched::Gone)),
                    held: !fetch_unasked(
                        a.size,
                        desk.wanted.contains(&a.blob),
                        chat.store().has_blob(&a.blob).unwrap_or(false),
                    ),
                    // **The name says what is drawn, not just which
                    // blob.** A texture is cached under a URI built
                    // from this, and a preview that changes from the
                    // sender's thumbnail to this device's own still
                    // under the same name is a picture nobody sees
                    // change. The old name stops being drawn, and
                    // `forget_what_is_gone` puts its texture down.
                    id: if desk.stills.contains_key(&a.blob) {
                        format!("{}-still", bs58::encode(a.blob).into_string())
                    } else {
                        bs58::encode(a.blob).into_string()
                    },
                })
                .collect(),
            standing: standing.get(&m.seq).copied().unwrap_or_default(),
            mentions: m
                .post
                .mentions()
                .map(|key| Mentioned {
                    key: *key,
                    label: people
                        .get(key)
                        .map(|p| p.label(key))
                        .unwrap_or_else(|| short(key)),
                })
                .collect(),
            me_mentioned: m.post.mentions().any(|key| *key == me),
            earlier,
        })
        .collect()
}

fn publish(chat: &impl Local, state: &watch::Sender<ChatState>, desk: &Desk, me: PubKey) -> bool {
    let verified: HashMap<PubKey, u64> = chat
        .store()
        .verified()
        .unwrap_or_default()
        .into_iter()
        .collect();
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
        picture: chat.avatar_of(&me).and_then(Face::of),
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
            // A direct message's row names a *person*, and `name_for` is the
            // order that decides which name. It used to set the label only
            // when a profile had arrived, and leave the key sitting there
            // when one had not.
            if let Some(peer) = k.peer {
                summary.label = name_for(&people, &peer, &k.label);
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
    // **Counted once.** Three places want the length of the open conversation
    // -- how much is in the window, how much is behind it, and where the
    // events above it start -- and each walked the fold to find out.
    // `messages()` iterates the whole map, so that was three passes over five
    // hundred messages to arrive at one number three times.
    let counted = open
        .map(|(_, k)| k.timeline.messages().count())
        .unwrap_or(0);

    // The **last** `wanted` of them, which is where a conversation is read
    // from. Everything before that stays in the fold and in the store; this
    // only bounds how much is turned into something drawable at once. See
    // [`PAGE`].
    let window = open
        .map(|(_, k)| counted.saturating_sub(k.wanted))
        .unwrap_or(0);
    let lines: Vec<Line> = open
        .map(|(_, k)| lines_of(&k.timeline, window, Some(k), false, me, &people, desk, chat))
        .unwrap_or_default();
    // SIP-60 §The client keeps what it read: what this client read of an
    // earlier incarnation of the same conversation -- a direct message
    // opened twice, or one folded when it turned out to be a stray. Shown
    // before the conversation and **never merged into it**: their sequence
    // numbers belong to channels that no longer exist.
    //
    // Read here, per publish, rather than cached on the channel. For every
    // conversation that was never folded it is one indexed query that
    // returns nothing, which is what almost all of them are; for one that
    // was, it re-folds a log that ends at the fold and cannot grow. If a
    // conversation with a large copy ever costs a frame, this is where the
    // cache goes -- and it needs a place to live, because `publish` holds
    // `desk` by shared reference.
    // SIP-53 §Posting again. Read here for the same reason the copies are:
    // one indexed query for a conversation that was never forked, which is
    // all of them until one is.
    let peer_home = open
        .and_then(|(_, k)| k.peer)
        .and_then(|peer| desk.peer_homes.get(&peer).cloned());
    let stranded: Vec<Stranded> = open
        .map(|(c, _)| chat.stranded_posts(&c))
        .unwrap_or_default();
    let copies: Vec<Vec<Line>> = open
        .map(|(c, k)| {
            chat.earlier(&c, &k.admins)
                .iter()
                .map(|t| lines_of(t, 0, None, true, me, &people, desk, chat))
                .filter(|lines| !lines.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // How many are behind the window, so the reader can be offered them.
    let earlier = open
        .map(|(_, k)| counted.saturating_sub(k.wanted))
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
            let behind = counted.saturating_sub(k.wanted);
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
                        // SIP-56: said with what it means, since "muted" alone
                        // reads as silenced on this screen only.
                        EVENT_MUTED => (
                            format!("{a} muted {b} — they read, and may not write"),
                            None,
                        ),
                        EVENT_UNMUTED => (format!("{a} unmuted {b}"), None),
                        // SIP-44: the old key's own signature is in the entry,
                        // which is the one thing that makes this believable.
                        EVENT_SUCCEEDED => (
                            format!("{a}'s account is now {b}"),
                            Some(
                                "They named this key to succeed them, signed by the key they \
                                 lost. Everything they held here is that key's now.",
                            ),
                        ),
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
                        call: None,
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
                // **Only a call nobody here took is coloured.** Declining
                // and cancelling are decisions somebody made, and a call
                // this side placed that went unanswered is the caller's
                // own business -- none of them is owed anything. A call
                // that rang here and was not answered is the one line in a
                // transcript that is still a thing to do.
                call: Some(match outcome {
                    CALL_MISSED if !mine => Call::Missed,
                    _ => Call::Was,
                }),
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
    // SIP-39: a cross-exchange ring lasts as long as the caller's exchange
    // keeps asking, which is bounded by their patience and not by anything
    // this side is told. A minute is longer than anybody rings for.
    let cross_ring = desk
        .cross_ring
        .as_ref()
        .filter(|(_, since)| since.elapsed() < CROSS_RING_FOR)
        .map(|(ring, _)| ring.clone());
    let mut over: Vec<([u8; 32], u64)> = Vec::new();
    for (channel, known) in &desk.channels {
        for call in known.timeline.calls() {
            if call.ended.is_some() {
                over.push((*channel, call.seq));
            }
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
                label: match known.peer {
                    Some(peer) => name_for(&people, &peer, &known.label),
                    None => known.label.clone(),
                },
                direct: call.media & MEDIA_DIRECT != 0,
                peer: known.peer,
            });
        }
    }
    ringing.sort_by_key(|r| r.seq);

    // Messages from others that arrived live, anywhere -- what gets said
    // out loud. From the log, like the rings, so nothing is stored: a
    // message is live if it came after what the channel had when the
    // exchange first answered for it this session, and history is not news.
    let mut arrivals: Vec<Arrival> = Vec::new();
    for (channel, known) in &desk.channels {
        let Some(live_from) = known.live_from else {
            continue;
        };
        for m in known.timeline.messages().filter(|m| m.seq > live_from) {
            if m.account == me || m.redacted {
                continue;
            }
            let words = m.post.body_text().unwrap_or("");
            arrivals.push(Arrival {
                channel: *channel,
                seq: m.seq,
                from: m.account,
                from_label: name_for(&people, &m.account, ""),
                conversation: match known.peer {
                    Some(peer) => name_for(&people, &peer, &known.label),
                    None => known.label.clone(),
                },
                public: known.public.unwrap_or(false),
                direct: known.peer.is_some(),
                said: if words.is_empty() {
                    only_files(m.post.attachments().map(|a| a.effective_kind()))
                } else {
                    stub(words)
                },
                in_open: desk.open == Some(*channel),
                mentions_me: m.post.mentions().any(|k| *k == me),
            });
        }
    }
    arrivals.sort_by_key(|m| (m.channel, m.seq));

    // The same record above a different floor: what this device did not
    // hold when the session started. For a phone woken while it slept, this
    // is what there is to say; `arrivals` is empty then, since nothing
    // arrived while it watched -- it arrived while it did not.
    let mut unseen: Vec<Arrival> = Vec::new();
    for (channel, known) in &desk.channels {
        for m in known
            .timeline
            .messages()
            .filter(|m| m.seq > known.held_from)
        {
            if m.account == me || m.redacted {
                continue;
            }
            let words = m.post.body_text().unwrap_or("");
            unseen.push(Arrival {
                channel: *channel,
                seq: m.seq,
                from: m.account,
                from_label: name_for(&people, &m.account, ""),
                conversation: match known.peer {
                    Some(peer) => name_for(&people, &peer, &known.label),
                    None => known.label.clone(),
                },
                public: known.public.unwrap_or(false),
                direct: known.peer.is_some(),
                said: if words.is_empty() {
                    only_files(m.post.attachments().map(|a| a.effective_kind()))
                } else {
                    stub(words)
                },
                in_open: desk.open == Some(*channel),
                mentions_me: m.post.mentions().any(|k| *k == me),
            });
        }
    }
    unseen.sort_by_key(|m| (m.channel, m.seq));

    let typing = open.map(|(_, k)| k.typing).unwrap_or(false);
    let trouble = open.map(|(_, k)| k.trouble.clone()).unwrap_or_default();
    // SIP-56: who is muted is what the transcript's signed entries say, the
    // last word per member winning; the roster itself carries no flag.
    let members = open
        .map(|(_, k)| {
            let mut muted: HashSet<PubKey> = HashSet::new();
            for h in k.timeline.events() {
                match h.what.event {
                    EVENT_MUTED => {
                        muted.insert(h.what.subject);
                    }
                    EVENT_UNMUTED | EVENT_REMOVED | EVENT_LEFT => {
                        muted.remove(&h.what.subject);
                    }
                    _ => {}
                }
            }
            k.members
                .iter()
                .map(|m| Member {
                    muted: muted.contains(&m.account),
                    ..m.clone()
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // What the **exchange** attests, not what anybody says about themselves.
    // This is the one place a role may be drawn as a role.
    let i_am_admin = members.iter().any(|m| m.account == me && m.admin);
    let topic = open
        .map(|(_, k)| k.timeline.topic.clone())
        .unwrap_or_default();
    let home = open.and_then(|(channel, _)| chat.homed_elsewhere(&channel));
    let link = chat.link();

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
        set!(copies, copies);
        set!(stranded, stranded);
        set!(peer_home, peer_home);
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
        set!(reports_pending, desk.reports_pending);
        set!(prekeys, desk.prekeys);
        set!(folds, desk.folds);
        set!(topic, topic);
        set!(home, home);
        set!(ringing, ringing);
        set!(cross_ring, cross_ring);
        set!(over, over);
        set!(arrivals, arrivals);
        set!(unseen, unseen);
        set!(presence, desk.presence.clone());
        set!(peers, desk.peers.clone());
        set!(verified, verified);
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

/// What a message that is only files carries, in words, for a quote of it.
///
/// **One wording, wherever a message is quoted.** The composer names what is
/// about to be answered and the reply then names what was; said in two places
/// they drifted to "a file" against "a picture" for the same photograph.
fn only_files(kinds: impl Iterator<Item = u8>) -> String {
    let kinds: Vec<u8> = kinds.collect();
    let pictures: Vec<u8> = kinds
        .iter()
        .copied()
        .filter(|&k| k == sqex_proto::blob::KIND_IMAGE || k == sqex_proto::blob::KIND_VIDEO)
        .collect();
    match (pictures.len(), kinds.len()) {
        (1, 1) if pictures[0] == sqex_proto::blob::KIND_VIDEO => "a clip".to_string(),
        (1, 1) => "a picture".to_string(),
        (n, f) if n == f => format!("{n} pictures"),
        (_, 1) => "a file".to_string(),
        (_, f) => format!("{f} files"),
    }
}

/// A message flattened to one line, to name it in a reply.
///
/// Cut generously. It was forty-eight characters, and that cut landed
/// *before* the bubble's width had any say -- a quote in a bubble three
/// quarters of the pane wide still ended at "any of us have ever …" with half
/// the row empty after it. The bubble truncates the line to the width it
/// actually has, so this only needs to be longer than any width a bubble can
/// be; past that a longer stub costs a little measuring and shows nothing.
fn stub(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > STUB_CHARS {
        let cut: String = flat.chars().take(STUB_CHARS - 1).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

/// More small-text characters than fit across three quarters of a wide pane.
const STUB_CHARS: usize = 200;

#[cfg(test)]
mod stub_tests {
    use super::{STUB_CHARS, stub};

    /// A quote shows what the bubble has room for, not what a constant had.
    ///
    /// The cut was forty-eight characters, and a reply's quote in a bubble
    /// three quarters of the pane wide still ended at "any of us have ever …"
    /// with half the row empty after it. The bubble truncates to its width;
    /// this only has to be longer than any width a bubble can be.
    #[test]
    fn a_long_message_is_quoted_past_the_old_cut() {
        let long = "word ".repeat(60);
        let quoted = stub(&long);
        assert!(
            quoted.chars().count() > 100,
            "the quote was cut short of what a wide bubble can show: {} chars",
            quoted.chars().count()
        );
        assert!(quoted.ends_with('…'), "and it says it was cut");
        assert_eq!(quoted.chars().count(), STUB_CHARS, "cut to exactly the cap");
    }

    /// Short enough is left alone, and newlines are flattened out of it.
    #[test]
    fn a_short_message_is_quoted_whole_and_on_one_line() {
        assert_eq!(stub("two\nlines"), "two lines");
        assert_eq!(stub("  spaced out  "), "spaced out");
        assert!(!stub("short").ends_with('…'));
    }
}

/// A key where a whole one will not fit: its first four characters, three
/// dots, and its last four. See [`sigil_ui::short`] for why both ends.
///
/// The **same** shortening the transcript uses on an author line, borrowed
/// rather than written again. Two of them put `3Kj9mNpQ` in one column and
/// `3Kj9...VfeR` in another, and a reader comparing the two has to work out
/// whether they are looking at one person or at two.
/// Where `needle` first occurs in `text`, letter case aside; `needle` is
/// already lowercase. The byte range is in `text` as written -- not in a
/// lowercased copy, whose bytes need not line up with the original ("İ"
/// lowercases to two characters) -- so it can mark the word in the message.
///
/// Compared a character at a time from each character boundary, which is
/// quadratic in the worst case and fine for a message: the alternative,
/// searching a lowercased copy, gives an index into the wrong string.
fn find_ignoring_case(text: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    let same = |a: char, b: char| a.to_lowercase().eq(b.to_lowercase());
    for (start, _) in text.char_indices() {
        let mut rest = text[start..].char_indices();
        let mut wanted = needle.chars();
        let mut end = start;
        loop {
            match (wanted.next(), rest.next()) {
                (None, _) => return Some(start..end),
                (Some(_), None) => return None,
                (Some(n), Some((i, c))) => {
                    if !same(c, n) {
                        break;
                    }
                    end = start + i + c.len_utf8();
                }
            }
        }
    }
    None
}

fn short(key: &PubKey) -> String {
    sigil_ui::short(&key.to_string())
}

/// Which unreadable entries this identity may take down.
///
/// The exchange's own rule for a redaction: the author may, and an admin may
/// for anybody. Offering more would be offering a button the exchange refuses;
/// offering less would leave somebody looking at their own message with no
/// way to remove it.
fn redactable(unopened: &[(u64, PubKey)], me: PubKey, admin: bool) -> Vec<u64> {
    unopened
        .iter()
        .filter(|(_, author)| admin || *author == me)
        .map(|(seq, _)| *seq)
        .collect()
}

#[cfg(test)]
mod redactable_tests {
    use super::redactable;
    use sqnr_core::PubKey;

    fn key(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    /// Mine, and only mine, when I am nobody in particular.
    #[test]
    fn a_member_may_take_down_their_own_and_nobody_elses() {
        let held = [(7, key(1)), (8, key(2)), (9, key(1))];
        assert_eq!(redactable(&held, key(1), false), vec![7, 9]);
        assert_eq!(redactable(&held, key(3), false), Vec::<u64>::new());
    }

    /// Everything, when I administer the channel.
    #[test]
    fn an_admin_may_take_down_any_of_them() {
        let held = [(7, key(1)), (8, key(2))];
        assert_eq!(redactable(&held, key(3), true), vec![7, 8]);
    }
}

/// What to call the person on the other end of a direct message.
///
/// Four answers, in this order, and the order is the whole of it:
///
/// 1. **What they publish about themselves** (SIP-21). It wins because a row
///    names a person and this is the person speaking. The list has always
///    preferred it.
/// 2. **What this machine was told to call them** when they were added. Above
///    the handle because somebody here chose it, and a chosen name is more use
///    to the person reading than a correct one.
/// 3. **The handle the exchange bound** (SIP-38). Not offered here before, so
///    somebody reachable as `alice@squic.org` and named nowhere was drawn as
///    their key.
/// 4. **A short key**, which is never wrong and always fits.
///
/// The local label is skipped when empty, and empty is what it now is when
/// nobody chose one -- it used to be the key repeated back, which no step of
/// this could tell from a name.
///
/// A free function over plain data, because the order is the part that can be
/// got wrong and this way it is testable without a session or an exchange.
fn name_for(people: &HashMap<PubKey, Person>, peer: &PubKey, local: &str) -> String {
    let person = people.get(peer);
    person
        .and_then(|p| p.name.clone())
        .or_else(|| (!local.is_empty()).then(|| local.to_string()))
        .or_else(|| person.and_then(|p| p.handle.clone()))
        .unwrap_or_else(|| short(peer))
}

/// What to call somebody. [`naming_tests`] below is the neighbouring
/// question -- *whose* names are worth asking the exchange for -- and
/// `crate::label_tests` is a third, about naming exchanges.
#[cfg(test)]
mod face_tests {
    use super::Face;

    /// **A picture nobody published is not a picture.** An empty avatar column
    /// is what the store holds for an account with no picture, and `Some` of
    /// nothing would make every such row try to decode zero bytes on the first
    /// frame that drew it.
    #[test]
    fn no_bytes_is_no_face() {
        assert!(Face::of(Vec::new()).is_none());
    }

    /// The hash is what a texture cache compares, so these two rules are the
    /// cache's correctness: the same picture must not be redecoded, and a
    /// changed one must not go on being drawn.
    #[test]
    fn the_hash_follows_the_bytes() {
        let a = Face::of(vec![1, 2, 3]).expect("three bytes is a picture here");
        let same = Face::of(vec![1, 2, 3]).expect("and so is the same three");
        let other = Face::of(vec![1, 2, 4]).expect("and so is a different three");
        assert_eq!(
            a.hash, same.hash,
            "the same bytes hashed differently, so every pass redecodes"
        );
        assert_ne!(
            a.hash, other.hash,
            "different bytes hashed the same, so a changed picture stays stale"
        );
        assert_eq!(&*a.bytes, &[1, 2, 3], "and the bytes themselves are kept");
    }
}

#[cfg(test)]
mod display_name_tests {
    use super::*;

    fn key(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    fn person(name: Option<&str>, handle: Option<&str>) -> Person {
        Person {
            name: name.map(str::to_string),
            title: None,
            handle: handle.map(str::to_string),
            picture: None,
        }
    }

    fn just(k: PubKey, p: Person) -> HashMap<PubKey, Person> {
        HashMap::from([(k, p)])
    }

    /// The regression this module exists for.
    ///
    /// `label` answered with the **whole** key, so anybody who had not
    /// published a name was drawn as forty-odd characters of base58 -- in the
    /// conversation list, in the ring banner and in the identity header at
    /// once, each of them a row with a name-shaped hole in it.
    #[test]
    fn an_unnamed_person_is_a_short_key_and_not_a_whole_one() {
        let k = key(7);
        let whole = k.to_string();
        let shown = Person::default().label(&k);

        assert_ne!(
            shown, whole,
            "the whole key is exactly what this used to be"
        );
        assert!(shown.chars().count() < whole.chars().count());
        let (head, tail) = shown
            .split_once("...")
            .expect("it has to say it was cut, or it reads as a whole key");
        assert!(
            whole.starts_with(head) && whole.ends_with(tail),
            "and it has to be both ends of the real one: {shown} against {whole}"
        );
    }

    /// One shortening, not two.
    ///
    /// The session had its own eight-character cut while the transcript used
    /// a different one, so the same person could appear as `3Kj9mNpQ` in a
    /// banner and something else in a bubble.
    #[test]
    fn a_key_is_shortened_the_same_way_everywhere() {
        let k = key(9);
        assert_eq!(short(&k), sigil_ui::short(&k.to_string()));
        assert_eq!(Person::default().label(&k), short(&k));
    }

    /// What somebody publishes beats what this machine happens to call them.
    #[test]
    fn what_they_publish_wins_over_what_we_call_them() {
        let k = key(1);
        let people = just(k, person(Some("Alice"), Some("alice@squic.org")));
        assert_eq!(name_for(&people, &k, "the plumber"), "Alice");
    }

    /// A chosen name beats a bound one: somebody here typed "the plumber".
    #[test]
    fn a_local_label_beats_a_handle_and_a_key() {
        let k = key(2);
        let people = just(k, person(None, Some("alice@squic.org")));
        assert_eq!(name_for(&people, &k, "the plumber"), "the plumber");
        assert_eq!(name_for(&HashMap::new(), &k, "the plumber"), "the plumber");
    }

    /// A handle names somebody when nothing else does.
    #[test]
    fn a_handle_names_somebody_when_nothing_else_will() {
        let k = key(3);
        let people = just(k, person(None, Some("alice@squic.org")));
        assert_eq!(name_for(&people, &k, ""), "alice@squic.org");
    }

    /// With none of them, a short key -- and **never** the empty string, which
    /// would draw a nameless row nobody could tell from another nameless row.
    #[test]
    fn with_nothing_at_all_it_is_a_short_key() {
        let k = key(4);
        assert_eq!(name_for(&HashMap::new(), &k, ""), short(&k));
        assert_ne!(
            name_for(&HashMap::new(), &k, ""),
            k.to_string(),
            "a whole key is what the list used to draw"
        );
        // An empty profile is not a name. A withheld one, an absent one and a
        // blocked one all arrive looking exactly like this.
        assert_eq!(name_for(&just(k, Person::default()), &k, ""), short(&k));
    }

    /// `named` is the question "can we name them at all", and its answer is
    /// what decides whether a view draws a name or falls back.
    #[test]
    fn named_is_none_only_when_there_is_nothing_to_show() {
        assert_eq!(Person::default().named(), None);
        assert_eq!(
            person(None, Some("alice@squic.org")).named().as_deref(),
            Some("alice@squic.org")
        );
        assert_eq!(
            person(Some("Alice"), Some("alice@squic.org"))
                .named()
                .as_deref(),
            Some("Alice")
        );
    }
}

/// SIP-60 §When a client presents a Move unasked (2026-09-21): the home this
/// identity's account lives at, recorded beside the identity where every
/// client reads it as the default exchange. Written when a Move naming this
/// exchange is presented or found on record; left alone when it already
/// says so, and never an error -- the exchange's record is the authority.
async fn record_home(identity: &std::path::Path, chat: &mut Chat) {
    if let Err(e) = chat.record_home_beside(identity).await {
        tracing::warn!(%e, "could not record the home beside the identity");
    }
}

/// The first eight hex characters of an identifier, for a channel with no name.
fn hex8(id: &[u8; 32]) -> String {
    id.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

async fn apply(chat: &mut Chat, cmd: Cmd, state: &watch::Sender<ChatState>, desk: &mut Desk) {
    match cmd {
        Cmd::OpenRemote { target, identity } => {
            // SIP-60: the create is carried to the other person's home as
            // this identity's own signed act, after its Move. Presented now
            // if it is not on record; a visitor (a store filed under another
            // exchange) or a linked device does not reach out from here.
            // This is the identity's default session (the interface sends
            // from no other), so a new store claims here: the person chose
            // this exchange, and the claim is written beside the identity.
            let said = match chat.ensure_home().await {
                Ok(HomeSaid::Unclaimed) => chat.claim_home().await,
                other => other,
            };
            match said {
                Ok(HomeSaid::Presented | HomeSaid::OnRecord) => {
                    if let Some(id) = &identity {
                        record_home(id, chat).await;
                    }
                }
                Ok(HomeSaid::Unclaimed) => {
                    return trouble(state, "this store has no home yet and could not claim one");
                }
                Ok(HomeSaid::Visitor { home, .. }) => {
                    return trouble(
                        state,
                        format!(
                            "this identity lives at {home}; write to people at other \
                             exchanges from there"
                        ),
                    );
                }
                Ok(HomeSaid::NotMine) => {
                    return trouble(
                        state,
                        "a linked device cannot reach out to another exchange; the \
                         account's own client can",
                    );
                }
                Err(e) => return trouble(state, e),
            }
            match chat.locate(&target).await {
                Ok(found) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let _ = chat.store().add_contact(&found.account, &target, now);
                    desk.restructure = true;
                    Box::pin(apply(chat, Cmd::OpenDm(found.account), state, desk)).await;
                }
                Err(e) => trouble(state, format!("could not find {target}: {e}")),
            }
        }
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
                    // Not their key: `publish` says what to call them.
                    label: String::new(),
                    admins: vec![chat.me, peer],
                    members: Vec::new(),
                    marks: Vec::new(),
                    timeline: Timeline::default(),
                    // Brand new here, and folded by the `open` below in the
                    // ordinary way -- a direct message opened a second time
                    // (SIP-60) has rows on the disc, and they are its own.
                    folded: false,
                    seen: 0,
                    wanted: PAGE,
                    last_at: 0,
                    unread: 0,
                    mentioned: 0,
                    live_from: None,
                    held_from: 0,
                    told: 0,
                    waiting,
                    typing: false,
                    fetched: false,
                    trouble: Trouble::default(),
                });
                if let Some(k) = desk.channels.get_mut(&channel) {
                    k.waiting = waiting;
                }
                open(&*chat, desk, state, channel);
            }
            Err(e) => state.send_modify(|s| s.trouble = Some(e.to_string())),
        },
        Cmd::Show(channel) => {
            open(&*chat, desk, state, channel);

            // **At once, from the disc.** `open` clears the transcript and
            // marks the channel for the next poll, and the poll is a round
            // trip: until this, opening a conversation showed an empty pane
            // for as long as the exchange took to answer -- on every open,
            // including the one sigil does for you on the way in. The history
            // is already folded and sitting in `desk`.
            let me = chat.me;
            let _ = publish(chat, state, desk, me);
        }
        Cmd::ShowAt { channel, seq } => {
            if desk.open != Some(channel) {
                open(&*chat, desk, state, channel);
            }
            // Opened already, and so folded already -- but a jump into a
            // conversation that is on screen because somebody left it there
            // still needs the whole of it to find a sequence number in.
            ensure_folded(&*chat, desk, &channel);
            if let Some(known) = desk.channels.get_mut(&channel) {
                let place = known.timeline.messages().position(|m| m.seq == seq);
                // Not in the fold -- redacted since, or a hit from before a
                // wipe -- and there is nothing to widen towards: opened as
                // `Show` would have.
                if let Some(place) = place {
                    let counted = known.timeline.messages().count();
                    known.wanted = known.wanted.max(counted - place + PAGE / 2);
                }
                desk.dirty.insert(channel);
            }
            let me = chat.me;
            let _ = publish(chat, state, desk, me);
        }
        Cmd::Refetch => {
            // Everything that failed, not one file: a fetch fails for reasons
            // that are rarely about the one blob — the link was down, the key
            // had not arrived — and a reader asking again means "try the lot".
            desk.unfetchable.clear();
        }
        Cmd::Fetch { seq, index } => {
            // Only remembered as wanted; `fetch_files` does the fetching, on
            // the same pass and with the same bounds as every other picture.
            if let Some(blob) = desk.open.and_then(|c| desk.channels.get(&c)).and_then(|k| {
                k.timeline
                    .messages()
                    .find(|m| m.seq == seq)
                    .and_then(|m| m.post.attachments().nth(index))
                    .map(|a| a.blob)
            }) {
                desk.wanted.insert(blob);
                desk.unfetchable.remove(&blob);
            }
        }
        Cmd::Earlier => reach_earlier(&*chat, desk),
        Cmd::PeerHome(peer) => {
            // **A failure here is said, not swallowed.** Where a peer lives
            // decides whether a call is placed at this exchange or bridged
            // to theirs (SIP-39), and an unknown home is read as "here" --
            // so a dropped error becomes a call that rings, connects to a
            // room at the wrong exchange and carries nothing. That is what
            // it did, twice, with nothing in the log to say why.
            if desk.peer_homes.contains_key(&peer) {
                return;
            }
            match chat.account_home(&peer).await {
                Ok(homed) if !homed.domain.is_empty() => {
                    desk.peer_homes.insert(peer, (homed.home, homed.domain));
                    desk.restructure = true;
                }
                // **No domain is an answer, and is recorded as one.** An
                // account at an exchange reached by address has no name to
                // be bridged to (SIP-39 takes a handle), so the ordinary
                // path is right for it -- and leaving the entry out made
                // that indistinguishable from never having found out,
                // which is a different thing entirely and wants different
                // handling at the handset. `elsewhere_handle` already
                // reads an empty domain as "here"; what it could not read
                // was the absence.
                Ok(homed) => {
                    tracing::debug!(%peer, "this peer's home has no domain; the ordinary path it is");
                    desk.peer_homes.insert(peer, (homed.home, String::new()));
                    desk.restructure = true;
                }
                Err(why) => tracing::warn!(%peer, %why, "could not ask where this peer lives"),
            }
        }
        Cmd::AskForKey => {
            let Some(channel) = desk.open else { return };
            match chat.ensure_epoch(&channel).await {
                Ok(epoch) => {
                    desk.dirty.insert(channel);
                    desk.restructure = true;
                    note(
                        state,
                        format!(
                            "You hold the key for epoch {epoch}. What was said under an \
                             earlier one stays with it."
                        ),
                    );
                }
                // The honest answer where nobody has sealed one and this
                // account may not mint: there is nothing more to press.
                Err(e) => trouble(state, e),
            }
        }
        // SIP-53 §Posting again: a new entry, carrying `Said` so every
        // reader shows it at the time it was first said and marks it. The
        // library drops it from the stranded set whichever way this goes.
        Cmd::PostAgain(seq) => {
            let Some(channel) = desk.open else { return };
            match chat.post_again(&channel, seq).await {
                Ok(_) => {
                    desk.dirty.insert(channel);
                    desk.restructure = true;
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::ForgetStranded(seq) => {
            let Some(channel) = desk.open else { return };
            match chat.forget_stranded(&channel, seq) {
                Ok(()) => {
                    desk.dirty.insert(channel);
                    desk.restructure = true;
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Close => {
            desk.open = None;
            state.send_modify(|s| {
                s.open = None;
                s.lines.clear();
                s.copies.clear();
                s.stranded.clear();
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
                //
                // SIP-56: a mute is said as what it is, not as a refusal code.
                let said = match &e {
                    sqex_chat::client::ChatError::Refused(_, r)
                        if r.code == sqex_proto::refusal::Code::Muted =>
                    {
                        "You are muted here: you can read, and may not write.".to_string()
                    }
                    _ => e.to_string(),
                };
                state.send_modify(|s| s.trouble = Some(said));
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
                            "The exchange answered {other}, which this version does not know."
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
        Cmd::Post(mut draft) => {
            let Some(channel) = desk.open else { return };
            // **An edit replaces the whole post** (SIP-19). What the original
            // carried besides its words comes back from the original here:
            // the message it replied to always -- a rewrite of one word must
            // not unthread it -- and the files the composer said to keep,
            // which it showed as tiles, so one taken out is taken off the
            // message. The mentions are the composer's: it loads them with
            // the text, so a name taken out of the words takes its mention
            // with it, as in a fresh message.
            let mut attachments = Vec::with_capacity(draft.files.len());
            if let Some(target) = draft.edit
                && let Some(m) = desk
                    .channels
                    .get(&channel)
                    .and_then(|k| k.timeline.get(target))
            {
                // Refused here rather than sent to be dropped by every
                // reader in silence -- the interface no longer offers it,
                // but a rewrite can be armed at the edge of the window.
                if !rewritable(m.posted, unix_now()) {
                    let why = "Too late to rewrite it: a message can be changed for a day \
                               after it is sent.";
                    trouble(state, why);
                    return posted(state, draft.token, Some(why.to_string()));
                }
                if draft.reply.is_none() {
                    draft.reply = m.post.reply_to();
                }
                attachments.extend(
                    m.post
                        .attachments()
                        .filter(|a| draft.keep.contains(&bs58::encode(a.blob).into_string()))
                        .cloned(),
                );
            }
            // The files, each uploaded and described; one that fails fails
            // the message, because half a message is not the message.
            for path in &draft.files {
                match attach_file(chat, state, &channel, path).await {
                    Ok(a) => attachments.push(a),
                    Err(e) => {
                        trouble(state, &e);
                        return posted(state, draft.token, Some(e.to_string()));
                    }
                }
            }
            let names: Vec<String> = draft
                .files
                .iter()
                .map(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .collect();
            let post = sqex_proto::message::Post {
                parts: draft.parts_with(attachments),
                ..Default::default()
            };
            let sent = match draft.edit {
                // Enforced at the **reader**: only from the account that
                // posted it and only inside the edit window. The window is
                // checked above, so an edit that would be ignored is said
                // to be rather than sent.
                Some(target) => chat.edit(&channel, target, post).await,
                None => chat.send_post(&channel, post).await,
            };
            match sent {
                Ok(_) => {
                    if !names.is_empty() {
                        note(state, format!("Sent {}.", names.join(", ")));
                    }
                    posted(state, draft.token, None);
                    desk.dirty.insert(channel);
                }
                Err(e) => {
                    // The interface took the words out of the box to send
                    // them; this is what tells it to put them back. Retyping
                    // a message the program lost is the worst thing a chat
                    // client can do to somebody.
                    trouble(state, &e);
                    posted(state, draft.token, Some(e.to_string()));
                }
            }
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
                // **A name this person verified that now resolves elsewhere
                // is said, not followed** (SIP-41). The mark is on the key;
                // the name is the exchange's word, and today it is a
                // different word.
                if let Some(held) = verified_holder_of(chat, &name, &who) {
                    return trouble(
                        state,
                        format!(
                            "{name} is a different key from the one you verified. Verified: \
                             {held}. Now: {who}. Write to the key you verified, or compare \
                             the words again."
                        ),
                    );
                }
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
        Cmd::Verify(who) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            if let Err(e) = chat.store().verify(&who, now) {
                return trouble(state, e);
            }
            let me = chat.me;
            let _ = publish(chat, state, desk, me);
        }
        Cmd::Unverify(who) => {
            if let Err(e) = chat.store().unverify(&who) {
                return trouble(state, e);
            }
            let me = chat.me;
            let _ = publish(chat, state, desk, me);
        }
        Cmd::Attest(who) => {
            let Some(mut client) = chat.connection() else {
                return trouble(state, "not connected");
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let a = sqex_proto::attest::Attestation::sign(
                &desk.seed,
                &who,
                sqex_proto::attest::CLAIM_VERIFIED_IN_PERSON,
                Vec::new(),
                now,
                now + 365 * 86_400,
            );
            match client.post("/attest/lodge", a.encode()).await {
                Ok((200, _)) => note(
                    state,
                    "Said, at this exchange, that you compared the words.".into(),
                ),
                Ok((code, _)) => trouble(
                    state,
                    format!("the exchange refused the statement ({code})"),
                ),
                Err(e) => trouble(state, e),
            }
        }
        Cmd::WakeEndpoint(endpoint) => {
            desk.wake = Some(endpoint);
            desk.wake_told = false;
            if chat.link() == Link::Up {
                tell_wake(chat, state, desk).await;
            }
        }
        Cmd::SuccessionOf(who) => match chat.succession_of(&who).await {
            Ok(found) => {
                // Checked here, not taken on the exchange's word: the proof
                // is the account's own will, or its guardians' vouches under
                // its own policy, and either verifies without the exchange.
                // A record whose proof does not prove what it says is no
                // record.
                let successor = found
                    .filter(|s| s.proof.account() == who && s.proof.proves(&s.successor))
                    .map(|s| s.successor);
                state.send_modify(|s| {
                    s.succeeded.insert(who, successor);
                });
            }
            Err(e) => trouble(state, e),
        },
        Cmd::Attested(who) => {
            let Some(mut client) = chat.connection() else {
                return trouble(state, "not connected");
            };
            let me = chat.me;
            let query = sqex_proto::attest::Query {
                subject: who,
                issuer: None,
            };
            match client.post("/attest/read", query.encode()).await {
                Ok((200, body)) => match sqex_proto::attest::Held::decode(&body) {
                    Ok(held) => {
                        // Verified here, not taken on the exchange's word:
                        // the whole point of a signature is that the reader
                        // checks it. Only the one claim sigil knows how to
                        // read, and not our own, which the dialog already
                        // shows as *verified*.
                        let mut issuers: Vec<PubKey> = held
                            .attestations
                            .iter()
                            .filter(|a| a.verify(held.now).is_ok())
                            .filter(|a| a.claim == sqex_proto::attest::CLAIM_VERIFIED_IN_PERSON)
                            .filter(|a| a.readable())
                            .filter(|a| a.issuer != me)
                            .map(|a| a.issuer)
                            .collect();
                        issuers.sort();
                        issuers.dedup();
                        state.send_modify(|s| {
                            s.attested.insert(who, issuers);
                        });
                    }
                    Err(e) => trouble(state, e),
                },
                // An exchange from before SIP-27 has nothing to say, which is
                // the same as nobody having said anything.
                Ok((404, _)) => state.send_modify(|s| {
                    s.attested.insert(who, Vec::new());
                }),
                Ok((code, _)) => trouble(state, format!("could not read what was said ({code})")),
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Away(away) => {
            if desk.away != away {
                desk.away = away;
                // Said now, not at the next beat: somebody back at the
                // keyboard is back.
                beat(chat, desk).await;
            }
        }
        Cmd::Search(query) => {
            let me = chat.me;
            search_local(&*chat, desk, state, me, &query);
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

        Cmd::Devices => refresh_devices(chat, state, desk).await,
        Cmd::BackupStatus => {
            backup_status(chat, state).await;
            succession_status(chat, state).await;
        }
        Cmd::BackupKey => {
            let key = match chat.backup_key() {
                Ok(Some(key)) => Ok(key),
                Ok(None) => chat.new_backup_key(),
                Err(e) => Err(e),
            };
            match key {
                Ok(key) => {
                    let words: Vec<String> = Chat::backup_words(&key)
                        .iter()
                        .map(|w| w.to_string())
                        .collect();
                    state.send_modify(|s| {
                        let b = s.backup.get_or_insert_with(Backup::default);
                        b.has_key = true;
                        b.words = Some(words);
                    });
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::HideBackupKey => state.send_modify(|s| {
            if let Some(b) = s.backup.as_mut() {
                b.words = None;
            }
        }),
        Cmd::BackupNow => {
            let key = match chat.backup_key() {
                Ok(Some(key)) => key,
                Ok(None) => {
                    return trouble(state, "make a backup key first, and write its words down");
                }
                Err(e) => return trouble(state, e),
            };
            match chat.backup(&key).await {
                Ok(done) => {
                    note(
                        state,
                        format!(
                            "Backed up: generation {}, {} channel(s) uploaded, {} kept, \
                             {} contact(s), {} bytes.",
                            done.generation, done.uploaded, done.kept, done.contacts, done.bytes
                        ),
                    );
                    backup_status(chat, state).await;
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Restore { words, identity } => {
            let split: Vec<&str> = words.split_whitespace().collect();
            let key = match sqex_proto::backup::from_words(&split) {
                Ok(key) => key,
                Err(e) => return trouble(state, e),
            };
            match chat.restore(&key, None).await {
                Ok(got) => {
                    // The words that opened it are this store's key from now
                    // on, so the next backup from here continues the line.
                    let _ = chat.set_backup_key(&key);
                    if let Some(id) = &identity {
                        record_home(id, chat).await;
                    }
                    let mut said = format!(
                        "Restored generation {}: {} channel(s), {} entries, {} keys, \
                         {} contact(s).",
                        got.generation, got.channels, got.entries, got.keys, got.contacts
                    );
                    if !got.skipped.is_empty() {
                        said.push_str(&format!(" Skipped: {}.", got.skipped.join(", ")));
                    }
                    note(state, said);
                    desk.restructure = true;
                    backup_status(chat, state).await;
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::SuccessionStatus => succession_status(chat, state).await,
        Cmd::WriteWill(successor) => match chat.sign_will(&successor) {
            Ok(will) => {
                let encoded = bs58::encode(will.encode()).into_string();
                state.send_modify(|s| {
                    s.succession.get_or_insert_with(Default::default).will = Some(encoded);
                });
                note(
                    state,
                    format!(
                        "A will: {successor} may take this account by presenting it. Keep it \
                         apart from that key's secret — together they are the account."
                    ),
                );
            }
            Err(e) => trouble(state, e),
        },
        Cmd::NameGuardians {
            threshold,
            guardians,
        } => match chat.sign_policy(threshold, &guardians) {
            Ok(policy) => match chat.lodge_policy(&policy).await {
                Ok(()) => {
                    note(
                        state,
                        format!(
                            "Lodged: any {threshold} of {} guardians may name your successor. \
                             Tell them.",
                            guardians.len()
                        ),
                    );
                    succession_status(chat, state).await;
                }
                Err(e) => trouble(state, e),
            },
            Err(e) => trouble(state, e),
        },
        Cmd::Vouch { account, successor } => {
            let vouch = chat.vouch(&account, &successor);
            let encoded = bs58::encode(vouch.encode()).into_string();
            state.send_modify(|s| {
                s.succession.get_or_insert_with(Default::default).vouch = Some(encoded);
            });
            note(
                state,
                format!("Your word that {successor} succeeds {account}. Give it to them."),
            );
        }
        Cmd::Succeed(pasted) => match proof_from(chat, &pasted).await {
            Ok(proof) => {
                let account = proof.account();
                match chat.succeed(proof).await {
                    Ok(()) => {
                        note(
                            state,
                            format!(
                                "{account} is yours: its names, its conversations, its place in \
                                 each. Its old devices are nobody's now; link yours."
                            ),
                        );
                        desk.restructure = true;
                    }
                    Err(e) => trouble(state, e),
                }
            }
            Err(e) => trouble(state, e),
        },
        // SIP-44 §The handover. `handover(None)` makes the new key itself,
        // signs the will under the old one, issues a credential from the new
        // key for each device, posts them together, and keeps the new seed
        // sealed in the store — so there is nothing for the interface to
        // carry and nothing for anybody to paste.
        Cmd::HandOver => match chat.handover(None).await {
            Ok(new) => {
                note(
                    state,
                    format!(
                        "This account's key is now {new}. Your names, conversations and \
                         devices came across; the old key is not the account any more."
                    ),
                );
                succession_status(chat, state).await;
            }
            Err(e) => trouble(state, e),
        },
        Cmd::HideSuccession => state.send_modify(|s| {
            if let Some(su) = s.succession.as_mut() {
                su.will = None;
                su.vouch = None;
            }
        }),
        Cmd::DropBackup => match chat.drop_backup().await {
            Ok(()) => {
                note(state, "Dropped. What is on this machine stays.".into());
                backup_status(chat, state).await;
            }
            Err(e) => trouble(state, e),
        },
        Cmd::CrossRingHandled => {
            desk.cross_ring = None;
        }
        Cmd::ClaimAccount(owner) => {
            let account = match owner.trim().parse::<PubKey>() {
                Ok(key) => Ok(key),
                Err(_) => chat.resolve_name(owner.trim()).await,
            };
            let account = match account {
                Ok(account) => account,
                Err(e) => {
                    trouble(state, e);
                    return;
                }
            };
            match chat.claim_listed(&account).await {
                Ok(credential) => {
                    tracing::info!(
                        account = %account,
                        until = credential.not_after,
                        "registered as a device of the account, by its own listing"
                    );
                    state.send_modify(|s| s.linked = Some(true));
                    note(
                        state,
                        format!(
                            "This device now acts for {account}. Its conversations arrive as \
                             your other devices hand them over."
                        ),
                    );
                    refresh_devices(chat, state, desk).await;
                    desk.dirty.extend(desk.channels.keys().copied());
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::LinkDevice { device, days } => {
            // This client registers itself first, and it has to. An account
            // with no registered devices is its own device; the moment one
            // is registered that fallback stops applying and everything
            // seals to the registered set only -- so linking without this
            // would cut *this* client out of every epoch minted afterwards,
            // and out of SIP-42, which admits only a listed device.
            let listed = match chat.my_devices().await {
                Ok(devices) => devices.iter().any(|d| d.device == chat.me),
                Err(e) => {
                    trouble(state, e);
                    return;
                }
            };
            if !listed {
                let own = match chat.issue_credential(&chat.me, days * 24 * 60 * 60) {
                    Ok(own) => own,
                    Err(e) => {
                        trouble(state, e);
                        return;
                    }
                };
                if let Err(e) = chat.register_self(&own).await {
                    trouble(state, e);
                    return;
                }
                refresh_devices(chat, state, desk).await;
            }
            match chat.issue_credential(&device, days * 24 * 60 * 60) {
                Ok(credential) => {
                    // **SIP-47 §Pairing, step 2: register it, from here.** The
                    // exchange takes a registration from an already-registered
                    // device of the same account -- which this one is, as of
                    // the block above -- and nothing ever posted one. So the
                    // other device's claim (`ClaimAccount`) looked for itself
                    // in the account's list and was never there: writing a
                    // credential is not registering anything. Registered, the
                    // other device only has to say where it was sent, which
                    // is the pairing somebody expects of a QR.
                    //
                    // The credential is still shown and still works by hand,
                    // for a device that cannot scan. A registration that
                    // fails is said and does not take the credential with it.
                    match chat.register_device(&credential).await {
                        Ok(()) => refresh_devices(chat, state, desk).await,
                        Err(e) => trouble(
                            state,
                            format!(
                                "the credential is written, but registering the device here failed: {e}"
                            ),
                        ),
                    }
                    let encoded = bs58::encode(credential.encode()).into_string();
                    state.send_modify(|s| s.credential = Some(encoded));
                    note(
                        state,
                        "The other device is registered: it can claim this account by \
                         your name here, or take this credential by hand. It names both \
                         keys in the clear, so hand it over the way you would a key."
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
                        refresh_devices(chat, state, desk).await;
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
        Cmd::SignOutDevice(device) => match chat.sign_out_device(&device).await {
            Ok(()) => {
                note(
                    state,
                    "Signed out. This device no longer acts for the account.".into(),
                );
                desk.restructure = true;
            }
            Err(e) => trouble(state, e),
        },
        Cmd::RevokeDevice(device) => match chat.revoke_device(&device).await {
            Ok(()) => {
                refresh_devices(chat, state, desk).await;
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
            // One file, nothing said: the same message the composer makes
            // with one file staged and no words.
            Box::pin(apply(
                chat,
                Cmd::Post(Draft {
                    files: vec![path],
                    ..Default::default()
                }),
                state,
                desk,
            ))
            .await;
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

        Cmd::Call { direct } => {
            let Some(channel) = desk.open else { return };
            let media = if direct {
                MEDIA_AUDIO | MEDIA_DIRECT
            } else {
                MEDIA_AUDIO
            };
            match chat.call(&channel, media, RING_SECS).await {
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
                open(&*chat, desk, state, channel);
                note(state, format!("Created {name}. Invite somebody to it."));
            }
            Err(e) => trouble(state, e),
        },
        Cmd::NewPublic { name, topic } => match chat.create_public(&name, &topic).await {
            Ok(channel) => {
                desk.restructure = true;
                open(&*chat, desk, state, channel);
                note(
                    state,
                    format!(
                        "Created {name}. Anybody may find and join it, and nothing in it is encrypted."
                    ),
                );
            }
            Err(e) => trouble(state, e),
        },
        // SIP-16 §Federated directory: the exchange's own rooms and its
        // peers' in one answer, each saying where it lives; an exchange
        // from before answers only its own, which `search` reads for us.
        Cmd::Find(query) => match chat.search(&query, 0).await {
            Ok(listing) => {
                // A new search is a new question, and a refusal from the
                // last one must not be drawn over its answer.
                state.send_modify(|s| s.join_trouble = None);
                let mut found = Vec::with_capacity(listing.rows.len());
                for c in listing.rows {
                    // SIP-43: a copy of a room from elsewhere is listed with
                    // where it lives. A public channel's home is answered to
                    // anyone at a copy; at its origin a non-member is refused
                    // and the room is named plainly, which is right. A room
                    // held nowhere here is named by the directory's word.
                    let name = if !c.here && !c.domain.is_empty() {
                        format!("{}@{}", c.name, c.domain)
                    } else {
                        match chat.home(&c.channel).await {
                            Ok(home) if home.origin != chat.exchange_key() => {
                                format!("{}@{}", c.name, at_home(&home))
                            }
                            _ => c.name,
                        }
                    };
                    found.push(Found {
                        channel: c.channel,
                        instance: c.instance,
                        name,
                        topic: c.topic,
                        members: c.members,
                        domain: c.domain,
                        here: c.here,
                    });
                }
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
                state.send_modify(|s| s.join_trouble = None);
                open(&*chat, desk, state, channel);
            }
            // Said in the general place as well, which is where somebody
            // looking at a conversation would see it -- and in the pane's
            // own field, which is the one the pane draws.
            Err(e) => {
                let why = e.to_string();
                trouble(state, e);
                state.send_modify(|s| s.join_trouble = Some(why));
            }
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
        Cmd::Mute { who, on } => {
            let Some(channel) = desk.open else { return };
            match chat.mute(&channel, &who, on).await {
                Ok(()) => {
                    desk.dirty.insert(channel);
                    note(
                        state,
                        if on {
                            "Muted: they read, and may not write.".into()
                        } else {
                            "Unmuted.".into()
                        },
                    );
                }
                Err(e) => trouble(state, e),
            }
        }
        Cmd::Report {
            target,
            reason,
            note: why,
        } => {
            let Some(channel) = desk.open else { return };
            match chat.report(&channel, target, reason, &why).await {
                Ok(()) => note(
                    state,
                    "Reported to the admins. They see who reported it; nobody else does.".into(),
                ),
                Err(e) => trouble(state, e),
            }
        }
        Cmd::LoadReports => {
            let Some(channel) = desk.open else { return };
            desk.reports_pending = 0;
            load_reports(chat, state, channel).await;
        }
        Cmd::Dismiss(id) => {
            let Some(channel) = desk.open else { return };
            match chat.dismiss(&channel, id).await {
                Ok(()) => {
                    desk.reports_pending = 0;
                    load_reports(chat, state, channel).await
                }
                Err(e) => trouble(state, e),
            }
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
/// SIP-48: what the exchange holds of this account's backup, and whether
/// this store can write one. The words, if shown, stay shown.
async fn backup_status(chat: &mut Chat, state: &watch::Sender<ChatState>) {
    let has_key = chat.backup_key().ok().flatten().is_some();
    let me = chat.me;
    match chat.backup_held(&me).await {
        Ok(held) => state.send_modify(|s| {
            let words = s.backup.as_ref().and_then(|b| b.words.clone());
            s.backup = Some(Backup {
                has_key,
                held: held.is_some().then_some(HeldBackup {
                    generation: held.generation,
                    written: held.written,
                    device: held.device,
                }),
                used: held.used,
                quota: held.quota,
                words,
            });
        }),
        Err(e) => trouble(state, e),
    }
}

/// SIP-44: what is arranged for this account's succession, as the exchange
/// has it. What was just written stays shown.
async fn succession_status(chat: &mut Chat, state: &watch::Sender<ChatState>) {
    let me = chat.me;
    let is_account = chat.credential().is_none();
    match chat.lodged_policy(&me).await {
        Ok(policy) => state.send_modify(|s| {
            let su = s.succession.get_or_insert_with(Default::default);
            su.is_account = is_account;
            su.lodged = policy.map(|p| (p.threshold, p.guardians));
        }),
        Err(e) => trouble(state, e),
    }
}

/// SIP-44 §The successor: what was pasted, read by its shape.
///
/// A will is one base58 token that decodes as one. Otherwise the first token
/// is the account's key, the rest are the guardians' vouches, and the policy
/// is fetched from where the account lodged it -- the successor was never
/// given the policy, only told there is one. Either way the exchange checks
/// everything again; this is so a wrong paste is said here, in words, before
/// anything is sent.
async fn proof_from(
    chat: &mut Chat,
    pasted: &str,
) -> Result<sqex_proto::succession::Proof, String> {
    use sqex_proto::succession::{Proof, Vouch, Will};
    let tokens: Vec<&str> = pasted.split_whitespace().collect();
    let [first, rest @ ..] = tokens.as_slice() else {
        return Err("paste a will, or an account's key and the vouches for you".into());
    };
    if let Ok(bytes) = bs58::decode(first).into_vec()
        && let Ok(will) = Will::decode(&bytes)
    {
        return Ok(Proof::Will(will));
    }
    let account: PubKey = first
        .parse()
        .map_err(|_| "that is neither a will nor an account's key".to_string())?;
    let policy = chat
        .lodged_policy(&account)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("{account} has no guardians' policy lodged here"))?;
    let mut vouches = Vec::with_capacity(rest.len());
    for v in rest {
        let bytes = bs58::decode(v)
            .into_vec()
            .map_err(|_| format!("that is not a vouch: {v}"))?;
        vouches.push(Vouch::decode(&bytes).map_err(|_| format!("that is not a vouch: {v}"))?);
    }
    if vouches.is_empty() {
        return Err(format!(
            "an account's key alone is not a claim: any {} of its {} guardians have to vouch \
             for you, one per line after it",
            policy.threshold,
            policy.guardians.len()
        ));
    }
    Ok(Proof::Guardians { policy, vouches })
}

/// SIP-56: read the room's reports for an admin, and clear the badge.
async fn load_reports(chat: &mut Chat, state: &watch::Sender<ChatState>, channel: [u8; 32]) {
    match chat.reports(&channel).await {
        Ok(rows) => {
            let reports: Vec<Report> = rows
                .into_iter()
                .map(|r| Report {
                    id: r.id,
                    reporter: r.reporter,
                    target: r.target,
                    reason: reason_word(r.reason),
                    at: r.at,
                    note: r.note,
                })
                .collect();
            state.send_modify(|s| {
                s.reports = reports;
                s.reports_pending = 0;
            });
        }
        Err(e) => trouble(state, e),
    }
}

fn note(state: &watch::Sender<ChatState>, said: String) {
    state.send_modify(|s| {
        s.note = Some(Note {
            said,
            at: unix_now(),
        })
    });
}

/// The wall clock, in Unix seconds.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn trouble(state: &watch::Sender<ChatState>, e: impl std::fmt::Display) {
    state.send_modify(|s| s.trouble = Some(e.to_string()));
}

/// Say what became of a draft; see [`Posted`].
fn posted(state: &watch::Sender<ChatState>, token: u64, trouble: Option<String>) {
    state.send_modify(|s| s.posted = Some(Posted { token, trouble }));
}

/// Put the open conversation away, without touching it at the exchange.
fn close(desk: &mut Desk, state: &watch::Sender<ChatState>) {
    desk.open = None;
    state.send_modify(|s| {
        s.open = None;
        s.lines.clear();
        s.copies.clear();
        s.stranded.clear();
        s.divider = None;
        s.unread_on_open = 0;
    });
}

/// Put a conversation on screen, taking the unread divider as it goes.
///
/// Takes the client because a conversation being looked at is folded whole:
/// what the list held of it was the last few rows. Here rather than at each
/// of the seven call sites, so that opening one and forgetting to fold it is
/// not a thing that can be written.
fn open(chat: &impl Local, desk: &mut Desk, state: &watch::Sender<ChatState>, channel: [u8; 32]) {
    ensure_folded(chat, desk, &channel);
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
        // Cleared with the lines and for the same reason: a frame can land
        // between this and the publish that fills them, and what it would
        // draw is the copies of the conversation just left, above the one
        // just opened. No test of it -- the publish recomputes them empty a
        // moment later either way, so an assertion here passes with the
        // line taken out, which is no assertion at all.
        s.copies.clear();
        s.stranded.clear();
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

#[cfg(test)]
mod naming_tests {
    use super::{Desk, Known, Trouble, wanted_names};
    use sqnr_core::PubKey;

    fn key(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    fn dm_with(peer: Option<PubKey>) -> Known {
        Known {
            peer,
            public: Some(false),
            group: peer.is_none(),
            label: String::new(),
            admins: Vec::new(),
            members: Vec::new(),
            marks: Vec::new(),
            timeline: Default::default(),
            folded: true,
            seen: 0,
            wanted: 0,
            last_at: 0,
            unread: 0,
            mentioned: 0,
            live_from: None,
            held_from: 0,
            told: 0,
            waiting: false,
            typing: false,
            fetched: false,
            trouble: Trouble::default(),
        }
    }

    /// The other party in each direct message, and nobody twice.
    ///
    /// Membership used to be a `contains` over a growing `Vec`, once per
    /// message in the conversation on screen — five hundred messages against
    /// fifty speakers is twenty-five thousand comparisons to produce a list of
    /// fifty. What must not change is the answer.
    #[test]
    fn every_direct_message_names_its_other_party_once() {
        let mut desk = Desk::default();
        desk.channels.insert([1u8; 32], dm_with(Some(key(7))));
        desk.channels.insert([2u8; 32], dm_with(Some(key(8))));
        // The same person in a second conversation, which is ordinary: an
        // identity at two exchanges, or a group and a direct message.
        desk.channels.insert([3u8; 32], dm_with(Some(key(7))));
        // A group has no single other party to name from here.
        desk.channels.insert([4u8; 32], dm_with(None));

        let mut want = wanted_names(&desk, key(1));
        want.sort_by_key(|k| k.to_string());
        assert_eq!(want, vec![key(7), key(8)]);
    }

    /// Never ourselves: we know who we are, and asking the exchange about it
    /// is a request per rebuild for a name already on screen.
    #[test]
    fn we_are_not_somebody_to_look_up() {
        let mut desk = Desk::default();
        desk.channels.insert([1u8; 32], dm_with(Some(key(9))));
        assert_eq!(wanted_names(&desk, key(9)), Vec::new());
    }

    /// Everybody in the room on screen, said anything or not: the mention
    /// picker offers them by name. And only the room on screen -- a member
    /// of a room nobody is looking at is not asked about.
    #[test]
    fn a_silent_member_of_the_open_room_is_named_and_of_another_is_not() {
        let mut desk = Desk::default();
        let mut open = dm_with(None);
        open.members = vec![
            super::Member {
                account: key(3),
                admin: true,
                muted: false,
            },
            super::Member {
                account: key(4),
                admin: false,
                muted: false,
            },
        ];
        let mut closed = dm_with(None);
        closed.members = vec![super::Member {
            account: key(5),
            admin: false,
            muted: false,
        }];
        desk.channels.insert([1u8; 32], open);
        desk.channels.insert([2u8; 32], closed);
        desk.open = Some([1u8; 32]);
        let mut want = wanted_names(&desk, key(3));
        want.sort_by_key(|k| k.to_string());
        assert_eq!(
            want,
            vec![key(4)],
            "the other member of the open room, not us, not the closed room's"
        );
    }

    /// Nothing held is nobody to ask about — and an empty list is what stops
    /// the round trip being made at all.
    #[test]
    fn an_empty_desk_asks_about_nobody() {
        assert!(wanted_names(&Desk::default(), key(1)).is_empty());
    }
}

#[cfg(test)]
mod searching_tests {
    use super::find_ignoring_case;

    /// The span is in the text as written, whatever case the word was said
    /// in, and it is the first one.
    #[test]
    fn a_word_is_found_whatever_its_case_and_the_span_is_in_the_original() {
        assert_eq!(
            find_ignoring_case("the Release check", "release"),
            Some(4..11)
        );
        assert_eq!(find_ignoring_case("RELEASE release", "release"), Some(0..7));
        assert_eq!(
            find_ignoring_case("said twice, said again", "said"),
            Some(0..4),
            "the first"
        );
        assert_eq!(&"the Release check"[4..11], "Release");
    }

    /// Letters outside ASCII fold too, and the span counts their bytes.
    #[test]
    fn letters_outside_ascii_fold_and_count_their_bytes() {
        let text = "wir gehen Über die Brücke";
        let found = find_ignoring_case(text, "über").unwrap();
        assert_eq!(&text[found.clone()], "Über");
        assert_eq!(found, 10..15, "Ü is two bytes");
    }

    /// Nothing there, a word longer than the text, and nothing asked for
    /// are all no hit -- an empty needle in particular must not match every
    /// message.
    #[test]
    fn what_is_not_there_is_not_found() {
        assert_eq!(find_ignoring_case("the release check", "rebase"), None);
        assert_eq!(find_ignoring_case("go", "gone"), None);
        assert_eq!(find_ignoring_case("anything", ""), None);
        assert_eq!(find_ignoring_case("", "a"), None);
    }
}

#[cfg(test)]
mod verified_tests {
    use super::verified_holder;
    use sqnr_core::PubKey;

    /// A name that resolves to the key it was verified as is fine; one that
    /// resolves elsewhere names the key that was verified; and a name never
    /// verified is nobody's business.
    #[test]
    fn a_verified_name_resolving_elsewhere_is_caught() {
        let k = |b: u8| PubKey::new([b; 32]);
        let handle = |key: &PubKey| match key.as_bytes()[0] {
            1 => Some("ada@squic.org".to_string()),
            2 => Some("bob@squic.org".to_string()),
            _ => None,
        };
        assert_eq!(
            verified_holder("ada@squic.org", &k(1), [k(1), k(2)], handle),
            None,
            "resolves to the verified key"
        );
        assert_eq!(
            verified_holder("Ada@Squic.org", &k(9), [k(1), k(2)], handle),
            Some(k(1)),
            "resolves elsewhere: the verified key is named, case aside"
        );
        assert_eq!(
            verified_holder("carol@squic.org", &k(9), [k(1), k(2)], handle),
            None,
            "never verified"
        );
        assert_eq!(
            verified_holder("ada@squic.org", &k(9), [k(2)], handle),
            None,
            "ada was never verified, only bob"
        );
    }
}

#[cfg(test)]
mod fetch_tests {
    use super::{Desk, RETRY_AFTER, Unfetched, after_a_failed_fetch, held_back};
    use std::time::Duration;

    /// **A dropped radio is not a deleted file.**
    ///
    /// Every failed fetch used to be remembered as final: the `Err(_)` arm
    /// put the blob in a set, nothing took it out, and the row said "no
    /// longer at the exchange" until the reader pressed Refetch or restarted.
    /// That is right for a blob past its retention window and wrong for a
    /// phone, where a link that blinks for one packet is the ordinary
    /// condition and arrives as the same `Err`.
    ///
    /// The exchange answering `found: false` is now the only thing that makes
    /// a file missing. Tested here rather than against a real exchange
    /// because the case that matters -- a fetch that fails on a blob the
    /// exchange still holds -- is one a test cannot easily produce, and this
    /// is the whole of the decision.
    #[test]
    fn only_the_exchange_saying_it_has_no_such_blob_makes_a_file_missing() {
        let now = std::time::Instant::now();
        assert_eq!(
            after_a_failed_fetch(Some(false), now),
            Unfetched::Gone,
            "the exchange says it does not hold it: that file is gone"
        );
        assert_eq!(
            after_a_failed_fetch(Some(true), now),
            Unfetched::Later(now),
            "the exchange still holds it, so the fetch failed for some other \
             reason and the file is not missing"
        );
        assert_eq!(
            after_a_failed_fetch(None, now),
            Unfetched::Later(now),
            "the exchange could not even be asked, which is a link that is \
             down -- the least likely moment to conclude a file is gone"
        );
    }

    /// And a blob waiting out a retry is left alone until it has.
    ///
    /// The part of the old set worth keeping: a link that is down must not
    /// turn into a fetch on every pass, which is sixty a second.
    #[test]
    fn a_blob_that_failed_is_left_alone_and_then_tried_again() {
        let mut desk = Desk::default();
        let blob = [7u8; 32];
        assert!(!held_back(&desk, &blob), "nothing has failed yet");

        desk.unfetchable
            .insert(blob, Unfetched::Later(std::time::Instant::now()));
        assert!(held_back(&desk, &blob), "just failed: not straight away");

        desk.unfetchable.insert(
            blob,
            Unfetched::Later(std::time::Instant::now() - RETRY_AFTER - Duration::from_secs(1)),
        );
        assert!(
            !held_back(&desk, &blob),
            "the pause has passed, so it is tried again without anybody \
             pressing anything"
        );

        desk.unfetchable.insert(blob, Unfetched::Gone);
        assert!(
            held_back(&desk, &blob),
            "gone stays gone, however long anybody waits"
        );
    }
}

/// How large a preview actually comes out.
#[cfg(test)]
mod preview_size_tests {
    use super::{MAX_PREVIEW, THUMBNAIL_EDGE, thumbnail_of};

    /// A photograph — smooth, which is what a camera gives — keeps most of
    /// the edge the cap allows. **The number that matters is the pixels**:
    /// this was 96 across for as long as it existed, and a phone draws a
    /// clip's thumbnail over seven hundred device pixels.
    #[test]
    fn a_photograph_gets_a_preview_worth_looking_at() {
        let photo = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(1920, 1080, |x, y| {
            // A gradient with a soft shape in it: what a JPEG is for.
            let r = (x * 255 / 1920) as u8;
            let g = (y * 255 / 1080) as u8;
            image::Rgb([r, g, 128])
        }));
        let preview = thumbnail_of(&photo).expect("a preview");
        assert!(preview.len() <= MAX_PREVIEW, "{} bytes", preview.len());
        let decoded = image::load_from_memory(&preview).expect("it decodes");
        assert!(
            decoded.width() >= THUMBNAIL_EDGE * 2 / 3,
            "a photograph's preview came out {} pixels across, against an edge of \
             {THUMBNAIL_EDGE}",
            decoded.width()
        );
    }

    /// And a picture that will not compress still gets one, smaller,
    /// inside the cap. The ladder's whole purpose, and the control for the
    /// test above: a single size would either fail here or be tiny there.
    #[test]
    fn a_busy_picture_still_gets_one_that_fits() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let noise = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(800, 600, |_, _| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let b = seed.to_le_bytes();
            image::Rgba([b[0], b[1], b[2], 255])
        }));
        let preview = thumbnail_of(&noise).expect("a preview");
        assert!(
            preview.len() <= MAX_PREVIEW,
            "{} bytes against {MAX_PREVIEW}",
            preview.len()
        );
    }
}

/// A clip's own first frame, as the thumbnail a bubble draws.
#[cfg(test)]
mod still_tests {
    use super::still_jpeg;

    /// **Bigger than the sender's, and a picture.** SIP-18 caps a preview
    /// at eight kilobytes, which makes it 96 pixels across; a phone draws a
    /// clip in a bubble over seven hundred device pixels, and the blur is
    /// the first thing anybody sees of a video. This is decoded from the
    /// blob itself, so it is bounded by what a bubble can show and not by
    /// what fits inside a message.
    #[test]
    fn a_still_is_a_picture_larger_than_a_preview_may_be() {
        let clip = include_bytes!("../../sigil-video/tests/fixtures/two_seconds.mp4");
        let jpeg = still_jpeg(clip).expect("the first frame decodes");
        let decoded = image::load_from_memory(&jpeg).expect("it is a picture");
        assert!(
            decoded.width() > 96,
            "a still {} pixels across is no better than the preview",
            decoded.width()
        );
        assert!(decoded.width() <= 960, "{}", decoded.width());
    }

    /// And nothing is made of what is not a clip. **The negative control**:
    /// a decoder that answered something for any bytes would pass the test
    /// above whatever it was handed.
    #[test]
    fn nothing_is_made_of_what_is_not_a_clip() {
        assert!(still_jpeg(b"not a video").is_none());
        assert!(still_jpeg(&[]).is_none());
    }
}

/// SIP-48: when the backup is written again.
#[cfg(test)]
mod backup_due_tests {
    use super::{BACKUP_EVERY, backup_due};

    const DAY: u64 = 24 * 60 * 60;

    /// A key made and nothing written is due now. **This is the case that
    /// matters**: somebody set a backup up, wrote the words down, and the
    /// account has been carrying on with nothing at the exchange.
    #[test]
    fn a_backup_never_written_is_due_at_once() {
        assert!(backup_due(0, 0, 1_758_559_925));
    }

    /// One written today is not.
    #[test]
    fn one_written_today_is_left_alone() {
        let now = 1_758_559_925;
        assert!(!backup_due(3, now - 60, now));
        assert!(!backup_due(3, now - DAY + 60, now));
    }

    /// One written a day ago is.
    #[test]
    fn one_a_day_old_is_written_again() {
        let now = 1_758_559_925;
        assert!(backup_due(3, now - DAY, now));
        assert!(backup_due(3, now - 9 * DAY, now));
        assert_eq!(BACKUP_EVERY.as_secs(), DAY, "the period this all rests on");
    }

    /// A clock that went backwards -- the exchange's stamp ahead of this
    /// machine's -- is not a reason to write one every tick.
    #[test]
    fn a_stamp_from_the_future_is_not_due() {
        let now = 1_758_559_925;
        assert!(!backup_due(3, now + 600, now));
    }
}

/// SIP-43: when this device's chain and the exchange's record of it are
/// two different things.
#[cfg(test)]
mod chain_apart_tests {
    use super::chain_apart;

    const A: [u8; 32] = [1u8; 32];
    const B: [u8; 32] = [2u8; 32];

    /// The same position, a different head: the one thing that means
    /// something. Two machines wrote under one device key, or this store
    /// was rolled back to before what the exchange kept.
    #[test]
    fn the_same_position_with_another_head_is_a_disagreement() {
        assert!(chain_apart(8, A, &[(7, B)]));
        assert!(chain_apart(8, A, &[(3, A), (7, B)]));
    }

    /// The same position with the same head is agreement -- **the negative
    /// control**: a check that answered "apart" for any answer at all would
    /// pass the test above and shout at every conversation.
    #[test]
    fn the_same_head_at_the_same_position_is_agreement() {
        assert!(!chain_apart(8, A, &[(7, A)]));
        assert!(!chain_apart(8, A, &[(5, B), (7, A)]));
    }

    /// An exchange that is behind is the ordinary case a second after
    /// posting, and one ahead is a copy this device has not caught up with:
    /// neither is a disagreement about a position they share.
    #[test]
    fn a_position_they_do_not_share_says_nothing() {
        assert!(!chain_apart(8, A, &[(6, B)]));
        assert!(!chain_apart(8, A, &[(9, B)]));
        assert!(!chain_apart(8, A, &[]));
    }

    /// A device that has signed nothing here has nothing to disagree about,
    /// whatever the exchange says.
    #[test]
    fn a_device_that_has_written_nothing_is_never_apart() {
        assert!(!chain_apart(0, A, &[(0, B), (1, B)]));
    }
}
