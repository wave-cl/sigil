//! Messaging, as a sigil app.

pub mod session;

pub use session::{
    Attached, ChatHandle, ChatState, Closing, Cmd, Found, Line, LinkState, Member, Person, Receipt,
    Ring, Summary, Trouble,
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
}

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
    /// The profile editor is open.
    editing_profile: bool,
    /// The message being replied to, if any.
    replying: Option<u64>,
    /// Whether we have told the channel we are typing, so the signal is sent
    /// on the edges rather than on every keystroke.
    announced_typing: bool,
    /// The message being rewritten, if any. What Enter does depends on it.
    editing: Option<u64>,
    /// The directory search box.
    query: String,
    /// The key being invited to the open channel.
    inviting: String,
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
            editing_profile: false,
            replying: None,
            announced_typing: false,
            editing: None,
            query: String::new(),
            inviting: String::new(),
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
    /// One live session per unlocked identity — not one for the identity being
    /// looked at. A message arriving for an account you are not currently
    /// showing is still a message you want to be told about.
    sessions: HashMap<PubKey, ChatHandle>,
    /// Sessions told to stop that still hold their store lock. An account here
    /// must not be reopened yet; see [`Closing`].
    closing: Vec<(PubKey, Closing)>,
    panes: HashMap<PubKey, Pane>,
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
        let mut keys: Vec<PubKey> = self.sessions.keys().copied().collect();
        keys.sort_by_key(|k| k.to_string());
        keys
    }

    /// The account being shown, if it is open.
    fn showing(ctx: &AppContext<'_>) -> Option<PubKey> {
        ctx.account().unlocked().map(|u| u.me())
    }

    fn state_of(&self, me: Option<PubKey>) -> ChatState {
        if let Some(fixed) = &self.fixed {
            return fixed.clone();
        }
        me.and_then(|me| self.sessions.get(&me))
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

    fn pane(&mut self, me: PubKey) -> &mut Pane {
        self.panes.entry(me).or_default()
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

        let held: Vec<(PubKey, std::path::PathBuf)> = ctx
            .accounts
            .unlocked()
            .map(|(me, u)| (me, u.path().to_path_buf()))
            .collect();

        // Stop anything no longer held. This is the half that matters: a
        // session left running for a discarded identity keeps connecting,
        // keeps succeeding, and is the wrong person.
        let live: Vec<PubKey> = self.sessions.keys().copied().collect();
        for me in live {
            if !held.iter().any(|(k, _)| *k == me) {
                if let Some(session) = self.sessions.remove(&me) {
                    self.closing.push((me, session.close()));
                }
                self.panes.remove(&me);
            }
        }

        for (me, path) in held {
            if self.sessions.contains_key(&me) {
                continue;
            }
            // Its predecessor has not let go of the store yet.
            if self.closing.iter().any(|(k, _)| *k == me) {
                continue;
            }
            let Some(unlocked) = ctx
                .accounts
                .unlocked()
                .find(|(k, _)| *k == me)
                .map(|(_, u)| u)
            else {
                continue;
            };
            let layers =
                discovery::layers(discovery::nothing_explicit(), &self.config, Some(&path));
            if !discovery::any_configured(&layers) {
                continue;
            }
            // One store per account, which is what makes several identities
            // safe to hold at once: different keys, different files, different
            // locks. The same identity twice would be refused its lock, and
            // rightly.
            let store_at = self
                .store_root
                .as_ref()
                .map(|root| root.join(format!("{me}.db")));
            let wake = egui_ctx.clone();
            self.sessions.insert(
                me,
                session::start(layers, unlocked.signer(), store_at, move || {
                    wake.request_repaint()
                }),
            );
        }
    }

    fn send_as(&mut self, me: Option<PubKey>, cmd: Cmd) {
        if let Some(s) = me.and_then(|me| self.sessions.get(&me)) {
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
        }
    }

    fn nav_title(&self, token: &std::rc::Rc<dyn std::any::Any>) -> Option<String> {
        Some(
            match Self::route(token) {
                Route::Conversations => return None,
                Route::Directory => "Public channels",
                Route::Members => "Members",
                Route::Settings => "Channel settings",
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
        let Some(me) = Self::showing(ctx) else {
            return AppResponse::default();
        };
        let state = self.state_of(Some(me));

        // The connection light says the *word* as well as the colour. A red dot
        // on its own is not a message, and this one matters more than usual:
        // while the link is down, messages do not arrive.
        ui.horizontal(|ui| {
            let colour = match state.link {
                LinkState::Up => theme.link_up,
                LinkState::Retrying => theme.link_retrying,
                LinkState::Gone => theme.link_gone,
            };
            // Painted, and it says the word. While the link is down messages
            // do not arrive, and nothing happening looks exactly like nobody
            // writing -- so this is the one indicator that must not be a bare
            // colour.
            sigil_ui::dot(
                ui,
                state.link == LinkState::Up,
                colour,
                colour,
                state.link.word(),
            );
            ui.colored_label(colour, state.link.word());
            if state.link != LinkState::Up && ui.button("Reconnect").clicked() {
                self.send_as(Some(me), Cmd::Reconnect);
            }
        });
        if let Some(trouble) = &state.trouble {
            ui.colored_label(theme.destructive, trouble);
        }
        ui.separator();

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
                    None => self.list_ui(ctx, me, &state, ui, &theme),
                    Some(_) => {
                        if ui.button("← Conversations").clicked() {
                            self.send_as(Some(me), Cmd::Close);
                        }
                        self.transcript_ui(ctx, me, &state, ui, &theme);
                    }
                }
            }
            sigil::Layout::Shared { column_width } | sigil::Layout::Scrolling { column_width } => {
                egui::Panel::left("chat_list")
                    .resizable(false)
                    .exact_size(column_width.min(360.0))
                    .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                        right: tokens::SPACING_MD as i8,
                        ..Default::default()
                    }))
                    .show(ui, |ui| self.list_ui(ctx, me, &state, ui, &theme));
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| self.transcript_ui(ctx, me, &state, ui, &theme));
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
}

impl ChatApp {
    /// Who you are here, and what everybody else sees.
    ///
    /// Your own key in full, your SIP-38 handle if you have one, and the
    /// SIP-21 profile you publish. The profile is **self-declared and attested
    /// by nobody**, which the pane says rather than leaving somebody to infer
    /// it from a field that looks like an account setting.
    fn me_ui(&mut self, me: PubKey, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
        let key = me.to_string();
        ui.horizontal(|ui| {
            sigil_ui::identicon(ui, &key, tokens::AVATAR_MD);
            ui.add_space(tokens::SPACING_SM);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(state.mine.label(&me)).strong());
                match &state.mine.handle {
                    Some(handle) => {
                        ui.colored_label(theme.text_secondary, egui::RichText::new(handle).small());
                    }
                    None => {
                        ui.colored_label(
                            theme.text_muted,
                            egui::RichText::new("no name at this exchange").small(),
                        );
                    }
                }
            });
        });
        // In full, selectable, and not behind anything. A name is an assertion
        // and this is not (SIP-21) -- it is the only thing that identifies you
        // to somebody who wants to write to you.
        ui.add(egui::Label::new(egui::RichText::new(&key).monospace().small()).selectable(true));

        let pane = self.panes.entry(me).or_default();
        if !pane.editing_profile {
            if ui.button("Edit profile").clicked() {
                pane.editing_profile = true;
                pane.name = state.mine.name.clone().unwrap_or_default();
                pane.title = state.mine.title.clone().unwrap_or_default();
            }
        } else {
            ui.add(
                egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().name)
                    .hint_text("display name"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().title)
                    .hint_text("title"),
            );
            // Said next to the field rather than in a help page. A title
            // asserts standing, and somebody typing one should know that
            // nothing behind it is checked.
            ui.colored_label(
                theme.text_muted,
                egui::RichText::new(
                    "Both are what you say about yourself. Nobody verifies either.",
                )
                .small(),
            );
            ui.horizontal(|ui| {
                if ui.button("Publish").clicked() {
                    let pane = self.panes.entry(me).or_default();
                    let (name, title) = (pane.name.clone(), pane.title.clone());
                    pane.editing_profile = false;
                    self.send_as(Some(me), Cmd::SetProfile { name, title });
                }
                if ui.button("Cancel").clicked() {
                    self.panes.entry(me).or_default().editing_profile = false;
                }
            });
        }
        ui.add_space(tokens::SPACING_SM);
        ui.separator();
    }

    /// The conversation list.
    fn list_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        me: PubKey,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let now = self.now();
        self.me_ui(me, state, ui, theme);
        ui.horizontal(|ui| {
            ui.heading("Conversations");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("New", |ui| {
                    if ui.button("Group").clicked() {
                        // A group's name is a sealed entry, so it is named
                        // after it exists rather than before.
                        self.send_as(Some(me), Cmd::NewGroup("New group".into()));
                        ui.close();
                    }
                    if ui
                        .button("Public channel")
                        .on_hover_text(
                            "Anybody may find and join it, and nothing said in it is \
                             encrypted.",
                        )
                        .clicked()
                    {
                        self.send_as(
                            Some(me),
                            Cmd::NewPublic {
                                name: "New channel".into(),
                                topic: String::new(),
                            },
                        );
                        ui.close();
                    }
                });
            });
        });
        ui.add_space(tokens::SPACING_XS);

        ui.horizontal(|ui| {
            // A visible label, not only a placeholder: a hint disappears the
            // moment somebody types, and it never reaches the accessibility
            // tree at all.
            ui.label("Write to");
            ui.add(
                egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().adding)
                    .hint_text("their key, base58")
                    .desired_width(ui.available_width() - 50.0),
            );
        });
        if ui.button("Add").clicked() {
            let typed = self.pane(me).adding.trim().to_string();
            match typed.parse::<PubKey>() {
                Ok(who) => {
                    self.pane(me).add_trouble = None;
                    self.send_as(Some(me), Cmd::AddContact(who, String::new()));
                    self.send_as(Some(me), Cmd::OpenDm(who));
                    self.pane(me).adding.clear();
                }
                Err(e) => self.pane(me).add_trouble = Some(format!("that is not a key: {e}")),
            }
        }
        if let Some(t) = self.panes.get(&me).and_then(|p| p.add_trouble.as_ref()) {
            ui.colored_label(theme.destructive, t);
        }
        if ui.button("Find a public channel").clicked() {
            ctx.navigator.push_here(Route::Directory);
        }

        ui.add_space(tokens::SPACING_SM);

        if state.conversations.is_empty() {
            ui.colored_label(
                theme.text_secondary,
                "No conversations yet. Write to somebody by their key.",
            );
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
                        self.send_as(Some(me), Cmd::Show(convo.channel));
                    }
                }
            });
    }

    /// The messages, and the box to write one in.
    fn transcript_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        me: PubKey,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let now = self.now();

        if state.open.is_none() {
            ui.centered_and_justified(|ui| {
                ui.colored_label(theme.text_secondary, "Pick a conversation.");
            });
            return;
        }

        // The header: what this conversation is, and the way into everything
        // that can be done about it.
        ui.horizontal(|ui| {
            let label = state
                .conversations
                .iter()
                .find(|c| Some(c.channel) == state.open)
                .map(|c| c.label.clone())
                .unwrap_or_default();
            ui.heading(label);
            if !state.topic.is_empty() {
                ui.colored_label(theme.text_secondary, &state.topic);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Settings").clicked() {
                    ctx.navigator.push_here(Route::Settings);
                }
                // Calling from inside the conversation, with audio. The
                // terminal client does the whole SIP-36 exchange and then
                // prints a room secret for somebody to paste into another
                // program, because it has nothing to play sound on.
                if !self.calls.contains_key(&me)
                    && !state.ringing.iter().any(|r| r.mine)
                    && ui.button("Call").clicked()
                {
                    self.send_as(Some(me), Cmd::Call);
                }
            });
        });
        if self.ringing_ui(ctx, me, state, ui, theme) {
            ui.add_space(tokens::SPACING_SM);
        }
        self.in_call_ui(me, ui, theme);
        // A call we placed that nobody has taken yet.
        if let Some(ring) = state.ringing.iter().find(|r| r.mine) {
            let (channel, seq) = (ring.channel, ring.seq);
            ui.horizontal(|ui| {
                ui.colored_label(theme.text_secondary, "Ringing…");
                if ui.button("Cancel").clicked() {
                    let seconds = self.leave_call(me).map(|(_, _, s)| s).unwrap_or(0);
                    self.send_as(
                        Some(me),
                        Cmd::Hangup {
                            channel,
                            seq,
                            seconds,
                        },
                    );
                }
            });
        }
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let members = state.members.len();
                let label = match members {
                    0 => "Members".to_string(),
                    n => format!("Members ({n})"),
                };
                if ui.button(label).clicked() {
                    ctx.navigator.push_here(Route::Members);
                }
            });
        });
        // A note is about something just done and a trouble is about a state.
        // Kept apart because the state is rebuilt every refresh, and merged
        // they would put every confirmation on screen for less than a tick.
        if let Some(note) = &state.note {
            ui.colored_label(theme.success, note);
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
            .show(ui, |ui| self.composer_ui(me, state, ui, theme));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        self.messages_ui(me, state, ui, theme, now);
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
    }

    /// The messages themselves, with day separators, grouping and the divider.
    fn messages_ui(
        &mut self,
        me: PubKey,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
        now: u64,
    ) {
        if state.lines.is_empty() {
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
        let mut acted: Option<(u64, String, PubKey, sigil_ui::BubbleAction)> = None;
        let mut previous_day: Option<String> = None;
        let mut previous_author: Option<PubKey> = None;
        let mut previous_at: u64 = 0;

        for line in &state.lines {
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
            };
            let did = sigil_ui::bubble(ui, &bubble);
            if !did.is_none() {
                acted = Some((line.seq, line.text.clone(), line.who, did));
            }

            previous_author = Some(line.who);
            previous_at = line.at;
        }

        if state.typing {
            ui.add_space(tokens::SPACING_SM);
            ui.colored_label(theme.text_muted, "typing…");
        }

        if let Some((seq, text, who, did)) = acted {
            if let Some(emoji) = did.react {
                self.send_as(Some(me), Cmd::React { target: seq, emoji });
            }
            if did.reply {
                self.pane(me).replying = Some(seq);
            }
            if did.edit {
                // The text is loaded into the composer so an edit is a
                // correction of what is there rather than a retyping of it.
                self.pane(me).editing = Some(seq);
                self.pane(me).composing = text;
            }
            if did.redact {
                self.send_as(Some(me), Cmd::Redact(seq));
            }
            if did.copy_key {
                ui.ctx().copy_text(who.to_string());
            }
            if let Some(index) = did.save {
                // The dialog is native and blocking, which is fine here: it is
                // a direct answer to a click, and the session goes on running
                // on its own task regardless.
                if let Some(to) = rfd::FileDialog::new().save_file() {
                    self.send_as(Some(me), Cmd::SaveFile { seq, index, to });
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
    fn composer_ui(
        &mut self,
        me: PubKey,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        // What Enter will do, said above the box. A composer that silently
        // means three different things depending on invisible state is one
        // that will eventually send an edit as a new message.
        let replying = self.pane(me).replying;
        let editing = self.pane(me).editing;
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
                .map(|l| sigil_ui::message::short(&l.text))
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.colored_label(theme.accent, format!("{what}: {said}"));
                if ui.button("Cancel").clicked() {
                    let pane = self.pane(me);
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
            let button = tokens::BUTTON_LG + tokens::SPACING_MD;
            let width = (ui.available_width() - button).max(80.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().composing)
                    .hint_text("Write a message")
                    .desired_width(width),
            );
            // Typing is published from the fact that the text changed, not from
            // the field having focus: a box somebody is sitting in front of and
            // not writing in is not typing, and saying otherwise is a claim
            // about them that they did not make.
            if field.changed() {
                let writing = !self.pane(me).composing.is_empty();
                if self.pane(me).announced_typing != writing {
                    self.pane(me).announced_typing = writing;
                    self.send_as(Some(me), Cmd::Typing(writing));
                }
            }
            let send = ui
                .button(if editing.is_some() { "Save" } else { "Send" })
                .clicked();
            if ui
                .button("Attach")
                .on_hover_text("Send a file. It is sealed before it leaves this machine.")
                .clicked()
                && let Some(path) = rfd::FileDialog::new().pick_file()
            {
                self.send_as(Some(me), Cmd::SendFile(path));
            }
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (entered || send) && !self.pane(me).composing.trim().is_empty() {
                // Taken, not cleared: if the send fails the text has to come
                // back, and the session is what knows whether it did.
                let text = std::mem::take(&mut self.pane(me).composing);
                let pane = self.pane(me);
                let (editing, replying) = (pane.editing.take(), pane.replying.take());
                pane.announced_typing = false;
                let cmd = match (editing, replying) {
                    (Some(target), _) => Cmd::Edit { target, text },
                    (None, Some(target)) => Cmd::Reply { target, text },
                    (None, None) => Cmd::Send(text),
                };
                self.send_as(Some(me), cmd);
                self.send_as(Some(me), Cmd::Typing(false));
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
        let Some(me) = Self::showing(ctx) else {
            return AppResponse::default();
        };
        let state = self.state_of(Some(me));

        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
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
            let pane = self.panes.entry(me).or_default();
            let field = ui.add(
                egui::TextEdit::singleline(&mut pane.query)
                    .hint_text("search, or leave empty for everything"),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if entered || ui.button("Search").clicked() {
                let query = self.pane(me).query.clone();
                self.send_as(Some(me), Cmd::Find(query));
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
                                    Some(me),
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
        let Some(me) = Self::showing(ctx) else {
            return AppResponse::default();
        };
        let state = self.state_of(Some(me));

        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
                ctx.navigator.back();
            }
            ui.heading("Members");
        });

        if state.i_am_admin {
            ui.add_space(tokens::SPACING_SM);
            ui.horizontal(|ui| {
                ui.label("Invite");
                ui.add(
                    egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().inviting)
                        .hint_text("their key or name@domain")
                        .desired_width(240.0),
                );
                if ui.button("Add").clicked() {
                    let typed = self.pane(me).inviting.trim().to_string();
                    match typed.parse::<PubKey>() {
                        Ok(who) => {
                            self.pane(me).inviting.clear();
                            self.send_as(Some(me), Cmd::Invite(who));
                        }
                        Err(e) => {
                            self.pane(me).add_trouble = Some(format!("that is not a key: {e}"))
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
                                        self.send_as(Some(me), Cmd::Kick(member.account));
                                    }
                                    let (label, admin) = if member.admin {
                                        ("Demote", false)
                                    } else {
                                        ("Make admin", true)
                                    };
                                    if ui.button(label).clicked() {
                                        self.send_as(
                                            Some(me),
                                            Cmd::Grant {
                                                who: member.account,
                                                admin,
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
        let Some(me) = Self::showing(ctx) else {
            return AppResponse::default();
        };
        let state = self.state_of(Some(me));

        ui.horizontal(|ui| {
            if ui.button("← Back").clicked() {
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
                ui.add(
                    egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().channel_name)
                        .desired_width(240.0),
                );
                if ui.button("Set").clicked() {
                    let name = self.pane(me).channel_name.clone();
                    self.send_as(Some(me), Cmd::SetName(name));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Topic");
                ui.add(
                    egui::TextEdit::singleline(
                        &mut self.panes.entry(me).or_default().channel_topic,
                    )
                    .desired_width(240.0),
                );
                if ui.button("Set").clicked() {
                    let topic = self.pane(me).channel_topic.clone();
                    self.send_as(Some(me), Cmd::SetTopic(topic));
                }
            });

            ui.add_space(tokens::SPACING_SM);
            ui.horizontal(|ui| {
                ui.label("Keep messages for");
                ui.add(
                    egui::DragValue::new(&mut self.panes.entry(me).or_default().retention_days)
                        .range(1..=365)
                        .suffix(" days"),
                );
                if ui.button("Set").clicked() {
                    let days = self.pane(me).retention_days;
                    self.send_as(
                        Some(me),
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
                self.send_as(Some(me), Cmd::Rotate);
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
            self.send_as(Some(me), Cmd::Leave);
            ctx.navigator.back();
        }

        ui.add_space(tokens::SPACING_SM);
        let pane = self.panes.entry(me).or_default();
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
                    self.panes.entry(me).or_default().confirming_destroy = false;
                    self.send_as(Some(me), Cmd::Destroy);
                    ctx.navigator.back();
                }
                if ui.button("Cancel").clicked() {
                    self.panes.entry(me).or_default().confirming_destroy = false;
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
        me: PubKey,
        ring: &Ring,
        egui_ctx: &egui::Context,
    ) {
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
        let mut fresh: Vec<(PubKey, u64, String)> = Vec::new();
        for (me, session) in &self.sessions {
            for ring in session.state().ringing {
                if ring.mine || self.announced.contains(&(ring.channel, ring.seq)) {
                    continue;
                }
                fresh.push((ring.from, ring.seq, ring.label.clone()));
                self.announced.insert((ring.channel, ring.seq));
                let _ = me;
            }
        }
        for (from, _, label) in fresh {
            ctx.notify.post(
                "Incoming call",
                &format!(
                    "{label} — from {}",
                    sigil_ui::message::short(&from.to_string())
                ),
            );
        }
    }

    /// A call ringing, and the two things to do about it.
    fn ringing_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        me: PubKey,
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
                                Some(me),
                                Cmd::Decline {
                                    channel: ring.channel,
                                    seq: ring.seq,
                                },
                            );
                        }
                        if ui.button("Answer").clicked() {
                            let ring = ring.clone();
                            self.send_as(
                                Some(me),
                                Cmd::Answer {
                                    channel: ring.channel,
                                    seq: ring.seq,
                                },
                            );
                            self.join_call(ctx, me, &ring, ui.ctx());
                        }
                    });
                });
            });
        true
    }

    /// The bar shown while audio is actually flowing.
    fn in_call_ui(&mut self, me: PubKey, ui: &mut egui::Ui, theme: &ColorTheme) {
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
                        if ui
                            .add(egui::Button::new(
                                egui::RichText::new("Hang up").color(theme.destructive),
                            ))
                            .clicked()
                            && let Some((channel, seq, seconds)) = self.leave_call(me)
                        {
                            self.send_as(
                                Some(me),
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
