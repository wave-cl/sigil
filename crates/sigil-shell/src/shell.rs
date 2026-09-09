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
    /// Making a new identity, rather than opening one that exists.
    making: bool,
    /// What it will be called, and the passphrase that will seal it. Twice,
    /// because there is nothing to check a mistyped one against: the file is
    /// the only copy of the key and a passphrase nobody knows loses it.
    new_name: String,
    new_passphrase: String,
    new_again: String,
}

/// The file a new identity would be written to, or why the name will not do.
///
/// A free function over plain data: what it decides is worth testing, and the
/// thing it guards against — a name that collides with an identity somebody
/// already has — cannot be arranged inside a rendering test.
///
/// The shape is `sqnr`'s: an identity is `identity` or `identity-<something>`
/// in one directory, and a name with a dot in it is a **sidecar**
/// (`identity.handles`) rather than an identity. So a dot is refused here
/// instead of writing a file that the scan for identities would skip, which
/// would look exactly like the identity never being created.
fn new_identity_path(dir: &std::path::Path, name: &str) -> Result<std::path::PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give it a name, so you can tell it from the others.".into());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Letters, numbers, dashes and underscores — it is a file name.".into());
    }
    let path = dir.join(format!("identity-{name}"));
    if path.exists() {
        // Checked here as well as in `sqnr::identity::generate`, which refuses
        // to overwrite and says so in a sentence about a path. This is the
        // same refusal in the words of the person choosing a name.
        return Err(format!("There is already an identity called {name}."));
    }
    Ok(path)
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
    /// Where identities live, when it is not `~/.sqnr`.
    ///
    /// **Tests must set this.** Making one writes a file, and a test that
    /// wrote into the real folder would leave an identity in somebody's list
    /// for ever -- discovered on their next launch, nowhere near the test that
    /// did it.
    identities: Option<std::path::PathBuf>,
    /// The opening screen, reached again to change identity.
    ///
    /// `Some(i)` while it is up, where `i` is the identity that was on screen
    /// when it was asked for -- so cancelling puts back what was there rather
    /// than whatever the list was left pointing at.
    ///
    /// **Nothing is locked to get here.** Every identity sigil holds stays
    /// open, its session running and its messages arriving; this only changes
    /// which one is drawn. Locking the one being left would drop a live
    /// connection to leave a screen.
    choosing: Option<usize>,
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
            identities: None,
            choosing: None,
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

    /// Keep identities somewhere other than `~/.sqnr`. Tests only.
    pub fn with_identities(mut self, dir: std::path::PathBuf) -> Self {
        self.identities = Some(dir);
        self
    }

    /// The folder new identities are written to.
    fn identity_dir(&self) -> Option<std::path::PathBuf> {
        if let Some(dir) = &self.identities {
            return Some(dir.clone());
        }
        sqnr::identity::default_identity_path()
            .ok()
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
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
                .show(ui, |ui| {
                    // Double-click to fill the screen, and again to go back:
                    // what a title bar has done on every desktop for thirty
                    // years, and this strip is the title bar.
                    //
                    // Only reached when the system did not handle it first --
                    // a click in that region goes to one place, so if egui was
                    // given it, macOS's own zoom was not.
                    let bar = ui.allocate_rect(ui.max_rect(), egui::Sense::click());
                    if bar.double_clicked() {
                        let full = ui.ctx().input(|i| i.viewport().maximized);
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(
                            !full.unwrap_or(false),
                        ));
                    }
                });
        }
        // Nothing sealed gets a rail. Every app behind it would be a tab onto
        // an identity that cannot do anything, and offering four of those is
        // offering a choice that does not exist yet.
        if !self.accounts.active().is_unlocked() || self.choosing.is_some() {
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
    /// Making one, rather than opening one that is already there.
    ///
    /// # Why it is sealed, and why the passphrase is asked for twice
    ///
    /// The file this writes is the **only** copy of the key: nothing else has
    /// it, and nothing can mint it again. A passphrase that was mistyped is
    /// therefore not an inconvenience -- it is an identity nobody will ever
    /// open, including the person who just made it, and they will not find out
    /// until the next time they try. So it is typed twice and the two are
    /// compared, which is the only check that is possible.
    ///
    /// Sealed and not optional, for the same reason `sqnr keygen` encrypts by
    /// default: an unsealed identity file is a private key sitting in a folder
    /// in the clear.
    fn making_ui(&mut self, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.label("Name");
        let name = sigil_ui::field(
            ui,
            &mut self.welcome.new_name,
            "work, phone, the-other-one",
            CARD_WIDTH,
        );
        // The keyboard lands in the first box, the same way it lands in the
        // passphrase box on the way in. Once, not every pass, or nothing else
        // on the screen could ever hold it -- see `Welcome::focused`.
        if !self.welcome.focused {
            self.welcome.focused = true;
            name.request_focus();
        }
        ui.add_space(tokens::SPACING_MD);

        ui.label("Passphrase");
        sigil_ui::password_field(
            ui,
            &mut self.welcome.new_passphrase,
            "something you will not lose",
            CARD_WIDTH,
        );
        ui.add_space(tokens::SPACING_SM);
        sigil_ui::password_field(ui, &mut self.welcome.new_again, "again", CARD_WIDTH);
        ui.add_space(tokens::SPACING_SM);
        ui.colored_label(
            theme.text_secondary,
            "It seals the key on this machine. Nothing can recover it and nothing else \
             holds a copy — a passphrase nobody knows is an identity nobody can open.",
        );
        ui.add_space(tokens::SPACING_MD);

        if ui
            .add_sized([CARD_WIDTH, tokens::BUTTON_LG], egui::Button::new("Create"))
            .clicked()
        {
            self.make_identity();
        }
        ui.add_space(tokens::SPACING_SM);
        if ui
            .add_sized([CARD_WIDTH, tokens::BUTTON_MD], egui::Button::new("Cancel"))
            .clicked()
        {
            // The name and both passphrases go with it. What was typed towards
            // an identity that was never made is not worth keeping, and one of
            // those fields is a passphrase.
            self.welcome = Welcome::default();
        }
        if let Some(trouble) = &self.welcome.trouble {
            ui.add_space(tokens::SPACING_SM);
            ui.colored_label(theme.destructive, trouble);
        }
    }

    /// Write it, hold it, and open it.
    ///
    /// Everything that can be wrong is said in one place and in the words of
    /// the person typing, rather than as whatever the layer underneath calls
    /// it: the identity is not made unless all of it is right.
    fn make_identity(&mut self) {
        let Some(dir) = self.identity_dir() else {
            self.welcome.trouble =
                Some("There is nowhere on this machine to keep an identity.".into());
            return;
        };
        let path = match new_identity_path(&dir, &self.welcome.new_name) {
            Ok(path) => path,
            Err(why) => {
                self.welcome.trouble = Some(why);
                return;
            }
        };
        if self.welcome.new_passphrase.is_empty() {
            self.welcome.trouble = Some("It needs a passphrase to be sealed with.".into());
            return;
        }
        if self.welcome.new_passphrase != self.welcome.new_again {
            // Both cleared, not one: the point of the second box is that
            // neither of them is known to be the one that was meant.
            self.welcome.new_passphrase.clear();
            self.welcome.new_again.clear();
            self.welcome.trouble = Some("Those two passphrases are not the same.".into());
            return;
        }
        if let Err(why) = std::fs::create_dir_all(&dir) {
            self.welcome.trouble = Some(format!("{} cannot be made: {why}", dir.display()));
            return;
        }
        let passphrase = std::mem::take(&mut self.welcome.new_passphrase);
        if let Err(why) = sqnr::identity::generate(&path, Some(&passphrase)) {
            self.welcome.trouble = Some(why);
            return;
        }
        // Held, and open. It was just sealed with a passphrase this screen
        // still has, so asking for it back a second later would be asking
        // somebody to prove they meant what they typed twice already.
        let i = self.accounts.use_path(path);
        if !self.accounts.unlock(i, &passphrase) {
            self.welcome.trouble = Some("It was made, but it will not open.".into());
            return;
        }
        self.welcome = Welcome::default();
        self.choosing = None;
        // Not saved here. Every path that changes the roster bumps its
        // generation and `reconcile_accounts` remembers it, which is stated
        // there as the reason no call site does its own -- and a second place
        // that writes the settings file is a second place that can be wrong
        // about them.
    }

    fn welcome(&mut self, ui: &mut egui::Ui, theme: &ColorTheme) {
        let found = Accounts::found();
        let active = self.accounts.active_index();
        let chosen = self.accounts.active().path().to_path_buf();

        ui.vertical_centered(|ui| {
            // Enough to sit off the top edge, and no more: the card grew a
            // mark and an eighty-pixel disc pushed the Unlock button off the
            // bottom of a short window. It came down again when the window
            // grew a strip of its own at the top for the system's buttons --
            // two gaps stacked read as one large one, and the card sat low.
            ui.add_space(ui.available_height() * 0.05);
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
                    // The same screen either way, and it says which errand it
                    // is on: arriving, or coming back to be somebody else.
                    ui.heading(if self.welcome.making {
                        "New identity"
                    } else if self.choosing.is_some() {
                        "Switch identity"
                    } else {
                        "Open an identity"
                    });
                    ui.colored_label(
                        theme.text_secondary,
                        "Your key is what identifies you. Everything sigil does is done as \
                         one of these.",
                    );
                    ui.add_space(tokens::SPACING_LG);

                    if self.welcome.making {
                        self.making_ui(ui, theme);
                        return;
                    }

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
                                // Opened, so this screen has done its errand.
                                self.choosing = None;
                            } else {
                                // Said here, and the box left empty rather
                                // than holding a passphrase that did not work.
                                self.welcome.trouble =
                                    Some("That passphrase did not open it.".into());
                            }
                        }
                    }
                    // Already open -- which is the ordinary case when
                    // switching, since every identity sigil holds stays live.
                    // There is nothing to unlock, so the button says the only
                    // thing left to do.
                    if self.choosing.is_some() && self.accounts.active().is_unlocked() {
                        if ui
                            .add_sized([CARD_WIDTH, tokens::BUTTON_LG], egui::Button::new("Open"))
                            .clicked()
                        {
                            self.choosing = None;
                        }
                        ui.add_space(tokens::SPACING_SM);
                    }
                    // A way back that changes nothing. Somebody who opened
                    // this to look at the list and then thought better of it
                    // would otherwise have to unlock their way out of it.
                    if let Some(previous) = self.choosing
                        && ui
                            .add_sized([CARD_WIDTH, tokens::BUTTON_MD], egui::Button::new("Cancel"))
                            .clicked()
                    {
                        self.accounts.switch_to(previous);
                        self.choosing = None;
                    }
                    // **Under everything, and always there.** Somebody with
                    // no identity at all meets this screen with nothing on it
                    // they can do -- the combo lists a folder that is empty
                    // and the passphrase box has nothing to open. That is also
                    // the first thing that ever happens to anybody.
                    ui.add_space(tokens::SPACING_MD);
                    if ui
                        .add_sized(
                            [CARD_WIDTH, tokens::BUTTON_MD],
                            egui::Button::new("Create a new identity"),
                        )
                        .clicked()
                    {
                        self.welcome.making = true;
                        self.welcome.trouble = None;
                        // The keyboard moves to the first box of the form.
                        self.welcome.focused = false;
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
            // Back to the opening screen. Where it came from is remembered
            // here and not there: the screen changes which identity is active
            // as somebody looks through the list, so by the time they cancel
            // it no longer knows what they started on.
            Some(AppAction::ChooseIdentity) => {
                self.choosing = Some(self.accounts.active_index());
                self.welcome = Welcome::default();
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

#[cfg(test)]
mod naming_tests {
    use super::new_identity_path;

    /// The shape `sqnr` scans for, and no other.
    #[test]
    fn a_name_becomes_an_identity_file_beside_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let path = new_identity_path(dir.path(), "work").expect("a path");
        assert_eq!(path.file_name().unwrap(), "identity-work");
        assert_eq!(path.parent().unwrap(), dir.path());
        // Trimmed, because a trailing space in a file name is a thing nobody
        // means and nothing shows.
        assert_eq!(
            new_identity_path(dir.path(), "  work  ").unwrap(),
            path,
            "the name was taken literally, spaces and all"
        );
    }

    /// **A dot makes a sidecar, not an identity.**
    ///
    /// `identity.handles` is the SIP-38 hints file beside `identity`, so the
    /// scan skips any name with a dot in it. Writing `identity-my.key` would
    /// therefore create a file that never appears in the list — an identity
    /// that was made, and cannot be found, and said nothing.
    #[test]
    fn a_name_that_would_not_be_found_again_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["my.key", "../elsewhere", "with/slash", "two words", ""] {
            assert!(
                new_identity_path(dir.path(), name).is_err(),
                "{name:?} was accepted"
            );
        }
    }

    /// An identity that exists is never written over. It is somebody's only
    /// copy of a key, and a second one under the same name is not a name
    /// collision -- it is the first key gone.
    #[test]
    fn a_name_already_taken_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("identity-work"), "not really a key").unwrap();
        let why = new_identity_path(dir.path(), "work").expect_err("refused");
        assert!(why.contains("work"), "which name, though: {why}");
    }
}
