//! The shell: the app roster, the global history, and the chrome around them.
//!
//! It owns which apps exist and where you are, and knows nothing about what any
//! of them do. Everything app-specific reaches it as an opaque `Rc<dyn Any>`
//! route token it hands straight back.
//!
//! The single idea worth stating: **`active` is derived from the top of the
//! navigation stack, not stored beside it.** Switching apps therefore *is*
//! navigating, and back and forward cross app boundaries without anything
//! being written to make that work.

use std::collections::HashMap;

use sigil::account::Account;
use sigil::app::{App, AppAction, AppContext};
use sigil::navigator::{AppId, NavEntry, NavRequest, Navigator};
use sigil::{ColorTheme, NavStack, tokens};

/// Wide enough that an app name and its unread count sit on one line. Icons
/// will make this narrower; until there are icons, a wrapped label reads worse
/// than a wide rail.
const RAIL_WIDTH: f32 = 104.0;

pub struct Shell {
    apps: Vec<Box<dyn App>>,
    /// The global history. Its top says both where we are and which app we are
    /// in; see the module note.
    nav: NavStack<NavEntry>,
    /// Which apps have ever been activated. Only these get `update`, so a
    /// never-opened app costs nothing per pass.
    opened: Vec<bool>,
    /// Where keyboard focus was in each app, so switching away and back does
    /// not silently lose it. egui drops focus for any widget it did not draw
    /// last pass, which is every widget in an app you just left.
    focus: HashMap<usize, egui::Id>,
    previous: usize,
    navigator: Navigator,
    chrome_visible: bool,
    /// One identity for the whole application. See `AppContext::account`.
    account: Account,
}

impl Shell {
    pub fn new(apps: Vec<Box<dyn App>>) -> Self {
        assert!(!apps.is_empty(), "a shell with no apps has nothing to show");
        let mut opened = vec![false; apps.len()];
        opened[0] = true;
        Self {
            apps,
            nav: NavStack::new(NavEntry::app_only(AppId(0))),
            opened,
            focus: HashMap::new(),
            previous: 0,
            navigator: Navigator::default(),
            chrome_visible: true,
            account: Account::discover(None),
        }
    }

    /// Start with a particular identity, rather than whatever `~/.sqnr` holds.
    /// Used by tests, which must never reach for the real one.
    pub fn with_account(mut self, account: Account) -> Self {
        self.account = account;
        self
    }

    /// Which app is on screen — read off the top of the history, never stored.
    fn active(&self) -> usize {
        self.nav.top().app.slot().min(self.apps.len() - 1)
    }

    /// Background work for every opened app. Runs while the window is hidden
    /// too, which is what keeps a call alive in the tray.
    pub fn update_all(&mut self, egui_ctx: &egui::Context, hidden: bool) {
        for (i, app) in self.apps.iter_mut().enumerate() {
            if !self.opened[i] {
                continue;
            }
            let mut ctx = AppContext {
                navigator: &mut self.navigator,
                account: &mut self.account,
                hidden,
            };
            app.update(&mut ctx, egui_ctx);
        }
        self.apply_nav();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.restore_focus(ui.ctx());
        self.handle_shell_keys(ui.ctx());

        let theme = ColorTheme::current(ui.ctx());
        // Panels rather than a bare horizontal layout: a panel takes the full
        // height of its parent and reserves its width, which is what makes the
        // rail a rail rather than a box the size of its text.
        if self.chrome_visible {
            egui::Panel::left("sigil_rail")
                .resizable(false)
                .exact_size(RAIL_WIDTH)
                .frame(
                    egui::Frame::NONE
                        .fill(theme.surface_secondary)
                        .inner_margin(egui::Margin::same(tokens::SPACING_SM as i8)),
                )
                .show(ui, |ui| self.rail(ui));
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme.surface_primary)
                    .inner_margin(egui::Margin::same(tokens::SPACING_LG as i8)),
            )
            .show(ui, |ui| self.body(ui));

        self.handle_fallback_keys(ui.ctx());
        self.remember_focus(ui.ctx());
        self.apply_nav();
    }

    /// The app rail: one entry per app, with its unread badge.
    fn rail(&mut self, ui: &mut egui::Ui) {
        let active = self.active();
        ui.vertical_centered_justified(|ui| {
            for i in 0..self.apps.len() {
                let title = self.apps[i].title().to_string();
                let badge = self.apps[i].tab_notifications();
                // The count goes in the label rather than a painted dot: a dot
                // says "something", a number says how much, and the
                // accessibility tree can read one of them out.
                let label = if badge.is_empty() {
                    title
                } else {
                    format!("{title} ({})", badge.count)
                };
                let selected = i == active;
                if ui.selectable_label(selected, label).clicked() && !selected {
                    self.navigator.switch_to(AppId(i));
                }
                ui.add_space(tokens::SPACING_XS);
            }
        });
    }

    /// The active app, drawn through the history entry that names it — so an
    /// app that pushed a route draws *that view*, not its whole self.
    fn body(&mut self, ui: &mut egui::Ui) {
        let active = self.active();
        let entry = self.nav.top().clone();
        let mut ctx = AppContext {
            navigator: &mut self.navigator,
            account: &mut self.account,
            hidden: false,
        };
        let response = self.apps[active].render_nav(&mut ctx, ui, &entry.token);
        match response.action {
            Some(AppAction::ToggleChrome) => self.chrome_visible = !self.chrome_visible,
            Some(AppAction::Present) | Some(AppAction::None) | None => {}
        }
    }

    /// Apply what apps asked for, and free anything that became unreachable.
    fn apply_nav(&mut self) {
        for request in self.navigator.take() {
            let active = self.active();
            let discarded = match request {
                NavRequest::Push(entry) => self.nav.push(entry),
                NavRequest::Replace(entry) => self.nav.replace(entry),
                NavRequest::PushActive(entry) => self.nav.push(entry.tag(AppId(active))),
                NavRequest::ReplaceActive(entry) => self.nav.replace(entry.tag(AppId(active))),
                NavRequest::Back => {
                    self.nav.go_back();
                    continue;
                }
                NavRequest::Forward => {
                    self.nav.go_forward();
                    continue;
                }
            };
            // A discarded route may own a live session. Hand each back to the
            // app that made it, which is the only party that knows how to end
            // whatever it started.
            for entry in discarded {
                let slot = entry.app.slot();
                if let Some(app) = self.apps.get_mut(slot) {
                    let mut ctx = AppContext {
                        navigator: &mut self.navigator,
                        account: &mut self.account,
                        hidden: false,
                    };
                    app.dispose(&mut ctx, &entry.token);
                }
            }
        }
        let active = self.active();
        if let Some(opened) = self.opened.get_mut(active) {
            *opened = true;
        }
    }

    /// Shell keys, consumed *before* apps draw, so no app can swallow them.
    fn handle_shell_keys(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F11)) {
            self.chrome_visible = !self.chrome_visible;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft)) {
            self.nav.go_back();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight)) {
            self.nav.go_forward();
        }
    }

    /// Keys that fire only if nothing else wanted them.
    fn handle_fallback_keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.nav.go_back();
        }
    }

    fn remember_focus(&mut self, ctx: &egui::Context) {
        let active = self.active();
        if let Some(id) = ctx.memory(|m| m.focused()) {
            // Only overwrite on a real focus: a pass where nothing is focused
            // is usually transient, and clobbering the memory with `None` is
            // how the restore silently stops working.
            self.focus.insert(active, id);
        }
        self.previous = active;
    }

    fn restore_focus(&mut self, ctx: &egui::Context) {
        let active = self.active();
        if active == self.previous {
            return;
        }
        if let Some(id) = self.focus.get(&active).copied() {
            ctx.memory_mut(|m| m.request_focus(id));
        }
    }
}
