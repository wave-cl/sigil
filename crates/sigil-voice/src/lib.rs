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
    CallHandle, CallOpts, CallState, Incoming, Phase, RingListener, RoomId, discovery, listen,
    spawn_call, spawn_room,
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
    /// Listens for somebody calling us, for as long as the identity is
    /// unlocked. Started once; not restarted per call.
    listener: Option<RingListener>,
    /// Rings that have arrived and not been answered. The newest is the one
    /// shown; the rest are already history.
    ringing: Vec<Incoming>,
    /// Calls that rang while we were busy or away, newest first.
    missed: Vec<Incoming>,
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
            listener: None,
            ringing: Vec::new(),
            missed: Vec::new(),
            log: Vec::new(),
            config: Config::load(),
        }
    }

    /// Pretend somebody rang, so the ringing interface can be tested without
    /// arranging a second client, an exchange and a caller.
    #[doc(hidden)]
    pub fn ring_for_test(&mut self, from: PubKey) {
        self.ringing.push(Incoming { from, at: 0 });
    }

    /// Point at an exchange without reading `~/.sqnr/config`, which a test must
    /// never depend on.
    #[doc(hidden)]
    pub fn set_exchange_for_test(&mut self, host: &str, key: &str) {
        self.config.server = Some(host.to_string());
        self.config.server_key = Some(key.to_string());
    }

    /// Whether a ring listener is running.
    ///
    /// Exposed for one reason: this was once wired up in `render` instead of
    /// `update` and therefore never started at all, and every test passed
    /// because they injected rings directly. Nothing observed the listener, so
    /// nothing noticed a phone that could not ring.
    #[doc(hidden)]
    pub fn listening_for_test(&self) -> bool {
        self.listener.is_some()
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

    /// Start listening for calls, once, as soon as there is an identity to
    /// listen as.
    ///
    /// Not tied to a call: the whole point is to be listening when there is no
    /// call, which is most of the time.
    fn start_listening(&mut self, account: &Account, egui_ctx: &egui::Context) {
        if self.listener.is_some() {
            return;
        }
        let Some(unlocked) = account.unlocked() else {
            return;
        };
        let Ok(layers) = self.where_to() else { return };
        let wake = egui_ctx.clone();
        self.listener = Some(listen(layers, unlocked.signer(), move || {
            wake.request_repaint()
        }));
    }

    /// Answer whoever is ringing: open a session with them, which is what
    /// consent consists of here.
    fn answer(&mut self, from: PubKey, account: &Account, egui_ctx: &egui::Context) {
        self.ringing.retain(|r| r.from != from);
        self.peer_input = from.to_string();
        self.place_call(account, egui_ctx);
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
    fn update(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        // Runs for every opened app and while the window is hidden, which is
        // exactly when a call has to be able to arrive. Starting the listener
        // here rather than in `render` is what lets the phone ring while
        // somebody is reading messages in the other tab, or nothing at all.
        self.start_listening(ctx.account, egui_ctx);

        if let Some(listener) = self.listener.as_mut() {
            let arrived = listener.drain();
            let busy = self.call.is_some();
            for ring in arrived {
                // Somebody already in a call is not rung at; it goes straight
                // to missed. A second ring over a live conversation is an
                // interruption nobody asked for, and the caller learns nothing
                // either way.
                if busy {
                    self.missed.insert(0, ring);
                } else if !self.ringing.iter().any(|r| r.from == ring.from) {
                    self.ringing.push(ring);
                }
            }
        }

        if let Some(call) = self.call.as_mut() {
            for event in call.drain() {
                self.note(event.describe());
            }
        }
    }

    fn render(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        ui.spacing_mut().item_spacing.y = tokens::SPACING_SM;

        if !ctx.account.is_unlocked() {
            self.identity_ui(ctx, ui, &theme);
            return AppResponse::default();
        }
        // A ring outranks everything else on screen. It is the one thing here
        // that is somebody else waiting on an answer.
        if let Some(ring) = self.ringing.last().copied() {
            return self.ringing_ui(ring, ctx, ui, &theme);
        }
        match self.state().phase {
            Phase::Idle | Phase::Ended => self.idle_ui(ctx, ui, &theme),
            Phase::Connecting | Phase::Waiting | Phase::Live => self.call_ui(ui, &theme),
        }
        AppResponse::default()
    }

    /// Badge the tab with anything ringing plus anything missed, so a call
    /// that arrived while somebody was reading messages is visible from the
    /// other tab rather than only on this one.
    fn tab_notifications(&self) -> sigil::TabNotifications {
        sigil::TabNotifications::count((self.ringing.len() + self.missed.len()) as u32)
    }

    fn title(&self) -> &str {
        "Calls"
    }
}

impl VoiceApp {
    /// Somebody is calling.
    ///
    /// The key is shown in full and the name is not shown at all, because a
    /// ring is not authenticated: who sent it is the exchange's observation of
    /// who connected, not a signature. Presenting that as an established
    /// identity would be a lie the interface told on the protocol's behalf.
    ///
    /// Answering is safe regardless, and it is worth knowing why. Accepting
    /// opens a session with *the identity named*, and the session derives from
    /// that identity's key — so a forged ring cannot connect you to the forger.
    /// It buys them a call that never establishes.
    fn ringing_ui(
        &mut self,
        ring: Incoming,
        ctx: &mut AppContext<'_>,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) -> AppResponse {
        ui.add_space(tokens::SPACING_XL);
        ui.heading("Incoming call");
        ui.add_space(tokens::SPACING_SM);
        ui.colored_label(theme.text_secondary, "from");
        ui.add(
            egui::Label::new(egui::RichText::new(ring.from.to_string()).monospace())
                .selectable(true),
        );
        ui.add_space(tokens::SPACING_SM);
        ui.colored_label(
            theme.text_muted,
            "Who a ring says it is from is the exchange's word, not a signature. \
             Answering opens a session with this key and nobody else.",
        );
        ui.add_space(tokens::SPACING_LG);

        let mut answered = None;
        let mut declined = false;
        ui.horizontal(|ui| {
            if ui.button("Answer").clicked() {
                answered = Some(ring.from);
            }
            if ui.button("Decline").clicked() {
                declined = true;
            }
        });
        if let Some(from) = answered {
            self.answer(from, ctx.account, ui.ctx());
        }
        if declined {
            // Declining is silent. There is no way to tell a caller "no"
            // without telling them you are here, and somebody who does not
            // want to be reached by them should not be made to announce it.
            self.ringing.retain(|r| r.from != ring.from);
            self.missed.insert(0, ring);
        }
        // Bring the window forward: a call is the one thing here worth
        // interrupting whatever else somebody is looking at.
        AppResponse::action(sigil::AppAction::Present)
    }

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
        self.missed_ui(ui, theme);
        self.listening_ui(ui, theme);
        self.log_ui(ui, theme);
    }

    /// Calls that rang while we were busy or away.
    fn missed_ui(&mut self, ui: &mut egui::Ui, theme: &ColorTheme) {
        if self.missed.is_empty() {
            return;
        }
        ui.add_space(tokens::SPACING_XL);
        ui.heading("Missed");
        for ring in self.missed.clone() {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(ring.from.to_string()).monospace())
                        .selectable(true),
                );
                if ui.button("Call back").clicked() {
                    self.peer_input = ring.from.to_string();
                    self.missed.retain(|m| m.from != ring.from);
                }
            });
        }
        if ui.button("Clear missed").clicked() {
            self.missed.clear();
        }
        let _ = theme;
    }

    /// Whether calls can arrive at all.
    ///
    /// Said out loud when it is not working. A phone that has quietly stopped
    /// ringing is worse than one that is obviously broken: the failure is
    /// invisible precisely when it matters, because nothing happening looks
    /// exactly like nobody calling.
    fn listening_ui(&self, ui: &mut egui::Ui, theme: &ColorTheme) {
        let Some(listener) = &self.listener else {
            return;
        };
        let state = listener.state();
        if state.listening {
            return;
        }
        ui.add_space(tokens::SPACING_MD);
        ui.colored_label(
            theme.destructive,
            state
                .trouble
                .unwrap_or_else(|| "not listening for calls".into()),
        );
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
