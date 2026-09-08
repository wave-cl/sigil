//! Messaging, as a sigil app.

pub mod session;

pub use session::{ChatHandle, ChatState, Closing, Cmd, Line, LinkState, Summary, Trouble};

use std::collections::HashMap;

use sigil::app::{App, AppContext, AppResponse, TabNotifications};
use sigil::{ColorTheme, tokens};
use sigil_net::discovery;
use sqnr::config::Config;
use sqnr_core::PubKey;

/// Where you are inside the chat app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Conversations,
}

/// What is being typed, per identity.
///
/// **Keyed by account, not shared.** A draft typed as one identity must not
/// still be in the box after switching to another: the next Return would send
/// it as somebody else, which is a mistake the interface would have made on
/// your behalf and not mentioned.
#[derive(Default)]
struct Pane {
    /// Kept out of the session so that a failed send leaves it on screen:
    /// retyping a message the program lost is the worst thing a chat client can
    /// do to somebody.
    composing: String,
    /// The key being added as a contact.
    adding: String,
    add_trouble: Option<String>,
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

impl App for ChatApp {
    fn update(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        self.reconcile(ctx, egui_ctx);
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
                    None => self.list_ui(me, &state, ui, &theme),
                    Some(_) => {
                        if ui.button("← Conversations").clicked() {
                            self.send_as(Some(me), Cmd::Close);
                        }
                        self.transcript_ui(me, &state, ui, &theme);
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
                    .show(ui, |ui| self.list_ui(me, &state, ui, &theme));
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| self.transcript_ui(me, &state, ui, &theme));
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
    /// The conversation list.
    fn list_ui(&mut self, me: PubKey, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
        let now = self.now();
        ui.horizontal(|ui| {
            ui.heading("Conversations");
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

        self.trouble_ui(&state.trouble_with, ui, theme);

        // The composer is laid out first, from the bottom, so the transcript
        // gets the remaining height rather than pushing it off the screen.
        egui::Panel::bottom("chat_composer")
            .frame(
                egui::Frame::NONE
                    .fill(theme.surface_primary)
                    .inner_margin(egui::Margin::symmetric(0, tokens::SPACING_SM as i8)),
            )
            .show(ui, |ui| self.composer_ui(me, ui, theme));

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        self.messages_ui(state, ui, theme, now);
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
    fn messages_ui(&mut self, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme, now: u64) {
        if state.lines.is_empty() {
            ui.add_space(tokens::SPACING_XL);
            ui.vertical_centered(|ui| {
                ui.colored_label(theme.text_secondary, "Nothing here yet.");
            });
            return;
        }

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
            let bubble = sigil_ui::Bubble {
                key: &key,
                name: line.name.as_deref(),
                text: &line.text,
                at: &sigil_ui::clock(line.at),
                mine: line.mine,
                grouped,
                edited: line.edited,
                redacted: line.redacted,
                reply_to: None,
                reactions: &[],
                receipt: None,
            };
            let _ = sigil_ui::bubble(ui, &bubble);

            previous_author = Some(line.who);
            previous_at = line.at;
        }

        if state.typing {
            ui.add_space(tokens::SPACING_SM);
            ui.colored_label(theme.text_muted, "typing…");
        }
    }

    /// The box a message is written in.
    ///
    /// The button is laid out **first and from the right**, and the field then
    /// takes what is left. Doing it the other way round -- a right-aligned
    /// button before the field -- consumed the whole row, and the field was
    /// allocated the nothing that remained: a composer with no box to write in,
    /// which is what the first snapshot of this showed.
    fn composer_ui(&mut self, me: PubKey, ui: &mut egui::Ui, theme: &ColorTheme) {
        let _ = theme;
        ui.horizontal(|ui| {
            let button = tokens::BUTTON_LG + tokens::SPACING_MD;
            let width = (ui.available_width() - button).max(80.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().composing)
                    .hint_text("Write a message")
                    .desired_width(width),
            );
            let send = ui.button("Send").clicked();
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (entered || send) && !self.pane(me).composing.trim().is_empty() {
                // Taken, not cleared: if the send fails the text has to come
                // back, and the session is what knows whether it did.
                let text = std::mem::take(&mut self.pane(me).composing);
                self.send_as(Some(me), Cmd::Send(text));
                field.request_focus();
            }
        });
    }
}
