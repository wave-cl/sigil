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
/// Wide enough for one icon and its hit target, and no wider.
///
/// It was 104px, which was right for a column of words and is most of an inch
/// of nothing beside a column of 34px icons.
const RAIL_WIDTH: f32 = 52.0;

/// The opening screen's card. Wide enough for a passphrase somebody actually
/// chose, and narrow enough to read as one thing to do.
const CARD_WIDTH: f32 = 420.0;

/// An identity's name, as somebody would say it: the file's name, with the
/// default one called what it is rather than "identity".
fn name_of(path: &std::path::Path) -> String {
    match path.file_name().and_then(|n| n.to_str()) {
        Some("identity") => "identity (the default)".to_string(),
        Some(name) => name.to_string(),
        None => path.display().to_string(),
    }
}

/// The opening screen: which identity, and its passphrase.
///
/// # Why the shell owns this
///
/// Unlocking used to be each app's own business — chat said "unlock your
/// identity to start chatting" and voice drew a passphrase box — so the first
/// thing anybody met depended on which tab happened to be in front, and
/// neither of them could offer the identities sitting in the same folder.
/// There is one identity in front at a time and one place to choose it, so
/// there is one screen.
#[derive(Default)]
struct Welcome {
    /// **Never persisted, and cleared only on success.** Making somebody
    /// retype a long passphrase because the program threw it away on a typo is
    /// its own small cruelty.
    passphrase: String,
    trouble: Option<String>,
    /// The chosen identity's key, and which file it came from.
    ///
    /// Cached because reading it is a file read, and this screen redraws
    /// whenever the caret blinks. Refreshed when the choice changes, which is
    /// the only thing that can change the answer.
    /// Held as base58 rather than as a key, which is what draws it -- and
    /// saves this crate a dependency on the key type for one field.
    mark: Option<(std::path::PathBuf, String)>,
    /// Whether the box has been given the keyboard once.
    ///
    /// Once, not every pass: asking for focus on every frame takes it back
    /// from anything else on the screen — the identity dropdown could not be
    /// opened, because the box grabbed the keyboard again the instant it was.
    focused: bool,
}

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
    /// The opening screen's state: which identity is chosen, and what has been
    /// typed at it. See [`Shell::welcome`].
    welcome: Welcome,
    /// Whether roster changes are written to disk.
    ///
    /// Off for a shell built with a fixed roster, which is what tests do. A
    /// test that remembered its accounts would overwrite the settings of
    /// whoever ran it, and the damage would show up on their *next* launch,
    /// nowhere near the test that did it.
    remember: bool,
    /// How much of the top of the window the desktop's own chrome sits over.
    ///
    /// sigil's window has a **transparent** title bar with the content drawn
    /// behind it, so that the bar is sigil's colour rather than the system's
    /// grey and carries no title -- somebody looking at their own chat client
    /// knows what it is. The close, minimise and zoom buttons are still the
    /// system's, still where macOS puts them, and still drag the window.
    ///
    /// What they are *not* is out of the way: they sit over the top-left of
    /// the content, which is where the rail and the chats column's own
    /// controls are. So the top of the window is left empty by this much.
    /// Zero everywhere but macOS, where nothing is drawn behind the bar.
    top_inset: f32,
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
            welcome: Welcome::default(),
            remember: true,
            top_inset: 0.0,
        }
    }

    /// How much of the top the window's own chrome sits over. See
    /// [`Shell::top_inset`].
    ///
    /// Set every pass by the host, which is the only thing holding a window
    /// handle to ask. Tests set it directly to check the room is left.
    pub fn set_top_inset(&mut self, points: f32) {
        self.top_inset = points.max(0.0);
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

        // The window's own buttons sit over the content, so the content
        // starts below them. Empty on purpose: this strip is the title bar's
        // drag region, and a widget in it is a widget somebody would have to
        // avoid to move their window. Painted in the application's colour,
        // which is the point of drawing behind the bar at all -- the bar has
        // no colour of its own to be wrong.
        //
        // First, and outside the sealed-identity branch below, so the opening
        // screen is not under the buttons either.
        if self.top_inset > 0.0 {
            egui::Panel::top("sigil_window_chrome")
                .resizable(false)
                .exact_size(self.top_inset)
                .frame(egui::Frame::NONE.fill(theme.surface_primary))
                .show(ui, |_| {});
        }
        // Nothing sealed gets a rail. Every app behind it would be a tab onto
        // an identity that cannot do anything, and offering four of those is
        // offering a choice that does not exist yet.
        if !self.accounts.active().is_unlocked() {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(theme.surface_primary)
                        .inner_margin(egui::Margin::same(tokens::SPACING_LG as i8)),
                )
                .show(ui, |ui| self.welcome(ui, &theme));
            return;
        }
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
                        .inner_margin(egui::Margin::symmetric(
                            tokens::SPACING_XS as i8,
                            tokens::SPACING_SM as i8,
                        )),
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

    /// Choose an identity, and open it.
    ///
    /// Centred and large, because it is the only thing on screen and the only
    /// decision to make. The list is **every identity in `~/.sqnr`**, not only
    /// the ones sigil happens to have been holding: somebody who made a second
    /// identity with `sqnr` should find it here rather than having to know
    /// about a roster file.
    fn welcome(&mut self, ui: &mut egui::Ui, theme: &ColorTheme) {
        let found = Accounts::found();
        let active = self.accounts.active_index();
        let chosen = self.accounts.active().path().to_path_buf();

        ui.vertical_centered(|ui| {
            // Enough to sit off the top edge, and no more: the card grew a
            // mark and an eighty-pixel disc pushed the Unlock button off the
            // bottom of a short window.
            ui.add_space(ui.available_height() * 0.10);
            ui.allocate_ui_with_layout(
                egui::vec2(CARD_WIDTH, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    // **The identity's own mark**, over the middle of the
                    // card: the same identicon it will carry in the corner
                    // once it is open, so the thing about to be unlocked is
                    // recognisable before it is. A sealed identity still names
                    // its key in the clear, which is what makes this possible
                    // without the passphrase.
                    //
                    // sigil's own disc stands in when there is no key to show
                    // — a missing or unreadable file, where a generated mark
                    // would be a picture of nothing.
                    if self.welcome.mark.as_ref().is_none_or(|(p, _)| *p != chosen) {
                        self.welcome.mark = self
                            .accounts
                            .active()
                            .public()
                            .map(|key| (chosen.clone(), key.to_string()));
                    }
                    let key = self.welcome.mark.as_ref().map(|(_, k)| k.clone());
                    ui.vertical_centered(|ui| match &key {
                        Some(key) => {
                            sigil_ui::identicon(ui, key, tokens::AVATAR_XL)
                                .on_hover_text(key.clone());
                            ui.add_space(tokens::SPACING_SM);
                            // **The key, not only the file name.** The list
                            // above names files, and a file name is something
                            // somebody typed; the key is the identity. A mark
                            // is a hint for the eye and two of them can
                            // collide, so this is what actually says which
                            // account is about to be opened.
                            ui.add(
                                egui::Label::new(egui::RichText::new(key).monospace().small())
                                    .wrap()
                                    .selectable(true),
                            );
                        }
                        None => {
                            sigil_ui::mark(ui, tokens::AVATAR_XL);
                        }
                    });
                    ui.add_space(tokens::SPACING_LG);
                    ui.heading("Open an identity");
                    ui.colored_label(
                        theme.text_secondary,
                        "Your key is what identifies you. Everything sigil does is done as \
                         one of these.",
                    );
                    ui.add_space(tokens::SPACING_LG);

                    ui.label("Identity");
                    // A name, not a path: the folder is the same for all of
                    // them and repeating it eight times says nothing.
                    let label = name_of(&chosen);
                    egui::ComboBox::from_id_salt("sigil_welcome_identity")
                        .width(CARD_WIDTH)
                        .height(320.0)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            for path in &found {
                                let selected = *path == chosen;
                                if ui
                                    .selectable_label(selected, name_of(path))
                                    .on_hover_text(path.display().to_string())
                                    .clicked()
                                    && !selected
                                {
                                    self.accounts.use_path(path.clone());
                                    self.welcome.passphrase.clear();
                                    self.welcome.trouble = None;
                                    // A different identity wants a different
                                    // passphrase, so the box is where the
                                    // keyboard should be again.
                                    self.welcome.focused = false;
                                    self.welcome.mark = None;
                                }
                            }
                            if found.is_empty() {
                                ui.colored_label(
                                    theme.text_muted,
                                    "No identities in ~/.sqnr. Make one with `sqnr identity \
                                     new`.",
                                );
                            }
                        });

                    ui.add_space(tokens::SPACING_MD);
                    // What is actually wrong with the file, when something is.
                    // "Missing" and "sealed" want completely different things
                    // from somebody and look identical from a blank box.
                    ui.colored_label(theme.text_secondary, self.accounts.active().describe());
                    ui.add_space(tokens::SPACING_MD);

                    if matches!(self.accounts.active(), Account::Locked { .. }) {
                        ui.label("Passphrase");
                        // The shared field, so its text sits in the middle of
                        // its box like every other one. It had a fixed 8px of
                        // padding in a 40px box, which leaves the words riding
                        // high — the same thing that was wrong in all of them.
                        let field = sigil_ui::password_field(
                            ui,
                            &mut self.welcome.passphrase,
                            "the passphrase that seals this identity",
                            CARD_WIDTH,
                        );
                        // Typing works from the moment the window opens. This
                        // is the only thing on screen and the only thing to do
                        // with it, so making somebody click it first is asking
                        // them to tell the program what it already knows.
                        if !self.welcome.focused {
                            self.welcome.focused = true;
                            field.request_focus();
                        }
                        let entered =
                            field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.add_space(tokens::SPACING_SM);
                        let go = ui
                            .add_sized([CARD_WIDTH, tokens::BUTTON_LG], egui::Button::new("Unlock"))
                            .clicked();
                        if entered || go {
                            let passphrase = std::mem::take(&mut self.welcome.passphrase);
                            if self.accounts.unlock(active, &passphrase) {
                                self.welcome.trouble = None;
                            } else {
                                // Said here, and the box left empty rather
                                // than holding a passphrase that did not work.
                                self.welcome.trouble =
                                    Some("That passphrase did not open it.".into());
                            }
                        }
                    }
                    if let Some(trouble) = &self.welcome.trouble {
                        ui.add_space(tokens::SPACING_SM);
                        ui.colored_label(theme.destructive, trouble);
                    }
                },
            );
        });
    }

    /// The app rail: one icon per app, with its unread badge.
    ///
    /// **No account switcher.** It was pinned to the bottom of this, which
    /// meant identities were chosen in one place and everything else about
    /// them — the key, the exchanges, the profile — read in another. They are
    /// all behind the identity block in the top right now, which is also where
    /// the name being switched away from is shown.
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
