//! Running an exchange, as a sigil app.
//!
//! The other apps act for a person; this one acts on the exchange. Everything
//! that changes anything is a signed SIP-10 transaction, and every one of them
//! is **confirmed before it is signed** — see [`session`].

pub mod session;

pub use session::{AdminHandle, AdminState, Answer, Cmd};

use std::collections::HashMap;

use sigil::app::{App, AppContext, AppResponse};
use sigil::{ColorTheme, tokens};
use sigil_net::discovery;
use sqex_proto::Op;
use sqnr::config::Config;
use sqnr_core::PubKey;

/// What is being typed into the console, per identity.
#[derive(Default)]
struct Pane {
    key: String,
    label: String,
    name: String,
    tail: u32,
    /// A batch waiting to be confirmed. **Nothing is signed while this is
    /// `Some`**: `sign_and_submit`'s review callback runs during signing and
    /// can only report, so the question has to be asked here, first.
    pending: Option<Vec<Op>>,
}

pub struct AdminApp {
    sessions: HashMap<PubKey, AdminHandle>,
    panes: HashMap<PubKey, Pane>,
    config: Config,
    /// A state to draw instead of a session's. Tests only.
    fixed: Option<AdminState>,
}

impl Default for AdminApp {
    fn default() -> Self {
        Self::new()
    }
}

impl AdminApp {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            panes: HashMap::new(),
            config: Config::load(),
            fixed: None,
        }
    }

    #[doc(hidden)]
    pub fn set_exchange_for_test(&mut self, host: &str, key: &str) {
        self.config.server = Some(host.to_string());
        self.config.server_key = Some(key.to_string());
    }

    /// Draw this instead of a session's state. See `sigil_chat`'s equivalent:
    /// it replaces the data and nothing else, so `render` is the same code on
    /// the same path either way.
    #[doc(hidden)]
    pub fn show_state_for_test(&mut self, state: AdminState) {
        self.fixed = Some(state);
    }

    fn state_of(&self, me: Option<PubKey>) -> AdminState {
        if let Some(fixed) = &self.fixed {
            return fixed.clone();
        }
        me.and_then(|me| self.sessions.get(&me))
            .map(|s| s.state())
            .unwrap_or_default()
    }

    fn pane(&mut self, me: PubKey) -> &mut Pane {
        self.panes.entry(me).or_default()
    }

    fn showing(ctx: &AppContext<'_>) -> Option<PubKey> {
        ctx.account().unlocked().map(|u| u.me())
    }

    fn send_as(&self, me: Option<PubKey>, cmd: Cmd) {
        if let Some(s) = me.and_then(|me| self.sessions.get(&me)) {
            s.send(cmd);
        }
    }

    /// One session per unlocked identity, like the other apps.
    ///
    /// An identity that is not an administrator of this exchange still gets
    /// one: it connects, `/health` answers, and every signed operation is
    /// refused. That is the honest arrangement — whether you may administer an
    /// exchange is the exchange's answer, not something to guess at from here.
    fn reconcile(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        let held: Vec<(PubKey, std::path::PathBuf)> = ctx
            .accounts
            .unlocked()
            .map(|(me, u)| (me, u.path().to_path_buf()))
            .collect();

        let live: Vec<PubKey> = self.sessions.keys().copied().collect();
        for me in live {
            if !held.iter().any(|(k, _)| *k == me)
                && let Some(mut session) = self.sessions.remove(&me)
            {
                session.stop();
            }
        }
        for (me, path) in held {
            if self.sessions.contains_key(&me) {
                continue;
            }
            let Some((_, unlocked)) = ctx.accounts.unlocked().find(|(k, _)| *k == me) else {
                continue;
            };
            // **The connection this identity's chat session already holds.**
            // Dialling a second is a second handshake, a second socket and a
            // second keep-alive timer for one identity talking to one exchange
            // -- and it is the connection the exchange would fan a call's
            // datagrams to as well.
            //
            // Which exchange, when the identity is on more than one, is
            // `Connections::one_of`'s to answer: this console acts on whichever
            // one the identity is actually connected to, and the endpoint comes
            // back with the connection because a signed command is bound to the
            // exchange's key.
            //
            // It also gains the reconnection this session has never had, for
            // free: the chat session redials and the slot is rewritten.
            let reach: sigil_net::Dial = match ctx.connections.one_of(me) {
                // Taken whether or not it is live yet: a slot exists from the
                // moment a chat session is started, and is filled a handshake
                // later. The session waits for it rather than dialling its own
                // over a second's difference.
                Some(held) => held.into(),
                None => {
                    let layers =
                        discovery::layers(discovery::nothing_explicit(), &self.config, Some(&path));
                    if !discovery::any_configured(&layers) {
                        continue;
                    }
                    layers.into()
                }
            };
            let wake = egui_ctx.clone();
            self.sessions.insert(
                me,
                session::start(reach, unlocked.signer(), move || wake.request_repaint()),
            );
        }
    }

    /// Queue a batch for confirmation. Nothing is signed until it is confirmed.
    fn propose(&mut self, me: PubKey, ops: Vec<Op>) {
        self.pane(me).pending = Some(ops);
    }
}

impl App for AdminApp {
    fn update(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        self.reconcile(ctx, egui_ctx);
    }

    fn title(&self) -> &str {
        "Exchange"
    }

    fn icon(&self) -> sigil::Icon {
        sigil::Icon::Settings
    }

    fn render(&mut self, ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        let Some(me) = Self::showing(ctx) else {
            ui.heading("Exchange");
            ui.colored_label(
                theme.text_secondary,
                "Unlock an identity to administer an exchange.",
            );
            return AppResponse::default();
        };
        let state = self.state_of(Some(me));

        self.header_ui(&state, ui, &theme);
        if let Some(trouble) = &state.trouble {
            ui.colored_label(theme.destructive, trouble);
        }
        ui.separator();

        // The confirmation stands in front of everything else while it is up.
        // A console where the question can be scrolled away from is a console
        // where it gets answered by accident.
        if self.confirm_ui(me, &state, ui, &theme) {
            return AppResponse::default();
        }

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.whitelist_ui(me, ui, &theme);
                ui.separator();
                self.admission_ui(me, ui, &theme);
                ui.separator();
                self.names_ui(me, ui, &theme);
                ui.separator();
                self.peers_ui(me, ui, &theme);
                ui.separator();
                self.audit_ui(me, ui, &theme);
                ui.separator();
                self.answers_ui(&state, ui, &theme);
            });
        AppResponse::default()
    }
}

impl AdminApp {
    /// Who we are signing as, and what we are signing at.
    fn header_ui(&self, state: &AdminState, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.horizontal(|ui| {
            ui.heading("Exchange");
            // Up and admitted are different questions. `/health` is unsigned
            // and unauthenticated, so it answers the first and says nothing
            // about the second — and an operator staring at a refusal wants to
            // know which one they are looking at.
            match state.healthy {
                Some(true) => {
                    sigil_ui::dot(ui, true, theme.success, theme.success, "answering");
                    ui.colored_label(theme.success, "answering");
                }
                Some(false) => {
                    sigil_ui::dot(ui, false, theme.destructive, theme.destructive, "silent");
                    ui.colored_label(theme.destructive, "not answering");
                }
                None => {
                    ui.colored_label(theme.text_muted, "connecting…");
                }
            }
        });
        if let Some(exchange) = &state.exchange {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("exchange {exchange}"))
                        .monospace()
                        .small(),
                )
                .selectable(true),
            );
        }
        if let Some(admin) = &state.admin {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("signing as {admin}"))
                        .monospace()
                        .small(),
                )
                .selectable(true),
            );
        }
        // Said once, plainly: whether this identity may administer anything is
        // the exchange's answer and not something the console can know.
        ui.colored_label(
            theme.text_muted,
            egui::RichText::new(
                "Whether you may administer this exchange is its answer, not this window's. \
                 An operation you are not an administrator for is refused.",
            )
            .small(),
        );
    }

    /// What is about to be signed, and the two answers to it.
    ///
    /// Returns whether it is up, because while it is nothing else is drawn.
    fn confirm_ui(
        &mut self,
        me: PubKey,
        state: &AdminState,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) -> bool {
        let Some(ops) = self.panes.get(&me).and_then(|p| p.pending.clone()) else {
            return false;
        };
        egui::Frame::NONE
            .fill(theme.surface_elevated)
            .corner_radius(tokens::RADIUS_LG)
            .inner_margin(egui::Margin::same(tokens::SPACING_MD as i8))
            .show(ui, |ui| {
                ui.heading("Sign this?");
                // The batch is atomic at the exchange, so what is being agreed
                // to is the batch and not the operations one at a time.
                ui.colored_label(
                    theme.text_secondary,
                    "The exchange applies all of it or none of it.",
                );
                ui.add_space(tokens::SPACING_SM);
                for op in &ops {
                    let operation = op.to_operation();
                    ui.label(egui::RichText::new(&operation.summary).strong());
                    for line in &operation.detail {
                        ui.colored_label(
                            theme.text_secondary,
                            egui::RichText::new(line).monospace().small(),
                        );
                    }
                }
                ui.add_space(tokens::SPACING_SM);
                ui.horizontal(|ui| {
                    let can = !state.busy;
                    if ui
                        .add_enabled(can, egui::Button::new("Sign and submit"))
                        .clicked()
                    {
                        self.pane(me).pending = None;
                        self.send_as(Some(me), Cmd::Submit(ops.clone()));
                    }
                    if ui.button("Cancel").clicked() {
                        self.pane(me).pending = None;
                    }
                    if state.busy {
                        ui.colored_label(theme.text_muted, "signing…");
                    }
                });
            });
        true
    }

    fn whitelist_ui(&mut self, me: PubKey, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Whitelist");
        // SIP-24: the whitelist is a closed set and chat is an open one, which
        // is why it gates some routes and not others. Saying so stops somebody
        // reading "enabled" as "the exchange is now private".
        ui.colored_label(
            theme.text_secondary,
            "A closed set of transport keys. It gates the routes that are gated — \
             turning it on does not close the exchange.",
        );
        ui.horizontal(|ui| {
            if ui.button("List").clicked() {
                self.propose(me, vec![Op::WhitelistList]);
            }
            if ui.button("Enable").clicked() {
                self.propose(me, vec![Op::WhitelistEnable]);
            }
            if ui.button("Disable").clicked() {
                self.propose(me, vec![Op::WhitelistDisable]);
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().key)
                    .hint_text("key, base58")
                    .desired_width(260.0),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().label)
                    .hint_text("label")
                    .desired_width(120.0),
            );
            if ui.button("Add").clicked()
                && let Some(key) = self.take_key(me)
            {
                let label = self.pane(me).label.clone();
                self.propose(
                    me,
                    vec![Op::WhitelistAdd {
                        key,
                        label: (!label.is_empty()).then_some(label),
                    }],
                );
            }
            if ui.button("Remove").clicked()
                && let Some(key) = self.take_key(me)
            {
                self.propose(me, vec![Op::WhitelistRemove(key)]);
            }
        });
    }

    fn admission_ui(&mut self, me: PubKey, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Admission");
        // The line that decides how this screen should be read.
        ui.colored_label(
            theme.text_secondary,
            "A credential is evidence, not authority: it says which account vouches for a \
             key, and entitles that key to nothing. Admitting one is a decision.",
        );
        ui.horizontal(|ui| {
            if ui.button("Pending").clicked() {
                self.propose(me, vec![Op::AdmissionList]);
            }
            if ui.button("Approve").clicked()
                && let Some(device) = self.take_key(me)
            {
                let label = self.pane(me).label.clone();
                self.propose(
                    me,
                    vec![Op::AdmissionApprove {
                        device,
                        label: (!label.is_empty()).then_some(label),
                    }],
                );
            }
            if ui.button("Deny").clicked()
                && let Some(device) = self.take_key(me)
            {
                self.propose(me, vec![Op::AdmissionDeny(device)]);
            }
        });
    }

    fn names_ui(&mut self, me: PubKey, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Names");
        ui.colored_label(
            theme.text_secondary,
            "SIP-38. Assigning reassigns an existing binding and is not subject to the \
             per-account cap — it is how a squatted name is corrected.",
        );
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.panes.entry(me).or_default().name)
                    .hint_text("name")
                    .desired_width(160.0),
            );
            if ui.button("List").clicked() {
                self.propose(me, vec![Op::NameList]);
            }
            if ui.button("Assign").clicked() {
                let name = self.pane(me).name.clone();
                if let Some(account) = self.take_key(me)
                    && !name.is_empty()
                {
                    self.propose(me, vec![Op::NameAssign { name, account }]);
                }
            }
            if ui.button("Release").clicked() {
                let name = self.pane(me).name.clone();
                if !name.is_empty() {
                    self.pane(me).name.clear();
                    self.propose(me, vec![Op::NameRelease(name)]);
                }
            }
        });
    }

    fn peers_ui(&mut self, me: PubKey, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Relay peers");
        ui.colored_label(
            theme.text_secondary,
            "SIP-39. Removing one stops the next call across it, not one already up.",
        );
        ui.horizontal(|ui| {
            if ui.button("List").clicked() {
                self.propose(me, vec![Op::PeerList]);
            }
            if ui.button("Add").clicked()
                && let Some(key) = self.take_key(me)
            {
                let label = self.pane(me).label.clone();
                self.propose(
                    me,
                    vec![Op::PeerAdd {
                        key,
                        label: (!label.is_empty()).then_some(label),
                    }],
                );
            }
            if ui.button("Remove").clicked()
                && let Some(key) = self.take_key(me)
            {
                self.propose(me, vec![Op::PeerRemove(key)]);
            }
        });
    }

    fn audit_ui(&mut self, me: PubKey, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Audit and status");
        let _ = theme;
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut self.panes.entry(me).or_default().tail)
                    .range(1..=500)
                    .prefix("last "),
            );
            if ui.button("Audit").clicked() {
                let n = self.pane(me).tail.max(1);
                self.propose(me, vec![Op::AuditTail(n)]);
            }
            if ui.button("Status").clicked() {
                self.propose(me, vec![Op::Status]);
            }
            if ui
                .button("Reload admins")
                .on_hover_text("Re-reads the admin list from the config file, without a restart.")
                .clicked()
            {
                self.propose(me, vec![Op::ReloadAdmins]);
            }
        });
    }

    fn answers_ui(&self, state: &AdminState, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Answers");
        if let Some(status) = &state.status {
            ui.collapsing("status", |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(status).monospace().small())
                        .selectable(true),
                );
            });
        }
        if state.answers.is_empty() {
            ui.colored_label(theme.text_muted, "Nothing asked yet.");
            return;
        }
        for answer in &state.answers {
            ui.colored_label(
                if answer.refused {
                    theme.destructive
                } else {
                    theme.text_secondary
                },
                &answer.asked,
            );
            ui.add(
                egui::Label::new(egui::RichText::new(&answer.said).monospace().small())
                    .selectable(true),
            );
            ui.separator();
        }
    }

    /// The key in the key box, if it is one. Cleared when it is taken.
    fn take_key(&mut self, me: PubKey) -> Option<PubKey> {
        let typed = self.pane(me).key.trim().to_string();
        match typed.parse::<PubKey>() {
            Ok(key) => {
                self.pane(me).key.clear();
                Some(key)
            }
            Err(_) => None,
        }
    }
}
