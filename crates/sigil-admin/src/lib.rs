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
    /// Why what is in the key box is not a key, when a press found it was
    /// not. Cleared by the next press that finds one.
    key_trouble: Option<String>,
}

pub struct AdminApp {
    sessions: HashMap<PubKey, AdminHandle>,
    /// Which exchange each session is acting on, by the roster's name --
    /// `""` for the default -- so a change of choice is seen as one.
    acting: HashMap<PubKey, String>,
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
            acting: HashMap::new(),
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

    /// Which exchange the console for `me` is acting on, by name.
    #[doc(hidden)]
    pub fn acting_on_for_test(&self, me: PubKey) -> Option<String> {
        self.acting.get(&me).cloned()
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
                self.acting.remove(&me);
            }
        }
        for (me, path) in held {
            // **The exchange this identity is looking at**, which is the
            // chat's answer as much as this console's: the control in the
            // title strip writes it for both. A session acting somewhere
            // else is stopped and one started where the person is looking.
            let want = Self::exchange_for(ctx, me);
            if self.sessions.contains_key(&me) {
                if self.acting.get(&me) == want.as_ref() {
                    continue;
                }
                if let Some(mut session) = self.sessions.remove(&me) {
                    session.stop();
                    self.acting.remove(&me);
                }
            }
            let Some((_, unlocked)) = ctx.accounts.unlocked().find(|(k, _)| *k == me) else {
                continue;
            };
            // **The connection this identity's chat session already holds.**
            // Dialling a second is a second handshake, a second socket and a
            // second keep-alive timer for one identity talking to one exchange
            // -- and it is the connection the exchange would fan a call's
            // datagrams to as well. It also gains the reconnection this
            // session has never had, for free: the chat session redials and
            // the slot is rewritten.
            //
            // Taken whether or not it is live yet: a slot exists from the
            // moment a chat session is started, and is filled a handshake
            // later. The session waits for it rather than dialling its own
            // over a second's difference.
            let (reach, name): (sigil_net::Dial, String) = match &want {
                Some(name) => match ctx.connections.of(me, name) {
                    Some(held) => (held.into(), name.clone()),
                    None => continue,
                },
                // Nothing held and nothing chosen: what it is configured to
                // do, which is what the chat would dial for the default.
                None => {
                    let layers =
                        discovery::layers(discovery::nothing_explicit(), &self.config, Some(&path));
                    if !discovery::any_configured(&layers) {
                        continue;
                    }
                    (layers.into(), String::new())
                }
            };
            let wake = egui_ctx.clone();
            self.sessions.insert(
                me,
                session::start(reach, unlocked.signer(), move || wake.request_repaint()),
            );
            self.acting.insert(me, name);
        }
    }

    /// Which of the identity's connected exchanges to act on: the one chosen
    /// in the title strip when it is connected, else the rule every borrower
    /// follows -- the default, or the only one -- and nothing when that rule
    /// declines to choose.
    fn exchange_for(ctx: &AppContext<'_>, me: PubKey) -> Option<String> {
        let held = ctx.connections.names_of(me);
        if let Some(chosen) = ctx.accounts.shown_exchange(me)
            && held.contains(chosen)
        {
            return Some(chosen.clone());
        }
        let names: Vec<&str> = held.iter().map(String::as_str).collect();
        sigil_net::held::to_borrow(&names).map(str::to_string)
    }

    /// The exchanges the active identity holds that are worth listing, with
    /// what to call each: the named ones by name, the default by the domain
    /// it resolves to -- and not at all when it resolves to nothing.
    fn rows_for(&self, ctx: &AppContext<'_>) -> Vec<sigil_ui::ExchangeRow> {
        let path = ctx.accounts.active().path().to_path_buf();
        let layers = discovery::layers(discovery::nothing_explicit(), &self.config, Some(&path));
        let default_label = discovery::any_configured(&layers)
            .then(|| sigil_net::domain_of(&layers).unwrap_or_else(|| "default".to_string()));
        ctx.accounts
            .active_held()
            .exchanges()
            .into_iter()
            .filter_map(|name| {
                let label = if name.is_empty() {
                    default_label.clone()?
                } else {
                    name.clone()
                };
                Some(sigil_ui::ExchangeRow {
                    removable: !name.is_empty(),
                    name,
                    label,
                })
            })
            .collect()
    }

    /// Queue a batch for confirmation. Nothing is signed until it is confirmed.
    fn propose(&mut self, me: PubKey, ops: Vec<Op>) {
        self.pane(me).pending = Some(ops);
    }
}

/// Why `typed` is not a key, in the words of the person who typed it.
fn key_trouble(typed: &str) -> String {
    if typed.is_empty() {
        return "Type or paste a key first.".into();
    }
    if typed.contains('…') || typed.contains("...") {
        return "That is a key shortened for display, not the whole key: copy it from \
                the identity's profile, or from Members, where the whole key is."
            .into();
    }
    if typed.contains('@') {
        return "That is a handle, not a key. The whitelist holds keys; look the handle \
                up and paste the key it names."
            .into();
    }
    format!(
        "That is not a key: a key is 43 or 44 characters of base58, or 64 of hex, and \
         this is {} characters.",
        typed.chars().count()
    )
}

impl App for AdminApp {
    fn update(&mut self, ctx: &mut AppContext<'_>, egui_ctx: &egui::Context) {
        self.reconcile(ctx, egui_ctx);
    }

    /// The same control the chat draws, over the same answer: which of this
    /// identity's exchanges is being looked at. Choosing here moves the
    /// console and the chat together. No "add" -- that is the chat's dialog,
    /// and an exchange is added to talk on before it is administered.
    fn chrome_ui(
        &mut self,
        ctx: &mut AppContext<'_>,
        ui: &mut egui::Ui,
        _token: &std::rc::Rc<dyn std::any::Any>,
    ) {
        let theme = ColorTheme::current(ui.ctx());
        let Some(me) = Self::showing(ctx) else {
            return;
        };
        let rows = self.rows_for(ctx);
        if rows.is_empty() {
            return;
        }
        let selected = self
            .acting
            .get(&me)
            .cloned()
            .or_else(|| ctx.accounts.shown_exchange(me).cloned())
            .unwrap_or_default();
        let shown = rows
            .iter()
            .find(|r| r.name == selected)
            .map(|r| r.label.clone())
            .unwrap_or_else(|| "no exchange".to_string());
        let did = sigil_ui::exchange_control(ui, &theme, &shown, &selected, &rows, false);
        if let Some(name) = did.chosen {
            ctx.accounts.show_exchange(me, Some(name));
        }
        if let Some(name) = did.removed {
            let which = ctx.accounts.active_index();
            ctx.accounts.drop_exchange(which, &name);
            ctx.accounts.show_exchange(me, None);
        }
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
        let acting = self.acting.get(&me).cloned();

        self.header_ui(&state, acting.as_deref(), ui, &theme);
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

        // **The newest answer, where the eye is.** Every answer used to land
        // in a list at the foot of the page, under five sections of
        // controls, so an operation that was applied looked like one that
        // did nothing -- the reply was there, below the fold. The last one
        // is drawn here, above everything, and the list stays at the foot
        // for the ones before it.
        self.latest_answer_ui(&state, ui, &theme);

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

/// How tall the pinned last answer may be before it scrolls inside itself.
///
/// Enough for a short reply whole -- a status, a refusal, a handful of keys
/// -- without a bar, and far short of pushing the console it is pinned above
/// off the screen. The rest of any answer is in Answers, at the foot.
const LAST_ANSWER: f32 = 160.0;

impl AdminApp {
    /// Who we are signing as, and what we are signing at.
    fn header_ui(
        &self,
        state: &AdminState,
        acting: Option<&str>,
        ui: &mut egui::Ui,
        theme: &ColorTheme,
    ) {
        ui.horizontal(|ui| {
            ui.heading("Exchange");
            // Which one, by the name somebody chose it under: the key below
            // is the exchange's answer, this is the person's.
            match acting {
                Some("") => {
                    ui.colored_label(theme.text_secondary, "the default");
                }
                Some(name) => {
                    ui.colored_label(theme.text_secondary, name);
                }
                None => {}
            }
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
        // Since sqexd 0.57.0 the list is on the transport: enabling it closes
        // the exchange to every key not on it, at the door, and closes what
        // is already connected. Administrators and peering exchanges pass
        // regardless; a YubiKey administrator, having no transport key, does
        // not. Said here because "enabled" is a large thing to press.
        ui.colored_label(
            theme.text_secondary,
            "A closed set of transport keys. Enabled, the exchange accepts connections only \
             from keys on it, its administrators and its peers, and closes the rest.",
        );
        ui.horizontal_wrapped(|ui| {
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
        ui.horizontal_wrapped(|ui| {
            // Room kept for the label box and the two buttons that follow.
            let key = Self::box_width(ui, 260.0, 260.0);
            sigil_ui::field(
                ui,
                &mut self.panes.entry(me).or_default().key,
                "key, base58",
                key,
            );
            let label = Self::box_width(ui, 120.0, 140.0);
            sigil_ui::field(
                ui,
                &mut self.panes.entry(me).or_default().label,
                "label",
                label,
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
        self.key_trouble_ui(me, ui, theme);
    }

    /// A box in an operation row, at the width there is.
    ///
    /// # Why not a number
    ///
    /// Every field here asked for a fixed width -- 260 for a key, 120 for a
    /// label, 160 for a name -- chosen against a 900-point window. On a
    /// 360-point pane the row holding two of them plus two buttons was half
    /// again as wide as the pane, so it ran off the right edge; and because
    /// egui grows a ui to whatever is drawn in it, the *prose* above and
    /// below then wrapped to that wider ui and was clipped by the pane
    /// instead. Every explanation on the console ended mid-word.
    ///
    /// `want` is still what it should be where there is room. It is only ever
    /// reduced, so a desktop is what it was.
    ///
    /// Below [`tokens::NARROW_WIDTH`] it is the whole line instead. Sharing a
    /// row was still wrong once it stopped overflowing: the key box came out
    /// ninety points, and a base58 key is forty-four characters. In a
    /// `horizontal_wrapped` a full-width box takes its own line and what
    /// follows wraps under it, which is the form a phone wants anyway.
    fn box_width(ui: &egui::Ui, want: f32, after: f32) -> f32 {
        if ui.available_width() < tokens::NARROW_WIDTH {
            return (ui.available_width() - tokens::SPACING_SM).max(90.0);
        }
        want.min((ui.available_width() - after).max(90.0))
    }

    /// Under a key box: why the last press did nothing, until one does.
    fn key_trouble_ui(&mut self, me: PubKey, ui: &mut egui::Ui, theme: &ColorTheme) {
        if let Some(why) = self.panes.get(&me).and_then(|p| p.key_trouble.clone()) {
            ui.colored_label(theme.destructive, why);
        }
    }

    fn admission_ui(&mut self, me: PubKey, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Admission");
        // The line that decides how this screen should be read.
        ui.colored_label(
            theme.text_secondary,
            "A credential is evidence, not authority: it says which account vouches for a \
             key, and entitles that key to nothing. Admitting one is a decision.",
        );
        ui.horizontal_wrapped(|ui| {
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
        ui.horizontal_wrapped(|ui| {
            let name = Self::box_width(ui, 160.0, 210.0);
            sigil_ui::field(
                ui,
                &mut self.panes.entry(me).or_default().name,
                "name",
                name,
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
        ui.horizontal_wrapped(|ui| {
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
        self.key_trouble_ui(me, ui, theme);
    }

    fn audit_ui(&mut self, me: PubKey, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.heading("Audit and status");
        let _ = theme;
        ui.horizontal_wrapped(|ui| {
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

    /// The exchange's most recent reply, pinned under the header.
    fn latest_answer_ui(&self, state: &AdminState, ui: &mut egui::Ui, theme: &ColorTheme) {
        let Some(answer) = state.answers.first() else {
            return;
        };
        egui::Frame::NONE
            .fill(theme.surface_secondary)
            .corner_radius(tokens::RADIUS_MD)
            .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // **Wrapped.** What was asked is a sentence with an exchange's
                // domain in it -- "whitelist/list at
                // an-exchange-with-a-long-name.example.org" -- and a
                // `horizontal` never wraps, so on a phone this one row took
                // the whole pane out past its edge and the reply under it
                // with it. `set_width` above sets the minimum, not a cap on
                // what children draw.
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(theme.text_muted, egui::RichText::new("Last answer").small());
                    ui.colored_label(
                        if answer.refused {
                            theme.destructive
                        } else {
                            theme.text_secondary
                        },
                        &answer.asked,
                    );
                });
                // **Bounded, and scrolled inside that bound.** This is a
                // *preview*, pinned above the console because a reply below
                // the fold reads as a button that did nothing -- and with no
                // height of its own it did the same thing in the other
                // direction. An audit tail of fifty lines is some 2400
                // points, and it is drawn outside the scroll area that holds
                // the operations: on a phone the Whitelist heading sat at
                // y 2392 of an 804-point screen, so every operation the
                // console offers was three screens down with nothing to
                // scroll. The whole answer is still below, in Answers, which
                // is what this is a preview *of*.
                egui::ScrollArea::vertical()
                    .max_height(LAST_ANSWER)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(&answer.said).monospace().small())
                                .selectable(true),
                        );
                    });
            });
        ui.add_space(tokens::SPACING_SM);
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

    /// The key in the box, if it is one -- and the reason it is not, said
    /// in the pane, if it is not. A press on Add that did nothing and said
    /// nothing was read as the exchange refusing; it was this returning
    /// `None` for a key copied in its shortened form.
    fn take_key(&mut self, me: PubKey) -> Option<PubKey> {
        let typed = self.pane(me).key.trim().to_string();
        match typed.parse::<PubKey>() {
            Ok(key) => {
                let pane = self.pane(me);
                pane.key.clear();
                pane.key_trouble = None;
                Some(key)
            }
            Err(_) => {
                self.pane(me).key_trouble = Some(key_trouble(&typed));
                None
            }
        }
    }
}
