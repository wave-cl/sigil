//! Messaging, as a sigil app.

pub mod session;

pub use session::{ChatHandle, ChatState, Cmd, Line, LinkState, Summary};

use sigil::account::Account;
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

pub struct ChatApp {
    session: Option<ChatHandle>,
    /// What is being typed. Kept here rather than in the session so that a
    /// failed send leaves it on screen: retyping a message the program lost is
    /// the worst thing a chat client can do to somebody.
    composing: String,
    /// The key being added as a contact.
    adding: String,
    add_trouble: Option<String>,
    config: Config,
}

impl Default for ChatApp {
    fn default() -> Self {
        Self::new()
    }
}

impl ChatApp {
    pub fn new() -> Self {
        Self {
            session: None,
            composing: String::new(),
            adding: String::new(),
            add_trouble: None,
            config: Config::load(),
        }
    }

    /// Point at an exchange without reading `~/.sqnr/config`.
    #[doc(hidden)]
    pub fn set_exchange_for_test(&mut self, host: &str, key: &str) {
        self.config.server = Some(host.to_string());
        self.config.server_key = Some(key.to_string());
    }

    #[doc(hidden)]
    pub fn running_for_test(&self) -> bool {
        self.session.is_some()
    }

    fn state(&self) -> ChatState {
        self.session.as_ref().map(|s| s.state()).unwrap_or_default()
    }

    /// Start the session once there is an identity to run it as.
    ///
    /// Started from `update`, not `render`, so it keeps running while somebody
    /// is on a call in the other tab -- and so that messages arrive whether or
    /// not this app is the one on screen.
    fn start(&mut self, account: &Account, egui_ctx: &egui::Context) {
        if self.session.is_some() {
            return;
        }
        let Some(unlocked) = account.unlocked() else {
            return;
        };
        let layers = discovery::layers(discovery::nothing_explicit(), &self.config);
        if !discovery::any_configured(&layers) {
            return;
        }
        let wake = egui_ctx.clone();
        self.session = Some(session::start(layers, unlocked.signer(), None, move || {
            wake.request_repaint()
        }));
    }

    fn send(&mut self, cmd: Cmd) {
        if let Some(s) = &self.session {
            s.send(cmd);
        }
    }
}

impl App for ChatApp {
    fn update(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        self.start(ctx.account, egui_ctx);
    }

    fn render(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        if !ctx.account.is_unlocked() {
            ui.heading("Chat");
            ui.colored_label(
                theme.text_secondary,
                "Unlock your identity to start chatting.",
            );
            return AppResponse::default();
        }
        let state = self.state();

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
                self.send(Cmd::Reconnect);
            }
        });
        if let Some(trouble) = &state.trouble {
            ui.colored_label(theme.destructive, trouble);
        }
        ui.separator();

        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(280.0);
                self.list_ui(&state, ui, &theme);
            });
            ui.separator();
            ui.vertical(|ui| self.transcript_ui(&state, ui, &theme));
        });
        AppResponse::default()
    }

    fn tab_notifications(&self) -> TabNotifications {
        TabNotifications::count(
            self.state()
                .conversations
                .iter()
                .map(|c| c.unread as u32)
                .sum(),
        )
    }

    fn title(&self) -> &str {
        "Chat"
    }
}

impl ChatApp {
    fn list_ui(&mut self, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Conversations");
        ui.horizontal(|ui| {
            // A visible label, not only a placeholder: a hint disappears the
            // moment somebody types, and it never reaches the accessibility
            // tree at all.
            ui.label("Write to");
            ui.add(
                egui::TextEdit::singleline(&mut self.adding)
                    .hint_text("their key, base58")
                    .desired_width(180.0),
            );
            if ui.button("Add").clicked() {
                match self.adding.trim().parse::<PubKey>() {
                    Ok(who) => {
                        self.add_trouble = None;
                        self.send(Cmd::AddContact(who, String::new()));
                        self.send(Cmd::OpenDm(who));
                        self.adding.clear();
                    }
                    Err(e) => self.add_trouble = Some(format!("that is not a key: {e}")),
                }
            }
        });
        if let Some(t) = &self.add_trouble {
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
                self.send(Cmd::Show(convo.channel));
            }
            if convo.waiting {
                ui.colored_label(
                    theme.text_muted,
                    "waiting for them to run a client — nothing can be sealed to them yet",
                );
            }
        }
    }

    fn transcript_ui(&mut self, state: &ChatState, ui: &mut egui::Ui, theme: &ColorTheme) {
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
                egui::TextEdit::singleline(&mut self.composing)
                    .hint_text("message")
                    .desired_width(420.0),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (entered || ui.button("Send").clicked()) && !self.composing.trim().is_empty() {
                let text = std::mem::take(&mut self.composing);
                self.send(Cmd::Send(text));
                field.request_focus();
            }
        });
    }
}
