//! Messaging, as a sigil app.

pub mod announce;
pub mod command;
pub mod files;
pub mod frequent;
pub mod mention;
pub mod presence;
pub mod session;
pub mod siblings;

use session::RING_WINDOW;
pub use session::{
    Attached, Backup, ChatHandle, ChatState, Closing, Cmd, CrossRing, Draft, Found, Happened,
    HeldBackup, Hit, Line, LinkState, Linked, Member, Person, Posted, Quoted, Receipt, Report,
    Ring, Standing, Succession, Summary, Thumb, Trouble,
};

use std::collections::{HashMap, HashSet};

use sigil::app::{App, AppAction, AppContext, AppResponse, Notify, TabNotifications, Target};
use sigil::{ColorTheme, tokens};
use sigil_net::discovery;
use sqnr::config::Config;
use sqnr_core::PubKey;

/// What the bar over a pane is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bar {
    /// A conversation's name and controls, and the identity at the right.
    /// `back`: the list is not on screen, so Back to it is the leftmost
    /// control.
    Conversation { back: bool },
    /// The identity alone: nothing is connected, and the chevron is the
    /// way to another identity.
    Identity,
}

/// Where you are inside the chat app.
///
/// These go into the shell's global history as `Rc<dyn Any>` tokens, so back
/// and forward cross app boundaries: leaving the directory can take you to the
/// call you were on before, which is what a single history is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// The list and whatever is open in it.
    Conversations,
    /// The public directory: search, and join what it turns up.
    Directory,
    /// Who is in the open conversation, and what may be done about them.
    Members,
    /// The open conversation's name, topic and retention.
    Settings,
    /// Which devices act for this account, and how to link or revoke one.
    Devices,
    /// Searching what has been said: the box and what it found, on a card
    /// of its own. A phone's list heading has room for a magnifier and not
    /// for a box, and a box that pushes the list down is a search that
    /// costs something to have open.
    Search,
    /// **You**: your mark, your name, your exchange's domain, your keys, the
    /// way to the other things sigil does, and everything that is done to
    /// this identity rather than to a conversation.
    ///
    /// A card, not a menu. On a phone the identity menu had grown into a
    /// popup as tall as the screen with a scroll in it, hanging off a mark
    /// in the corner -- and the way between Chat, Exchange and Phone was in
    /// a *second* popup behind the title. Two menus, neither of them a
    /// place. This is the place.
    Me,
}

/// What a search here can and cannot reach. Said every time, in the count
/// and again on hovering it, not once in a help page: an empty result means
/// "not in what this client has opened", which is a different fact from
/// "never said", and only this client can tell them apart.
const ONLY_HERE: &str = "Searches what this client has opened. The exchange holds \
                         ciphertext and cannot search it.";

/// What the whole-screen picture viewer is drawing: which file, when it
/// was sent, its bytes, and where it sits among the message's others.
struct Whole {
    seq: u64,
    index: usize,
    moment: u64,
    /// The blob's name, so the texture is the one the transcript already
    /// decoded rather than a second copy of the same picture.
    id: String,
    bytes: std::sync::Arc<[u8]>,
    /// Which of how many, when the message carries more than one.
    among: Option<(usize, usize)>,
    previous: Option<usize>,
    next: Option<usize>,
}

/// What a saved file is called: `sigil-2026-09-22-18-32-05.png`.
///
/// The moment is the message's, not the save's, so saving the same picture
/// twice writes the same name rather than two copies an hour apart; the
/// extension is read out of the bytes ([`sigil_ui::attachment::extension`])
/// and never off the sender's word. Files used to be offered to the save
/// dialog under `[image 1920x1080, 2.1 MB]` -- what the bubble *says* about
/// one -- or under nothing at all, and landed on disk with no extension,
/// which on every desktop means no program will open them.
fn save_name(moment: u64, index: usize, bytes: Option<&[u8]>) -> String {
    let stamp = sigil_ui::clock::file_stamp(moment);
    let ext = sigil_ui::attachment::extension(bytes.unwrap_or(&[]));
    // **The second file of a message is not the first.** A gallery's
    // pictures are all the same moment, so a name built from the moment
    // alone has the second landing on top of the first. The first keeps the
    // plain name; the rest say which they are.
    match index {
        0 => format!("sigil-{stamp}.{ext}"),
        n => format!("sigil-{stamp}-{}.{ext}", n + 1),
    }
}

/// The exchange to suggest to an identity that names none.
///
/// A fresh identity on a fresh machine has no handle sidecar and no
/// `~/.sqnr/config`, so it has nowhere to talk to and the screen that says so
/// used to end at "add an exchange" -- a box, and a question somebody new has
/// no answer to. This is the answer: a public exchange anybody can join, so
/// the first thing after making an identity is one press rather than a
/// search for a domain name. Offered, not applied; nobody is connected
/// anywhere they did not choose.
pub const SUGGESTED_EXCHANGE: &str = "trunk.exchange";

/// What to call the default exchange in the switcher.
///
/// The default has no name in the roster — it is whatever this identity's own
/// SIP-38 handle and `~/.sqnr/config` resolve to — so it was labelled with a
/// truncated public key. That is unreadable and says nothing about *where* it
/// is, which is the only question a switcher answers. The domain it was
/// discovered at is the answer when there is one; the key is the fallback,
/// because a connection made to an address has no domain to report and a key
/// is still better than a word that names nothing.
/// SIP-85 §What the member MUST NOT do: whether a call may ask the exchange for
/// a SIP-25 introduction -- the invitation says the other side will, this side
/// allows it, and the connection is not carried by the home, whose address is
/// the one that would be introduced.
fn direct_allowed(ring_says: bool, preference: bool, carried: bool) -> bool {
    ring_says && preference && !carried
}

fn default_label(its: Option<ChatState>) -> String {
    match its {
        Some(s) => match (s.domain, s.exchange) {
            (Some(domain), _) if !domain.is_empty() => domain,
            (_, Some(key)) => sigil_ui::message::short(&key.to_string()),
            (_, None) => "default".to_string(),
        },
        None => "default".to_string(),
    }
}

/// Which of an identity's exchanges to show.
///
/// # Why the default is not always the answer
///
/// An identity's *default* exchange is whatever its own SIP-38 handle sidecar
/// and `~/.sqnr/config` resolve to, and an identity with neither has no
/// default at all — nothing is configured, so no session is started for it.
/// Falling back to the default regardless then showed **"not connected"** for
/// an identity that was connected perfectly well at a named exchange nobody
/// was looking at; and adding that exchange again was refused, correctly, as
/// one it already had. Not connected and already connected, about the same
/// account, at the same moment.
///
/// An explicit choice is always honoured, including a choice of a default
/// that does not work — somebody who picked it is owed the truth about it
/// rather than a silent move somewhere else.
fn showing_exchange(me: PubKey, chosen: Option<&String>, live: &[At]) -> String {
    if let Some(named) = chosen {
        return named.clone();
    }
    let default = (me, String::new());
    if live.contains(&default) {
        return String::new();
    }
    live.iter()
        .find(|at| at.0 == me)
        .map(|at| at.1.clone())
        .unwrap_or_default()
}

thread_local! {
    /// Bubbles drawn, as against reserved.
    ///
    /// **Counted here because the screen cannot say.** egui culls what is
    /// outside the clip rect from the accessibility tree already, so a
    /// transcript reports the same sixteen messages whether two hundred were
    /// laid out or twenty were — and laying them out is the entire cost this
    /// exists to avoid.
    ///
    /// Thread-local rather than a field: the harness owns the app, so a test
    /// has no handle to read a field through, and kittest runs its tests on a
    /// thread each — which makes this per-test rather than shared.
    static DREW: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many bubbles this thread has drawn since [`reset_drawn`].
#[doc(hidden)]
pub fn drawn_so_far() -> usize {
    DREW.with(|n| n.get())
}

#[doc(hidden)]
pub fn reset_drawn() {
    DREW.with(|n| n.set(0));
}

/// Everything about a message that decides how tall it draws.
///
/// A reserved row is a promise that the drawn one would be this tall, so what
/// this misses is what makes the transcript jump. Cheap on purpose: it runs per
/// message per frame, and it is a hash of the things a bubble's height is made
/// of, not of the message.
///
/// `grouped` is in it because the author line comes and goes with it; the width
/// is kept beside it rather than hashed, so a resized pane invalidates every
/// row at once and obviously.
fn shape_of(line: &Line, grouped: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    line.text.len().hash(&mut h);
    line.redacted.hash(&mut h);
    line.edited.hash(&mut h);
    grouped.hash(&mut h);
    line.name.is_some().hash(&mut h);
    line.reactions.len().hash(&mut h);
    line.standing.hash(&mut h);
    line.reply_to
        .as_ref()
        .map(|q| q.who.len() + q.said.len())
        .hash(&mut h);
    // A mention is a row of chips, and a name changes the row's width.
    line.me_mentioned.hash(&mut h);
    line.mentions
        .iter()
        .map(|m| m.label.len())
        .sum::<usize>()
        .hash(&mut h);
    h.finish()
}

/// The call we placed that has been picked up and not yet joined.
///
/// A free function over plain data, because the rule is the part worth
/// testing and the rest is a room being joined: **ours** (a ring that is not
/// ours is answered by pressing Answer, not by this), **answered** (there is
/// somebody in the room to talk to), and **not already in a call** (one
/// microphone, one room).
///
/// `in_a_call` rather than the map, so the test says what it means.
/// Whether a call this window is carrying has finished.
///
/// **The room is the fastest word.** A peer that was present and is no
/// longer has left, and the roster says so within a tick; the channel's own
/// end entry is authoritative but arrives when the exchange gets round to
/// it -- nine seconds, on one measured call, against the roster's one. Both
/// are kept: the entry covers a peer whose media path is still up, the
/// roster covers an entry that has not arrived.
///
/// `saw_peer` is what keeps the start of a call from reading as the end of
/// one: a room nobody has joined yet is also a room with nobody in it. A
/// two-party call that never goes through a room leaves it false, and ends
/// by the other two routes.
fn call_is_over(
    phase: sigil_net::Phase,
    present: usize,
    connecting: usize,
    saw_peer: bool,
    hung_up: bool,
    waiting: std::time::Duration,
) -> bool {
    let never = !matches!(phase, sigil_net::Phase::Live);
    let left = saw_peer && present == 0 && connecting == 0;
    phase == sigil_net::Phase::Ended || hung_up || left || (never && waiting > RING_WINDOW)
}

fn to_join<'a>(
    rings: &'a [Ring],
    in_a_call: bool,
    left: &HashSet<([u8; 32], u64)>,
) -> Option<&'a Ring> {
    if in_a_call {
        return None;
    }
    // Not one this window has already left: the ring stays listed until
    // the channel records the end, and for those seconds a call just hung
    // up was joined again and hung up again.
    rings
        .iter()
        .find(|r| r.mine && r.answered && !left.contains(&(r.channel, r.seq)))
}

/// How long to leave a session that died before starting it again.
///
/// A session ends by failing — a store still locked by the sigil that just
/// quit, an exchange that could not be reached to publish prekeys — and until
/// this existed it stayed dead for the life of the window: `reconcile` starts a
/// session only for an identity that has none, and a dead one is still one.
/// Restarting sigil a second after quitting it was enough to come up with four
/// identities holding nothing, with no way back but closing and opening each of
/// them.
///
/// Long enough that a store lock or a handshake has time to come good, short
/// enough that somebody watching does not conclude it is broken.
const RETRY: std::time::Duration = std::time::Duration::from_secs(3);

/// A session that keeps dying is tried again on a clock that doubles from
/// `RETRY`, up to this: a store locked for good, a domain that never
/// resolves, an exchange that is down. Every start is DNS, a handshake, a
/// fold of the whole store and a round of prekeys, on the runtime every
/// session shares -- at three seconds for ever, two such sessions kept a
/// window from drawing (2026-09-22). Reset once a session has stayed up.
const RETRY_MAX: std::time::Duration = std::time::Duration::from_secs(600);

/// Long enough to count as "it worked": a session up for this long that
/// then dies starts its retries from `RETRY` again.
const STAYED_UP: std::time::Duration = std::time::Duration::from_secs(60);

/// SIP-59: a session parked because the exchange said the account lives
/// elsewhere is tried again this often. An account can move back, and a
/// former home can be told so by another device; neither happens in three
/// seconds.
const PARKED_RETRY: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// Where the sequence numbers of messages *this client is still sending*
/// start: above anything an exchange will ever order, so an echo cannot be
/// confused with a message, and so the two sort in the order they happened.
const ECHO_SEQ: u64 = u64::MAX - 1024;

/// SIP-39: a cross-exchange ring has no conversation, and everything about a
/// ring here is keyed on one. The bridge is sixteen bytes and a channel is
/// thirty-two; the bridge in the first half and nothing in the second is a
/// key no real channel has, since a channel is a hash.
pub(crate) fn cross_key(bridge: [u8; 16]) -> [u8; 32] {
    let mut key = [0u8; 32];
    key[..16].copy_from_slice(&bridge);
    key
}

/// The words for a ringing call.
///
/// A free function over plain data for the same reason as [`duplicate_of`]:
/// what it decides is worth testing and a live session is the one thing a
/// test cannot arrange.
///
/// `called` is the identity being rung and `held` how many this host is
/// holding. With one there is nothing to tell apart, and naming it states a
/// fact nobody was in doubt about — the same rule as the exchange switcher
/// that is not drawn over a single exchange.
pub(crate) fn ring_said(from: &PubKey, label: &str, called: &str, held: usize) -> String {
    let who = sigil_ui::message::short(&from.to_string());
    if held > 1 {
        format!("{label} — from {who}, to {called}")
    } else {
        format!("{label} — from {who}")
    }
}

/// What was pressed on a ring's card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RingPress {
    Answer,
    Decline,
}

/// The card a ring is drawn as, from anywhere: who, by the name this client
/// has for them and by their key in full; what it is under them -- the
/// conversation, or "from another exchange"; and the two handsets.
///
/// **One shape for both kinds of ring, and one that fits a phone.** Each
/// ring drew its own, as a row of identicon, a text column carrying the key,
/// and two named buttons at the right -- and a 44-character key beside two
/// buttons is wider than a 360-point pane, so on the phone the buttons were
/// painted over the key and the words under the name. Seen on the device,
/// with a real call from another exchange. Now the handsets end the name's
/// row, where a long name truncates rather than pushes, and the key has a
/// row of its own, wrapped: the key in full, on the ring, always, because a
/// name is an assertion and this is the one screen where acting on the
/// wrong one puts somebody in a call with a stranger who chose a confusable
/// name.
fn ring_card(
    ui: &mut egui::Ui,
    theme: &ColorTheme,
    caller: &PubKey,
    named: &str,
    under: &str,
) -> Option<RingPress> {
    let key = caller.to_string();
    let mut pressed = None;
    egui::Frame::NONE
        .fill(theme.surface_elevated)
        .corner_radius(tokens::RADIUS_LG)
        .inner_margin(egui::Margin::same(tokens::SPACING_MD as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                sigil_ui::identicon(ui, &key, tokens::AVATAR_MD);
                ui.add_space(tokens::SPACING_SM);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // The handsets: the struck-through one in the destructive
                    // colour, the whole one in the colour of a thing going
                    // well. Each carries its word for anything that cannot
                    // see a shape.
                    if sigil_ui::icon_button_as_named(
                        ui,
                        sigil_ui::Icon::HangUp,
                        "Decline",
                        Some(theme.destructive),
                        false,
                    )
                    .clicked()
                    {
                        pressed = Some(RingPress::Decline);
                    }
                    if sigil_ui::icon_button_as_named(
                        ui,
                        sigil_ui::Icon::Call,
                        "Answer",
                        Some(theme.success),
                        false,
                    )
                    .clicked()
                    {
                        pressed = Some(RingPress::Answer);
                    }
                    ui.add_space(tokens::SPACING_SM);
                    // What the handsets left, and no more: a name that would
                    // not fit is cut, not laid over them.
                    // The name alone on its line: "Claude Marlow is ca…"
                    // is what the phone made of the name and the verb
                    // together beside two handsets. The verb goes under,
                    // with where the call is from or in.
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(egui::RichText::new(named).strong()).truncate());
                        // Wrapped, not cut: "is calling from another
                        // exchange" is a line and a half beside two
                        // handsets, and the half is the part that matters.
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("is calling {under}"))
                                    .small()
                                    .color(theme.text_secondary),
                            )
                            .wrap(),
                        );
                    });
                });
            });
            ui.add(
                egui::Label::new(egui::RichText::new(&key).monospace().small())
                    .wrap()
                    .selectable(true),
            );
        });
    pressed
}

/// The words for a mention: the summary and the body of the notification.
///
/// The same rules as [`ring_said`]: the sender by the name this client has
/// for them and by their key, since the name is an assertion (SIP-21); the
/// conversation as it is called here, `#` before a public one; the identity
/// it arrived at only when this host holds several. The body carries what
/// was said, shortened, so the notification is worth reading on its own.
pub(crate) fn mention_said(m: &session::Mention, called: &str, held: usize) -> (String, String) {
    let room = if m.public {
        format!("#{}", m.conversation)
    } else {
        m.conversation.clone()
    };
    let summary = if held > 1 {
        format!("{} mentioned you in {room}, as {called}", m.from_label)
    } else {
        format!("{} mentioned you in {room}", m.from_label)
    };
    let who = sigil_ui::message::short(&m.from.to_string());
    let body = if m.said.is_empty() {
        who
    } else {
        format!("{who} — {}", m.said)
    };
    (summary, body)
}

/// One or several messages arrived in one conversation, in words: who and
/// where, and what the last one said. With several identities held, which
/// one it came to.
pub(crate) fn arrivals_said(
    together: &[session::Arrival],
    called: &str,
    held: usize,
) -> (String, String) {
    let last = together.last().expect("at least one arrival");
    let room = if last.public {
        format!("#{}", last.conversation)
    } else {
        last.conversation.clone()
    };
    let mut summary = match (together.len(), last.direct) {
        (1, true) => last.from_label.clone(),
        (1, false) => format!("{} in {room}", last.from_label),
        (n, true) => format!("{n} new messages from {}", last.from_label),
        (n, false) => format!("{n} new messages in {room}"),
    };
    if held > 1 {
        summary.push_str(&format!(", as {called}"));
    }
    let body = if together.len() > 1 && !last.direct {
        format!("{}: {}", last.from_label, last.said)
    } else {
        last.said.clone()
    };
    (summary, body)
}

/// Where a notification about `channel` at `at` leads.
pub(crate) fn target(at: &At, channel: [u8; 32]) -> Target {
    Target {
        identity: at.0,
        exchange: at.1.clone(),
        channel,
        // A notification leads to a conversation. A ring's Answer is the
        // one that answers, and it is built where the ring is.
        answer: false,
    }
}

/// Where a picture sits inside the viewer, and how large.
///
/// `zoom` is a multiple of the size the picture is drawn at when it fits: 1 is
/// the whole of it on screen, and anything more is closer in. `pan` moves it
/// under the viewport, in screen pixels, from the middle.
///
/// A type of its own with the arithmetic on it, because the arithmetic is the
/// part worth testing: which part of a picture is under the pointer is exactly
/// the sort of thing that comes out wrong by a factor of the zoom and still
/// looks plausible.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Look {
    zoom: f32,
    pan: egui::Vec2,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
        }
    }
}

impl Look {
    /// The picture's size on screen: what it is when it fits, times the zoom.
    fn size(self, fitted: egui::Vec2) -> egui::Vec2 {
        fitted * self.zoom
    }

    /// Put the part of the picture the pointer is over under the pointer.
    ///
    /// **Moving, not dragging.** A zoomed picture in a window is a thing to
    /// look around, and asking somebody to hold a button down to do it makes
    /// them work for it: the pointer at the left edge of the window shows the
    /// left edge of the picture, and everything between follows.
    ///
    /// `from_middle` is where the pointer is relative to the middle of the
    /// window, so the two halves of the window map to the two halves of the
    /// picture and the middle maps to the middle.
    fn following(self, from_middle: egui::Vec2, view: egui::Vec2, fitted: egui::Vec2) -> Self {
        let half = view / 2.0;
        let across = egui::vec2(
            if half.x > 0.0 {
                (from_middle.x / half.x).clamp(-1.0, 1.0)
            } else {
                0.0
            },
            if half.y > 0.0 {
                (from_middle.y / half.y).clamp(-1.0, 1.0)
            } else {
                0.0
            },
        );
        let room = self.room(view, fitted);
        Look {
            zoom: self.zoom,
            // Opposite ways: to see the picture's left, it moves right.
            pan: egui::vec2(-across.x * room.x, -across.y * room.y),
        }
    }

    /// How far the picture may move before its own edge would come inside the
    /// window. Zero along an axis where it fits, so there is nothing to
    /// explore and it stays in the middle.
    fn room(self, view: egui::Vec2, fitted: egui::Vec2) -> egui::Vec2 {
        ((self.size(fitted) - view) / 2.0).max(egui::Vec2::ZERO)
    }

    /// The same picture at a different zoom, still over the window it is seen
    /// through.
    /// Moved by a drag, kept within the picture.
    fn panned(self, by: egui::Vec2, view: egui::Vec2, fitted: egui::Vec2) -> Self {
        let room = self.room(view, fitted);
        Look {
            zoom: self.zoom,
            pan: egui::vec2(
                (self.pan.x + by.x).clamp(-room.x, room.x),
                (self.pan.y + by.y).clamp(-room.y, room.y),
            ),
        }
    }

    fn zoomed(self, to: f32, view: egui::Vec2, fitted: egui::Vec2) -> Self {
        let now = Look { zoom: to, ..self };
        let room = now.room(view, fitted);
        Look {
            zoom: to,
            pan: egui::vec2(
                now.pan.x.clamp(-room.x, room.x),
                now.pan.y.clamp(-room.y, room.y),
            ),
        }
    }
}

/// Which conversation to open on arriving, if any.
///
/// The newest one, chosen **by its time** rather than by taking the first row
/// of the list: the list happens to arrive newest-first, and a view that
/// depends on somebody else's sort order is one that breaks silently the day
/// the sort changes. The tie-break is the channel, the same one the session
/// sorts by, so two conversations of the same age do not disagree about which
/// is newer.
///
/// `None` — leave it alone — when something is already open, when this has
/// been done once for this identity, when there is nothing to open, or in a
/// one-pane window, where opening a conversation *is* hiding the list.
fn first_look(
    open: Option<[u8; 32]>,
    done: bool,
    one_pane: bool,
    conversations: &[Summary],
) -> Option<[u8; 32]> {
    if open.is_some() || done || one_pane {
        return None;
    }
    conversations
        .iter()
        .max_by(|a, b| {
            a.at.unwrap_or(0)
                .cmp(&b.at.unwrap_or(0))
                .then_with(|| b.channel.cmp(&a.channel))
        })
        .map(|c| c.channel)
}

/// The added exchange name to drop, when a session lost the store lock to
/// another session of the **same identity**.
///
/// A free function over plain data rather than a method reaching into the
/// live sessions: what it decides is worth testing, and two sessions on one
/// exchange is precisely the state that cannot be arranged in a test — the
/// second one is refused, which is the whole subject.
///
/// `locked_out` is the exchange this session could not lock; `others` is every
/// other live session and the exchange it reports. `None` when the lock is
/// genuinely somebody else's — another sigil, or the terminal client — which
/// is a different problem with a different answer and keeps its own words.
fn duplicate_of(
    at: &At,
    locked_out: Option<PubKey>,
    others: &[(At, Option<PubKey>)],
) -> Option<String> {
    let wanted = locked_out?;
    let sibling = others
        .iter()
        .find(|(other, exchange)| other.0 == at.0 && other != at && *exchange == Some(wanted))?;
    // Always the *named* one of the pair. The default is not a name in the
    // roster — it is whatever the identity resolves to — so there would be
    // nothing to remove.
    let spare = if at.1.is_empty() {
        sibling.0.1.clone()
    } else {
        at.1.clone()
    };
    (!spare.is_empty()).then_some(spare)
}

/// What the identity block says when the exchange knows no name for you.
///
/// One word, because it goes under your own name in a corner and the sentence
/// it replaced — "no name at this exchange" — wrapped onto a second line and
/// pushed the block down rather than out.
const UNREGISTERED: &str = "unregistered";

/// A short form, shown over everything as a dialog.
///
/// # Why the column holds no forms
///
/// Adding and editing used to happen inline in the conversation list: a field
/// appeared under the profile, another under the heading, and the list moved
/// down to make room. Three consequences, all bad. The column had to be wide
/// enough for forms it shows for a few seconds a week; the fields never lined
/// up with each other or with anything either side of them; and a form that
/// pushes the list down loses your place in it.
///
/// So the rule is: **a form is never in the column.** A short one is a dialog,
/// which is what this is; anything about the open conversation is a route in
/// the content pane -- [`Route::Members`], [`Route::Settings`],
/// [`Route::Devices`], [`Route::Directory`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dialog {
    /// Start something: write to somebody, or make a group or a channel.
    Compose,
    /// Your own SIP-21 profile.
    Profile,
    /// Connect this identity to another exchange.
    Exchange,
    /// Claim a SIP-38 name here.
    Name,
    /// Compare safety words with this key's owner (SIP-41).
    Verify(PubKey),
    /// SIP-56: report a message (`target` is its seq) or the room (0) to the
    /// admins.
    Report { target: u64 },
}

/// One identity at one exchange: what a session, a store lock and a
/// conversation list all belong to.
///
/// The identity is the same key at every exchange and nothing else is — its
/// conversations, its channel keys and its SIP-17 counters belong to one and
/// do not move. So this pair, and not the key alone, is what everything here
/// is keyed on.
///
/// The exchange is the **name** somebody gave it, not the key it resolves to:
/// the key is not known until something dials it, and a session has to exist
/// before it can find out.
type At = (PubKey, String);

/// A field's width in a row that has a control after it: what it asked
/// for, or what is left once the control has its room. Width-driven, so a
/// narrow desktop window and a phone get the same answer.
/// What may be done about a member of a room, decided once and drawn twice.
///
/// The roster's row is a line of buttons where there is room and a menu where
/// there is not, and the two used to be two copies of the same five
/// decisions. Naming them means a control added here appears in both, rather
/// than in whichever arm whoever added it happened to be looking at.
#[derive(Clone, Copy)]
enum MemberAct {
    Verify,
    Kick,
    /// Make an admin, or stop being one.
    Grant(bool),
    /// SIP-56: they read, and may not write.
    Mute(bool),
    /// SIP-21, and not an admin's: personal, and applies everywhere.
    Block(bool),
}

/// What may be done about one member, decided once.
///
/// A free function rather than a method: it reads the state and the member
/// and nothing else, and keeping it out of the row's closure is what lets
/// the row borrow `ui` mutably twice over.
fn member_acts(
    state: &ChatState,
    member: &session::Member,
) -> Vec<(String, &'static str, MemberAct)> {
    let verified = state.verified.contains_key(&member.account);
    let blocked = state.blocked.contains(&member.account);
    let mut acts: Vec<(String, &'static str, MemberAct)> = Vec::new();
    acts.push((
        if verified { "Verified" } else { "Verify" }.to_string(),
        if verified {
            "You compared safety words with them. Open to see them again, or to take \
             the mark back."
        } else {
            "Compare six words with them, in person or over a call, and mark their key \
             as theirs."
        },
        MemberAct::Verify,
    ));
    if state.i_am_admin {
        acts.push((
            "Remove".to_string(),
            "Removes them and mints a new key, so what follows is not theirs. What they \
             already hold, they keep.",
            MemberAct::Kick,
        ));
        acts.push((
            if member.admin { "Demote" } else { "Make admin" }.to_string(),
            if member.admin {
                "They stop being an admin of this room."
            } else {
                "They may invite, remove and mute here."
            },
            MemberAct::Grant(!member.admin),
        ));
        // SIP-56. An admin cannot be muted (the SIP says demote first), so
        // this is for members.
        if !member.admin {
            acts.push((
                if member.muted { "Unmute" } else { "Mute" }.to_string(),
                if member.muted {
                    "They may write again. Everybody sees this."
                } else {
                    "An admin's act, seen by all: they read, and may not write. Not the \
                     same as muting this conversation for yourself."
                },
                MemberAct::Mute(!member.muted),
            ));
        }
    }
    // **Anybody's, not an admin's.** SIP-21's list is "per account, applies
    // everywhere" -- a personal act, and the hover says as much: *you* stop
    // hearing from them. It sat after the admin check's early return, so the
    // one control a member being harassed in somebody else's room actually
    // needs was the one they could not reach.
    acts.push((
        if blocked { "Unblock" } else { "Block" }.to_string(),
        if blocked {
            "They can reach you again."
        } else {
            // Never over-claimed: the exchange answers on your behalf and
            // tells them nothing, but a delivery mark that stops moving is a
            // thing somebody can notice.
            "You stop hearing from them. They are told nothing, though it can be worked \
             out."
        },
        MemberAct::Block(!blocked),
    ));
    acts
}

/// The same list drawn two ways: a row of buttons where there is room, a
/// menu where there is not.
///
/// One list either way, so the phone cannot quietly lose a control the
/// desktop has, which is how the two drift apart.
fn member_actions_ui(
    ui: &mut egui::Ui,
    acts: &[(String, &'static str, MemberAct)],
    narrow: bool,
    key: &str,
) -> Option<MemberAct> {
    let mut chose = None;
    if narrow {
        let more =
            sigil_ui::icon_button_named(ui, sigil_ui::Icon::More, "What may be done about them");
        egui::Popup::menu(&more).show(|ui| {
            // Rows with their icons, the shape every menu has: this one was
            // two words and a bare icon under them.
            for (label, hover, act) in acts {
                let icon = match act {
                    MemberAct::Verify => sigil_ui::Icon::Verified,
                    MemberAct::Kick => sigil_ui::Icon::Close,
                    MemberAct::Grant(_) => sigil_ui::Icon::People,
                    MemberAct::Mute(true) => sigil_ui::Icon::BellOff,
                    MemberAct::Mute(false) => sigil_ui::Icon::Bell,
                    MemberAct::Block(true) => sigil_ui::Icon::Muted,
                    MemberAct::Block(false) => sigil_ui::Icon::Sound,
                };
                if sigil_ui::icon_item(ui, icon, label)
                    .on_hover_text(*hover)
                    .clicked()
                {
                    chose = Some(*act);
                }
            }
            // The whole key, which the row itself has no width for.
            if sigil_ui::icon_item(ui, sigil_ui::Icon::Copy, "Copy key").clicked() {
                ui.ctx().copy_text(key.to_string());
            }
        });
    } else {
        for (label, hover, act) in acts {
            if ui.button(label.as_str()).on_hover_text(*hover).clicked() {
                chose = Some(*act);
            }
        }
    }
    chose
}

/// What is being typed, per identity.
///
/// **Keyed by account, not shared.** A draft typed as one identity must not
/// still be in the box after switching to another: the next Return would send
/// it as somebody else, which is a mistake the interface would have made on
/// your behalf and not mentioned.
/// What the bubble is told about a voice note.
///
/// `None` until somebody presses play: a note is not fetched for being
/// scrolled past, and a row with no player is a waveform and a length --
/// both of which came in the message itself (SIP-18).
fn voice_view(pane: &Pane, a: &session::Attached) -> Option<sigil_ui::attachment::Voice> {
    let note = pane.notes.get(&a.id);
    let fetching = pane.play_when_fetched.contains(&a.id);
    let trouble = pane.unplayable.contains_key(&a.id);
    if note.is_none() && !fetching && !trouble {
        return None;
    }
    Some(sigil_ui::attachment::Voice {
        playing: note.is_some_and(|n| n.playing()),
        done: note.map(|n| n.done()).unwrap_or(0.0),
        position_ms: note.map(|n| n.position_ms()).unwrap_or(0),
        fetching,
        // The word is fixed rather than the decoder's own: what a reader
        // can do about it is the same whatever it says, and the detail is
        // in the log where it is of use.
        trouble: trouble.then_some("this voice note will not play"),
    })
}

/// What the bubble is told about a video, from the pane's player for it if
/// there is one and the message's own word otherwise.
fn video_view<'a>(
    pane: &'a Pane,
    a: &'a session::Attached,
    place: sigil_ui::video::Place,
) -> sigil_ui::Video<'a> {
    let playing = pane.players.get(&a.id);
    let standing = if playing.is_some() {
        sigil_ui::Standing::Ready
    } else if pane.play_when_fetched.contains(&a.id) {
        sigil_ui::Standing::Fetching
    } else {
        sigil_ui::Standing::Held
    };
    sigil_ui::Video {
        frame: playing.and_then(|p| p.texture.as_ref()),
        preview: &a.preview,
        id: &a.id,
        standing,
        position_ms: playing.map(|p| p.player.position_ms()).unwrap_or(0),
        duration_ms: playing
            .map(|p| p.player.duration_ms())
            .or(a.duration_ms)
            .unwrap_or(0),
        playing: playing.is_some_and(|p| p.player.playing()),
        ended: playing.is_some_and(|p| p.player.ended()),
        volume: playing.map(|p| p.player.volume()).unwrap_or(1.0),
        trouble: pane.unplayable.get(&a.id).map(String::as_str),
        shape: playing
            .map(|p| {
                let d = p.player.description();
                (d.width, d.height)
            })
            .or(a.shape),
        described: &a.described,
        place,
    }
}

/// A video being played in this pane: the player, and the texture the
/// picture due now is uploaded into.
struct Playing {
    player: sigil_video::Player,
    texture: Option<egui::TextureHandle>,
    /// The time of the picture in the texture, so a pass that finds the
    /// same one uploads nothing.
    shown: Option<u64>,
    trace: VideoTrace,
}

/// How pictures land on the window's frames, printed every few seconds to
/// stderr when `SIGIL_VIDEO_TRACE` is set. The window is the only honest
/// instrument for stutter, and this is how it is read.
#[derive(Default)]
struct VideoTrace {
    on: bool,
    started: Option<std::time::Instant>,
    repaints: Vec<u64>,
    /// (wall ms, position ms, picture ms) at each change of picture.
    landed: Vec<(u64, u64, u64)>,
    reported: u64,
}

impl VideoTrace {
    fn new() -> VideoTrace {
        VideoTrace {
            on: std::env::var_os("SIGIL_VIDEO_TRACE").is_some(),
            ..Default::default()
        }
    }

    fn wall(&mut self) -> u64 {
        self.started
            .get_or_insert_with(std::time::Instant::now)
            .elapsed()
            .as_millis() as u64
    }

    fn repaint(&mut self, _position: u64) {
        if !self.on {
            return;
        }
        let wall = self.wall();
        self.repaints.push(wall);
        if wall >= self.reported + 5_000 {
            self.reported = wall;
            self.report();
        }
    }

    fn landed(&mut self, position: u64, at: u64) {
        if !self.on {
            return;
        }
        let wall = self.wall();
        self.landed.push((wall, position, at));
    }

    fn report(&mut self) {
        let mut gaps: Vec<i64> = self
            .repaints
            .windows(2)
            .map(|w| (w[1] - w[0]) as i64)
            .collect();
        gaps.sort();
        if gaps.is_empty() {
            return;
        }
        let n = gaps.len();
        let mut between: Vec<i64> = self
            .landed
            .windows(2)
            .map(|w| (w[1].0 - w[0].0) as i64)
            .collect();
        let skipped = self
            .landed
            .windows(2)
            .filter(|w| w[1].2 - w[0].2 > 40)
            .count();
        let mut late: Vec<i64> = self
            .landed
            .iter()
            .map(|(_, p, a)| *p as i64 - *a as i64)
            .collect();
        between.sort();
        late.sort();
        let m = between.len().max(1);
        eprintln!(
            "video: {} repaints, gap ms median {} p90 {} p99 {} max {} | {} pictures ({skipped} skipped), late ms median {} max {}, between ms min {} median {} p90 {} p99 {} max {}",
            n + 1,
            gaps[n / 2],
            gaps[n * 9 / 10],
            gaps[n * 99 / 100],
            gaps[n - 1],
            late.len(),
            late.get(late.len() / 2).copied().unwrap_or(0),
            late.last().copied().unwrap_or(0),
            between.first().copied().unwrap_or(0),
            between.get(m / 2).copied().unwrap_or(0),
            between.get(m * 9 / 10).copied().unwrap_or(0),
            between.get(m * 99 / 100).copied().unwrap_or(0),
            between.last().copied().unwrap_or(0),
        );
        self.repaints.clear();
        self.landed.clear();
    }
}

impl Playing {
    /// Upload the picture due now, if it is a new one.
    fn refresh(&mut self, ctx: &egui::Context, id: &str) {
        self.trace.repaint(self.player.position_ms());
        let Some((at, image)) = self.player.frame() else {
            return;
        };
        if self.shown == Some(at) {
            return;
        }
        self.trace.landed(self.player.position_ms(), at);
        let data = egui::ImageData::Color(image);
        match &mut self.texture {
            Some(t) => t.set(data, egui::TextureOptions::LINEAR),
            None => {
                self.texture = Some(ctx.load_texture(
                    format!("video-{id}"),
                    data,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        self.shown = Some(at);
    }
}

/// A file in the composer, not yet sent.
struct Staged {
    /// A file on this machine, to be uploaded -- or one already on the
    /// message being rewritten, carried across unless taken out.
    source: Source,
    name: String,
    /// SIP-18's kind, from the name.
    kind: u8,
    /// A small PNG of it, once the decoding thread has made one; `None`
    /// until then, and for a kind that has no picture.
    preview: Option<std::sync::Arc<[u8]>>,
}

/// Where a staged file comes from.
///
/// **A rewrite is a whole post**, so the files on the original are part of
/// it: they are staged like new ones, count against the same cap, and go
/// unless somebody removes them -- which is how a picture is taken off a
/// message.
enum Source {
    File(std::path::PathBuf),
    /// By blob id; the session picks it out of the original.
    Carried(String),
}

impl Staged {
    /// Loaded from a message being rewritten.
    fn carried(a: &Attached) -> Staged {
        Staged {
            source: Source::Carried(a.id.clone()),
            name: a.described.clone(),
            kind: a.kind,
            preview: (!a.preview.is_empty()).then(|| a.preview.clone()),
        }
    }

    /// The URI its thumbnail is registered under: a path or a blob id,
    /// never a position, so two tiles never share a picture.
    fn uri(&self) -> String {
        match &self.source {
            Source::File(path) => format!("bytes://staged-{}", path.display()),
            Source::Carried(id) => format!("bytes://{id}-preview"),
        }
    }
}

/// The wire's cap on files in one message (SIP-19).
const MOST_FILES: usize = sqex_proto::message::MAX_ATTACHMENTS;

/// How long the wash on a message the transcript went to takes to fade, in
/// seconds, and how strong it starts. Long enough to be found, short enough
/// that the message is not marked when the next thing is read.
const WASH_FOR: f64 = 1.5;
const WASH: f32 = 0.35;

/// A screen this short has no room for a dialog laid out down the page.
///
/// A phone held sideways is 360 points tall; a phone upright is 804. Nothing
/// in between is a real device, so the exact number matters less than which
/// side of it each one falls: a desktop window has to be deliberately
/// squashed to get under this.
const SHORT: f32 = 520.0;

/// What a dialog may take on a short screen, when it has a second column to
/// put something in. See [`SHORT`].
const WIDE: f32 = 640.0;

/// How much of the wash is left at `now`, from 1 at `since` to 0 at
/// [`WASH_FOR`] later.
fn wash_left(since: f64, now: f64) -> f32 {
    (1.0 - (now - since) / WASH_FOR).clamp(0.0, 1.0) as f32
}

/// What was in the box when a rewrite began: the words, their mentions and
/// the files staged with them.
struct Stashed {
    composing: String,
    mentions: Vec<(String, PubKey)>,
    staged: Vec<Staged>,
}

/// Everything the composer held when a message was sent, so the message can
/// be put back if it does not go.
struct Unsent {
    token: u64,
    composing: String,
    mentions: Vec<(String, PubKey)>,
    carried_mentions: Vec<PubKey>,
    staged: Vec<Staged>,
    editing: Option<u64>,
    replying: Option<u64>,
}

struct Pane {
    /// Videos with a player, by blob id. A player is made when the viewer
    /// opens on a video and dropped when it closes, or when the message
    /// leaves the conversation on screen; a conversation switched away
    /// from stops its video.
    players: HashMap<String, Playing>,
    /// A voice note being recorded here, if one is. Dropping it stops
    /// the microphone, which is what leaving the conversation does.
    recording: Option<sigil_video::note::Recorder>,
    /// SIP-18 voice notes this pane has open, by attachment id. Made when
    /// somebody presses play and the bytes are in hand; dropped with the
    /// attachment, which stops the sound.
    notes: HashMap<String, sigil_video::note::Note>,
    /// Videos play was pressed on before their bytes had arrived: they
    /// start the moment they do -- in the viewer, which is where a press
    /// on a video in the transcript goes.
    play_when_fetched: HashSet<String>,
    /// Which message each of those is on, so the viewer can open on it.
    open_when_fetched: HashMap<String, (u64, usize)>,
    /// The viewer is showing a video on the whole screen; put back when it
    /// closes.
    whole_screen: bool,
    /// Why a video will not play, by blob id.
    unplayable: HashMap<String, String>,
    /// What is in the box. Taken out to be sent, and held in `in_flight`
    /// until the session says the message went: a message the exchange
    /// refused comes back here, because retyping a message the program lost
    /// is the worst thing a chat client can do to somebody.
    composing: String,
    /// Messages sent and not yet answered, oldest first, each under the
    /// token its draft carried; see `session::Posted`. Kept whole -- words,
    /// mentions, files, what it replied to or rewrote -- so a refused one
    /// can be put back exactly as it was.
    in_flight: Vec<Unsent>,
    /// The next draft's token. From one: zero is a draft nobody waits on.
    next_token: u64,
    /// A refused message that could not go straight back into the box
    /// because something else had been typed since. Offered under the box
    /// instead, until it is taken or dismissed.
    put_back: Option<(Unsent, String)>,
    /// What was in the box when a rewrite began, put back when the rewrite
    /// is sent or abandoned. A message half typed is not the price of
    /// correcting an older one.
    stashed: Option<Stashed>,
    /// Mentions on the message being rewritten whose names are not in its
    /// words any more (the person was renamed since). Sent with the rewrite
    /// as they are: nothing this rewrite typed can stand for them.
    carried_mentions: Vec<PubKey>,
    /// Whom the text mentions so far: each label chosen from the list, and
    /// the key it stood for. Read at send time against the text, so a name
    /// deleted from the box is a mention that is not sent.
    mentions: Vec<(String, PubKey)>,
    /// Files waiting in the composer, to go with the next message: each
    /// with its thumbnail once one is decoded, and a way to take it back
    /// out before sending.
    staged: Vec<Staged>,
    /// Thumbnails arriving from the threads that decode them, by path.
    previews: Option<std::sync::mpsc::Receiver<(std::path::PathBuf, Option<Vec<u8>>)>>,
    previews_tx: Option<std::sync::mpsc::Sender<(std::path::PathBuf, Option<Vec<u8>>)>>,
    /// Why not every file picked was staged.
    staging_trouble: Option<String>,
    /// Why the last line typed as a command did nothing.
    command_trouble: Option<String>,
    /// The verify dialog's checkbox: lodge the SIP-27 claim as well.
    attest_too: bool,
    /// Which row of the mention list the keyboard is on.
    picking: usize,
    /// Escape closed the list for this `@`; typing reopens it.
    picker_dismissed: bool,
    /// The box's widget id, once drawn: what "the box had the keyboard"
    /// is asked of.
    field: Option<egui::Id>,
    /// The key being added as a contact.
    adding: String,
    add_trouble: Option<String>,
    /// Which dialog is open over this pane, if any. One at a time: two forms
    /// over each other is a thing nobody can back out of.
    dialog: Option<Dialog>,
    /// The message being replied to, if any.
    replying: Option<u64>,
    /// Whether we have told the channel we are typing, so the signal is sent
    /// on the edges rather than on every keystroke.
    announced_typing: bool,
    /// The message being rewritten, if any. What Enter does depends on it.
    editing: Option<u64>,
    /// The directory search box.
    query: String,
    /// The directory has been asked once for everything, on arriving. Not
    /// [`Pane::looked`], which is about opening the newest conversation.
    listed: bool,
    /// The key of a device being linked.
    linking: String,
    /// A credential another device wrote, being presented by this one.
    presenting: String,
    /// SIP-47's `sqx-pair:` string, or a name, being typed.
    pairing: String,
    /// Whom this device is claiming, while the sessions ask.
    claim_pending: Option<String>,
    /// The message search box.
    searching: String,
    /// The box was just opened and has not been given the finger yet: a box
    /// that opens without the keyboard is a box you have to press twice.
    search_focus: bool,
    /// The exchange being added.
    exchange: String,
    /// SIP-85: the "through my home" box on Add an exchange.
    exchange_via: bool,
    /// SIP-56: the report dialog's reason and note.
    report_reason: u8,
    report_note: String,
    /// SIP-48: the words typed to restore from, and the drop's second step.
    restoring: String,
    confirming_drop: bool,
    /// SIP-44: the fields of the succession section -- a successor's key, a
    /// guardian being added and the ones added, how many it takes, who is
    /// being vouched for, and what a successor pasted to claim.
    successor: String,
    guardian: String,
    guardians: Vec<PubKey>,
    threshold: u8,
    vouch_account: String,
    vouch_successor: String,
    claiming: String,
    /// The message whose file is being forwarded.
    forwarding: Option<(u64, usize)>,
    /// The picture being looked at full size: a message and which of its
    /// files. Kept per identity like everything else here, so switching away
    /// and back does not leave somebody else's picture over the screen.
    viewing: Option<(u64, usize)>,
    /// How that picture is being looked at: how close in, and where.
    look: Look,
    /// The key being invited to the open channel.
    inviting: String,
    /// The SIP-38 name being claimed.
    naming: String,
    /// The channel settings fields, and the channel they were filled from.
    /// `None` until the pane has been drawn; see `fill_settings`.
    channel_name: String,
    channel_topic: String,
    settings_for: Option<[u8; 32]>,
    retention_days: u32,
    /// Destroying a channel is asked twice, because it cannot be undone.
    confirming_destroy: bool,
    /// What is being typed into it. Held separately from the published
    /// profile so cancelling really cancels.
    name: String,
    title: String,
    /// Whether a page has been asked for and has not arrived.
    ///
    /// **A page is asked for once.** The control that asks is drawn at the top
    /// of the transcript and asks by *being on screen*, which is every frame
    /// until the answer arrives -- sixty a second against a session that
    /// answers every seven hundred milliseconds. So reaching the top ordered
    /// forty pages, the transcript grew by hundreds of messages, and the
    /// reader ended up somewhere around the middle of the conversation.
    asking: bool,
    /// The conversation on screen and how much of it was still above the top
    /// of it, last pass.
    ///
    /// **This is what says a page arrived**: `earlier` falling is prepending
    /// and nothing else -- a message arriving at the bottom does not change
    /// it, and neither does a picture finding its size. Kept with the channel
    /// because a pane is per identity, and switching conversations would
    /// otherwise read as a page arriving in the new one.
    saw: (Option<[u8; 32]>, usize),
    /// The transcript's content height and scroll offset last pass.
    ///
    /// Kept here because the control that asks for earlier messages is drawn
    /// *inside* the scroll area, where neither is known yet.
    scrolled: (f32, f32),
    /// How tall each message drew, and at what width and shape, so one nobody
    /// can see can be **reserved** rather than drawn. See `messages_ui`.
    ///
    /// Cleared when the conversation changes: a height is about one message in
    /// one conversation and means nothing in the next.
    tall: HashMap<u64, (u64, f32, f32)>,
    /// A message to scroll to -- a quote was pressed, or a search result
    /// chosen -- by the channel it is in and its place there. Held until the
    /// message is on screen, which may take a page or two arriving first; see
    /// `messages_ui`. The channel is carried because a sequence number is only
    /// meaningful in one, and the conversation on screen can change before
    /// the message is found.
    jump: Option<([u8; 32], u64)>,
    /// The message a jump landed on, and when, so it can be washed in the
    /// accent for a moment: a transcript that has scrolled to one bubble
    /// among fifty alike gives no sign of which, and the reader who chose a
    /// search result is left looking for the word again.
    marked: Option<(u64, f64)>,
    /// The search result last chosen, drawn as chosen in the results while
    /// the search stands, so the list says which one the transcript went to.
    chosen: Option<([u8; 32], u64)>,
    /// Whether the first look has happened for this identity.
    ///
    /// Opening the newest conversation is something sigil does **once**, on
    /// arriving. Without the flag, closing a conversation would reopen it on
    /// the very next pass, and there would be no way to sit in the list.
    looked: bool,
}

impl Pane {
    /// Begin answering a message. A rewrite in progress is abandoned first:
    /// the two are not a pair -- a rewrite that also picked up a reply would
    /// silently re-thread the message -- and only one of them is shown above
    /// the box, so only one may be armed.
    fn reply_to(&mut self, seq: u64) {
        if self.editing.is_some() {
            self.stop_rewriting();
        }
        self.replying = Some(seq);
    }

    /// Begin rewriting one of our messages: its words and its mentions come
    /// into the box, so a rewrite is a correction of what is there rather
    /// than a retyping of it. A reply being written is dropped, for the
    /// reason `reply_to` gives; words being typed are kept aside and come
    /// back when the rewrite is done with.
    fn rewrite(&mut self, line: &Line) {
        self.replying = None;
        if self.editing.is_none() && (!self.composing.trim().is_empty() || !self.staged.is_empty())
        {
            self.stashed = Some(Stashed {
                composing: std::mem::take(&mut self.composing),
                mentions: std::mem::take(&mut self.mentions),
                staged: std::mem::take(&mut self.staged),
            });
        }
        self.editing = Some(line.seq);
        self.composing = line.text.clone();
        // The names it mentions, so a name left in the words keeps its key
        // and a name taken out loses it -- the rewrite is a whole post, not
        // a patch to the words. One whose name is no longer in the words as
        // they stand -- the person has been renamed since -- was not typed
        // out by this rewrite either, and is carried as it is.
        let (named, renamed): (Vec<_>, Vec<_>) = line.mentions.iter().partition(|m| {
            mention::mentions_in(&line.text, &[(m.label.clone(), m.key)]).len() == 1
        });
        self.mentions = named.iter().map(|m| (m.label.clone(), m.key)).collect();
        self.carried_mentions = renamed.iter().map(|m| m.key).collect();
        // And its files, as tiles beside any new ones.
        self.staged = line.attachments.iter().map(Staged::carried).collect();
        self.staging_trouble = None;
    }

    /// Abandon a rewrite. The old text must not stay in the box, where the
    /// next Return would post it again as a new message; what was being
    /// typed before comes back instead.
    fn stop_rewriting(&mut self) {
        self.editing = None;
        self.composing.clear();
        self.mentions.clear();
        self.carried_mentions.clear();
        // The original's files, and any staged for the rewrite: neither
        // belongs to the next message.
        self.staged.clear();
        self.staging_trouble = None;
        self.unstash();
    }

    /// Whatever was set aside for a rewrite, back in the box.
    fn unstash(&mut self) {
        if let Some(was) = self.stashed.take() {
            self.composing = was.composing;
            self.mentions = was.mentions;
            self.staged = was.staged;
        }
    }

    /// The × above the box: whichever of the two is armed.
    fn cancel_head(&mut self) {
        if self.editing.is_some() {
            self.stop_rewriting();
        } else {
            self.replying = None;
        }
    }
}

impl Default for Pane {
    fn default() -> Self {
        Pane {
            pairing: String::new(),
            exchange_via: false,
            report_reason: 1,
            report_note: String::new(),
            restoring: String::new(),
            confirming_drop: false,
            successor: String::new(),
            guardian: String::new(),
            guardians: Vec::new(),
            threshold: 2,
            vouch_account: String::new(),
            vouch_successor: String::new(),
            claiming: String::new(),
            claim_pending: None,
            players: HashMap::new(),
            notes: HashMap::new(),
            recording: None,
            play_when_fetched: HashSet::new(),
            open_when_fetched: HashMap::new(),
            whole_screen: false,
            unplayable: HashMap::new(),
            composing: String::new(),
            in_flight: Vec::new(),
            next_token: 1,
            put_back: None,
            stashed: None,
            carried_mentions: Vec::new(),
            mentions: Vec::new(),
            staged: Vec::new(),
            previews: None,
            previews_tx: None,
            staging_trouble: None,
            command_trouble: None,
            attest_too: false,
            picking: 0,
            picker_dismissed: false,
            field: None,
            adding: String::new(),
            add_trouble: None,
            dialog: None,
            replying: None,
            announced_typing: false,
            editing: None,
            query: String::new(),
            listed: false,
            linking: String::new(),
            presenting: String::new(),
            searching: String::new(),
            search_focus: false,
            exchange: String::new(),
            forwarding: None,
            viewing: None,
            look: Look::default(),
            inviting: String::new(),
            naming: String::new(),
            channel_name: String::new(),
            channel_topic: String::new(),
            settings_for: None,
            asking: false,
            saw: (None, 0),
            tall: HashMap::new(),
            jump: None,
            marked: None,
            chosen: None,
            scrolled: (0.0, 0.0),
            looked: false,
            // The protocol's own default, not zero: a retention field starting
            // outside its own range offers to set something the exchange will
            // refuse, and the refusal would read as sigil's fault.
            retention_days: 30,
            confirming_destroy: false,
            name: String::new(),
            title: String::new(),
        }
    }
}

pub struct ChatApp {
    /// One live session per identity **at each of its exchanges**, and not one
    /// for whichever is being looked at. A message arriving somewhere you are
    /// not currently showing is still a message you want to be told about.
    sessions: HashMap<At, ChatHandle>,
    /// What every call this window joins is opened with: the microphone and
    /// the speaker, unless a test says a tone and nothing -- a runner with
    /// no sound card ends a call on opening it, which reads as "the answer
    /// did not connect" from outside.
    call_opts: sigil_net::CallOpts,
    /// When each session was last started, so one that dies is tried again —
    /// and not faster than [`RETRY`].
    started: HashMap<At, std::time::Instant>,
    /// How many times in a row a session has died without staying up, so
    /// the retry can back off; and when each dead one was first seen dead,
    /// which is what the wait is counted from.
    failures: HashMap<At, u32>,
    died: HashMap<At, std::time::Instant>,
    /// Each identity's file, by key -- what a session records beside it
    /// (the home, SIP-60); filled as sessions are reconciled.
    identity_paths: HashMap<PubKey, std::path::PathBuf>,
    /// How many sessions this app has started, for tests to count.
    starts: usize,
    /// What the platform was last told about a call being up, so it is told
    /// on change rather than on every pass. See [`ChatApp::announce_calling`].
    told_calling: Option<String>,
    /// Sessions told to stop that still hold their store lock. One of these
    /// must not be reopened yet; see [`Closing`].
    closing: Vec<(At, Closing)>,
    panes: HashMap<At, Pane>,
    /// Somebody asked for the opening screen, to be somebody else.
    ///
    /// Set in the identity menu and answered at the end of `render`, because
    /// the menu is drawn several layers inside it and an app says what it
    /// wants of the shell by **returning** it, not by reaching for it.
    switching: bool,
    /// Whether the conversation column is on screen. **Open to begin with.**
    ///
    /// One preference for the whole app rather than one per identity: it is
    /// about how much room the transcript gets, which is a fact about the
    /// window and not about who you are being in it.
    ///
    /// It started closed, on the reasoning that a conversation is what
    /// somebody opened sigil to read. Signing in to a transcript with no
    /// list beside it reads as an application with one chat in it, so the
    /// list is there from the start and the hamburger puts it away.
    columns_open: bool,
    /// Whether the last pass drew one pane: the list *or* a conversation.
    /// Read by the phone's app bar, which is drawn before the body and
    /// has to know whether it is heading a list or a conversation.
    single: bool,
    config: Config,
    /// Where the stores live, when it is not `~/.sqex/chat`.
    ///
    /// **Tests must set this.** The real store is somebody's only copy of their
    /// conversations — an epoch key arrives sealed against a one-time prekey and
    /// opening it spends the prekey, so what is on disk is the only copy that
    /// will exist tomorrow. A test that reconciled against the real path would
    /// also take its `flock`, and refuse the person running it their own client.
    store_root: Option<std::path::PathBuf>,
    /// A pinned clock, for snapshots. See `set_now_for_test`.
    now: Option<u64>,
    /// A state to draw instead of a session's. See `show_state_for_test`.
    fixed: Option<ChatState>,
    /// What has been asked of the session while a fixed state is installed.
    /// See `send_as`; tests only, and empty in the real application.
    sent: Vec<String>,
    /// Every picture this app has handed to egui, by the name it gave it.
    ///
    /// **egui keeps what it is given until it is told not to.** Nothing in
    /// this tree ever called `forget_image`, so the encoded bytes of every
    /// picture ever drawn stayed in its loader cache for the life of the
    /// process -- beside the session's own copy and the texture on the GPU.
    /// The session now puts files down when it holds too many; this is how
    /// egui hears about it.
    drawn: std::collections::HashSet<String>,
    /// The call this identity is carrying audio for, and which invitation it
    /// belongs to.
    ///
    /// Signalling lives in the session (it is chat traffic); the audio is a
    /// SIP-13 room on its own connection, which is `sigil-net`'s job. This is
    /// the join between them, and nothing else needs to know both halves.
    calls: HashMap<PubKey, Live>,
    /// Calls this window has left, by conversation and ring, so a ring still
    /// listed as answered is not joined again on the way out.
    left: HashSet<([u8; 32], u64)>,
    /// A ring whose **Answer** was pressed on a notification, waiting for
    /// the conversation it belongs to to be in hand.
    ///
    /// Not answered on the spot, because the press can arrive before there
    /// is anything to answer: on a phone the window may be starting from
    /// cold, and the session, its state and the ring itself all land later.
    /// Held with the moment it was pressed so a ring that never turns up
    /// stops being waited for rather than latching forever.
    answering: Option<(At, [u8; 32], std::time::Instant)>,
    /// What tells the platform about rings, mentions and arrivals -- from a
    /// frame, or from a session's wake when no frame is coming. See
    /// [`announce::Announcer`].
    announcer: std::sync::Arc<announce::Announcer>,
    /// SIP-44: the other parties whose succession the registry has been
    /// asked about, per session, so a direct message asks once on opening
    /// and not on every pass. The answer lives in the state.
    asked_succession: std::collections::HashSet<(At, PubKey)>,
    /// SIP-45: the platform's latest word on where this device may be
    /// woken, kept so a session started later is told too.
    wake: Option<Option<(String, u32)>>,
    /// Files being chosen to attach, for which conversation. One at a time:
    /// the dialog is modal on a desktop and an activity on a phone, and
    /// neither runs two.
    picking: Option<(At, files::Pick)>,
    /// Where to save an attachment, and which one: conversation, entry,
    /// index.
    saving: Option<(At, u64, usize, files::Pick)>,
    /// What this pass's background work wants of the shell, handed over
    /// through `App::asked`.
    asked: Vec<AppAction>,
    /// What is not to be said out loud, as of the last pass: a copy of the
    /// roster's, so the count on the tab -- asked for without the roster in
    /// hand -- can leave the muted out.
    quiet: sigil::Quiet,
    /// What the shell last said about anybody being here, so the sessions
    /// are told only when it changes.
    away: bool,
    /// The emoji this person sends most, for the picker's own row.
    frequent: frequent::Frequent,
}

/// A call this client is actually carrying audio for.
struct Live {
    /// Whether anybody else has been in this call yet. A room that had
    /// somebody and now has nobody is a call the far end has left; one
    /// that never had anybody is a call still being answered.
    saw_peer: bool,
    channel: [u8; 32],
    /// The invitation's `seq`. Everything about a call is keyed on it.
    seq: u64,
    handle: sigil_net::CallHandle,
    /// When we joined, for the duration written into the closing entry.
    since: std::time::Instant,
    /// SIP-39: a call another exchange carried here. It has no
    /// conversation, so there is no entry to record its end in --
    /// `channel` and `seq` are zero and `leave_call` writes nothing.
    cross: bool,
}

impl Default for ChatApp {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatApp {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            call_opts: sigil_net::CallOpts::default(),
            started: HashMap::new(),
            failures: HashMap::new(),
            died: HashMap::new(),
            identity_paths: HashMap::new(),
            starts: 0,
            told_calling: None,
            closing: Vec::new(),
            panes: HashMap::new(),
            switching: false,
            // Open, with the newest conversation in it: what somebody signs
            // in to is their chats, not an empty pane.
            columns_open: true,
            single: false,
            config: Config::load(),
            store_root: None,
            now: None,
            fixed: None,
            sent: Vec::new(),
            drawn: std::collections::HashSet::new(),
            calls: HashMap::new(),
            left: HashSet::new(),
            answering: None,
            announcer: announce::Announcer::new(None),
            asked_succession: std::collections::HashSet::new(),
            wake: None,
            picking: None,
            saving: None,
            away: false,
            asked: Vec::new(),
            quiet: sigil::Quiet::default(),
            frequent: frequent::Frequent::load(),
        }
    }

    /// Keep the stores somewhere other than `~/.sqex/chat`. Tests only.
    ///
    /// The emoji counts go with them: a test that reacts must not add to
    /// the counts on the machine it runs on.
    #[doc(hidden)]
    /// **A notifier for when no frame is coming.** A phone stops drawing the
    /// moment it is not in front, and everything that says a call arrived
    /// ran from a frame -- so a ring that reached a phone in the background
    /// was said to nobody. With this, a session's wake waits for the frame
    /// it asked for and, if none comes, says it through here. A desktop
    /// passes nothing: it draws a frame whenever it is asked.
    pub fn with_off_frame_notify(
        mut self,
        notify: std::sync::Arc<dyn Notify + Send + Sync>,
    ) -> Self {
        self.announcer = announce::Announcer::new(Some(notify));
        self
    }

    pub fn set_store_root_for_test(&mut self, root: std::path::PathBuf) {
        self.frequent = frequent::Frequent::at(root.join("emoji.txt"));
        self.store_root = Some(root);
    }

    /// Pin the clock. A day separator says "Today", which is different
    /// tomorrow, so a snapshot taken against the real clock passes until it
    /// does not and then looks like a regression in whatever changed last.
    #[doc(hidden)]
    pub fn set_now_for_test(&mut self, now: u64) {
        self.now = Some(now);
    }

    /// Now, in seconds. Only used for how a time is *written* — never for
    /// deciding what is true, which the exchange's own stamps settle.
    fn now(&self) -> u64 {
        self.now.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        })
    }

    /// Point at an exchange without reading `~/.sqnr/config`.
    #[doc(hidden)]
    pub fn set_exchange_for_test(&mut self, host: &str, key: &str) {
        self.config.server = Some(host.to_string());
        self.config.server_key = Some(key.to_string());
    }

    #[doc(hidden)]
    pub fn running_for_test(&self) -> bool {
        !self.sessions.is_empty()
    }

    /// Which identities have a live session. The negative control for a
    /// switch: after changing identity this must no longer contain the old key.
    #[doc(hidden)]
    pub fn running_as_for_test(&self) -> Vec<PubKey> {
        let mut keys: Vec<PubKey> = self.sessions.keys().map(|(me, _)| *me).collect();
        keys.sort_by_key(|k| k.to_string());
        keys.dedup();
        keys
    }

    /// The state of `me`'s default session, dead or alive -- for a test to
    /// read what a session said before it ended (parked, SIP-59).
    #[doc(hidden)]
    pub fn state_of_for_test(&self, me: &PubKey) -> Option<ChatState> {
        self.sessions.get(&(*me, String::new())).map(|s| s.state())
    }

    /// Whether every session this app holds has ended.
    #[doc(hidden)]
    pub fn stopped_for_test(&self) -> bool {
        !self.sessions.is_empty() && self.sessions.values().all(|s| s.stopped())
    }

    /// How many times a session has been started for each identity.
    ///
    /// The negative control for the retry: a session that dies and is never
    /// started again counts one, for ever.
    #[doc(hidden)]
    pub fn starts_for_test(&self) -> usize {
        self.starts
    }

    /// Which identity-and-exchange pairs have a live session.
    ///
    /// The pair, not the key: one identity at two exchanges is two sessions,
    /// and a test that counted keys would not see the difference.
    #[doc(hidden)]
    pub fn running_at_for_test(&self) -> Vec<(PubKey, String)> {
        let mut all: Vec<(PubKey, String)> = self.sessions.keys().cloned().collect();
        all.sort_by_key(|(me, at)| (me.to_string(), at.clone()));
        all
    }

    /// Put a call in this app's hands, as answering one does.
    ///
    /// The handle is a real one: a call to an exchange that will never answer
    /// ends by itself, which is precisely the state that used to be left
    /// holding the microphone.
    #[doc(hidden)]
    pub fn hold_call_for_test(
        &mut self,
        me: PubKey,
        channel: [u8; 32],
        seq: u64,
        handle: sigil_net::CallHandle,
    ) {
        self.calls.insert(
            me,
            Live {
                channel,
                seq,
                handle,
                since: std::time::Instant::now(),
                saw_peer: false,
                cross: false,
            },
        );
    }

    /// Which identities this window is carrying audio for.
    #[doc(hidden)]
    pub fn calls_for_test(&self) -> Vec<PubKey> {
        let mut all: Vec<PubKey> = self.calls.keys().copied().collect();
        all.sort_by_key(|k| k.to_string());
        all
    }

    /// The account being shown, if it is open.
    fn showing(ctx: &AppContext<'_>) -> Option<PubKey> {
        ctx.account().unlocked().map(|u| u.me())
    }

    /// The identity **and exchange** being shown.
    ///
    /// Both, because a conversation list belongs to a pair: the identity is
    /// the same key at every exchange and its conversations are not.
    /// What to call one of this identity's exchanges.
    ///
    /// A named one is its name. The default has no name in the roster, so it
    /// is labelled with what it turned out to be: the domain it was discovered
    /// at, and only failing that the key. A truncated key is unreadable and
    /// says nothing about *where* it is, which is the whole question a
    /// switcher answers. Read from the default's **own** session rather than
    /// from whichever one is on screen: the row is about the default whether
    /// or not the default is what is being shown.
    fn exchange_label(&self, me: PubKey, name: &str) -> String {
        if name.is_empty() {
            default_label(self.sessions.get(&(me, String::new())).map(|s| s.state()))
        } else {
            // SIP-85: carried through the home, said where the name is.
            match self
                .sessions
                .get(&(me, name.to_string()))
                .and_then(|s| s.state().carried)
            {
                Some(home) => format!("{name} via {home}"),
                None => name.to_string(),
            }
        }
    }

    /// SIP-85: the domain of the active identity's default exchange -- the
    /// home a new exchange can be reached through -- or `None` when the
    /// default is an address, which a home cannot be asked to find by.
    fn home_domain(&self, ctx: &AppContext<'_>) -> Option<String> {
        let path = ctx.accounts.active().path().to_path_buf();
        let layers = discovery::layers(discovery::nothing_explicit(), &self.config, Some(&path));
        sigil_net::domain_of(&layers)
    }

    /// Whether the active identity's default exchange is anywhere at all.
    ///
    /// The default is derived -- the handle sidecar, `~/.sqnr/config` -- and
    /// an identity with neither has one that resolves to nothing. No session
    /// is started for it, and **nothing is shown for it either**: not a row
    /// in the switcher, not "default" in the title strip. An entry that names
    /// nowhere, beside the exchanges that do, was a choice that could only
    /// disappoint.
    fn default_resolves(&self, ctx: &AppContext<'_>) -> bool {
        let path = ctx.accounts.active().path().to_path_buf();
        let layers = discovery::layers(discovery::nothing_explicit(), &self.config, Some(&path));
        discovery::any_configured(&layers)
    }

    /// The exchanges the active identity holds that are worth listing: the
    /// named ones, and the default only when it resolves somewhere.
    fn listable_exchanges(&self, ctx: &AppContext<'_>) -> Vec<String> {
        let resolves = self.default_resolves(ctx);
        ctx.accounts
            .active_held()
            .exchanges()
            .into_iter()
            .filter(|name| !name.is_empty() || resolves)
            .collect()
    }

    fn showing_at(&self, ctx: &AppContext<'_>) -> Option<At> {
        let me = Self::showing(ctx)?;
        let live: Vec<At> = self.sessions.keys().cloned().collect();
        Some((
            me,
            showing_exchange(me, ctx.accounts.shown_exchange(me), &live),
        ))
    }

    fn state_of(&self, at: Option<&At>) -> ChatState {
        if let Some(fixed) = &self.fixed {
            return fixed.clone();
        }
        at.and_then(|at| self.sessions.get(at))
            .map(|s| s.state())
            .unwrap_or_default()
    }

    /// Draw this state instead of a session's.
    ///
    /// A snapshot of a transcript needs messages in it, and arranging real ones
    /// means two identities, an exchange and a conversation — which is an
    /// integration test, and a slow one, for a question about layout.
    ///
    /// **It replaces the data and nothing else.** `render` is the same code on
    /// the same path either way, so this cannot hide a bug in how a message is
    /// drawn — only in how one is fetched, which is what `chat_session.rs`
    /// covers against a real `sqexd`.
    #[doc(hidden)]
    /// Every command sent while a fixed state was shown, as `Debug` strings:
    /// the fixed state has no session to send to, so this is how a test
    /// learns what a press asked for.
    pub fn sent_for_test(&self) -> &[String] {
        &self.sent
    }

    pub fn show_state_for_test(&mut self, state: ChatState) {
        // A fixed state is a test, and a test's reactions are not this
        // machine's habits: the counts stay in memory unless a root was set.
        if self.store_root.is_none() {
            self.frequent = frequent::Frequent::in_memory();
        }
        self.fixed = Some(state);
    }

    /// Open a dialog, as the control that opens it does.
    ///
    /// Sets exactly what the button sets -- `dialogs_ui` draws from
    /// `pane.dialog` and nothing else -- so everything below this line is the
    /// production path. It exists because two of the six are behind menus a
    /// test would have to walk to reach, and what is being checked is the
    /// dialog, not the walk.
    ///
    /// `who` is only read by `verify`.
    #[doc(hidden)]
    pub fn open_dialog_for_test(&mut self, at: (PubKey, String), which: &str, who: PubKey) {
        let dialog = match which {
            "compose" => Dialog::Compose,
            "profile" => Dialog::Profile,
            "exchange" => Dialog::Exchange,
            "name" => Dialog::Name,
            "verify" => Dialog::Verify(who),
            "report" => Dialog::Report { target: 3 },
            other => panic!("no dialog called {other:?}"),
        };
        self.panes.entry(at).or_default().dialog = Some(dialog);
    }

    /// The emoji this person sends most. Tests read it; the picker draws it.
    pub fn frequent_for_test(&self) -> Vec<String> {
        self.frequent.top(sigil_ui::emoji::FREQUENT)
    }

    /// What the background work asked the shell for this pass; see
    /// `App::asked`.
    #[doc(hidden)]
    pub fn asked_of_shell_for_test(&mut self) -> Vec<AppAction> {
        std::mem::take(&mut self.asked)
    }

    /// Everything the interface has asked the session for, as `Debug` writes
    /// it. Recorded only while a fixed state is installed; see `send_as`.
    /// The notification a ring from another exchange would be pressed
    /// under, if one is ringing on any session: what the platform hands
    /// back to [`App::open`] when its Answer is pressed.
    /// How calls are opened -- a tone and a null sink where the machine has
    /// no microphone or speaker to open.
    pub fn set_call_opts_for_test(&mut self, opts: sigil_net::CallOpts) {
        self.call_opts = opts;
    }

    pub fn cross_ring_target_for_test(&self) -> Option<Target> {
        self.sessions.iter().find_map(|(at, s)| {
            let cross = s.cross_ring()?;
            let mut to = target(at, cross_key(cross.bridge));
            to.answer = true;
            Some(to)
        })
    }

    pub fn asked_for_test(&self) -> &[String] {
        &self.sent
    }

    fn pane(&mut self, at: &At) -> &mut Pane {
        self.panes.entry(at.clone()).or_default()
    }

    /// Start playing a voice note whose bytes are in hand.
    ///
    /// **Decoded whole, here.** A note is seconds of Opus; there is
    /// nothing to keep in step with a picture and nothing to stream. A
    /// file that will not decode is remembered as such, so the row says so
    /// once rather than trying again on every pass.
    fn start_note(&mut self, at: &At, ctx: &egui::Context, id: &str, bytes: &[u8]) {
        let pane = self.pane(at);
        pane.play_when_fetched.remove(id);
        if let Some(note) = pane.notes.get(id) {
            note.play();
            return;
        }
        match sigil_video::note::Note::open(bytes, ctx.clone()) {
            Ok(note) => {
                note.play();
                pane.notes.insert(id.to_owned(), note);
            }
            Err(why) => {
                tracing::info!(id, %why, "a voice note will not decode");
                pane.unplayable.insert(id.to_owned(), why.to_string());
            }
        }
    }

    /// Start playing a video whose bytes are in hand.
    fn start_video(&mut self, at: &At, ctx: &egui::Context, id: &str, bytes: std::sync::Arc<[u8]>) {
        let pane = self.pane(at);
        pane.play_when_fetched.remove(id);
        if pane.players.contains_key(id) {
            return;
        }
        match sigil_video::Player::open(bytes, ctx.clone()) {
            Ok(player) => {
                player.play();
                pane.players.insert(
                    id.to_owned(),
                    Playing {
                        player,
                        texture: None,
                        shown: None,
                        trace: VideoTrace::new(),
                    },
                );
            }
            Err(why) => {
                pane.unplayable.insert(id.to_owned(), why.to_string());
            }
        }
    }

    /// Bring the live sessions into line with the roster.
    ///
    /// Runs every pass from `update`, not only when the generation moves, so a
    /// session that could not start yet — an identity unlocked before its
    /// exchange was known — gets another go. Starting is guarded by the map, so
    /// repeating it costs a lookup.
    ///
    /// Started from `update` rather than `render` so messages arrive whether or
    /// not this app is the one on screen, and while the window is hidden.
    fn reconcile(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        // A closed session keeps the store lock until its task really ends.
        self.closing.retain(|(_, c)| !c.is_finished());

        // Every identity at every exchange it is connected to. The pair is
        // what a session belongs to: the identity is the same key everywhere,
        // and its conversations are not.
        let mut held: Vec<(At, std::path::PathBuf, Option<String>)> = Vec::new();
        for one in ctx.accounts.all() {
            let Some(unlocked) = one.account().unlocked() else {
                continue;
            };
            for exchange in one.exchanges() {
                let via = one.via_of(&exchange).map(str::to_string);
                held.push((
                    (unlocked.me(), exchange),
                    unlocked.path().to_path_buf(),
                    via,
                ));
            }
        }

        // Stop anything no longer held. This is the half that matters: a
        // session left running for a discarded identity keeps connecting,
        // keeps succeeding, and is the wrong person.
        let live: Vec<At> = self.sessions.keys().cloned().collect();
        for at in live {
            if !held.iter().any(|(k, _, _)| *k == at) {
                if let Some(session) = self.sessions.remove(&at) {
                    // Nothing else may go on borrowing what this session was
                    // holding: the identity is gone, and so is the connection
                    // as soon as the task lets go of it.
                    ctx.connections.forget(at.0, &at.1);
                    self.closing.push((at.clone(), session.close()));
                }
                self.panes.remove(&at);
            }
        }

        // **A session that died is not a session.** It ends by failing, and
        // the handle stays in hand looking exactly like a working one, so the
        // loop below -- which starts a session for any identity that has none
        // -- skipped it for ever. Dropped here, and started again by that same
        // loop, no sooner than `RETRY` after the last attempt so a store that
        // is locked for good does not become a restart every frame.
        let now = std::time::Instant::now();
        let stopped: Vec<At> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.stopped())
            .map(|(at, _)| at.clone())
            .collect();
        let mut dead = Vec::new();
        for at in stopped {
            let died = *self.died.entry(at.clone()).or_insert(now);
            let parked = self
                .sessions
                .get(&at)
                .is_some_and(|s| s.state().moved_to.is_some());
            // Parked (SIP-59: the account lives elsewhere): a long clock.
            // Died: a clock that doubles with each death in a row, from the
            // moment it was seen dead.
            let wait = if parked {
                PARKED_RETRY
            } else {
                let n = self.failures.get(&at).copied().unwrap_or(0);
                (RETRY * 2u32.saturating_pow(n.min(16))).min(RETRY_MAX)
            };
            if died.elapsed() >= wait {
                // A session that stayed up before dying starts over at
                // `RETRY`; one that died at once has failed again.
                let stayed = self
                    .started
                    .get(&at)
                    .is_some_and(|since| died.duration_since(*since) >= STAYED_UP);
                if stayed || parked {
                    self.failures.remove(&at);
                } else {
                    *self.failures.entry(at.clone()).or_insert(0) += 1;
                }
                self.died.remove(&at);
                dead.push(at);
            }
        }
        for at in dead {
            self.sessions.remove(&at);
            // Nothing may go on borrowing what a dead session was holding.
            ctx.connections.forget(at.0, &at.1);
        }

        for (at, path, via) in held {
            // Where the identity file is, for what the session records
            // beside it (the home, SIP-60).
            self.identity_paths.insert(at.0, path.clone());
            if self.sessions.contains_key(&at) {
                continue;
            }
            // Its predecessor has not let go of the store yet.
            if self.closing.iter().any(|(k, _)| *k == at) {
                continue;
            }
            let me = at.0;
            let named = at.1.clone();
            let Some(unlocked) = ctx
                .accounts
                .unlocked()
                .find(|(k, _)| *k == me)
                .map(|(_, u)| u)
            else {
                continue;
            };
            // An added exchange is named explicitly; the default one is
            // whatever the identity's own SIP-38 handle and `~/.sqnr/config`
            // resolve to, which is the answer for almost everybody.
            let layers = if named.is_empty() {
                discovery::layers(discovery::nothing_explicit(), &self.config, Some(&path))
            } else {
                vec![sigil_net::Layer {
                    server: Some(named.clone()),
                    ..Default::default()
                }]
            };
            if !discovery::any_configured(&layers) {
                continue;
            }
            // SIP-85: reached through the home -- the default exchange, by
            // its domain. The roster remembers the home's domain, and the
            // session is given the default's own layers to reach it by, so a
            // changed default does not silently carry through a stranger:
            // the two must still agree.
            let dial = match via {
                Some(home) => {
                    let home_layers =
                        discovery::layers(discovery::nothing_explicit(), &self.config, Some(&path));
                    if sigil_net::domain_of(&home_layers).as_deref() != Some(home.as_str()) {
                        // The default is no longer the home this exchange was
                        // added through. Said once, as a session that never
                        // comes up; the person removes and re-adds.
                        tracing::warn!(exchange = %named, home = %home,
                            "the default exchange is no longer this home; not connecting");
                        continue;
                    }
                    sigil_net::Dial::Via {
                        home: Box::new(sigil_net::Dial::Discover(home_layers)),
                        target: Box::new(sigil_net::Dial::Discover(layers)),
                        target_domain: named.clone(),
                    }
                }
                None => sigil_net::Dial::Discover(layers),
            };
            // **One store file per identity, shared by its exchanges.** The
            // store scopes every row by exchange and the lock is per (account,
            // exchange), so two sessions on one file do not collide -- and a
            // file each would put one identity's contact list in two places.
            let store_at = self
                .store_root
                .as_ref()
                .map(|root| root.join(format!("{me}.db")));
            let wake = egui_ctx.clone();
            let announcer = self.announcer.clone();
            let session = session::start(dial, unlocked.signer(), store_at, move || {
                // The frame first; then, if it never comes, the announcer
                // says what changed itself. See `announce`.
                wake.request_repaint();
                announcer.woken();
            });
            self.announcer.watch(at.clone(), session.watch());
            if let Some(wake) = &self.wake {
                session.send(Cmd::WakeEndpoint(wake.clone()));
            }
            // **The connection this session is about to hold, offered to the
            // rest of the window.** A call and the administrative console
            // borrow it rather than dialling their own -- see
            // `sigil_net::Connections`. Offered before it exists: what is lent
            // is the slot, which this session fills when the link comes up and
            // rewrites when it redials.
            ctx.connections.lend(me, &named, session.connection());
            self.started.insert(at.clone(), std::time::Instant::now());
            self.starts += 1;
            // A session starts believing somebody is here; one started
            // while nobody is is told so, or it would beat active until
            // somebody came back.
            let away = self.away;
            self.sessions.insert(at.clone(), session);
            if away {
                self.send_as(Some(&at), Cmd::Away(true));
            }
        }
    }

    fn send_as(&mut self, at: Option<&At>, cmd: Cmd) {
        // **Written down when nothing is listening.** A command with no
        // session behind it is dropped, which is right -- and it leaves a test
        // harness, which never has one, unable to see what a control asked
        // for. "Asked once" is exactly the kind of thing that has to be
        // counted: a control that asks by *being on screen* asks sixty times a
        // second, and looks identical from the outside.
        //
        // Only while a fixed state is installed, which is a test and nothing
        // else; the real app would grow this for ever.
        if self.fixed.is_some() {
            self.sent.push(format!("{cmd:?}"));
        }
        if let Some(s) = at.and_then(|at| self.sessions.get(at)) {
            s.send(cmd);
        }
    }
}

impl ChatApp {
    /// The route a token names, or the list when it names none of ours.
    ///
    /// A token from another app -- or the `()` of a plain tab switch -- is not
    /// an error and must never panic: the shell cannot tell them apart and
    /// hands over whatever it is holding.
    fn route(token: &std::rc::Rc<dyn std::any::Any>) -> Route {
        token
            .downcast_ref::<Route>()
            .cloned()
            .unwrap_or(Route::Conversations)
    }
}

/// Whether the shell's app bar is already carrying this view's name and its
/// way back.
///
/// On a phone it is: a view with a `nav_title` owns the bar, Back included,
/// exactly as an open conversation does. Drawing the same two things again
/// in the pane cost a finger's height on the four panes with the least room
/// to spare. On a desktop the pane is one of two columns with no bar over
/// it, so it keeps its own.
fn bar_has_the_head(ui: &egui::Ui) -> bool {
    sigil::Form::of(ui.ctx()).is_phone()
}

impl App for ChatApp {
    fn render_nav(
        &mut self,
        ctx: &mut AppContext<'_>,
        ui: &mut egui::Ui,
        token: &std::rc::Rc<dyn std::any::Any>,
    ) -> AppResponse {
        let route = Self::route(token);
        // **A ring is drawn wherever the eye is.** It was drawn at the top of
        // the transcript and nowhere else, so a call arriving while the list,
        // Devices or Members was on screen -- which on a phone is most of the
        // time -- showed nothing but the notification, and the words "drawn
        // over whatever is on screen" beside the cross ring were not true.
        // Seen on the device: the card appeared only because a broken Answer
        // had opened a conversation to draw it in. Incoming only: a call
        // *being placed* belongs to the conversation it is placed from.
        if let Some(at) = self.showing_at(ctx) {
            let state = self.state_of(Some(&at));
            let incoming =
                state.cross_ring.is_some() || state.ringing.iter().any(|r| !r.mine && !r.answered);
            if incoming && (route != Route::Conversations || state.open.is_none()) {
                let theme = ColorTheme::current(ui.ctx());
                // **From the bottom on a phone.** A card at the top of a
                // phone is at the far end of the hand holding it: Answer
                // and Decline were a stretch away, over the thing the
                // reader was looking at. At the bottom they are under the
                // thumb, which is where a phone puts the two buttons of a
                // call. A wide pane keeps it at the top, where a window's
                // banners belong.
                if sigil::Form::of(ui.ctx()).is_phone() {
                    egui::Panel::bottom("phone_ring")
                        .show_separator_line(false)
                        .frame(egui::Frame::NONE.fill(theme.surface_primary).inner_margin(
                            egui::Margin {
                                top: tokens::SPACING_SM as i8,
                                ..Default::default()
                            },
                        ))
                        .show(ui, |ui| {
                            self.ringing_ui(ctx, &at, &state, ui, &theme);
                        });
                } else if self.ringing_ui(ctx, &at, &state, ui, &theme) {
                    ui.add_space(tokens::SPACING_SM);
                }
            }
        }
        let response = match route {
            Route::Conversations => self.render(ctx, ui),
            Route::Directory => self.directory_view(ctx, ui),
            Route::Members => self.members_view(ctx, ui),
            Route::Settings => self.settings_view(ctx, ui),
            Route::Devices => self.devices_view(ctx, ui),
            Route::Search => self.search_view(ctx, ui),
            Route::Me => self.me_view(ctx, ui),
        };
        // A dialog opened from a view -- verifying somebody from Members --
        // is drawn over that view. `render` draws its own.
        if route != Route::Conversations
            && let Some(at) = self.showing_at(ctx)
        {
            let theme = ColorTheme::current(ui.ctx());
            let state = self.state_of(Some(&at));
            self.dialogs_ui(ctx, &at, &state, ui, &theme);
        }
        response
    }

    fn nav_title(&self, token: &std::rc::Rc<dyn std::any::Any>) -> Option<String> {
        Some(
            match Self::route(token) {
                Route::Conversations => return None,
                Route::Directory => "Public channels",
                Route::Members => "Members",
                Route::Settings => "Channel settings",
                Route::Devices => "Devices",
                Route::Search => "Search",
                Route::Me => "Settings",
            }
            .to_string(),
        )
    }

    fn asked(&mut self) -> Vec<AppAction> {
        std::mem::take(&mut self.asked)
    }

    /// A notification pressed: the identity it came to is shown, at its
    /// exchange, with the conversation open.
    fn open(&mut self, ctx: &mut AppContext<'_>, target: &Target) -> bool {
        let at: At = (target.identity, target.exchange.clone());
        if !self.sessions.contains_key(&at) {
            // Said, because a press that goes nowhere looks like a broken
            // button and the phone's log was the only place to read why.
            tracing::info!(
                identity = %target.identity, exchange = %target.exchange,
                held = ?self.sessions.keys().map(|k| k.1.clone()).collect::<Vec<_>>(),
                "a notification was pressed for a session this window does not hold"
            );
            return false;
        }
        tracing::info!(
            identity = %target.identity, exchange = %target.exchange,
            channel = ?target.channel, answer = target.answer,
            "a notification was pressed"
        );
        let held = ctx
            .accounts
            .iter()
            .position(|a| a.unlocked().is_some_and(|u| u.me() == target.identity));
        if let Some(i) = held {
            ctx.accounts.switch_to(i);
        }
        ctx.accounts.show_exchange(
            target.identity,
            (!target.exchange.is_empty()).then(|| target.exchange.clone()),
        );
        // SIP-39: a ring from another exchange is notified under the key of
        // its bridge, which is not a conversation. Showing it opened a
        // conversation that did not exist -- "Loading this conversation…"
        // for ever -- and the answer then looked for a ring in a list the
        // cross ring is not in, so pressing Answer on the phone's
        // notification answered nothing and the caller gave up. Seen on
        // the device. The ring is drawn over whatever is on screen, so
        // there is nothing to show; the answer is deferred as any other.
        let is_cross = self
            .sessions
            .get(&at)
            .and_then(|s| s.cross_ring())
            .is_some_and(|c| cross_key(c.bridge) == target.channel);
        if !is_cross {
            self.send_as(Some(&at), Cmd::Show(target.channel));
        }
        if target.answer {
            self.answering = Some((at, target.channel, std::time::Instant::now()));
        }
        true
    }

    /// **A `sigil://contact/<key>` link, once somebody has said yes to it.**
    ///
    /// A conversation with that key on the identity that is showing: the same
    /// `OpenDm` the compose dialog sends, so it creates one where there is
    /// none and opens the one there is. The other two kinds of link are the
    /// Calls app's.
    ///
    /// No when there is no session to send it to -- nothing is open, or the
    /// identity is sealed -- and then the shell says so rather than leaving a
    /// yes that did nothing.
    fn follow(&mut self, ctx: &mut AppContext<'_>, link: &sigil::Link) -> bool {
        let sigil::Link::Contact(who) = link else {
            return false;
        };
        let Some(at) = self.showing_at(ctx) else {
            return false;
        };
        if !self.sessions.contains_key(&at) {
            return false;
        }
        self.send_as(Some(&at), Cmd::OpenDm(*who));
        true
    }

    fn update(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        self.reconcile(ctx, egui_ctx);
        self.take_choices(egui_ctx);
        if self.quiet != ctx.accounts.quiet {
            self.quiet = ctx.accounts.quiet.clone();
        }
        // Nobody here, or somebody again: every session says so to its
        // exchange at once, since the beat that would say it otherwise is
        // up to half a minute off.
        if self.away != ctx.away {
            self.away = ctx.away;
            let ats: Vec<At> = self.sessions.keys().cloned().collect();
            for at in ats {
                self.send_as(Some(&at), Cmd::Away(ctx.away));
            }
        }
        // SIP-45: the platform's word on where to wake this device, to
        // every session, and remembered for the ones not started yet.
        if let Some(offered) = sigil::wake::take() {
            let wake = offered.url.map(|url| (url, offered.ttl_secs));
            self.wake = Some(wake.clone());
            for session in self.sessions.values() {
                session.send(Cmd::WakeEndpoint(wake.clone()));
            }
        }
        // What the frame knows, then what has not been said. The same
        // announcer speaks from a session's wake when no frame is coming.
        self.announcer.frame(
            self.sessions
                .iter()
                .map(|(at, s)| (at.clone(), s.watch()))
                .collect(),
            &ctx.accounts.quiet,
        );
        self.announcer.run(ctx.notify, ctx.unfocused);
        self.asked.extend(self.announcer.take_wants());
        self.join_answered_calls(ctx, egui_ctx);
        self.answer_pressed(ctx, egui_ctx);
        self.end_calls_nobody_is_in();
        self.announce_calling(ctx);
        // **A window carrying audio is not idle.** Everything else here sleeps
        // until something happens, which is what makes a quiet sigil cost
        // nothing -- but a call that nobody is in produces no events at all,
        // and the pass that would notice it is the pass that never runs. This
        // also keeps the clock on the call bar ticking, which is the same
        // second-by-second need.
        if !self.calls.is_empty() {
            egui_ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    fn accounts_changed(&mut self, _ctx: &mut AppContext<'_>) {
        // Reconciliation happens in `update`, which runs immediately after
        // this and every pass besides. Nothing to do here that would not be
        // undone or repeated a moment later -- and a second reconcile path is
        // a second thing to keep correct.
    }

    fn render(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        if !ctx.account().is_unlocked() {
            ui.heading("Chat");
            ui.colored_label(
                theme.text_secondary,
                "Unlock your identity to start chatting.",
            );
            return AppResponse::default();
        }
        let Some(at) = self.showing_at(ctx) else {
            return AppResponse::default();
        };
        let at = &at;
        // **No session at all is not the same as a session with nothing in
        // it.** `state_of` falls back to a default `ChatState`, and every
        // control here then talks to a session that does not exist: `send_as`
        // drops the command, the list is empty because there is nothing to
        // list, and the light said *connected* because that was the default.
        // Nothing anybody typed did anything and nothing said why.
        if self.fixed.is_none() && !self.sessions.contains_key(at) {
            let none = ChatState::default();
            self.dialogs_ui(ctx, at, &none, ui, &theme);
            // **The bar stays.** Without it this screen had no identity block
            // and therefore no chevron — so there was no way to switch to an
            // identity that does work, and nothing on it reflected an exchange
            // being added either. The one instruction it gave pointed at a
            // corner that was empty.
            self.bar_ui(ctx, at, &none, ui, &theme, Bar::Identity);
            self.unconnected_ui(ctx, at, ui, &theme);
            // **Here too.** The identity menu is on this screen as well, and
            // it is the screen somebody is most likely to want to leave: an
            // identity with no exchange is exactly when you go and be another
            // one. Returning the default here made the item do nothing on the
            // one pane it matters most on.
            return self.answer();
        }
        let state = self.state_of(Some(at));
        self.forget_what_is_gone(&state, ui.ctx());

        // Before anything else, and **outside every branch below**. It hung
        // off the conversation list, which is not drawn at all when the column
        // is hidden or when a narrow window is showing a conversation -- so a
        // dialog opened and then collapsed behind was one nobody could get out
        // of, and it also has several early returns under it.
        self.dialogs_ui(ctx, at, &state, ui, &theme);
        self.picture_ui(at, &state, ui, &theme);
        // **Here, and not in the conversation view.** A call is a state of the
        // window: a microphone is open. Drawn from inside a conversation it
        // vanished whenever the reader went anywhere else -- the list, another
        // identity, a narrow window showing the other pane -- taking the only
        // control that ends a call with it.
        self.in_call_ui(at, ui, &theme);

        // Two panes when there is room, one when there is not -- decided at
        // **runtime** from the width actually available, never from the
        // platform. Narrowing a desktop window has to collapse the layout
        // live, and a phone-shaped window on a desktop is a real thing.
        //
        // `sigil::layout` is the shared rule for this, so the deck and the
        // conversation view cannot drift into two answers about what "narrow"
        // means.
        let layout = sigil::layout(ui.available_width(), 2);
        self.single = matches!(layout, sigil::Layout::Single);
        let phone = sigil::Form::of(ui.ctx()).is_phone();

        // Arriving at a conversation rather than at an empty pane. Decided
        // once per identity, and never in a one-pane window -- there, opening
        // something *is* hiding the list, which is not a thing to do to
        // somebody who has just signed in.
        if let Some(channel) = first_look(
            state.open,
            self.pane(at).looked,
            matches!(layout, sigil::Layout::Single),
            &state.conversations,
        ) {
            self.pane(at).looked = true;
            self.send_as(Some(at), Cmd::Show(channel));
        }

        match layout {
            sigil::Layout::Single => {
                // One pane: the list until something is open, then the
                // conversation with a way back. Not both squeezed together --
                // two unusable columns are worse than one usable one.
                match state.open {
                    None => {
                        // One pane and nothing open: the list is the whole
                        // window, and the identity sits in its heading row
                        // rather than in a bar with nothing else on it. On
                        // a phone the identity is in the app bar (`head_ui`),
                        // so the heading row is the list's alone.
                        self.list_ui(ctx, at, &state, ui, &theme, !phone);
                    }
                    Some(_) => {
                        // On a phone the app bar *is* the conversation's bar
                        // (`head_ui` and `chrome_ui`): Back, the name, and
                        // More. A second bar under it was two headers for
                        // one screen.
                        if !phone {
                            self.bar_ui(
                                ctx,
                                at,
                                &state,
                                ui,
                                &theme,
                                Bar::Conversation { back: true },
                            );
                        }
                        self.transcript_ui(ctx, at, &state, ui, &theme);
                    }
                }
            }
            sigil::Layout::Shared { column_width } | sigil::Layout::Scrolling { column_width } => {
                if self.columns_open {
                    egui::Panel::left("chat_list")
                        .resizable(false)
                        // Narrower than a third of a wide window. A list of
                        // names and one line of preview needs about this much,
                        // and everything past it is width taken from the
                        // conversation, which is what somebody is reading.
                        .exact_size(column_width.min(280.0))
                        .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                            right: tokens::SPACING_LG as i8,
                            ..Default::default()
                        }))
                        .show(ui, |ui| self.list_ui(ctx, at, &state, ui, &theme, false));
                }
                // The conversation gets a margin of its own. Without one the
                // messages start hard against the divider and the composer
                // runs off the right edge -- both of which this had.
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                        left: tokens::SPACING_LG as i8,
                        right: tokens::SPACING_XS as i8,
                        ..Default::default()
                    }))
                    .show(ui, |ui| {
                        self.bar_ui(
                            ctx,
                            at,
                            &state,
                            ui,
                            &theme,
                            Bar::Conversation { back: false },
                        );
                        self.transcript_ui(ctx, at, &state, ui, &theme);
                    });
            }
        }
        self.answer()
    }

    /// Unread across **every** identity, not the one on screen.
    ///
    /// The badge is what tells somebody to come back, and an account they are
    /// not currently looking at is exactly the one they would otherwise miss.
    /// Which exchange this identity is looking at, in the window's title
    /// strip, and the way to look at another or add one.
    ///
    /// It was a list in the identity menu, two clicks behind a chevron, with
    /// the "add" button beside the exchange's key. But it is not a fact about
    /// the identity so much as about **what is on screen**: switching it
    /// changes the whole conversation list, so it belongs where a reader can
    /// see it at all times, which is the strip the window's own buttons live
    /// in.
    /// A phone's app bar, at its left end. With a conversation open on one
    /// pane: Back and the conversation's name, and the bar is the
    /// conversation's. Otherwise the identity's mark, and the title follows.
    fn head_ui(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> bool {
        let theme = ColorTheme::current(ui.ctx());
        let Some(at) = self.showing_at(ctx) else {
            return false;
        };
        let state = self.state_of(Some(&at));
        let open = state
            .conversations
            .iter()
            .find(|c| Some(c.channel) == state.open)
            .cloned();
        match open {
            Some(c) if self.single => {
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                    self.send_as(Some(&at), Cmd::Close);
                }
                self.conversation_name_ui(ctx, &at, &state, &c, ui, &theme);
                true
            }
            _ => {
                self.me_ui(ctx, &at, &state, ui, &theme, true);
                false
            }
        }
    }

    /// One step back on a phone: a conversation closes for the list. Not
    /// while a picture or a dialog is over it -- those close on Escape,
    /// which is what the shell sends when this says no.
    fn back(&mut self, ctx: &mut AppContext<'_>) -> bool {
        let Some(at) = self.showing_at(ctx) else {
            return false;
        };
        let state = self.state_of(Some(&at));
        let pane = self.panes.entry(at.clone()).or_default();
        if pane.viewing.is_some() || pane.dialog.is_some() {
            return false;
        }
        if self.single && state.open.is_some() {
            self.send_as(Some(&at), Cmd::Close);
            return true;
        }
        // **Behind the list is the identity it belongs to.** Back at the
        // list with nothing open had nowhere left to go and did nothing,
        // which on a phone reads as a dead key -- and the screen this one
        // came from is the opening screen, where the identity was chosen.
        // The flag is taken by `answer`, so this asks the shell once.
        if self.single {
            self.switching = true;
            return true;
        }
        false
    }

    fn chrome_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        ui: &mut egui::Ui,
        token: &std::rc::Rc<dyn std::any::Any>,
    ) {
        let theme = ColorTheme::current(ui.ctx());
        let Some(at) = self.showing_at(ctx) else {
            return;
        };
        // **A named view's one action, on a phone.** The shell has taken
        // this view's name and its Back into the bar, so the corner is the
        // rest of that bar -- and it is the exchange control that has no
        // business here: nothing on these four panes is chosen by exchange,
        // and switching one from Devices would change what the list below
        // is about without saying so.
        if sigil::Form::of(ui.ctx()).is_phone() {
            match Self::route(token) {
                Route::Devices => {
                    if sigil_ui::icon_button(ui, sigil_ui::Icon::Refresh).clicked() {
                        self.send_as(Some(&at), Cmd::Devices);
                        self.send_as(Some(&at), Cmd::BackupStatus);
                    }
                    return;
                }
                Route::Directory | Route::Members | Route::Settings | Route::Search | Route::Me => {
                    return;
                }
                Route::Conversations => {}
            }
        }
        // On a phone with a conversation open the corner is the
        // conversation's controls, folded behind More; the exchange is
        // chosen from the list, which is where one is left by Back.
        if sigil::Form::of(ui.ctx()).is_phone() && self.single {
            let state = self.state_of(Some(&at));
            if let Some(c) = state
                .conversations
                .iter()
                .find(|c| Some(c.channel) == state.open)
                .cloned()
            {
                self.conversation_controls_ui(ctx, &at, &state, &c, ui, &theme, true);
                return;
            }
        }
        let me = at.0;
        let named = self.listable_exchanges(ctx);
        // A dead default on show -- nothing named yet, nowhere to go -- is
        // said as what it is, not as a "default" that sounds like a place.
        let shown = if at.1.is_empty() && !named.iter().any(String::is_empty) {
            "no exchange".to_string()
        } else {
            self.exchange_label(me, &at.1)
        };
        let rows: Vec<sigil_ui::ExchangeRow> = named
            .iter()
            .map(|name| sigil_ui::ExchangeRow {
                name: name.clone(),
                label: self.exchange_label(me, name),
                // **A way out, beside the way in.** There was a control to
                // add an exchange and none to remove one, so a name added by
                // mistake -- or one that turned out to be the default under
                // another spelling -- could only be taken back by editing
                // the roster file by hand. The default is not one of these.
                removable: !name.is_empty(),
            })
            .collect();
        let did = sigil_ui::exchange_control(ui, &theme, &shown, &at.1, &rows, true);
        if let Some(name) = did.chosen {
            ctx.accounts.show_exchange(me, Some(name));
        }
        if let Some(name) = did.removed {
            let which = ctx.accounts.active_index();
            ctx.accounts.drop_exchange(which, &name);
            // Back to the default, or the interface would be showing a
            // conversation list for an exchange it is no longer connected to.
            ctx.accounts.show_exchange(me, None);
        }
        if did.add {
            self.panes.entry(at.clone()).or_default().dialog = Some(Dialog::Exchange);
        }
    }

    fn tab_notifications(&self) -> TabNotifications {
        // Not the muted ones: a conversation muted is one whose count the
        // icon must not carry, or muting it would not be quiet.
        let quiet = &self.quiet;
        TabNotifications::count(
            self.sessions
                .iter()
                .map(|(at, s)| s.unread_but(|channel| quiet.is_muted(&at.1, channel)) as u32)
                .sum(),
        )
    }

    fn title(&self) -> &str {
        "Chat"
    }

    fn icon(&self) -> sigil::Icon {
        sigil::Icon::Compose
    }
}

impl ChatApp {
    /// Tell egui about pictures the session has put down.
    ///
    /// egui holds the encoded bytes of everything it has been given until it is
    /// told to forget them, so without this its cache grew for the life of the
    /// process even after the session had dropped its own copy — which made the
    /// session's own eviction pointless, since two of the three copies stayed.
    ///
    /// Named by what the widget names them, which is what `forget_image` wants.
    fn forget_what_is_gone(&mut self, state: &ChatState, ctx: &egui::Context) {
        let here: std::collections::HashSet<String> = state
            .lines
            .iter()
            .flat_map(|l| l.attachments.iter())
            .filter(|a| a.bytes.is_some())
            .map(|a| a.id.clone())
            .collect();
        for id in self.drawn.difference(&here) {
            ctx.forget_image(&format!("bytes://{id}"));
            ctx.forget_image(&format!("bytes://{id}-preview"));
        }
        self.drawn = here;
    }

    /// What the shell is being asked for, if anything.
    ///
    /// Taken rather than read: an ask is one event, and a flag left set would
    /// send the shell back to the opening screen on every pass afterwards --
    /// which looks exactly like an interface that cannot be dismissed.
    fn answer(&mut self) -> AppResponse {
        if std::mem::take(&mut self.switching) {
            AppResponse::action(sigil::app::AppAction::ChooseIdentity)
        } else {
            AppResponse::default()
        }
    }

    /// This identity is not talking to any exchange, and why.
    ///
    /// The reason is always the same one: nothing names an exchange for it.
    /// An identity gets its default from its own SIP-38 handle sidecar or
    /// `~/.sqnr/config`, and one with neither has nowhere to connect — so it
    /// sits in the roster looking like every other account and can do nothing
    /// at all. The way out is to name an exchange, which is the same control
    /// as everywhere else.
    fn unconnected_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let me = at.0;
        // **Which of the two it is.** An identity with no exchange at all and
        // one whose *shown* exchange has no session are different problems
        // with different answers, and saying the first about the second told
        // somebody their identity named nothing while it was connected
        // perfectly well somewhere they were not looking.
        // The default counts only when it goes somewhere; otherwise an
        // identity with a dead default and nothing named is one with nothing.
        let held = self.listable_exchanges(ctx);
        let only = held.iter().all(String::is_empty);
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.25);
            sigil_ui::identicon(ui, &me.to_string(), tokens::AVATAR_LG);
            ui.add_space(tokens::SPACING_MD);
            ui.heading("Not connected");
            ui.colored_label(
                theme.text_secondary,
                if only {
                    "This identity names no exchange, so there is nothing for it to talk to."
                } else {
                    "Not connected to this exchange. The others this identity holds are in \
                     the block in the corner."
                },
            );
            ui.add_space(tokens::SPACING_SM);
            ui.add(
                egui::Label::new(egui::RichText::new(me.to_string()).monospace().small())
                    .wrap()
                    .selectable(true),
            );
            ui.add_space(tokens::SPACING_MD);
            if only {
                // The suggestion, first and biggest: the answer to the
                // question the sentence above raises.
                ui.colored_label(
                    theme.text_secondary,
                    format!("{SUGGESTED_EXCHANGE} is a public exchange anybody can join."),
                );
                ui.add_space(tokens::SPACING_SM);
                if ui.button(format!("Add {SUGGESTED_EXCHANGE}")).clicked()
                    && ctx.accounts.add_exchange(
                        ctx.accounts.active_index(),
                        SUGGESTED_EXCHANGE,
                        None,
                    )
                {
                    // Shown straight away, as the dialog does: adding one and
                    // staying on "not connected" looks like nothing happened.
                    ctx.accounts
                        .show_exchange(me, Some(SUGGESTED_EXCHANGE.to_string()));
                }
                ui.add_space(tokens::SPACING_SM);
            }
            if ui
                .button(if only {
                    "Add a different exchange"
                } else {
                    "Add an exchange"
                })
                .clicked()
            {
                self.panes.entry(at.clone()).or_default().dialog = Some(Dialog::Exchange);
            }
            // Somebody arriving here by switching identity wants the way back
            // more often than the way forward.
            if ctx.accounts.len() > 1 {
                ui.add_space(tokens::SPACING_SM);
                ui.colored_label(
                    theme.text_muted,
                    egui::RichText::new(
                        "Or switch to another identity, from the block in the corner.",
                    )
                    .small(),
                );
            }
            let _ = ctx;
        });
    }

    /// The one row over the conversation: what it is and everything that
    /// can be done about it, and at its right end who you are and whether
    /// the link is up.
    ///
    /// **One row, where there were two.** The session -- the dot, the
    /// avatar, the name over the handle, the chevron -- had a bar of its
    /// own above the conversation's, and a narrow window put Back on a
    /// third. Seventy pixels of "here is where you are" above every
    /// transcript. The identity is now the avatar with the dot on its
    /// corner and the chevron; the name and handle are the head of the
    /// chevron's menu, which already held the key and the exchange.
    ///
    /// **Over the conversation and not over the whole window.** It used to
    /// span both, which cost the conversation column its top and made the
    /// list start under a bar that has nothing to do with it.
    fn bar_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        bar: Bar,
    ) {
        let me = at.0;
        let open = match bar {
            Bar::Conversation { .. } => state
                .conversations
                .iter()
                .find(|c| Some(c.channel) == state.open),
            Bar::Identity => None,
        };
        // **The controls are laid out first, from the right.** Given the
        // name first, a long one takes the row and the controls wrap onto a
        // second line -- which is what this did, and it moved the header
        // about as a member count appeared. Whatever is left is the name's,
        // and it truncates rather than pushing anything off the row.
        //
        // That holds only while the controls themselves fit. In a narrow
        // pane -- a phone, or a desktop window pulled in -- six of them
        // beside the identity are wider than the row, and a right-to-left
        // row that overflows pushes Back and the name off the left edge
        // and drags the transcript after them. So a narrow pane folds most
        // of them behind one More button.
        let narrow = ui.available_width() < tokens::NARROW_WIDTH;
        ui.horizontal(|ui| {
            ui.set_min_height(tokens::AVATAR_MD);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.me_ui(ctx, at, state, ui, theme, narrow);
                if let Some(c) = open {
                    // A rule between what is yours and what is the
                    // conversation's.
                    let (gap, _) = ui.allocate_exact_size(
                        egui::vec2(tokens::SPACING_MD, tokens::AVATAR_MD),
                        egui::Sense::hover(),
                    );
                    ui.painter().vline(
                        gap.center().x,
                        gap.y_range(),
                        egui::Stroke::new(tokens::STROKE_THIN, theme.border_default),
                    );
                    self.conversation_controls_ui(ctx, at, state, c, ui, theme, narrow);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // The one way out at the left end, and never two: back
                    // to the list where the list is not on screen, or the
                    // column back where it was put away. Only when there is
                    // something to bring back does either appear, so the
                    // bar is not carrying a control that does nothing.
                    match bar {
                        Bar::Conversation { back: true } => {
                            if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                                self.send_as(Some(at), Cmd::Close);
                            }
                        }
                        _ if !self.columns_open
                            && sigil_ui::icon_button_named(
                                ui,
                                sigil_ui::Icon::Menu,
                                "Show the chats",
                            )
                            .clicked() =>
                        {
                            self.columns_open = true;
                        }
                        _ => {}
                    }
                    if let Some(c) = open {
                        self.conversation_name_ui(ctx, at, state, c, ui, theme);
                    }
                });
            });
        });
        // Under the row, each only while there is something to say, so the
        // row itself stays one clean line.
        //
        // Whatever was just done. It was above the transcript, where it
        // pushed every message down by a line for a moment and then let
        // them back up. A note is about an **action** and a trouble is about
        // a state; the state is rebuilt every refresh, so merging them would
        // put each confirmation on screen for less than a tick.
        if let Some(note) = &state.note {
            ui.add(
                egui::Label::new(egui::RichText::new(&note.said).small().color(theme.success))
                    .truncate(),
            );
        }
        // A lock this identity's *own* other session is holding reads, from
        // the store's point of view, exactly like a second program. It is not
        // one, and saying so sends somebody hunting for a client that is not
        // running.
        match self.duplicate_exchange(at, state) {
            Some(spare) => {
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(
                        theme.warning,
                        format!(
                            "Already connected to this exchange — \"{spare}\" resolves to \
                             the same place as this identity's default."
                        ),
                    );
                    let which = ctx.accounts.active_index();
                    if ui.button("Remove it").clicked() {
                        ctx.accounts.drop_exchange(which, &spare);
                        ctx.accounts.show_exchange(me, None);
                    }
                });
            }
            None => {
                if let Some(trouble) = &state.trouble {
                    ui.colored_label(theme.destructive, trouble);
                }
            }
        }
        ui.separator();
    }

    /// The conversation's controls, from the right: Settings, the bell,
    /// Devices, Members with their count, Call.
    #[allow(clippy::too_many_arguments)]
    fn conversation_controls_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        open: &session::Summary,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        narrow: bool,
    ) {
        let me = at.0;
        let dm = open.peer.is_some();
        // **No calling a public channel.** SIP-36 confines calls to
        // private channels: a public channel's invitation is in the clear,
        // and the room secret in it is a bearer capability, so posting one
        // there hands the call to anybody who can read the channel. Only
        // when the exchange has said it is private: `None` is "not yet
        // answered", and offering a call on a guess is offering the wrong
        // thing to the wrong room.
        let callable = open.public == Some(false);
        // Muted or not, in the header where the conversation is named:
        // the one control about *this* conversation that is not about
        // its members or its settings.
        let channel = open.channel;
        let muted = ctx.accounts.quiet.is_muted(&at.1, &channel);
        // How many, beside the way to see who. Not for a direct message:
        // two people is what one is, and "2" beside it says nothing.
        let members = state.members.len();
        let mut go = None;
        let may_call =
            callable && !self.calls.contains_key(&me) && !state.ringing.iter().any(|r| r.mine);
        let mut call = false;
        if narrow {
            // **The call is its own button, not a menu item.** It is the
            // control somebody reaches for in a hurry, and a hurry is the
            // worst time to open a menu and read it. It was folded in once
            // because a call button beside the name cost a 360-point phone
            // the name itself -- but that was a bar carrying five controls,
            // and this one carries two. `callable` keeps it to the places a
            // call belongs: a direct message or a private channel, never a
            // public one.
            let more = sigil_ui::icon_button_named(
                ui,
                sigil_ui::Icon::More,
                "More about this conversation",
            );
            // **After the menu, not before it.** This bar lays out from the
            // right, so drawing the call first would push the menu button
            // left -- and `a_hidden_strip_does_not_come_back_when_another_menu_opens`
            // then fails: with the menu button moved, a press on it leaves a
            // message's action strip on the transcript. I could not account
            // for that from the geometry the harness reports, so I have not
            // claimed to: what is established is that the bar's positions
            // are load-bearing for that behaviour, and this keeps every
            // existing control exactly where it was. Worth understanding
            // before anything else is added to this bar.
            if may_call && sigil_ui::icon_button_named(ui, sigil_ui::Icon::Call, "Call").clicked() {
                call = true;
            }
            egui::Popup::menu(&more).show(|ui| {
                let mut who = if members > 0 && !dm {
                    format!("Members ({members})")
                } else {
                    "Members".to_string()
                };
                // SIP-56: reports the exchange announced since an admin last
                // read them, said where the reports are.
                if state.i_am_admin && state.reports_pending > 0 {
                    who.push_str(&format!(" · {} new report(s)", state.reports_pending));
                }
                // The same rows as the wide bar's buttons, each with its
                // icon: the menu is that bar folded, and the identity menu
                // beside it draws its rows this way.
                if sigil_ui::icon_item(ui, sigil_ui::Icon::People, &who).clicked() {
                    go = Some(Route::Members);
                }
                let (bell, word) = if muted {
                    (sigil_ui::Icon::BellOff, "Unmute this conversation")
                } else {
                    (sigil_ui::Icon::Bell, "Mute this conversation")
                };
                if sigil_ui::icon_item(ui, bell, word).clicked() {
                    ctx.accounts.quiet.set_muted(&at.1, &channel, !muted);
                }
                if sigil_ui::icon_item(ui, sigil_ui::Icon::Device, "Devices").clicked() {
                    go = Some(Route::Devices);
                }
                if sigil_ui::icon_item(ui, sigil_ui::Icon::Settings, "Settings").clicked() {
                    go = Some(Route::Settings);
                }
            });
        } else {
            if sigil_ui::icon_button(ui, sigil_ui::Icon::Settings).clicked() {
                go = Some(Route::Settings);
            }
            let bell = if muted {
                sigil_ui::Icon::BellOff
            } else {
                sigil_ui::Icon::Bell
            };
            if sigil_ui::icon_button(ui, bell).clicked() {
                ctx.accounts.quiet.set_muted(&at.1, &channel, !muted);
            }
            if sigil_ui::icon_button(ui, sigil_ui::Icon::Device).clicked() {
                go = Some(Route::Devices);
            }
            if members > 0 && !dm {
                ui.colored_label(theme.text_muted, members.to_string());
            }
            if sigil_ui::icon_button(ui, sigil_ui::Icon::People).clicked() {
                go = Some(Route::Members);
            }
        }
        match go {
            Some(Route::Devices) => {
                self.send_as(Some(at), Cmd::Devices);
                self.send_as(Some(at), Cmd::BackupStatus);
                ctx.navigator.push_here(Route::Devices);
            }
            Some(Route::Members) => {
                self.send_as(Some(at), Cmd::Blocked);
                ctx.navigator.push_here(Route::Members);
            }
            Some(route) => ctx.navigator.push_here(route),
            None => {}
        }
        if !narrow && may_call && sigil_ui::icon_button(ui, sigil_ui::Icon::Call).clicked() {
            call = true;
        }
        if call {
            // Say in the invitation whether this side will ask for an
            // introduction, so the other side is not left waiting on one
            // that is never asked for.
            let direct = ctx.accounts.prefs.direct_calls;
            self.send_as(Some(at), Cmd::Call { direct });
        }
    }

    /// The conversation's name, and its topic beside it; both truncate.
    /// An admin's name is the way to rename it.
    fn conversation_name_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        _at: &At,
        state: &ChatState,
        open: &session::Summary,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let dm = open.peer.is_some();
        // Whether the other person is there, before their name: the one
        // fact about a direct message that changes while it is open.
        if let Some(peer) = open.peer {
            let (seen, hover) = self.presence_of(state, &peer);
            seen.dot(ui, &hover);
        }
        let may_rename = state.i_am_admin && !dm;
        let named = ui.scope_builder(
            egui::UiBuilder::new().sense(if may_rename {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            }),
            |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                let response = ui.response();
                let mut text = egui::RichText::new(&open.label).heading();
                if may_rename && response.hovered() {
                    text = text.color(theme.accent);
                }
                ui.add(egui::Label::new(text).truncate());
            },
        );
        if may_rename {
            let response = named.response;
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            // Only for the word: a public channel and a private group are
            // renamed by the same route.
            let response = response.on_hover_text(if open.public == Some(true) {
                "Rename this channel"
            } else {
                "Rename this group"
            });
            if response.clicked() {
                ctx.navigator.push_here(Route::Settings);
            }
        }
        if let Some(peer) = open.peer
            && state.verified.contains_key(&peer)
        {
            sigil_ui::verified_mark(ui);
        }
        if !state.topic.is_empty() {
            ui.add(
                egui::Label::new(egui::RichText::new(&state.topic).color(theme.text_secondary))
                    .truncate(),
            );
        }
        // SIP-43: a conversation that lives at another exchange says so, in
        // the bar, once and quietly. What is said here is carried there.
        if let Some((origin, domain)) = &state.home {
            let at = if domain.is_empty() {
                let key = origin.to_string();
                format!("lives at {}…", &key[..key.len().min(8)])
            } else {
                format!("lives at {domain}")
            };
            ui.add(
                egui::Label::new(egui::RichText::new(at).small().color(theme.text_secondary))
                    .truncate(),
            )
            .on_hover_text(format!(
                "This conversation is ordered by another exchange ({origin}). \
                 What you write here is carried there and comes back with its place."
            ));
        }
    }

    /// Whether somebody is there, as the interface draws it, with the words
    /// a pointer learns. Nothing known is offline with nothing to say.
    fn presence_of(&self, state: &ChatState, who: &PubKey) -> (sigil_ui::Presence, String) {
        let known = state.presence.get(who).copied().unwrap_or_default();
        let seen = match known.seen {
            presence::Seen::Active => sigil_ui::Presence::Active,
            presence::Seen::Away => sigil_ui::Presence::Away,
            presence::Seen::Offline => sigil_ui::Presence::Offline,
        };
        // Both on the exchange's clock: `last_seen` as recorded, `read_at`
        // as of the read -- which is within a tick of now, so "last seen
        // Thu" is said against the right day.
        let hover = sigil_ui::presence_hover(
            seen,
            known.last_seen,
            known.read_at.max(known.last_seen),
            &who.to_string(),
        );
        (seen, hover)
    }

    /// The added exchange name to drop, when this session lost the store lock
    /// to another of **this identity's own** sessions.
    ///
    /// `None` when the lock is genuinely somebody else's — another sigil, or
    /// the terminal client — which is a different problem with a different
    /// answer, and must keep its own words.
    ///
    /// Always the *named* one of the pair: the default is not a name in the
    /// roster, it is whatever the identity resolves to, and there would be
    /// nothing to remove.
    fn duplicate_exchange(&self, at: &At, state: &ChatState) -> Option<String> {
        // **Nothing at all unless this session is locked out.** It is the only
        // case the answer is about, and the walk below reads one field from
        // every other session -- which used to mean cloning every other
        // session's entire state, on every pass, to find out that nobody was
        // locked out of anything.
        state.locked_out?;
        let others: Vec<(At, Option<PubKey>)> = self
            .sessions
            .iter()
            .map(|(other, session)| (other.clone(), session.exchange()))
            .collect();
        duplicate_of(at, state.locked_out, &others)
    }

    /// Who you are, at the top right, with everything about you behind it.
    ///
    /// # Why it is a block and not a pane
    ///
    /// This was the head of the conversation column: an avatar, a name, a
    /// handle, a key in full, the exchange in full, and two controls. Six
    /// lines that never change, above a list that does — so a third of the
    /// column was spent on something nobody was reading, and the list started
    /// halfway down the window.
    ///
    /// Now it is one block on the status row, and the things it used to say
    /// are one click away in its menu. **The key is still reachable**, which
    /// is the part that is not negotiable: a name is an assertion attested by
    /// nobody (SIP-21), and the key is what actually identifies you to
    /// somebody who wants to write to you.
    fn me_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        compact: bool,
    ) {
        let away = ctx.away;
        let me = at.0;
        let key = me.to_string();

        // Laid out from the right, which is where this sits: the chevron
        // first, then the mark. On a narrow bar there is no chevron and
        // the mark is the button: beside a mark that opens the menu, a
        // chevron said the same thing again for a button's width, which
        // is the width the conversation's name was short of.
        let chevron = (!compact).then(|| {
            sigil_ui::icon_button_named(ui, sigil_ui::Icon::Chevron, "Your identity")
                .on_hover_text("Your key, your exchanges, and the other identities you hold")
        });
        // The mark with your presence on its corner: what everybody else
        // reads of you -- active, or away when nobody has touched this
        // machine for a while -- and, with the link down, the link's own
        // word. **The word appears when it is worth reading.** A link that
        // is up is the ordinary case and the dot says it. A link that is
        // not is the case where nothing arriving looks exactly like nobody
        // writing, and no colour can tell somebody that -- so that one
        // keeps its word, and its way back. Either way the word is on the
        // mark's hover and in the accessibility tree, where a colour
        // reaches nobody at all.
        let up = state.link == LinkState::Up;
        let (seen, word) = match (up, away) {
            (true, false) => (sigil_ui::Presence::Active, "active"),
            (true, true) => (sigil_ui::Presence::Away, "away"),
            (false, _) => (sigil_ui::Presence::Offline, state.link.word()),
        };
        let hover = if up {
            format!("{word} — what others see of you\n{key}")
        } else {
            format!("{word}\n{key}")
        };
        let mark = if compact {
            // A size down from the desktop's: it heads a phone's app bar,
            // beside a word, and the bar is a finger tall.
            let mark = ui
                .scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
                    sigil_ui::presence(ui, &key, None, tokens::ICON_LG, seen, word, &hover);
                })
                .response;
            mark.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Your identity")
            });
            Some(mark)
        } else {
            sigil_ui::presence(ui, &key, None, tokens::AVATAR_MD, seen, word, &hover);
            None
        };
        // Not on a phone: the dot on the mark says the link is down, the
        // session reconnects by itself, and a word about it in a bar a
        // finger tall is a word nobody asked for. The word is still on the
        // mark's hover and in the accessibility tree.
        if !up && !compact {
            let colour = match state.link {
                LinkState::Gone => theme.link_gone,
                _ => theme.link_retrying,
            };
            ui.add_space(tokens::SPACING_XS);
            if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Refresh, "Reconnect").clicked() {
                self.send_as(Some(at), Cmd::Reconnect);
            }
            ui.colored_label(colour, state.link.word());
        }

        let anchor = chevron.or(mark).expect("the chevron or the mark");
        // **On a phone the mark is a door, not a menu.** What was behind it
        // had grown a scroll bar, and a popup that scrolls is a card that
        // has not admitted it: no name in the bar, no Back, and nowhere to
        // put the way to the other things sigil does. See [`Route::Me`].
        if sigil::Form::of(ui.ctx()).is_phone() {
            if anchor.clicked() {
                ctx.navigator.push_here(Route::Me);
            }
            return;
        }
        egui::Popup::menu(&anchor).show(|ui| {
            let screen = ui.ctx().content_rect().width();
            let wide = 320.0f32.min(screen - 2.0 * tokens::SPACING_LG).max(200.0);
            ui.set_min_width(wide);
            // **And no wider.** A row laid out from the right -- the copy
            // beside a key, the ✕ beside a name -- is laid out from wherever
            // the ui ends, and a menu with a minimum and no maximum ends
            // wherever its widest row does: on the phone that was past the
            // screen, with both controls half off it. Seen on the device,
            // after the copy buttons were added.
            ui.set_max_width(wide);
            self.identity_menu(ctx, at, state, ui, theme);
        });
    }

    /// What used to be the top of the column, behind the chevron.
    fn identity_menu(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let me = at.0;

        // The head: your name, or -- with no name published -- the first
        // characters of your key, and under it the name this exchange knows
        // you by. **Controls, not captions.** On seeing the name the thing
        // somebody wants is to set it; on seeing "unregistered" it is to
        // claim one, and the way to one is a claim at this exchange. These
        // were the two lines beside the avatar on the bar; the bar is one
        // row now and they are the first thing behind its chevron.
        let name = ui
            .add(
                egui::Label::new(egui::RichText::new(state.mine.label(&me)).strong())
                    .truncate()
                    .sense(egui::Sense::click()),
            )
            .on_hover_text("Set your name and title");
        if name.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if name.clicked() {
            self.open_profile(at, state);
            ui.close();
        }
        match &state.mine.handle {
            Some(handle) => {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(handle)
                            .small()
                            .color(theme.text_secondary),
                    )
                    .truncate(),
                );
            }
            None => {
                if ui
                    .add(
                        egui::Label::new(
                            egui::RichText::new(UNREGISTERED)
                                .small()
                                .color(theme.text_muted),
                        )
                        .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Claim a name at this exchange")
                    .clicked()
                {
                    self.panes.entry(at.clone()).or_default().dialog = Some(Dialog::Name);
                    ui.close();
                }
            }
        }
        ui.separator();

        // In full, selectable, and wrapped rather than clipped. A name is an
        // assertion (SIP-21) and this is not -- it is the only thing that
        // identifies you to somebody who wants to write to you.
        //
        // **And copyable, because a finger cannot select text.** Selectable
        // is a pointer's answer: on the phone the one thing anybody wants
        // to do with their own key -- send it to somebody -- could not be
        // done from the one place that shows it.
        ui.horizontal(|ui| {
            ui.colored_label(theme.text_muted, egui::RichText::new("You").small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Copy, "Copy your key").clicked()
                {
                    ui.ctx().copy_text(me.to_string());
                }
            });
        });
        ui.add(
            egui::Label::new(egui::RichText::new(me.to_string()).monospace().small())
                .wrap()
                .selectable(true),
        );
        // The name this exchange knows you by, and the way to let go of it.
        // Claiming one has been offered since there was a claim route; giving
        // one up had no control at all, so a name taken by mistake was taken
        // for good.
        if let Some(handle) = &state.mine.handle {
            ui.separator();
            ui.colored_label(
                theme.text_muted,
                egui::RichText::new("Your name at this exchange").small(),
            );
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(egui::RichText::new(handle).monospace().small()).wrap());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Close, "Give it up")
                        .on_hover_text(
                            "Stop being reachable at this name. Nothing is deleted — your \
                             conversations, keys and counters are untouched — and somebody \
                             else may take it afterwards.",
                        )
                        .clicked()
                    {
                        // The bare name, without the domain: a claim and a
                        // release both name the local part, and the exchange
                        // it is released at is the one being talked to.
                        let local = handle.split('@').next().unwrap_or(handle).to_string();
                        self.send_as(Some(at), Cmd::ReleaseName(local));
                        ui.close();
                    }
                });
            });
        }

        ui.separator();
        self.exchanges_ui(state, ui, theme);

        // **One way out, not a roster.** This menu used to list every
        // identity in the roster, which put a second and shorter list of them
        // beside the opening screen's -- and only the roster's, so an identity
        // sitting in `~/.sqnr` that sigil had never been told about could not
        // be reached from here at all. The opening screen is the one place
        // that lists them all, draws the mark, says what is wrong with a file
        // and can ask for a passphrase. This goes back to it.
        ui.separator();
        // Beside the other thing done *to* this identity rather than seen
        // about it, and in the same shape: it was a plain button on its own
        // between the key and the exchange, which is where the facts are.
        if sigil_ui::icon_item(ui, sigil_ui::Icon::Pencil, "Edit your profile")
            .on_hover_text("Your name and title, as others see them")
            .clicked()
        {
            self.open_profile(at, state);
            ui.close();
        }
        // **Devices are the identity's, so the way to them is here.** They
        // were reachable only from an open conversation's header, which on
        // a phone meant the list had no way to them at all: to link a
        // phone, write a will or name guardians (SIP-44) one first had to
        // open a chat with somebody. The header keeps its button; this is
        // the one that does not need a conversation.
        if sigil_ui::icon_item(ui, sigil_ui::Icon::Device, "Your devices")
            .on_hover_text("Every key that acts as you, and what happens if you lose this one")
            .clicked()
        {
            self.send_as(Some(at), Cmd::Devices);
            self.send_as(Some(at), Cmd::BackupStatus);
            ctx.navigator.push_here(Route::Devices);
            ui.close();
        }
        if sigil_ui::icon_item(ui, sigil_ui::Icon::Switch, "Switch identity")
            .on_hover_text(
                "Choose another identity. This one stays open — its messages keep arriving \
                 and a call on it keeps running.",
            )
            .clicked()
        {
            self.switching = true;
            ui.close();
        }
        let _ = me;
    }

    /// The key of the exchange this identity is talking to.
    ///
    /// **Only the key.** Which exchange, and the way to another or a new one,
    /// moved to the window's title strip -- see [`ChatApp::chrome_ui`] -- where
    /// it can be seen without opening anything. What stays here is the full
    /// key of whatever is being talked to, always reachable and **labelled**:
    /// unlabelled beside the account's own key it was a second string of
    /// base58 with nothing saying which was which.
    ///
    /// # Why an exchange is not a setting
    ///
    /// The identity is the same key at every exchange, and **nothing else
    /// is**. Conversations, channel keys and SIP-17 counters belong to one
    /// exchange and do not move — SIP-31 binds the exchange into every entry
    /// signature so that they cannot. Adding one is therefore much closer to
    /// adding an account than to changing a preference, and switching between
    /// them changes the whole conversation list.
    fn exchanges_ui(&mut self, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
        let Some(key) = state.exchange else {
            ui.horizontal(|ui| {
                ui.colored_label(theme.text_muted, egui::RichText::new("at").small());
                ui.colored_label(theme.text_muted, egui::RichText::new("connecting…").small());
            });
            return;
        };
        // **The caption and the control share a row; the key has its own.**
        // A `horizontal` does not wrap: a 44-character key beside a button
        // is wider than a phone's menu, and what came after it was drawn
        // off the edge. The same shape as the key above it.
        ui.horizontal(|ui| {
            ui.colored_label(theme.text_muted, egui::RichText::new("at").small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Copy, "Copy the exchange's key")
                    .clicked()
                {
                    ui.ctx().copy_text(key.to_string());
                }
            });
        });
        // **In full**, by `the_exchange_this_list_belongs_to_is_shown_in_full`:
        // it is the key a receipt verifies under and the one a client pins
        // independently of whatever it is connected to, and a phone has no
        // hover to hide the rest of it behind.
        ui.add(
            egui::Label::new(egui::RichText::new(key.to_string()).monospace().small())
                .wrap()
                .selectable(true),
        )
        .on_hover_text("the exchange this conversation list belongs to");
        // SIP-85: it sees the home's address, not this machine's.
        if let Some(home) = &state.carried {
            ui.colored_label(
                theme.text_muted,
                egui::RichText::new(format!("through {home}")).small(),
            )
            .on_hover_text(
                "your home carries this connection: the exchange sees your home's \
                 address and your own key, never where you are",
            );
        }
    }

    /// One picture, as large as the window will take.
    ///
    /// The transcript draws a thumbnail — a picture the width of a bubble is
    /// the right size for reading past and the wrong size for looking at. This
    /// is what clicking one gets: the same bytes, bounded only by the window,
    /// and three ways out because a thing covering everything must be easy to
    /// dismiss.
    fn picture_ui(&mut self, at: &At, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
        let Some((seq, index)) = self.pane(at).viewing else {
            return;
        };
        // **A message's pictures are a set to move through.** Left and
        // Right go to the previous and next picture or clip in the same
        // message, from whichever the viewer opened on; the controls in the
        // picture's dialog do the same. Files that are neither are skipped,
        // as they are in the gallery.
        let siblings: Vec<usize> = state
            .lines
            .iter()
            .find(|l| l.seq == seq)
            .map(|l| {
                l.attachments
                    .iter()
                    .enumerate()
                    .filter(|(_, a)| {
                        a.kind == sigil_ui::attachment::IMAGE
                            || a.kind == sigil_ui::attachment::VIDEO
                    })
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default();
        let place = siblings.iter().position(|i| *i == index);
        let (left, right) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            )
        });
        let step = |place: usize, by: isize| -> Option<usize> {
            let n = siblings.len() as isize;
            let to = (place as isize + by).rem_euclid(n.max(1));
            siblings.get(to as usize).copied()
        };
        if let Some(p) = place
            && siblings.len() > 1
            && let Some(to) = match (left, right) {
                (true, _) => step(p, -1),
                (_, true) => step(p, 1),
                _ => None,
            }
        {
            self.pane(at).viewing = Some((seq, to));
            self.pane(at).look = Look::default();
            // Drawn next pass, on the new one.
            ui.ctx().request_repaint();
            return;
        }
        // Gone from under it — the message was deleted, or the conversation
        // changed — is not an error, it is nothing to show.
        let Some(line) = state.lines.iter().find(|l| l.seq == seq) else {
            self.pane(at).viewing = None;
            return;
        };
        let moment = line.at;
        let Some(file) = line.attachments.get(index) else {
            self.pane(at).viewing = None;
            return;
        };
        // **Not fetched yet is not nothing to show.** Next onto a picture
        // still on its way used to shut the viewer; it stays up on the
        // thumbnail, saying what is happening -- and, for one too big to
        // come unasked, offering to ask.
        let bytes = file.bytes.clone();
        if file.kind == sigil_ui::attachment::VIDEO
            && let Some(bytes) = bytes.clone()
        {
            return self.video_viewer_ui(at, ui, theme, seq, index, moment, file, bytes);
        }
        if sigil::Form::of(ui.ctx()).is_phone()
            && let Some(bytes) = bytes.clone()
        {
            let step = |by: isize| place.and_then(|p| step(p, by));
            return self.picture_whole_screen(
                at,
                ui,
                Whole {
                    seq,
                    index,
                    moment,
                    id: file.id.clone(),
                    bytes,
                    among: place.map(|p| (p + 1, siblings.len())),
                    previous: step(-1),
                    next: step(1),
                },
            );
        }

        let egui_ctx = ui.ctx().clone();
        // The window, so a big picture fills it and a small one does not
        // grow. `available_rect` is the whole surface here: this draws over
        // everything by construction.
        let screen = ui.ctx().viewport_rect().size();
        // **A phone shows it on the whole screen.** A dialog with margins
        // and rounded corners inside a 360-point pane is a small picture in
        // a frame, on a screen with nothing else worth looking at. The room
        // is what the system leaves, less the row of controls under it.
        let phone = sigil::Form::of(&egui_ctx).is_phone();
        let safe = sigil::Insets::safe_rect(&egui_ctx);
        let mut forward = false;
        // Set from inside the dialog, acted on after it: the viewer cannot
        // be taken down while it is being drawn.
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new(("picture", seq, index)))
            .frame(if phone {
                egui::Frame::NONE
                    .fill(egui::Color32::BLACK)
                    .inner_margin(egui::Margin::same(tokens::SPACING_XS as i8))
            } else {
                egui::Frame::NONE
                    .fill(theme.surface_primary)
                    .corner_radius(tokens::RADIUS_LG)
                    .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8))
            })
            .show(&egui_ctx, |ui| {
                ui.vertical_centered(|ui| {
                    // The same URI the transcript uses, so the decoded texture
                    // is the one already in hand rather than a second copy of
                    // the same picture under another name.
                    let room = if phone {
                        egui::vec2(
                            safe.width() - 2.0 * tokens::SPACING_XS,
                            safe.height() - tokens::BUTTON_LG - 4.0 * tokens::SPACING_SM,
                        )
                    } else {
                        screen * 0.86
                    };
                    if let Some(bytes) = bytes.clone() {
                        let image = egui::Image::from_bytes(format!("bytes://{}", file.id), bytes)
                            .corner_radius(tokens::RADIUS_MD);
                        match image.load_for_size(ui.ctx(), room) {
                            Ok(egui::load::TexturePoll::Ready { texture }) => {
                                // **The window on the picture stays the size the
                                // whole picture needs**, and zooming happens
                                // inside it. A viewer that grows as it zooms
                                // pushes its own controls off the screen.
                                let scale = (room.x / texture.size.x)
                                    .min(room.y / texture.size.y)
                                    .min(1.0);
                                let fitted = texture.size * scale;
                                let (view, held) =
                                    ui.allocate_exact_size(fitted, egui::Sense::click_and_drag());
                                let look = self.pane(at).look;
                                let touched = ui.ctx().input(|i| i.has_touch_screen());

                                if held.clicked() {
                                    if touched {
                                        // **A tap is the way out.** On a phone
                                        // the viewer *is* the whole screen, and
                                        // the tap that opened it closes it; a
                                        // finger zooms by pinching, which is
                                        // below, so nothing is lost.
                                        close = true;
                                    } else {
                                        // In to the picture's own pixels, or twice
                                        // its size when that is smaller than the
                                        // window -- clicking must always do
                                        // something -- and out again from anywhere
                                        // closer than fitting.
                                        let closest = (1.0 / scale).max(2.0);
                                        let to = if look.zoom > 1.01 { 1.0 } else { closest };
                                        self.pane(at).look = look.zoomed(to, fitted, fitted);
                                    }
                                }

                                // **Moving the pointer looks around it**, with no
                                // button held: a zoomed picture in a window is a
                                // thing to look around, and making somebody drag
                                // it makes them work for it. Only while the
                                // pointer is over the picture, so it holds still
                                // when they take it away to press Save.
                                //
                                // **A finger drags and pinches instead.** It has no
                                // position when it is not touching, so following it
                                // would jump the picture to wherever it last was.
                                let look = self.pane(at).look;
                                if touched {
                                    if held.dragged() && look.zoom > 1.01 {
                                        self.pane(at).look =
                                            look.panned(held.drag_delta(), fitted, fitted);
                                    }
                                    if let Some(pinch) =
                                        ui.input(|i| i.multi_touch().map(|m| m.zoom_delta))
                                        && (pinch - 1.0).abs() > 0.001
                                    {
                                        let look = self.pane(at).look;
                                        let to = (look.zoom * pinch).clamp(1.0, 8.0);
                                        self.pane(at).look = look.zoomed(to, fitted, fitted);
                                    }
                                } else if look.zoom > 1.01
                                    && let Some(p) = ui.ctx().pointer_latest_pos()
                                    && view.contains(p)
                                {
                                    self.pane(at).look =
                                        look.following(p - view.center(), fitted, fitted);
                                }
                                let look = self.pane(at).look;
                                ui.ctx().set_cursor_icon(if look.zoom > 1.01 {
                                    egui::CursorIcon::ZoomOut
                                } else {
                                    egui::CursorIcon::ZoomIn
                                });
                                // Clipped to the window, so what is outside it is
                                // out of sight rather than over the rest of the
                                // dialog.
                                let painting = ui.new_child(
                                    egui::UiBuilder::new().id_salt("picture").max_rect(view),
                                );
                                let mut painting = painting;
                                painting.set_clip_rect(view);
                                image.paint_at(
                                    &painting,
                                    egui::Rect::from_center_size(
                                        view.center() + look.pan,
                                        look.size(fitted),
                                    ),
                                );
                            }
                            _ => {
                                ui.add(image.max_size(room));
                            }
                        }
                    } else {
                        self.waiting_ui(at, ui, theme, seq, index, file, room);
                    }
                    ui.add_space(tokens::SPACING_SM);
                    ui.horizontal(|ui| {
                        // Where this one is among the message's pictures,
                        // and the way to the others, when there are others.
                        if let Some(p) = place
                            && siblings.len() > 1
                        {
                            if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Back, "Previous")
                                .clicked()
                                && let Some(to) = step(p, -1)
                            {
                                self.pane(at).viewing = Some((seq, to));
                                self.pane(at).look = Look::default();
                            }
                            ui.colored_label(
                                theme.text_muted,
                                egui::RichText::new(format!("{} of {}", p + 1, siblings.len()))
                                    .small(),
                            );
                            if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Forward, "Next")
                                .clicked()
                                && let Some(to) = step(p, 1)
                            {
                                self.pane(at).viewing = Some((seq, to));
                                self.pane(at).look = Look::default();
                            }
                        }
                        ui.colored_label(
                            theme.text_muted,
                            egui::RichText::new(&file.described).small(),
                        );
                        // Nothing to save until it is here.
                        // Nothing to save or to hand on until it is here.
                        if bytes.is_some() {
                            if sigil_ui::icon_button_named(
                                ui,
                                sigil_ui::Icon::Forward,
                                "Forward it",
                            )
                            .clicked()
                            {
                                forward = true;
                            }
                            if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Save, "Save…")
                                .clicked()
                            {
                                let name = save_name(moment, index, bytes.as_deref());
                                self.saving =
                                    Some((at.clone(), seq, index, files::save_file(&name)));
                                ui.ctx().request_repaint();
                            }
                        }
                        if sigil_ui::icon_button(ui, sigil_ui::Icon::Close).clicked() {
                            close = true;
                        }
                    });
                });
            });
        // The backdrop and Escape, which `should_close` covers, and the
        // control above. Three ways out of something that covers the window.
        if response.should_close() || close {
            self.pane(at).viewing = None;
        }
        // Where to send it is chosen under the composer, which is behind
        // this: the viewer closes, and the list is there.
        if forward {
            self.pane(at).forwarding = Some((seq, index));
            self.pane(at).viewing = None;
        }
    }

    /// **A phone's picture viewer is the screen.** Black from edge to edge,
    /// the picture as large as it fits, and the controls floating over it:
    /// a dialog with margins and rounded corners inside a 360-point pane is
    /// a small picture in a frame, on a screen with nothing else on it.
    ///
    /// A tap on the picture leaves -- the gesture that opened it, which is
    /// what a phone's viewer answers to everywhere -- and a pinch zooms,
    /// which is what a finger has instead of a click.
    fn picture_whole_screen(&mut self, at: &At, ui: &mut egui::Ui, w: Whole) {
        let Whole {
            seq,
            index,
            moment,
            id,
            bytes,
            among,
            previous,
            next,
        } = w;
        let egui_ctx = ui.ctx().clone();
        let theme = ColorTheme::current(&egui_ctx);
        let whole = egui_ctx.viewport_rect();
        let safe = sigil::Insets::safe_rect(&egui_ctx);
        let mut close = false;
        let mut forward = false;
        let mut save = false;
        let mut go: Option<usize> = None;
        egui::Area::new(egui::Id::new(("picture-whole", seq, index)))
            .order(egui::Order::Foreground)
            .fixed_pos(whole.min)
            .show(&egui_ctx, |ui| {
                ui.set_min_size(whole.size());
                ui.painter().rect_filled(whole, 0.0, egui::Color32::BLACK);
                // The picture, in what the system leaves less the bar over
                // it: a picture under the status bar is a picture with a
                // clock on it.
                let bar = tokens::BUTTON_LG + 2.0 * tokens::SPACING_SM;
                let room = egui::Rect::from_min_max(
                    egui::pos2(safe.left(), safe.top() + bar),
                    egui::pos2(safe.right(), safe.bottom() - bar),
                );
                let uri = format!("bytes://{id}");
                let image = egui::Image::from_bytes(uri, egui::load::Bytes::Shared(bytes.clone()));
                if let Ok(egui::load::TexturePoll::Ready { texture }) =
                    image.load_for_size(&egui_ctx, room.size())
                {
                    let scale = (room.width() / texture.size.x)
                        .min(room.height() / texture.size.y)
                        .min(1.0);
                    let fitted = texture.size * scale;
                    let view = egui::Rect::from_center_size(room.center(), fitted);
                    let held = ui.interact(
                        view,
                        egui::Id::new(("picture-whole-held", seq, index)),
                        egui::Sense::click_and_drag(),
                    );
                    let look = self.pane(at).look;
                    // Pinched, and dragged about once there is more of it
                    // than fits.
                    if held.dragged() && look.zoom > 1.01 {
                        self.pane(at).look = look.panned(held.drag_delta(), fitted, fitted);
                    }
                    if let Some(pinch) = ui.input(|i| i.multi_touch().map(|m| m.zoom_delta))
                        && (pinch - 1.0).abs() > 0.001
                    {
                        let look = self.pane(at).look;
                        let to = (look.zoom * pinch).clamp(1.0, 8.0);
                        self.pane(at).look = look.zoomed(to, fitted, fitted);
                    }
                    if held.clicked() {
                        close = true;
                    }
                    let look = self.pane(at).look;
                    let mut painting =
                        ui.new_child(egui::UiBuilder::new().id_salt("whole").max_rect(view));
                    painting.set_clip_rect(view);
                    image.paint_at(
                        &painting,
                        egui::Rect::from_center_size(view.center() + look.pan, look.size(fitted)),
                    );
                } else {
                    let mut waiting =
                        ui.new_child(egui::UiBuilder::new().id_salt("whole-wait").max_rect(room));
                    waiting.add(image.max_size(room.size()));
                }

                // The bar: the way out at the left, and what is done to the
                // picture at the right. Over the picture, not under it --
                // the picture is the screen.
                let row = egui::Rect::from_min_size(
                    egui::pos2(
                        safe.left() + tokens::SPACING_SM,
                        safe.top() + tokens::SPACING_SM,
                    ),
                    egui::vec2(safe.width() - 2.0 * tokens::SPACING_SM, tokens::BUTTON_LG),
                );
                let mut top =
                    ui.new_child(egui::UiBuilder::new().id_salt("whole-bar").max_rect(row));
                top.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
                top.horizontal(|ui| {
                    if sigil_ui::icon_button(ui, sigil_ui::Icon::Close).clicked() {
                        close = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Save, "Save…").clicked()
                        {
                            save = true;
                        }
                        if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Forward, "Forward it")
                            .clicked()
                        {
                            forward = true;
                        }
                    });
                });

                // Where this one is among the message's pictures, and the
                // way to the others: along the foot, where a thumb is.
                if let Some((which, of)) = among
                    && of > 1
                {
                    let foot = egui::Rect::from_min_size(
                        egui::pos2(
                            safe.left() + tokens::SPACING_SM,
                            safe.bottom() - tokens::BUTTON_LG - tokens::SPACING_SM,
                        ),
                        egui::vec2(safe.width() - 2.0 * tokens::SPACING_SM, tokens::BUTTON_LG),
                    );
                    let mut bottom =
                        ui.new_child(egui::UiBuilder::new().id_salt("whole-foot").max_rect(foot));
                    bottom.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
                    bottom.horizontal(|ui| {
                        if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Back, "Previous")
                            .clicked()
                        {
                            go = previous;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Forward, "Next")
                                .clicked()
                            {
                                go = next;
                            }
                            ui.with_layout(
                                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                                |ui| {
                                    ui.colored_label(
                                        theme.text_muted,
                                        egui::RichText::new(format!("{which} of {of}")).small(),
                                    );
                                },
                            );
                        });
                    });
                }
            });
        if egui_ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if let Some(to) = go {
            self.pane(at).viewing = Some((seq, to));
            self.pane(at).look = Look::default();
            egui_ctx.request_repaint();
            return;
        }
        if save {
            let name = save_name(moment, index, Some(&bytes));
            self.saving = Some((at.clone(), seq, index, files::save_file(&name)));
            egui_ctx.request_repaint();
        }
        if forward {
            self.pane(at).forwarding = Some((seq, index));
            close = true;
        }
        if close {
            self.pane(at).viewing = None;
            self.pane(at).look = Look::default();
        }
    }

    /// The viewer on a picture or clip whose bytes have not arrived: its
    /// thumbnail, as large as it goes, and what is happening under it --
    /// the same three states the bubble's own row distinguishes, because
    /// "fetching" over a fetch that will never start is a lie a reader
    /// waits on.
    #[allow(clippy::too_many_arguments)]
    fn waiting_ui(
        &mut self,
        at: &At,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        seq: u64,
        index: usize,
        file: &session::Attached,
        room: egui::Vec2,
    ) {
        if !file.preview.is_empty() {
            let uri = format!("bytes://{}-preview", file.id);
            ui.ctx()
                .include_bytes(uri.clone(), egui::load::Bytes::Shared(file.preview.clone()));
            ui.add(
                egui::Image::from_bytes(uri, egui::load::Bytes::Shared(file.preview.clone()))
                    .corner_radius(tokens::RADIUS_MD)
                    .show_loading_spinner(false)
                    .max_size(room * 0.6),
            );
        }
        ui.horizontal(|ui| {
            if file.missing {
                ui.colored_label(
                    theme.warning,
                    egui::RichText::new("preview — could not be fetched").small(),
                );
                if ui.small_button("Try again").clicked() {
                    self.send_as(Some(at), Cmd::Refetch);
                }
            } else if file.held {
                ui.colored_label(
                    theme.text_muted,
                    egui::RichText::new(format!(
                        "preview — {}",
                        sigil_ui::attachment::human(file.size)
                    ))
                    .small(),
                );
                if ui.small_button("Fetch").clicked() {
                    self.send_as(Some(at), Cmd::Fetch { seq, index });
                }
            } else {
                ui.colored_label(
                    theme.text_muted,
                    egui::RichText::new("preview — fetching the full image").small(),
                );
            }
        });
    }

    /// A video, as large as the window will take: the one place it plays,
    /// with the same three ways out as any dialog. Closing stops it.
    #[allow(clippy::too_many_arguments)]
    fn video_viewer_ui(
        &mut self,
        at: &At,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        seq: u64,
        index: usize,
        moment: u64,
        file: &session::Attached,
        bytes: std::sync::Arc<[u8]>,
    ) {
        let egui_ctx = ui.ctx().clone();
        if !self.pane(at).players.contains_key(&file.id) {
            self.start_video(at, &egui_ctx, &file.id, bytes.clone());
        }
        if let Some(playing) = self.pane(at).players.get_mut(&file.id) {
            playing.refresh(&egui_ctx, &file.id);
        }
        let screen = ui.ctx().viewport_rect().size();
        let room = screen * 0.86;
        let mut done = sigil_ui::VideoAction::default();
        let mut close = false;
        let mut save = false;
        let mut forward = false;
        // **A phone plays it on the whole screen.** There is nothing else
        // worth looking at while a clip is open on a 360-point pane, and a
        // dialog with margins inside a screen that size is a small video in
        // a frame. A window keeps its dialog until somebody asks for the
        // whole screen.
        let phone = sigil::Form::of(&egui_ctx).is_phone();
        if self.pane(at).whole_screen || phone {
            // **The video is the screen.** Edge to edge on black, its own bar
            // over it, nothing else drawn: the viewer's dialog inside a
            // fullscreen window was still a small video with a frame round
            // it, which is not what "whole screen" means. Escape, the bar's
            // own control, or a press on the backdrop bring the window back.
            let whole = ui.ctx().viewport_rect();
            // **One area, the video and its bar in it.** The bar was an
            // area of its own over the video's, and on the phone it was
            // nowhere to be seen: two areas of the same order are stacked
            // in whatever way egui last moved them, and the one holding a
            // widget that gets pressed comes to the top. Drawn inside,
            // after the picture, it is over the picture by construction.
            let safe = sigil::Insets::safe_rect(&egui_ctx);
            egui::Area::new(egui::Id::new(("video-whole", seq, index)))
                .order(egui::Order::Foreground)
                .fixed_pos(whole.min)
                .show(&egui_ctx, |ui| {
                    ui.set_min_size(whole.size());
                    ui.painter().rect_filled(whole, 0.0, egui::Color32::BLACK);
                    let pane = self.panes.entry(at.clone()).or_default();
                    let view = video_view(pane, file, sigil_ui::video::Place::Viewer);
                    let size = sigil_ui::video::size_for(view.shape, whole.width(), whole.height());
                    let at_pos = whole.center() - size / 2.0;
                    let mut inner = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(egui::Rect::from_min_size(at_pos, size))
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    done = sigil_ui::video(&mut inner, &view, whole.width(), whole.height());

                    // What is done *to* the clip, along the top and inside
                    // what the system leaves: the way out at the left, and
                    // at the right the two things anybody wants a clip
                    // for. The video's own bar is along the bottom of it
                    // and has the playing.
                    let row = egui::Rect::from_min_size(
                        egui::pos2(
                            safe.left() + tokens::SPACING_SM,
                            safe.top() + tokens::SPACING_SM,
                        ),
                        egui::vec2(safe.width() - 2.0 * tokens::SPACING_SM, tokens::BUTTON_LG),
                    );
                    let mut top =
                        ui.new_child(egui::UiBuilder::new().id_salt("clip-bar").max_rect(row));
                    top.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
                    top.horizontal(|ui| {
                        if sigil_ui::icon_button(ui, sigil_ui::Icon::Close).clicked() {
                            close = true;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Save, "Save…")
                                .clicked()
                            {
                                save = true;
                            }
                            if sigil_ui::icon_button_named(
                                ui,
                                sigil_ui::Icon::Forward,
                                "Forward it",
                            )
                            .clicked()
                            {
                                forward = true;
                            }
                        });
                    });
                });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            // A press on the picture itself: on a phone the way out of the
            // whole screen, which is what a phone's viewer is; in a window,
            // play or pause, as it has always been.
            if done.tapped {
                if phone {
                    close = true;
                } else if let Some(playing) = self.pane(at).players.get(&file.id) {
                    playing.player.toggle();
                }
            }
            if let Some(playing) = self.pane(at).players.get(&file.id) {
                if done.toggle {
                    playing.player.toggle();
                }
                if let Some(ms) = done.seek {
                    playing.player.seek(ms);
                }
                if let Some(mute) = done.mute {
                    playing.player.set_volume(if mute { 0.0 } else { 1.0 });
                }
            }
            if save {
                let name = save_name(moment, index, Some(&bytes));
                self.saving = Some((at.clone(), seq, index, files::save_file(&name)));
                egui_ctx.request_repaint();
            }
            if forward {
                self.pane(at).forwarding = Some((seq, index));
                close = true;
            }
            // On a phone the whole screen *is* the viewer: the control that
            // would shrink it back into a dialog leaves instead.
            if (done.fullscreen && phone) || close {
                self.pane(at).viewing = None;
                self.pane(at).players.remove(&file.id);
            }
            if done.fullscreen || close {
                self.pane(at).whole_screen = false;
                if !phone {
                    egui_ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                }
            }
            return;
        }
        let response = egui::Modal::new(egui::Id::new(("video", seq, index)))
            .frame(
                egui::Frame::NONE
                    .fill(theme.surface_primary)
                    .corner_radius(tokens::RADIUS_LG)
                    .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8)),
            )
            .show(&egui_ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let pane = self.panes.entry(at.clone()).or_default();
                    let view = video_view(pane, file, sigil_ui::video::Place::Viewer);
                    done = sigil_ui::video(ui, &view, room.x, room.y - tokens::BUTTON_SM * 2.0);
                    ui.add_space(tokens::SPACING_SM);
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            theme.text_muted,
                            egui::RichText::new(&file.described).small(),
                        );
                        if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Save, "Save…").clicked()
                        {
                            save = true;
                        }
                        if sigil_ui::icon_button(ui, sigil_ui::Icon::Close).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if let Some(playing) = self.pane(at).players.get(&file.id) {
            if done.toggle || done.tapped {
                playing.player.toggle();
            }
            if let Some(ms) = done.seek {
                playing.player.seek(ms);
            }
            if let Some(mute) = done.mute {
                playing.player.set_volume(if mute { 0.0 } else { 1.0 });
            }
        }
        if save {
            let name = save_name(moment, index, Some(&bytes));
            self.saving = Some((at.clone(), seq, index, files::save_file(&name)));
            egui_ctx.request_repaint();
        }
        // A phone's viewer already fills the screen, and a window command
        // there is a command to nothing.
        if done.fullscreen && !sigil::Form::of(&egui_ctx).is_phone() {
            let whole = !self.pane(at).whole_screen;
            self.pane(at).whole_screen = whole;
            egui_ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(whole));
        }
        if close || response.should_close() {
            self.pane(at).viewing = None;
            // The viewer was the only place it played: leaving takes the
            // player with it, so nothing goes on sounding behind a
            // thumbnail.
            self.pane(at).players.remove(&file.id);
            if self.pane(at).whole_screen {
                self.pane(at).whole_screen = false;
                egui_ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
            }
        }
    }

    /// The short forms, over the top of everything.
    ///
    /// See [`Dialog`] for why none of these is in the column any more. One
    /// dialog at a time, dismissed by the backdrop, by Escape, or by its own
    /// control -- three ways out, because a form somebody cannot leave is
    /// worse than one they never opened.
    fn dialogs_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let Some(which) = self.pane(at).dialog else {
            return;
        };
        let me = at.0;
        let egui_ctx = ui.ctx().clone();
        // Its own margin, deeper than a popup's. A dialog is the only thing on
        // screen while it is up, and the default popup padding puts the
        // heading a few pixels from the edge -- which reads as a tooltip that
        // grew rather than as something to fill in.
        let frame = egui::Frame::popup(&ui.style().clone())
            .inner_margin(egui::Margin::same(tokens::SPACING_LG as i8))
            .corner_radius(tokens::RADIUS_LG);
        let response = egui::Modal::new(egui::Id::new(("chat-dialog", &at.0, &at.1)))
            .frame(frame)
            .show(&egui_ctx, |ui| {
                // As wide as a dialog likes, or as wide as the screen has
                // once a margin is kept: a phone is narrower than a dialog.
                //
                // **Wider when the screen is short.** A dialog cannot scroll
                // (see `every_dialog_fits_a_phones_screen`), and a phone held
                // sideways is 360 points tall -- on which Verify, which is
                // six words *and* a QR *and* a key, stood 482 points high
                // and ran 61 points off the top. It is the one dialog with
                // something to put in a second column, and a short screen is
                // exactly the one with width to spare.
                let screen = ui.ctx().content_rect();
                let want = if matches!(which, Dialog::Verify(_)) && screen.height() < SHORT {
                    WIDE
                } else {
                    360.0
                };
                ui.set_width(
                    want.min(screen.width() - 2.0 * tokens::SPACING_XL)
                        .max(200.0),
                );
                match which {
                    Dialog::Compose => self.compose_dialog(at, ui, theme),
                    Dialog::Profile => self.profile_dialog(at, ui, theme),
                    Dialog::Exchange => self.exchange_dialog(ctx, at, me, ui, theme),
                    Dialog::Name => self.name_dialog(at, ui, theme),
                    Dialog::Verify(who) => self.verify_dialog(at, state, who, ui, theme),
                    Dialog::Report { target } => self.report_dialog(at, target, ui, theme),
                }
            });
        if response.should_close() {
            self.pane(at).dialog = None;
        }
        let _ = state;
    }

    /// Start something: somebody by key or name, a group, or a public channel.
    fn compose_dialog(&mut self, at: &At, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("New conversation");
        ui.add_space(tokens::SPACING_SM);
        // A visible label, not only a hint: a hint disappears the moment
        // somebody types and never reaches the accessibility tree at all.
        // The same row every other field in sigil is: the label above on a
        // narrow pane, the box given what is left, and the action beside it
        // rather than under it. It was a full-width box with "Add" on a row
        // of its own below, which on a phone is a third row for a dialog
        // that is four things tall.
        let (field, go) = sigil_ui::labelled_field(
            ui,
            "Write to",
            &mut self.panes.entry(at.clone()).or_default().adding,
            "their key, or name@domain at any exchange",
            Some(sigil_ui::Action::Mark(sigil_ui::Icon::Plus, "Add")),
        );
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if go || entered {
            let typed = self.pane(at).adding.trim().to_string();
            // SIP-60: `label@domain` naming another exchange is reached
            // through the home -- this identity's default session, and only
            // that one: the create is carried after this identity's Move,
            // and a Move presented from an added exchange would move the
            // home there.
            let here = self.state_of(Some(at)).domain;
            if let Some((label, domain)) = typed.rsplit_once('@')
                && !label.is_empty()
                && !domain.is_empty()
                && typed.parse::<PubKey>().is_err()
                && here.as_deref() != Some(domain)
            {
                if at.1.is_empty() {
                    let pane = self.pane(at);
                    pane.add_trouble = None;
                    pane.adding.clear();
                    pane.dialog = None;
                    let identity = self.identity_paths.get(&at.0).cloned();
                    self.send_as(
                        Some(at),
                        Cmd::OpenRemote {
                            target: typed,
                            identity,
                        },
                    );
                } else {
                    self.pane(at).add_trouble = Some(format!(
                        "{domain} is another exchange: write to people there from your \
                         home, not from {}.",
                        at.1
                    ));
                }
                return;
            }
            match typed.parse::<PubKey>() {
                Ok(who) => {
                    let pane = self.pane(at);
                    pane.add_trouble = None;
                    pane.adding.clear();
                    pane.dialog = None;
                    self.send_as(Some(at), Cmd::AddContact(who, String::new()));
                    self.send_as(Some(at), Cmd::OpenDm(who));
                }
                // Not a key, so try it as a SIP-38 name. A name is looked up
                // at the exchange and resolves to exactly one account, which
                // is the whole of what makes it usable here.
                Err(_) if typed.contains('@') => {
                    let pane = self.pane(at);
                    pane.add_trouble = None;
                    pane.adding.clear();
                    pane.dialog = None;
                    self.send_as(Some(at), Cmd::OpenByName(typed));
                }
                Err(e) => {
                    self.pane(at).add_trouble =
                        Some(format!("not a key, and not a name@domain: {e}"))
                }
            }
        }
        // Refused where it was typed, rather than swallowed.
        if let Some(trouble) = self.panes.get(at).and_then(|p| p.add_trouble.clone()) {
            ui.colored_label(theme.destructive, trouble);
        }

        ui.add_space(tokens::SPACING_MD);
        ui.separator();
        ui.add_space(tokens::SPACING_SM);
        // Two rows with their icons, the shape every menu in sigil has:
        // they were two named buttons side by side under a field whose
        // action is a mark.
        if sigil_ui::icon_item(ui, sigil_ui::Icon::Plus, "New group").clicked() {
            // A group's name is a sealed entry, so it is named after it
            // exists rather than before.
            self.pane(at).dialog = None;
            self.send_as(Some(at), Cmd::NewGroup("New group".into()));
        }
        if sigil_ui::icon_item(ui, sigil_ui::Icon::Public, "New public channel")
            .on_hover_text("Anybody may find and join it, and nothing said in it is encrypted.")
            .clicked()
        {
            self.pane(at).dialog = None;
            self.send_as(
                Some(at),
                Cmd::NewPublic {
                    name: "New channel".into(),
                    topic: String::new(),
                },
            );
        }
        // Said beside the control, not in a help page. A public channel is
        // plaintext by design -- anybody may join, so any key in it is public.
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new("A public channel is not encrypted.").small(),
        );
    }

    /// Fill the channel settings fields from what the channel actually is.
    ///
    /// **They were never filled.** Both boxes opened empty over a channel that
    /// had a name and a topic -- which reads as "this has no name" -- and
    /// pressing Set beside an empty box publishes the empty string, so the
    /// pane offered to erase the name of every channel somebody opened it on.
    ///
    /// Done **when the view is drawn for a channel it has not been drawn for**
    /// rather than by whatever opened it. A route is reached more ways than
    /// there are controls that push it -- back, forward, a restored stack --
    /// and seeding at the call sites leaves every other way in showing empty
    /// boxes, which is the same bug with a smaller footprint. This way the
    /// pane cannot be on screen unseeded.
    ///
    /// Typing is kept: the channel is what it keys on, so a half-typed name
    /// survives going to Members and back and only a *different* channel
    /// replaces it.
    fn fill_settings(&mut self, at: &At, state: &ChatState) {
        let Some(channel) = state.open else { return };
        if self.panes.entry(at.clone()).or_default().settings_for == Some(channel) {
            return;
        }
        let name = state
            .conversations
            .iter()
            .find(|c| c.channel == channel)
            .map(|c| c.label.clone())
            .unwrap_or_default();
        let topic = state.topic.clone();
        let pane = self.panes.entry(at.clone()).or_default();
        pane.channel_name = name;
        pane.channel_topic = topic;
        pane.settings_for = Some(channel);
    }

    /// Open "Your profile", seeded from what is actually published.
    ///
    /// One path, reached from the menu item and from clicking your own name.
    /// Two would seed it two ways, and one of them would eventually open an
    /// empty box over a name that exists -- which reads as "you have no name"
    /// and *publishes* that the moment somebody presses the button.
    fn open_profile(&mut self, at: &At, state: &ChatState) {
        let (name, title) = (
            state.mine.name.clone().unwrap_or_default(),
            state.mine.title.clone().unwrap_or_default(),
        );
        let pane = self.panes.entry(at.clone()).or_default();
        pane.name = name;
        pane.title = title;
        pane.dialog = Some(Dialog::Profile);
    }

    /// Your own SIP-21 profile: self-declared, attested by nobody.
    fn profile_dialog(&mut self, at: &At, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Your profile");
        ui.add_space(tokens::SPACING_SM);
        ui.label("Name");
        let width = ui.available_width();
        sigil_ui::field(
            ui,
            &mut self.panes.entry(at.clone()).or_default().name,
            "what you would like to be called",
            width,
        );
        ui.add_space(tokens::SPACING_XS);
        ui.label("Title");
        sigil_ui::field(
            ui,
            &mut self.panes.entry(at.clone()).or_default().title,
            "what you do, if you want it shown",
            width,
        );
        // Said next to the field rather than in a help page. A title asserts
        // standing, and somebody typing one should know that nothing behind
        // it is checked.
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new("Both are what you say about yourself. Nobody verifies either.")
                .small(),
        );
        ui.add_space(tokens::SPACING_SM);
        ui.horizontal(|ui| {
            if ui.button("Publish").clicked() {
                let pane = self.panes.entry(at.clone()).or_default();
                let (name, title) = (pane.name.clone(), pane.title.clone());
                pane.dialog = None;
                self.send_as(Some(at), Cmd::SetProfile { name, title });
            }
            if ui.button("Cancel").clicked() {
                self.pane(at).dialog = None;
            }
        });
    }

    /// Claim a SIP-38 name at this exchange.
    ///
    /// # Why this is not the profile
    ///
    /// A profile name is what somebody says about themselves and **nobody
    /// attests it** (SIP-21). A SIP-38 name is bound at the exchange, resolves
    /// to exactly one account, and is what lets anybody write to you as
    /// `name@domain`. They are two different things that both get called a
    /// name, so they get two dialogs and each says which it is.
    fn name_dialog(&mut self, at: &At, ui: &mut egui::Ui, theme: &ColorTheme) {
        // What is held now, if anything: the dialog is reached both from an
        // unclaimed domain line and from the name itself, and it had only
        // ever been able to say the first of those.
        let held = self
            .state_of(Some(at))
            .mine
            .handle
            .clone()
            .filter(|h| !h.is_empty());
        ui.heading(if held.is_some() {
            "Your name here"
        } else {
            "Claim a name"
        });
        ui.add_space(tokens::SPACING_SM);
        if let Some(handle) = &held {
            ui.add(
                egui::Label::new(egui::RichText::new(handle).monospace())
                    .wrap()
                    .selectable(true),
            );
            ui.add_space(tokens::SPACING_SM);
            ui.label("Take another instead");
        } else {
            ui.label("Name");
        }
        let width = ui.available_width();
        let field = sigil_ui::field(
            ui,
            &mut self.panes.entry(at.clone()).or_default().naming,
            "the name you want, without the domain",
            width,
        );
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(
                "Bound at this exchange, so it means nothing at another one. Whether \
                 anybody may take a free name is the operator's policy.",
            )
            .small(),
        );
        ui.add_space(tokens::SPACING_SM);
        let mut release = None;
        ui.horizontal(|ui| {
            if ui.button("Claim").clicked() || entered {
                let name = self.pane(at).naming.trim().to_string();
                if !name.is_empty() {
                    self.pane(at).naming.clear();
                    self.pane(at).dialog = None;
                    self.send_as(Some(at), Cmd::ClaimName(name));
                }
            }
            // **The only way to give a name up**, and it is here rather than
            // on the card because it cannot be undone: the name goes back to
            // the pool and somebody else may take it. Beside the sentence
            // that says so.
            if let Some(handle) = &held
                && ui
                    .button("Give it up")
                    .on_hover_text(
                        "Stop being reachable at this name. Nothing is deleted — your \
                         conversations, keys and counters are untouched — and somebody \
                         else may take it afterwards.",
                    )
                    .clicked()
            {
                // The bare local part: a release names it, and the exchange
                // it is released at is the one being talked to.
                release = Some(handle.split('@').next().unwrap_or(handle).to_string());
            }
            if ui.button("Cancel").clicked() {
                let pane = self.pane(at);
                pane.naming.clear();
                pane.dialog = None;
            }
        });
        if let Some(local) = release {
            let pane = self.pane(at);
            pane.naming.clear();
            pane.dialog = None;
            self.send_as(Some(at), Cmd::ReleaseName(local));
        }
    }

    /// SIP-56: what is wrong, in one of the four words the wire has, and a
    /// note. Said plainly where it is typed: the note is stored in the clear
    /// at the exchange, and the admins see who reported.
    fn report_dialog(&mut self, at: &At, target: u64, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading(if target == 0 {
            "Report this room"
        } else {
            "Report this message"
        });
        ui.add_space(tokens::SPACING_SM);
        ui.label("Why");
        {
            let pane = self.panes.entry(at.clone()).or_default();
            ui.horizontal_wrapped(|ui| {
                for (n, word) in session::REASONS {
                    ui.radio_value(&mut pane.report_reason, n, word);
                }
            });
        }
        ui.add_space(tokens::SPACING_SM);
        ui.label("Note");
        let width = ui.available_width();
        sigil_ui::field(
            ui,
            &mut self.panes.entry(at.clone()).or_default().report_note,
            "what the admins should know (optional)",
            width,
        );
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(
                "The note is stored in the clear at the exchange. The admins see who \
                 reported it; nobody else does.",
            )
            .small(),
        );
        ui.add_space(tokens::SPACING_SM);
        ui.horizontal(|ui| {
            if ui.button("Report").clicked() {
                let pane = self.pane(at);
                let reason = pane.report_reason;
                let note = pane.report_note.trim().to_string();
                pane.report_note.clear();
                pane.dialog = None;
                self.send_as(
                    Some(at),
                    Cmd::Report {
                        target,
                        reason,
                        note,
                    },
                );
            }
            if ui.button("Cancel").clicked() {
                let pane = self.pane(at);
                pane.report_note.clear();
                pane.dialog = None;
            }
        });
    }

    /// The safety words for us and `who`, and the code a camera reads, with
    /// the one question that matters: did they match. Nothing here decides
    /// it; the two people do, by a channel the exchange is not in.
    fn verify_dialog(
        &mut self,
        at: &At,
        state: &ChatState,
        who: PubKey,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let me = at.0;
        let label = state
            .people
            .get(&who)
            .map(|p| p.label(&who))
            .unwrap_or_else(|| sigil_ui::message::short(&who.to_string()));
        let code = sqex_proto::safety::code(&me, &who);
        let words = sqex_proto::safety::words_for(&me, &who);
        // **Beside, not below, when the page is short.** Upright this reads
        // straight down: what to do, the words, the code, the key. Sideways
        // there are only 360 points of page and this stood 482 high, which a
        // dialog cannot scroll its way out of -- so the code and the key go
        // into a column of their own and the words keep the page. The wider
        // box is asked for in `dialogs_ui`; this reads the width it was
        // given rather than the screen, so the two agree by construction.
        let beside = ui.available_width() >= WIDE - tokens::SPACING_XL;
        let code_ui = |ui: &mut egui::Ui, theme: &ColorTheme| {
            ui.horizontal(|ui| {
                sigil_ui::qr(ui, &code, 132.0);
                ui.add_space(tokens::SPACING_SM);
                ui.vertical(|ui| {
                    ui.colored_label(theme.text_muted, egui::RichText::new("their key").small());
                    ui.add(
                        egui::Label::new(egui::RichText::new(who.to_string()).monospace().small())
                            .wrap()
                            .selectable(true),
                    );
                });
            });
        };
        let words_ui = |ui: &mut egui::Ui| {
            // Large, in two rows of three: read at speaking speed, not
            // squinted at.
            for row in words.chunks(3) {
                ui.horizontal(|ui| {
                    for word in row {
                        ui.label(egui::RichText::new(*word).heading().strong());
                        ui.add_space(tokens::SPACING_MD);
                    }
                });
            }
        };
        ui.heading(format!("Verify {label}"));
        ui.add_space(tokens::SPACING_SM);
        ui.colored_label(
            theme.text_secondary,
            "Read these six words to each other, or scan the code. They are the same \
             on both screens or they are not, and only the two of you can tell.",
        );
        ui.add_space(tokens::SPACING_MD);
        if beside {
            let room = ui.available_width();
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(room * 0.5, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| words_ui(ui),
                );
                ui.add_space(tokens::SPACING_LG);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| code_ui(ui, theme),
                );
            });
        } else {
            words_ui(ui);
            ui.add_space(tokens::SPACING_MD);
            code_ui(ui, theme);
        }
        ui.add_space(tokens::SPACING_MD);
        let verified_at = state.verified.get(&who).copied();
        if let Some(at_secs) = verified_at {
            ui.horizontal(|ui| {
                sigil_ui::verified_mark(ui);
                ui.colored_label(
                    theme.text_secondary,
                    format!("Verified {}", sigil_ui::brief(at_secs, self.now())),
                );
            });
            ui.add_space(tokens::SPACING_SM);
        }
        // SIP-27: what others have said at this exchange about this key --
        // the statement the checkbox below offers to make, read back. Their
        // word, shown to be read and not acted on: a name is an assertion
        // and so is this, and the words above are the only check. Absent
        // until the exchange has answered, so an empty line is never drawn
        // for a question still in flight.
        if let Some(issuers) = state.attested.get(&who) {
            if issuers.is_empty() {
                ui.colored_label(
                    theme.text_muted,
                    egui::RichText::new("Nobody else has said they compared these words.").small(),
                );
            } else {
                ui.colored_label(
                    theme.text_muted,
                    egui::RichText::new(format!(
                        "{} said at this exchange that they compared them: {}. Their word, \
                         not a check — the words above are.",
                        if issuers.len() == 1 {
                            "One person has".to_string()
                        } else {
                            format!("{} people have", issuers.len())
                        },
                        issuers
                            .iter()
                            .map(|k| {
                                state
                                    .people
                                    .get(k)
                                    .map(|p| p.label(k))
                                    .unwrap_or_else(|| sigil_ui::message::short(&k.to_string()))
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                    .small(),
                );
            }
            ui.add_space(tokens::SPACING_SM);
        }
        let pane = self.panes.entry(at.clone()).or_default();
        ui.checkbox(
            &mut pane.attest_too,
            "Say at the exchange that we compared them",
        )
        .on_hover_text(
            "A signed statement others may read and must not act on. It tells the \
             exchange, and anyone who asks, that the two of you spoke.",
        );
        ui.add_space(tokens::SPACING_SM);
        ui.horizontal(|ui| {
            if verified_at.is_none() {
                if ui.button("They match").clicked() {
                    let attest = self.pane(at).attest_too;
                    self.pane(at).attest_too = false;
                    self.pane(at).dialog = None;
                    self.send_as(Some(at), Cmd::Verify(who));
                    if attest {
                        self.send_as(Some(at), Cmd::Attest(who));
                    }
                }
                if ui.button("Not yet").clicked() {
                    self.pane(at).attest_too = false;
                    self.pane(at).dialog = None;
                }
            } else {
                if ui
                    .button("Withdraw")
                    .on_hover_text("Take your mark back.")
                    .clicked()
                {
                    self.pane(at).attest_too = false;
                    self.pane(at).dialog = None;
                    self.send_as(Some(at), Cmd::Unverify(who));
                }
                if ui.button("Close").clicked() {
                    self.pane(at).attest_too = false;
                    self.pane(at).dialog = None;
                }
            }
        });
    }

    /// Connect this identity to another exchange.
    fn exchange_dialog(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        me: PubKey,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let which = ctx.accounts.active_index();
        let home = self.home_domain(ctx);
        ui.heading("Add an exchange");
        ui.add_space(tokens::SPACING_SM);
        ui.label("Exchange");
        let width = ui.available_width();
        sigil_ui::field(
            ui,
            &mut self.panes.entry(at.clone()).or_default().exchange,
            "a domain, or host:port",
            width,
        );
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(
                "Your key is the same there. Your conversations are not — they belong to \
                 one exchange and cannot be moved.",
            )
            .small(),
        );
        // SIP-85: through the home. The exchange then sees the home's
        // address and this identity's own key, never where this machine is.
        // A home is reached by name, so a default that is an address cannot
        // be one -- said, rather than a box that does nothing.
        match &home {
            Some(home) => {
                let pane = self.panes.entry(at.clone()).or_default();
                ui.checkbox(&mut pane.exchange_via, format!("Reach it through {home}"))
                    .on_hover_text(
                        "your home carries the connection: the exchange sees your home's \
                         address and your own key, never where you are (SIP-85)",
                    );
            }
            None => {
                ui.colored_label(
                    theme.text_muted,
                    egui::RichText::new(
                        "Your default exchange is an address, not a domain, so it cannot \
                         carry this connection for you.",
                    )
                    .small(),
                );
            }
        }
        if let Some(trouble) = self.panes.get(at).and_then(|p| p.add_trouble.clone()) {
            ui.colored_label(theme.destructive, trouble);
        }
        // **What this exchange federates with** (SIP-39 §The peer directory), each one press
        // away -- through the same path as a domain typed by hand, which
        // discovers it and pins the key it finds. A listing is a hint the
        // exchange gave; the pin is the fact. Only peers with a domain: a
        // key alone is not something the roster can dial.
        let state = self.state_of(Some(at));
        let held = ctx
            .accounts
            .held(which)
            .map(|h| h.exchanges())
            .unwrap_or_default();
        let offered: Vec<(PubKey, String)> = state
            .peers
            .iter()
            .filter(|(_, d)| {
                !d.is_empty()
                    && state.domain.as_deref() != Some(d.as_str())
                    && !held.iter().any(|h| h.eq_ignore_ascii_case(d))
            })
            .cloned()
            .collect();
        if !offered.is_empty() {
            ui.add_space(tokens::SPACING_SM);
            ui.colored_label(
                theme.text_muted,
                egui::RichText::new(format!(
                    "{} federates with",
                    state.domain.as_deref().unwrap_or("This exchange")
                ))
                .small(),
            );
            for (key, domain) in offered {
                ui.horizontal(|ui| {
                    if ui
                        .button(&domain)
                        .on_hover_text(format!(
                            "Add {domain}. It is discovered over DNSSEC and refused if its key \
                             is not {key}."
                        ))
                        .clicked()
                    {
                        self.pane(at).exchange = domain.clone();
                    }
                    ui.colored_label(
                        theme.text_muted,
                        egui::RichText::new(sigil_ui::message::short(&key.to_string()))
                            .monospace()
                            .small(),
                    );
                });
            }
        }
        ui.add_space(tokens::SPACING_SM);
        ui.horizontal(|ui| {
            if ui.button("Add").clicked() {
                let named = self.pane(at).exchange.trim().to_string();
                let via = home.filter(|_| self.pane(at).exchange_via);
                if via
                    .as_deref()
                    .is_some_and(|h| h.eq_ignore_ascii_case(&named))
                {
                    self.pane(at).add_trouble = Some(format!(
                        "{named} is your home; it carries connections to other exchanges."
                    ));
                } else if ctx.accounts.add_exchange(which, &named, via) {
                    let pane = self.pane(at);
                    pane.exchange.clear();
                    pane.exchange_via = false;
                    pane.add_trouble = None;
                    pane.dialog = None;
                    // Shown straight away: adding one and staying where you
                    // were makes it look as though nothing happened.
                    ctx.accounts.show_exchange(me, Some(named));
                } else {
                    // **Refused where it was typed.** `add_exchange` answers
                    // `false` for an empty name and for one already held, and
                    // this dropped both on the floor: the dialog stayed open
                    // with the text still in it and nothing said why.
                    self.pane(at).add_trouble = Some(if named.is_empty() {
                        "Name an exchange — a domain, or host:port.".to_string()
                    } else {
                        format!("This identity is already connected to {named}.")
                    });
                }
            }
            if ui.button("Cancel").clicked() {
                let pane = self.pane(at);
                pane.exchange.clear();
                pane.exchange_via = false;
                pane.dialog = None;
            }
        });
    }

    /// What a search turned up, in place of the list.
    fn hits_ui(
        &mut self,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        now: u64,
    ) {
        // Said every time, in the count and again on hovering it, not once
        // in a help page: an empty result here means "not in what this
        // client has opened", which is a different fact from "never said",
        // and only this client can tell them apart.
        if state.hits.is_empty() {
            ui.colored_label(
                theme.text_secondary,
                if state.searched_messages {
                    "Nothing here matched."
                } else {
                    "…"
                },
            );
            if state.searched_messages {
                ui.colored_label(theme.text_muted, egui::RichText::new(ONLY_HERE).small());
            }
            return;
        }
        let n = state.hits.len();
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(format!(
                "{n} result{} in what this client holds",
                if n == 1 { "" } else { "s" }
            ))
            .small(),
        )
        .on_hover_text(ONLY_HERE);
        ui.add_space(tokens::SPACING_XS);
        let chosen = self.pane(at).chosen;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for hit in &state.hits {
                    let row = sigil_ui::SearchHit {
                        label: &hit.label,
                        who: &hit.who,
                        text: &hit.text,
                        found: hit.found.clone(),
                        at: &sigil_ui::brief(hit.at, now),
                    };
                    let selected = chosen == Some((hit.channel, hit.seq));
                    if sigil_ui::search_hit(ui, &row, selected).clicked() {
                        self.choose_hit(at, hit.channel, hit.seq);
                    }
                }
            });
    }

    /// Go to a search result: open its conversation with the message in the
    /// window, and scroll to it once it is drawn. `ShowAt` rather than
    /// `Show`, which opens on the last page: a hit carried its sequence
    /// number and for a while nothing used it, so the result opened the
    /// conversation at the bottom with the message somewhere above.
    fn choose_hit(&mut self, at: &At, channel: [u8; 32], seq: u64) {
        self.send_as(Some(at), Cmd::ShowAt { channel, seq });
        let pane = self.pane(at);
        pane.jump = Some((channel, seq));
        pane.chosen = Some((channel, seq));
    }

    /// The search box over the conversation list, with its magnifier.
    ///
    /// Its own function because on a phone it is not always drawn: the
    /// heading's magnifier opens it there, and a row that is sometimes
    /// absent is easier to reason about as a thing than as a branch in the
    /// middle of the list.
    fn search_ui(&mut self, at: &At, state: &ChatState, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let form = sigil::Form::of(ui.ctx());
            let phone = form.is_phone();
            let control = form.button_size() + ui.spacing().item_spacing.x * 2.0;
            let width = ui.available_width() - if phone { 0.0 } else { control };
            // No label beside it. A search box is the one control everybody
            // recognises without being told, and the word is still on the
            // magnifier next to it -- which is a button, so it reaches the
            // accessibility tree where a placeholder would not. On a phone
            // the box is the whole width and the magnifier sits inside it,
            // at the right.
            let (field, slot) = if phone {
                let (field, slot) = sigil_ui::field_with_slot(
                    ui,
                    &mut self.panes.entry(at.clone()).or_default().searching,
                    "Search chats",
                    width,
                    tokens::FIELD_LG,
                    form.button_size(),
                );
                (field, Some(slot))
            } else {
                (
                    sigil_ui::field(
                        ui,
                        &mut self.panes.entry(at.clone()).or_default().searching,
                        "Search chats",
                        width,
                    ),
                    None,
                )
            };
            if std::mem::take(&mut self.pane(at).search_focus) {
                field.request_focus();
            }
            if field.changed() {
                let query = self.pane(at).searching.clone();
                self.pane(at).chosen = None;
                self.send_as(Some(at), Cmd::Search(query));
            }
            let searching = !self.pane(at).searching.is_empty();
            // Enter chooses the newest result, as pressing it would; Escape
            // clears the search. Escape has already taken the focus by the
            // time this pass runs, so the box is asked whether it *had* it.
            let entered =
                searching && field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let escaped = searching
                && ui.memory(|m| m.had_focus_last_frame(field.id))
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if entered && let Some(hit) = state.hits.first() {
                self.choose_hit(at, hit.channel, hit.seq);
            }
            // The control tells you what it will do: clear the search while
            // there is one, and otherwise say what the box is for.
            let control = |ui: &mut egui::Ui| {
                if searching {
                    sigil_ui::icon_button(ui, sigil_ui::Icon::Close).clicked()
                } else {
                    sigil_ui::icon_button(ui, sigil_ui::Icon::Search).clicked()
                }
            };
            let pressed = match slot {
                Some(slot) => sigil_ui::in_slot(ui, slot, control),
                None => control(ui),
            };
            if searching {
                if pressed || escaped {
                    self.pane(at).searching.clear();
                    self.pane(at).chosen = None;
                    self.send_as(Some(at), Cmd::Search(String::new()));
                }
            } else if pressed {
                // Focuses the box rather than doing nothing: it is beside a
                // field and the obvious thing to press first.
                field.request_focus();
            }
        });
    }

    /// The conversation list.
    /// `identity`: the identity sits in the heading row -- one pane, nothing
    /// open, no bar for it to be in. In the wide layouts it is in the bar
    /// over the transcript and this is false.
    fn list_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        identity: bool,
    ) {
        let now = self.now();
        // The heading carries what is done to the list, rather than each
        // having a row of its own below it. All icons: a word in a heading
        // row reads as part of the heading, not as a control.
        let phone = sigil::Form::of(ui.ctx()).is_phone();
        ui.horizontal(|ui| {
            // As tall as the bar over the transcript, so the two line up
            // across the divider.
            ui.set_min_height(tokens::AVATAR_MD);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if identity {
                    self.me_ui(ctx, at, state, ui, theme, false);
                    ui.add_space(tokens::SPACING_XS);
                }
                // **A phone's heading holds two controls, not four.** The
                // identity, a magnifier and a burger fit beside the word;
                // a fifth pushes the heading off its own row. So the
                // magnifier -- the one thing done to this list most often
                // -- keeps its place, and what is done *from* it goes
                // behind the burger.
                if phone {
                    // Three dots, not the hamburger: the hamburger is what
                    // hides the column beside a wide window, and one shape
                    // cannot mean both "a menu of this list's actions" and
                    // "put this list away".
                    let dots =
                        sigil_ui::icon_button_named(ui, sigil_ui::Icon::More, "More choices");
                    egui::Popup::menu(&dots).show(|ui| {
                        if sigil_ui::icon_item(ui, sigil_ui::Icon::Compose, "Write to somebody")
                            .clicked()
                        {
                            self.panes.entry(at.clone()).or_default().dialog =
                                Some(Dialog::Compose);
                            ui.close();
                        }
                        if sigil_ui::icon_item(ui, sigil_ui::Icon::Public, "Public channels")
                            .clicked()
                        {
                            ctx.navigator.push_here(Route::Directory);
                            ui.close();
                        }
                    });
                    // The magnifier is the way to the search card, and it
                    // opens on an empty box with the finger already in it:
                    // a search is a thing one goes to do, not a row the
                    // list carries about in case.
                    if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Search, "Find a chat")
                        .clicked()
                    {
                        let pane = self.panes.entry(at.clone()).or_default();
                        pane.searching.clear();
                        pane.chosen = None;
                        pane.search_focus = true;
                        self.send_as(Some(at), Cmd::Search(String::new()));
                        ctx.navigator.push_here(Route::Search);
                    }
                } else {
                    if sigil_ui::icon_button(ui, sigil_ui::Icon::Compose)
                        .on_hover_text("Write to somebody, or start a group or a channel")
                        .clicked()
                    {
                        self.panes.entry(at.clone()).or_default().dialog = Some(Dialog::Compose);
                    }
                    if sigil_ui::icon_button(ui, sigil_ui::Icon::Public)
                        .on_hover_text("Find a public channel")
                        .clicked()
                    {
                        ctx.navigator.push_here(Route::Directory);
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // Beside the heading, and it puts the column away. The
                    // control that brings it back is in the conversation's own
                    // bar, because a control inside the thing it hides is a
                    // control nobody can reach once they have used it. Not
                    // when the list is the whole window: there is no column
                    // to put away, and on a phone the heading row has no
                    // room for a button that does nothing.
                    if !self.single
                        && sigil_ui::icon_button_named(ui, sigil_ui::Icon::Menu, "Hide the chats")
                            .clicked()
                    {
                        self.columns_open = false;
                    }
                    ui.heading("Chats");
                });
            });
        });
        ui.add_space(tokens::SPACING_XS);

        if !phone {
            self.search_ui(at, state, ui);
            ui.add_space(tokens::SPACING_SM);
        }

        // A search replaces the list while there is one. The list is still
        // there underneath, and clearing the box brings it back. Not on a
        // phone: there the search is its own card, and what it found is
        // shown there.
        if !phone && !self.pane(at).searching.trim().is_empty() {
            self.hits_ui(at, state, ui, theme, now);
            return;
        }

        // **Empty and unasked are not the same screen.** An empty list means
        // either "you have no conversations" or "we have not been told yet",
        // and the first was being said during the second on every launch --
        // to somebody whose conversations were about to appear underneath it.
        if state.conversations.is_empty() && !state.synced {
            ui.add_space(tokens::SPACING_SM);
            ui.horizontal(|ui| {
                sigil_ui::working(ui);
                ui.colored_label(theme.text_secondary, "Loading your chats…");
            });
            return;
        }

        if state.conversations.is_empty() {
            // Both halves, every time: that it is empty, and what to do about
            // it. A bare "nothing here" leaves somebody hunting for a control.
            // The control is now a dialog, so the empty state opens it rather
            // than pointing at an icon and hoping it was found.
            ui.add_space(tokens::SPACING_SM);
            ui.colored_label(
                theme.text_secondary,
                "No conversations yet. Write to somebody by their key, or by name@domain.",
            );
            ui.add_space(tokens::SPACING_SM);
            if ui.button("Write to somebody").clicked() {
                self.panes.entry(at.clone()).or_default().dialog = Some(Dialog::Compose);
            }
            return;
        }

        // **Room for the scrollbar, whether or not it is showing.**
        //
        // egui's bars float: they allocate no width and draw *over* the last
        // ten pixels of whatever is there, which here is the time on every row
        // and the unread count beside it. The same treatment the transcript's
        // own bubbles get, for the same reason.
        //
        // Not, as this comment first claimed, to stop the rows shifting when
        // the pointer arrives: a floating bar takes no width, so nothing
        // moves, and the test written for that could not be made to fail.
        let bar = ui.spacing().scroll.bar_width;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width((ui.available_width() - bar).max(0.0));
                for convo in &state.conversations {
                    let id = bs58::encode(convo.channel).into_string();
                    let selected = state.open == Some(convo.channel);
                    let row = sigil_ui::ConversationRow {
                        id: &id,
                        label: &convo.label,
                        preview: convo.preview.as_deref().unwrap_or(""),
                        at: &convo
                            .at
                            .map(|t| sigil_ui::brief(t, now))
                            .unwrap_or_default(),
                        unread: convo.unread as u32,
                        public: convo.public,
                        group: convo.group,
                        waiting: convo.waiting,
                        typing: convo.typing,
                        mentioned: convo.mentioned > 0,
                        muted: ctx.accounts.quiet.is_muted(&at.1, &convo.channel),
                        presence: convo.peer.map(|peer| self.presence_of(state, &peer)),
                        verified: convo.peer.is_some_and(|p| state.verified.contains_key(&p)),
                    };
                    if sigil_ui::conversation_row(ui, &row, selected).clicked() {
                        self.send_as(Some(at), Cmd::Show(convo.channel));
                    }
                }
            });
    }

    /// The messages, and the box to write one in.
    fn transcript_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let now = self.now();

        if state.open.is_none() {
            // "Pick a conversation" beside a column that is not on screen is
            // an instruction to use something nobody can see. The list starts
            // away, so the empty pane offers it rather than assuming it.
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.35);
                if self.columns_open {
                    ui.colored_label(theme.text_secondary, "Pick a conversation.");
                } else {
                    ui.colored_label(theme.text_secondary, "Nothing open.");
                    ui.add_space(tokens::SPACING_SM);
                    if ui.button("Show chats").clicked() {
                        self.columns_open = true;
                    }
                }
            });
            return;
        }

        // The header is the bar above, drawn by whoever placed this pane;
        // what is happening in the conversation is said at the top of it --
        // except on a phone, where it is said at the bottom, above the box.
        // See the ring in `render_nav`: the end of a phone the hand is at.
        let public = state
            .conversations
            .iter()
            .find(|c| Some(c.channel) == state.open)
            .is_some_and(|c| c.public == Some(true));
        let phone = sigil::Form::of(ui.ctx()).is_phone();
        if !phone {
            if self.ringing_ui(ctx, at, state, ui, theme) {
                ui.add_space(tokens::SPACING_SM);
            }
            self.succeeded_ui(at, state, ui, theme);
            for seq in self.trouble_ui(&state.trouble_with, public, ui, theme) {
                self.send_as(Some(at), Cmd::Redact(seq));
            }
        }

        // The composer is laid out first, from the bottom, so the transcript
        // gets the remaining height rather than pushing it off the screen.
        let mut redact: Vec<u64> = Vec::new();
        egui::Panel::bottom("chat_composer")
            .frame(
                egui::Frame::NONE
                    .fill(theme.surface_primary)
                    // Room above it, and more of it than below. The rule the
                    // panel draws on its own top edge is a line between two
                    // things, and a line with the box against it reads as the
                    // box's own border rather than as the end of the
                    // transcript. Below it the window's own margin is already
                    // there, so this adds almost nothing: every pixel here is
                    // one the transcript does not get.
                    .inner_margin(egui::Margin {
                        top: tokens::SPACING_MD as i8,
                        bottom: tokens::SPACING_XS as i8,
                        ..Default::default()
                    }),
            )
            .show(ui, |ui| {
                // **A phone says it above the box**, in the composer's
                // own panel: one panel, and the same place on the screen
                // as a second one below the transcript would be.
                if phone {
                    if self.ringing_ui(ctx, at, state, ui, theme) {
                        ui.add_space(tokens::SPACING_SM);
                    }
                    self.succeeded_ui(at, state, ui, theme);
                    redact = self.trouble_ui(&state.trouble_with, public, ui, theme);
                }
                self.composer_ui(ctx, at, state, ui, theme)
            });
        for seq in std::mem::take(&mut redact) {
            self.send_as(Some(at), Cmd::Redact(seq));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                // **Inside** the scroll area, not around it. The margin has to
                // shrink the *content* while the bar stays against the pane
                // edge; put it on the outside and the bar moves in with the
                // text and clashes with it again.
                //
                // Wide enough for the bar at its widest: egui's scroll bars
                // float by default, which means they allocate no width and
                // draw *over* the last ten pixels of whatever is there. A
                // right-aligned bubble is exactly what is there, so one's own
                // messages sat under the scrollbar. Taken from the style
                // rather than written as a number, so it follows if the style
                // changes.
                let bar = ui.spacing().scroll.bar_width;
                // **Inside** the scroll area, not around it. The margin has to
                // shrink the *content* while the bar stays against the pane
                // edge; put it on the outside and the bar moves in with the
                // text and clashes with it again.
                //
                // Wide enough for the bar at its widest: egui's scroll bars
                // float by default, which means they allocate no width and
                // draw *over* the last ten pixels of whatever is there. A
                // right-aligned bubble is exactly what is there, so one's own
                // messages sat under the scrollbar.
                let margin = egui::Margin {
                    right: (bar + tokens::SPACING_XS) as i8,
                    // And room under the last message, so the transcript ends
                    // before the rule does rather than against it.
                    bottom: tokens::SPACING_MD as i8,
                    ..Default::default()
                };

                // **Keep the reader where they were when a page arrives above
                // them.**
                //
                // Earlier messages are asked for the moment the control
                // reaches the screen, so this happens by scrolling and not by
                // choosing -- and a scroll offset is measured from the top,
                // which means everything the reader was looking at moves down
                // by the height of the page. Measured on a real conversation:
                // the content went from 5,762 to 10,859 pixels while the
                // offset stayed at 4,042. Anchored to the **bottom** instead,
                // because that is the end the new content is not arriving at.
                //
                // Decided by `earlier` falling rather than by having asked:
                // that is prepending and nothing else -- a message arriving at
                // the bottom does not change it, nor does a picture finding
                // its size -- so it holds for every helping a page arrives in
                // and needs no guess about which is the last.
                let paged =
                    self.pane(at).saw.0 == state.open && state.earlier < self.pane(at).saw.1;

                // Asked for before the pass that answers it: the end of the
                // content is only a position once the content is drawn, so
                // the control below sets this and the next pass acts on it.
                let out = egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        egui::Frame::NONE.inner_margin(margin).show(ui, |ui| {
                            self.messages_ui(at, state, ui, theme, now);
                        });
                    });
                self.way_back_ui(state, &out, ui, theme);

                // **The pass that discovered it is thrown away.**
                //
                // The new height is only known once the pass that drew it is
                // over, so correcting the offset afterwards puts the right
                // number in the right place a frame too late -- and that frame
                // is drawn. For one sixtieth of a second the transcript sat
                // five thousand pixels from where it belonged and then snapped
                // back: arithmetically perfect, and visibly a jump. Somebody
                // reading it called it "subtle, but it still jumps".
                //
                // `request_discard` is egui's own answer to needing a pass to
                // find out how big something is: this one is dropped and run
                // again immediately, with the offset already corrected, and
                // only the second one is shown. The events came with the first
                // pass and are gone by the second, so nothing anybody pressed
                // happens twice.
                //
                // A page arrives rarely -- once per fifty messages, by
                // scrolling to the top of them -- so the extra pass is not a
                // cost anybody will meet often.
                if paged {
                    let (content, offset) = (out.content_size.y, out.state.offset.y);
                    let (was_content, was_offset) = self.pane(at).scrolled;
                    let held = (content - (was_content - was_offset)).max(0.0);
                    let mut moved = out.state;
                    moved.offset.y = held;
                    moved.store(ui.ctx(), out.id);
                    ui.ctx().request_discard("a page arrived above the reader");
                    ui.ctx().request_repaint();
                    // Remembered as corrected, or the next helping of the same
                    // page would anchor against a position that no longer
                    // exists.
                    self.pane(at).scrolled = (content, held);
                    let _ = offset;
                }
                if paged {
                    self.pane(at).asking = false;
                }
                // A different conversation is not this one's page arriving,
                // and an ask that was never answered must not outlive the
                // conversation it was made in -- otherwise coming back to one
                // leaves a transcript that will never fetch its own history.
                //
                // `is_some`, because the first pass of a pane has seen no
                // conversation at all -- and reading that as "a different one"
                // cleared the ask that had just been made, which asked again
                // on the next pass. Exactly twice, which is the sort of
                // "nearly right" a counting test is for.
                if self.pane(at).saw.0.is_some() && self.pane(at).saw.0 != state.open {
                    self.pane(at).asking = false;
                    // A remembered height is about one message in one
                    // conversation, and `seq` starts again in the next.
                    self.pane(at).tall.clear();
                    self.pane(at).marked = None;
                }
                self.pane(at).saw = (state.open, state.earlier);
                if !paged {
                    self.pane(at).scrolled = (out.content_size.y, out.state.offset.y);
                }
            });
    }

    /// **Back to the newest message.** A transcript sticks to the bottom
    /// while the reader is at it, and stays where it is put once they have
    /// scrolled up -- which is right, and leaves a phone dragging a
    /// conversation's whole history back to reach what was just said. Every
    /// messenger answers this with one control, and it is the one thing on
    /// the transcript that is *not* about a message.
    ///
    /// Only while it does something: at the bottom there is nothing to go
    /// back to, and a control that is always there is a control nobody
    /// reads. `SLACK` is a message's height, so it neither flickers at the
    /// foot nor hides while a screen of conversation is still below.
    ///
    /// It says how many arrived while the reader was away, when any did:
    /// the number is the reason to press it.
    fn way_back_ui(
        &mut self,
        state: &ChatState,
        out: &egui::scroll_area::ScrollAreaOutput<()>,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        /// How far from the foot counts as away from it.
        const SLACK: f32 = 48.0;
        let below = out.content_size.y - (out.state.offset.y + out.inner_rect.height());
        if below <= SLACK {
            return;
        }
        let unread = state
            .conversations
            .iter()
            .find(|c| Some(c.channel) == state.open)
            .map(|c| c.unread)
            .unwrap_or(0);
        // **One name, with the count after it.** The word is what anything
        // that cannot see the shape is given, and a name that changes with
        // the count is one nobody -- and no test -- can ask for by name.
        let word = match unread {
            0 => "Go to the latest".to_string(),
            1 => "Go to the latest — 1 new".to_string(),
            n => format!("Go to the latest — {n} new"),
        };
        // Over the foot of the transcript, from the right, and inside it:
        // the composer is below, and a control over the composer is one
        // that takes a press meant for the box.
        let side = tokens::BUTTON_LG + tokens::SPACING_SM;
        let spot = egui::Rect::from_min_max(
            out.inner_rect.right_bottom() - egui::vec2(side + tokens::SPACING_MD, side),
            out.inner_rect.right_bottom() - egui::vec2(tokens::SPACING_MD, 0.0),
        );
        // **A layer of its own, not a scope in the transcript's ui.** A
        // scope advances the parent's cursor to the rectangle it was given,
        // so the pill -- drawn at the foot of the transcript -- pushed the
        // composer down by its own height whenever it appeared, and the
        // transcript's height changed under the reader. An area floats.
        egui::Area::new(ui.id().with("way_back"))
            .order(egui::Order::Foreground)
            .fixed_pos(spot.min)
            .constrain_to(out.inner_rect)
            .show(ui.ctx(), |ui| {
                ui.set_max_size(spot.size());
                // **It floats over the transcript, so it must not read as
                // part of it.** On the phone it lands over the last bubble
                // -- seen on the device, sitting on a message's time -- and
                // an elevated ground is the bubbles' own. The pane's ground
                // with a rule around it is a thing *over* the conversation.
                egui::Frame::NONE
                    .fill(theme.surface_primary)
                    .stroke(egui::Stroke::new(1.0, theme.border_strong))
                    .corner_radius(side / 2.0)
                    .inner_margin(egui::Margin::symmetric(tokens::SPACING_XS as i8, 0))
                    .show(ui, |ui| {
                        if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Chevron, &word).clicked()
                        {
                            // The offset put where the content ends -- the
                            // same way a page arriving above the reader is
                            // corrected. Scrolling from inside the content
                            // is the other way round and a pass late: the
                            // content is drawn before this is pressed.
                            let mut moved = out.state;
                            moved.offset.y =
                                (out.content_size.y - out.inner_rect.height()).max(0.0);
                            moved.store(ui.ctx(), out.id);
                            ui.ctx().request_repaint();
                        }
                        if unread > 0 {
                            ui.colored_label(
                                theme.accent,
                                egui::RichText::new(unread.to_string()).small().strong(),
                            );
                        }
                    });
            });
    }

    /// What is wrong with this conversation, said in words.
    ///
    /// Each of these is a **different thing to do**, so none of them may be
    /// collapsed into the others. The pair most easily confused is the pair
    /// that matters most: an unreadable entry is one whose key may still
    /// arrive, so waiting is right; a lost one is under a superseded epoch and
    /// is gone, so waiting is forever.
    ///
    /// Returns the unreadable messages somebody asked to delete, if they did:
    /// this draws with `&self`, and the deleting is the caller's.
    fn trouble_ui(
        &self,
        trouble: &Trouble,
        public: bool,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) -> Vec<u64> {
        let mut delete = Vec::new();
        if trouble.is_clear() {
            return delete;
        }
        // A function rather than a closure over `ui`, so the button below can
        // borrow `ui` too.
        fn say(ui: &mut egui::Ui, colour: egui::Color32, text: String) {
            ui.colored_label(colour, text);
        }
        if trouble.chain_apart {
            // SIP-43 §The heads by position. Said, not acted on: there is
            // nothing here to press. What it means is that two things wrote
            // under this device's key -- a store rolled back by a restore or
            // a copied file, or the key itself on a second machine -- and
            // the reader is the only one who can know which.
            say(
                ui,
                theme.warning,
                "The exchange's record of what this device wrote here does not match this \
                 device's own. Something else has written here under this device's key, or \
                 this machine's history was rolled back."
                    .to_string(),
            );
        }
        if let Some(epoch) = trouble.no_key {
            // SIP-17's stranded member: every entry fetches and none of them
            // open. Without this the conversation simply reads as empty, which
            // is indistinguishable from nobody having written.
            say(
                ui,
                theme.destructive,
                format!(
                    "You hold no key for this conversation (epoch {epoch}). \
                     An admin has to hand you one before anything here can be read."
                ),
            );
        }
        if trouble.unreadable > 0 {
            // **Which kind of unreadable.** The fold says "well formed and not
            // understood, or sealed under a key we lack -- either way it
            // happened", and the two need different words. In a private
            // channel it is nearly always the key, and waiting is right. In a
            // public channel nothing is ever sealed and no key will ever
            // arrive: the entry is one this version could not make sense of,
            // and telling somebody to wait for a key would have them waiting
            // for ever. Found by exactly that: two pictures posted with a
            // preview over the protocol's limit, reported as "their key may
            // still arrive" in a channel that has no keys.
            say(
                ui,
                theme.warning,
                match (public, trouble.unreadable) {
                    (false, 1) => {
                        "1 message here has not been opened yet — its key may still arrive."
                            .to_string()
                    }
                    (false, n) => format!(
                        "{n} messages here have not been opened yet — their key may still arrive."
                    ),
                    (true, 1) => {
                        "1 message here could not be read by this version of sigil.".to_string()
                    }
                    (true, n) => {
                        format!("{n} messages here could not be read by this version of sigil.")
                    }
                },
            );
            // **The way to take them down**, for whoever may. A message that
            // will never open is not always waiting for a key -- two pictures
            // with previews over SIP-18's cap sat in a public channel as two
            // unreadable messages for everybody -- and the one thing to do
            // with one is delete it. Nothing draws it as a bubble, so nothing
            // else offers the control; it goes on the notice, which is the
            // one place its author is looking.
            if !trouble.redactable.is_empty() {
                let n = trouble.redactable.len();
                let label = if n == trouble.unreadable {
                    if n == 1 {
                        "Delete it".to_string()
                    } else {
                        "Delete them".to_string()
                    }
                } else {
                    format!("Delete the {n} of them that are yours")
                };
                if ui
                    .button(label)
                    .on_hover_text(
                        "Remove them from the conversation for everybody. Anything they \
                         carried that this client never opened cannot be detached with \
                         them, and stays at the exchange until its retention window \
                         closes.",
                    )
                    .clicked()
                {
                    delete = trouble.redactable.clone();
                }
            }
        }
        if trouble.lost > 0 {
            say(
                ui,
                theme.destructive,
                match trouble.lost {
                    1 => "1 message here can never be read: its key is gone.".to_string(),
                    n => format!("{n} messages here can never be read: their key is gone."),
                },
            );
        }
        if trouble.gap {
            say(
                ui,
                theme.text_secondary,
                "Older messages have passed this channel's retention window and are gone."
                    .to_string(),
            );
        }
        if trouble.restarted {
            say(
                ui,
                theme.warning,
                "This conversation was destroyed and started again under the same name. \
                 Nothing above is related to what follows."
                    .to_string(),
            );
        }
        if trouble.forged > 0 {
            // Never shown as messages — the whole point is that nobody
            // vouched for them — but said, because something arrived claiming
            // to be from somebody here and the signature did not hold. A
            // client that silently dropped them would leave the only party
            // who could notice unable to.
            say(
                ui,
                theme.destructive,
                match trouble.forged {
                    1 => "1 entry claimed to be from somebody here and was not signed by \
                          them. It is not shown."
                        .to_string(),
                    n => format!(
                        "{n} entries claimed to be from somebody here and were not signed \
                         by them. They are not shown."
                    ),
                },
            );
        }
        delete
    }

    /// One membership or metadata change, centred in the transcript.
    ///
    /// **A key is always reachable from a name.** These name people, and a
    /// name is an assertion attested by nobody (SIP-21) — so the accounts the
    /// exchange actually recorded are on the same line, one hover away.
    fn event_ui(&self, event: &session::Happened, ui: &mut egui::Ui) {
        let row = sigil_ui::system_line(ui, &event.said);
        let mut hover = format!("{}\nby {}", event.subject, event.actor);
        if let Some(caveat) = event.caveat {
            hover = format!("{caveat}\n\n{hover}");
        }
        row.on_hover_text(hover);
    }

    /// The messages themselves, with day separators, grouping and the divider.
    fn messages_ui(
        &mut self,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        now: u64,
    ) {
        if state.lines.is_empty() && state.events.is_empty() {
            ui.add_space(tokens::SPACING_XL);
            ui.vertical_centered(|ui| {
                // **"Nothing here yet" is a claim about the conversation**,
                // and it was being made about every conversation that had not
                // been fetched -- which is all of them, for as long as the
                // exchange took to answer. What this machine holds is drawn
                // the moment a conversation is opened, so an empty pane and a
                // pending fetch together mean the answer is still coming.
                if state.loading {
                    sigil_ui::working(ui);
                    ui.add_space(tokens::SPACING_SM);
                    ui.colored_label(theme.text_secondary, "Loading this conversation…");
                } else {
                    ui.colored_label(theme.text_secondary, "Nothing here yet.");
                }
            });
            return;
        }

        // What was done to a message, collected rather than acted on inside the
        // loop: acting there would need `&mut self` while `state` is borrowed
        // from it, and a frame-local queue is the shape the rest of the host
        // uses anyway.
        // The way back into the rest of the conversation.
        //
        // A conversation opens on its last page rather than on all of it --
        // see `session::PAGE`. So the top of the transcript is a door and not
        // the beginning, and it says which: a reader who cannot tell the two
        // apart believes a channel started where their screen does.
        if state.earlier > 0 {
            ui.add_space(tokens::SPACING_SM);
            ui.vertical_centered(|ui| {
                let more = ui.button(match state.earlier {
                    1 => "1 earlier message".to_string(),
                    n => format!("{n} earlier messages"),
                });
                // Asked for by *approaching* the top as well as by pressing
                // it. Scrolling is how anybody actually gets there, and a
                // control that only answers a click makes somebody hunt for a
                // button they have already scrolled past.
                //
                // A screen early, so the page is on its way before the reader
                // arrives at the end of what they have: waiting until the
                // control is visible means reaching the top of the transcript
                // and stopping there while the exchange is asked, which is the
                // pause that made this read as a wall rather than as more
                // conversation.
                //
                // **From where the transcript is scrolled, not from whether
                // this button can be seen.** On the pass that first draws a
                // conversation there is no clip rectangle worth the name --
                // everything is "visible" -- so a rule about visibility asks
                // for the previous page the instant a conversation is opened,
                // which is the whole of what paging exists to avoid. The
                // scroll position is measured and says nothing until there is
                // something to measure.
                let (content, offset) = self.pane(at).scrolled;
                let view = ui.clip_rect().height();
                let near_top = content > 0.0 && (content <= view || offset <= view);
                // **Once, and not again until it has arrived.** Being on
                // screen is a state and not an event: without the guard this
                // asks on every frame it can see itself.
                if (more.clicked() || near_top) && !self.pane(at).asking {
                    self.pane(at).asking = true;
                    self.send_as(Some(at), Cmd::Earlier);
                }
            });
            ui.add_space(tokens::SPACING_SM);
        }

        let mut acted: Option<(&Line, sigil_ui::BubbleAction)> = None;
        // Once per pass, not per message: the counts are sorted to find it.
        let frequent = self.frequent.top(sigil_ui::emoji::FREQUENT);
        // Whether this conversation is already somebody's direct message,
        // in which case "direct message" on their bubbles would open the
        // conversation it is in.
        let in_direct = state
            .conversations
            .iter()
            .any(|c| Some(c.channel) == state.open && c.peer.is_some());
        // **The videos, before the bubbles.** Players for messages no longer
        // on screen are dropped -- which stops them -- and a video whose
        // bytes have just arrived after it was pressed is started, in the
        // viewer. The pictures are the viewer's to upload: a bubble shows
        // the thumbnail whatever the player is doing.
        {
            let ctx = ui.ctx().clone();
            let here: HashSet<&str> = state
                .lines
                .iter()
                .flat_map(|l| l.attachments.iter().map(|a| a.id.as_str()))
                .collect();
            self.pane(at)
                .players
                .retain(|id, _| here.contains(id.as_str()));
            self.pane(at)
                .notes
                .retain(|id, _| here.contains(id.as_str()));
            let arrived: Vec<(String, u8, std::sync::Arc<[u8]>)> = state
                .lines
                .iter()
                .flat_map(|l| l.attachments.iter())
                .filter(|a| self.pane(at).play_when_fetched.contains(&a.id))
                .filter_map(|a| a.bytes.clone().map(|b| (a.id.clone(), a.kind, b)))
                .collect();
            for (id, kind, bytes) in arrived {
                if kind == sigil_ui::attachment::VOICE {
                    self.start_note(at, &ctx, &id, &bytes);
                    continue;
                }
                self.start_video(at, &ctx, &id, bytes);
                if let Some((seq, index)) = self.pane(at).open_when_fetched.remove(&id) {
                    self.pane(at).viewing = Some((seq, index));
                    self.pane(at).look = Look::default();
                }
            }
        }
        // Where the message somebody asked to go to landed this pass, drawn or
        // reserved. See the end of the loop. Only when it is in *this*
        // conversation: the same number names a different message anywhere
        // else.
        let jump = self
            .pane(at)
            .jump
            .filter(|(channel, _)| Some(*channel) == state.open)
            .map(|(_, seq)| seq);
        let mut landed: Option<egui::Rect> = None;
        let marked = self.pane(at).marked;
        let mut previous_day: Option<String> = None;
        let mut previous_author: Option<PubKey> = None;
        let mut previous_at: u64 = 0;

        // What happened to the channel, in the order it happened relative to
        // what was said. **Both sequences come from the exchange**, so one
        // pass over the events, advanced as the messages go by, puts each in
        // its place -- rather than a merged list that would have to copy every
        // message to build.
        let mut events = state.events.iter().peekable();

        // **What was just sent, before the exchange has answered.** A press
        // on Send takes the words out of the box and hands them to the
        // session, which posts them and only then publishes a transcript
        // with them in it -- a round trip during which the message is
        // nowhere at all, and on a phone with a slow link that reads as a
        // press that did nothing. `in_flight` already holds exactly what was
        // sent, for putting back if it fails; drawn here, it is the message
        // itself, without a tick, until the real one lands.
        //
        // Not an edit: a rewrite has a message of its own on screen already,
        // and drawing the new words under it would read as two messages.
        let echo: Vec<session::Line> =
            self.pane(at)
                .in_flight
                .iter()
                // **And the files.** A message that is only a picture had no
                // echo at all: the phone showed nothing whatever between the
                // press and the exchange's answer, which on an uplink and a
                // twenty-megabyte clip is a minute of a press that did nothing.
                .filter(|u| {
                    u.editing.is_none() && !(u.composing.trim().is_empty() && u.staged.is_empty())
                })
                .enumerate()
                .map(|(i, u)| session::Line {
                    seq: ECHO_SEQ + i as u64,
                    who: at.0,
                    name: None,
                    mine: true,
                    at: now,
                    text: u.composing.clone(),
                    redacted: false,
                    edited: false,
                    via: None,
                    reactions: Vec::new(),
                    reply_to: None,
                    receipt: None,
                    attachments: u
                        .staged
                        .iter()
                        .enumerate()
                        .map(|(k, f)| session::Attached {
                            kind: f.kind,
                            described: f.name.clone(),
                            // What is not known until the exchange has it: the
                            // size it will be stored as, the shape the decoder
                            // will say, and the blob's own name.
                            size: 0,
                            duration_ms: None,
                            shape: None,
                            // Read off the file when it is opened, which
                            // has not happened while it is on its way.
                            waveform: std::sync::Arc::from(Vec::new().into_boxed_slice()),
                            preview: f.preview.clone().unwrap_or_else(|| {
                                std::sync::Arc::from(Vec::new().into_boxed_slice())
                            }),
                            missing: false,
                            held: false,
                            bytes: None,
                            id: format!("sending-{}-{k}", u.token),
                        })
                        .collect(),
                    standing: session::Standing::Sound,
                    mentions: Vec::new(),
                    me_mentioned: false,
                })
                .collect();

        for line in state.lines.iter().chain(echo.iter()) {
            // Nothing is done *to* a message that is not there yet: its
            // sequence number is this client's invention, and a reaction or
            // a deletion aimed at it would name a message the exchange has
            // never heard of.
            let waiting = line.seq >= ECHO_SEQ;
            // Everything the exchange recorded before this message. `previous_
            // author` is cleared so the next message starts its own group: a
            // bubble grouped across a membership change reads as having been
            // said before it.
            while events.peek().is_some_and(|e| e.seq < line.seq) {
                let event = events.next().expect("peeked");
                self.event_ui(event, ui);
                previous_author = None;
            }

            // A separator on each new day, and the year on anything from
            // another one -- a bare date is a trap on old history.
            let day = sigil_ui::day_of(line.at);
            if day != previous_day {
                sigil_ui::day_separator(ui, &sigil_ui::day_label(line.at, now));
                previous_day = day;
                // A new day always starts a new group, however soon after.
                previous_author = None;
            }

            // The unread divider is **frozen** where it was on opening.
            // Reading advances the read mark, so one that tracked it would
            // vanish exactly when somebody wanted to see where they had got to.
            if state.divider == Some(line.seq) {
                sigil_ui::unread_divider(ui, state.unread_on_open);
                previous_author = None;
            }

            // Grouped when the same person said it recently. Five minutes,
            // because a reply an hour later is a new thought and should carry
            // its own time and name.
            let grouped = previous_author == Some(line.who)
                && line.at.saturating_sub(previous_at) < 300
                && state.divider != Some(line.seq);

            // **Reserved rather than drawn, when nobody can see it.**
            //
            // Everything in the window is laid out every frame: 51 µs a
            // message, measured, which is a 60 Hz frame's whole budget by four
            // hundred of them. A row well outside the viewport is given the
            // height it drew to last time and nothing else.
            //
            // The promise is the height. If a reserved row would have drawn
            // taller or shorter, the content above the reader changes size and
            // the transcript jumps under them -- the exact fault that
            // `request_discard` and the anchoring above exist to prevent. So a
            // height is only reused when the **shape** it was measured at is
            // unchanged (see `shape_of`) and the pane is still the same width.
            //
            // **A message carrying a file is never reserved.** A picture's
            // height moves as it loads -- preview, then full, then a remembered
            // natural size -- and that is precisely the row whose promise could
            // not be kept. They are a minority of messages and the whole of the
            // risk.
            let shape = shape_of(line, grouped);
            let width = ui.available_width();
            let known = (line.attachments.is_empty())
                .then(|| self.pane(at).tall.get(&line.seq).copied())
                .flatten()
                .filter(|(was, at_width, _)| *was == shape && *at_width == width)
                .map(|(_, _, tall)| tall);
            // A screen either side, so scrolling always arrives at rows that
            // have already been drawn rather than at reserved space.
            let near = ui
                .clip_rect()
                .expand2(egui::vec2(0.0, ui.clip_rect().height()));
            let top = ui.cursor().top();
            if let Some(tall) = known
                && (top + tall < near.top() || top > near.bottom())
            {
                let (_, rect) = ui.allocate_space(egui::vec2(width, tall));
                if jump == Some(line.seq) {
                    landed = Some(rect);
                }
                previous_author = Some(line.who);
                previous_at = line.at;
                continue;
            }

            let key = line.who.to_string();
            let title = state.people.get(&line.who).and_then(|p| p.title.as_deref());
            let pane = self.panes.entry(at.clone()).or_default();
            let files: Vec<sigil_ui::Attachment<'_>> = line
                .attachments
                .iter()
                .map(|a| sigil_ui::Attachment {
                    kind: a.kind,
                    described: &a.described,
                    preview: &a.preview,
                    bytes: a.bytes.as_ref(),
                    missing: a.missing,
                    held: a.held,
                    size: a.size,
                    id: &a.id,
                    video: (a.kind == sigil_ui::attachment::VIDEO)
                        .then(|| video_view(pane, a, sigil_ui::video::Place::Bubble)),
                    voice: (a.kind == sigil_ui::attachment::VOICE)
                        .then(|| voice_view(pane, a))
                        .flatten(),
                    // Still going up: this line is one of ours that the
                    // exchange has not answered about yet.
                    sending: line.seq >= ECHO_SEQ,
                    waveform: &a.waveform,
                    duration_ms: a.duration_ms,
                })
                .collect();
            // The keys as text, owned here, so the chips can borrow them.
            let mention_keys: Vec<String> =
                line.mentions.iter().map(|m| m.key.to_string()).collect();
            let mentioned: Vec<sigil_ui::message::Mentioned<'_>> = line
                .mentions
                .iter()
                .zip(&mention_keys)
                .map(|(m, key)| sigil_ui::message::Mentioned {
                    label: &m.label,
                    key,
                })
                .collect();
            let bubble = sigil_ui::Bubble {
                id: egui::Id::new(("message", at, line.seq)),
                key: &key,
                name: line.name.as_deref(),
                title,
                text: &line.text,
                at: &sigil_ui::clock(line.at),
                mine: line.mine,
                grouped,
                edited: line.edited,
                via: line.via.as_deref(),
                redacted: line.redacted,
                reply_to: line.reply_to.as_ref().map(|q| sigil_ui::Quote {
                    seq: q.seq,
                    who: &q.who,
                    said: &q.said,
                    preview: q.preview.as_ref().map(thumb),
                }),
                reactions: &line.reactions,
                frequent: &frequent,
                receipt: line.receipt.map(|r| match r {
                    Receipt::Sent => sigil_ui::Receipt::Sent,
                    Receipt::Delivered => sigil_ui::Receipt::Delivered,
                    Receipt::Read => sigil_ui::Receipt::Read,
                }),
                attachments: &files,
                standing: line.standing.word().zip(line.standing.means()),
                alarming: line.standing == session::Standing::Fork,
                direct: !line.mine && !in_direct,
                mentions: &mentioned,
                mentions_me: line.me_mentioned,
                verified: !line.mine && state.verified.contains_key(&line.who),
                editable: line.mine && session::rewritable(line.at, now),
            };
            // Measured as it is drawn, so the next frame can reserve it.
            DREW.with(|n| n.set(n.get() + 1));
            // Reserved before the bubble and painted after it, so the wash
            // a landed message wears goes *under* the bubble.
            let wash = marked
                .filter(|(seq, _)| *seq == line.seq)
                .map(|(_, since)| (ui.painter().add(egui::Shape::Noop), since));
            let drawn = ui.scope(|ui| sigil_ui::bubble(ui, &bubble));
            let did = drawn.inner;
            let rect = drawn.response.rect;
            // `files` borrows the pane's players for the frames; what
            // follows wants the pane mutably.
            drop(files);
            if jump == Some(line.seq) {
                landed = Some(rect);
            }
            if let Some((shape, since)) = wash {
                let left = wash_left(since, ui.input(|i| i.time));
                ui.painter().set(
                    shape,
                    egui::epaint::RectShape::filled(
                        rect.expand(tokens::SPACING_XS),
                        tokens::RADIUS_MD,
                        theme.accent.gamma_multiply(WASH * left),
                    ),
                );
                if left > 0.0 {
                    ui.ctx().request_repaint();
                } else {
                    self.pane(at).marked = None;
                }
            }
            self.pane(at)
                .tall
                .insert(line.seq, (shape, width, rect.height()));
            if !did.is_none() && !waiting {
                acted = Some((line, did));
            }

            previous_author = Some(line.who);
            previous_at = line.at;
        }

        // And anything after the last message -- somebody removed from a quiet
        // channel would otherwise leave no trace at all.
        for event in events {
            self.event_ui(event, ui);
        }

        // **Going to a message somebody asked for.**
        //
        // A quote was pressed. If the message it quotes was laid out this pass
        // -- drawn, or reserved at the height it drew to last time, either is
        // a rectangle -- the transcript scrolls to it and the ask is done. If
        // it was not, it is on a page that has not been fetched: the window
        // opens on the last page, and a reply can point anywhere before it.
        // So the previous page is asked for, the ask is kept, and the next
        // pass looks again. `asking` is the same guard the top-of-transcript
        // control uses, so this cannot ask twice for one page.
        //
        // A message that is not here and has no page to arrive on is let go
        // of: nothing more can be done, and an ask that never resolves would
        // fetch every page of the channel.
        if let Some(target) = jump {
            if let Some(rect) = landed {
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
                ui.ctx().request_repaint();
                let now = ui.input(|i| i.time);
                let pane = self.pane(at);
                pane.jump = None;
                pane.marked = Some((target, now));
            } else if state.lines.first().is_some_and(|l| l.seq > target) && state.earlier > 0 {
                if !self.pane(at).asking {
                    self.pane(at).asking = true;
                    self.send_as(Some(at), Cmd::Earlier);
                }
            } else {
                self.pane(at).jump = None;
            }
        }

        if state.typing {
            ui.add_space(tokens::SPACING_SM);
            ui.colored_label(theme.text_muted, "typing…");
        }

        // Where to forward a file to. A list rather than a key field: the
        // destination is always somewhere you are already in.
        if let Some((seq, index)) = self.pane(at).forwarding {
            ui.add_space(tokens::SPACING_SM);
            egui::Frame::NONE
                .fill(theme.surface_elevated)
                .corner_radius(tokens::RADIUS_MD)
                .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8))
                .show(ui, |ui| {
                    ui.label("Forward to");
                    // Said before the click, not after: forwarding hands over
                    // the key to the file, and everybody in the destination can
                    // then open it.
                    ui.colored_label(
                        theme.warning,
                        egui::RichText::new(
                            "Whoever is there will be able to open it — the key travels \
                             inside the message.",
                        )
                        .small(),
                    );
                    for convo in &state.conversations {
                        if Some(convo.channel) == state.open {
                            continue;
                        }
                        if ui.selectable_label(false, &convo.label).clicked() {
                            self.pane(at).forwarding = None;
                            self.send_as(
                                Some(at),
                                Cmd::Forward {
                                    seq,
                                    index,
                                    to: convo.channel,
                                },
                            );
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.pane(at).forwarding = None;
                    }
                });
        }

        if let Some((line, did)) = acted {
            let (seq, who) = (line.seq, line.who);
            if let Some(emoji) = did.react {
                // Counted when it is *sent*, not when it is taken back: the
                // session toggles, so whether this press adds is read from
                // the line — ours already there means this removes it.
                let ours = line.reactions.iter().any(|r| r.ours && r.emoji == emoji);
                if !ours {
                    self.frequent.bump(&emoji);
                }
                self.send_as(Some(at), Cmd::React { target: seq, emoji });
            }
            if did.reply {
                self.pane(at).reply_to(seq);
            }
            if did.edit {
                self.pane(at).rewrite(line);
            }
            if did.redact {
                self.send_as(Some(at), Cmd::Redact(seq));
            }
            if did.copy_key {
                ui.ctx().copy_text(who.to_string());
            }
            if did.direct {
                // The one with them, whether it exists yet or not: a direct
                // message's channel is the pair's, so opening it a second
                // time is switching to it.
                self.send_as(Some(at), Cmd::OpenDm(who));
            }
            if let Some(m) = &did.mentioned {
                use sigil_ui::message::MentionDo;
                match (m.what, m.key.parse::<PubKey>()) {
                    (MentionDo::Direct, Ok(key)) => self.send_as(Some(at), Cmd::OpenDm(key)),
                    (MentionDo::CopyKey, _) => ui.ctx().copy_text(m.key.clone()),
                    (MentionDo::Verify, Ok(key)) => {
                        self.pane(at).dialog = Some(Dialog::Verify(key));
                        self.send_as(Some(at), Cmd::Attested(key));
                    }
                    (MentionDo::Mention, Ok(key)) => {
                        // Into the box, the way the picker puts one: the
                        // name as text, and the key remembered for it.
                        let pane = self.pane(at);
                        if !pane.composing.is_empty() && !pane.composing.ends_with(' ') {
                            pane.composing.push(' ');
                        }
                        pane.composing.push('@');
                        pane.composing.push_str(&m.label);
                        pane.composing.push(' ');
                        pane.mentions.push((m.label.clone(), key));
                    }
                    (_, Err(_)) => {}
                }
            }
            if let Some(index) = did.forward {
                self.pane(at).forwarding = Some((seq, index));
            }
            if did.verify {
                self.pane(at).dialog = Some(Dialog::Verify(line.who));
                self.send_as(Some(at), Cmd::Attested(line.who));
            }
            if did.report {
                let pane = self.pane(at);
                pane.report_note.clear();
                pane.report_reason = 1;
                pane.dialog = Some(Dialog::Report { target: seq });
            }
            if let (Some(target), Some(channel)) = (did.jump, state.open) {
                self.pane(at).jump = Some((channel, target));
            }
            if did.retry {
                self.send_as(Some(at), Cmd::Refetch);
            }
            if let Some(index) = did.fetch {
                self.send_as(Some(at), Cmd::Fetch { seq, index });
            }
            if let Some((index, done)) = did.video
                && let Some(a) = state
                    .lines
                    .iter()
                    .find(|l| l.seq == seq)
                    .and_then(|l| l.attachments.get(index))
            {
                let ctx = ui.ctx().clone();
                // A bubble has one control, and it opens the viewer: a
                // video plays there and nowhere else.
                if done.open {
                    // Into the viewer, playing. Not here yet: asked for,
                    // and the viewer opens on it when it arrives.
                    if let Some(bytes) = a.bytes.clone() {
                        if !self.pane(at).players.contains_key(&a.id) {
                            self.start_video(at, &ctx, &a.id, bytes);
                        } else {
                            self.pane(at).players[&a.id].player.play();
                        }
                        self.pane(at).viewing = Some((seq, index));
                        self.pane(at).look = Look::default();
                    } else {
                        self.pane(at).play_when_fetched.insert(a.id.clone());
                        self.pane(at)
                            .open_when_fetched
                            .insert(a.id.clone(), (seq, index));
                        self.send_as(Some(at), Cmd::Fetch { seq, index });
                    }
                }
            }
            // **A voice note plays in the bubble.** There is nothing to
            // look at, so there is nowhere to go: the control, the shape
            // and the clock are the whole of it, where the message is.
            if let Some(index) = did.play
                && let Some(a) = state
                    .lines
                    .iter()
                    .find(|l| l.seq == seq)
                    .and_then(|l| l.attachments.get(index))
            {
                let ctx = ui.ctx().clone();
                if let Some(note) = self.pane(at).notes.get(&a.id) {
                    note.toggle();
                } else if let Some(bytes) = a.bytes.clone() {
                    self.start_note(at, &ctx, &a.id, &bytes);
                } else if !self.pane(at).unplayable.contains_key(&a.id) {
                    // Not here: a note is not fetched for being scrolled
                    // past. Asked for now, and played when it lands.
                    self.pane(at).play_when_fetched.insert(a.id.clone());
                    self.send_as(Some(at), Cmd::Fetch { seq, index });
                }
            }
            if let Some((index, done)) = did.seek
                && let Some(id) = state
                    .lines
                    .iter()
                    .find(|l| l.seq == seq)
                    .and_then(|l| l.attachments.get(index))
                    .map(|a| a.id.clone())
                && let Some(note) = self.pane(at).notes.get(&id)
            {
                note.seek(done);
            }
            if let Some(index) = did.open {
                self.pane(at).viewing = Some((seq, index));
                // Opened as it is: whole, and in the middle. A picture that
                // came back at the zoom somebody left the *last* one at is a
                // picture that opens showing a corner of itself.
                self.pane(at).look = Look::default();
            }
            if let Some(index) = did.save {
                // Asked here, acted on in `take_choices`: on a desktop the
                // dialog blocks and the answer is in by the next pass; on a
                // phone it arrives when the platform's activity ends.
                let name = save_name(
                    line.at,
                    index,
                    line.attachments.get(index).and_then(|a| a.bytes.as_deref()),
                );
                self.saving = Some((at.clone(), seq, index, files::save_file(&name)));
                ui.ctx().request_repaint();
            }
        }
    }

    /// The box a message is written in.
    ///
    /// The button is laid out **first and from the right**, and the field then
    /// takes what is left. Doing it the other way round -- a right-aligned
    /// button before the field -- consumed the whole row, and the field was
    /// allocated the nothing that remained: a composer with no box to write in,
    /// which is what the first snapshot of this showed.
    /// Stage files without a file dialog, for a test that cannot open one.
    #[doc(hidden)]
    pub fn stage_for_test(&mut self, me: PubKey, exchange: &str, paths: Vec<std::path::PathBuf>) {
        let at = (me, exchange.to_string());
        self.stage(&at, paths, &egui::Context::default());
    }

    /// Put files in the composer, to go with the next message. No more
    /// than the wire takes in one message; the rest are refused, and said
    /// so. Thumbnails are made on a thread of their own -- a photograph
    /// decodes in a good fraction of a second, a clip's first frame longer
    /// -- and land through `previews`.
    /// SIP-47's `sqx-pair:` string for this identity: the account, and every
    /// exchange this identity holds a session at whose domain is known. An
    /// exchange reached by address alone has no domain to name and is left
    /// out -- a phone reaches exchanges by SIP-33, by name or not at all.
    fn pairing_string(&self, at: &At) -> Option<String> {
        let mut domains: Vec<String> = self
            .sessions
            .iter()
            .filter(|(key, _)| key.0 == at.0)
            .filter_map(|(_, session)| session.state().domain)
            .collect();
        domains.sort();
        domains.dedup();
        if domains.is_empty() {
            return None;
        }
        Some(format!("sqx-pair:{}@{}", at.0, domains.join(",")))
    }

    /// Act on a file choice that has been answered since the last pass.
    ///
    /// The choice is asked for in `render`, where the click is, and acted
    /// on here: on a desktop the dialog blocked and the answer is already
    /// in, one pass later; on a phone it arrives whenever the platform's
    /// activity ends. Either way one place stages the files.
    fn take_choices(&mut self, ctx: &egui::Context) {
        if let Some((at, pick)) = &self.picking
            && let Some(answer) = pick.take()
        {
            let at = at.clone();
            self.picking = None;
            if let Some(paths) = answer {
                self.stage(&at, paths, ctx);
            }
        }
        if let Some((at, seq, index, pick)) = &self.saving
            && let Some(answer) = pick.take()
        {
            let (at, seq, index) = (at.clone(), *seq, *index);
            self.saving = None;
            if let Some(mut paths) = answer
                && let Some(to) = paths.pop()
            {
                self.send_as(Some(&at), Cmd::SaveFile { seq, index, to });
            }
        }
    }

    fn stage(&mut self, at: &At, paths: Vec<std::path::PathBuf>, ctx: &egui::Context) {
        let pane = self.pane(at);
        // One channel for the life of the pane; the sending half is cloned
        // into each decoding thread.
        if pane.previews.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            pane.previews = Some(rx);
            pane.previews_tx = Some(tx);
        }
        let tx = pane.previews_tx.clone().expect("made above");
        let mut refused = 0;
        let mut not_files: Vec<String> = Vec::new();
        for path in paths {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // Found out here, where it can be said beside the tiles, rather
            // than at the upload, where it failed the whole message.
            if !path.is_file() {
                not_files.push(name);
                continue;
            }
            // Once. The same file dropped twice is one file on the message.
            if pane
                .staged
                .iter()
                .any(|s| matches!(&s.source, Source::File(p) if *p == path))
            {
                continue;
            }
            if pane.staged.len() >= MOST_FILES {
                refused += 1;
                continue;
            }
            let (kind, _) = sqex_chat::kind_of(&name);
            pane.staged.push(Staged {
                source: Source::File(path.clone()),
                name,
                kind,
                preview: None,
            });
            let tx = tx.clone();
            let wake = ctx.clone();
            std::thread::spawn(move || {
                let preview = session::preview_of(&path, kind).map(|(_, p)| p);
                let _ = tx.send((path, preview));
                wake.request_repaint();
            });
        }
        let mut said = Vec::new();
        if refused > 0 {
            let carried = pane
                .staged
                .iter()
                .filter(|s| matches!(s.source, Source::Carried(_)))
                .count();
            said.push(if carried > 0 {
                format!(
                    "A message carries up to {MOST_FILES} files, and {carried} are already on \
                     this one; {refused} left out."
                )
            } else {
                format!("A message carries up to {MOST_FILES} files; {refused} left out.")
            });
        }
        if !not_files.is_empty() {
            said.push(format!("Not a file: {}.", not_files.join(", ")));
        }
        if !said.is_empty() {
            pane.staging_trouble = Some(said.join(" "));
        }
    }

    /// The staged files, above the box: a thumbnail each, the name under
    /// it while there is no picture, and a way to take it back out.
    fn staged_ui(&mut self, at: &At, ui: &mut egui::Ui, theme: &ColorTheme) {
        // Thumbnails that have arrived since last pass.
        let landed: Vec<(std::path::PathBuf, Option<Vec<u8>>)> = self
            .pane(at)
            .previews
            .as_ref()
            .map(|rx| rx.try_iter().collect())
            .unwrap_or_default();
        for (path, preview) in landed {
            if let Some(s) = self
                .pane(at)
                .staged
                .iter_mut()
                .find(|s| matches!(&s.source, Source::File(p) if *p == path))
            {
                s.preview = preview.filter(|p| !p.is_empty()).map(|p| p.into());
            }
        }
        if self.pane(at).staged.is_empty() {
            // Nothing staged can still have something to say -- "not a
            // file" is about what was refused, not about what is there.
            if let Some(why) = self.pane(at).staging_trouble.clone() {
                ui.colored_label(theme.warning, egui::RichText::new(why).small());
                ui.add_space(tokens::SPACING_XS);
            }
            return;
        }
        const TILE: f32 = 72.0;
        let mut remove: Option<usize> = None;
        ui.horizontal_wrapped(|ui| {
            for (i, s) in self.pane(at).staged.iter().enumerate() {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(TILE, TILE), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, tokens::RADIUS_MD, theme.surface_secondary);
                match &s.preview {
                    Some(bytes) => {
                        let uri = s.uri();
                        ui.ctx()
                            .include_bytes(uri.clone(), egui::load::Bytes::Shared(bytes.clone()));
                        egui::Image::from_bytes(uri, egui::load::Bytes::Shared(bytes.clone()))
                            .corner_radius(tokens::RADIUS_MD)
                            .show_loading_spinner(false)
                            .paint_at(ui, rect);
                    }
                    None => {
                        // The name, for a file with no picture -- or one
                        // whose picture is still being made.
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            sigil_ui::message::preview(&s.name, 10),
                            egui::TextStyle::Small.resolve(ui.style()),
                            theme.text_secondary,
                        );
                    }
                }
                if s.kind == sigil_ui::attachment::VIDEO {
                    let r = TILE * 0.18;
                    ui.painter().circle_filled(
                        rect.center(),
                        r,
                        egui::Color32::from_black_alpha(140),
                    );
                    sigil::icon::draw(
                        ui.painter(),
                        egui::Rect::from_center_size(rect.center(), egui::vec2(r, r)),
                        sigil::Icon::Play,
                        egui::Color32::WHITE,
                    );
                }
                // The way out, in the corner, over the picture.
                let corner = egui::Rect::from_center_size(
                    rect.right_top() + egui::vec2(-tokens::SPACING_SM, tokens::SPACING_SM),
                    egui::vec2(tokens::BUTTON_SM, tokens::BUTTON_SM),
                );
                let out = ui.scope_builder(egui::UiBuilder::new().max_rect(corner), |ui| {
                    sigil_ui::icon_button_named(
                        ui,
                        sigil_ui::Icon::Close,
                        &format!("Remove {}", s.name),
                    )
                    .clicked()
                });
                if out.inner {
                    remove = Some(i);
                }
                // Said to the tree, since the picture says nothing to it.
                out.response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &s.name)
                });
            }
        });
        if let Some(i) = remove {
            let pane = self.pane(at);
            pane.staged.remove(i);
            // Room was made; what was said about there being none is stale.
            pane.staging_trouble = None;
        }
        if let Some(why) = self.pane(at).staging_trouble.clone() {
            ui.colored_label(theme.warning, egui::RichText::new(why).small());
        }
        ui.add_space(tokens::SPACING_XS);
    }

    /// Settle what the session has said about the messages in flight: the
    /// ones that went are forgotten, and one that did not comes back.
    ///
    /// Straight into the box when the box is empty -- what somebody who
    /// pressed Send and saw an error expects to find. If something else has
    /// been typed since, it is offered under the box instead of written
    /// over it: one message must not be lost while putting another back.
    fn took_back(&mut self, at: &At, state: &ChatState) {
        let Some(posted) = &state.posted else { return };
        let pane = self.pane(at);
        let Some(i) = pane.in_flight.iter().position(|u| u.token == posted.token) else {
            return;
        };
        // Everything up to it went, in order, or was answered before.
        let mut settled = pane.in_flight.drain(..=i);
        let this = settled.next_back().expect("the one at i");
        drop(settled);
        let Some(why) = posted.trouble.clone() else {
            return;
        };
        let empty = pane.composing.trim().is_empty()
            && pane.staged.is_empty()
            && pane.editing.is_none()
            && pane.replying.is_none();
        if empty {
            pane.composing = this.composing;
            pane.mentions = this.mentions;
            pane.carried_mentions = this.carried_mentions;
            pane.staged = this.staged;
            pane.editing = this.editing;
            pane.replying = this.replying;
        } else {
            pane.put_back = Some((this, why));
        }
    }

    /// Write a recorded note where sigil keeps its own files, named for the
    /// moment it was made.
    ///
    /// **Not a temporary file.** `std::env::temp_dir` is `/data/local/tmp` on
    /// Android, which the app cannot write to; sigil's own data directory is
    /// pointed inside the app's files directory by the Android entry point and
    /// exists on every other platform too. The name carries the stamp for the
    /// same reason a saved picture's does: a directory of `note.ogg`,
    /// `note-1.ogg` says nothing about which is which.
    fn write_note(bytes: &[u8]) -> Result<std::path::PathBuf, String> {
        let dir = dirs::data_local_dir()
            .ok_or_else(|| "nowhere to write the recording".to_string())?
            .join("sigil")
            .join("notes");
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let path = dir.join(format!("voice-{}.ogg", sigil_ui::clock::file_stamp(at)));
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    /// The composer while a voice note is being recorded: throw it away,
    /// what it is hearing, how long it has been, and send.
    ///
    /// **The meter is the level the note will carry**, on SIP-15's scale,
    /// so what somebody watches while talking is the same measurement the
    /// bars are drawn from at the other end -- not a second, prettier one
    /// that could disagree with it.
    fn recording_ui(&mut self, at: &At, ui: &mut egui::Ui, theme: &ColorTheme) {
        let (elapsed, level, deaf) = match self.pane(at).recording.as_ref() {
            Some(r) => (r.elapsed_ms(), r.level(), r.deaf()),
            None => return,
        };
        let mut throw_away = false;
        let mut send = false;
        ui.horizontal(|ui| {
            ui.set_min_height(tokens::FIELD_LG);
            if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Close, "Throw it away").clicked() {
                throw_away = true;
            }
            if deaf {
                // The microphone would not open -- busy with a call, or
                // refused. Said here, where the press was, rather than
                // left as a recording that turns out to be silence.
                ui.colored_label(theme.warning, "no microphone");
            } else {
                // A dot that keeps time, the way a camera's does: it is
                // the one part of this that says *recording* rather than
                // *ready to*.
                let on = self.now().is_multiple_of(2);
                let (dot, _) = ui.allocate_exact_size(
                    egui::vec2(tokens::ICON_SM, tokens::ICON_SM),
                    egui::Sense::hover(),
                );
                if on {
                    ui.painter()
                        .circle_filled(dot.center(), tokens::ICON_SM * 0.25, theme.warning);
                }
                ui.colored_label(theme.text_primary, sigil_ui::video::clock(elapsed));
                // The meter, in the room the row has left over.
                let room = ui.available_width() - tokens::BUTTON_LG - tokens::SPACING_MD;
                if room > tokens::ICON_MD {
                    sigil_ui::attachment::waveform(
                        ui,
                        // One bar's worth of *now*, repeated: a meter is a
                        // waveform of length one, and drawing it with the
                        // same function is what stops the two disagreeing.
                        &[level; 24],
                        theme.accent,
                        room,
                        None,
                    );
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Send)
                    .on_hover_text("Send it")
                    .clicked()
                {
                    send = true;
                }
            });
        });
        // Repainted while it records: the clock and the meter are only
        // worth having if they move.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
        if throw_away {
            // Dropped, which stops the microphone. Nothing is written and
            // nothing is sent: a recording thrown away leaves no file on
            // the machine to be found later.
            self.pane(at).recording = None;
            return;
        }
        if send {
            let recorder = self.pane(at).recording.take();
            match recorder.and_then(|r| r.finish()) {
                Some(bytes) => {
                    let ctx = ui.ctx().clone();
                    match Self::write_note(&bytes) {
                        // Staged rather than sent: it goes through the
                        // same path a picked file does, which is what
                        // gives it its waveform (`preview_of`) and lets
                        // somebody put words beside it before it goes.
                        Ok(path) => self.stage(at, vec![path], &ctx),
                        Err(why) => self.pane(at).command_trouble = Some(why),
                    }
                }
                // Nothing was heard: a press and a release, or a
                // microphone that never opened. Saying so beats sending an
                // empty file with a length of nought.
                None => {
                    self.pane(at).command_trouble =
                        Some("nothing was recorded — the microphone heard nothing".into())
                }
            }
        }
    }

    fn composer_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        self.took_back(at, state);
        // A refused message that could not go straight back: said, with
        // its first words, and the way to have it back or to let it go.
        if let Some((unsent, why)) = self.pane(at).put_back.take() {
            let mut keep = Some((unsent, why));
            ui.horizontal_wrapped(|ui| {
                let (unsent, why) = keep.as_ref().expect("set above");
                ui.colored_label(
                    theme.destructive,
                    egui::RichText::new(format!(
                        "Not sent: \"{}\" — {why}",
                        sigil_ui::message::preview(&unsent.composing, 32)
                    ))
                    .small(),
                );
                if ui.small_button("Put it back").clicked() {
                    let (unsent, _) = keep.take().expect("set above");
                    let pane = self.pane(at);
                    // Over what is there: chosen, this time.
                    pane.composing = unsent.composing;
                    pane.mentions = unsent.mentions;
                    pane.carried_mentions = unsent.carried_mentions;
                    pane.staged = unsent.staged;
                    pane.editing = unsent.editing;
                    pane.replying = unsent.replying;
                }
                if ui.small_button("Forget it").clicked() {
                    keep = None;
                }
            });
            self.pane(at).put_back = keep;
        }
        // What Enter will do, said above the box. A composer that silently
        // means three different things depending on invisible state is one
        // that will eventually send an edit as a new message.
        let replying = self.pane(at).replying;
        let editing = self.pane(at).editing;
        if let Some(target) = editing.or(replying) {
            // The message being answered, as the reply will quote it: the
            // same head a reply bubble has, with the × in its corner. A
            // message no longer in the window is quoted by number alone.
            let quoted = state
                .lines
                .iter()
                .find(|l| l.seq == target)
                .map(session::Line::quoted)
                .unwrap_or_else(|| session::Quoted::unheld(target));
            let head = sigil_ui::message::reply_preview(
                ui,
                sigil_ui::Quote {
                    seq: quoted.seq,
                    who: &quoted.who,
                    said: &quoted.said,
                    preview: quoted.preview.as_ref().map(thumb),
                },
                editing.is_some(),
            );
            if head.jump
                && let Some(channel) = state.open
            {
                self.pane(at).jump = Some((channel, target));
            }
            if head.cancel {
                self.pane(at).cancel_head();
            }
            ui.add_space(tokens::SPACING_XS);
        }

        // **`@` offers the room.** While the text ends in `@` and some of a
        // name, the members it could mean are listed above the box, and the
        // keys that would otherwise go to the box go to the list: Up and
        // Down move, Enter or Tab choose, Escape closes it for this `@`.
        // Taken from the input before the field is drawn, so an Enter that
        // chose a name is not also an Enter that sent the message.
        let picker = {
            let pane = self.pane(at);
            mention::mention_query(&pane.composing)
                .filter(|_| !pane.picker_dismissed)
                .map(|q| mention::candidates(q, &state.members, &state.people, state.me))
                .filter(|c| !c.is_empty())
        };
        let mut chosen: Option<String> = None;
        let mut refocus = false;
        if let Some(rows) = &picker {
            let (picking, choose, dismiss) = picker_keys(ui, self.pane(at).picking, rows.len());
            if choose {
                chosen = Some(rows[picking].0.clone());
            }
            if dismiss {
                self.pane(at).picker_dismissed = true;
                // egui surrenders a text field's focus on Escape before any
                // widget runs, so the field is given it back below: Escape
                // here means "not that name", not "leave the box".
                refocus = true;
            }
            self.pane(at).picking = picking;
            egui::Frame::NONE
                .fill(theme.surface_elevated)
                .corner_radius(tokens::RADIUS_MD)
                .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (i, (label, key)) in rows.iter().enumerate() {
                        let row = ui.horizontal(|ui| {
                            let pressed = ui
                                .selectable_label(i == picking, format!("@{label}"))
                                .clicked();
                            // The key beside the name, here as everywhere a
                            // name is: the name is what this client calls
                            // the key, and two members can share one.
                            ui.colored_label(
                                theme.text_muted,
                                egui::RichText::new(sigil_ui::message::short(&key.to_string()))
                                    .monospace()
                                    .small(),
                            );
                            pressed
                        });
                        if row.inner {
                            chosen = Some(label.clone());
                        }
                    }
                });
            ui.add_space(tokens::SPACING_XS);
        }
        let mut completed = false;
        if let Some(label) = chosen
            && let Some(rows) = &picker
            && let Some((_, key)) = rows.iter().find(|(l, _)| *l == label)
        {
            let pane = self.pane(at);
            mention::complete_mention(&mut pane.composing, &label);
            pane.mentions.push((label, *key));
            pane.picking = 0;
            completed = true;
        }

        // **`/` offers the commands.** While the box holds a slash and some
        // of a word, the commands it could be are listed above it with what
        // each does, and the keys go to the list as they do for `@`. One
        // that takes nothing more is run the moment it is chosen; one that
        // does is completed, and the argument is typed after it.
        let commands = {
            let pane = self.pane(at);
            command::command_query(&pane.composing)
                .filter(|_| !pane.picker_dismissed)
                .map(command::candidates)
                .filter(|c| !c.is_empty())
        };
        let mut run: Option<command::Command> = None;
        if let Some(rows) = &commands {
            let (picking, choose, dismiss) = picker_keys(ui, self.pane(at).picking, rows.len());
            let mut pick: Option<&command::Spec> = choose.then(|| rows[picking]);
            if dismiss {
                self.pane(at).picker_dismissed = true;
                refocus = true;
            }
            self.pane(at).picking = picking;
            egui::Frame::NONE
                .fill(theme.surface_elevated)
                .corner_radius(tokens::RADIUS_MD)
                .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    for (i, spec) in rows.iter().enumerate() {
                        let row = ui.horizontal(|ui| {
                            let word = if spec.arg.is_empty() {
                                format!("/{}", spec.name)
                            } else {
                                format!("/{} {}", spec.name, spec.arg)
                            };
                            let pressed = ui.selectable_label(i == picking, word).clicked();
                            ui.colored_label(
                                theme.text_muted,
                                egui::RichText::new(spec.what).small(),
                            );
                            pressed
                        });
                        if row.inner {
                            pick = Some(spec);
                        }
                    }
                });
            ui.add_space(tokens::SPACING_XS);
            if let Some(spec) = pick {
                let pane = self.pane(at);
                command::complete(&mut pane.composing, spec);
                pane.picking = 0;
                if spec.arg.is_empty() {
                    run = command::parse(&pane.composing).ok().flatten();
                } else {
                    completed = true;
                }
            }
        }
        if let Some(why) = self.pane(at).command_trouble.clone() {
            ui.colored_label(theme.warning, egui::RichText::new(why).small());
        }

        // **Escape does what the × does**, while the box has the keyboard
        // and no list is up to take the key first. The focus test is of
        // last pass: egui surrenders a field's focus on Escape before any
        // widget runs, so by now the box has already let go.
        if picker.is_none() && commands.is_none() {
            let pane = self.pane(at);
            let armed = pane.editing.or(pane.replying).is_some();
            let in_the_box = pane
                .field
                .is_some_and(|id| ui.memory(|m| m.had_focus_last_frame(id)));
            if armed
                && in_the_box
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                pane.cancel_head();
                refocus = true;
            }
        }

        self.staged_ui(at, ui, theme);

        // **While a note is being recorded the row is the recording.**
        // Not a box with a microphone beside it: there is nothing to type
        // into while talking, and a composer that looks unchanged is one
        // somebody keeps typing in. Three things, which is all there is to
        // decide -- throw it away, look at what it is hearing, send it.
        if self.pane(at).recording.is_some() {
            self.recording_ui(at, ui, theme);
            return;
        }

        ui.horizontal(|ui| {
            // Room for both controls, measured rather than guessed: the field
            // took `available - one button` while two sat beside it, and Send
            // ran off the edge of the window. The button's size is the
            // form's: a phone's is bigger, and measured at the desktop's
            // Send ran off the edge of the phone.
            let form = sigil::Form::of(ui.ctx());
            let phone = form.is_phone();
            let button = form.button_size();
            let controls = (button + ui.spacing().item_spacing.x) * 2.0;
            // **The microphone takes room from the box, and is decided
            // before the box is drawn.** A widget outside its clip cannot
            // be pressed -- so a microphone drawn after a field that took
            // `available_width()` is a button that is *there*, in the
            // accessibility tree and in a screenshot, and dead to a
            // finger. Found by a test that pressed it and got nothing.
            let ready =
                !self.pane(at).composing.trim().is_empty() || !self.pane(at).staged.is_empty();
            let mic = !ready;
            // On a phone the box is the whole width and a little taller,
            // with its controls inside it at the right; there is no Send
            // button, because the keyboard's own key sends. On a desktop
            // the two buttons sit beside it as they always have.
            let width = if phone {
                ui.available_width()
            } else {
                (ui.available_width() - controls).max(80.0)
            };
            // **Two controls inside the box, not one beside it.** The
            // microphone first went next to the field, which took a
            // button's width off a box that is meant to run to the edge --
            // and a test that had been asserting exactly that since the
            // phone composer was built caught it.
            let slot_width = if mic {
                button * 2.0 + tokens::SPACING_XS
            } else {
                button
            };
            let (field, slot) = if phone {
                let (field, slot) = sigil_ui::field_with_slot(
                    ui,
                    &mut self.panes.entry(at.clone()).or_default().composing,
                    "Write message",
                    width,
                    tokens::FIELD_LG,
                    slot_width,
                );
                (field, Some(slot))
            } else {
                (
                    sigil_ui::field(
                        ui,
                        &mut self.panes.entry(at.clone()).or_default().composing,
                        "Write message",
                        width,
                    ),
                    None,
                )
            };
            self.pane(at).field = Some(field.id);
            if refocus {
                field.request_focus();
            }
            // A name was just completed: the caret goes after it, or it sits
            // where the `@` was and the next word lands inside the name.
            if completed {
                let chars = self.pane(at).composing.chars().count();
                if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), field.id) {
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(
                        egui::text::CCursor::new(chars),
                    )));
                    egui::TextEdit::store_state(ui.ctx(), field.id, st);
                }
                field.request_focus();
            }
            // Typing is published from the fact that the text changed, not from
            // the field having focus: a box somebody is sitting in front of and
            // not writing in is not typing, and saying otherwise is a claim
            // about them that they did not make.
            if field.changed() {
                // Typing again reopens a list Escape closed, and what was
                // said about the last command is about the last command.
                self.pane(at).picker_dismissed = false;
                self.pane(at).command_trouble = None;
                let writing = !self.pane(at).composing.is_empty();
                if self.pane(at).announced_typing != writing {
                    self.pane(at).announced_typing = writing;
                    self.send_as(Some(at), Cmd::Typing(writing));
                }
            }
            // Attach sits before Send, which is where every messenger puts
            // it: the last control on the row is the one that commits.
            let attach = |ui: &mut egui::Ui| {
                sigil_ui::icon_button(ui, sigil_ui::Icon::Attach)
                    .on_hover_text(
                        "Attach files — pictures, clips, anything, up to four in a message. \
                         They are sealed before they leave this machine.",
                    )
                    .clicked()
            };
            // **On a phone the slot commits once there is something to
            // commit.** The clip becomes the dart: a phone keyboard's
            // Enter is the keyboard's to define -- several send a newline
            // the box swallows, and then nothing happens at all -- so a
            // messenger that relies on it has no way to send. Empty, the
            // slot is the paperclip, which is what an empty box is for.
            let commits = phone && ready;
            let word = if editing.is_some() {
                "Save the rewrite"
            } else {
                "Send"
            };
            let in_the_slot = |ui: &mut egui::Ui| {
                if commits {
                    sigil_ui::icon_button(ui, sigil_ui::Icon::Send)
                        .on_hover_text(word)
                        .clicked()
                } else {
                    attach(ui)
                }
            };
            // The microphone sits beside whatever the slot holds: it is
            // the same sort of thing as the paperclip -- something to put
            // in a message -- and it goes away once there is something to
            // send, which is when the slot becomes the dart.
            let with_mic = |ui: &mut egui::Ui| {
                let first = in_the_slot(ui);
                let recorded = mic
                    && sigil_ui::icon_button(ui, sigil_ui::Icon::Mic)
                        .on_hover_text(
                            "Record a voice note. It is sealed before it leaves this \
                             machine, like any other file.",
                        )
                        .clicked();
                (first, recorded)
            };
            let (pressed, record) = match slot {
                Some(slot) => sigil_ui::in_slot_row(ui, slot, with_mic),
                None => with_mic(ui),
            };
            if record {
                let ctx = ui.ctx().clone();
                self.pane(at).recording = Some(sigil_video::note::Recorder::start(ctx));
            }
            if pressed && !commits {
                self.picking = Some((at.clone(), files::pick_files()));
                ui.ctx().request_repaint();
            }
            let send = (pressed && commits)
                || (!phone
                    && sigil_ui::icon_button(ui, sigil_ui::Icon::Send)
                        .on_hover_text(word)
                        .clicked());
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            // Words, or files -- the original's included, on a rewrite, so
            // a picture's caption can be taken off and a picture without
            // one can still be rewritten.
            let something =
                !self.pane(at).composing.trim().is_empty() || !self.pane(at).staged.is_empty();
            // A line that begins with a slash is asked of this client, not
            // posted -- unless it begins with two, which is a message that
            // begins with one.
            let line = self.pane(at).composing.clone();
            let mut trouble = None;
            let is_command = command::as_message(line.trim()).is_none() && !line.trim().is_empty();
            if (entered || send) && is_command {
                match command::parse(&line) {
                    Ok(Some(cmd)) => run = Some(cmd),
                    Ok(None) => {}
                    Err(why) => trouble = Some(why),
                }
                field.request_focus();
            }
            if let Some(why) = trouble {
                self.pane(at).command_trouble = Some(why);
            }
            if let Some(cmd) = run.take() {
                let pane = self.pane(at);
                pane.composing.clear();
                pane.command_trouble = None;
                if let Err(why) = self.run_command(ctx, at, state, cmd, ui.ctx()) {
                    self.pane(at).command_trouble = Some(why);
                }
                field.request_focus();
            }
            if (entered || send) && something && !is_command {
                // Taken, and kept: the box is emptied for the next message,
                // and everything that was in it goes into `in_flight` under
                // the draft's token until the session says whether it went.
                // See `took_back`.
                let pane = self.pane(at);
                let token = pane.next_token;
                pane.next_token += 1;
                let text = std::mem::take(&mut pane.composing);
                // `//x` was typed to send `/x`.
                let text = command::as_message(text.trim()).unwrap_or(text);
                let (editing, replying) = (pane.editing.take(), pane.replying.take());
                pane.announced_typing = false;
                // The keys, for every name still in the text.
                let mut mentions = mention::mentions_in(&text, &pane.mentions);
                let labels = std::mem::take(&mut pane.mentions);
                let carried_mentions = std::mem::take(&mut pane.carried_mentions);
                mentions.extend(carried_mentions.iter().copied());
                // A rewrite sent: whatever was being typed before it began
                // comes back.
                if editing.is_some() {
                    pane.unstash();
                }
                // And the files, in the order they were staged: the ones
                // to upload, and the original's to keep on a rewrite.
                let staged = std::mem::take(&mut pane.staged);
                pane.staging_trouble = None;
                let mut files = Vec::new();
                let mut keep = Vec::new();
                for s in &staged {
                    match &s.source {
                        Source::File(path) => files.push(path.clone()),
                        Source::Carried(id) => keep.push(id.clone()),
                    }
                }
                pane.in_flight.push(Unsent {
                    token,
                    composing: text.clone(),
                    mentions: labels,
                    carried_mentions,
                    staged,
                    editing,
                    replying,
                });
                self.send_as(
                    Some(at),
                    Cmd::Post(session::Draft {
                        text: text.trim().to_string(),
                        reply: replying,
                        edit: editing,
                        mentions,
                        files,
                        keep,
                        token,
                    }),
                );
                self.send_as(Some(at), Cmd::Typing(false));
                field.request_focus();
            }
        });
    }
}

/// The keys a list above the box takes while it is up: Up and Down move,
/// Enter or Tab choose, Escape dismisses. Taken from the input before the
/// field is drawn, so an Enter that chose is not also an Enter that sent.
/// Returns where the keyboard is now, and whether a choice or a dismissal
/// was made.
fn picker_keys(ui: &egui::Ui, picking: usize, len: usize) -> (usize, bool, bool) {
    let mut picking = picking.min(len.saturating_sub(1));
    let (up, down, choose, dismiss) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        )
    });
    if up {
        picking = picking.saturating_sub(1);
    }
    if down {
        picking = (picking + 1).min(len.saturating_sub(1));
    }
    (picking, choose, dismiss)
}

impl ChatApp {
    /// Do what a line beginning with a slash asked. Each command is the
    /// same thing its control does -- the Call button, the bell, the
    /// Answer button -- by the same path, so there is one way each thing
    /// happens and the command cannot drift from the control. `Err` is
    /// for the person: the command made sense and cannot be done here.
    fn run_command(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        cmd: command::Command,
        egui_ctx: &egui::Context,
    ) -> Result<(), String> {
        use command::Command;
        let me = at.0;
        let open = state
            .conversations
            .iter()
            .find(|c| Some(c.channel) == state.open);
        let here = || open.ok_or_else(|| "Nothing is open.".to_string());
        let ringing_here = || {
            state
                .ringing
                .iter()
                .find(|r| Some(r.channel) == state.open && !r.mine && !r.answered)
                .cloned()
                .ok_or_else(|| "Nothing is ringing here.".to_string())
        };
        match cmd {
            Command::Call => {
                let c = here()?;
                if c.public != Some(false) {
                    return Err("Only a private conversation can be called.".into());
                }
                if self.calls.contains_key(&me) || state.ringing.iter().any(|r| r.mine) {
                    return Err("You are already in a call, or placing one.".into());
                }
                let direct = ctx.accounts.prefs.direct_calls;
                self.send_as(Some(at), Cmd::Call { direct });
            }
            Command::Answer => {
                let ring = ringing_here()?;
                self.send_as(
                    Some(at),
                    Cmd::Answer {
                        channel: ring.channel,
                        seq: ring.seq,
                    },
                );
                self.join_call(ctx, at, &ring, egui_ctx);
            }
            Command::Decline => {
                let ring = ringing_here()?;
                self.send_as(
                    Some(at),
                    Cmd::Decline {
                        channel: ring.channel,
                        seq: ring.seq,
                    },
                );
            }
            Command::Hangup => {
                if let Some((channel, seq, seconds)) = self.leave_call(me) {
                    self.send_as(
                        Some(at),
                        Cmd::Hangup {
                            channel,
                            seq,
                            seconds,
                        },
                    );
                } else if let Some(ring) = state.ringing.iter().find(|r| r.mine) {
                    self.send_as(
                        Some(at),
                        Cmd::Hangup {
                            channel: ring.channel,
                            seq: ring.seq,
                            seconds: 0,
                        },
                    );
                } else {
                    return Err("You are not in a call.".into());
                }
            }
            Command::Mute | Command::Unmute => {
                let c = here()?;
                ctx.accounts
                    .quiet
                    .set_muted(&at.1, &c.channel, matches!(cmd, Command::Mute));
            }
            Command::Topic(text) => {
                here()?;
                self.send_as(Some(at), Cmd::SetTopic(text));
            }
            Command::Name(text) => {
                let c = here()?;
                if c.peer.is_some() {
                    return Err("A direct message is named after the person in it.".into());
                }
                self.send_as(Some(at), Cmd::SetName(text));
            }
            Command::Invite(key) => {
                here()?;
                let key: PubKey = key
                    .parse()
                    .map_err(|_| format!("{key} is not a key. /invite takes a base58 key."))?;
                self.send_as(Some(at), Cmd::Invite(key));
            }
            Command::Verify => {
                let c = here()?;
                let peer = c
                    .peer
                    .ok_or("Safety words are for two people: open a direct message.")?;
                self.pane(at).dialog = Some(Dialog::Verify(peer));
                self.send_as(Some(at), Cmd::Attested(peer));
            }
            Command::Members => {
                here()?;
                self.send_as(Some(at), Cmd::Blocked);
                ctx.navigator.push_here(Route::Members);
            }
            Command::Settings => {
                here()?;
                ctx.navigator.push_here(Route::Settings);
            }
            Command::Devices => {
                self.send_as(Some(at), Cmd::Devices);
                self.send_as(Some(at), Cmd::BackupStatus);
                ctx.navigator.push_here(Route::Devices);
            }
            Command::Search(text) => {
                self.columns_open = true;
                self.pane(at).searching = text.clone();
                self.pane(at).chosen = None;
                self.send_as(Some(at), Cmd::Search(text));
            }
            Command::New => {
                self.pane(at).dialog = Some(Dialog::Compose);
            }
            Command::Profile => self.open_profile(at, state),
            Command::Reconnect => self.send_as(Some(at), Cmd::Reconnect),
            Command::Help => {
                // The list is the help: a lone slash shows all of it.
                let pane = self.pane(at);
                pane.composing = "/".into();
                pane.picker_dismissed = false;
            }
        }
        Ok(())
    }
}

/// A session thumbnail, borrowed for drawing.
fn thumb(t: &session::Thumb) -> sigil_ui::Thumb<'_> {
    sigil_ui::Thumb {
        id: &t.id,
        bytes: &t.bytes,
    }
}

impl ChatApp {
    /// The public directory: search it, and join what it turns up.
    ///
    /// This is the only way into a public channel and the only channel route
    /// open to anybody — every other one names an identifier, and an identifier
    /// is not an authorisation. A private channel is not merely un-joinable, it
    /// is unmentionable, so nothing here can be made to admit one exists.
    fn directory_view(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        let Some(at) = self.showing_at(ctx) else {
            return AppResponse::default();
        };
        let at = &at;
        let state = self.state_of(Some(at));

        if !bar_has_the_head(ui) {
            ui.horizontal(|ui| {
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                    ctx.navigator.back();
                }
                ui.heading("Public channels");
            });
        }
        ui.colored_label(
            theme.text_secondary,
            egui::RichText::new(
                "Anybody may join these, and nothing said in one is encrypted — everyone \
                 who may join would hold any key it used.",
            )
            .small(),
        );
        ui.add_space(tokens::SPACING_SM);
        // **A directory arrives full.** The empty box means "everything",
        // which the hint says and nobody reads -- so this card opened on a
        // sentence telling somebody to search a list it could simply have
        // shown them. Asked once per pane: a search of their own replaces
        // it, and coming back does not ask again.
        if !self.pane(at).listed {
            self.pane(at).listed = true;
            self.send_as(Some(at), Cmd::Find(String::new()));
        }

        // The same box as the chat list's, so the phone has one search
        // control rather than two that look unrelated: full width, a little
        // taller, and the magnifier *in* the box at the right. It was the
        // desktop's row here -- a 320-point field asked for in a 360-point
        // pane, with a "Search" button after it -- which showed twenty
        // characters of a hint that is a sentence.
        let form = sigil::Form::of(ui.ctx());
        ui.horizontal(|ui| {
            let phone = form.is_phone();
            let control = form.button_size() + ui.spacing().item_spacing.x * 2.0;
            let width = ui.available_width() - if phone { 0.0 } else { control };
            let pane = self.panes.entry(at.clone()).or_default();
            let (field, slot) = if phone {
                let (field, slot) = sigil_ui::field_with_slot(
                    ui,
                    &mut pane.query,
                    "name a channel, or leave empty for everything",
                    width,
                    tokens::FIELD_LG,
                    form.button_size(),
                );
                (field, Some(slot))
            } else {
                (
                    sigil_ui::field(
                        ui,
                        &mut pane.query,
                        "name a channel, or leave empty for everything",
                        width,
                    ),
                    None,
                )
            };
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let control = |ui: &mut egui::Ui| sigil_ui::icon_button(ui, sigil_ui::Icon::Search);
            let pressed = match slot {
                Some(slot) => sigil_ui::in_slot(ui, slot, control),
                None => control(ui),
            }
            .clicked();
            if entered || pressed {
                let query = self.pane(at).query.clone();
                self.send_as(Some(at), Cmd::Find(query));
            }
        });
        ui.add_space(tokens::SPACING_SM);
        // A join that was refused says so here, where the button is. It
        // used to be said only in a conversation's bar, which this pane is
        // not -- so a refused join looked like a button that did nothing.
        if let Some(trouble) = &state.trouble {
            ui.colored_label(theme.destructive, trouble);
            ui.add_space(tokens::SPACING_SM);
        }

        if state.found.is_empty() {
            // "Nothing matched" and "nobody has searched" are different facts
            // and the pane says which.
            ui.colored_label(
                theme.text_secondary,
                if state.searched {
                    "Nothing matched."
                } else {
                    "Search to see what this exchange is carrying."
                },
            );
            return AppResponse::default();
        }

        let narrow = ui.available_width() < tokens::NARROW_WIDTH;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for found in &state.found {
                    let already = state
                        .conversations
                        .iter()
                        .any(|c| c.channel == found.channel);
                    ui.horizontal(|ui| {
                        let id = bs58::encode(found.channel).into_string();
                        sigil_ui::identicon(ui, &id, tokens::AVATAR_MD);
                        ui.add_space(tokens::SPACING_SM);
                        // **The action first, from the right.** A `vertical`
                        // given the row takes the whole of it -- the topic
                        // wraps to the width there is, so "the width there
                        // is" became all of it -- and the button drawn after
                        // it was painted over the topic it had left no room
                        // for. Laying the action out first and the text in
                        // what remains is what `search_hit` does with a time,
                        // and for the same reason.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if already {
                                ui.colored_label(theme.text_muted, "joined");
                            } else if !found.here {
                                // Add that exchange -- through the home when
                                // there is one to carry it: the reason to look
                                // from here rather than there is not wanting
                                // to be there in person. The box stays on the
                                // dialog to untick.
                                //
                                // The domain is in the label where there is
                                // room for it and in the row's own last line
                                // -- "lives at trunk.exchange" -- either way,
                                // so a narrow pane drops it from the button
                                // rather than spending half the row on a
                                // fact already on screen.
                                let label = if narrow {
                                    "Add exchange".to_string()
                                } else {
                                    format!("Add {}", found.domain)
                                };
                                if ui
                                    .button(label)
                                    .on_hover_text(format!(
                                        "connect this identity to {} to join it",
                                        found.domain
                                    ))
                                    .clicked()
                                {
                                    let home = self.home_domain(ctx);
                                    let pane = self.pane(at);
                                    pane.exchange = found.domain.clone();
                                    pane.exchange_via = home
                                        .is_some_and(|h| !h.eq_ignore_ascii_case(&found.domain));
                                    pane.add_trouble = None;
                                    pane.dialog = Some(Dialog::Exchange);
                                }
                            } else if sigil_ui::icon_button_named(
                                ui,
                                sigil_ui::Icon::Plus,
                                "Join this channel",
                            )
                            .clicked()
                            {
                                // Reading a public channel *is* joining it:
                                // fetching requires membership, so there is no
                                // way to look without becoming a member.
                                self.send_as(
                                    Some(at),
                                    Cmd::Join {
                                        channel: found.channel,
                                        instance: found.instance,
                                    },
                                );
                            }
                            // What is left of the row, and no more.
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&found.name).strong())
                                        .truncate(),
                                );
                                if !found.topic.is_empty() {
                                    ui.colored_label(
                                        theme.text_secondary,
                                        egui::RichText::new(&found.topic).small(),
                                    );
                                }
                                ui.colored_label(
                                    theme.text_muted,
                                    egui::RichText::new(match found.members {
                                        1 => "1 member".to_string(),
                                        n => format!("{n} members"),
                                    })
                                    .small(),
                                );
                                // SIP-16 §What a client does with a search
                                // row: a room listed from elsewhere is
                                // joined there.
                                if !found.here {
                                    ui.colored_label(
                                        theme.text_muted,
                                        egui::RichText::new(format!(
                                            "lives at {}; no copy is held here",
                                            found.domain
                                        ))
                                        .small(),
                                    );
                                }
                            });
                        });
                    });
                    ui.separator();
                }
            });
        AppResponse::default()
    }

    /// Who is in the open conversation.
    fn members_view(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        let Some(at) = self.showing_at(ctx) else {
            return AppResponse::default();
        };
        let at = &at;
        let me = at.0;
        let state = self.state_of(Some(at));

        if !bar_has_the_head(ui) {
            ui.horizontal(|ui| {
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                    ctx.navigator.back();
                }
                ui.heading("Members");
            });
        }

        if state.i_am_admin {
            ui.add_space(tokens::SPACING_SM);
            let (_, add) = sigil_ui::labelled_field(
                ui,
                "Invite",
                &mut self.panes.entry(at.clone()).or_default().inviting,
                "paste their key, or type name@domain",
                Some(sigil_ui::Action::Mark(sigil_ui::Icon::Plus, "Add them")),
            );
            if add {
                let typed = self.pane(at).inviting.trim().to_string();
                match typed.parse::<PubKey>() {
                    Ok(who) => {
                        self.pane(at).inviting.clear();
                        self.send_as(Some(at), Cmd::Invite(who));
                    }
                    Err(e) => self.pane(at).add_trouble = Some(format!("that is not a key: {e}")),
                }
            }
            // Inviting grants the history, and that is a decision rather than
            // a side effect: sealing the current epoch is what hands it over,
            // and rotating instead would deny it.
            ui.colored_label(
                theme.text_muted,
                egui::RichText::new(
                    "Somebody invited is given the key in force, so they can read what is \
                     already here.",
                )
                .small(),
            );
        }

        ui.add_space(tokens::SPACING_SM);
        // **Five buttons do not fit a phone.** A member's row carried Verify,
        // Remove, Make admin, Mute and Block inline, laid out right to left.
        // At 360 points that row was some 200 points wider than the pane, so
        // it overflowed *and* -- because egui grows a ui to whatever is drawn
        // in it -- every row after it was laid out for a pane that wide: the
        // reports below it started off the left edge, and the buttons were
        // painted over the member above. The conversation bar already answers
        // this by folding to a More menu on a narrow pane; a roster row folds
        // the same way, which is also what a phone roster is.
        let narrow = ui.available_width() < tokens::NARROW_WIDTH;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for member in &state.members {
                    let key = member.account.to_string();
                    let person = state
                        .people
                        .get(&member.account)
                        .cloned()
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        // Whether they are there, on the mark: ours from
                        // this machine, everybody else's from their beacon.
                        let (seen, hover) = if member.account == me {
                            let seen = if ctx.away {
                                sigil_ui::Presence::Away
                            } else {
                                sigil_ui::Presence::Active
                            };
                            (seen, format!("{} — you\n{key}", seen.word()))
                        } else {
                            self.presence_of(&state, &member.account)
                        };
                        sigil_ui::presence(
                            ui,
                            &key,
                            None,
                            tokens::AVATAR_SM,
                            seen,
                            seen.word(),
                            &hover,
                        );
                        ui.add_space(tokens::SPACING_SM);
                        // **The actions first, from the right; the text in
                        // what is left.** A `vertical` given the row takes
                        // the whole of it, so the name inside it had nothing
                        // to be truncated *to*: it drew at whatever length
                        // somebody had typed and carried the roster out past
                        // the pane with it. Laying the menu out first bounds
                        // the text, which is what makes `truncate` mean
                        // anything -- the same order the directory's rows
                        // use, and for the same reason.
                        let acts = member_acts(&state, member);
                        let mut chose = None;
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if member.account != me {
                                chose = member_actions_ui(ui, &acts, narrow, &key);
                            }
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                // **Pinned, not merely available.** A child
                                // ui in a right-to-left row is given the rest
                                // of the row as its *max* rect, and a
                                // `horizontal` inside it still reports the
                                // parent's width to a `truncate` -- so the
                                // name drew in full and pushed the menu
                                // button off the row it was supposed to sit
                                // beside. Setting the width makes the bound
                                // one the label can see.
                                ui.set_max_width(ui.available_width());
                                // **The badges measured, the name given the
                                // rest.** `truncate` shrinks a label to what
                                // is available *at the moment it is added* --
                                // which was the whole row -- so a long name
                                // took all of it and "admin", "muted" and
                                // "you" went past the edge after it.
                                //
                                // Laying the badges out first from the right
                                // fixed the phone and broke the desktop: a
                                // right-to-left row spans what it is given,
                                // so on a 990-point window the words flew to
                                // the far edge, a pane away from the member
                                // they belong to. So they are *measured*
                                // instead, the name is given the remainder,
                                // and the reading order is the one it always
                                // was. Same shape as `labelled_field`: the
                                // fixed part is measured, the variable part
                                // gets what is left.
                                let badges = {
                                    let gap = ui.spacing().item_spacing.x;
                                    let mut w = 0.0f32;
                                    let mut word = |text: &str| {
                                        let font = egui::TextStyle::Body.resolve(ui.style());
                                        w += gap
                                            + ui.ctx().fonts_mut(|f| {
                                                f.layout_no_wrap(
                                                    text.to_string(),
                                                    font,
                                                    egui::Color32::PLACEHOLDER,
                                                )
                                                .rect
                                                .width()
                                            });
                                    };
                                    if member.admin {
                                        word("admin");
                                    }
                                    if member.muted {
                                        word("muted");
                                    }
                                    if member.account == me {
                                        word("you");
                                    }
                                    if state.verified.contains_key(&member.account) {
                                        w += gap + ui.text_style_height(&egui::TextStyle::Body);
                                    }
                                    w
                                };
                                // A line of text tall, not a button tall: a
                                // `horizontal` is at least `interact_size`
                                // high, which is a finger on a phone, and the
                                // name centred in it left the key some 36
                                // points under the name it identifies --
                                // seen on the device, where every row of the
                                // roster was loose.
                                let line = ui.text_style_height(&egui::TextStyle::Body);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(ui.available_width(), line),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        // Only when there is one to show. `label`
                                        // falls back to the first characters of
                                        // the key, and the whole key is on the
                                        // very next line -- so an unnamed member
                                        // would read as a prefix of themselves,
                                        // above themselves.
                                        if let Some(named) = person.named() {
                                            let room = (ui.available_width() - badges).max(60.0);
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(room, line),
                                                egui::Layout::left_to_right(egui::Align::Center),
                                                |ui| {
                                                    ui.add(egui::Label::new(named).truncate());
                                                },
                                            );
                                        }
                                        if member.admin {
                                            // The exchange attests this one, so it
                                            // may be drawn as a role. A SIP-21
                                            // title may not, which is why it is
                                            // not here.
                                            ui.colored_label(theme.accent, "admin");
                                        }
                                        if member.muted {
                                            // SIP-56: the exchange's own signed
                                            // entry says so; the roster's word.
                                            ui.colored_label(theme.text_muted, "muted")
                                                .on_hover_text("They read, and may not write.");
                                        }
                                        if member.account == me {
                                            ui.colored_label(theme.text_muted, "you");
                                        }
                                        if state.verified.contains_key(&member.account) {
                                            sigil_ui::verified_mark(ui);
                                        }
                                    },
                                );
                                // This is the only thing that identifies
                                // them; everything above it is a claim. In
                                // full on a desktop -- but 44 base58
                                // characters wrap to two lines on a phone and
                                // make the roster a wall of key, so there it
                                // is the short form, with the whole of it a
                                // press away on the row's menu.
                                let shown = if narrow {
                                    sigil_ui::short(&key)
                                } else {
                                    key.clone()
                                };
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(shown).monospace().small(),
                                    )
                                    .selectable(true),
                                );
                            });
                        });
                        match chose {
                            Some(MemberAct::Verify) => {
                                self.pane(at).dialog = Some(Dialog::Verify(member.account));
                                self.send_as(Some(at), Cmd::Attested(member.account));
                            }
                            Some(MemberAct::Kick) => {
                                self.send_as(Some(at), Cmd::Kick(member.account));
                            }
                            Some(MemberAct::Grant(admin)) => self.send_as(
                                Some(at),
                                Cmd::Grant {
                                    who: member.account,
                                    admin,
                                },
                            ),
                            Some(MemberAct::Mute(on)) => self.send_as(
                                Some(at),
                                Cmd::Mute {
                                    who: member.account,
                                    on,
                                },
                            ),
                            Some(MemberAct::Block(blocked)) => self.send_as(
                                Some(at),
                                Cmd::SetBlocked {
                                    who: member.account,
                                    blocked,
                                },
                            ),
                            None => {}
                        }
                    });
                    ui.separator();
                }
                // SIP-56. Anybody may report the room itself; the admins see
                // what was reported, with who reported it, and dismiss.
                ui.add_space(tokens::SPACING_SM);
                // A row with its mark, the shape every other thing to press
                // on this card has: a bare word in a box, alone under a list
                // of rows, read as something left over rather than as the
                // one thing anybody does *to* a room they are in.
                if sigil_ui::icon_item(ui, sigil_ui::Icon::Flag, "Report this room…")
                    .on_hover_text("Tell the room's admins something is wrong with it.")
                    .clicked()
                {
                    let pane = self.pane(at);
                    pane.report_note.clear();
                    pane.report_reason = 1;
                    pane.dialog = Some(Dialog::Report { target: 0 });
                }
                if state.i_am_admin {
                    ui.add_space(tokens::SPACING_MD);
                    ui.separator();
                    ui.add_space(tokens::SPACING_SM);
                    ui.horizontal(|ui| {
                        ui.heading("Reports");
                        if sigil_ui::icon_button_named(
                            ui,
                            sigil_ui::Icon::Refresh,
                            "Read the room's reports from the exchange",
                        )
                        .clicked()
                        {
                            self.send_as(Some(at), Cmd::LoadReports);
                        }
                    });
                    if state.reports.is_empty() {
                        ui.colored_label(theme.text_secondary, "No reports.");
                    }
                    for report in &state.reports {
                        // **Not one row.** The report's sentence and its two
                        // buttons on a single line is 420 points of content
                        // in a 360-point pane, and a right-to-left row that
                        // overflows starts off the left edge -- which is how
                        // "Ada reported message 3 as spam: links" came out as
                        // "rted message 3 as spam: links". The sentence wraps
                        // to the width there is; the buttons go under it.
                        ui.vertical(|ui| {
                            let who = state
                                .people
                                .get(&report.reporter)
                                .and_then(|p| p.named())
                                .unwrap_or_else(|| sigil_ui::short(&report.reporter.to_string()));
                            let what = if report.target == 0 {
                                "the room".to_string()
                            } else {
                                format!("message {}", report.target)
                            };
                            let mut said = format!("{who} reported {what} as {}", report.reason);
                            if !report.note.is_empty() {
                                said.push_str(": ");
                                said.push_str(&report.note);
                            }
                            ui.add(egui::Label::new(said).wrap().sense(egui::Sense::hover()));
                            ui.add_space(tokens::SPACING_XS);
                            // Wrapped, so a third control added here folds to
                            // the next line rather than off the edge.
                            ui.horizontal_wrapped(|ui| {
                                if report.target != 0
                                    && ui
                                        .button("Show")
                                        .on_hover_text("Go to the message reported.")
                                        .clicked()
                                    && let Some(channel) = state.open
                                {
                                    self.send_as(
                                        Some(at),
                                        Cmd::ShowAt {
                                            channel,
                                            seq: report.target,
                                        },
                                    );
                                    ctx.navigator.back();
                                }
                                if sigil_ui::icon_button_named(
                                    ui,
                                    sigil_ui::Icon::Close,
                                    "Dismiss this report",
                                )
                                .clicked()
                                {
                                    self.send_as(Some(at), Cmd::Dismiss(report.id));
                                }
                            });
                            ui.add_space(tokens::SPACING_SM);
                        });
                    }
                }
            });
        AppResponse::default()
    }

    /// The open conversation's name, topic, retention, and how to end it.
    fn settings_view(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        let Some(at) = self.showing_at(ctx) else {
            return AppResponse::default();
        };
        let at = &at;
        let state = self.state_of(Some(at));
        self.fill_settings(at, &state);

        if !bar_has_the_head(ui) {
            ui.horizontal(|ui| {
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                    ctx.navigator.back();
                }
                ui.heading("Channel settings");
            });
        }

        // Yours, whatever your standing here: what this machine says out
        // loud about the conversation.
        if let Some(channel) = state.open {
            let mut muted = ctx.accounts.quiet.is_muted(&at.1, &channel);
            if ui
                .checkbox(&mut muted, "Mute this conversation")
                .on_hover_text(
                    "Nothing in it is said out loud or counted on sigil's icon; its own \
                     row still shows what is waiting.",
                )
                .changed()
            {
                ctx.accounts.quiet.set_muted(&at.1, &channel, muted);
            }
            ui.add_space(tokens::SPACING_SM);
        }

        if !state.i_am_admin {
            ui.colored_label(
                theme.text_secondary,
                "Only an admin can change these. You can still leave.",
            );
        }

        ui.add_space(tokens::SPACING_SM);
        ui.add_enabled_ui(state.i_am_admin, |ui| {
            let (_, set_name) = sigil_ui::labelled_field(
                ui,
                "Name",
                &mut self.panes.entry(at.clone()).or_default().channel_name,
                "what this channel is called",
                Some(sigil_ui::Action::Mark(
                    sigil_ui::Icon::Check,
                    "Set the name",
                )),
            );
            if set_name {
                let name = self.pane(at).channel_name.clone();
                self.send_as(Some(at), Cmd::SetName(name));
            }
            ui.add_space(tokens::SPACING_SM);
            let (_, set_topic) = sigil_ui::labelled_field(
                ui,
                "Topic",
                &mut self.panes.entry(at.clone()).or_default().channel_topic,
                "a line about what it is for",
                Some(sigil_ui::Action::Mark(
                    sigil_ui::Icon::Check,
                    "Set the topic",
                )),
            );
            if set_topic {
                let topic = self.pane(at).channel_topic.clone();
                self.send_as(Some(at), Cmd::SetTopic(topic));
            }

            ui.add_space(tokens::SPACING_MD);
            ui.horizontal(|ui| {
                ui.label("Keep messages for");
                ui.add(
                    egui::DragValue::new(
                        &mut self.panes.entry(at.clone()).or_default().retention_days,
                    )
                    .range(1..=365)
                    .suffix(" days"),
                );
                if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Check, "Set how long").clicked()
                {
                    let days = self.pane(at).retention_days;
                    self.send_as(
                        Some(at),
                        Cmd::SetRetention {
                            secs: days * 24 * 60 * 60,
                            max_entries: 0,
                        },
                    );
                }
            });
            // Narrowing a window deletes, at once. It is not a policy that
            // takes effect later, and somebody shortening it should know that
            // before they press the button rather than after.
            ui.colored_label(
                theme.warning,
                egui::RichText::new(
                    "Shortening this deletes anything already outside the window, \
                     immediately and for everybody.",
                )
                .small(),
            );

            ui.add_space(tokens::SPACING_SM);
            // A row with its mark, the shape everything else pressable in
            // sigil has. **Not the destruction below**, which keeps a
            // button of its own: a row that looks like every other row is
            // the wrong shape for the one act that cannot be undone.
            if sigil_ui::icon_item(ui, sigil_ui::Icon::Refresh, "Mint a new key")
                .on_hover_text(
                    "Everybody present is given a new key. Anybody who has left keeps \
                     what they already had.",
                )
                .clicked()
            {
                self.send_as(Some(at), Cmd::Rotate);
            }
        });

        ui.add_space(tokens::SPACING_LG);
        ui.separator();
        ui.add_space(tokens::SPACING_SM);

        // Leaving and destroying are not the same control and must not look
        // like one. One takes you out; the other ends it for everybody.
        if sigil_ui::icon_item(ui, sigil_ui::Icon::Back, "Leave")
            .on_hover_text("You stop receiving this conversation. Nobody else loses it.")
            .clicked()
        {
            self.send_as(Some(at), Cmd::Leave);
            ctx.navigator.back();
        }

        ui.add_space(tokens::SPACING_SM);
        let pane = self.panes.entry(at.clone()).or_default();
        if !pane.confirming_destroy {
            if ui
                .add(egui::Button::new(
                    egui::RichText::new("Destroy this channel").color(theme.destructive),
                ))
                .clicked()
            {
                pane.confirming_destroy = true;
            }
        } else {
            ui.colored_label(
                theme.destructive,
                "This ends the conversation for everybody in it and cannot be undone.",
            );
            ui.horizontal(|ui| {
                if ui.button("Yes, destroy it").clicked() {
                    self.panes.entry(at.clone()).or_default().confirming_destroy = false;
                    self.send_as(Some(at), Cmd::Destroy);
                    ctx.navigator.back();
                }
                if ui.button("Cancel").clicked() {
                    self.panes.entry(at.clone()).or_default().confirming_destroy = false;
                }
            });
        }
        AppResponse::default()
    }
}

impl ChatApp {
    /// Join the SIP-13 room a call invitation carries.
    ///
    /// **The invitation is a bearer capability**: the secret is the whole of
    /// what joining needs, so anybody who can read the entry can join. That is
    /// SIP-36's design and the reason a call entry wants a short expiry.
    fn join_call(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        ring: &Ring,
        egui_ctx: &egui::Context,
    ) {
        let me = at.0;
        if self.calls.contains_key(&me) {
            return;
        }
        // The signer is minted here rather than kept: a seed held in a second
        // place is a second place to leak it from, and expanding one costs
        // nothing worth caring about.
        let Some((_, unlocked)) = ctx.accounts.unlocked().find(|(k, _)| *k == me) else {
            return;
        };
        let path = unlocked.path().to_path_buf();
        let signer = unlocked.signer();
        // **On the connection this identity already holds**, when there is one.
        // Dialling a second costs a handshake at the moment somebody presses
        // answer, and costs bandwidth for as long as the call lasts: the
        // exchange writes a relayed datagram to every connection an identity
        // holds, so each audio frame would also be written to the chat
        // connection, where nothing reads it.
        //
        // Falling back to dialling rather than refusing: the link may be down
        // and coming back, and a call is worth more than the saving.
        let held = self.sessions.get(at).map(|s| s.connection());
        // SIP-85: at an exchange reached through the home, the call rides the
        // session's tunnelled connection or does not happen -- a dial of its
        // own would go to the default exchange's layers below, which is the
        // wrong exchange, and directly, which is the address the tunnel
        // exists to keep from it. And no introduction is asked for on it:
        // the address the exchange would introduce is the home's.
        let carried = self
            .sessions
            .get(at)
            .is_some_and(|s| s.state().carried.is_some());
        let layers = discovery::layers(discovery::nothing_explicit(), &self.config, Some(&path));
        let reach = if carried {
            match held.filter(|h| h.is_live()) {
                Some(h) => sigil_net::Dial::On(h),
                None => return,
            }
        } else {
            match sigil_net::Dial::borrowed_or(held, layers) {
                Some(reach) => reach,
                None => return,
            }
        };
        let wake = egui_ctx.clone();
        let room = sigil_net::RoomId::new(ring.secret);
        // A direct message goes straight to the other person when both
        // sides will ask for an introduction -- the invitation says the
        // caller will, and the setting says this side may -- and through
        // the room otherwise. A group is a room.
        let handle = match ring.peer {
            Some(peer) => sigil_net::spawn_dm_call(
                reach,
                signer,
                peer,
                room,
                direct_allowed(ring.direct, ctx.accounts.prefs.direct_calls, carried),
                self.call_opts.clone(),
                move || wake.request_repaint(),
            ),
            None => sigil_net::spawn_room(reach, signer, room, self.call_opts.clone(), move || {
                wake.request_repaint()
            }),
        };
        self.calls.insert(
            me,
            Live {
                channel: ring.channel,
                seq: ring.seq,
                handle,
                since: std::time::Instant::now(),
                saw_peer: false,
                cross: false,
            },
        );
    }

    /// Join the call we placed, once somebody has picked it up.
    ///
    /// **Placing a call did not join one.** `join_call` — the only thing that
    /// carries audio — was reached from the Answer button and from nowhere
    /// else, so pressing Call posted an invitation and stopped there. The
    /// person who answered sat alone in a room with their microphone open,
    /// sending to nobody, and the caller had no audio task at all. Nothing
    /// crossed, in either direction, and the interface said "Ringing…" and
    /// then "Connecting…" as though it were working.
    ///
    /// Found by making a call by hand and watching the exchange: a room with
    /// one member in it.
    ///
    /// **On answered, not on placed.** Joining when the call is placed would
    /// be simpler and would open the microphone while it rings — for as long
    /// as it rings, and for a call that is never picked up at all. `answered`
    /// arrives from SIP-36's accept signal, which is a moment later than the
    /// other end's button and is the first instant there is anybody to talk
    /// to.
    ///
    /// From `update` rather than `render`, like `announce_rings`: a caller
    /// who has gone to another conversation, another identity, or another tab
    /// while it rings must still join when it is answered.
    fn join_answered_calls(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        // Collected first: joining borrows `self` mutably, and this is reading
        // the sessions it would be borrowing.
        let mut joining: Vec<(At, Ring)> = Vec::new();
        for (at, session) in &self.sessions {
            let rings = session.ringing();
            if let Some(ring) = to_join(&rings, self.calls.contains_key(&at.0), &self.left) {
                joining.push((at.clone(), ring.clone()));
            }
        }
        for (at, ring) in joining {
            self.join_call(ctx, &at, &ring, egui_ctx);
        }
    }

    /// Answer a ring whose notification Answer was pressed.
    ///
    /// **Opening the conversation is not answering the call.** A ring's
    /// Answer on a phone led into the window and stopped: the conversation
    /// came up and the call went on ringing behind it, so the button
    /// appeared to do nothing at all.
    ///
    /// From `update`, beside [`ChatApp::join_answered_calls`], and for the
    /// same reason -- the press can arrive before there is a ring to answer,
    /// when the window is starting from cold and the session has not
    /// reconnected yet. Answering goes through the same two steps the Answer
    /// button takes, so there is one way a call is answered.
    fn answer_pressed(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        let Some((at, channel, asked)) = self.answering.clone() else {
            return;
        };
        // A ring nobody can find within the window it would have rung for is
        // one that ended while the window was coming up.
        if asked.elapsed() > RING_WINDOW {
            self.answering = None;
            return;
        }
        let Some(session) = self.sessions.get(&at) else {
            return;
        };
        // A ring from another exchange first: it is keyed on its bridge,
        // and answered on the connection this session holds.
        if let Some(cross) = session
            .cross_ring()
            .filter(|c| cross_key(c.bridge) == channel)
        {
            self.answering = None;
            tracing::info!(caller = %cross.caller, "answering a call from another exchange, from its notification");
            self.answer_cross(ctx, &at, cross.caller, egui_ctx);
            return;
        }
        let Some(ring) = session
            .ringing()
            .into_iter()
            .find(|r| r.channel == channel && !r.mine && !r.answered)
        else {
            return;
        };
        self.answering = None;
        self.send_as(
            Some(&at),
            Cmd::Answer {
                channel: ring.channel,
                seq: ring.seq,
            },
        );
        self.join_call(ctx, &at, &ring, egui_ctx);
    }

    /// Put down a call that is over, or that never became one.
    ///
    /// **A call holds the microphone, and until this existed the only thing
    /// that let go of it was a button.** `in_call_ui` draws that button from
    /// the identity on screen, so answering as one identity and then looking at
    /// another took the control away and left the call running: audio still
    /// being captured and sent, and nothing in the window saying so. Quitting
    /// sigil was the only way out, and the microphone light was the only sign.
    ///
    /// Two ways a call stops being one:
    ///
    /// - **It ended.** The task is finished — the far end left, the room went,
    ///   the connection did. The handle is dropped, and the entry written, so
    ///   the transcript says what happened rather than nothing.
    /// - **Nobody came.** A room whose only member is us is a call nobody
    ///   answered, and SIP-36 already fixes how long that is worth waiting:
    ///   `RING_SECS`, after which every reader derives *missed*. Past that, a
    ///   call still connecting is a microphone left open for a call that is
    ///   not going to happen.
    ///
    /// From `update` rather than `render`, for the reason `announce_rings` is:
    /// a call that cannot be seen is exactly the one this has to reach.
    fn end_calls_nobody_is_in(&mut self) {
        let mut over: Vec<PubKey> = Vec::new();
        let mut present_now: Vec<(PubKey, bool)> = Vec::new();
        for (me, live) in self.calls.iter() {
            let call = live.handle.state();
            // The channel says it is over: the other side hung up and
            // said so where every reader sees it, whether or not the
            // path between the two has noticed yet.
            let hung_up = self
                .at_for(*me)
                .map(|at| self.state_of(Some(&at)))
                .is_some_and(|state| state.over.contains(&(live.channel, live.seq)));
            present_now.push((*me, !call.present.is_empty()));
            if call_is_over(
                call.phase,
                call.present.len(),
                call.connecting,
                live.saw_peer,
                hung_up,
                live.since.elapsed(),
            ) {
                over.push(*me);
            }
        }
        // Remembered after the decision, so the pass a peer first appears in
        // is not also the pass that reads them as having left.
        for (me, here) in present_now {
            if here && let Some(live) = self.calls.get_mut(&me) {
                live.saw_peer = true;
            }
        }
        for me in over {
            // The same act as pressing hang up, and recorded the same way: a
            // call that ends because nobody came still happened, and a
            // transcript that says nothing about it is a transcript with a
            // hole where somebody tried to reach you.
            if let Some((channel, seq, seconds)) = self.leave_call(me) {
                let at = self.at_for(me);
                self.send_as(
                    at.as_ref(),
                    Cmd::Hangup {
                        channel,
                        seq,
                        seconds,
                    },
                );
            }
        }
    }

    /// Which session a call belongs to. A call is keyed by identity, and a
    /// command has to go to the session that placed it.
    fn at_for(&self, me: PubKey) -> Option<At> {
        self.sessions.keys().find(|at| at.0 == me).cloned()
    }

    /// Stop carrying audio, and say how long it lasted.
    fn leave_call(&mut self, me: PubKey) -> Option<([u8; 32], u64, u32)> {
        let live = self.calls.remove(&me)?;
        let seconds = live.since.elapsed().as_secs().min(u32::MAX as u64) as u32;
        live.handle.hang_up();
        // A cross-exchange call has no conversation to record its end in:
        // nothing to write, and `None` is what every caller reads as that.
        if live.cross {
            return None;
        }
        self.left.insert((live.channel, live.seq));
        Some((live.channel, live.seq, seconds))
    }

    /// **SIP-39: take a call another exchange carried here.** A session is
    /// opened back toward the caller on the connection this identity already
    /// holds -- our exchange matches it to the bridge it is holding for them
    /// -- and then it is a call like any other. Nothing is dialled: the
    /// caller's exchange did that, which is the whole reason the ring
    /// arrived on this connection and not another.
    fn answer_cross(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        caller: PubKey,
        egui_ctx: &egui::Context,
    ) {
        let me = at.0;
        if self.calls.contains_key(&me) {
            tracing::info!("not answering: already in a call");
            return;
        }
        let Some((_, unlocked)) = ctx.accounts.unlocked().find(|(k, _)| *k == me) else {
            tracing::warn!(%me, "not answering: this identity is not unlocked here");
            return;
        };
        let signer = unlocked.signer();
        let Some(held) = self
            .sessions
            .get(at)
            .map(|s| s.connection())
            .filter(|h| h.is_live())
        else {
            tracing::warn!("not answering: the session holds no live connection");
            return;
        };
        tracing::info!(%caller, "answering a call from another exchange");
        let wake = egui_ctx.clone();
        let handle = sigil_net::spawn_cross_answer(
            sigil_net::Dial::On(held),
            signer,
            caller,
            120,
            self.call_opts.clone(),
            move || wake.request_repaint(),
        );
        self.calls.insert(
            me,
            Live {
                channel: [0u8; 32],
                seq: 0,
                handle,
                since: std::time::Instant::now(),
                saw_peer: false,
                cross: true,
            },
        );
        self.send_as(Some(at), Cmd::CrossRingHandled);
    }

    /// **SIP-39: refuse one.** A refusal is a message of its own -- the
    /// caller is told, rather than left until their exchange gives up on
    /// ours. Posted on the held connection, where the bridge is, and off
    /// the draw: it is a round trip.
    fn decline_cross(&mut self, at: &At, bridge: [u8; 16]) {
        if let Some(held) = self.sessions.get(at).map(|s| s.connection()) {
            tokio::spawn(async move {
                if let Err(e) = sigil_net::decline_cross(&held, bridge).await {
                    tracing::warn!("could not decline the call: {e}");
                }
            });
        }
        self.send_as(Some(at), Cmd::CrossRingHandled);
    }
}

impl ChatApp {
    /// Say out loud that a call is ringing.
    ///
    /// From `update`, not `render`: a call has to reach somebody who is
    /// looking at another tab or at nothing at all, which is most of the time
    /// and is the entire reason the desktop integration exists. Wiring this
    /// into `render` is a mistake already made once here — the ring was drawn
    /// and never announced, and no test caught it, because a missing notifier
    /// produces silence and silence is what a working one looks like from
    /// inside a test.
    /// Tell the platform whether a call is up, and with whom.
    ///
    /// **Derived from `self.calls`, not hooked onto the two sites that change
    /// it.** There is one place a call is joined and one place it is left,
    /// and that is true today; a third would be a platform that thinks a call
    /// is still running, which on Android is a notification that will not go
    /// away. Reading the state each pass cannot drift from it.
    ///
    /// Said only when it differs from what was last said, because on Android
    /// this starts a foreground service with a notification and doing that
    /// sixty times a second is not a call, it is a fault.
    fn announce_calling(&mut self, ctx: &mut AppContext<'_>) {
        // Whoever the call is with, as the conversation is named -- the same
        // label the transcript uses, so the notification and the screen agree.
        let now = self.calls.iter().next().and_then(|(me, live)| {
            let at = self.at_for(*me)?;
            let state = self.state_of(Some(&at));
            state
                .conversations
                .iter()
                .find(|c| c.channel == live.channel)
                .map(|c| c.label.clone())
        });
        // A call whose conversation this window cannot name is still a call:
        // the service exists to keep the process alive, and a missing label
        // must not be the reason it is not started.
        let now = match (self.calls.is_empty(), now) {
            (true, _) => None,
            (false, Some(label)) => Some(label),
            (false, None) => Some(String::new()),
        };
        if now != self.told_calling {
            ctx.notify.calling(now.as_deref());
            self.told_calling = now;
        }
    }

    /// The deciding halves, for the tests that hand them lists. The walks
    /// live in [`announce`].
    #[cfg(test)]
    fn announce_rings_in(&mut self, ctx: &mut AppContext<'_>, fresh: Vec<(String, Target)>) {
        self.announcer.frame(Vec::new(), &ctx.accounts.quiet);
        self.announcer.rings_in(ctx.notify, fresh);
        self.asked.extend(self.announcer.take_wants());
    }

    #[cfg(test)]
    fn announce_mentions_in(
        &mut self,
        ctx: &mut AppContext<'_>,
        held: usize,
        found: Vec<(At, String, Vec<session::Mention>)>,
    ) {
        self.announcer.frame(Vec::new(), &ctx.accounts.quiet);
        self.announcer
            .mentions_in(ctx.notify, ctx.unfocused, held, found);
        self.asked.extend(self.announcer.take_wants());
    }

    #[cfg(test)]
    fn announce_arrivals_in(
        &mut self,
        ctx: &mut AppContext<'_>,
        held: usize,
        found: Vec<(At, String, Vec<session::Arrival>)>,
    ) {
        self.announcer.frame(Vec::new(), &ctx.accounts.quiet);
        self.announcer
            .arrivals_in(ctx.notify, ctx.unfocused, held, found);
        self.asked.extend(self.announcer.take_wants());
    }

    /// The rings walk, with the withdrawals, as a frame runs it.
    #[cfg(test)]
    fn announce_rings(&mut self, ctx: &mut AppContext<'_>) {
        self.announcer.frame(
            self.sessions
                .iter()
                .map(|(at, s)| (at.clone(), s.watch()))
                .collect(),
            &ctx.accounts.quiet,
        );
        self.announcer.rings_only(ctx.notify);
        self.asked.extend(self.announcer.take_wants());
    }

    /// A call we are placing, drawn where a call arriving is drawn.
    ///
    /// **The same banner, because it is the same kind of thing**: a call that
    /// has not started, and one control to do something about it. It used to
    /// be a bare row under the banner — the words "Ringing…" and a button —
    /// while everything else about a call (the ring, Answer, Decline, the
    /// in-call bar) was in the elevated frame above it. One state of one call
    /// in a different place from the rest.
    ///
    /// "Answer" on a call you are making is nonsense, so this shape has one
    /// button and not two, and it names the conversation being called rather
    /// than a caller.
    /// SIP-39's ring. See `ringing_ui`, which this is the first branch of.
    fn cross_ring_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        cross: &CrossRing,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let named = state
            .people
            .get(&cross.caller)
            .map(|p| p.label(&cross.caller))
            .unwrap_or_else(|| cross.caller.to_string());
        // The one thing about it that is different, said in words: a caller
        // from another exchange is one this exchange cannot vouch for.
        match ring_card(ui, theme, &cross.caller, &named, "from another exchange") {
            Some(RingPress::Answer) => self.answer_cross(ctx, at, cross.caller, ui.ctx()),
            Some(RingPress::Decline) => self.decline_cross(at, cross.bridge),
            None => {}
        }
    }

    /// **SIP-44: the other party's account has moved.** Asked of the registry
    /// once per direct message opened -- a transcript says so only where the
    /// move was written into a channel both are in, and a contact who moved
    /// while nothing was said reads as merely silent. The old key is still
    /// this conversation's, so what is offered is a conversation with the
    /// new one, not a rewrite of this.
    fn succeeded_ui(&mut self, at: &At, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
        let Some(peer) = state
            .conversations
            .iter()
            .find(|c| Some(c.channel) == state.open)
            .and_then(|c| c.peer)
        else {
            return;
        };
        if !state.succeeded.contains_key(&peer) && self.asked_succession.insert((at.clone(), peer))
        {
            self.send_as(Some(at), Cmd::SuccessionOf(peer));
        }
        let Some(Some(successor)) = state.succeeded.get(&peer) else {
            return;
        };
        let named = state
            .people
            .get(&peer)
            .map(|p| p.label(&peer))
            .unwrap_or_else(|| sigil_ui::message::short(&peer.to_string()));
        let successor = *successor;
        egui::Frame::NONE
            .fill(theme.surface_elevated)
            .corner_radius(tokens::RADIUS_LG)
            .inner_margin(egui::Margin::same(tokens::SPACING_MD as i8))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(format!(
                        "{named}'s account is now {}",
                        sigil_ui::message::short(&successor.to_string())
                    ))
                    .wrap(),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "The exchange holds their own word for it. What was said here \
                             stays with the old key; write to them at the new one.",
                        )
                        .small()
                        .color(theme.text_secondary),
                    )
                    .wrap(),
                );
                if sigil_ui::icon_item(ui, sigil_ui::Icon::Compose, "Write to them there").clicked()
                {
                    self.send_as(Some(at), Cmd::AddContact(successor, String::new()));
                    self.send_as(Some(at), Cmd::OpenDm(successor));
                }
            });
        ui.add_space(tokens::SPACING_SM);
    }

    fn calling_ui(
        &mut self,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) -> bool {
        // Only while it is ringing: once the other side has answered, the
        // call's own banner says what is happening, and "Ringing…" under
        // "In a call" was two answers to one question.
        let Some(ring) = state.ringing.iter().find(|r| r.mine && !r.answered) else {
            return false;
        };
        let (channel, seq) = (ring.channel, ring.seq);
        // The person being called, when there is one person: a direct message
        // has a peer, a group has a name and no single face.
        let peer = state
            .conversations
            .iter()
            .find(|c| Some(c.channel) == state.open)
            .and_then(|c| c.peer);
        let me = at.0;
        egui::Frame::NONE
            .fill(theme.surface_elevated)
            .corner_radius(tokens::RADIUS_LG)
            .inner_margin(egui::Margin::same(tokens::SPACING_MD as i8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(peer) = peer {
                        sigil_ui::identicon(ui, &peer.to_string(), tokens::AVATAR_MD);
                        ui.add_space(tokens::SPACING_SM);
                    }
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new(format!("Calling {}", ring.label)).strong());
                        ui.colored_label(
                            theme.text_secondary,
                            egui::RichText::new("Ringing…").small(),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // "Cancel" and not "Hang up": giving up on a call
                        // nobody has taken is not the same act as ending one in
                        // progress.
                        if ui.button("Cancel").clicked() {
                            let seconds = self.leave_call(me).map(|(_, _, s)| s).unwrap_or(0);
                            self.send_as(
                                Some(at),
                                Cmd::Hangup {
                                    channel,
                                    seq,
                                    seconds,
                                },
                            );
                        }
                    });
                });
            });
        true
    }

    /// A call ringing, and the two things to do about it.
    fn ringing_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) -> bool {
        // **Incoming first.** Answering matters more than cancelling, and the
        // two cannot both be true of one conversation without somebody having
        // called somebody who was already calling them.
        //
        // SIP-39 first of all: a call carried here from another exchange has
        // no conversation of its own to ring in, so this is the one place it
        // can. It is drawn as any other ring -- who, their key in full, and
        // two answers -- and says where it is from, because that is the one
        // thing about it that is different.
        if let Some(cross) = &state.cross_ring {
            let cross = cross.clone();
            self.cross_ring_ui(ctx, at, state, &cross, ui, theme);
            return true;
        }
        let Some(ring) = state.ringing.iter().find(|r| !r.mine && !r.answered) else {
            return self.calling_ui(at, state, ui, theme);
        };
        let named = state
            .people
            .get(&ring.from)
            .map(|p| p.label(&ring.from))
            .unwrap_or_else(|| ring.from.to_string());
        match ring_card(ui, theme, &ring.from, &named, &format!("in {}", ring.label)) {
            Some(RingPress::Decline) => {
                self.send_as(
                    Some(at),
                    Cmd::Decline {
                        channel: ring.channel,
                        seq: ring.seq,
                    },
                );
            }
            Some(RingPress::Answer) => {
                let ring = ring.clone();
                self.send_as(
                    Some(at),
                    Cmd::Answer {
                        channel: ring.channel,
                        seq: ring.seq,
                    },
                );
                self.join_call(ctx, at, &ring, ui.ctx());
            }
            None => {}
        }
        true
    }

    /// The bar shown while audio is actually flowing.
    ///
    /// **Every call, not the shown identity's.** A call is a state of the
    /// window — a microphone is open — and this used to be drawn from
    /// `self.calls.get(&at.0)`, so answering as one identity and then looking
    /// at another took the bar away and with it the only control that ends a
    /// call. The audio went on being captured and sent with nothing on screen
    /// admitting it. See `end_calls_nobody_is_in`, which is the other half of
    /// that: this makes a call visible, and that one stops it being possible
    /// to leave one running by accident.
    fn in_call_ui(&mut self, at: &At, ui: &mut egui::Ui, theme: &ColorTheme) {
        for me in self.calls.keys().copied().collect::<Vec<_>>() {
            self.one_call_ui(me, at, ui, theme);
        }
    }

    /// One call's bar, wherever the reader happens to be.
    fn one_call_ui(&mut self, me: PubKey, at: &At, ui: &mut egui::Ui, theme: &ColorTheme) {
        let Some(live) = self.calls.get(&me) else {
            return;
        };
        let call = live.handle.state();
        let seconds = live.since.elapsed().as_secs();
        // Whose call it is, when it is not the identity being looked at. Named
        // rather than implied: a hang-up button that ends somebody else's call
        // has to say whose.
        let elsewhere = (me != at.0).then(|| {
            self.sessions
                .iter()
                .find(|(k, _)| k.0 == me)
                .map(|(k, s)| s.state().mine.label(&k.0))
                // No session to ask, which is a moment rather than a state --
                // the same short key their own header shows, and not the
                // whole one, which would make the banner the one place a
                // forty-four character key still turned up.
                .unwrap_or_else(|| sigil_ui::short(&me.to_string()))
        });
        egui::Frame::NONE
            .fill(theme.surface_elevated)
            .corner_radius(tokens::RADIUS_LG)
            .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let up = matches!(call.phase, sigil_net::Phase::Live);
                    sigil_ui::dot(
                        ui,
                        up,
                        theme.success,
                        theme.warning,
                        if up { "connected" } else { "connecting" },
                    );
                    ui.colored_label(
                        if up { theme.success } else { theme.warning },
                        match (&elsewhere, up) {
                            (Some(who), true) => format!("In a call as {who}"),
                            (Some(who), false) => format!("Connecting… as {who}"),
                            (None, true) => "In a call".to_string(),
                            (None, false) => "Connecting…".to_string(),
                        },
                    );
                    ui.colored_label(
                        theme.text_muted,
                        format!("{:02}:{:02}", seconds / 60, seconds % 60),
                    );
                    // Which way the audio is going, once that is settled.
                    // Said in a word because it is the one fact about a
                    // call a person can do something about -- and, for a
                    // while, the one the field test needs to read.
                    match call.path {
                        Some(sigil_net::Path::Direct) => {
                            ui.colored_label(
                                theme.text_muted,
                                egui::RichText::new("direct").small(),
                            )
                            .on_hover_text(
                                "Connected straight to them: the exchange introduced you \
                                     and is not carrying the call.",
                            );
                        }
                        Some(sigil_net::Path::Relayed) => {
                            ui.colored_label(
                                theme.text_muted,
                                egui::RichText::new("via exchange").small(),
                            )
                            .on_hover_text(format!(
                                "Relayed by the exchange: {}.",
                                call.why
                                    .as_deref()
                                    .unwrap_or("no introduction was asked for")
                            ));
                        }
                        None => {}
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // The struck-through handset, in the destructive
                        // colour. It carries the word "Hang up" for anything
                        // that cannot see a shape.
                        if sigil_ui::icon_button_tinted(
                            ui,
                            sigil_ui::Icon::HangUp,
                            Some(theme.destructive),
                        )
                        .clicked()
                            && let Some((channel, seq, seconds)) = self.leave_call(me)
                        {
                            // To the session whose call it is, which is not
                            // necessarily the one on screen.
                            let whose = self.at_for(me);
                            self.send_as(
                                whose.as_ref(),
                                Cmd::Hangup {
                                    channel,
                                    seq,
                                    seconds,
                                },
                            );
                        }
                    });
                });
            });
    }
}

impl ChatApp {
    /// Which devices act for this account.
    ///
    /// # Why this is a screen and not a settings row
    ///
    /// An epoch key arrives sealed against a **one-time** prekey, and opening
    /// it spends that prekey. Ask the exchange for the same envelope tomorrow
    /// and it hands over the same bytes, and they will not open. So the copy on
    /// this disk is the only one that will ever exist — and a second linked
    /// device is the only backup of it there can be.
    ///
    /// Losing this store with nothing else linked loses those conversations
    /// permanently, for everybody in them and not only for you.
    /// Searching what has been said: the box, and what it found.
    ///
    /// A card of its own on a phone, reached by the list's magnifier. The
    /// box is the same one a wide pane keeps above its list -- one search,
    /// drawn in two places -- and choosing a hit leaves the card for the
    /// conversation it is in, which is what pressing a result means.
    /// The card behind your own mark: who you are, where you are, and
    /// everything that is done to this identity rather than to a
    /// conversation -- with the way to the other things sigil does on it.
    ///
    /// # Why a card and not the menu it replaced
    ///
    /// On a phone the identity menu was a popup hanging off a mark in the
    /// corner, and it had grown a scroll bar. The way between Chat, Exchange
    /// and Phone was in a *second* popup, behind the title. Two menus and
    /// neither of them a place you could go back to. A card has a name in
    /// the bar, a Back that means something, and room to put a face at the
    /// top of it.
    ///
    /// # What is tapped to change what
    ///
    /// The three things at the head are the three things somebody comes here
    /// to change, and each is its own control: the mark and the name open
    /// the profile, the handle claims or gives up a name at the exchange.
    /// **Nothing here is a caption.** A line of text that cannot be pressed,
    /// beside two that can, is the one somebody presses.
    fn me_view(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        let Some(at) = self.showing_at(ctx) else {
            return AppResponse::default();
        };
        let at = &at;
        let state = self.state_of(Some(at));
        let me = at.0;
        let key = me.to_string();
        if !bar_has_the_head(ui) {
            ui.horizontal(|ui| {
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                    ctx.navigator.back();
                }
                ui.heading("Settings");
            });
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.me_head_ui(ctx, at, &state, ui, &theme, &key);
                ui.add_space(tokens::SPACING_LG);
                self.me_keys_ui(&state, ui, &theme, &key);
                ui.add_space(tokens::SPACING_MD);
                self.me_apps_ui(ctx, ui, &theme);
                ui.add_space(tokens::SPACING_MD);
                self.me_settings_ui(ctx, at, &state, ui, &theme);
            });
        AppResponse::default()
    }

    /// The head: the mark, the name, the handle, the link -- centred, and
    /// every one of them a control.
    fn me_head_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        key: &str,
    ) {
        let me = at.0;
        let mut edit_profile = false;
        let mut claim_name = false;
        ui.vertical_centered(|ui| {
            ui.add_space(tokens::SPACING_MD);
            // **The mark is a button.** An avatar is where everybody has
            // learned to press to change what others see of them, and the
            // mark itself is not editable -- it is the key drawn -- so a
            // press on it opens the one thing about you that is.
            let mark = ui
                .scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
                    sigil_ui::identicon(ui, key, tokens::AVATAR_XL);
                })
                .response
                .on_hover_text("Your name and title, as others see them");
            mark.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Edit your profile")
            });
            if mark.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            edit_profile |= mark.clicked();
            ui.add_space(tokens::SPACING_SM);

            // Your name. With none published this is the head of your key,
            // which is what everybody else sees too -- so pressing it is
            // how a name gets published.
            let name = ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(state.mine.label(&me))
                            .heading()
                            .color(theme.text_primary),
                    )
                    .truncate()
                    .sense(egui::Sense::click()),
                )
                .on_hover_text("Your name and title, as others see them");
            if name.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            edit_profile |= name.clicked();

            // Where you are reachable. Registered: `ada@trunk.exchange`,
            // and pressing it gives the name up. Unregistered: the domain
            // alone, and pressing it claims one.
            match &state.mine.handle {
                Some(handle) => {
                    let row = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(handle).color(theme.text_secondary),
                            )
                            .truncate()
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_text("The name you are reachable at here");
                    if row.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    // **To the dialog, not to the release.** This line sits
                    // directly under your own name, a thumb's width from it,
                    // and giving a name up cannot be undone -- somebody else
                    // may take it. One stray tap must not be able to do that,
                    // so the card opens the place where both claiming another
                    // and giving this one up are deliberate.
                    claim_name |= row.clicked();
                }
                None => {
                    // **The domain the handle would have had.** `state.domain`
                    // is what `Chat::handle` composes `name@domain` from, so
                    // the line before a name is claimed and the line after it
                    // cannot disagree about where you are. With none -- a
                    // connection made to a bare address -- the switcher's own
                    // label is the best there is.
                    let domain = state
                        .domain
                        .clone()
                        .unwrap_or_else(|| self.exchange_label(me, &at.1));
                    let row = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(format!("@{domain}")).color(theme.text_muted),
                            )
                            .truncate()
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_text("Claim a name at this exchange");
                    if row.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    claim_name |= row.clicked();
                }
            }

            // What others see of you, and -- when the link is down -- the
            // one thing worth doing about it. A colour says nothing to
            // somebody who cannot see it, so the word is here too.
            ui.add_space(tokens::SPACING_XS);
            match state.link {
                LinkState::Up => {
                    let (word, colour) = if ctx.away {
                        ("away", theme.text_muted)
                    } else {
                        ("active", theme.success)
                    };
                    ui.colored_label(colour, egui::RichText::new(word).small())
                        .on_hover_text("what others see of you");
                }
                _ => {
                    let colour = match state.link {
                        LinkState::Gone => theme.link_gone,
                        _ => theme.link_retrying,
                    };
                    ui.colored_label(colour, egui::RichText::new(state.link.word()).small());
                    if sigil_ui::icon_item(ui, sigil_ui::Icon::Refresh, "Reconnect").clicked() {
                        self.send_as(Some(at), Cmd::Reconnect);
                    }
                }
            }
        });
        if edit_profile {
            self.open_profile(at, state);
        }
        if claim_name {
            self.panes.entry(at.clone()).or_default().dialog = Some(Dialog::Name);
        }
    }

    /// The keys, short, with the way to the whole of each.
    ///
    /// **Short and copyable, not long and selectable.** These are 44
    /// characters of base58 and nobody reads one: what anybody does with
    /// their own key is send it to somebody, and what they do with the
    /// exchange's is compare it -- which the head and tail settle. The whole
    /// of it goes to the clipboard, and is on the hover for a pointer.
    fn me_keys_ui(&mut self, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme, key: &str) {
        let mut rows: Vec<(&str, String, &str, &str)> = vec![(
            "You",
            key.to_string(),
            "Copy your key",
            "the only thing that identifies you to somebody who wants to write to you",
        )];
        if let Some(exchange) = state.exchange {
            rows.push((
                "Exchange",
                exchange.to_string(),
                "Copy the exchange's key",
                "the exchange this conversation list belongs to",
            ));
        }
        for (said, whole, button, hover) in rows {
            ui.horizontal(|ui| {
                ui.colored_label(theme.text_muted, egui::RichText::new(said).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Copy, button).clicked() {
                        ui.ctx().copy_text(whole.clone());
                    }
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(sigil_ui::short(&whole))
                                .monospace()
                                .small(),
                        )
                        .truncate(),
                    )
                    .on_hover_text(format!("{whole}\n{hover}"));
                });
            });
        }
        // SIP-85: the exchange sees the home's address, not this machine's.
        if let Some(home) = &state.carried {
            ui.colored_label(
                theme.text_muted,
                egui::RichText::new(format!("through {home}")).small(),
            )
            .on_hover_text(
                "your home carries this connection: the exchange sees your home's address \
                 and your own key, never where you are",
            );
        }
    }

    /// The other things sigil does, and the way to them.
    ///
    /// **The rail, for a screen with no room for one.** On a desktop these
    /// are icons down the side; on a phone they were behind the title, in a
    /// popup nobody found. The one you are on is filled, so the card says
    /// where you are as well as where you can go.
    fn me_apps_ui(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui, theme: &ColorTheme) {
        let siblings = ctx.navigator.siblings().to_vec();
        if siblings.len() < 2 {
            return;
        }
        ui.separator();
        ui.colored_label(theme.text_muted, egui::RichText::new("sigil").small());
        for s in siblings {
            let said = if s.badge == 0 {
                s.title.clone()
            } else {
                format!("{} ({})", s.title, s.badge)
            };
            if sigil_ui::icon_item_as(ui, s.icon, &said, s.active).clicked() && !s.active {
                ctx.navigator.switch_to(s.id);
            }
        }
    }

    /// Everything done *to* this identity, and the one setting that is not
    /// about an identity at all.
    fn me_settings_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        ui.separator();
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new("This identity").small(),
        );
        if sigil_ui::icon_item(ui, sigil_ui::Icon::Pencil, "Edit your profile")
            .on_hover_text("Your name and title, as others see them")
            .clicked()
        {
            self.open_profile(at, state);
        }
        if sigil_ui::icon_item(ui, sigil_ui::Icon::Device, "Your devices")
            .on_hover_text("Every key that acts as you, and what happens if you lose this one")
            .clicked()
        {
            self.send_as(Some(at), Cmd::Devices);
            self.send_as(Some(at), Cmd::BackupStatus);
            ctx.navigator.push_here(Route::Devices);
        }
        // **An exchange is not a preference.** The identity is the same key
        // everywhere and nothing else is -- conversations, channel keys and
        // SIP-17 counters belong to one exchange and do not move. So the row
        // says what it opens: adding one, which is much closer to adding an
        // account than to changing a setting. *Switching* between the ones
        // already added is in the corner of the chats list, where changing
        // it changes the list under it.
        if sigil_ui::icon_item(ui, sigil_ui::Icon::Public, "Add an exchange")
            .on_hover_text(
                "Your key is the same everywhere. Conversations are not: they belong to the \
                 exchange they were had at.",
            )
            .clicked()
        {
            self.panes.entry(at.clone()).or_default().dialog = Some(Dialog::Exchange);
        }
        if sigil_ui::icon_item(ui, sigil_ui::Icon::Switch, "Switch identity")
            .on_hover_text(
                "Choose another identity. This one stays open — its messages keep arriving \
                 and a call on it keeps running.",
            )
            .clicked()
        {
            self.switching = true;
        }

        ui.separator();
        ui.colored_label(theme.text_muted, egui::RichText::new("Everywhere").small());
        // Not this identity's: quiet is the person's, and it covers every
        // identity this window holds and every ring on any of them.
        let dnd = ctx.accounts.quiet.dnd;
        let said = if dnd {
            "Do not disturb"
        } else {
            "Notifications"
        };
        let icon = if dnd {
            sigil_ui::Icon::BellOff
        } else {
            sigil_ui::Icon::Bell
        };
        if sigil_ui::icon_item_as(ui, icon, said, dnd)
            .on_hover_text(if dnd {
                "Nothing is said out loud on any identity, rings included."
            } else {
                "Press to silence everything on every identity, rings included."
            })
            .clicked()
        {
            ctx.accounts.quiet.set_dnd(!dnd);
        }
        ui.add_space(tokens::SPACING_MD);
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(format!("sigil {}", env!("CARGO_PKG_VERSION"))).small(),
        );
    }

    fn search_view(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        let Some(at) = self.showing_at(ctx) else {
            return AppResponse::default();
        };
        let at = &at;
        let state = self.state_of(Some(at));
        let now = self.now();
        if !bar_has_the_head(ui) {
            ui.horizontal(|ui| {
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                    ctx.navigator.back();
                }
                ui.heading("Search");
            });
        }
        let before = self.pane(at).chosen;
        self.search_ui(at, &state, ui);
        ui.add_space(tokens::SPACING_SM);
        // **An empty box has not failed to find anything.** The card opens
        // with nothing typed, and "Nothing here matched." under an empty
        // box is an answer to a question nobody has asked yet. What it can
        // reach is worth saying there instead.
        if self.pane(at).searching.trim().is_empty() {
            ui.colored_label(theme.text_muted, egui::RichText::new(ONLY_HERE).small());
        } else {
            self.hits_ui(at, &state, ui, &theme, now);
        }
        // A hit was pressed, or Enter took the newest: the conversation is
        // what was asked for, and it is on the card behind this one.
        if self.pane(at).chosen != before {
            ctx.navigator.back();
        }
        AppResponse::default()
    }

    fn devices_view(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        let Some(at) = self.showing_at(ctx) else {
            return AppResponse::default();
        };
        let at = &at;
        let state = self.state_of(Some(at));

        // Ask again. On a phone this is in the bar's corner, where a
        // view's one action goes; here it would be the only thing left of a
        // row whose other two halves the bar has taken.
        if !bar_has_the_head(ui) {
            ui.horizontal(|ui| {
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                    ctx.navigator.back();
                }
                ui.heading("Devices");
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Refresh).clicked() {
                    self.send_as(Some(at), Cmd::Devices);
                    self.send_as(Some(at), Cmd::BackupStatus);
                }
            });
        }

        // **The rest of it scrolls.** This pane is four sections and a
        // backup, and with the 24 words showing it reaches 1825 points --
        // more than twice a phone's screen, with nothing to scroll. The
        // words are the whole of what opens the backup and they were *off
        // the bottom*, along with Restore and Drop; Members and the
        // directory have had one all along, and this one was simply never
        // given it. The heading stays put above, as theirs do.
        let scrolled = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.devices_body(ctx, at, &state, ui, &theme));
        scrolled.inner
    }

    /// Everything under the Devices heading. Split out so the heading does
    /// not scroll with it.
    #[allow(clippy::too_many_arguments)]
    fn devices_body(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) -> AppResponse {
        if state.linked == Some(false) {
            // Otherwise learned only by being refused as a stranger to every
            // conversation this client can see, which reads as everything
            // being broken rather than as this one fact.
            ui.colored_label(
                theme.destructive,
                "This device has been revoked. It can no longer act for the account, and \
                 nothing it sends will be accepted.",
            );
        }

        ui.add_space(tokens::SPACING_SM);
        let backed_up = state.backup.as_ref().is_some_and(|b| b.held.is_some());
        if state.devices.len() <= 1 && !backed_up {
            ui.colored_label(
                theme.warning,
                "Nothing else is linked. The conversations on this machine cannot be \
                 recovered from the exchange — opening a key spends it, so what is here \
                 is the only copy. Back it up below, or link a second device.",
            );
            ui.add_space(tokens::SPACING_SM);
        }

        for device in &state.devices {
            let key = device.device.to_string();
            // The ring card's shape, for the ring card's reason: the key in
            // a column beside a named button is wider than a phone, and the
            // button was drawn over it. The row carries what is short --
            // the mark, the short key, whether it is this one -- and the
            // revocation at its end; the whole key and the dates have rows
            // of their own, wrapped.
            let mut revoke = false;
            ui.horizontal(|ui| {
                sigil_ui::identicon(ui, &key, tokens::AVATAR_SM);
                ui.add_space(tokens::SPACING_SM);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !device.is_this_one {
                        revoke = sigil_ui::icon_button_as_named(
                            ui,
                            sigil_ui::Icon::Close,
                            "Revoke",
                            Some(theme.destructive),
                            false,
                        )
                        .on_hover_text(
                            "It stops acting for you. It keeps every key it was already \
                             given, so rotate anything it could read.",
                        )
                        .clicked();
                        ui.add_space(tokens::SPACING_SM);
                    }
                    let line = ui.text_style_height(&egui::TextStyle::Body);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), line),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.label(sigil_ui::message::short(&key));
                            if device.is_this_one {
                                ui.colored_label(theme.accent, "this device");
                            }
                        },
                    );
                });
            });
            ui.add(
                egui::Label::new(egui::RichText::new(&key).monospace().small())
                    .wrap()
                    .selectable(true),
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!(
                        "linked {} · credential expires {}",
                        sigil_ui::brief(device.added, self.now()),
                        sigil_ui::brief(device.not_after, self.now())
                    ))
                    .small()
                    .color(theme.text_muted),
                )
                .wrap(),
            );
            if revoke {
                self.send_as(Some(at), Cmd::RevokeDevice(device.device));
            }
            ui.separator();
        }

        ui.add_space(tokens::SPACING_MD);
        ui.heading("Link another device");
        // **Small, under its heading**: the prose on this card explains a
        // security-relevant act and is worth every word, and at body size
        // three paragraphs of it were the whole of a phone's screen before
        // a single field. The shape the Backup section below has had all
        // along.
        ui.colored_label(
            theme.text_secondary,
            egui::RichText::new(
                "Write a credential here, then give it to the other device. It names both \
                 keys in the clear, so hand it over the way you would hand over a key.",
            )
            .small(),
        );
        {
            let (_, write) = sigil_ui::labelled_field(
                ui,
                "Its key",
                &mut self.panes.entry(at.clone()).or_default().linking,
                "the new device's key, in base58",
                Some(sigil_ui::Action::Mark(
                    sigil_ui::Icon::Check,
                    "Write credential",
                )),
            );
            if write {
                // A phone shows its key as `sqx-device:<key>` (SIP-47), and
                // somebody typing what they see should not have to trim it.
                let typed = self.pane(at).linking.trim().to_string();
                let typed = typed
                    .strip_prefix("sqx-device:")
                    .unwrap_or(&typed)
                    .trim()
                    .to_string();
                match typed.parse::<PubKey>() {
                    Ok(device) => {
                        self.pane(at).linking.clear();
                        self.send_as(Some(at), Cmd::LinkDevice { device, days: 90 });
                    }
                    Err(e) => self.pane(at).add_trouble = Some(format!("that is not a key: {e}")),
                }
            }
        }
        if let Some(credential) = &state.credential {
            ui.add_space(tokens::SPACING_SM);
            ui.add(
                egui::TextEdit::multiline(&mut credential.clone())
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Copy, "Copy").clicked() {
                ui.ctx().copy_text(credential.clone());
            }
            // **Where the phone goes next.** A phone paired by SIP-47 has
            // been told nothing but that it is registered; this names the
            // account and every exchange this identity is at, as one string
            // it can scan or type. Public facts, both -- the registration
            // is the act of trust, and it is already signed.
            if let Some(pair) = self.pairing_string(at) {
                ui.add_space(tokens::SPACING_SM);
                ui.colored_label(
                    theme.text_secondary,
                    "Then show the other device where to go — a phone scans this:",
                );
                sigil_ui::qr(ui, &pair, 160.0);
                ui.add(
                    egui::Label::new(egui::RichText::new(&pair).monospace().small())
                        .selectable(true),
                );
                if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Copy, "Copy where to go")
                    .clicked()
                {
                    ui.ctx().copy_text(pair);
                }
            }
        }

        // **The phone's half of SIP-47.** This device showed its key, another
        // device registered it and showed where to go; typed or scanned here,
        // this device joins the exchanges named and claims the account by
        // finding itself in its list.
        ui.add_space(tokens::SPACING_XL);
        ui.heading("Where the other device sent you");
        ui.colored_label(
            theme.text_secondary,
            egui::RichText::new(
                "If another of your devices read this one's key and showed you an \
                 sqx-pair: string — or you know your name at an exchange — put it here.",
            )
            .small(),
        );
        ui.add_space(tokens::SPACING_SM);
        let (_, go) = sigil_ui::labelled_field(
            ui,
            "",
            &mut self.panes.entry(at.clone()).or_default().pairing,
            "sqx-pair:<account>@<domain>, or name@domain",
            Some(sigil_ui::Action::Mark(sigil_ui::Icon::Check, "Go there")),
        );
        if go {
            let typed = self.pane(at).pairing.trim().to_string();
            match pairing::parse(&typed) {
                Ok((owner, domains)) => {
                    self.pane(at).pairing.clear();
                    // The exchanges, so a session opens at each; the claim
                    // is asked of every session this identity holds, and an
                    // exchange that has not heard of the registration says so.
                    let index = ctx.accounts.active_index();
                    for domain in &domains {
                        ctx.accounts.add_exchange(index, domain, None);
                    }
                    let ats: Vec<At> = self
                        .sessions
                        .keys()
                        .filter(|k| k.0 == at.0)
                        .cloned()
                        .collect();
                    for session_at in ats {
                        self.send_as(Some(&session_at), Cmd::ClaimAccount(owner.clone()));
                    }
                    self.pane(at).claim_pending = Some(owner);
                }
                Err(why) => self.pane(at).add_trouble = Some(why),
            }
        }
        if let Some(owner) = &self.pane(at).claim_pending {
            ui.colored_label(
                theme.text_muted,
                egui::RichText::new(format!(
                    "asking each exchange to list this device under {owner}…"
                ))
                .small(),
            );
        }

        // **The other half.** The screen could write a credential and had
        // nowhere to present one, so the second device of an account could be
        // named and never enrolled — and a linked device is the only backup an
        // epoch key can have.
        ui.add_space(tokens::SPACING_XL);
        ui.heading("Use a credential");
        ui.colored_label(
            theme.text_secondary,
            egui::RichText::new(
                "If another of your devices wrote one for this one, paste it here. The \
                 exchange checks it names this very device, so one somebody found is one \
                 they cannot use.",
            )
            .small(),
        );
        ui.add_space(tokens::SPACING_SM);
        let (_, register) = sigil_ui::labelled_field(
            ui,
            "",
            &mut self.panes.entry(at.clone()).or_default().presenting,
            "the credential your other device wrote, in base58",
            Some(sigil_ui::Action::Mark(
                sigil_ui::Icon::Check,
                "Register this device",
            )),
        );
        if register {
            let typed = self.pane(at).presenting.trim().to_string();
            if !typed.is_empty() {
                self.pane(at).presenting.clear();
                self.send_as(Some(at), Cmd::RegisterSelf(typed));
            }
        }
        // Said before it is needed, not after it is missed: registering makes
        // this device act for the account and hands it no keys at all.
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(
                "Registering does not bring any conversation with it. An epoch key is \
                 sealed to a device, so the other one has to hand them over before \
                 anything already said can be read here.",
            )
            .small(),
        );

        self.backup_ui(at, state, ui, theme);
        self.succession_ui(at, state, ui, theme);
        AppResponse::default()
    }

    /// SIP-48: the account's sealed backup at the exchange. A key the exchange
    /// never sees, shown once as 24 words to write down; a copy written on
    /// demand; restored into any store of the account with the words.
    fn backup_ui(&mut self, at: &At, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.add_space(tokens::SPACING_MD);
        ui.separator();
        ui.add_space(tokens::SPACING_SM);
        ui.heading("Backup");
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(
                "A sealed copy of this account's conversations and contacts, kept at the \
                 exchange under a key it never sees. The 24 words are the whole of what \
                 opens it.",
            )
            .small(),
        );
        ui.add_space(tokens::SPACING_SM);
        let Some(backup) = state.backup.as_ref() else {
            ui.colored_label(theme.text_secondary, "Asking the exchange…");
            return;
        };
        match &backup.held {
            Some(held) => {
                ui.label(format!(
                    "Backed up: generation {}, written {} by {}.",
                    held.generation,
                    sigil_ui::brief(held.written, self.now()),
                    sigil_ui::message::short(&held.device.to_string())
                ));
            }
            None => {
                ui.colored_label(theme.text_secondary, "Nothing backed up at this exchange.");
            }
        }
        // Said where the generation and the date are, because "daily" is a
        // claim about *those* -- and because a backup that keeps itself up
        // to date is a thing somebody should know is happening. It starts
        // the moment there is a key: nothing is written under a key nobody
        // has written down.
        if backup.has_key {
            ui.colored_label(
                theme.text_muted,
                egui::RichText::new(
                    "Written again by itself once a day, while sigil is running and \
                     connected. What has not changed is kept, not sent again.",
                )
                .small(),
            );
        }
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(format!("{} of {} bytes used.", backup.used, backup.quota)).small(),
        );
        ui.add_space(tokens::SPACING_SM);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(backup.has_key, egui::Button::new("Back up now"))
                .on_disabled_hover_text("Make a backup key first, and write its words down.")
                .on_hover_text("Write what is here to the exchange, sealed under the key.")
                .clicked()
            {
                self.send_as(Some(at), Cmd::BackupNow);
            }
            let key_label = if backup.has_key {
                "Show backup key"
            } else {
                "Make a backup key"
            };
            if backup.words.is_none() && ui.button(key_label).clicked() {
                self.send_as(Some(at), Cmd::BackupKey);
            }
            if backup.words.is_some()
                && sigil_ui::icon_button_named(ui, sigil_ui::Icon::Close, "Hide").clicked()
            {
                self.send_as(Some(at), Cmd::HideBackupKey);
            }
        });
        if let Some(words) = &backup.words {
            ui.add_space(tokens::SPACING_SM);
            ui.colored_label(
                theme.warning,
                "Write these words down, in order, where this machine is not. They are \
                 the whole of what opens the backup, and nothing can get them back.",
            );
            // **How many across depends on the width there is.** Six cells
            // of `"24. youthful"` in monospace is some 650 points, which on
            // a 360-point pane is not a grid, it is a pane torn in half --
            // and because egui grows a ui to what is drawn in it, it took
            // the fields *above* it out with it. The words are the whole of
            // what opens the backup and this is the screen somebody copies
            // them from, so it is the last place to be clever about space.
            //
            // Measured rather than chosen: the widest cell is what decides,
            // and a language whose words are longer gets fewer columns
            // rather than an overflow.
            let cell = {
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                words
                    .iter()
                    .enumerate()
                    .map(|(i, w)| {
                        ui.ctx().fonts_mut(|f| {
                            f.layout_no_wrap(
                                format!("{:>2}. {w}", i + 1),
                                font.clone(),
                                egui::Color32::PLACEHOLDER,
                            )
                            .rect
                            .width()
                        })
                    })
                    .fold(0.0f32, f32::max)
            };
            let gap = tokens::SPACING_MD;
            let columns =
                (((ui.available_width() + gap) / (cell + gap)).floor() as usize).clamp(1, 6);
            egui::Grid::new("backup-words")
                .num_columns(columns)
                .spacing([gap, tokens::SPACING_XS])
                .show(ui, |ui| {
                    for (i, word) in words.iter().enumerate() {
                        ui.label(egui::RichText::new(format!("{:>2}. {word}", i + 1)).monospace());
                        if (i + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        }

        ui.add_space(tokens::SPACING_MD);
        ui.label("Restore");
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(
                "Restoring adds what the backup holds to what is here; nothing here is \
                 removed. On a fresh install, this is how the conversations come back.",
            )
            .small(),
        );
        // The same row as every field on this pane: the box given what the
        // mark beside it leaves, two lines tall for twenty-four words. It was
        // a full-width box with a named button on the row under it, the one
        // field here whose action was not a mark at its right.
        let mut restore = false;
        let rows = ui.text_style_height(&egui::TextStyle::Body) * 2.0
            + 2.0 * ui.spacing().button_padding.y
            + tokens::SPACING_SM;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), rows),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                restore = sigil_ui::icon_button_named(ui, sigil_ui::Icon::Check, "Restore")
                    .on_hover_text("Open the backup with these words and add what it holds")
                    .clicked();
                ui.add_space(tokens::SPACING_SM);
                let width = ui.available_width();
                ui.add(
                    egui::TextEdit::multiline(
                        &mut self.panes.entry(at.clone()).or_default().restoring,
                    )
                    .hint_text("the 24 words, in order")
                    .desired_rows(2)
                    .desired_width(width),
                );
            },
        );
        if restore {
            let words = self.pane(at).restoring.trim().to_string();
            if !words.is_empty() {
                self.pane(at).restoring.clear();
                // The identity file, so the restore records the home
                // beside it (SIP-60): a backup lives at the home.
                let identity = self.identity_paths.get(&at.0).cloned();
                self.send_as(Some(at), Cmd::Restore { words, identity });
            }
        }
        // Only when there is a row to draw: an empty `horizontal` still
        // takes a row's height, which was a gap under the field.
        if backup.held.is_some() {
            ui.horizontal(|ui| {
                // Two steps, as destroying a conversation is: releasing the
                // copy is not undone by anything but writing another.
                let confirming = self.pane(at).confirming_drop;
                if !confirming {
                    if ui
                        .add(egui::Button::new(
                            egui::RichText::new("Drop backup").color(theme.destructive),
                        ))
                        .on_hover_text("Release the copy the exchange holds. What is here stays.")
                        .clicked()
                    {
                        self.pane(at).confirming_drop = true;
                    }
                } else {
                    ui.colored_label(
                        theme.destructive,
                        "This releases the copy at the exchange. What is on this machine stays.",
                    );
                    if ui.button("Yes, drop it").clicked() {
                        self.pane(at).confirming_drop = false;
                        self.send_as(Some(at), Cmd::DropBackup);
                    }
                    if ui.button("Cancel").clicked() {
                        self.pane(at).confirming_drop = false;
                    }
                }
            });
        }
    }
}

impl ChatApp {
    /// SIP-44: what to arrange for the day this key is lost, and what to do
    /// on it. Four things, in the order somebody meets them: a will and
    /// guardians, written by the account while it still can; a vouch, as one
    /// of somebody's guardians; and the claim, as the successor.
    ///
    /// **This was a terminal's.** The CLI has had all four since SIP-44
    /// landed, and sigil's own documentation said so. A phone has no
    /// terminal, and the phone is the device most likely to be the one that
    /// is lost.
    fn succession_ui(&mut self, at: &At, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.add_space(tokens::SPACING_MD);
        ui.separator();
        ui.add_space(tokens::SPACING_SM);
        ui.heading("If you lose your key");
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(
                "Your account is your key. Say now who takes over when it is gone, and the \
                 exchange carries everything across when they do: names, conversations, \
                 your place in each.",
            )
            .small(),
        );
        ui.add_space(tokens::SPACING_SM);
        let Some(su) = state.succession.as_ref() else {
            ui.colored_label(theme.text_secondary, "Asking the exchange…");
            return;
        };

        if su.is_account {
            // -- a will ------------------------------------------------------
            ui.label(egui::RichText::new("A will").strong());
            ui.colored_label(
                theme.text_secondary,
                egui::RichText::new(
                    "Names one key that may take this account by presenting it. Keep the will \
                     where that key's secret is not: together they are the account.",
                )
                .small(),
            );
            let (_, write) = sigil_ui::labelled_field(
                ui,
                "",
                &mut self.panes.entry(at.clone()).or_default().successor,
                "the successor's key, in base58",
                Some(sigil_ui::Action::Mark(
                    sigil_ui::Icon::Check,
                    "Write the will",
                )),
            );
            if write {
                let typed = self.pane(at).successor.trim().to_string();
                match typed.parse::<PubKey>() {
                    Ok(key) => self.send_as(Some(at), Cmd::WriteWill(key)),
                    Err(_) => self.pane(at).add_trouble = Some("that is not a key".into()),
                }
            }
            if let Some(will) = &su.will {
                self.shown_secret_ui(at, ui, theme, will, "the will");
            }

            // -- guardians ---------------------------------------------------
            ui.add_space(tokens::SPACING_MD);
            ui.label(egui::RichText::new("Guardians").strong());
            ui.colored_label(
                theme.text_secondary,
                egui::RichText::new(
                    "People you name, and a number of them it takes. When the key is gone, \
                     that many of them each vouch for the key that succeeds you, and no one of \
                     them can move your account alone.",
                )
                .small(),
            );
            match &su.lodged {
                Some((threshold, guardians)) => {
                    ui.colored_label(
                        theme.success,
                        format!(
                            "Lodged: any {threshold} of {} guardians may name your successor.",
                            guardians.len()
                        ),
                    );
                    for g in guardians {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(g.to_string()).monospace().small(),
                            )
                            .selectable(true),
                        );
                    }
                    ui.colored_label(
                        theme.text_muted,
                        egui::RichText::new("Naming guardians again replaces these.").small(),
                    );
                }
                None => {
                    ui.colored_label(theme.text_muted, egui::RichText::new("None named.").small());
                }
            }
            let (_, add) = sigil_ui::labelled_field(
                ui,
                "",
                &mut self.panes.entry(at.clone()).or_default().guardian,
                "a guardian's key, in base58",
                Some(sigil_ui::Action::Mark(
                    sigil_ui::Icon::Plus,
                    "Add a guardian",
                )),
            );
            if add {
                let typed = self.pane(at).guardian.trim().to_string();
                match typed.parse::<PubKey>() {
                    Ok(key) => {
                        let pane = self.pane(at);
                        if !pane.guardians.contains(&key) {
                            pane.guardians.push(key);
                        }
                        pane.guardian.clear();
                    }
                    Err(_) => self.pane(at).add_trouble = Some("that is not a key".into()),
                }
            }
            let named = self.pane(at).guardians.clone();
            if !named.is_empty() {
                let mut drop: Option<PubKey> = None;
                for g in &named {
                    ui.horizontal(|ui| {
                        if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Close, "Remove")
                            .clicked()
                        {
                            drop = Some(*g);
                        }
                        // Wrapped: a label in a horizontal does not, and a
                        // key beside a button is wider than a phone.
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(g.to_string()).monospace().small(),
                            )
                            .wrap()
                            .selectable(true),
                        );
                    });
                }
                if let Some(g) = drop {
                    self.pane(at).guardians.retain(|k| *k != g);
                }
                let n = named.len();
                let mut threshold = self.pane(at).threshold.clamp(1, n as u8);
                ui.horizontal(|ui| {
                    ui.label("It takes");
                    ui.add(egui::DragValue::new(&mut threshold).range(1..=n as u8));
                    ui.label(format!("of {n}"));
                    if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Check, "Lodge").clicked() {
                        self.send_as(
                            Some(at),
                            Cmd::NameGuardians {
                                threshold,
                                guardians: named.clone(),
                            },
                        );
                        self.pane(at).guardians.clear();
                    }
                });
                self.pane(at).threshold = threshold;
            }
        } else {
            ui.colored_label(
                theme.text_secondary,
                egui::RichText::new(
                    "This is one of the account's devices, not the account: a will or guardians \
                     are written from a device that holds the account's own key.",
                )
                .small(),
            );
        }

        // -- as a guardian ---------------------------------------------------
        ui.add_space(tokens::SPACING_MD);
        ui.label(egui::RichText::new("As somebody's guardian").strong());
        ui.colored_label(
            theme.text_secondary,
            egui::RichText::new(
                "Sign that a key succeeds an account that named you. Your word, for the \
                 successor to collect; it moves nothing on its own.",
            )
            .small(),
        );
        sigil_ui::labelled_field(
            ui,
            "",
            &mut self.panes.entry(at.clone()).or_default().vouch_account,
            "the account being succeeded, in base58",
            None,
        );
        let (_, vouch) = sigil_ui::labelled_field(
            ui,
            "",
            &mut self.panes.entry(at.clone()).or_default().vouch_successor,
            "the key that succeeds it, in base58",
            Some(sigil_ui::Action::Mark(sigil_ui::Icon::Check, "Vouch")),
        );
        if vouch {
            let (a, s) = {
                let pane = self.pane(at);
                (
                    pane.vouch_account.trim().to_string(),
                    pane.vouch_successor.trim().to_string(),
                )
            };
            match (a.parse::<PubKey>(), s.parse::<PubKey>()) {
                (Ok(account), Ok(successor)) => {
                    self.send_as(Some(at), Cmd::Vouch { account, successor })
                }
                _ => self.pane(at).add_trouble = Some("both have to be keys".into()),
            }
        }
        if let Some(v) = &su.vouch {
            self.shown_secret_ui(at, ui, theme, v, "the vouch");
        }

        // -- the claim -------------------------------------------------------
        ui.add_space(tokens::SPACING_MD);
        ui.label(egui::RichText::new("Take an account that named you").strong());
        ui.colored_label(
            theme.text_secondary,
            egui::RichText::new(
                "Paste the will you were given; or the account's key, and under it the \
                 vouches its guardians gave you, one per line. The account becomes this \
                 key's: its names, its conversations, its place in each.",
            )
            .small(),
        );
        // The same shape as every field above it -- the action is a mark at
        // the field's right -- only taller, since a claim by vouches is one
        // line per guardian. It was a named button under the field, the one
        // control on the pane that did not look like the others.
        let rows = ui.text_style_height(&egui::TextStyle::Body) * 2.0
            + 2.0 * ui.spacing().button_padding.y
            + tokens::SPACING_SM;
        let mut take = false;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), rows),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                take = sigil_ui::icon_button_named(ui, sigil_ui::Icon::Check, "Take it")
                    .on_hover_text("Present this and become the account")
                    .clicked();
                ui.add_space(tokens::SPACING_SM);
                let width = ui.available_width();
                ui.add(
                    egui::TextEdit::multiline(
                        &mut self.panes.entry(at.clone()).or_default().claiming,
                    )
                    .hint_text("a will, or an account's key and the vouches for you")
                    .desired_rows(2)
                    .desired_width(width),
                );
            },
        );
        if take {
            let pasted = self.pane(at).claiming.trim().to_string();
            if !pasted.is_empty() {
                self.pane(at).claiming.clear();
                self.send_as(Some(at), Cmd::Succeed(pasted));
            }
        }
        if let Some(t) = self.pane(at).add_trouble.clone() {
            ui.colored_label(theme.destructive, t);
        }
    }

    /// A signed thing just written -- a will, a vouch -- shown to be copied
    /// and then put away. Base58, in full, selectable, with the one thing to
    /// say about it beside a way to hide it.
    fn shown_secret_ui(
        &mut self,
        at: &At,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        text: &str,
        what: &str,
    ) {
        ui.add_space(tokens::SPACING_XS);
        egui::Frame::NONE
            .fill(theme.surface_elevated)
            .corner_radius(tokens::RADIUS_MD)
            .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8))
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(text).monospace().small())
                        .wrap()
                        .selectable(true),
                );
                ui.horizontal(|ui| {
                    if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Copy, "Copy").clicked() {
                        ui.ctx().copy_text(text.to_string());
                    }
                    if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Close, "Hide").clicked() {
                        self.send_as(Some(at), Cmd::HideSuccession);
                    }
                    ui.colored_label(
                        theme.text_muted,
                        egui::RichText::new(format!("Give {what} to the person it is for."))
                            .small(),
                    );
                });
            });
    }
}

#[cfg(test)]
mod duplicate_tests {
    use super::*;

    fn key(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    /// The added name is what gets offered, whichever of the two lost the race.
    ///
    /// Which one loses depends on which connected first, so both orders have
    /// to give the same answer — otherwise the advice would be "remove the
    /// default", which is not a thing that can be done.
    #[test]
    fn the_added_name_is_the_one_to_remove_either_way() {
        let me = key(1);
        let exchange = key(9);
        let default = (me, String::new());
        let added = (me, "squic.org".to_string());

        // The added one lost.
        assert_eq!(
            duplicate_of(&added, Some(exchange), &[(default.clone(), Some(exchange))]),
            Some("squic.org".to_string())
        );
        // The default lost.
        assert_eq!(
            duplicate_of(&default, Some(exchange), &[(added.clone(), Some(exchange))]),
            Some("squic.org".to_string())
        );
    }

    /// A lock somebody else holds keeps its own words.
    ///
    /// Another sigil, or the terminal client, is a different problem with a
    /// different answer — and telling somebody to remove an exchange they have
    /// only one session for would leave them without it and no better off.
    #[test]
    fn a_lock_nobody_here_holds_is_not_called_a_duplicate() {
        let me = key(1);
        let exchange = key(9);
        let added = (me, "squic.org".to_string());

        // Nothing else running.
        assert_eq!(duplicate_of(&added, Some(exchange), &[]), None);
        // Something running, at a different exchange.
        assert_eq!(
            duplicate_of(
                &added,
                Some(exchange),
                &[((me, String::new()), Some(key(8)))]
            ),
            None
        );
        // Another identity, at the same exchange. Its lock is on its own
        // store, so it cannot be what refused this one.
        assert_eq!(
            duplicate_of(
                &added,
                Some(exchange),
                &[((key(2), String::new()), Some(exchange))]
            ),
            None
        );
        // And no refusal at all.
        assert_eq!(
            duplicate_of(&added, None, &[((me, String::new()), Some(exchange))]),
            None
        );
    }
}

#[cfg(test)]
mod ring_tests {
    use super::*;

    fn key(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    /// With more than one identity, the notification says which was called.
    ///
    /// A call reaches somebody who is looking at another identity — that is
    /// the entire reason it is announced from `update` and not from `render`.
    /// Naming the caller and the conversation without naming the identity
    /// leaves them with no way to tell where either of those is.
    #[test]
    fn a_ring_names_the_identity_it_arrived_at() {
        let said = ring_said(&key(2), "Ada", "colin@squic.org", 3);
        assert!(said.contains("Ada"), "{said}");
        assert!(said.contains("colin@squic.org"), "{said}");
        // And the caller's key, abbreviated. A name is an assertion (SIP-21)
        // and the label above is one; this is not.
        assert!(
            said.contains(&sigil_ui::message::short(&key(2).to_string())),
            "{said}"
        );
    }

    /// With one identity there is nothing to tell apart.
    #[test]
    fn a_ring_at_the_only_identity_does_not_name_it() {
        let said = ring_said(&key(2), "Ada", "colin@squic.org", 1);
        assert!(said.contains("Ada"), "{said}");
        assert!(
            !said.contains("colin@squic.org"),
            "a fact nobody was in doubt about: {said}"
        );
    }
}

#[cfg(test)]
mod mention_notice_tests {
    use super::*;
    use sigil::app::Sound;
    use std::cell::RefCell;

    fn key(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    /// Where the test's session lives: the identity, at its default
    /// exchange.
    fn at() -> At {
        (key(4), String::new())
    }

    /// A notifier that writes down what it was asked to say. `Silent` is
    /// the only other one, and silence is what a working notifier looks
    /// like from inside a test.
    struct Noted(
        RefCell<Vec<(String, String)>>,
        RefCell<Vec<(Option<Target>, Sound)>>,
        RefCell<Vec<Target>>,
    );

    impl Noted {
        fn new() -> Noted {
            Noted(
                RefCell::new(Vec::new()),
                RefCell::new(Vec::new()),
                RefCell::new(Vec::new()),
            )
        }

        /// Rings taken down since the last time this was asked.
        fn withdrawn(&self) -> Vec<Target> {
            std::mem::take(&mut self.2.borrow_mut())
        }
    }

    impl sigil::app::Notify for Noted {
        fn notice(&self, notice: sigil::Notice<'_>) -> bool {
            self.0
                .borrow_mut()
                .push((notice.summary.into(), notice.body.into()));
            self.1.borrow_mut().push((notice.target, notice.sound));
            true
        }

        fn withdraw(&self, target: &Target) {
            self.2.borrow_mut().push(target.clone());
        }
    }

    fn a_mention(seq: u64, in_open: bool) -> session::Mention {
        let mut m = an_arrival(seq, in_open);
        m.mentions_me = true;
        m
    }

    fn an_arrival(seq: u64, in_open: bool) -> session::Arrival {
        session::Arrival {
            channel: [8u8; 32],
            seq,
            from: key(2),
            direct: false,
            mentions_me: false,
            from_label: "Ada".into(),
            conversation: "general".into(),
            public: true,
            said: "look at this".into(),
            in_open,
        }
    }

    fn ctx<'a>(
        nav: &'a mut sigil::navigator::Navigator,
        accounts: &'a mut sigil::accounts::Accounts,
        notify: &'a dyn sigil::app::Notify,
        connections: &'a sigil_net::Connections,
        unfocused: bool,
    ) -> AppContext<'a> {
        AppContext {
            navigator: nav,
            accounts,
            unfocused,
            away: false,
            notify,
            connections,
        }
    }

    /// Said once while the window is not in front, and not again on the
    /// next pass; not said at all for a mention in the conversation on
    /// screen while the window is in front, which was read as it arrived.
    #[test]
    fn a_mention_is_said_once_when_not_looking_and_never_when_looking() {
        let noted = Noted::new();
        let mut nav = sigil::navigator::Navigator::default();
        let mut accounts =
            sigil::accounts::Accounts::of(vec![sigil::Account::unlocked_for_test([4u8; 32])]);
        let connections = sigil_net::Connections::new();
        let mut app = ChatApp::new();

        // Not in front: said.
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_mentions_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![a_mention(1, true)])],
        );
        assert_eq!(noted.0.borrow().len(), 1, "{:?}", noted.0.borrow());
        let (summary, body) = noted.0.borrow()[0].clone();
        assert_eq!(summary, "Ada mentioned you in #general");
        assert!(
            body.contains("look at this")
                && body.contains(&sigil_ui::message::short(&key(2).to_string())),
            "{body}"
        );

        // The same mention on the next pass: not again.
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_mentions_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![a_mention(1, true)])],
        );
        assert_eq!(noted.0.borrow().len(), 1, "told twice");

        // In front and on screen: read as it arrived, nothing to say.
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, false);
        app.announce_mentions_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![a_mention(2, true)])],
        );
        assert_eq!(noted.0.borrow().len(), 1, "said about a message on screen");

        // In front but in another conversation: said.
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, false);
        app.announce_mentions_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![a_mention(3, false)])],
        );
        assert_eq!(noted.0.borrow().len(), 2);
    }

    /// A mention while the window is not in front asks for attention --
    /// the icon bounces once -- and one while in front does not, wherever
    /// the conversation is: a bounce under somebody looking at the window
    /// is a twitch. A ring asks to be presented, and for attention until
    /// answered.
    #[test]
    fn a_mention_away_asks_for_attention_and_a_ring_asks_to_be_presented() {
        let noted = Noted::new();
        let mut nav = sigil::navigator::Navigator::default();
        let mut accounts =
            sigil::accounts::Accounts::of(vec![sigil::Account::unlocked_for_test([4u8; 32])]);
        let connections = sigil_net::Connections::new();
        let mut app = ChatApp::new();

        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_mentions_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![a_mention(1, true)])],
        );
        assert_eq!(
            app.asked(),
            vec![AppAction::Attention(sigil::Attention::Informational)]
        );
        assert_eq!(app.asked(), vec![], "taken once");

        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, false);
        app.announce_mentions_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![a_mention(2, false)])],
        );
        assert_eq!(app.asked(), vec![], "in front: nothing asked");

        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, false);
        app.announce_rings_in(
            &mut c,
            vec![("Ada is calling".into(), target(&at(), [8u8; 32]))],
        );
        assert_eq!(
            app.asked(),
            vec![
                AppAction::Present,
                AppAction::Attention(sigil::Attention::Critical)
            ]
        );
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, false);
        app.announce_rings_in(&mut c, vec![]);
        assert_eq!(app.asked(), vec![], "no ring, nothing asked");
    }

    /// A ring that has stopped ringing is taken down, once, and a ring that
    /// is still ringing is left alone.
    ///
    /// **Nothing withdrew a ring on any platform.** The Android glue had the
    /// call and no Rust ever made it, so a ring posted there stayed on the
    /// shade for good -- ongoing, unswipeable, still offering Answer for a
    /// call that had ended. Pressing it opened the conversation and did
    /// nothing else, which is what a broken button looks like.
    #[test]
    fn a_ring_that_has_stopped_ringing_is_taken_down_once() {
        let noted = Noted::new();
        let mut nav = sigil::navigator::Navigator::default();
        let mut accounts =
            sigil::accounts::Accounts::of(vec![sigil::Account::unlocked_for_test([4u8; 32])]);
        let connections = sigil_net::Connections::new();
        let mut app = ChatApp::new();

        // Two rings posted. No session holds either, so both have gone.
        let gone = target(&at(), [8u8; 32]);
        let other = target(&at(), [9u8; 32]);
        app.announcer
            .post_ring_for_test(([8u8; 32], 1), gone.clone());
        app.announcer
            .post_ring_for_test(([9u8; 32], 7), other.clone());

        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, false);
        // **Through `announce_rings`, not straight into the pass.** The
        // defect being fixed is a withdrawal nothing called; a test that
        // calls the pass itself passes with the wiring cut out, and I
        // checked that it did before writing it this way.
        app.announce_rings(&mut c);
        let mut taken = noted.withdrawn();
        taken.sort_by_key(|t| t.channel);
        assert_eq!(taken, vec![gone, other], "both rings should be taken down");
        assert!(app.announcer.ringing_out_for_test().is_empty());

        // And not again: an ongoing notification withdrawn twice is a second
        // call into the platform for a notification that is already gone.
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, false);
        app.announce_rings(&mut c);
        assert!(noted.withdrawn().is_empty(), "a ring was taken down twice");
    }

    /// With several identities held, which one was mentioned; with one,
    /// nothing nobody was in doubt about. A private room has no `#`.
    #[test]
    fn a_mention_names_the_identity_only_when_there_are_several() {
        let (one, _) = mention_said(&a_mention(1, false), "colin@squic.org", 1);
        assert_eq!(one, "Ada mentioned you in #general");
        let (several, _) = mention_said(&a_mention(1, false), "colin@squic.org", 3);
        assert!(several.contains("as colin@squic.org"), "{several}");
        let mut private = a_mention(1, false);
        private.public = false;
        private.conversation = "the four of us".into();
        let (s, _) = mention_said(&private, "x", 1);
        assert_eq!(s, "Ada mentioned you in the four of us");
    }

    /// A message arriving while the window is not in front is said once,
    /// leading to its conversation, with the ordinary sound; several in one
    /// conversation together are one notification that counts them; a
    /// mention is left to its own words; in front, nothing is said and
    /// nothing is owed later.
    #[test]
    fn arrivals_are_said_while_away_grouped_by_conversation_and_lead_back() {
        let noted = Noted::new();
        let mut nav = sigil::navigator::Navigator::default();
        let mut accounts =
            sigil::accounts::Accounts::of(vec![sigil::Account::unlocked_for_test([4u8; 32])]);
        let connections = sigil_net::Connections::new();
        let mut app = ChatApp::new();

        // Away: one message, said with who and where and what.
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_arrivals_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![an_arrival(1, false)])],
        );
        assert_eq!(noted.0.borrow().len(), 1, "{:?}", noted.0.borrow());
        let (summary, body) = noted.0.borrow()[0].clone();
        assert_eq!(summary, "Ada in #general");
        assert_eq!(body, "look at this");
        let (target, sound) = noted.1.borrow()[0].clone();
        assert_eq!(
            target,
            Some(Target {
                identity: key(4),
                exchange: String::new(),
                channel: [8u8; 32],
                answer: false,
            })
        );
        assert_eq!(sound, Sound::Default);

        // The same message again: not twice.
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_arrivals_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![an_arrival(1, false)])],
        );
        assert_eq!(noted.0.borrow().len(), 1);

        // Three at once in one conversation: one notification, counted.
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_arrivals_in(
            &mut c,
            1,
            vec![(
                at(),
                "me".into(),
                vec![
                    an_arrival(2, false),
                    an_arrival(3, false),
                    an_arrival(4, false),
                ],
            )],
        );
        assert_eq!(noted.0.borrow().len(), 2, "{:?}", noted.0.borrow());
        let (summary, body) = noted.0.borrow()[1].clone();
        assert_eq!(summary, "3 new messages in #general");
        assert_eq!(body, "Ada: look at this");

        // A mention among them is not said here: it has its own words.
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_arrivals_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![a_mention(5, false)])],
        );
        assert_eq!(noted.0.borrow().len(), 2);

        // In front: nothing said, and the message is not owed later.
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, false);
        app.announce_arrivals_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![an_arrival(6, false)])],
        );
        assert_eq!(noted.0.borrow().len(), 2);
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_arrivals_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![an_arrival(6, false)])],
        );
        assert_eq!(noted.0.borrow().len(), 2, "read as it arrived: not owed");
    }

    /// A direct message names the person, not a room; several identities
    /// held say which one it came to. A ring carries the ring sound and
    /// leads to the ringing conversation; a mention carries the ordinary
    /// one.
    #[test]
    fn arrivals_are_worded_for_direct_messages_and_several_identities() {
        let mut direct = an_arrival(1, false);
        direct.direct = true;
        direct.conversation = "Ada".into();
        let (summary, body) = arrivals_said(&[direct.clone()], "me", 1);
        assert_eq!(summary, "Ada");
        assert_eq!(body, "look at this");
        let (summary, body) = arrivals_said(&[direct.clone(), direct.clone()], "me", 2);
        assert_eq!(summary, "2 new messages from Ada, as me");
        assert_eq!(body, "look at this");

        let noted = Noted::new();
        let mut nav = sigil::navigator::Navigator::default();
        let mut accounts =
            sigil::accounts::Accounts::of(vec![sigil::Account::unlocked_for_test([4u8; 32])]);
        let connections = sigil_net::Connections::new();
        let mut app = ChatApp::new();
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_rings_in(
            &mut c,
            vec![("Ada is calling".into(), target(&at(), [9u8; 32]))],
        );
        app.announce_mentions_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![a_mention(1, false)])],
        );
        let carried = noted.1.borrow().clone();
        assert_eq!(carried.len(), 2);
        assert_eq!(carried[0].1, Sound::Ring);
        assert_eq!(carried[0].0.as_ref().map(|t| t.channel), Some([9u8; 32]));
        assert_eq!(carried[1].1, Sound::Default);
        assert_eq!(carried[1].0.as_ref().map(|t| t.channel), Some([8u8; 32]));
    }

    /// A muted conversation is not said out loud -- not its messages, not a
    /// mention in it, not a ring from it -- and asks for no attention;
    /// under do-not-disturb, nothing anywhere is. What went unsaid is not
    /// owed later: unmuting does not replay it.
    #[test]
    fn a_muted_conversation_and_do_not_disturb_say_nothing() {
        let noted = Noted::new();
        let mut nav = sigil::navigator::Navigator::default();
        let mut accounts =
            sigil::accounts::Accounts::of(vec![sigil::Account::unlocked_for_test([4u8; 32])]);
        let connections = sigil_net::Connections::new();
        let mut app = ChatApp::new();

        accounts.quiet.set_muted("", &[8u8; 32], true);
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_arrivals_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![an_arrival(1, false)])],
        );
        app.announce_mentions_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![a_mention(2, false)])],
        );
        assert!(noted.0.borrow().is_empty(), "{:?}", noted.0.borrow());
        assert_eq!(app.asked(), vec![], "and nothing asked for");
        // Another conversation is still said.
        let mut elsewhere = an_arrival(3, false);
        elsewhere.channel = [9u8; 32];
        app.announce_arrivals_in(&mut c, 1, vec![(at(), "me".into(), vec![elsewhere])]);
        assert_eq!(noted.0.borrow().len(), 1);

        // Unmuted: what went unsaid stays unsaid.
        accounts.quiet.set_muted("", &[8u8; 32], false);
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        app.announce_arrivals_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![an_arrival(1, false)])],
        );
        assert_eq!(noted.0.borrow().len(), 1, "not replayed");

        // Do not disturb: nothing anywhere, rings included.
        accounts.quiet.set_dnd(true);
        let mut c = ctx(&mut nav, &mut accounts, &noted, &connections, true);
        let mut anywhere = an_arrival(4, false);
        anywhere.channel = [9u8; 32];
        app.announce_arrivals_in(&mut c, 1, vec![(at(), "me".into(), vec![anywhere])]);
        app.announce_mentions_in(
            &mut c,
            1,
            vec![(at(), "me".into(), vec![a_mention(5, false)])],
        );
        app.announce_rings_in(
            &mut c,
            vec![("Ada is calling".into(), target(&at(), [9u8; 32]))],
        );
        assert_eq!(noted.0.borrow().len(), 1, "{:?}", noted.0.borrow());
        assert_eq!(app.asked(), vec![], "not even a ring");
    }
}

#[cfg(test)]
mod showing_tests {
    use super::*;

    fn key(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    /// An identity connected only at a named exchange is shown there.
    ///
    /// Its default is nothing — no handle sidecar, no `server` in the config —
    /// so no session is started for it, and showing the default anyway said
    /// *not connected* about an account that was connected, while adding the
    /// exchange it already had was refused as a duplicate.
    #[test]
    fn an_identity_with_no_working_default_is_shown_where_it_is_connected() {
        let me = key(1);
        let live = [(me, "squic.org".to_string())];
        assert_eq!(showing_exchange(me, None, &live), "squic.org");
    }

    /// The default wins when it works.
    #[test]
    fn the_default_is_preferred_when_there_is_a_session_for_it() {
        let me = key(1);
        let live = [(me, String::new()), (me, "squic.org".to_string())];
        assert_eq!(showing_exchange(me, None, &live), "");
    }

    /// A choice is a choice, even a choice of something broken.
    #[test]
    fn an_explicit_choice_is_honoured_whether_or_not_it_works() {
        let me = key(1);
        let live = [(me, "squic.org".to_string())];
        let chosen = String::new();
        assert_eq!(showing_exchange(me, Some(&chosen), &live), "");
        let other = "indra.org".to_string();
        assert_eq!(showing_exchange(me, Some(&other), &live), "indra.org");
    }

    /// Somebody else's sessions are not this identity's.
    #[test]
    fn another_identitys_exchange_is_not_borrowed() {
        let me = key(1);
        let live = [(key(2), "squic.org".to_string())];
        assert_eq!(showing_exchange(me, None, &live), "");
    }
}

#[cfg(test)]
mod label_tests {
    use super::*;

    fn at(domain: Option<&str>, key: Option<u8>) -> ChatState {
        ChatState {
            domain: domain.map(str::to_string),
            exchange: key.map(|b| PubKey::new([b; 32])),
            ..ChatState::default()
        }
    }

    /// The domain, when the connection was discovered at one.
    #[test]
    fn the_default_exchange_is_called_by_its_domain() {
        assert_eq!(
            default_label(Some(at(Some("squic.org"), Some(3)))),
            "squic.org"
        );
    }

    /// The key when there is no domain — an address was dialled, and an
    /// address has none. Still better than a word naming nothing.
    #[test]
    fn without_a_domain_the_key_stands_in() {
        let said = default_label(Some(at(None, Some(3))));
        assert_ne!(said, "default");
        // Both ends of the key, in the form every key on screen takes.
        let whole = PubKey::new([3u8; 32]).to_string();
        let (head, tail) = said.split_once("...").expect("a short key: {said}");
        assert!(
            whole.starts_with(head) && whole.ends_with(tail),
            "{said} is not the two ends of {whole}"
        );
    }

    /// Nothing known yet, and nothing invented.
    #[test]
    fn with_no_session_at_all_it_is_just_the_default() {
        assert_eq!(default_label(None), "default");
        assert_eq!(default_label(Some(at(None, None))), "default");
    }
}

#[cfg(test)]
mod first_look_tests {
    use super::*;

    fn convo(channel: u8, at: Option<u64>) -> Summary {
        Summary {
            channel: [channel; 32],
            peer: None,
            label: String::new(),
            unread: 0,
            mentioned: 0,
            preview: None,
            at,
            public: Some(false),
            group: false,
            typing: false,
            waiting: false,
        }
    }

    /// The newest, and not the first row handed over.
    #[test]
    fn arriving_opens_the_latest_conversation() {
        // Deliberately out of order: a list sorted the other way must still
        // give the same answer, or this is testing the session's sort.
        let convos = [convo(1, Some(10)), convo(2, Some(90)), convo(3, Some(50))];
        assert_eq!(first_look(None, false, false, &convos), Some([2; 32]));
        let backwards = [convo(3, Some(50)), convo(2, Some(90)), convo(1, Some(10))];
        assert_eq!(first_look(None, false, false, &backwards), Some([2; 32]));
    }

    /// Once. Closing a conversation must not reopen it on the next pass,
    /// which is the whole reason the flag exists.
    #[test]
    fn it_happens_once_and_not_on_every_pass() {
        let convos = [convo(1, Some(10)), convo(2, Some(90))];
        assert_eq!(first_look(None, true, false, &convos), None);
    }

    /// Something already open is what somebody chose, and is not replaced.
    #[test]
    fn what_is_already_open_stays_open() {
        let convos = [convo(1, Some(10)), convo(2, Some(90))];
        assert_eq!(first_look(Some([1; 32]), false, false, &convos), None);
    }

    /// One pane: opening a conversation is hiding the list, so arriving does
    /// not do it on somebody's behalf.
    #[test]
    fn a_one_pane_window_is_left_on_the_list() {
        let convos = [convo(1, Some(10)), convo(2, Some(90))];
        assert_eq!(first_look(None, false, true, &convos), None);
    }

    /// Nothing to open, and nothing invented.
    #[test]
    fn an_empty_list_opens_nothing() {
        assert_eq!(first_look(None, false, false, &[]), None);
    }

    /// A conversation with nothing said in it still counts, because a new
    /// chat somebody just started is the one they are looking for.
    #[test]
    fn a_conversation_with_no_messages_is_still_a_conversation() {
        let convos = [convo(1, None), convo(2, None)];
        // The tie-break, so it is an answer rather than whichever the
        // iterator happened to reach last.
        assert_eq!(first_look(None, false, false, &convos), Some([1; 32]));
    }
}

#[cfg(test)]
mod look_tests {
    use super::Look;
    use egui::vec2;

    /// A hundred-pixel window onto a hundred-pixel picture.
    const VIEW: egui::Vec2 = vec2(100.0, 100.0);

    /// The pointer at the edge of the window shows that edge of the picture.
    ///
    /// The whole point of following the pointer is that the two halves of the
    /// window reach the two halves of the picture: anything less and there is
    /// part of it nobody can get to without a scrollbar.
    #[test]
    fn the_pointer_at_an_edge_shows_that_edge_of_the_picture() {
        let close = Look {
            zoom: 2.0,
            pan: egui::Vec2::ZERO,
        };
        // Twice the size in a window of one: fifty pixels of room each way.
        let left = close.following(vec2(-50.0, 0.0), VIEW, VIEW);
        assert_eq!(left.pan, vec2(50.0, 0.0), "the picture moves the other way");
        let right = close.following(vec2(50.0, 0.0), VIEW, VIEW);
        assert_eq!(right.pan, vec2(-50.0, 0.0));
        // And the middle of the window is the middle of the picture.
        assert_eq!(
            close.following(egui::Vec2::ZERO, VIEW, VIEW).pan,
            egui::Vec2::ZERO
        );
    }

    /// Past the edge is still the edge.
    ///
    /// A pointer beyond the window -- which happens, since it is followed
    /// while it is anywhere near -- must not push the picture out of its own
    /// frame and leave a band of nothing along one side.
    #[test]
    fn the_picture_never_comes_off_its_own_window() {
        let close = Look {
            zoom: 2.0,
            pan: egui::Vec2::ZERO,
        };
        let far = close.following(vec2(-500.0, 900.0), VIEW, VIEW);
        assert_eq!(far.pan, vec2(50.0, -50.0));
    }

    /// A picture that fits has nothing to look around.
    #[test]
    fn a_picture_that_fits_does_not_move() {
        let whole = Look::default();
        assert_eq!(
            whole.following(vec2(50.0, 50.0), VIEW, VIEW).pan,
            egui::Vec2::ZERO
        );
    }

    /// Zooming back out puts the whole picture in the middle, because at that
    /// size there is nowhere else for it to be.
    #[test]
    fn zooming_out_brings_it_back_to_the_middle() {
        let close = Look {
            zoom: 4.0,
            pan: vec2(120.0, -90.0),
        };
        assert_eq!(close.zoomed(1.0, VIEW, VIEW).pan, egui::Vec2::ZERO);
    }

    /// **A drag moves the picture by the drag**, which is how a phone looks
    /// around one.
    ///
    /// `following` is the pointer's way and has three tests; `panned` is the
    /// finger's and had none, which is the wrong way round for a viewer whose
    /// own comment says the arithmetic is the part worth testing. A drag that
    /// moves by the wrong amount still looks like a picture moving.
    #[test]
    fn a_drag_moves_the_picture_by_the_drag() {
        let close = Look {
            zoom: 2.0,
            pan: egui::Vec2::ZERO,
        };
        // Twice the size in a window of one: fifty pixels of room each way.
        let moved = close.panned(vec2(20.0, -10.0), VIEW, VIEW);
        assert_eq!(moved.pan, vec2(20.0, -10.0));
        assert_eq!(moved.zoom, 2.0, "a drag does not change the zoom");

        // And a second drag carries on from the first rather than starting
        // again, which is what makes a long look around one gesture.
        let again = moved.panned(vec2(20.0, 0.0), VIEW, VIEW);
        assert_eq!(again.pan, vec2(40.0, -10.0));
    }

    /// A drag past the edge stops at it.
    ///
    /// The same rule `following` has, and it has to hold for the finger too:
    /// a flick must not leave a band of nothing down one side of the viewer.
    #[test]
    fn a_drag_cannot_pull_the_picture_off_its_own_window() {
        let close = Look {
            zoom: 2.0,
            pan: egui::Vec2::ZERO,
        };
        let far = close.panned(vec2(500.0, -900.0), VIEW, VIEW);
        assert_eq!(far.pan, vec2(50.0, -50.0), "clamped to the room there is");
        // Already at the edge, and pushed further: it stays.
        assert_eq!(far.panned(vec2(100.0, 0.0), VIEW, VIEW).pan.x, 50.0);
    }

    /// A picture that fits does not move for a drag either.
    ///
    /// The finger's half of `a_picture_that_fits_does_not_move`: at zoom 1
    /// there is nothing to look around, and a picture that slides under a
    /// stray touch reads as something coming loose.
    #[test]
    fn a_picture_that_fits_does_not_move_for_a_drag() {
        let whole = Look::default();
        assert_eq!(
            whole.panned(vec2(40.0, 40.0), VIEW, VIEW).pan,
            egui::Vec2::ZERO
        );
    }

    /// **A pinch keeps what you were looking at.**
    ///
    /// Zooming in gives the picture more room, so where it had been moved to
    /// is still somewhere it may be: the part under the fingers stays under
    /// them. Zooming *out* takes the room away and the pan has to come back
    /// with it, which is the case already covered; this is the other
    /// direction, and it was the untested one.
    #[test]
    fn pinching_in_keeps_where_you_were_looking() {
        let close = Look {
            zoom: 2.0,
            // At the right edge of what zoom 2 allows.
            pan: vec2(50.0, 0.0),
        };
        let closer = close.zoomed(4.0, VIEW, VIEW);
        assert_eq!(closer.zoom, 4.0);
        assert_eq!(
            closer.pan,
            vec2(50.0, 0.0),
            "zooming in has more room, not less, so nothing needs moving"
        );
        // And zooming part of the way back out clamps to the smaller room.
        assert_eq!(
            Look {
                zoom: 4.0,
                pan: vec2(150.0, 0.0),
            }
            .zoomed(2.0, VIEW, VIEW)
            .pan,
            vec2(50.0, 0.0),
            "at zoom 2 there are fifty pixels of room, not a hundred and fifty"
        );
    }
}

#[cfg(test)]
mod joining_tests {
    use super::{RING_WINDOW, Ring, call_is_over, to_join};
    use sqnr_core::PubKey;
    use std::collections::HashSet;

    fn ring(mine: bool, answered: bool) -> Ring {
        Ring {
            channel: [1u8; 32],
            seq: 4,
            from: PubKey::new([2u8; 32]),
            mine,
            secret: [3u8; 32],
            answered,
            label: "somebody".into(),
            direct: false,
            peer: None,
        }
    }

    /// The one case that joins: our call, picked up, and we are not in one.
    /// The room emptying ends the call, and only once somebody had been
    /// in it: a call still being answered has nobody present either.
    #[test]
    fn a_room_that_had_somebody_and_now_has_nobody_is_over() {
        use sigil_net::Phase;
        let nothing = std::time::Duration::ZERO;
        // Somebody was there, and has gone.
        assert!(call_is_over(Phase::Live, 0, 0, true, false, nothing));
        // Still ringing: nobody has arrived yet, so nobody has left.
        assert!(!call_is_over(Phase::Live, 0, 0, false, false, nothing));
        // They are still here.
        assert!(!call_is_over(Phase::Live, 1, 0, true, false, nothing));
        // Gone from the roster, but another is still connecting.
        assert!(!call_is_over(Phase::Live, 0, 1, true, false, nothing));
    }

    /// The other three routes are unchanged: the task finished, the
    /// channel recorded the end, or nobody ever answered.
    #[test]
    fn a_call_also_ends_when_the_task_the_channel_or_the_ring_window_says_so() {
        use sigil_net::Phase;
        let nothing = std::time::Duration::ZERO;
        assert!(call_is_over(Phase::Ended, 1, 0, true, false, nothing));
        assert!(call_is_over(Phase::Live, 1, 0, true, true, nothing));
        let past = RING_WINDOW + std::time::Duration::from_secs(1);
        assert!(call_is_over(Phase::Connecting, 0, 0, false, false, past));
        assert!(!call_is_over(
            Phase::Connecting,
            0,
            0,
            false,
            false,
            nothing
        ));
    }

    #[test]
    fn our_own_call_being_answered_is_the_one_to_join() {
        let rings = vec![ring(true, true)];
        assert!(to_join(&rings, false, &HashSet::new()).is_some());
    }

    /// A ring this window has already left is not joined again while the
    /// channel still lists it as answered.
    #[test]
    fn a_call_just_left_is_not_joined_again() {
        let rings = vec![ring(true, true)];
        let left: HashSet<([u8; 32], u64)> = [(rings[0].channel, rings[0].seq)].into();
        assert!(to_join(&rings, false, &left).is_none());
        assert!(to_join(&rings, false, &HashSet::new()).is_some());
    }

    /// Still ringing is not yet a call. Joining here would open the
    /// microphone for as long as it rings, and for calls nobody ever answers.
    #[test]
    fn a_call_still_ringing_is_not_joined() {
        assert!(to_join(&[ring(true, false)], false, &HashSet::new()).is_none());
    }

    /// Somebody else's ring is answered by pressing Answer. Joining it here
    /// would pick up every incoming call by itself.
    #[test]
    fn somebody_elses_ring_is_not_ours_to_join() {
        assert!(to_join(&[ring(false, true)], false, &HashSet::new()).is_none());
        assert!(to_join(&[ring(false, false)], false, &HashSet::new()).is_none());
    }

    /// One microphone, one room: already being in a call ends the question,
    /// which is also what stops this joining the same call on every pass.
    #[test]
    fn a_call_already_being_carried_is_not_joined_again() {
        assert!(to_join(&[ring(true, true)], true, &HashSet::new()).is_none());
    }

    /// Ours among others.
    #[test]
    fn the_answered_one_is_found_among_several() {
        let rings = vec![ring(false, true), ring(true, false), ring(true, true)];
        let found = to_join(&rings, false, &HashSet::new()).expect("ours, answered");
        assert!(found.mine && found.answered);
    }
}

/// SIP-47's `sqx-pair:` grammar, the little of it this pane needs: an owner
/// (a key or a `name@domain`) and the domains after the `@`. The phone
/// crate has the full parser and its tests; this crate cannot depend on it
/// (it depends on this one), so the pane accepts the same shapes and lets
/// the session's `ClaimAccount` sort key from name.
mod pairing {
    pub fn parse(shown: &str) -> Result<(String, Vec<String>), String> {
        let shown = shown.trim();
        if shown.len() > 512 {
            return Err("that is longer than any pairing string".into());
        }
        let body = shown.strip_prefix("sqx-pair:").unwrap_or(shown).trim();
        let (who, where_) = body
            .rsplit_once('@')
            .ok_or_else(|| "a pairing string is <account or name>@<domain>".to_string())?;
        let who = who.trim();
        if who.is_empty() {
            return Err("nothing before the @".into());
        }
        let mut domains: Vec<String> = Vec::new();
        for d in where_.split(',') {
            let d = d.trim().to_ascii_lowercase();
            if d.is_empty() {
                continue;
            }
            if !d.contains('.')
                || d.chars()
                    .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
            {
                return Err(format!("`{d}` is not a domain name"));
            }
            if !domains.contains(&d) {
                domains.push(d);
            }
        }
        if domains.is_empty() {
            return Err("a pairing string names at least one domain".into());
        }
        // A key stands alone; a name lives at its one domain and is handed
        // on as `name@domain`, which the session resolves.
        let owner = if who.parse::<sqnr_core::PubKey>().is_ok() {
            who.to_string()
        } else {
            if domains.len() != 1 {
                return Err("a name lives at one domain".into());
            }
            format!("{}@{}", who.to_ascii_lowercase(), domains[0])
        };
        Ok((owner, domains))
    }

    #[cfg(test)]
    mod tests {
        use super::parse;

        #[test]
        fn a_pair_string_yields_its_owner_and_domains() {
            let (owner, domains) = parse("sqx-pair:colin@squic.org").unwrap();
            assert_eq!(owner, "colin@squic.org");
            assert_eq!(domains, ["squic.org"]);
            let key = sqnr_core::PubKey::new([3; 32]).to_string();
            let (owner, domains) =
                parse(&format!("sqx-pair:{key}@Squic.org,trunk.exchange")).unwrap();
            assert_eq!(owner, key);
            assert_eq!(domains, ["squic.org", "trunk.exchange"]);
            assert!(parse("sqx-pair:nobody").is_err());
            assert!(parse("colin@squic.org,trunk.exchange").is_err());
        }
    }
}

#[cfg(test)]
mod carried_tests {
    use super::direct_allowed;

    /// SIP-85 §What the member MUST NOT do: on a connection the home
    /// carries, no introduction is asked for, whatever the invitation and
    /// the preference say -- the address the exchange would introduce is
    /// the home's, which answers for nobody.
    #[test]
    fn a_carried_connection_never_asks_for_an_introduction() {
        assert!(
            direct_allowed(true, true, false),
            "direct, when everybody agrees"
        );
        assert!(!direct_allowed(true, true, true), "carried: never");
        assert!(
            !direct_allowed(false, true, false),
            "the invitation did not say so"
        );
        assert!(
            !direct_allowed(true, false, false),
            "this side does not allow it"
        );
    }
}

/// What a saved file is called.
#[cfg(test)]
mod save_name_tests {
    use super::*;

    /// The shape the user asked for, and the extension read out of the
    /// bytes: `sigil-2026-09-22-18-32-05.png`.
    #[test]
    fn a_saved_file_is_named_for_its_moment_and_its_kind() {
        let png = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR".to_vec();
        let said = save_name(1_758_559_925, 0, Some(&png));
        assert!(said.starts_with("sigil-"), "{said}");
        assert!(said.ends_with(".png"), "{said}");
        // Date, clock and extension: nothing else, and nothing a
        // filesystem argues with.
        assert_eq!(said.len(), "sigil-".len() + 19 + ".png".len(), "{said}");
        assert!(
            said.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.'),
            "{said}"
        );
    }

    /// A clip is a clip, and what nothing recognises is not given an
    /// extension that would hand it to the wrong program. **The negative
    /// control**: a name built from the kind rather than the bytes would
    /// say `.png` for all three of these.
    #[test]
    fn the_extension_follows_the_bytes_and_not_a_guess() {
        let mp4 = b"\x00\x00\x00\x18ftypmp42".to_vec();
        assert!(save_name(1_758_559_925, 0, Some(&mp4)).ends_with(".mp4"));
        assert!(save_name(1_758_559_925, 0, Some(b"nothing known")).ends_with(".bin"));
        assert!(save_name(1_758_559_925, 0, None).ends_with(".bin"));
    }

    /// Two files of the *same* message are two names -- a gallery's
    /// pictures share a moment, and the second landing on the first is a
    /// picture somebody saved and no longer has.
    #[test]
    fn two_files_of_one_message_are_two_names() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        assert_ne!(
            save_name(1_758_559_925, 0, Some(&png)),
            save_name(1_758_559_925, 1, Some(&png))
        );
    }

    /// And two moments are two names.
    #[test]
    fn a_later_message_gets_a_later_name() {
        let png = b"\x89PNG\r\n\x1a\n".to_vec();
        assert_ne!(
            save_name(1_758_559_925, 0, Some(&png)),
            save_name(1_758_559_999, 0, Some(&png))
        );
    }
}
