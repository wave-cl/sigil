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
use sigil::accounts::Accounts;
use sigil::app::Notify;
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
    /// Every identity sigil is holding. See [`sigil::accounts`].
    accounts: Accounts,
    /// The roster generation the apps were last reconciled against.
    ///
    /// Compared rather than diffed: the shell cannot know what any app keeps
    /// per identity, so it says "something moved" and lets each app bring its
    /// own state into line.
    seen_generation: u64,
    /// What the desktop can do, and somewhere to say things out loud. Built on
    /// the main thread by the shell's owner, because the tray insists on it.
    platform: Box<dyn Notify>,
    tray: Option<sigil_platform::Tray>,
    /// The badge last given to the tray, so it is only set when it changes.
    shown_unread: u32,
    /// Whether roster changes are written to disk.
    ///
    /// Off for a shell built with a fixed roster, which is what tests do. A
    /// test that remembered its accounts would overwrite the settings of
    /// whoever ran it, and the damage would show up on their *next* launch,
    /// nowhere near the test that did it.
    remember: bool,
}

impl Shell {
    /// Build the shell.
    ///
    /// The desktop is a **parameter, not a builder step**. It used to be
    /// `with_platform`, and `main` forgot to call it — so the real binary ran
    /// with no notifier and no tray, and a ring was drawn but never announced.
    /// Nothing caught it: the tests pass their own notifier, and a missing one
    /// is silence, which is what a working notifier looks like from inside a
    /// test. Making it an argument makes forgetting it a compile error.
    ///
    /// `None` is for tests and headless rendering, which have no tray and no
    /// notification daemon and should not pretend otherwise.
    pub fn new(apps: Vec<Box<dyn App>>, platform: Option<sigil_platform::Platform>) -> Self {
        assert!(!apps.is_empty(), "a shell with no apps has nothing to show");
        let mut opened = vec![false; apps.len()];
        opened[0] = true;
        let (notify, tray): (Box<dyn Notify>, Option<sigil_platform::Tray>) = match platform {
            Some(sigil_platform::Platform { notifier, tray, .. }) => {
                (Box::new(notifier), Some(tray))
            }
            None => (Box::new(sigil::Silent), None),
        };
        Self {
            apps,
            nav: NavStack::new(NavEntry::app_only(AppId(0))),
            opened,
            focus: HashMap::new(),
            previous: 0,
            navigator: Navigator::default(),
            chrome_visible: true,
            accounts: Accounts::load(),
            seen_generation: 0,
            platform: notify,
            tray,
            shown_unread: 0,
            remember: true,
        }
    }

    /// Start with a particular identity, rather than whatever `~/.sqnr` holds.
    /// Used by tests, which must never reach for the real one.
    pub fn with_account(self, account: Account) -> Self {
        self.with_accounts(Accounts::of(vec![account]))
    }

    /// Start with a whole roster. Used by tests that switch between them.
    pub fn with_accounts(mut self, accounts: Accounts) -> Self {
        self.seen_generation = accounts.generation();
        self.accounts = accounts;
        self.remember = false;
        self
    }

    /// The roster, for the shell's own switcher and for tests.
    pub fn accounts(&self) -> &Accounts {
        &self.accounts
    }

    /// Which app is on screen — read off the top of the history, never stored.
    fn active(&self) -> usize {
        self.nav.top().app.slot().min(self.apps.len() - 1)
    }

    /// Background work for every opened app. Runs while the window is hidden
    /// too, which is what keeps a call alive in the tray.
    pub fn update_all(&mut self, egui_ctx: &egui::Context, hidden: bool) {
        self.reconcile_accounts();
        for (i, app) in self.apps.iter_mut().enumerate() {
            if !self.opened[i] {
                continue;
            }
            let mut ctx = AppContext {
                navigator: &mut self.navigator,
                accounts: &mut self.accounts,
                hidden,
                notify: self.platform.as_ref(),
            };
            app.update(&mut ctx, egui_ctx);
        }
        self.badge_tray();
        self.apply_nav();
    }

    /// Tell every app the roster moved, once per change.
    ///
    /// **Every app, not just the opened ones.** An app that has never been
    /// looked at can still hold a session — chat does, so that messages arrive
    /// before you first click on it — and one left running as a discarded
    /// identity is exactly the failure this exists to prevent.
    fn reconcile_accounts(&mut self) {
        let generation = self.accounts.generation();
        if generation == self.seen_generation {
            return;
        }
        self.seen_generation = generation;
        // Remembered here rather than at each call site: every path that
        // changes the roster goes through the generation, and one that forgot
        // to save would lose an account silently at the next launch.
        if self.remember {
            self.accounts.save();
        }
        for app in self.apps.iter_mut() {
            let mut ctx = AppContext {
                navigator: &mut self.navigator,
                accounts: &mut self.accounts,
                hidden: true,
                notify: self.platform.as_ref(),
            };
            app.accounts_changed(&mut ctx);
        }
    }

    /// Keep the tray's tooltip current, and only when it changes: setting it
    /// every pass would be a D-Bus round trip fifty times a second.
    fn badge_tray(&mut self) {
        let Some(tray) = &self.tray else { return };
        let unread: u32 = self.apps.iter().map(|a| a.tab_notifications().count).sum();
        if unread != self.shown_unread {
            tray.set_unread(unread);
            self.shown_unread = unread;
        }
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

    /// The app rail: one icon per app, with its unread badge.
    ///
    /// Icons, because a rail is narrow by definition and a column of words is
    /// a column of labels. Each still **says its name** — to the accessibility
    /// tree and on hover — since an icon alone is a convention somebody has to
    /// already know.
    fn rail(&mut self, ui: &mut egui::Ui) {
        let active = self.active();
        ui.vertical_centered(|ui| {
            for i in 0..self.apps.len() {
                let title = self.apps[i].title().to_string();
                let badge = self.apps[i].tab_notifications();
                let selected = i == active;
                let theme = ColorTheme::current(ui.ctx());
                // The count stays a **number**, beside the icon rather than
                // inside it: a dot says "something" and a number says how
                // much, and only one of them can be read out.
                let said = if badge.is_empty() {
                    title.clone()
                } else {
                    format!("{title} ({})", badge.count)
                };
                let response = sigil::icon::icon_button_as_named(
                    ui,
                    self.apps[i].icon(),
                    &said,
                    selected.then_some(theme.accent),
                    selected,
                );
                if response.clicked() && !selected {
                    self.navigator.switch_to(AppId(i));
                }
                if !badge.is_empty() {
                    ui.colored_label(
                        theme.accent,
                        egui::RichText::new(badge.count.to_string()).small(),
                    );
                }
                ui.add_space(tokens::SPACING_XS);
            }
        });
        // Pinned to the bottom, the way an account switcher is everywhere
        // else. Only when there is a choice to make: a switcher over one
        // account is a control that cannot do anything.
        if self.accounts.len() > 1 {
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                self.switcher(ui);
            });
        }
    }

    /// Which identity is on screen, and how to look at another.
    ///
    /// Every account here is **live** whether or not it is the one selected —
    /// see [`sigil::accounts`]. Selecting one changes what is drawn and stops
    /// nothing, so nothing here needs a warning about what will be lost.
    fn switcher(&mut self, ui: &mut egui::Ui) {
        let theme = ColorTheme::current(ui.ctx());
        let active = self.accounts.active_index();
        for i in (0..self.accounts.len()).rev() {
            let label = self.accounts.label(i);
            let open = self.accounts.get(i).is_some_and(|a| a.is_unlocked());
            let selected = i == active;
            // A sealed account reads differently from an open one, because
            // selecting it gets a passphrase field rather than a conversation.
            let text = if open {
                egui::RichText::new(label)
            } else {
                egui::RichText::new(format!("{label} (locked)")).color(theme.text_muted)
            };
            let response = ui.selectable_label(selected, text);
            // The full key on hover. A name -- even an abbreviated key -- is an
            // assertion; the key is the thing that identifies somebody (SIP-21),
            // so it stays reachable from wherever the short form is shown.
            if let Some(account) = self.accounts.get(i)
                && let Some(unlocked) = account.unlocked()
            {
                response.clone().on_hover_text(unlocked.me().to_string());
            }
            if response.clicked() && !selected {
                self.accounts.switch_to(i);
            }
        }
        ui.add_space(tokens::SPACING_XS);
        ui.separator();
    }

    /// The active app, drawn through the history entry that names it — so an
    /// app that pushed a route draws *that view*, not its whole self.
    fn body(&mut self, ui: &mut egui::Ui) {
        let active = self.active();
        let entry = self.nav.top().clone();
        let mut ctx = AppContext {
            navigator: &mut self.navigator,
            accounts: &mut self.accounts,
            hidden: false,
            // The real notifier, not `Silent`. It used to be `Silent` here, so
            // an app could only ever say something out loud from `update` --
            // and a view that had something worth announcing found a notifier
            // that reported success and posted nothing.
            notify: self.platform.as_ref(),
        };
        let response = self.apps[active].render_nav(&mut ctx, ui, &entry.token);
        match response.action {
            Some(AppAction::ToggleChrome) => self.chrome_visible = !self.chrome_visible,
            // Come to the front. Asked for by a ringing call, and previously
            // dropped on the floor here: the app raised its hand every pass and
            // the window stayed wherever it was, behind whatever was in front.
            //
            // Both commands, in this order. A window closed to the tray is
            // hidden as well as unfocused, and focusing something invisible
            // does nothing on any of the three desktops sigil targets.
            Some(AppAction::Present) => {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Some(AppAction::None) | None => {}
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
                        accounts: &mut self.accounts,
                        hidden: false,
                        notify: &sigil::Silent,
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
