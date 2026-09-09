//! Messaging, as a sigil app.

pub mod session;

pub use session::{
    Attached, ChatHandle, ChatState, Closing, Cmd, Found, Happened, Hit, Line, LinkState, Linked,
    Member, Person, Receipt, Ring, Standing, Summary, Trouble,
};

use std::collections::HashMap;

use sigil::app::{App, AppContext, AppResponse, TabNotifications};
use sigil::{ColorTheme, tokens};
use sigil_net::discovery;
use sqnr::config::Config;
use sqnr_core::PubKey;

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
}

/// What to call the default exchange in the switcher.
///
/// The default has no name in the roster — it is whatever this identity's own
/// SIP-38 handle and `~/.sqnr/config` resolve to — so it was labelled with a
/// truncated public key. That is unreadable and says nothing about *where* it
/// is, which is the only question a switcher answers. The domain it was
/// discovered at is the answer when there is one; the key is the fallback,
/// because a connection made to an address has no domain to report and a key
/// is still better than a word that names nothing.
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
fn ring_said(from: &PubKey, label: &str, called: &str, held: usize) -> String {
    let who = sigil_ui::message::short(&from.to_string());
    if held > 1 {
        format!("{label} — from {who}, to {called}")
    } else {
        format!("{label} — from {who}")
    }
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

/// What is being typed, per identity.
///
/// **Keyed by account, not shared.** A draft typed as one identity must not
/// still be in the box after switching to another: the next Return would send
/// it as somebody else, which is a mistake the interface would have made on
/// your behalf and not mentioned.
struct Pane {
    /// Kept out of the session so that a failed send leaves it on screen:
    /// retyping a message the program lost is the worst thing a chat client can
    /// do to somebody.
    composing: String,
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
    /// The key of a device being linked.
    linking: String,
    /// A credential another device wrote, being presented by this one.
    presenting: String,
    /// The message search box.
    searching: String,
    /// The exchange being added.
    exchange: String,
    /// The message whose file is being forwarded.
    forwarding: Option<u64>,
    /// The picture being looked at full size: a message and which of its
    /// files. Kept per identity like everything else here, so switching away
    /// and back does not leave somebody else's picture over the screen.
    viewing: Option<(u64, usize)>,
    /// The key being invited to the open channel.
    inviting: String,
    /// The SIP-38 name being claimed.
    naming: String,
    /// The channel settings fields.
    channel_name: String,
    channel_topic: String,
    retention_days: u32,
    /// Destroying a channel is asked twice, because it cannot be undone.
    confirming_destroy: bool,
    /// What is being typed into it. Held separately from the published
    /// profile so cancelling really cancels.
    name: String,
    title: String,
}

impl Default for Pane {
    fn default() -> Self {
        Pane {
            composing: String::new(),
            adding: String::new(),
            add_trouble: None,
            dialog: None,
            replying: None,
            announced_typing: false,
            editing: None,
            query: String::new(),
            linking: String::new(),
            presenting: String::new(),
            searching: String::new(),
            exchange: String::new(),
            forwarding: None,
            viewing: None,
            inviting: String::new(),
            naming: String::new(),
            channel_name: String::new(),
            channel_topic: String::new(),
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
    /// Sessions told to stop that still hold their store lock. One of these
    /// must not be reopened yet; see [`Closing`].
    closing: Vec<(At, Closing)>,
    panes: HashMap<At, Pane>,
    /// Which exchange is being shown, for each identity. Absent means the
    /// default one.
    showing: HashMap<PubKey, String>,
    /// Whether the conversation column is on screen. **Closed to begin with.**
    ///
    /// One preference for the whole app rather than one per identity: it is
    /// about how much room the transcript gets, which is a fact about the
    /// window and not about who you are being in it.
    ///
    /// The control that opens it lives in the conversation's own bar and is
    /// drawn whenever the column is away, so starting closed hides the list
    /// and never the way to it.
    columns_open: bool,
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
    /// The call this identity is carrying audio for, and which invitation it
    /// belongs to.
    ///
    /// Signalling lives in the session (it is chat traffic); the audio is a
    /// SIP-13 room on its own connection, which is `sigil-net`'s job. This is
    /// the join between them, and nothing else needs to know both halves.
    calls: HashMap<PubKey, Live>,
    /// Calls already announced, so a ring is said out loud once and not on
    /// every pass for as long as it rings.
    announced: std::collections::HashSet<([u8; 32], u64)>,
}

/// A call this client is actually carrying audio for.
struct Live {
    channel: [u8; 32],
    /// The invitation's `seq`. Everything about a call is keyed on it.
    seq: u64,
    handle: sigil_net::CallHandle,
    /// When we joined, for the duration written into the closing entry.
    since: std::time::Instant,
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
            closing: Vec::new(),
            panes: HashMap::new(),
            showing: HashMap::new(),
            // Closed. The conversation is what somebody opened sigil to read,
            // and a list of the others beside it is a thing they ask for.
            columns_open: false,
            config: Config::load(),
            store_root: None,
            now: None,
            fixed: None,
            calls: HashMap::new(),
            announced: std::collections::HashSet::new(),
        }
    }

    /// Keep the stores somewhere other than `~/.sqex/chat`. Tests only.
    #[doc(hidden)]
    pub fn set_store_root_for_test(&mut self, root: std::path::PathBuf) {
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

    /// The account being shown, if it is open.
    fn showing(ctx: &AppContext<'_>) -> Option<PubKey> {
        ctx.account().unlocked().map(|u| u.me())
    }

    /// The identity **and exchange** being shown.
    ///
    /// Both, because a conversation list belongs to a pair: the identity is
    /// the same key at every exchange and its conversations are not.
    fn showing_at(&self, ctx: &AppContext<'_>) -> Option<At> {
        let me = Self::showing(ctx)?;
        let live: Vec<At> = self.sessions.keys().cloned().collect();
        Some((me, showing_exchange(me, self.showing.get(&me), &live)))
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
    pub fn show_state_for_test(&mut self, state: ChatState) {
        self.fixed = Some(state);
    }

    fn pane(&mut self, at: &At) -> &mut Pane {
        self.panes.entry(at.clone()).or_default()
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
        let mut held: Vec<(At, std::path::PathBuf)> = Vec::new();
        for one in ctx.accounts.all() {
            let Some(unlocked) = one.account().unlocked() else {
                continue;
            };
            for exchange in one.exchanges() {
                held.push(((unlocked.me(), exchange), unlocked.path().to_path_buf()));
            }
        }

        // Stop anything no longer held. This is the half that matters: a
        // session left running for a discarded identity keeps connecting,
        // keeps succeeding, and is the wrong person.
        let live: Vec<At> = self.sessions.keys().cloned().collect();
        for at in live {
            if !held.iter().any(|(k, _)| *k == at) {
                if let Some(session) = self.sessions.remove(&at) {
                    self.closing.push((at.clone(), session.close()));
                }
                self.panes.remove(&at);
            }
        }

        for (at, path) in held {
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
            // **One store file per identity, shared by its exchanges.** The
            // store scopes every row by exchange and the lock is per (account,
            // exchange), so two sessions on one file do not collide -- and a
            // file each would put one identity's contact list in two places.
            let store_at = self
                .store_root
                .as_ref()
                .map(|root| root.join(format!("{me}.db")));
            let wake = egui_ctx.clone();
            self.sessions.insert(
                at,
                session::start(layers, unlocked.signer(), store_at, move || {
                    wake.request_repaint()
                }),
            );
        }
    }

    fn send_as(&mut self, at: Option<&At>, cmd: Cmd) {
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

impl App for ChatApp {
    fn render_nav(
        &mut self,
        ctx: &mut AppContext<'_>,
        ui: &mut egui::Ui,
        token: &std::rc::Rc<dyn std::any::Any>,
    ) -> AppResponse {
        match Self::route(token) {
            Route::Conversations => self.render(ctx, ui),
            Route::Directory => self.directory_view(ctx, ui),
            Route::Members => self.members_view(ctx, ui),
            Route::Settings => self.settings_view(ctx, ui),
            Route::Devices => self.devices_view(ctx, ui),
        }
    }

    fn nav_title(&self, token: &std::rc::Rc<dyn std::any::Any>) -> Option<String> {
        Some(
            match Self::route(token) {
                Route::Conversations => return None,
                Route::Directory => "Public channels",
                Route::Members => "Members",
                Route::Settings => "Channel settings",
                Route::Devices => "Devices",
            }
            .to_string(),
        )
    }

    fn update(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        self.reconcile(ctx, egui_ctx);
        self.announce_rings(ctx);
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
            self.session_bar_ui(ctx, at, &none, ui, &theme);
            self.unconnected_ui(ctx, at, ui, &theme);
            return AppResponse::default();
        }
        let state = self.state_of(Some(at));

        // Before anything else, and **outside every branch below**. It hung
        // off the conversation list, which is not drawn at all when the column
        // is hidden or when a narrow window is showing a conversation -- so a
        // dialog opened and then collapsed behind was one nobody could get out
        // of, and it also has several early returns under it.
        self.dialogs_ui(ctx, at, &state, ui, &theme);
        self.picture_ui(at, &state, ui, &theme);

        // Two panes when there is room, one when there is not -- decided at
        // **runtime** from the width actually available, never from the
        // platform. Narrowing a desktop window has to collapse the layout
        // live, and a phone-shaped window on a desktop is a real thing.
        //
        // `sigil::layout` is the shared rule for this, so the deck and the
        // conversation view cannot drift into two answers about what "narrow"
        // means.
        match sigil::layout(ui.available_width(), 2) {
            sigil::Layout::Single => {
                // One pane: the list until something is open, then the
                // conversation with a way back. Not both squeezed together --
                // two unusable columns are worse than one usable one.
                match state.open {
                    None => {
                        // One pane and nothing open: the bar has nowhere else
                        // to be, and the list is the whole window.
                        self.session_bar_ui(ctx, at, &state, ui, &theme);
                        self.list_ui(ctx, at, &state, ui, &theme);
                    }
                    Some(_) => {
                        ui.horizontal(|ui| {
                            if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                                self.send_as(Some(at), Cmd::Close);
                            }
                        });
                        self.session_bar_ui(ctx, at, &state, ui, &theme);
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
                        .show(ui, |ui| self.list_ui(ctx, at, &state, ui, &theme));
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
                        self.session_bar_ui(ctx, at, &state, ui, &theme);
                        self.transcript_ui(ctx, at, &state, ui, &theme);
                    });
            }
        }
        AppResponse::default()
    }

    /// Unread across **every** identity, not the one on screen.
    ///
    /// The badge is what tells somebody to come back, and an account they are
    /// not currently looking at is exactly the one they would otherwise miss.
    fn tab_notifications(&self) -> TabNotifications {
        TabNotifications::count(
            self.sessions
                .values()
                .flat_map(|s| s.state().conversations)
                .map(|c| c.unread as u32)
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
        let held = ctx.accounts.active_held().exchanges();
        let only = held.len() == 1;
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
            if ui.button("Add an exchange").clicked() {
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

    /// The one row about this session: whether the link is up, and who you are.
    ///
    /// **Over the conversation and not over the whole window.** It used to
    /// span both, which cost the conversation column its top and made the list
    /// start under a bar that has nothing to do with it. The column is the
    /// full height of the application now, the way every client with a sidebar
    /// draws one, and this belongs to the pane it sits over.
    fn session_bar_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        // One row, laid out from the right: who you are, and beside it whether
        // the link is up. Both are facts about *this session* rather than
        // about any conversation, so they share a corner.
        ui.horizontal(|ui| {
            // Only when there is nothing to bring back does this appear, so
            // the bar is not carrying a control that does nothing.
            if !self.columns_open
                && sigil_ui::icon_button_named(ui, sigil_ui::Icon::Menu, "Show the chats").clicked()
            {
                self.columns_open = true;
            }
            let colour = match state.link {
                LinkState::Up => theme.link_up,
                // Not up, and not yet an outage either.
                LinkState::Connecting | LinkState::Retrying => theme.link_retrying,
                LinkState::Gone => theme.link_gone,
            };
            let up = state.link == LinkState::Up;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.me_ui(ctx, at, state, ui, theme);
                ui.add_space(tokens::SPACING_SM);
                // **The word appears when it is worth reading.** A link that
                // is up is the ordinary case and a green dot says it. A link
                // that is not is the case where nothing arriving looks exactly
                // like nobody writing, and no colour can tell somebody that --
                // so that one keeps its word, and its way back.
                //
                // Either way the word is on the dot's hover and in the
                // accessibility tree, where a colour reaches nobody at all.
                if !up {
                    if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Refresh, "Reconnect")
                        .clicked()
                    {
                        self.send_as(Some(at), Cmd::Reconnect);
                    }
                    ui.colored_label(colour, state.link.word());
                }
                sigil_ui::dot(ui, up, colour, colour, state.link.word());

                // Whatever was just done, at the other end of the row. It was
                // above the transcript, where it pushed every message down by
                // a line for a moment and then let them back up.
                //
                // A note is about an **action** and a trouble is about a
                // state; the state is rebuilt every refresh, so merging them
                // would put each confirmation on screen for less than a tick.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    if let Some(note) = &state.note {
                        ui.add(
                            egui::Label::new(egui::RichText::new(&note.said).color(theme.success))
                                .truncate(),
                        );
                    }
                });
            });
        });
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
                        self.showing.remove(&at.0);
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
        let others: Vec<(At, Option<PubKey>)> = self
            .sessions
            .iter()
            .map(|(other, session)| (other.clone(), session.state().exchange))
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
    ) {
        let me = at.0;
        let key = me.to_string();

        // Laid out from the right, which is where this sits: the chevron
        // first, then the name, then the picture.
        let chevron = sigil_ui::icon_button_named(ui, sigil_ui::Icon::Chevron, "Your identity")
            .on_hover_text("Your key, your exchanges, and the other identities you hold");
        // Wide enough for the two lines and no wider, right-aligned inside it.
        //
        // A bare `vertical` takes all the space left on the row, so in a
        // right-to-left layout it began at the far left and drew the name
        // straight over the connection state. A fixed width fixed that and
        // left the picture stranded 200px from its own name, so the width is
        // measured instead.
        let label = state.mine.label(&me);
        // Whatever the second line will actually say, so the block is wide
        // enough for it. Measured against an empty string when there is no
        // handle, "unregistered" wrapped onto a third line and the block grew
        // downwards instead of leftwards.
        let second = state
            .mine
            .handle
            .clone()
            .unwrap_or_else(|| UNREGISTERED.to_string());
        let measure = |text: &str, style: egui::TextStyle| {
            ui.ctx()
                .fonts_mut(|f| {
                    f.layout_no_wrap(
                        text.to_string(),
                        style.resolve(ui.style()),
                        theme.text_primary,
                    )
                })
                .rect
                .width()
        };
        let width = measure(&label, egui::TextStyle::Body)
            .max(measure(&second, egui::TextStyle::Small))
            .clamp(60.0, 220.0);
        ui.allocate_ui_with_layout(
            egui::vec2(width, tokens::AVATAR_MD),
            egui::Layout::top_down(egui::Align::Max),
            |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(state.mine.label(&me)).strong())
                        .truncate(),
                );
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
                        // A control, not a note. "You have no name here" is
                        // only useful beside the way to get one, and the way
                        // to get one is a claim at this exchange.
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
                        }
                    }
                }
            },
        );
        sigil_ui::identicon(ui, &key, tokens::AVATAR_MD);

        egui::Popup::menu(&chevron).show(|ui| {
            ui.set_min_width(320.0);
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

        // In full, selectable, and wrapped rather than clipped. A name is an
        // assertion (SIP-21) and this is not -- it is the only thing that
        // identifies you to somebody who wants to write to you.
        ui.colored_label(theme.text_muted, egui::RichText::new("You").small());
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
            ui.colored_label(theme.text_muted, egui::RichText::new("Name here").small());
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
            ui.separator();
        }

        if ui.button("Edit your profile").clicked() {
            let (name, title) = (
                state.mine.name.clone().unwrap_or_default(),
                state.mine.title.clone().unwrap_or_default(),
            );
            let pane = self.panes.entry(at.clone()).or_default();
            // Seeded from what is published, so the dialog opens on what is
            // true rather than on an empty box that would read as "you have no
            // name".
            pane.name = name;
            pane.title = title;
            pane.dialog = Some(Dialog::Profile);
            ui.close();
        }

        ui.separator();
        self.exchanges_ui(ctx, at, state, ui, theme);

        // Every identity this host is holding. **All of them are live** --
        // switching changes what is drawn and stops nothing, so there is
        // nothing here to warn about losing.
        if ctx.accounts.len() > 1 {
            ui.separator();
            ui.colored_label(theme.text_muted, egui::RichText::new("Identities").small());
            let active = ctx.accounts.active_index();
            for i in 0..ctx.accounts.len() {
                let label = ctx.accounts.label(i);
                let open = ctx.accounts.get(i).is_some_and(|a| a.is_unlocked());
                // A sealed account reads differently from an open one, because
                // choosing it gets a passphrase field and not a conversation.
                let text = if open {
                    egui::RichText::new(label)
                } else {
                    egui::RichText::new(format!("{label} (locked)")).color(theme.text_muted)
                };
                let selected = i == active;
                let response = ui.selectable_label(selected, text);
                // The full key on hover, wherever a short form is shown.
                if let Some(unlocked) = ctx.accounts.get(i).and_then(|a| a.unlocked()) {
                    response.clone().on_hover_text(unlocked.me().to_string());
                }
                if response.clicked() && !selected {
                    ctx.accounts.switch_to(i);
                    ui.close();
                }
            }
        }
        let _ = me;
    }

    /// Which exchange this identity is talking to, and how to add another.
    ///
    /// # Why this is not a setting
    ///
    /// The identity is the same key at every exchange, and **nothing else
    /// is**. Conversations, channel keys and SIP-17 counters belong to one
    /// exchange and do not move — SIP-31 binds the exchange into every entry
    /// signature so that they cannot. Adding one is therefore much closer to
    /// adding an account than to changing a preference, and switching between
    /// them changes the whole conversation list.
    fn exchanges_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let me = at.0;
        let named = ctx.accounts.active_held().exchanges();

        // Only when there is a choice. A switcher over one exchange is a
        // control that cannot do anything.
        if named.len() > 1 {
            let which = ctx.accounts.active_index();
            for name in &named {
                ui.horizontal(|ui| {
                    let selected = *name == at.1;
                    let label = if name.is_empty() {
                        // The default has no name in the roster, so it is
                        // labelled with what it turned out to be: the domain
                        // it was discovered at, and only failing that the key.
                        // A truncated key is unreadable and says nothing about
                        // *where* it is, which is the whole question a
                        // switcher answers.
                        //
                        // Read from the default's **own** session rather than
                        // from whichever one is on screen: this row is about
                        // the default whether or not the default is what is
                        // being shown.
                        default_label(self.sessions.get(&(me, String::new())).map(|s| s.state()))
                    } else {
                        name.clone()
                    };
                    if ui.selectable_label(selected, label).clicked() && !selected {
                        self.showing.insert(me, name.clone());
                    }
                    // **A way out, beside the way in.** There was a control to
                    // add an exchange and none to remove one, so a name added
                    // by mistake -- or one that turned out to be the default
                    // under another spelling -- could only be taken back by
                    // editing the roster file by hand.
                    //
                    // The default is not one of these: it is not a name in the
                    // roster, it is whatever this identity resolves to, and
                    // there would be nothing to remove.
                    if name.is_empty() {
                        return;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Close, "Remove")
                            .on_hover_text(
                                "Stop connecting to this exchange. Nothing said there is \
                                 deleted -- the conversations stay in this store and come \
                                 back if it is added again.",
                            )
                            .clicked()
                        {
                            ctx.accounts.drop_exchange(which, name);
                            // Back to the default, or the interface would be
                            // showing a conversation list for an exchange it
                            // is no longer connected to.
                            self.showing.remove(&me);
                        }
                    });
                });
            }
        }
        // The full key of whatever is being talked to, always reachable, and
        // **labelled** -- unlabelled beside the account's own key it was just
        // a second string of base58 with nothing saying which was which.
        ui.horizontal(|ui| {
            ui.colored_label(theme.text_muted, egui::RichText::new("at").small());
            match state.exchange {
                Some(key) => {
                    ui.add(
                        egui::Label::new(egui::RichText::new(key.to_string()).monospace().small())
                            .wrap()
                            .selectable(true),
                    )
                    .on_hover_text("the exchange this conversation list belongs to");
                }
                None => {
                    ui.colored_label(theme.text_muted, egui::RichText::new("connecting…").small());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Plus, "Add an exchange")
                    .on_hover_text("Connect this identity to another exchange")
                    .clicked()
                {
                    self.panes.entry(at.clone()).or_default().dialog = Some(Dialog::Exchange);
                }
            });
        });
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
        // Gone from under it — the message was deleted, or the conversation
        // changed — is not an error, it is nothing to show.
        let Some(file) = state
            .lines
            .iter()
            .find(|l| l.seq == seq)
            .and_then(|l| l.attachments.get(index))
        else {
            self.pane(at).viewing = None;
            return;
        };
        let Some(bytes) = file.bytes.clone() else {
            self.pane(at).viewing = None;
            return;
        };

        let egui_ctx = ui.ctx().clone();
        // The window, so a big picture fills it and a small one does not
        // grow. `available_rect` is the whole surface here: this draws over
        // everything by construction.
        let screen = ui.ctx().viewport_rect().size();
        let response = egui::Modal::new(egui::Id::new(("picture", seq, index)))
            .frame(
                egui::Frame::NONE
                    .fill(theme.surface_primary)
                    .corner_radius(tokens::RADIUS_LG)
                    .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8)),
            )
            .show(&egui_ctx, |ui| {
                ui.vertical_centered(|ui| {
                    // The same URI the transcript uses, so the decoded texture
                    // is the one already in hand rather than a second copy of
                    // the same picture under another name.
                    ui.add(
                        egui::Image::from_bytes(format!("bytes://{}", file.id), bytes)
                            .max_size(screen * 0.86)
                            .corner_radius(tokens::RADIUS_MD),
                    );
                    ui.add_space(tokens::SPACING_SM);
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            theme.text_muted,
                            egui::RichText::new(&file.described).small(),
                        );
                        if ui.button("Save…").clicked()
                            && let Some(to) = rfd::FileDialog::new().save_file()
                        {
                            self.send_as(Some(at), Cmd::SaveFile { seq, index, to });
                        }
                        if sigil_ui::icon_button(ui, sigil_ui::Icon::Close).clicked() {
                            self.pane(at).viewing = None;
                        }
                    });
                });
            });
        // The backdrop and Escape, which `should_close` covers, and the
        // control above. Three ways out of something that covers the window.
        if response.should_close() {
            self.pane(at).viewing = None;
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
                ui.set_width(360.0);
                match which {
                    Dialog::Compose => self.compose_dialog(at, ui, theme),
                    Dialog::Profile => self.profile_dialog(at, ui, theme),
                    Dialog::Exchange => self.exchange_dialog(ctx, at, me, ui, theme),
                    Dialog::Name => self.name_dialog(at, ui, theme),
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
        ui.label("Write to");
        let width = ui.available_width();
        let field = sigil_ui::field(
            ui,
            &mut self.panes.entry(at.clone()).or_default().adding,
            "paste their key, or type name@domain",
            width,
        );
        let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(tokens::SPACING_XS);
        let go = ui.horizontal(|ui| ui.button("Add").clicked()).inner;
        if go || entered {
            let typed = self.pane(at).adding.trim().to_string();
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
        ui.horizontal(|ui| {
            if ui.button("New group").clicked() {
                // A group's name is a sealed entry, so it is named after it
                // exists rather than before.
                self.pane(at).dialog = None;
                self.send_as(Some(at), Cmd::NewGroup("New group".into()));
            }
            if ui
                .button("New public channel")
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
        });
        // Said beside the control, not in a help page. A public channel is
        // plaintext by design -- anybody may join, so any key in it is public.
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new("A public channel is not encrypted.").small(),
        );
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
        ui.heading("Claim a name");
        ui.add_space(tokens::SPACING_SM);
        ui.label("Name");
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
        ui.horizontal(|ui| {
            if ui.button("Claim").clicked() || entered {
                let name = self.pane(at).naming.trim().to_string();
                if !name.is_empty() {
                    self.pane(at).naming.clear();
                    self.pane(at).dialog = None;
                    self.send_as(Some(at), Cmd::ClaimName(name));
                }
            }
            if ui.button("Cancel").clicked() {
                let pane = self.pane(at);
                pane.naming.clear();
                pane.dialog = None;
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
        if let Some(trouble) = self.panes.get(at).and_then(|p| p.add_trouble.clone()) {
            ui.colored_label(theme.destructive, trouble);
        }
        ui.add_space(tokens::SPACING_SM);
        ui.horizontal(|ui| {
            if ui.button("Add").clicked() {
                let named = self.pane(at).exchange.trim().to_string();
                if ctx.accounts.add_exchange(which, &named) {
                    let pane = self.pane(at);
                    pane.exchange.clear();
                    pane.add_trouble = None;
                    pane.dialog = None;
                    // Shown straight away: adding one and staying where you
                    // were makes it look as though nothing happened.
                    self.showing.insert(me, named);
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
                pane.dialog = None;
            }
        });
    }

    /// The conversation list.
    fn list_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        at: &At,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let now = self.now();
        // The heading carries the two things you do *to* the list, rather
        // than each having a row of its own below it. Both are icons: a word
        // in a heading row reads as part of the heading, not as a control.
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // Beside the heading, and it puts the column away. The
                    // control that brings it back is in the conversation's own
                    // bar, because a control inside the thing it hides is a
                    // control nobody can reach once they have used it.
                    if sigil_ui::icon_button_named(ui, sigil_ui::Icon::Menu, "Hide the chats")
                        .clicked()
                    {
                        self.columns_open = false;
                    }
                    ui.heading("Chats");
                });
            });
        });
        ui.add_space(tokens::SPACING_XS);

        ui.horizontal(|ui| {
            let control = tokens::BUTTON_MD + ui.spacing().item_spacing.x * 2.0;
            let width = ui.available_width() - control;
            // No label beside it. A search box is the one control everybody
            // recognises without being told, and the word is still on the
            // magnifier next to it -- which is a button, so it reaches the
            // accessibility tree where a placeholder would not.
            let field = sigil_ui::field(
                ui,
                &mut self.panes.entry(at.clone()).or_default().searching,
                "Search chats",
                width,
            );
            if field.changed() {
                let query = self.pane(at).searching.clone();
                self.send_as(Some(at), Cmd::Search(query));
            }
            let searching = !self.pane(at).searching.is_empty();
            // The control tells you what it will do: clear the search while
            // there is one, and otherwise say what the box is for.
            if searching {
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Close).clicked() {
                    self.pane(at).searching.clear();
                    self.send_as(Some(at), Cmd::Search(String::new()));
                }
            } else if sigil_ui::icon_button(ui, sigil_ui::Icon::Search).clicked() {
                // Focuses the box rather than doing nothing: it is beside a
                // field and the obvious thing to press first.
                field.request_focus();
            }
        });

        ui.add_space(tokens::SPACING_SM);

        // A search replaces the list while there is one. The list is still
        // there underneath, and clearing the box brings it back.
        if !self.pane(at).searching.trim().is_empty() {
            // Said every time, not once in a help page: an empty result here
            // means "not in what this client has opened", which is a different
            // fact from "never said", and only this client can tell them apart.
            ui.colored_label(
                theme.text_muted,
                egui::RichText::new(
                    "Searches what this client has opened. The exchange holds ciphertext \
                     and cannot search it.",
                )
                .small(),
            );
            if state.hits.is_empty() {
                ui.colored_label(
                    theme.text_secondary,
                    if state.searched_messages {
                        "Nothing here matched."
                    } else {
                        "…"
                    },
                );
                return;
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for hit in &state.hits {
                        let response = ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&hit.label).strong().small());
                            ui.colored_label(
                                theme.text_secondary,
                                // A preview, not eight characters: a search
                                // result you cannot read is a result you have
                                // to open to reject.
                                egui::RichText::new(sigil_ui::message::preview(&hit.text, 64))
                                    .small(),
                            );
                            ui.colored_label(
                                theme.text_muted,
                                egui::RichText::new(sigil_ui::brief(hit.at, now)).small(),
                            );
                        });
                        if response.response.interact(egui::Sense::click()).clicked() {
                            self.send_as(Some(at), Cmd::Show(hit.channel));
                        }
                        ui.separator();
                    }
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

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for convo in &state.conversations {
                    let id = bs58::encode(convo.channel).into_string();
                    let key = convo.peer.map(|p| p.to_string());
                    let selected = state.open == Some(convo.channel);
                    let row = sigil_ui::ConversationRow {
                        id: &id,
                        label: &convo.label,
                        key: key.as_deref(),
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
        let me = at.0;
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

        // The header: what this conversation is, and the way into everything
        // that can be done about it. One row, and it stays one row -- the
        // controls wrapped onto a second line as soon as a count appeared
        // beside them, which made the header jump about as members arrived.
        ui.horizontal(|ui| {
            let open = state
                .conversations
                .iter()
                .find(|c| Some(c.channel) == state.open);
            let label = open.map(|c| c.label.clone()).unwrap_or_default();
            // A direct message has two people in it and cannot have any other
            // number, so the count is a fact about the kind of conversation
            // and not about this one.
            let dm = open.is_some_and(|c| c.peer.is_some());
            // **No calling a public channel.** Anybody may join one, so the
            // ring would go to a membership nobody chose and the room secret
            // — a bearer capability, SIP-36 — would be handed to whoever
            // turned up next. There is nothing to fix about that at the point
            // somebody presses it, so the control is not there.
            let public = open.is_some_and(|c| c.public);

            // **The controls are laid out first, from the right.** Given the
            // name first, a long one takes the row and the controls wrap onto
            // a second line -- which is what this did, and it moved the header
            // about as a member count appeared.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Settings).clicked() {
                    ctx.navigator.push_here(Route::Settings);
                }
                if sigil_ui::icon_button(ui, sigil_ui::Icon::Device).clicked() {
                    self.send_as(Some(at), Cmd::Devices);
                    ctx.navigator.push_here(Route::Devices);
                }
                // The one that keeps a number beside it: an icon can say
                // "members" and cannot say "four of them", and the count is
                // half of what somebody wants from this control. Not in a
                // direct message, where it is always two and says nothing.
                let members = state.members.len();
                if members > 0 && !dm {
                    ui.colored_label(theme.text_muted, members.to_string());
                }
                if sigil_ui::icon_button(ui, sigil_ui::Icon::People).clicked() {
                    self.send_as(Some(at), Cmd::Blocked);
                    ctx.navigator.push_here(Route::Members);
                }
                if !public
                    && !self.calls.contains_key(&me)
                    && !state.ringing.iter().any(|r| r.mine)
                    && sigil_ui::icon_button(ui, sigil_ui::Icon::Call).clicked()
                {
                    self.send_as(Some(at), Cmd::Call);
                }

                // Whatever is left is the name's, and it truncates rather than
                // pushing anything off the row.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(label).heading()).truncate());
                    if !state.topic.is_empty() {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&state.topic).color(theme.text_secondary),
                            )
                            .truncate(),
                        );
                    }
                });
            });
        });
        if self.ringing_ui(ctx, at, state, ui, theme) {
            ui.add_space(tokens::SPACING_SM);
        }
        self.in_call_ui(at, ui, theme);
        // A call we placed that nobody has taken yet.
        if let Some(ring) = state.ringing.iter().find(|r| r.mine) {
            let (channel, seq) = (ring.channel, ring.seq);
            ui.horizontal(|ui| {
                ui.colored_label(theme.text_secondary, "Ringing…");
                // Named for what it does here: giving up on a call nobody has
                // taken is not the same act as ending one in progress.
                if sigil_ui::icon_button_named(ui, sigil_ui::Icon::HangUp, "Cancel the call")
                    .clicked()
                {
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
        }
        self.trouble_ui(&state.trouble_with, ui, theme);

        // The composer is laid out first, from the bottom, so the transcript
        // gets the remaining height rather than pushing it off the screen.
        egui::Panel::bottom("chat_composer")
            .frame(
                egui::Frame::NONE
                    .fill(theme.surface_primary)
                    .inner_margin(egui::Margin::symmetric(0, tokens::SPACING_SM as i8)),
            )
            .show(ui, |ui| self.composer_ui(at, state, ui, theme));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        self.messages_ui(at, state, ui, theme, now);
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
    fn trouble_ui(&self, trouble: &Trouble, ui: &mut egui::Ui, theme: &ColorTheme) {
        if trouble.is_clear() {
            return;
        }
        let mut say = |colour: egui::Color32, text: String| {
            ui.colored_label(colour, text);
        };
        if let Some(epoch) = trouble.no_key {
            // SIP-17's stranded member: every entry fetches and none of them
            // open. Without this the conversation simply reads as empty, which
            // is indistinguishable from nobody having written.
            say(
                theme.destructive,
                format!(
                    "You hold no key for this conversation (epoch {epoch}). \
                     An admin has to hand you one before anything here can be read."
                ),
            );
        }
        if trouble.unreadable > 0 {
            say(
                theme.warning,
                match trouble.unreadable {
                    1 => "1 message here has not been opened yet — its key may still arrive."
                        .to_string(),
                    n => format!(
                        "{n} messages here have not been opened yet — their key may still arrive."
                    ),
                },
            );
        }
        if trouble.lost > 0 {
            say(
                theme.destructive,
                match trouble.lost {
                    1 => "1 message here can never be read: its key is gone.".to_string(),
                    n => format!("{n} messages here can never be read: their key is gone."),
                },
            );
        }
        if trouble.gap {
            say(
                theme.text_secondary,
                "Older messages have passed this channel's retention window and are gone."
                    .to_string(),
            );
        }
        if trouble.restarted {
            say(
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
                ui.colored_label(theme.text_secondary, "Nothing here yet.");
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
                // Asked for by reaching the top as well as by pressing it.
                // Scrolling is how anybody actually gets there, and a control
                // that only answers a click makes somebody hunt for a button
                // they have already scrolled past.
                let reached = ui.clip_rect().contains(more.rect.center());
                if more.clicked() || reached {
                    self.send_as(Some(at), Cmd::Earlier);
                }
            });
            ui.add_space(tokens::SPACING_SM);
        }

        let mut acted: Option<(u64, String, PubKey, sigil_ui::BubbleAction)> = None;
        let mut previous_day: Option<String> = None;
        let mut previous_author: Option<PubKey> = None;
        let mut previous_at: u64 = 0;

        // What happened to the channel, in the order it happened relative to
        // what was said. **Both sequences come from the exchange**, so one
        // pass over the events, advanced as the messages go by, puts each in
        // its place -- rather than a merged list that would have to copy every
        // message to build.
        let mut events = state.events.iter().peekable();

        for line in &state.lines {
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

            let key = line.who.to_string();
            let title = state.people.get(&line.who).and_then(|p| p.title.as_deref());
            let files: Vec<sigil_ui::Attachment<'_>> = line
                .attachments
                .iter()
                .map(|a| sigil_ui::Attachment {
                    kind: a.kind,
                    described: &a.described,
                    preview: &a.preview,
                    bytes: a.bytes.as_deref(),
                    id: &a.id,
                })
                .collect();
            let bubble = sigil_ui::Bubble {
                key: &key,
                name: line.name.as_deref(),
                title,
                text: &line.text,
                at: &sigil_ui::clock(line.at),
                mine: line.mine,
                grouped,
                edited: line.edited,
                redacted: line.redacted,
                reply_to: line
                    .reply_to
                    .as_ref()
                    .map(|(who, said)| (who.as_str(), said.as_str())),
                reactions: &line.reactions,
                receipt: line.receipt.map(|r| match r {
                    Receipt::Sent => sigil_ui::Receipt::Sent,
                    Receipt::Delivered => sigil_ui::Receipt::Delivered,
                    Receipt::Read => sigil_ui::Receipt::Read,
                }),
                attachments: &files,
                standing: line.standing.word().zip(line.standing.means()),
                alarming: line.standing == session::Standing::Fork,
            };
            let did = sigil_ui::bubble(ui, &bubble);
            if !did.is_none() {
                acted = Some((line.seq, line.text.clone(), line.who, did));
            }

            previous_author = Some(line.who);
            previous_at = line.at;
        }

        // And anything after the last message -- somebody removed from a quiet
        // channel would otherwise leave no trace at all.
        for event in events {
            self.event_ui(event, ui);
        }

        if state.typing {
            ui.add_space(tokens::SPACING_SM);
            ui.colored_label(theme.text_muted, "typing…");
        }

        // Where to forward a file to. A list rather than a key field: the
        // destination is always somewhere you are already in.
        if let Some(seq) = self.pane(at).forwarding {
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
                                    index: 0,
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

        if let Some((seq, text, who, did)) = acted {
            if let Some(emoji) = did.react {
                self.send_as(Some(at), Cmd::React { target: seq, emoji });
            }
            if did.reply {
                self.pane(at).replying = Some(seq);
            }
            if did.edit {
                // The text is loaded into the composer so an edit is a
                // correction of what is there rather than a retyping of it.
                self.pane(at).editing = Some(seq);
                self.pane(at).composing = text;
            }
            if did.redact {
                self.send_as(Some(at), Cmd::Redact(seq));
            }
            if did.copy_key {
                ui.ctx().copy_text(who.to_string());
            }
            if did.forward {
                self.pane(at).forwarding = Some(seq);
            }
            if let Some(index) = did.open {
                self.pane(at).viewing = Some((seq, index));
            }
            if let Some(index) = did.save {
                // The dialog is native and blocking, which is fine here: it is
                // a direct answer to a click, and the session goes on running
                // on its own task regardless.
                if let Some(to) = rfd::FileDialog::new().save_file() {
                    self.send_as(Some(at), Cmd::SaveFile { seq, index, to });
                }
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
    fn composer_ui(&mut self, at: &At, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
        // What Enter will do, said above the box. A composer that silently
        // means three different things depending on invisible state is one
        // that will eventually send an edit as a new message.
        let replying = self.pane(at).replying;
        let editing = self.pane(at).editing;
        if let Some(target) = editing.or(replying) {
            let what = if editing.is_some() {
                "Rewriting"
            } else {
                "Replying to"
            };
            let said = state
                .lines
                .iter()
                .find(|l| l.seq == target)
                .map(|l| sigil_ui::message::preview(&l.text, 48))
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.colored_label(theme.accent, format!("{what}: {said}"));
                if ui.button("Cancel").clicked() {
                    let pane = self.pane(at);
                    pane.replying = None;
                    if pane.editing.take().is_some() {
                        // An abandoned rewrite must not leave the old text in
                        // the box, where the next Return would post it again
                        // as a new message.
                        pane.composing.clear();
                    }
                }
            });
        }

        ui.horizontal(|ui| {
            // Room for both controls, measured rather than guessed: the field
            // took `available - one button` while two sat beside it, and Send
            // ran off the edge of the window.
            let controls = (tokens::BUTTON_MD + ui.spacing().item_spacing.x) * 2.0;
            let width = (ui.available_width() - controls).max(80.0);
            let field = sigil_ui::field(
                ui,
                &mut self.panes.entry(at.clone()).or_default().composing,
                "write a message, or / for a command",
                width,
            );
            // Typing is published from the fact that the text changed, not from
            // the field having focus: a box somebody is sitting in front of and
            // not writing in is not typing, and saying otherwise is a claim
            // about them that they did not make.
            if field.changed() {
                let writing = !self.pane(at).composing.is_empty();
                if self.pane(at).announced_typing != writing {
                    self.pane(at).announced_typing = writing;
                    self.send_as(Some(at), Cmd::Typing(writing));
                }
            }
            // Attach sits before Send, which is where every messenger puts
            // it: the last control on the row is the one that commits.
            if sigil_ui::icon_button(ui, sigil_ui::Icon::Attach)
                .on_hover_text("Send a file. It is sealed before it leaves this machine.")
                .clicked()
                && let Some(path) = rfd::FileDialog::new().pick_file()
            {
                self.send_as(Some(at), Cmd::SendFile(path));
            }
            let send = sigil_ui::icon_button(ui, sigil_ui::Icon::Send)
                .on_hover_text(if editing.is_some() {
                    "Save the rewrite"
                } else {
                    "Send"
                })
                .clicked();
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (entered || send) && !self.pane(at).composing.trim().is_empty() {
                // Taken, not cleared: if the send fails the text has to come
                // back, and the session is what knows whether it did.
                let text = std::mem::take(&mut self.pane(at).composing);
                let pane = self.pane(at);
                let (editing, replying) = (pane.editing.take(), pane.replying.take());
                pane.announced_typing = false;
                let cmd = match (editing, replying) {
                    (Some(target), _) => Cmd::Edit { target, text },
                    (None, Some(target)) => Cmd::Reply { target, text },
                    (None, None) => Cmd::Send(text),
                };
                self.send_as(Some(at), cmd);
                self.send_as(Some(at), Cmd::Typing(false));
                field.request_focus();
            }
        });
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

        ui.horizontal(|ui| {
            if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                ctx.navigator.back();
            }
            ui.heading("Public channels");
        });
        ui.colored_label(
            theme.text_secondary,
            "Anybody may join these, and nothing said in one is encrypted — everyone \
             who may join would hold any key it used.",
        );
        ui.add_space(tokens::SPACING_SM);

        ui.horizontal(|ui| {
            let pane = self.panes.entry(at.clone()).or_default();
            let field = sigil_ui::field(
                ui,
                &mut pane.query,
                "name a channel, or leave empty for everything",
                320.0,
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if entered || ui.button("Search").clicked() {
                let query = self.pane(at).query.clone();
                self.send_as(Some(at), Cmd::Find(query));
            }
        });
        ui.add_space(tokens::SPACING_SM);

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
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(&found.name).strong());
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
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if already {
                                ui.colored_label(theme.text_muted, "joined");
                            } else if ui.button("Join").clicked() {
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

        ui.horizontal(|ui| {
            if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                ctx.navigator.back();
            }
            ui.heading("Members");
        });

        if state.i_am_admin {
            ui.add_space(tokens::SPACING_SM);
            ui.horizontal(|ui| {
                ui.label("Invite");
                sigil_ui::field(
                    ui,
                    &mut self.panes.entry(at.clone()).or_default().inviting,
                    "paste their key, or type name@domain",
                    280.0,
                );
                if ui.button("Add").clicked() {
                    let typed = self.pane(at).inviting.trim().to_string();
                    match typed.parse::<PubKey>() {
                        Ok(who) => {
                            self.pane(at).inviting.clear();
                            self.send_as(Some(at), Cmd::Invite(who));
                        }
                        Err(e) => {
                            self.pane(at).add_trouble = Some(format!("that is not a key: {e}"))
                        }
                    }
                }
            });
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
                        sigil_ui::identicon(ui, &key, tokens::AVATAR_SM);
                        ui.add_space(tokens::SPACING_SM);
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(person.label(&member.account));
                                if member.admin {
                                    // The exchange attests this one, so it may
                                    // be drawn as a role. A SIP-21 title may
                                    // not, which is why it is not here.
                                    ui.colored_label(theme.accent, "admin");
                                }
                                if member.account == me {
                                    ui.colored_label(theme.text_muted, "you");
                                }
                            });
                            // In full. This is the only thing that identifies
                            // them; everything above it is a claim.
                            ui.add(
                                egui::Label::new(egui::RichText::new(&key).monospace().small())
                                    .selectable(true),
                            );
                        });
                        if state.i_am_admin && member.account != me {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button("Remove")
                                        .on_hover_text(
                                            "Removes them and mints a new key, so what \
                                             follows is not theirs. What they already \
                                             hold, they keep.",
                                        )
                                        .clicked()
                                    {
                                        self.send_as(Some(at), Cmd::Kick(member.account));
                                    }
                                    let (label, admin) = if member.admin {
                                        ("Demote", false)
                                    } else {
                                        ("Make admin", true)
                                    };
                                    if ui.button(label).clicked() {
                                        self.send_as(
                                            Some(at),
                                            Cmd::Grant {
                                                who: member.account,
                                                admin,
                                            },
                                        );
                                    }
                                    let blocked = state.blocked.contains(&member.account);
                                    if ui
                                        .button(if blocked { "Unblock" } else { "Block" })
                                        .on_hover_text(if blocked {
                                            "They can reach you again."
                                        } else {
                                            // Never over-claimed: the exchange
                                            // answers on your behalf and tells
                                            // them nothing, but a delivery
                                            // mark that stops moving is a
                                            // thing somebody can notice.
                                            "You stop hearing from them. They are told nothing, \
                                             though it can be worked out."
                                        })
                                        .clicked()
                                    {
                                        self.send_as(
                                            Some(at),
                                            Cmd::SetBlocked {
                                                who: member.account,
                                                blocked: !blocked,
                                            },
                                        );
                                    }
                                },
                            );
                        }
                    });
                    ui.separator();
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

        ui.horizontal(|ui| {
            if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                ctx.navigator.back();
            }
            ui.heading("Channel settings");
        });

        if !state.i_am_admin {
            ui.colored_label(
                theme.text_secondary,
                "Only an admin can change these. You can still leave.",
            );
        }

        ui.add_space(tokens::SPACING_SM);
        ui.add_enabled_ui(state.i_am_admin, |ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                sigil_ui::field(
                    ui,
                    &mut self.panes.entry(at.clone()).or_default().channel_name,
                    "what this channel is called",
                    280.0,
                );
                if ui.button("Set").clicked() {
                    let name = self.pane(at).channel_name.clone();
                    self.send_as(Some(at), Cmd::SetName(name));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Topic");
                sigil_ui::field(
                    ui,
                    &mut self.panes.entry(at.clone()).or_default().channel_topic,
                    "a line about what it is for",
                    280.0,
                );
                if ui.button("Set").clicked() {
                    let topic = self.pane(at).channel_topic.clone();
                    self.send_as(Some(at), Cmd::SetTopic(topic));
                }
            });

            ui.add_space(tokens::SPACING_SM);
            ui.horizontal(|ui| {
                ui.label("Keep messages for");
                ui.add(
                    egui::DragValue::new(
                        &mut self.panes.entry(at.clone()).or_default().retention_days,
                    )
                    .range(1..=365)
                    .suffix(" days"),
                );
                if ui.button("Set").clicked() {
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
            if ui
                .button("Mint a new key")
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
        if ui
            .button("Leave")
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
        let layers = discovery::layers(discovery::nothing_explicit(), &self.config, Some(&path));
        if !discovery::any_configured(&layers) {
            return;
        }
        let wake = egui_ctx.clone();
        let handle = sigil_net::spawn_room(
            layers,
            signer,
            sigil_net::RoomId::new(ring.secret),
            Default::default(),
            move || wake.request_repaint(),
        );
        self.calls.insert(
            me,
            Live {
                channel: ring.channel,
                seq: ring.seq,
                handle,
                since: std::time::Instant::now(),
            },
        );
    }

    /// Stop carrying audio, and say how long it lasted.
    fn leave_call(&mut self, me: PubKey) -> Option<([u8; 32], u64, u32)> {
        let live = self.calls.remove(&me)?;
        let seconds = live.since.elapsed().as_secs().min(u32::MAX as u64) as u32;
        live.handle.hang_up();
        Some((live.channel, live.seq, seconds))
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
    fn announce_rings(&mut self, ctx: &mut AppContext<'_>) {
        // **Which identity is being called.** Every session is walked, so a
        // call arriving at one identity reaches somebody looking at another —
        // and the notification then has to say which, or it names a caller,
        // a conversation, and no way to tell where either of them is.
        let held = self.sessions.len();
        let mut fresh: Vec<String> = Vec::new();
        for (at, session) in &self.sessions {
            let state = session.state();
            for ring in &state.ringing {
                if ring.mine || self.announced.contains(&(ring.channel, ring.seq)) {
                    continue;
                }
                self.announced.insert((ring.channel, ring.seq));
                fresh.push(ring_said(
                    &ring.from,
                    &ring.label,
                    &state.mine.label(&at.0),
                    held,
                ));
            }
        }
        for said in fresh {
            ctx.notify.post("Incoming call", &said);
        }
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
        // Ours is not a ring, it is a call being placed. Drawn differently,
        // because "answer" on a call you are making is nonsense.
        let Some(ring) = state.ringing.iter().find(|r| !r.mine && !r.answered) else {
            return false;
        };
        let key = ring.from.to_string();
        egui::Frame::NONE
            .fill(theme.surface_elevated)
            .corner_radius(tokens::RADIUS_LG)
            .inner_margin(egui::Margin::same(tokens::SPACING_MD as i8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    sigil_ui::identicon(ui, &key, tokens::AVATAR_MD);
                    ui.add_space(tokens::SPACING_SM);
                    ui.vertical(|ui| {
                        let named = state
                            .people
                            .get(&ring.from)
                            .map(|p| p.label(&ring.from))
                            .unwrap_or_else(|| key.clone());
                        ui.label(egui::RichText::new(format!("{named} is calling")).strong());
                        ui.colored_label(
                            theme.text_secondary,
                            egui::RichText::new(&ring.label).small(),
                        );
                        // The key in full, on the ring, always. A name is an
                        // assertion and this is the one screen where acting on
                        // the wrong one puts somebody in a call with a stranger
                        // who chose a confusable name.
                        ui.add(
                            egui::Label::new(egui::RichText::new(&key).monospace().small())
                                .selectable(true),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Decline").color(theme.destructive),
                            ))
                            .clicked()
                        {
                            self.send_as(
                                Some(at),
                                Cmd::Decline {
                                    channel: ring.channel,
                                    seq: ring.seq,
                                },
                            );
                        }
                        if ui.button("Answer").clicked() {
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
                    });
                });
            });
        true
    }

    /// The bar shown while audio is actually flowing.
    fn in_call_ui(&mut self, at: &At, ui: &mut egui::Ui, theme: &ColorTheme) {
        let me = at.0;
        let Some(live) = self.calls.get(&me) else {
            return;
        };
        let call = live.handle.state();
        let seconds = live.since.elapsed().as_secs();
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
                        if up { "In a call" } else { "Connecting…" },
                    );
                    ui.colored_label(
                        theme.text_muted,
                        format!("{:02}:{:02}", seconds / 60, seconds % 60),
                    );
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
    fn devices_view(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        let Some(at) = self.showing_at(ctx) else {
            return AppResponse::default();
        };
        let at = &at;
        let state = self.state_of(Some(at));

        ui.horizontal(|ui| {
            if sigil_ui::icon_button(ui, sigil_ui::Icon::Back).clicked() {
                ctx.navigator.back();
            }
            ui.heading("Devices");
            if sigil_ui::icon_button(ui, sigil_ui::Icon::Refresh).clicked() {
                self.send_as(Some(at), Cmd::Devices);
            }
        });

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
        if state.devices.len() <= 1 {
            ui.colored_label(
                theme.warning,
                "Nothing else is linked. The conversations on this machine cannot be \
                 recovered from the exchange — opening a key spends it, so what is here \
                 is the only copy. Link a second device and it becomes the backup.",
            );
            ui.add_space(tokens::SPACING_SM);
        }

        for device in &state.devices {
            let key = device.device.to_string();
            ui.horizontal(|ui| {
                sigil_ui::identicon(ui, &key, tokens::AVATAR_SM);
                ui.add_space(tokens::SPACING_SM);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(sigil_ui::message::short(&key));
                        if device.is_this_one {
                            ui.colored_label(theme.accent, "this device");
                        }
                    });
                    ui.add(
                        egui::Label::new(egui::RichText::new(&key).monospace().small())
                            .selectable(true),
                    );
                    ui.colored_label(
                        theme.text_muted,
                        egui::RichText::new(format!(
                            "linked {} · credential expires {}",
                            sigil_ui::brief(device.added, self.now()),
                            sigil_ui::brief(device.not_after, self.now())
                        ))
                        .small(),
                    );
                });
                if !device.is_this_one {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Revoke").color(theme.destructive),
                            ))
                            .on_hover_text(
                                "It stops acting for you. It keeps every key it was already \
                                 given, so rotate anything it could read.",
                            )
                            .clicked()
                        {
                            self.send_as(Some(at), Cmd::RevokeDevice(device.device));
                        }
                    });
                }
            });
            ui.separator();
        }

        ui.add_space(tokens::SPACING_MD);
        ui.heading("Link another device");
        ui.colored_label(
            theme.text_secondary,
            "Write a credential here, then give it to the other device. It names both \
             keys in the clear, so hand it over the way you would hand over a key.",
        );
        ui.horizontal(|ui| {
            ui.label("Its key");
            sigil_ui::field(
                ui,
                &mut self.panes.entry(at.clone()).or_default().linking,
                "the new device's key, in base58",
                300.0,
            );
            if ui.button("Write credential").clicked() {
                let typed = self.pane(at).linking.trim().to_string();
                match typed.parse::<PubKey>() {
                    Ok(device) => {
                        self.pane(at).linking.clear();
                        self.send_as(Some(at), Cmd::LinkDevice { device, days: 90 });
                    }
                    Err(e) => self.pane(at).add_trouble = Some(format!("that is not a key: {e}")),
                }
            }
        });
        if let Some(credential) = &state.credential {
            ui.add_space(tokens::SPACING_SM);
            ui.add(
                egui::TextEdit::multiline(&mut credential.clone())
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(credential.clone());
            }
        }

        // **The other half.** The screen could write a credential and had
        // nowhere to present one, so the second device of an account could be
        // named and never enrolled — and a linked device is the only backup an
        // epoch key can have.
        ui.add_space(tokens::SPACING_XL);
        ui.heading("Use a credential");
        ui.colored_label(
            theme.text_secondary,
            "If another of your devices wrote one for this one, paste it here. The \
             exchange checks it names *this* device, so one somebody found is one they \
             cannot use.",
        );
        ui.add_space(tokens::SPACING_SM);
        let width = ui.available_width();
        sigil_ui::field(
            ui,
            &mut self.panes.entry(at.clone()).or_default().presenting,
            "the credential your other device wrote, in base58",
            width,
        );
        ui.add_space(tokens::SPACING_SM);
        if ui.button("Register this device").clicked() {
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
        AppResponse::default()
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
        assert!(
            said.starts_with(&PubKey::new([3u8; 32]).to_string()[..8]),
            "{said}"
        );
    }

    /// Nothing known yet, and nothing invented.
    #[test]
    fn with_no_session_at_all_it_is_just_the_default() {
        assert_eq!(default_label(None), "default");
        assert_eq!(default_label(Some(at(None, None))), "default");
    }
}
