//! Calls and rooms, as a sigil app.
//!
//! Everything that holds a call is elsewhere: `sqex_voice::engine` runs the
//! loop, `sigil_net` runs it on a task and reports what it is doing. This draws
//! the result and collects the two decisions a person makes — who to call, and
//! when to stop.
//!
//! Nothing here awaits anything. A frame that waits on the network is a frame
//! that is not drawn, and the whole arrangement exists to make that impossible
//! rather than merely unlikely.

use sigil::account::Account;
use sigil::app::{App, AppContext, AppResponse};
use sigil::{ColorTheme, tokens};
use sigil_net::{
    CallHandle, CallOpts, CallState, Phase, RoomId, discovery, spawn_call, spawn_room,
};
use sqnr::config::Config;
use sqnr_core::PubKey;

/// How many lines of narrative to keep. Enough to see what happened during a
/// call, bounded so a machine left running for a week does not accumulate one
/// allocation per second forever.
const LOG_LIMIT: usize = 500;

/// Where you are inside the voice app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Roster,
}

pub struct VoiceApp {
    /// The key typed into the call field, and why it was refused if it was.
    peer_input: String,
    peer_trouble: Option<String>,
    /// The room secret typed in, or one just minted and not yet joined.
    room_input: String,
    room_trouble: Option<String>,
    /// The passphrase field, when the identity is sealed. Held here rather than
    /// in `Account` because it is a transient piece of interface, not a
    /// property of the identity.
    passphrase: String,
    call: Option<CallHandle>,
    /// What the engine has said, newest last.
    log: Vec<String>,
    /// The exchange, read once at startup. Re-read when settings can change it.
    config: Config,
}

impl Default for VoiceApp {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceApp {
    pub fn new() -> Self {
        Self {
            peer_input: String::new(),
            peer_trouble: None,
            room_input: String::new(),
            room_trouble: None,
            passphrase: String::new(),
            call: None,
            log: Vec::new(),
            config: Config::load(),
        }
    }

    fn note(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > LOG_LIMIT {
            // Drain from the front in one go rather than one `remove(0)` per
            // line, which is quadratic and happens on the frame path.
            let excess = self.log.len() - LOG_LIMIT;
            self.log.drain(..excess);
        }
    }

    /// The current call's state, or a default one when there is no call.
    fn state(&self) -> CallState {
        self.call.as_ref().map(|c| c.state()).unwrap_or_default()
    }

    /// The exchange to dial, or why we cannot.
    ///
    /// Shared by calling and joining so that both refuse for the same reasons
    /// in the same words — two copies of this drifted apart in the CLI once,
    /// which is why resolution itself lives in one place.
    fn where_to(&self) -> Result<[sigil_net::Layer; 3], String> {
        let layers = discovery::layers(discovery::nothing_explicit(), &self.config);
        if !discovery::any_configured(&layers) {
            return Err("no exchange configured — set SQEX_SERVER or ~/.sqnr/config".into());
        }
        Ok(layers)
    }

    fn place_call(&mut self, account: &Account, egui_ctx: &egui::Context) {
        self.peer_trouble = None;
        let Some(unlocked) = account.unlocked() else {
            self.peer_trouble = Some("unlock your identity first".into());
            return;
        };
        let peer: PubKey = match self.peer_input.trim().parse() {
            Ok(k) => k,
            Err(e) => {
                self.peer_trouble = Some(format!("that is not a key: {e}"));
                return;
            }
        };
        if peer == unlocked.me() {
            // Worth catching here rather than at the exchange, which would only
            // ever answer `Waiting`: a session needs two identities.
            self.peer_trouble = Some("that is you — a call needs somebody else".into());
            return;
        }
        let layers = match self.where_to() {
            Ok(l) => l,
            Err(e) => {
                self.peer_trouble = Some(e);
                return;
            }
        };

        let wake = egui_ctx.clone();
        self.call = Some(spawn_call(
            layers,
            unlocked.signer(),
            peer,
            120,
            CallOpts::default(),
            // The only thing that makes this interface redraw. Everything else
            // is idle, which is what lets a silent call cost nothing.
            move || wake.request_repaint(),
        ));
        self.log.clear();
    }

    fn join_room(&mut self, account: &Account, egui_ctx: &egui::Context) {
        self.room_trouble = None;
        let Some(unlocked) = account.unlocked() else {
            self.room_trouble = Some("unlock your identity first".into());
            return;
        };
        let room: RoomId = match self.room_input.trim().parse() {
            Ok(r) => r,
            Err(e) => {
                self.room_trouble = Some(format!("that is not a room secret: {e}"));
                return;
            }
        };
        let layers = match self.where_to() {
            Ok(l) => l,
            Err(e) => {
                self.room_trouble = Some(e);
                return;
            }
        };
        let wake = egui_ctx.clone();
        self.call = Some(spawn_room(
            layers,
            unlocked.signer(),
            room,
            CallOpts::default(),
            move || wake.request_repaint(),
        ));
        self.log.clear();
    }
}

impl App for VoiceApp {
    /// Runs every pass, for every opened app, and while the window is hidden.
    /// Draining here rather than in `render` is what keeps a call's history
    /// intact while you are reading messages in the other tab.
    fn update(&mut self, _ctx: &mut AppContext<'_>, _egui_ctx: &egui::Context) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        for event in call.drain() {
            self.note(event.describe());
        }
    }

    fn render(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        ui.spacing_mut().item_spacing.y = tokens::SPACING_SM;

        if !ctx.account.is_unlocked() {
            self.identity_ui(ctx, ui, &theme);
            return AppResponse::default();
        }
        match self.state().phase {
            Phase::Idle | Phase::Ended => self.idle_ui(ctx, ui, &theme),
            Phase::Connecting | Phase::Waiting | Phase::Live => self.call_ui(ui, &theme),
        }
        AppResponse::default()
    }

    fn title(&self) -> &str {
        "Calls"
    }
}

impl VoiceApp {
    /// Unlocking, without a terminal prompt anywhere in sight.
    fn identity_ui(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Identity");
        ui.colored_label(theme.text_secondary, ctx.account.describe());
        ui.add_space(tokens::SPACING_SM);

        if let Account::Locked { .. } = ctx.account {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.passphrase)
                    .password(true)
                    .hint_text("passphrase")
                    .desired_width(320.0),
            );
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (entered || ui.button("Unlock").clicked()) && ctx.account.unlock(&self.passphrase) {
                // Only cleared on success. Making somebody retype a long
                // passphrase because the program threw it away is its own
                // small cruelty.
                self.passphrase.clear();
            }
        }
        if let Account::Missing { .. } | Account::Broken { .. } = ctx.account {
            ui.colored_label(
                theme.text_muted,
                "Voice and chat act as an identity on the transport, so they need a \
                 software identity. A YubiKey signs but never releases a seed, and \
                 cannot be a transport key.",
            );
        }
    }

    /// No call in progress: who would you like to call?
    fn idle_ui(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Calls");
        if let Some(me) = ctx.account.unlocked().map(|u| u.me()) {
            // In full, and selectable, because a key is the only thing that
            // actually identifies somebody (SIP-21).
            ui.horizontal(|ui| {
                ui.colored_label(theme.text_secondary, "You are");
                ui.add(
                    egui::Label::new(egui::RichText::new(me.to_string()).monospace())
                        .selectable(true),
                );
            });
        }
        ui.add_space(tokens::SPACING_MD);

        let state = self.state();
        if let Some(trouble) = &state.trouble {
            ui.colored_label(theme.destructive, trouble);
        }
        if let Some(summary) = &state.final_stats {
            ui.colored_label(theme.text_secondary, format!("Last call — {summary}"));
        }
        ui.add_space(tokens::SPACING_SM);

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.peer_input)
                    .hint_text("their key, base58")
                    .desired_width(420.0),
            );
            if ui.button("Call").clicked() {
                self.place_call(ctx.account, ui.ctx());
            }
        });
        if let Some(trouble) = &self.peer_trouble {
            ui.colored_label(theme.destructive, trouble);
        }

        ui.add_space(tokens::SPACING_XL);
        self.room_entry_ui(ctx, ui, theme);
        self.log_ui(ui, theme);
    }

    /// Minting or joining a room.
    fn room_entry_ui(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Rooms");
        ui.colored_label(
            theme.text_secondary,
            "A room is named by a secret, and holding it is what being in the room \
             consists of.",
        );
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.room_input)
                    .hint_text("room secret, base58")
                    .desired_width(420.0),
            );
            if ui.button("Join").clicked() {
                self.join_room(ctx.account, ui.ctx());
            }
            if ui.button("New room").clicked() {
                self.room_input = RoomId::generate().to_base58();
                self.room_trouble = None;
            }
        });
        if let Some(trouble) = &self.room_trouble {
            ui.colored_label(theme.destructive, trouble);
        }
        if !self.room_input.is_empty() {
            // Said wherever a secret is on screen, because it is the whole
            // security model and it is not what people expect from a group
            // chat: there is no owner, nobody can be removed, and anyone you
            // give it to can pass it on. Excluding somebody means a new room.
            ui.colored_label(
                theme.warning,
                "Anyone you give this to is in the room, and can give it to anyone else. \
                 It cannot be taken back — to leave somebody out, mint a new room.",
            );
        }
    }

    /// A call or a room in progress.
    fn call_ui(&mut self, ui: &mut egui::Ui, theme: &ColorTheme) {
        let state = self.state();
        let in_room = state.room.is_some();
        ui.heading(match (state.phase, in_room) {
            (Phase::Connecting, _) => "Connecting…",
            (Phase::Waiting, _) => "Waiting for them to answer",
            (_, true) => "In a room",
            (_, false) => "On a call",
        });

        if in_room {
            self.roster_ui(&state, ui);
        }

        if let Some(peer) = state.peer {
            ui.horizontal(|ui| {
                ui.colored_label(theme.text_secondary, "with");
                ui.add(
                    egui::Label::new(egui::RichText::new(peer.to_string()).monospace())
                        .selectable(true),
                );
            });
        }
        if state.phase == Phase::Waiting {
            // The wait is mutual consent, not a fault. Say so, or it reads as
            // a program that has hung.
            ui.colored_label(
                theme.text_muted,
                "A session opens only when both sides have named the other, so nothing \
                 happens until they call you back.",
            );
        }
        if state.deaf {
            ui.colored_label(
                theme.warning,
                "Nothing has arrived from them at all — see the log below.",
            );
        }
        ui.add_space(tokens::SPACING_MD);

        let leave = if in_room { "Leave" } else { "Hang up" };
        if ui.button(leave).clicked()
            && let Some(call) = &self.call
        {
            call.hang_up();
        }
        ui.add_space(tokens::SPACING_MD);

        if let Some(stats) = &state.stats {
            ui.colored_label(theme.text_secondary, egui::RichText::new(stats).monospace());
        }
        self.log_ui(ui, theme);
    }

    /// Who is in the room, and who is talking.
    ///
    /// The drawing is `sigil_ui::roster`, which takes plain data; this only
    /// maps the protocol's `PeerStatus` onto it. Keeping the widget free of the
    /// wire format is what lets it be tested against a five-person room without
    /// arranging one.
    fn roster_ui(&self, state: &CallState, ui: &mut egui::Ui) {
        ui.add_space(tokens::SPACING_SM);
        let rows: Vec<sigil_ui::Row> = state
            .present
            .iter()
            .map(|p| sigil_ui::Row {
                key: p.identity.to_string(),
                speaking: p.speaking,
                level: p.level,
                detail: format!(
                    "loss {:.0}% · conceal {} · buf {}",
                    p.loss_pct, p.concealed, p.buffered
                ),
            })
            .collect();
        sigil_ui::roster(ui, &rows, state.connecting);
    }

    fn log_ui(&self, ui: &mut egui::Ui, theme: &ColorTheme) {
        if self.log.is_empty() {
            return;
        }
        ui.add_space(tokens::SPACING_MD);
        ui.separator();
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .max_height(240.0)
            .show(ui, |ui| {
                for line in &self.log {
                    ui.colored_label(theme.text_muted, egui::RichText::new(line).monospace());
                }
            });
    }
}
