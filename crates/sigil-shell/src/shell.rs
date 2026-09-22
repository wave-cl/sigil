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
use sigil::{ColorTheme, Form, Insets, NavStack, tokens};

// The rail's width is `tokens::RAIL_WIDTH`, chosen by `Form::rail_width`: it
// was 104px, which was right for a column of words and is most of an inch of
// nothing beside a column of 34px icons.

/// How long without a keypress or a click before this machine is *away*.
pub const AWAY_AFTER: f64 = 300.0;

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
///
/// **No name is the first one.** Somebody making their first identity has
/// nothing to tell it apart from, and `sqnr` calls that one `identity`. A
/// name is asked for only once that exists -- which is the moment it starts
/// meaning something.
fn new_identity_path(dir: &std::path::Path, name: &str) -> Result<std::path::PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        let path = dir.join("identity");
        if path.exists() {
            return Err(
                "There is already an identity here. Give this one a name, so you can tell them apart."
                    .into(),
            );
        }
        return Ok(path);
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
    /// Every exchange connection sigil is holding, so a call or the console
    /// uses the one the chat session already has rather than dialling its own.
    /// See [`sigil_net::Connections`].
    connections: sigil_net::Connections,
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
    /// The count on sigil's own icon in the Dock or the launcher.
    badge: Option<sigil_platform::Badge>,
    /// The badge last given to the tray, so it is only set when it changes.
    shown_unread: u32,
    /// Tray actions handed in by a test, in place of a desktop's.
    tray_actions: Vec<sigil_platform::tray::TrayAction>,
    /// Do-not-disturb as last shown on the tray and the icon, so they are
    /// only told when it changes.
    shown_quiet: bool,
    /// Quit was chosen: the next close of the window is a real one.
    pub quitting: bool,
    /// The window is closed to the tray: hidden on a close request while
    /// there is a tray to come back from, cleared when the window is
    /// brought back; while set, every app is told it is not in front.
    hidden: bool,
    /// When sigil's own window last saw a keypress or the pointer move, on
    /// egui's clock. What decides *away* where the desktop cannot say.
    last_input: f64,
    /// Nobody at this machine for [`AWAY_AFTER`]. Told to every app each
    /// pass; the chat app tells the exchange.
    away: bool,
    /// Whether to ask the desktop how long since the last input. Off in
    /// tests, which run on a desktop somebody is typing at.
    ask_desktop_idle: bool,
    /// A tray in all but fact, for tests of closing to it.
    hideable: bool,
    /// The `sigil://` link being asked about, if any.
    ///
    /// One at a time: two questions about two rooms, stacked, is two rooms
    /// joined by whoever presses through them. The rest wait in
    /// `sigil_platform::deeplink`'s queue until this one is answered.
    offered: Option<sigil::Link>,
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
    insets: Insets,
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
        let mut opened: Vec<bool> = apps.iter().map(|app| app.runs_unopened()).collect();
        opened[0] = true;
        let (notify, tray, badge): (
            Box<dyn Notify>,
            Option<sigil_platform::Tray>,
            Option<sigil_platform::Badge>,
        ) = match platform {
            Some(sigil_platform::Platform {
                notifier,
                tray,
                badge,
                ..
            }) => (Box::new(notifier), Some(tray), Some(badge)),
            None => (Box::new(sigil::Silent), None, None),
        };
        Self {
            apps,
            nav: NavStack::new(NavEntry::app_only(AppId(0))),
            opened,
            focus: HashMap::new(),
            previous: 0,
            connections: sigil_net::Connections::new(),
            navigator: Navigator::default(),
            chrome_visible: true,
            accounts: Accounts::load(),
            seen_generation: 0,
            platform: notify,
            tray,
            badge,
            shown_unread: 0,
            tray_actions: Vec::new(),
            shown_quiet: false,
            quitting: false,
            hidden: false,
            last_input: 0.0,
            away: false,
            ask_desktop_idle: true,
            hideable: false,
            offered: None,
            welcome: Welcome::default(),
            remember: true,
            identities: None,
            choosing: None,
            insets: Insets::NONE,
        }
    }

    /// How much of the top the window's own chrome sits over.
    ///
    /// Set every pass by the host, which is the only thing holding a window
    /// handle to ask. Tests set it directly to check the room is left. The
    /// desktop's form of [`set_insets`](Self::set_insets): only the top.
    pub fn set_top_inset(&mut self, points: f32) {
        self.insets = Insets::top(points);
    }

    /// What the system draws over the surface, all four sides: a phone's
    /// status bar and gesture bar, its keyboard while it is up, a cutout.
    /// Set every pass by the host; the shell keeps everything clear of them.
    pub fn set_insets(&mut self, insets: Insets) {
        self.insets = insets.clamped();
    }

    /// The insets, where anything drawn outside the shell's panels can ask
    /// -- a popup, which is a layer of its own. Installed every pass, since
    /// the keyboard rising changes them.
    fn publish_insets(&self, ctx: &egui::Context) {
        Insets::install(ctx, self.insets);
    }

    /// Start with a particular identity, rather than whatever `~/.sqnr` holds.
    /// Used by tests, which must never reach for the real one.
    pub fn with_account(self, account: Account) -> Self {
        self.with_accounts(Accounts::of(vec![account]))
    }

    /// Start with a whole roster. Used by tests that switch between them.
    pub fn with_accounts(self, accounts: Accounts) -> Self {
        self.with_roster(accounts, false)
    }

    /// Start with a roster the host built, and say whether changes to it
    /// are written back to `accounts.json` as the shell's own roster's are.
    ///
    /// A test passes `false`: it must never write the real file. A host that
    /// opened the identity itself -- the phone, whose key store holds the
    /// passphrase -- passes `true`, or every exchange added is forgotten at
    /// the next launch, which is how sigil-android lost trunk.exchange on
    /// its first day.
    pub fn with_roster(mut self, accounts: Accounts, remember: bool) -> Self {
        self.seen_generation = accounts.generation();
        self.accounts = accounts;
        self.remember = remember;
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
    ///
    /// `unfocused` is the window not being in front — see
    /// [`AppContext::unfocused`], which says what it is not.
    pub fn update_all(&mut self, egui_ctx: &egui::Context, unfocused: bool) {
        self.reconcile_accounts();
        // **Closing the window puts sigil in the tray**, where there is one:
        // a telephone that hangs up when its window is closed is not one.
        // Quit, from the tray's menu, is how it ends; without a tray the
        // close is the quit, as it always was.
        if egui_ctx.input(|i| i.viewport().close_requested()) && self.can_hide() && !self.quitting {
            egui_ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            egui_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.hidden = true;
        }
        // Closed to the tray is not in front either.
        let unfocused = unfocused || self.hidden;
        self.notice_input(egui_ctx, unfocused);
        let mut asked = Vec::new();
        for (i, app) in self.apps.iter_mut().enumerate() {
            if !self.opened[i] {
                continue;
            }
            let mut ctx = AppContext {
                navigator: &mut self.navigator,
                accounts: &mut self.accounts,
                unfocused,
                away: self.away,
                notify: self.platform.as_ref(),
                connections: &self.connections,
            };
            app.update(&mut ctx, egui_ctx);
            asked.extend(app.asked());
        }
        for action in asked {
            self.act(action, egui_ctx);
        }
        self.open_pressed(egui_ctx);
        self.badge();
        self.remember_quiet();
        self.tray_actions(egui_ctx);
        self.apply_nav();
    }

    /// Whether anybody is here, decided once for every app.
    ///
    /// **The desktop's word where it has one.** macOS says how long since
    /// the last keypress or click anywhere; somebody typing in another
    /// program is not away, and only the desktop knows that. Where it
    /// cannot say, sigil's own window is the witness: a pass that carried
    /// input is somebody here, and a window out of front with no input for
    /// as long is somebody gone.
    ///
    /// The flip to away happens on a pass, and nothing prompts a pass while
    /// nobody is here -- so one is asked for when the time comes, once,
    /// rather than the window painting itself every second to find out.
    fn notice_input(&mut self, egui_ctx: &egui::Context, unfocused: bool) {
        let now = egui_ctx.input(|i| i.time);
        let touched = egui_ctx.input(|i| {
            i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key { .. }
                        | egui::Event::Text(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::PointerMoved(_)
                        | egui::Event::MouseWheel { .. }
                        | egui::Event::Touch { .. }
                )
            })
        });
        if touched {
            self.last_input = now;
        }
        let desktop = if self.ask_desktop_idle {
            sigil_platform::idle::seconds_since_input()
        } else {
            None
        };
        let idle = match desktop {
            Some(secs) => secs,
            // No desktop to ask: what this window saw. In front with the
            // pointer resting is not away; out of front for as long is.
            None => {
                if unfocused || now - self.last_input >= AWAY_AFTER {
                    now - self.last_input
                } else {
                    0.0
                }
            }
        };
        let away = idle >= AWAY_AFTER;
        if away != self.away {
            self.away = away;
        }
        if !away {
            egui_ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                (AWAY_AFTER - idle).max(1.0),
            ));
        }
    }

    /// Whether nobody has been here for [`AWAY_AFTER`].
    pub fn away(&self) -> bool {
        self.away
    }

    /// Decide away from this window's own input alone, as a desktop that
    /// cannot say would have it -- for a test, which runs on one somebody
    /// is typing at.
    pub fn watch_own_input_only_for_test(&mut self) {
        self.ask_desktop_idle = false;
    }

    /// What was done at the tray since last pass: the window brought up,
    /// do-not-disturb flipped, or quitting.
    fn tray_actions(&mut self, egui_ctx: &egui::Context) {
        use sigil_platform::tray::TrayAction;
        let mut actions = std::mem::take(&mut self.tray_actions);
        if let Some(tray) = &self.tray {
            actions.extend(tray.events());
        }
        for action in actions {
            match action {
                TrayAction::Open => self.present(egui_ctx),
                TrayAction::QuietToggled => {
                    let dnd = !self.accounts.quiet.dnd;
                    self.accounts.quiet.set_dnd(dnd);
                }
                TrayAction::Quit => {
                    self.quitting = true;
                    egui_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// Notifications pressed since last pass: the window comes up, and the
    /// app the notification was about shows it.
    fn open_pressed(&mut self, egui_ctx: &egui::Context) {
        for target in self.platform.pressed() {
            self.present(egui_ctx);
            for (i, app) in self.apps.iter_mut().enumerate() {
                let mut ctx = AppContext {
                    navigator: &mut self.navigator,
                    accounts: &mut self.accounts,
                    unfocused: false,
                    away: self.away,
                    notify: self.platform.as_ref(),
                    connections: &self.connections,
                };
                if app.open(&mut ctx, &target) {
                    self.opened[i] = true;
                    self.navigator.switch_to(AppId(i));
                    break;
                }
            }
        }
    }

    /// A `sigil://` link offered since last pass: the window comes up and
    /// the question is asked.
    ///
    /// **One at a time.** Whatever hands these over is outside sigil, and
    /// two questions stacked on each other is two rooms joined by somebody
    /// pressing through them. The rest stay in the queue.
    fn take_a_link(&mut self, egui_ctx: &egui::Context) {
        if self.offered.is_some() {
            return;
        }
        if let Some(link) = sigil_platform::deeplink::offered().into_iter().next() {
            self.present(egui_ctx);
            self.offered = Some(link);
        }
    }

    /// Whether a link is waiting to be asked about, for a test that wants to
    /// know without reading the screen.
    #[doc(hidden)]
    pub fn asking_about_a_link(&self) -> bool {
        self.offered.is_some()
    }

    /// The question a link asks, and the two answers.
    ///
    /// A link is a thing somebody else wrote and put where you would press
    /// it, so nothing acts until this is answered -- see
    /// `sigil::deeplink`. The wording is the link's own
    /// (`deeplink::confirmation`), which says what joining a room cannot
    /// take back.
    fn link_ui(&mut self, egui_ctx: &egui::Context) {
        // Drained here rather than beside the notifications, because this is
        // the pass that can draw the question: a link taken off the queue on
        // a pass that does not ask about it is a link nobody is told about
        // until something else happens.
        self.take_a_link(egui_ctx);
        let Some(link) = self.offered.clone() else {
            return;
        };
        let t = sigil::ColorTheme::current(egui_ctx);
        let mut answered = None;
        let response = egui::Modal::new(egui::Id::new("sigil-link"))
            .frame(
                egui::Frame::NONE
                    .fill(t.surface_primary)
                    .corner_radius(tokens::RADIUS_LG)
                    .inner_margin(egui::Margin::same(tokens::SPACING_LG as i8)),
            )
            .show(egui_ctx, |ui| {
                // As wide as a dialog likes, or as wide as the screen has
                // once a margin is kept: a phone is narrower than a dialog.
                let screen = ui.ctx().content_rect().width();
                ui.set_width(360.0f32.min(screen - 2.0 * tokens::SPACING_XL).max(200.0));
                ui.heading("A sigil link");
                ui.add_space(tokens::SPACING_SM);
                ui.label(sigil_platform::deeplink::confirmation(&link));
                ui.add_space(tokens::SPACING_MD);
                ui.horizontal(|ui| {
                    if ui.button("Yes").clicked() {
                        answered = Some(true);
                    }
                    if ui.button("No").clicked() {
                        answered = Some(false);
                    }
                });
            });
        // Pressing away from it is the same as No: a question nobody
        // answered has not been said yes to.
        if response.should_close() && answered.is_none() {
            answered = Some(false);
        }
        match answered {
            None => {}
            Some(false) => self.offered = None,
            Some(true) => {
                self.offered = None;
                let mut taken = false;
                for i in 0..self.apps.len() {
                    let mut ctx = AppContext {
                        navigator: &mut self.navigator,
                        accounts: &mut self.accounts,
                        unfocused: false,
                        away: self.away,
                        notify: self.platform.as_ref(),
                        connections: &self.connections,
                    };
                    if self.apps[i].follow(&mut ctx, &link) {
                        self.opened[i] = true;
                        self.navigator.switch_to(AppId(i));
                        taken = true;
                        break;
                    }
                }
                // **Said out loud when nothing took it.** A yes that does
                // nothing looks exactly like a yes that worked, and the
                // reason is usually that the app it belongs to is not in
                // this build.
                if !taken {
                    tracing::warn!(?link, "no app took the link");
                }
            }
        }
    }

    /// What the window is asked to do about something that happened while
    /// nobody was pressing anything: from a tray action, a notification, or
    /// an app's background work. The things somebody *did* press are
    /// answered in `ui`, which has the interface to answer with.
    fn act(&mut self, action: AppAction, egui_ctx: &egui::Context) {
        match action {
            AppAction::Present => self.present(egui_ctx),
            // Not while in front: the Dock bouncing under somebody who is
            // looking at the window is a twitch, not a signal.
            AppAction::Attention(how) => {
                let kind = match how {
                    sigil::Attention::Informational => egui::UserAttentionType::Informational,
                    sigil::Attention::Critical => egui::UserAttentionType::Critical,
                };
                egui_ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(kind));
            }
            // The same as Quit from the tray: the next close is a close.
            AppAction::Quit => {
                self.quitting = true;
                egui_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }

    /// Bring the window forward. Both commands, in this order: a window
    /// closed to the tray is hidden as well as unfocused, and focusing
    /// something invisible does nothing on any of the three desktops sigil
    /// targets.
    pub fn present(&mut self, egui_ctx: &egui::Context) {
        self.hidden = false;
        egui_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        egui_ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// Whether the window can be closed to the tray rather than quit: there
    /// is a tray to come back from.
    pub fn can_hide(&self) -> bool {
        self.hideable || self.tray.as_ref().is_some_and(|t| t.support().is_yes())
    }

    /// Behave as if a tray were up, without one. Tests only.
    #[doc(hidden)]
    pub fn pretend_tray_for_test(&mut self) {
        self.hideable = true;
    }

    /// Whether the window is closed to the tray.
    pub fn hidden(&self) -> bool {
        self.hidden
    }

    /// Somewhere else to say things: a recorder, in a test.
    pub fn with_notify(mut self, notify: Box<dyn Notify>) -> Self {
        self.platform = notify;
        self
    }

    /// Hand the shell what a tray would have reported. Tests only.
    #[doc(hidden)]
    pub fn tray_actions_for_test(&mut self, actions: Vec<sigil_platform::tray::TrayAction>) {
        self.tray_actions.extend(actions);
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
                unfocused: true,
                away: self.away,
                notify: self.platform.as_ref(),
                connections: &self.connections,
            };
            app.accounts_changed(&mut ctx);
        }
    }

    /// Keep the count on the tray and on the application's own icon
    /// current, and only when it changes: setting it every pass would be a
    /// D-Bus round trip fifty times a second.
    fn badge(&mut self) {
        let unread: u32 = self.apps.iter().map(|a| a.tab_notifications().count).sum();
        let quiet = self.accounts.quiet.dnd;
        if unread == self.shown_unread && quiet == self.shown_quiet {
            return;
        }
        self.shown_unread = unread;
        self.shown_quiet = quiet;
        if let Some(tray) = &mut self.tray {
            tray.set_unread(unread, quiet);
            tray.set_quiet(quiet);
        }
        if let Some(badge) = &mut self.badge {
            badge.set_count(unread);
        }
    }

    /// Write what is not to be said out loud, when it changed. Like the
    /// roster, and for the same reason it is here: every path that changes
    /// it -- the tray, the Desktop pane, a conversation's own control --
    /// goes through this pass, and one that forgot to write would lose a
    /// mute at the next launch.
    fn remember_quiet(&mut self) {
        if self.accounts.quiet.take_changed() && self.remember {
            self.accounts.quiet.save();
        }
        if self.accounts.prefs.take_changed() && self.remember {
            self.accounts.prefs.save();
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.publish_insets(ui.ctx());
        self.restore_focus(ui.ctx());
        self.handle_shell_keys(ui.ctx());

        let theme = ColorTheme::current(ui.ctx());

        // The window's own buttons sit over the content, so the content
        // starts below them. This strip is the title bar's drag region, and
        // almost empty on purpose: a widget in it is a widget somebody has to
        // avoid to move their window. Painted in the application's colour,
        // which is the point of drawing behind the bar at all -- the bar has
        // no colour of its own to be wrong.
        //
        // **Almost.** The app on screen may put one small control at the far
        // right -- see `App::chrome_ui` -- which is where a title bar has
        // always had room, and where the chat app says which exchange an
        // identity is looking at. So the strip exists everywhere, one control
        // tall at least: on macOS that is the buttons' own height anyway, and
        // on Linux, where the desktop draws its title bar above the window,
        // it is a band of sigil's own.
        //
        // First, and outside the sealed-identity branch below, so the opening
        // screen is not under the buttons either.
        let form = Form::of(ui.ctx());
        // **A phone's Back button.** winit hands it to egui as BrowserBack
        // and nothing else looked at it, so it did nothing. It closes
        // whatever menu is open; with none open, the view goes back the
        // way Escape does. Before anything is drawn, so the menu is gone
        // in this pass and not the next.
        if form.is_phone()
            && ui
                .ctx()
                .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::BrowserBack))
        {
            if egui::Popup::is_any_open(ui.ctx()) {
                egui::Popup::close_all(ui.ctx());
            } else if !self.nav.go_back() {
                let active = self.active();
                let stepped = {
                    let mut ctx = AppContext {
                        navigator: &mut self.navigator,
                        accounts: &mut self.accounts,
                        unfocused: false,
                        away: self.away,
                        notify: self.platform.as_ref(),
                        connections: &self.connections,
                    };
                    self.apps[active].back(&mut ctx)
                };
                if !stepped {
                    // Escape, then: what closes a viewer or a dialog.
                    ui.ctx().input_mut(|i| {
                        i.events.push(egui::Event::Key {
                            key: egui::Key::Escape,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        });
                    });
                }
            }
        }
        let app_on_screen = self.accounts.active().is_unlocked() && self.choosing.is_none();
        // **On a phone the strip is the app bar.** The system's status bar
        // lies over the top of the surface, so the bar starts under it; it
        // is a finger tall; the app's corner control is where it always is;
        // and there is no drag region, since there is no window to drag.
        // The top inset on a desktop is the window's own buttons, which the
        // strip is drawn *behind*; on a phone it is a bar of the system's,
        // drawn over, so the strip is drawn *below* it.
        let strip = match form {
            Form::Desktop => self.insets.top.max(tokens::BUTTON_SM),
            Form::Phone => tokens::BUTTON_LG,
        };
        if form.is_phone() && self.insets.top > 0.0 {
            egui::Panel::top("sigil_inset_top")
                .resizable(false)
                .exact_size(self.insets.top)
                .frame(egui::Frame::NONE.fill(theme.surface_primary))
                // No rule under it: the bar below draws its own, and over
                // the opening screen there is no bar -- a line under the
                // system's own strip read as a bar with nothing on it.
                .show_separator_line(false)
                .show(ui, |_| {});
        }
        // **No bar over the opening screen on a phone.** Everything the
        // bar carries there is the app's -- its name, the exchange, the
        // corner -- and with no app on screen it drew a blank strip a
        // finger tall over a screen that has its own heading. A desktop
        // keeps its strip: it is the title bar, and the window's buttons
        // live in it.
        let strip_wanted = !(form.is_phone() && !app_on_screen);
        if strip_wanted {
            egui::Panel::top("sigil_window_chrome")
                .resizable(false)
                .exact_size(strip)
                .frame(egui::Frame::NONE.fill(theme.surface_primary))
                .show(ui, |ui| {
                    // Double-click to fill the screen, and again to go back:
                    // what a title bar has done on every desktop for thirty
                    // years, and this strip is the title bar.
                    //
                    // Only reached when the system did not handle it first --
                    // a click in that region goes to one place, so if egui was
                    // given it, macOS's own zoom was not.
                    let whole = ui.max_rect();
                    if !form.is_phone() {
                        let bar = ui.allocate_rect(whole, egui::Sense::click());
                        if bar.double_clicked() {
                            let full = ui.ctx().input(|i| i.viewport().maximized);
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(
                                !full.unwrap_or(false),
                            ));
                        }
                    }
                    // On a phone the bar also says where you are: the app's own
                    // title for the view on screen, at the left, the way every
                    // phone's app bar does. A desktop has the tab strip for that.
                    //
                    // The title is also the way to the other apps. A phone has no
                    // rail -- a column of icons beside a 360-point screen was a
                    // sixth of it -- so the apps are a menu on the title, and a
                    // phone that only chats never sees it.
                    // The app's corner of it, drawn **over** the drag region --
                    // a later widget wins the press -- from the right edge in, and
                    // inset from it the way the buttons are inset from the left.
                    //
                    // **Before the head, and measured.** Both were given the whole
                    // bar and drawn one over the other, which is invisible while
                    // the name is short and wrong the moment it is not: a
                    // conversation with a long display name had its name painted
                    // *under* the call and More buttons. The drag region above is
                    // the only thing the corner has to come after, and that is
                    // drawn on a desktop only -- so on a phone the corner goes
                    // first and the head is given what it did not use.
                    let mut corner_used = 0.0f32;
                    if app_on_screen {
                        let corner = whole.shrink2(egui::vec2(tokens::SPACING_SM, 0.0));
                        let active = self.active();
                        let drawn = ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(corner)
                                .layout(egui::Layout::right_to_left(egui::Align::Center)),
                            |ui| {
                                let mut ctx = AppContext {
                                    navigator: &mut self.navigator,
                                    accounts: &mut self.accounts,
                                    unfocused: false,
                                    away: self.away,
                                    notify: self.platform.as_ref(),
                                    connections: &self.connections,
                                };
                                let token = self.nav.top().token.clone();
                                self.apps[active].chrome_ui(&mut ctx, ui, &token);
                            },
                        );
                        corner_used = drawn.response.rect.width();
                    }
                    if form.is_phone() && app_on_screen {
                        let entry = self.nav.top().clone();
                        let active = self.active();
                        // **A named view owns the bar.** `nav_title` answers for a
                        // view that was pushed onto the history and has a name of
                        // its own -- Devices, Members, Channel settings. On a
                        // phone that name and the way back belong *here*, as a
                        // conversation's already do, and the view draws no second
                        // bar under this one. It did: the name was in the strip
                        // and again in the pane below it, under a Back button of
                        // its own, which cost a finger's height of a 804-point
                        // screen on the four panes that have the least room.
                        //
                        // The home app's bar says what the product is; another
                        // app's says which it is, so nobody wonders where they
                        // are.
                        let named = self.apps[active].nav_title(&entry.token);
                        let title = named.clone().unwrap_or_else(|| {
                            if active == 0 {
                                sigil::NAME.to_string()
                            } else {
                                self.apps[active].title().to_string()
                            }
                        });
                        // What the corner left. A gap between them, so a
                        // truncated name does not read as running into a button.
                        let mut left = whole.shrink2(egui::vec2(tokens::SPACING_MD, 0.0));
                        left.max.x =
                            (left.max.x - corner_used - tokens::SPACING_SM).max(left.min.x);
                        let mut switch = None;
                        let mut back = false;
                        ui.scope_builder(
                            egui::UiBuilder::new()
                                .max_rect(left)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            |ui| {
                                // A named view: the way back, then its name.
                                // Back is the history's own step, which is what
                                // the phone's hardware button does -- the view
                                // asked for exactly that when it drew this
                                // itself.
                                if named.is_some() {
                                    if sigil_ui::icon_button(ui, sigil::Icon::Back).clicked() {
                                        back = true;
                                    }
                                    ui.label(egui::RichText::new(&title).heading());
                                    return;
                                }
                                // The app's own head first: an identity's mark,
                                // or Back and a name, in which case there is no
                                // title to draw.
                                let named = {
                                    let mut ctx = AppContext {
                                        navigator: &mut self.navigator,
                                        accounts: &mut self.accounts,
                                        unfocused: false,
                                        away: self.away,
                                        notify: self.platform.as_ref(),
                                        connections: &self.connections,
                                    };
                                    self.apps[active].head_ui(&mut ctx, ui)
                                };
                                if named {
                                    return;
                                }
                                let heading = egui::RichText::new(&title).heading();
                                if self.apps.len() < 2 {
                                    ui.label(heading);
                                    return;
                                }
                                let button = ui
                                    .add(egui::Button::new(heading).frame(false))
                                    .on_hover_text("The other things sigil does");
                                egui::Popup::menu(&button).show(|ui| {
                                    // **Each app by its own mark**, the shape
                                    // this menu has everywhere else: the
                                    // rail's icons are what these apps are
                                    // recognised by on a wide window, and a
                                    // phone showing three bare words was the
                                    // one menu in sigil with nothing to look
                                    // at. The one you are on is filled.
                                    for i in 0..self.apps.len() {
                                        let badge = self.apps[i].tab_notifications();
                                        let said = if badge.is_empty() {
                                            self.apps[i].title().to_string()
                                        } else {
                                            format!("{} ({})", self.apps[i].title(), badge.count)
                                        };
                                        let icon = self.apps[i].icon();
                                        if sigil_ui::icon_item_as(ui, icon, &said, i == active)
                                            .clicked()
                                            && i != active
                                        {
                                            switch = Some(i);
                                        }
                                    }
                                });
                            },
                        );
                        if back {
                            self.nav.go_back();
                        }
                        if let Some(i) = switch {
                            self.navigator.switch_to(AppId(i));
                        }
                    }
                });
        }
        // The notice band: what an app has to say to everybody, whichever
        // tab is open -- see `App::notice_ui`. Under the strip, above the
        // rail and the body, and only when some opened app has something,
        // so there is no empty stripe the rest of the time.
        let noticing: Vec<usize> = (0..self.apps.len())
            .filter(|&i| self.opened[i] && self.apps[i].has_notice())
            .collect();
        if !noticing.is_empty() {
            egui::Panel::top("sigil_notice")
                .resizable(false)
                .frame(
                    egui::Frame::NONE
                        .fill(theme.surface_secondary)
                        .inner_margin(egui::Margin::symmetric(
                            tokens::SPACING_LG as i8,
                            tokens::SPACING_SM as i8,
                        )),
                )
                .show(ui, |ui| {
                    for i in noticing {
                        let mut ctx = AppContext {
                            navigator: &mut self.navigator,
                            accounts: &mut self.accounts,
                            unfocused: false,
                            away: self.away,
                            notify: self.platform.as_ref(),
                            connections: &self.connections,
                        };
                        self.apps[i].notice_ui(&mut ctx, ui);
                    }
                });
        }
        // What the system draws over the bottom and the sides: a gesture
        // bar, the keyboard while it is up, a cutout. Empty panels rather
        // than margins, because a keyboard is three hundred points tall and
        // a margin is an `i8`. The keyboard's is what lifts a composer above
        // it: an edge-to-edge window is never resized for the keyboard, and
        // the composer is the bottom panel of whatever is left.
        for (side, size) in [
            ("sigil_inset_bottom", self.insets.bottom),
            ("sigil_inset_left", self.insets.left),
            ("sigil_inset_right", self.insets.right),
        ] {
            if size <= 0.0 {
                continue;
            }
            let panel = match side {
                "sigil_inset_left" => egui::Panel::left(side),
                "sigil_inset_right" => egui::Panel::right(side),
                _ => egui::Panel::bottom(side),
            };
            panel
                .resizable(false)
                .exact_size(size)
                .frame(egui::Frame::NONE.fill(theme.surface_primary))
                .show(ui, |_| {});
        }
        // Nothing sealed gets a rail. Every app behind it would be a tab onto
        // an identity that cannot do anything, and offering four of those is
        // offering a choice that does not exist yet.
        if !self.accounts.active().is_unlocked() || self.choosing.is_some() {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(theme.surface_primary)
                        .inner_margin(egui::Margin::same(form.body_margin() as i8)),
                )
                .show(ui, |ui| self.welcome(ui, &theme));
            return;
        }
        // Panels rather than a bare horizontal layout: a panel takes the full
        // height of its parent and reserves its width, which is what makes the
        // rail a rail rather than a box the size of its text.
        // No rail on a phone: the screen is the app's, and the other apps
        // are behind the title in the app bar.
        if self.chrome_visible && !form.is_phone() {
            egui::Panel::left("sigil_rail")
                .resizable(false)
                .exact_size(form.rail_width())
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
                    .inner_margin(egui::Margin::same(form.body_margin() as i8)),
            )
            .show(ui, |ui| self.body(ui));

        // Over everything, because it is a question about something from
        // outside and the answer decides whether anything happens at all.
        let egui_ctx = ui.ctx().clone();
        self.link_ui(&egui_ctx);

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
        let card = CARD_WIDTH.min(ui.available_width());
        // Optional, and said so: the first identity is just `identity`, and
        // a name is for telling a second one from it.
        let first = self
            .identity_dir()
            .is_none_or(|dir| !dir.join("identity").exists());
        ui.label(if first { "Name (optional)" } else { "Name" });
        let name = sigil_ui::field(
            ui,
            &mut self.welcome.new_name,
            if first {
                "leave blank for your first"
            } else {
                "work, phone, the-other-one"
            },
            card,
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
            card,
        );
        ui.add_space(tokens::SPACING_SM);
        let again = sigil_ui::password_field(ui, &mut self.welcome.new_again, "again", card);
        // Return in the last box is the form, the same as Return in the
        // passphrase box on the way in: the next thing after typing it twice
        // is Create, and reaching for the mouse to say so is a step nobody
        // wants between them and the thing they just made.
        let entered = again.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(tokens::SPACING_SM);
        ui.colored_label(
            theme.text_secondary,
            "It seals the key on this machine. Nothing can recover it and nothing else \
             holds a copy — a passphrase nobody knows is an identity nobody can open.",
        );
        ui.add_space(tokens::SPACING_MD);

        if ui
            .add_sized([card, tokens::BUTTON_LG], egui::Button::new("Create"))
            .clicked()
            || entered
        {
            self.make_identity();
        }
        ui.add_space(tokens::SPACING_SM);
        if ui
            .add_sized([card, tokens::BUTTON_MD], egui::Button::new("Cancel"))
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
            // With the state it is in, because "will not open" on its own
            // sent somebody looking at the passphrase when the roster was
            // what had it wrong.
            self.welcome.trouble = Some(format!(
                "It was made, but it will not open: {}",
                self.accounts.active().describe()
            ));
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
        // As wide as it likes on a desktop, as wide as there is on a phone.
        let card = CARD_WIDTH.min(ui.available_width());
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
                egui::vec2(card, 0.0),
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
                        .width(card)
                        .height(320.0f32.min(ui.available_height() * 0.5))
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
                            card,
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
                            .add_sized([card, tokens::BUTTON_LG], egui::Button::new("Unlock"))
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
                            .add_sized([card, tokens::BUTTON_LG], egui::Button::new("Open"))
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
                            .add_sized([card, tokens::BUTTON_MD], egui::Button::new("Cancel"))
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
                            [card, tokens::BUTTON_MD],
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

    /// The app rail: one icon per app.
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
                // The count is in the icon's name for the tree, and drawn
                // **on** the icon: a disc at its top right corner, the way a
                // Dock or a phone badges an app. It once hung under the icon
                // as a small number, which is a poor place for it -- not
                // attached to anything, and moving the icons below it as it
                // came and went.
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
                if !badge.is_empty() {
                    rail_badge(ui, response.rect, badge.count, &theme);
                }
                if response.clicked() && !selected {
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
            accounts: &mut self.accounts,
            unfocused: false,
            away: self.away,
            // The real notifier, not `Silent`. It used to be `Silent` here, so
            // an app could only ever say something out loud from `update` --
            // and a view that had something worth announcing found a notifier
            // that reported success and posted nothing.
            notify: self.platform.as_ref(),
            connections: &self.connections,
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
            Some(AppAction::Present) => self.present(ui.ctx()),
            Some(AppAction::Attention(how)) => self.act(AppAction::Attention(how), ui.ctx()),
            Some(AppAction::Quit) => self.act(AppAction::Quit, ui.ctx()),
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
                        unfocused: false,
                        away: self.away,
                        notify: &sigil::Silent,
                        connections: &self.connections,
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

/// The count on a rail icon: a disc in the accent over the icon's top right
/// corner, the number in it, "99+" past two digits so it never outgrows
/// the disc.
fn rail_badge(ui: &mut egui::Ui, icon: egui::Rect, count: u32, theme: &ColorTheme) {
    let text = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    // White on the accent, as the count on a bubble of one's own is.
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text, font, egui::Color32::WHITE);
    let pad = tokens::SPACING_XS;
    let height = galley.size().y + pad;
    let width = (galley.size().x + pad * 2.0).max(height);
    let rect = egui::Rect::from_center_size(
        icon.right_top() + egui::vec2(-pad, pad),
        egui::vec2(width, height),
    );
    ui.painter()
        .rect_filled(rect, tokens::RADIUS_PILL, theme.accent);
    ui.painter().galley(
        rect.center() - galley.size() / 2.0,
        galley,
        egui::Color32::WHITE,
    );
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
        for name in ["my.key", "../elsewhere", "with/slash", "two words"] {
            assert!(
                new_identity_path(dir.path(), name).is_err(),
                "{name:?} was accepted"
            );
        }
    }

    /// No name makes the plain `identity` -- but only while there is none:
    /// the second one has to say what it is.
    #[test]
    fn no_name_is_the_first_identity_and_only_the_first() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["", "   "] {
            assert_eq!(
                new_identity_path(dir.path(), name).unwrap(),
                dir.path().join("identity")
            );
        }
        std::fs::write(dir.path().join("identity"), "not really a key").unwrap();
        let why = new_identity_path(dir.path(), "").expect_err("refused");
        assert!(why.contains("name"), "{why}");
        // A named one is still fine beside it.
        assert!(new_identity_path(dir.path(), "work").is_ok());
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
