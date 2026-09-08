//! Messaging, as a sigil app.

pub mod session;

pub use session::{ChatHandle, ChatState, Closing, Cmd, Line, LinkState, Summary};

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
        }
    }

    /// Keep the stores somewhere other than `~/.sqex/chat`. Tests only.
    #[doc(hidden)]
    pub fn set_store_root_for_test(&mut self, root: std::path::PathBuf) {
        self.store_root = Some(root);
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
        me.and_then(|me| self.sessions.get(&me))
            .map(|s| s.state())
            .unwrap_or_default()
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

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(280.0);
                self.list_ui(me, &state, ui, &theme);
            });
            ui.separator();
            ui.vertical(|ui| self.transcript_ui(me, &state, ui, &theme));
        });
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
    fn list_ui(&mut self, me: PubKey, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Conversations");
        ui.horizontal(|ui| {
            // A visible label, not only a placeholder: a hint disappears the
            // moment somebody types, and it never reaches the accessibility
            // tree at all.
            ui.label("Write to");
            ui.add(
                egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().adding)
                    .hint_text("their key, base58")
                    .desired_width(180.0),
            );
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
        });
        if let Some(t) = self.panes.get(&me).and_then(|p| p.add_trouble.as_ref()) {
            ui.colored_label(theme.destructive, t);
        }
        ui.add_space(tokens::SPACING_SM);

        if state.conversations.is_empty() {
            ui.colored_label(
                theme.text_muted,
                "Nobody yet. Add somebody by their key to write to them first.",
            );
        }
        for convo in &state.conversations {
            // Selected by channel, never by position: the list reorders as
            // conversations move, and an index would follow whoever happened to
            // land there.
            let selected = state.open == Some(convo.channel);
            let label = if convo.unread > 0 {
                format!("{} ({})", convo.label, convo.unread)
            } else {
                convo.label.clone()
            };
            if ui.selectable_label(selected, label).clicked() {
                self.send_as(Some(me), Cmd::Show(convo.channel));
            }
            if convo.waiting {
                ui.colored_label(
                    theme.text_muted,
                    "waiting for them to run a client — nothing can be sealed to them yet",
                );
            }
        }
    }

    fn transcript_ui(
        &mut self,
        me: PubKey,
        state: &ChatState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        let Some(_channel) = state.open else {
            ui.colored_label(theme.text_secondary, "Choose a conversation.");
            return;
        };
        if state.lost > 0 {
            // Said out loud rather than silently missing: these were held under
            // a superseded epoch and are gone for good.
            ui.colored_label(
                theme.warning,
                format!(
                    "{} earlier messages were lost with this client's keys.",
                    state.lost
                ),
            );
        }

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .max_height(360.0)
            .show(ui, |ui| {
                for line in &state.lines {
                    if line.redacted {
                        // A deleted message is still shown, as a gap. The
                        // tombstone is the record.
                        ui.colored_label(theme.text_muted, "(deleted)");
                        continue;
                    }
                    ui.horizontal_wrapped(|ui| {
                        let who = if line.mine {
                            "you"
                        } else {
                            &line.who.to_string()[..8]
                        };
                        ui.colored_label(
                            if line.mine {
                                theme.accent
                            } else {
                                theme.text_secondary
                            },
                            format!("{who}:"),
                        );
                        ui.label(&line.text);
                        if line.edited {
                            // Presenting an edit as the original hides that the
                            // text changed after it was read.
                            ui.colored_label(theme.text_muted, "(edited)");
                        }
                    });
                }
            });

        if state.typing {
            ui.colored_label(theme.text_muted, "typing…");
        }
        ui.add_space(tokens::SPACING_SM);
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().composing)
                    .hint_text("message")
                    .desired_width(420.0),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (entered || ui.button("Send").clicked())
                && !self.pane(me).composing.trim().is_empty()
            {
                let text = std::mem::take(&mut self.pane(me).composing);
                self.send_as(Some(me), Cmd::Send(text));
                field.request_focus();
            }
        });
    }
}
