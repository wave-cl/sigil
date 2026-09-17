//! What the shell actually looks like, rendered headlessly.
//!
//! Snapshot tests are `#[ignore]`d so an ordinary `cargo test` does not need a
//! GPU. Run them with `scripts/snapshot-test`, which pins the renderer so the
//! pixels are the same on every machine and in CI.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::app::{App, AppAction, AppContext, AppResponse};
use sigil::theme;

/// A stand-in app, so this tests the *shell* rather than whatever voice and
/// chat happen to be drawing this week.
struct Stub {
    title: &'static str,
    unread: u32,
    /// Ask the shell for the opening screen, once, on the next pass. An app
    /// that asked on every pass would be an app nobody could leave, which is
    /// the failure the shell's own `take` is there to prevent.
    asks_to_switch: bool,
    /// Wants its background work run before anybody has opened it.
    unopened: bool,
    /// How many times the shell ran its background work.
    updates: std::rc::Rc<std::cell::Cell<u32>>,
    /// What the background work asks of the shell on the next pass; a
    /// test puts something here.
    asks: std::rc::Rc<std::cell::RefCell<Vec<sigil::app::AppAction>>>,
}

impl Stub {
    fn named(title: &'static str, unread: u32) -> Self {
        Stub {
            title,
            unread,
            asks_to_switch: false,
            unopened: false,
            updates: Default::default(),
            asks: Default::default(),
        }
    }
}

impl App for Stub {
    fn runs_unopened(&self) -> bool {
        self.unopened
    }
    fn update(&mut self, _ctx: &mut AppContext<'_>, _egui_ctx: &egui::Context) {
        self.updates.set(self.updates.get() + 1);
    }
    fn asked(&mut self) -> Vec<sigil::app::AppAction> {
        std::mem::take(&mut *self.asks.borrow_mut())
    }
    fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        ui.heading(self.title);
        if std::mem::take(&mut self.asks_to_switch) {
            return AppResponse::action(sigil::app::AppAction::ChooseIdentity);
        }
        AppResponse::default()
    }
    /// Something in the title strip, named after the app so a test can tell
    /// whose corner was drawn.
    fn chrome_ui(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) {
        let _ = ui.button(format!("{} corner", self.title));
    }
    fn title(&self) -> &str {
        self.title
    }
    fn tab_notifications(&self) -> sigil::TabNotifications {
        sigil::TabNotifications::count(self.unread)
    }
}

/// Everything the screen says.
///
/// Walks the whole tree: the rail's labels are nested several frames deep, and
/// the root's direct children are the panels rather than the buttons. Both
/// label and value, because accesskit puts an interactive widget's text in the
/// former and a plain one's in the latter -- reading only labels sees the
/// buttons and none of the prose.
fn said(h: &Harness<'static>) -> String {
    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let n = node.accesskit_node();
        if let Some(l) = n.label() {
            out.push(l.to_string());
        }
        if let Some(v) = n.value() {
            out.push(v.to_string());
        }
        for child in node.children() {
            walk(child, out);
        }
    }
    let mut found = Vec::new();
    walk(h.root(), &mut found);
    found.join(" | ")
}

fn harness(dark: bool) -> Harness<'static> {
    // Open, because the shell shows the opening screen instead of the rail
    // while nothing is unlocked -- see `welcome_dark`, which is that screen.
    with_account(dark, sigil::Account::unlocked_for_test([4u8; 32]))
}

/// The shell as somebody meets it: nothing open yet.
///
/// The path points at nothing, so there is no key to read and the screen
/// falls back to sigil's own disc. That is deliberate for the **snapshot**:
/// `sqnr::identity::generate` mints a random key, so a real sealed identity
/// draws a different mark on every run and no snapshot of one could pass
/// twice. The identicon a real identity gets is covered by
/// `the_opening_screen_shows_the_chosen_identitys_own_mark`, which writes one
/// and reads its key back rather than comparing pixels.
fn sealed(dark: bool) -> Harness<'static> {
    with_account(
        dark,
        sigil::Account::Locked {
            path: "/tmp/sigil-test/identity".into(),
            trouble: None,
        },
    )
}

fn with_account(dark: bool, account: sigil::Account) -> Harness<'static> {
    let apps: Vec<Box<dyn App>> = vec![
        Box::new(Stub::named("Calls", 0)),
        Box::new(Stub::named("Chat", 3)),
    ];
    // No platform: this is headless, with no tray and no notification
    // daemon, and it should not pretend to have either.
    //
    // And a fixed roster, not the remembered one: `Shell::new` re-opens
    // whatever this machine was last holding, so a snapshot taken without this
    // draws however many accounts the person running it happens to have. That
    // is the "snapshot the layout, not the machine" lesson again, and it would
    // have shown up as CI disagreeing with every developer.
    let mut shell = sigil_shell::Shell::new(apps, None)
        .with_accounts(sigil::accounts::Accounts::of(vec![account]));
    Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            shell.ui(ui);
        })
}

/// A shell whose app asks, on its first pass, to be shown the opening screen.
/// Background work runs for the app on screen and for any app that asked
/// to run unopened -- and for nothing else, so an app nobody has looked at
/// costs nothing per pass.
#[test]
fn an_app_that_runs_unopened_gets_its_update_before_it_is_ever_shown() {
    let counters: Vec<std::rc::Rc<std::cell::Cell<u32>>> =
        (0..3).map(|_| Default::default()).collect();
    let mut apps: Vec<Box<dyn App>> = Vec::new();
    for (i, title) in ["Chat", "Quiet", "Watching"].into_iter().enumerate() {
        let mut stub = Stub::named(title, 0);
        stub.unopened = title == "Watching";
        stub.updates = counters[i].clone();
        apps.push(Box::new(stub));
    }
    let mut shell =
        sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
        ]));
    let ctx = egui::Context::default();
    shell.update_all(&ctx, false);
    shell.update_all(&ctx, false);
    assert_eq!(counters[0].get(), 2, "the app on screen");
    assert_eq!(counters[1].get(), 0, "never opened, never run");
    assert_eq!(counters[2].get(), 2, "asked to run unopened");
}

fn asking_to_switch() -> Harness<'static> {
    let apps: Vec<Box<dyn App>> = vec![Box::new(Stub {
        asks_to_switch: true,
        ..Stub::named("Calls", 0)
    })];
    let mut shell =
        sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
            sigil::Account::unlocked_for_test([5u8; 32]),
        ]));
    Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            shell.ui(ui);
        })
}

/// Switching identity is done on the opening screen, and getting there costs
/// nothing.
///
/// The identity menu used to carry its own list of identities. It now carries
/// one item, which brings this screen back -- the one place that lists every
/// identity in `~/.sqnr`, draws each one's mark and can ask for a passphrase.
///
/// **Nothing is locked to get here.** The identity being left stays open, so
/// the screen offers to open it again rather than asking for its passphrase.
#[test]
fn an_app_can_ask_for_the_opening_screen_without_locking_anything() {
    let mut h = asking_to_switch();
    h.run();
    let seen = said(&h);
    assert!(
        seen.contains("Switch identity"),
        "the app asked for the opening screen and did not get it: {seen}"
    );
    assert!(
        seen.contains("Open") && !seen.contains("Passphrase"),
        "the identity is still open, so there is nothing to unlock: {seen}"
    );
}

/// And the way back changes nothing.
///
/// Somebody who opens this to look and thinks better of it would otherwise
/// have to unlock their way out of a screen they never meant to be on.
#[test]
fn the_opening_screen_can_be_left_again() {
    let mut h = asking_to_switch();
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    let seen = said(&h);
    assert!(
        !seen.contains("Switch identity"),
        "the opening screen stayed up: {seen}"
    );
    assert!(
        seen.contains("Calls"),
        "and what was on screen before did not come back: {seen}"
    );
}

/// The ask is one event, not a state.
///
/// A flag left set sends the shell back to the opening screen on every pass
/// afterwards, which looks exactly like a screen that cannot be dismissed --
/// and both halves of that (the app's and the shell's) have to take it.
#[test]
fn asking_once_does_not_ask_for_ever() {
    let mut h = asking_to_switch();
    h.run();
    h.get_by_label("Open").click();
    h.run();
    h.run();
    let seen = said(&h);
    assert!(
        !seen.contains("Switch identity"),
        "the opening screen came back on its own: {seen}"
    );
}

/// The screen with nothing on it yet: making one.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn making_dark() {
    let dir = tempfile::tempdir().unwrap();
    let apps: Vec<Box<dyn App>> = vec![Box::new(Stub::named("Calls", 0))];
    let mut shell = sigil_shell::Shell::new(apps, None)
        .with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::Missing {
                path: dir.path().join("nothing-here"),
            },
        ]))
        .with_identities(dir.path().to_path_buf());
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            shell.ui(ui);
        });
    h.run();
    h.get_by_label("Create a new identity").click();
    h.run();
    h.remove_cursor();
    h.run();
    h.snapshot("making_dark");
}

/// The same screen on its other errand: coming back to be somebody else.
///
/// Worth a picture of its own because it is not the opening screen with a
/// different heading -- the identity is already open, so where the passphrase
/// box would be there are two buttons, and one of them undoes coming here.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn switching_dark() {
    let mut h = asking_to_switch();
    h.run();
    h.snapshot("switching_dark");
}

/// A shell told how much of its top the window's own chrome covers.
fn with_inset(points: f32) -> Harness<'static> {
    let apps: Vec<Box<dyn App>> = vec![
        Box::new(Stub::named("Calls", 0)),
        Box::new(Stub::named("Chat", 3)),
    ];
    let mut shell =
        sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
        ]));
    Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        // **Fast enough for a double click to be one.** The harness gives each
        // queued event a frame of its own and advances a quarter of a second
        // per frame by default, so a press and a release are half a second
        // apart and two clicks are a second: egui's double-click window is
        // three tenths, and every double click in this harness was two single
        // ones. Nothing said so -- the widget simply never saw a double.
        .with_step_dt(0.05)
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            shell.set_top_inset(points);
            shell.ui(ui);
        })
}

/// A phone: the shell told what it is, and what the system draws over it.
fn with_phone(insets: sigil::Insets) -> Harness<'static> {
    let apps: Vec<Box<dyn App>> = vec![
        Box::new(Stub::named("Calls", 0)),
        Box::new(Stub::named("Chat", 3)),
    ];
    let mut shell =
        sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
        ]));
    Harness::builder()
        .with_size(egui::vec2(412.0, 915.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            shell.set_insets(insets);
            shell.ui(ui);
        })
}

/// On a phone the status bar lies over the top of the surface and the app
/// bar sits under it, so the rail starts below both; a keyboard or gesture
/// bar at the bottom takes its room the same way. The desktop's strip is
/// drawn *behind* the window's buttons; the phone's is drawn *below* the
/// system's bar, so the whole inset moves things, not the part past the
/// strip.
#[test]
fn on_a_phone_the_rail_starts_under_the_status_bar_and_the_app_bar() {
    let mut bare = with_phone(sigil::Insets::NONE);
    bare.run();
    let without = bare.get_by_label("Chat (3)").rect().top();
    assert!(
        without >= sigil::tokens::BUTTON_LG,
        "the app bar is a finger tall, and the rail starts under it: {without}"
    );

    let insets = sigil::Insets {
        top: 24.0,
        bottom: 48.0,
        ..sigil::Insets::NONE
    };
    let mut below = with_phone(insets);
    below.run();
    let with = below.get_by_label("Chat (3)").rect().top();
    assert!(
        (with - without - insets.top).abs() < 0.5,
        "the rail moved down by {}, and the status bar is {}",
        with - without,
        insets.top
    );
    // The rail's icon is bigger under a finger than under a pointer.
    let icon = below.get_by_label("Chat (3)").rect();
    assert!(
        icon.height() >= sigil::tokens::BUTTON_LG - 0.5,
        "a phone's rail icon is {} tall",
        icon.height()
    );
}

/// Double-clicking the top strip fills the screen, and again puts it back.
///
/// That strip **is** the title bar -- sigil draws behind a transparent one --
/// so it has to do what a title bar does. macOS handles the double-click
/// itself when the click reaches its own bar; a click goes to one place, so
/// egui seeing one means the system did not, and this is what answers it.
#[test]
fn double_clicking_the_top_strip_maximises_and_restores() {
    let mut h = with_inset(40.0);
    h.run();
    // In the strip: below the top edge, and past the buttons on the left.
    let at = egui::pos2(400.0, 20.0);

    h.hover_at(at);
    // Press and release, twice. Each queued event gets a frame of its own, so
    // what makes this a double click rather than two is the harness's step
    // being short -- see `with_inset`.
    for _ in 0..2 {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }

    assert_eq!(
        maximised_asks(&h),
        vec![true],
        "a double click on the title bar did not ask the window to fill the screen"
    );

    // And back. The harness does not resize a window -- there is no window --
    // so the state the toggle reads is set here, which is the same thing the
    // desktop reports once it has done as it was asked.
    // A pause first, or the next pair is a *triple* click rather than a second
    // double one -- egui looks back twice the double-click window for that,
    // and a triple is not a double. Two seconds of frames at this step.
    for _ in 0..40 {
        h.step();
    }
    h.input_mut()
        .viewports
        .get_mut(&egui::ViewportId::ROOT)
        .expect("the root viewport")
        .maximized = Some(true);
    for _ in 0..2 {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        });
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    assert_eq!(
        maximised_asks(&h),
        vec![false],
        "the second double click did not put the window back"
    );
}

/// What the last frame asked the window to become, if anything.
fn maximised_asks(h: &Harness<'static>) -> Vec<bool> {
    h.output()
        .viewport_output
        .values()
        .flat_map(|v| v.commands.iter())
        .filter_map(|c| match c {
            egui::ViewportCommand::Maximized(to) => Some(*to),
            _ => None,
        })
        .collect()
}

/// The window's buttons are not drawn over the rail.""
///
/// sigil's title bar is transparent with the content behind it, which is what
/// makes the top of the window sigil's colour rather than the system's grey.
/// The price is that close, minimise and zoom sit **over** the top-left of the
/// content -- exactly where the rail's first icon is -- so the shell leaves
/// that much room and everything starts below it.
#[test]
fn the_windows_own_buttons_leave_the_rail_alone() {
    let inset = 40.0;

    // The rail's second icon, whose label carries its badge and so names one
    // node: the first app's title is also drawn as the heading of the pane it
    // is showing, and a query for "Calls" finds both.
    let mut bare = with_inset(0.0);
    bare.run();
    let without = bare.get_by_label("Chat (3)").rect().top();

    let mut below = with_inset(inset);
    below.run();
    let with = below.get_by_label("Chat (3)").rect().top();

    // The strip is always at least one control tall -- it has the app's
    // corner in it whether or not there are buttons to clear -- so with no
    // inset the rail already starts that far down. What the buttons need is
    // the rest.
    let already = sigil::tokens::BUTTON_SM;
    assert!(
        with >= without + (inset - already),
        "the rail moved down by {}, and the buttons need {inset} of which the \
         strip already gave {already}",
        with - without
    );
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn shell_phone() {
    let mut h = with_phone(sigil::Insets {
        top: 24.0,
        bottom: 48.0,
        ..sigil::Insets::NONE
    });
    h.run();
    h.snapshot("shell_phone");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn shell_dark() {
    let mut h = harness(true);
    h.run();
    h.snapshot("shell_dark");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn shell_light() {
    let mut h = harness(false);
    h.run();
    h.snapshot("shell_light");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn welcome_dark() {
    let mut h = sealed(true);
    h.run();
    h.snapshot("welcome_dark");
}

/// Nothing sealed gets a rail.
///
/// Every app behind it would be a tab onto an identity that cannot do
/// anything, so the whole window is the one decision there is to make.
#[test]
fn a_sealed_identity_gets_the_opening_screen_and_no_rail() {
    let mut h = sealed(true);
    h.run();
    let said = said(&h);
    assert!(
        said.contains("Open an identity"),
        "the opening screen is not there: {said}"
    );
    assert!(
        said.contains("Passphrase"),
        "and it does not ask for anything: {said}"
    );
    // The apps are not offered while there is nobody to be them.
    assert!(!said.contains("Chat (3)"), "the rail is up too: {said}");
}

/// The mark on the opening screen is the identity's own.
///
/// The same identicon it will carry in the corner once it is open, so the
/// thing about to be unlocked is recognisable before it is — and the key is on
/// it, because a mark is a hint for the eye and never an identity (SIP-21).
#[test]
fn the_opening_screen_shows_the_chosen_identitys_own_mark() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity");
    // Written by sqnr's own `generate`, so this tests the file format sqnr
    // actually writes rather than a guess at it.
    sqnr::identity::generate(&path, Some("open sesame")).expect("an identity");
    let key = sqnr::identity::read_public(&path)
        .expect("its key")
        .to_string();

    let mut h = with_account(
        true,
        sigil::Account::Locked {
            path,
            trouble: None,
        },
    );
    h.run();
    assert!(
        said(&h).contains(&key),
        "the screen does not say which identity it is about to open: {}",
        said(&h)
    );
    // And the list above it names a *file*, which is something somebody typed.
    assert!(said(&h).contains("identity (the default)"), "{}", said(&h));
}

/// An identity can be made from the opening screen, and it opens straight
/// into the application.
///
/// The first thing that ever happens to anybody is this screen with nothing on
/// it they can use: a list of a folder that is empty, and a passphrase box
/// with nothing to open. Somebody in that position had to go and find `sqnr`
/// on a command line.
///
/// **Into a temporary folder.** Making an identity writes a file, and a test
/// that wrote into the real `~/.sqnr` would leave one in somebody's list for
/// ever -- found on their next launch, nowhere near the test that did it.
#[test]
fn an_identity_can_be_made_here_and_is_open_when_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let made = dir.path().join("identity-work");

    let apps: Vec<Box<dyn App>> = vec![Box::new(Stub::named("Calls", 0))];
    let mut shell = sigil_shell::Shell::new(apps, None)
        .with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::Missing {
                path: dir.path().join("nothing-here"),
            },
        ]))
        .with_identities(dir.path().to_path_buf());
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            shell.ui(ui);
        });
    h.run();

    h.get_by_label("Create a new identity").click();
    h.run();
    type_into(&mut h, &["work", "open sesame", "open sesame"]);

    h.get_by_label("Create").click();
    h.run();

    assert!(made.exists(), "no identity was written");
    assert!(
        sqnr::identity::is_encrypted(&made).expect("readable"),
        "the key was written in the clear"
    );
    let seen = said(&h);
    assert!(
        !seen.contains("Open an identity") && !seen.contains("Create a new identity"),
        "the screen stayed up, so it was made and not opened: {seen}"
    );
    assert!(
        seen.contains("Calls"),
        "and the application is not there: {seen}"
    );
}

/// The first identity needs no name: left blank, it is `identity`, which is
/// what `sqnr` calls the one you have when you have one. The screen says the
/// name is optional while that is so.
#[test]
fn the_first_identity_needs_no_name() {
    let dir = tempfile::tempdir().unwrap();
    let made = dir.path().join("identity");

    let apps: Vec<Box<dyn App>> = vec![Box::new(Stub::named("Calls", 0))];
    let mut shell = sigil_shell::Shell::new(apps, None)
        .with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::Missing {
                path: dir.path().join("nothing-here"),
            },
        ]))
        .with_identities(dir.path().to_path_buf());
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            shell.ui(ui);
        });
    h.run();

    h.get_by_label("Create a new identity").click();
    h.run();
    assert!(said(&h).contains("Name (optional)"), "{}", said(&h));
    type_into(&mut h, &["", "open sesame", "open sesame"]);
    h.get_by_label("Create").click();
    h.run();

    assert!(made.exists(), "no identity was written: {}", said(&h));
    assert!(!dir.path().join("identity-").exists());
    assert!(sqnr::identity::is_encrypted(&made).expect("readable"));
    assert!(said(&h).contains("Calls"), "not opened: {}", said(&h));
}

/// Return in the second passphrase box is Create: the next thing after
/// typing it twice, without the mouse.
#[test]
fn return_in_the_last_box_makes_the_identity() {
    let dir = tempfile::tempdir().unwrap();
    let made = dir.path().join("identity-work");
    let apps: Vec<Box<dyn App>> = vec![Box::new(Stub::named("Calls", 0))];
    let mut shell = sigil_shell::Shell::new(apps, None)
        .with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::Missing {
                path: dir.path().join("nothing-here"),
            },
        ]))
        .with_identities(dir.path().to_path_buf());
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            shell.ui(ui);
        });
    h.run();
    h.get_by_label("Create a new identity").click();
    h.run();
    type_into(&mut h, &["work", "open sesame", "open sesame"]);
    assert!(!made.exists(), "not before Return");
    // The last box still has the keyboard from `type_into`.
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(made.exists(), "Return did not create it: {}", said(&h));
    assert!(said(&h).contains("Calls"), "not opened: {}", said(&h));
}

/// Two passphrases that do not match make nothing.
///
/// The file is the only copy of the key, so a mistyped passphrase is not an
/// inconvenience: it is an identity nobody can ever open, found out later.
/// Comparing two boxes is the only check that is possible.
#[test]
fn two_passphrases_that_differ_make_no_identity() {
    let dir = tempfile::tempdir().unwrap();

    let apps: Vec<Box<dyn App>> = vec![Box::new(Stub::named("Calls", 0))];
    let mut shell = sigil_shell::Shell::new(apps, None)
        .with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::Missing {
                path: dir.path().join("nothing-here"),
            },
        ]))
        .with_identities(dir.path().to_path_buf());
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            shell.ui(ui);
        });
    h.run();
    h.get_by_label("Create a new identity").click();
    h.run();
    type_into(&mut h, &["work", "open sesame", "open sesamd"]);
    h.get_by_label("Create").click();
    h.run();

    assert!(
        !dir.path().join("identity-work").exists(),
        "an identity was made with a passphrase nobody typed twice"
    );
    assert!(
        said(&h).contains("not the same"),
        "and nothing said why: {}",
        said(&h)
    );
}

/// Type into the screen's fields in the order they appear.
///
/// By position, because there is nothing else to go on: a field's hint never
/// reaches the accessibility tree, and a password field's value is empty on
/// purpose. The order is the order they are drawn in.
fn type_into(h: &mut Harness<'static>, texts: &[&str]) {
    fn fields<'a>(node: egui_kittest::Node<'a>, out: &mut Vec<egui_kittest::Node<'a>>) {
        let role = format!("{:?}", node.accesskit_node().role());
        if role == "TextInput" || role == "PasswordInput" {
            out.push(node);
        }
        for child in node.children() {
            fields(child, out);
        }
    }
    for (i, text) in texts.iter().enumerate() {
        let mut found = Vec::new();
        fields(h.root(), &mut found);
        assert!(
            found.len() > i,
            "there are {} boxes on the screen and this is number {}",
            found.len(),
            i + 1
        );
        found[i].focus();
        found[i].type_text(text);
        h.run();
    }
}

/// You can type your passphrase the moment the window opens.
///
/// It is the only thing on screen and the only thing to do with it, so making
/// somebody click it first is asking them to tell the program what it already
/// knows.
#[test]
fn the_passphrase_box_has_the_keyboard_from_launch() {
    fn focused_field(node: egui_kittest::Node<'_>) -> bool {
        let n = node.accesskit_node();
        // A password field's accessibility value is deliberately empty, which
        // is the whole point of one — so what is asked is where the keyboard
        // *is*, not what arrived.
        // Two roles: egui gives a masked field `PasswordInput` and a plain
        // one `TextInput`, and this box is the former.
        let role = format!("{:?}", n.role());
        if (role == "PasswordInput" || role == "TextInput") && n.is_focused() {
            return true;
        }
        node.children().any(focused_field)
    }

    let mut h = sealed(true);
    // Twice: focus is asked for while the first pass is being drawn, so it is
    // the *next* tree that carries it.
    h.run();
    h.run();
    assert!(
        focused_field(h.root()),
        "nothing has the keyboard, so somebody has to click before typing: {}",
        said(&h)
    );
}

/// Every app is in the rail, and the unread count travels with the one that
/// has it.
///
/// It is **not drawn** there -- it used to hang under the icon as a small
/// accent number, unattached to anything and moving the icons below it as it
/// came and went. What it must not do is stop existing: this is the name the
/// accessibility tree reads out, and the same count is what badges the tray,
/// which is what tells somebody a message arrived while they were on a call.
///
/// Checked through the accessibility tree, so it needs no renderer and runs in
/// ordinary CI.
#[test]
fn the_rail_names_each_app_and_carries_the_unread_count() {
    let mut h = harness(true);
    h.run();
    let joined = said(&h);
    assert!(
        joined.contains("Calls"),
        "the rail lists every app: {joined}"
    );
    assert!(
        joined.contains("Chat (3)"),
        "the unread count no longer reaches anything that can read it out, so \
         nothing says a message arrived: {joined}"
    );
}

/// The desktop pane must say what each capability is *for*, so an unavailable
/// row tells somebody what they lose rather than only that something is
/// missing — and must give the reason, not merely the fact.
#[test]
fn the_desktop_pane_explains_what_is_missing_and_why() {
    use sigil_platform::Platform;
    use sigil_shell::PlatformApp;

    // A releases server that is nobody: this test is about the
    // capabilities, and must not ask GitHub anything.
    let mut app = PlatformApp::new(&Platform::new(), "http://127.0.0.1:1", || {});
    let mut harness = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = sigil::navigator::Navigator::default();
            let mut accounts =
                sigil::accounts::Accounts::of(vec![sigil::account::Account::Missing {
                    path: "nowhere".into(),
                }]);
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        });
    harness.run();

    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let n = node.accesskit_node();
        if let Some(l) = n.label() {
            out.push(l.to_string());
        }
        if let Some(v) = n.value() {
            out.push(v.to_string());
        }
        for c in node.children() {
            walk(c, out);
        }
    }
    let mut found = Vec::new();
    walk(harness.root(), &mut found);
    let said = found.join(" | ");

    for name in [
        "Notifications",
        "Tray icon",
        "Global shortcuts",
        "Start at login",
    ] {
        assert!(said.contains(name), "every capability is listed: {said}");
    }
    // What each is for, not only its name.
    assert!(
        said.contains("when sigil is not in front"),
        "a row says what it is for: {said}"
    );
    // Every row is marked available or not, in words rather than colour alone.
    assert!(
        said.contains("available") || said.contains("unavailable"),
        "each is marked, in words: {said}"
    );
}

/// A fixed report, so the snapshot is of the *layout* and not of whichever
/// machine took it. The real pane says what this desktop can do, which differs
/// on every desktop -- snapshotting that compared macOS against Linux and found
/// eleven thousand differing pixels, correctly.
///
/// One row available and one not, so both states are drawn.
fn a_report() -> sigil_shell::Report {
    use sigil_platform::{Capability, Support};
    sigil_shell::Report {
        session: "an example desktop".into(),
        capabilities: vec![
            Capability::new(
                "Notifications",
                "tells you about a call or a message when sigil is not in front",
                Support::Yes,
            ),
            Capability::new(
                "Global shortcuts",
                "mute and push-to-talk while sigil is not focused",
                Support::no("this session cannot claim a key combination"),
            ),
        ],
        reachable_when_away: true,
        autostart: Support::Yes,
        autostart_enabled: false,
        version: "0.1.5".into(),
        install: sigil_update::Install::MacBundle {
            app: "/Applications/Sigil.app".into(),
        },
        update: sigil_update::UpdateState::Unknown,
    }
}

fn version(s: &str) -> sigil_update::Version {
    sigil_update::Version::parse(s).unwrap()
}

/// The report with the update in a given state.
fn a_report_with(update: sigil_update::UpdateState) -> sigil_shell::Report {
    sigil_shell::Report {
        update,
        ..a_report()
    }
}

fn platform_harness() -> Harness<'static> {
    platform_harness_of(a_report())
}

fn platform_harness_of(report: sigil_shell::Report) -> Harness<'static> {
    use sigil_shell::PlatformApp;
    let mut app = PlatformApp::from_report(report);
    Harness::builder()
        .with_size(egui::vec2(760.0, 480.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let t = sigil::ColorTheme::current(&ctx);
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(t.surface_primary)
                        .inner_margin(egui::Margin::same(sigil::tokens::SPACING_LG as i8)),
                )
                .show(ui, |ui| {
                    let mut nav = sigil::navigator::Navigator::default();
                    let mut accounts =
                        sigil::accounts::Accounts::of(vec![sigil::account::Account::Missing {
                            path: "nowhere".into(),
                        }]);
                    let mut app_ctx = AppContext {
                        navigator: &mut nav,
                        accounts: &mut accounts,
                        unfocused: false,
                        away: false,
                        notify: &sigil::Silent,
                        connections: &Default::default(),
                    };
                    let _ = app.render(&mut app_ctx, ui);
                });
        })
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn desktop_pane_dark() {
    let mut h = platform_harness();
    h.run();
    h.snapshot("desktop_pane_dark");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn desktop_pane_update_dark() {
    let mut h = platform_harness_of(a_report_with(sigil_update::UpdateState::Available {
        version: version("0.1.6"),
        notes_url: "https://github.com/wave-cl/sigil/releases/tag/v0.1.6".into(),
        asset: "sigil-v0.1.6-aarch64-apple-darwin.zip".into(),
    }));
    h.run();
    h.snapshot("desktop_pane_update_dark");
}

/// The pane says what this build is, and when there is a newer one, offers
/// it -- and the tab is marked, so somebody who never opens this pane still
/// learns.
#[test]
fn the_desktop_pane_says_the_version_and_offers_the_newer_one() {
    use sigil_update::UpdateState;
    let available = UpdateState::Available {
        version: version("0.1.6"),
        notes_url: "https://example.invalid/v0.1.6".into(),
        asset: "sigil-v0.1.6-aarch64-apple-darwin.zip".into(),
    };
    let mut h = platform_harness_of(a_report_with(available.clone()));
    h.run();
    let words = said(&h);
    assert!(words.contains("sigil 0.1.5"), "{words}");
    assert!(
        words.contains("installed as /Applications/Sigil.app"),
        "{words}"
    );
    assert!(words.contains("sigil 0.1.6 is available"), "{words}");
    assert!(h.query_by_label("Update").is_some(), "{words}");
    let app = sigil_shell::PlatformApp::from_report(a_report_with(available));
    assert_eq!(app.tab_notifications().count, 1);

    let ready = UpdateState::Ready {
        version: version("0.1.6"),
    };
    let mut h = platform_harness_of(a_report_with(ready.clone()));
    h.run();
    assert!(h.query_by_label("Restart").is_some(), "{}", said(&h));
    assert!(
        h.query_by_label("Check now").is_none(),
        "a check now would only lie"
    );
    assert_eq!(
        sigil_shell::PlatformApp::from_report(a_report_with(ready))
            .tab_notifications()
            .count,
        1
    );

    // Up to date: no offer, no mark, and a way to ask again.
    let up_to_date = UpdateState::UpToDate {
        checked_at: std::time::SystemTime::now(),
    };
    let mut h = platform_harness_of(a_report_with(up_to_date.clone()));
    h.run();
    let words = said(&h);
    assert!(words.contains("Up to date"), "{words}");
    assert!(h.query_by_label("Update").is_none(), "{words}");
    assert!(h.query_by_label("Check now").is_some(), "{words}");
    assert_eq!(
        sigil_shell::PlatformApp::from_report(a_report_with(up_to_date))
            .tab_notifications()
            .count,
        0
    );
}

/// The two ways a check ends without an answer are told apart, in words.
#[test]
fn the_desktop_pane_says_unreachable_and_unsigned_apart() {
    use sigil_update::UpdateState;
    let mut h = platform_harness_of(a_report_with(UpdateState::Unreachable {
        why: "could not reach api.github.com".into(),
    }));
    h.run();
    let words = said(&h);
    assert!(words.contains("could not reach GitHub"), "{words}");
    assert!(!words.contains("not signed"), "{words}");

    let mut h = platform_harness_of(a_report_with(UpdateState::Unsigned {
        version: version("0.1.6"),
        notes_url: "https://example.invalid/v0.1.6".into(),
    }));
    h.run();
    let words = said(&h);
    assert!(words.contains("not signed"), "{words}");
    assert!(!words.contains("could not reach"), "{words}");
    assert!(h.query_by_label("Update").is_none(), "{words}");

    let mut h = platform_harness_of(a_report_with(UpdateState::Unknown));
    h.run();
    let words = said(&h);
    assert!(
        !words.contains("could not reach") && !words.contains("not signed"),
        "{words}"
    );
}

/// A copy that cannot update itself says why where the button would be,
/// and offers no button.
#[test]
fn a_copy_that_cannot_update_itself_says_why() {
    let report = sigil_shell::Report {
        install: sigil_update::Install::Unsupported {
            why:
                "running from target/release, not from a .app bundle, so sigil cannot update itself"
                    .into(),
        },
        ..a_report()
    };
    let mut h = platform_harness_of(report);
    h.run();
    let words = said(&h);
    assert!(words.contains("not from a .app bundle"), "{words}");
    assert!(h.query_by_label("Check now").is_none(), "{words}");
}

/// A shell whose Desktop app is in the given update state, with Chat on
/// screen -- the update has to be visible from *there*.
fn shell_with_update(update: sigil_update::UpdateState, dark: bool) -> Harness<'static> {
    let apps: Vec<Box<dyn App>> = vec![
        Box::new(Stub::named("Chat", 0)),
        Box::new(sigil_shell::PlatformApp::from_report(a_report_with(update))),
    ];
    let mut shell =
        sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
        ]));
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            shell.ui(ui);
        });
    h.run();
    h
}

/// An update is announced across the window, whichever tab is open: a
/// band under the title strip with the button in it. Nothing is drawn
/// there the rest of the time.
#[test]
fn an_update_is_offered_across_the_window_not_only_on_the_desktop_tab() {
    use sigil_update::UpdateState;
    let h = shell_with_update(
        UpdateState::Available {
            version: version("0.1.6"),
            notes_url: String::new(),
            asset: String::new(),
        },
        true,
    );
    let words = said(&h);
    assert!(words.contains("Chat"), "Chat is the tab on screen: {words}");
    assert!(words.contains("sigil 0.1.6 is available"), "{words}");
    assert!(h.query_by_label("Update").is_some(), "{words}");

    let h = shell_with_update(
        UpdateState::Ready {
            version: version("0.1.6"),
        },
        true,
    );
    let words = said(&h);
    assert!(words.contains("sigil 0.1.6 is installed"), "{words}");
    assert!(h.query_by_label("Restart").is_some(), "{words}");

    let h = shell_with_update(
        UpdateState::Failed {
            why: "the digest did not match".into(),
        },
        true,
    );
    let words = said(&h);
    assert!(
        words.contains("The update failed: the digest did not match"),
        "{words}"
    );

    for quiet in [
        UpdateState::Unknown,
        UpdateState::UpToDate {
            checked_at: std::time::SystemTime::now(),
        },
        UpdateState::Unreachable { why: "no".into() },
        UpdateState::Unsigned {
            version: version("0.1.6"),
            notes_url: String::new(),
        },
    ] {
        let h = shell_with_update(quiet.clone(), true);
        let words = said(&h);
        assert!(
            !words.contains("is available") && !words.contains("is installed"),
            "{quiet:?}: {words}"
        );
        assert!(h.query_by_label("Update").is_none(), "{quiet:?}");
    }
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn shell_update_dark() {
    let mut h = shell_with_update(
        sigil_update::UpdateState::Available {
            version: version("0.1.6"),
            notes_url: "https://github.com/wave-cl/sigil/releases/tag/v0.1.6".into(),
            asset: "sigil-v0.1.6-aarch64-apple-darwin.zip".into(),
        },
        true,
    );
    h.run();
    h.snapshot("shell_update_dark");
}

/// The rail carries the mark: "Desktop (1)" while there is an update to
/// press, plain "Desktop" otherwise.
#[test]
fn the_rail_marks_the_desktop_tab_while_an_update_waits() {
    let rail_with = |update: sigil_update::UpdateState| {
        let apps: Vec<Box<dyn App>> = vec![
            Box::new(Stub::named("Chat", 0)),
            Box::new(sigil_shell::PlatformApp::from_report(a_report_with(update))),
        ];
        let mut shell =
            sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
                sigil::Account::unlocked_for_test([4u8; 32]),
            ]));
        let mut h = Harness::builder()
            .with_size(egui::vec2(900.0, 600.0))
            .build_ui(move |ui| {
                let ctx = ui.ctx().clone();
                theme::install(&ctx, theme::light(), theme::dark());
                shell.ui(ui);
            });
        h.run();
        h
    };
    let h = rail_with(sigil_update::UpdateState::Available {
        version: version("0.1.6"),
        notes_url: String::new(),
        asset: String::new(),
    });
    assert!(h.query_by_label("Desktop (1)").is_some(), "{}", said(&h));
    let h = rail_with(sigil_update::UpdateState::Unknown);
    assert!(h.query_by_label("Desktop").is_some(), "{}", said(&h));
    assert!(h.query_by_label("Desktop (1)").is_none());
}

/// Identities are not chosen here.
///
/// The switcher was pinned to the bottom of the rail, so identities were
/// chosen in one place and everything else about them read in another. It
/// lives behind the identity block in the top right now, and
/// `every_identity_is_offered_and_one_is_not_a_choice` in `sigil-chat` is
/// where that property is asserted.
#[test]
fn the_rail_does_not_offer_identities() {
    let one = sigil::Account::unlocked_for_test([1u8; 32]);
    let key = one.unlocked().unwrap().me().to_string();
    let mut h = with_account(true, one);
    h.run();
    assert!(
        !said(&h).contains(&key[..10]),
        "the rail is offering identities: {}",
        said(&h)
    );
}

/// The app on screen gets the right-hand end of the title strip.
///
/// The strip is the drag region under the window's own buttons and was empty
/// on purpose. An app may now put one small control at its far right -- the
/// chat app says which exchange an identity is looking at there -- and it is
/// the **active** app's corner only: two apps drawing into one strip would be
/// two things in the one place a title bar has room for.
#[test]
fn the_active_app_draws_its_corner_of_the_title_strip() {
    let mut h = harness(true);
    h.run();

    // Calls is the app on screen.
    let corner = h.get_by_label("Calls corner").rect();
    // The app's own heading -- the widest "Calls", the rail's icon being the
    // other and square.
    let heading = h
        .get_all_by_label("Calls")
        .map(|n| n.rect())
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .expect("the app draws its heading");
    assert!(
        corner.bottom() <= heading.top(),
        "the corner is not in the strip above the app: {corner:?} against {heading:?}"
    );
    assert!(
        corner.right() >= 900.0 - 3.0 * sigil::tokens::SPACING_SM,
        "the corner is not against the right edge: {corner:?} in 900"
    );
    assert!(
        h.query_by_label("Chat corner").is_none(),
        "an app that is not on screen drew into the strip: {}",
        said(&h)
    );
}

/// And nothing sealed gets one: the strip above the opening screen is empty.
#[test]
fn a_sealed_identity_has_no_app_corner() {
    let mut h = sealed(true);
    h.run();
    assert!(
        h.query_by_label("Calls corner").is_none() && h.query_by_label("Chat corner").is_none(),
        "an app drew into the strip with nothing unlocked: {}",
        said(&h)
    );
}

/// The shell with a hand on the tray: actions pushed here reach it on the
/// next pass, as a desktop's would.
fn with_tray_actions(
    actions: std::rc::Rc<std::cell::RefCell<Vec<sigil_platform::tray::TrayAction>>>,
) -> Harness<'static> {
    let apps: Vec<Box<dyn App>> = vec![Box::new(Stub::named("Chat", 0))];
    let mut shell =
        sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
        ]));
    Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            shell.tray_actions_for_test(std::mem::take(&mut *actions.borrow_mut()));
            shell.update_all(&ctx, false);
            shell.ui(ui);
        })
}

/// The window's commands from the last pass, as words.
fn window_asks(h: &Harness<'static>) -> Vec<String> {
    h.output()
        .viewport_output
        .values()
        .flat_map(|v| v.commands.iter())
        .map(|c| format!("{c:?}"))
        .collect()
}

/// A press on the tray's mark, or Open in its menu, brings the window up:
/// shown, then focused, in that order, since focusing a hidden window does
/// nothing. Quit closes it. Nothing happens on a pass with nothing pressed.
#[test]
fn the_tray_brings_the_window_up_and_can_quit() {
    use sigil_platform::tray::TrayAction;
    let actions = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = with_tray_actions(actions.clone());
    h.run();
    assert!(window_asks(&h).is_empty(), "{:?}", window_asks(&h));

    actions.borrow_mut().push(TrayAction::Open);
    h.step();
    let asks = window_asks(&h);
    let shown = asks.iter().position(|a| a == "Visible(true)");
    let focused = asks.iter().position(|a| a == "Focus");
    assert!(
        matches!((shown, focused), (Some(s), Some(f)) if s < f),
        "shown, then focused: {asks:?}"
    );

    actions.borrow_mut().push(TrayAction::Quit);
    h.step();
    assert!(
        window_asks(&h).iter().any(|a| a == "Close"),
        "{:?}",
        window_asks(&h)
    );
}

/// The shell with one app whose background work can be made to ask for
/// things, and a hand on the tray.
fn with_asking_app(
    asks: std::rc::Rc<std::cell::RefCell<Vec<sigil::app::AppAction>>>,
    unread: u32,
) -> Harness<'static> {
    let mut stub = Stub::named("Chat", unread);
    stub.asks = asks;
    let apps: Vec<Box<dyn App>> = vec![Box::new(stub)];
    let mut shell =
        sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
        ]));
    Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            shell.update_all(&ctx, false);
            shell.ui(ui);
        })
}

/// What an app's background work asks for reaches the window: a ring's
/// Present shows and focuses it, and a mention's request for attention is
/// passed on as the desktop's own, without taking focus.
#[test]
fn what_background_work_asks_for_reaches_the_window() {
    use sigil::app::AppAction;
    let asks = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = with_asking_app(asks.clone(), 0);
    h.run();
    assert!(window_asks(&h).is_empty(), "{:?}", window_asks(&h));

    asks.borrow_mut().push(AppAction::Present);
    h.step();
    let said = window_asks(&h);
    assert!(
        said.contains(&"Visible(true)".to_string()) && said.contains(&"Focus".to_string()),
        "{said:?}"
    );

    asks.borrow_mut()
        .push(AppAction::Attention(sigil::Attention::Informational));
    h.step();
    let said = window_asks(&h);
    assert!(
        said.iter().any(|a| a.starts_with("RequestUserAttention")),
        "{said:?}"
    );
    assert!(
        !said.contains(&"Focus".to_string()),
        "attention is not focus: {said:?}"
    );
}

/// The rail's icon carries its count as a disc over its top right corner,
/// and no disc at nought. "99+" past two digits.
#[test]
fn the_rail_icon_carries_its_count_on_a_disc() {
    let asks = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = with_asking_app(asks, 7);
    h.run();
    let icon = h.get_by_label("Chat (7)").rect();
    // The disc is painted, not a widget: look at what was painted. Over
    // the icon's top right corner a small filled shape is there; at nought
    // it is not.
    let corner = icon.right_top() + egui::vec2(-4.0, 4.0);
    assert!(
        small_fill_at(&h, corner),
        "a small filled shape over the corner at {corner:?}"
    );

    let asks = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = with_asking_app(asks, 0);
    h.run();
    // The heading says "Chat" too; the rail's icon is the leftmost.
    let icon = h
        .get_all_by_label("Chat")
        .map(|n| n.rect())
        .min_by(|a, b| a.left().total_cmp(&b.left()))
        .expect("the rail icon");
    let corner = icon.right_top() + egui::vec2(-4.0, 4.0);
    assert!(!small_fill_at(&h, corner), "no disc at nought");
}

/// Whether the last pass painted a small filled rectangle over `at`.
fn small_fill_at(h: &Harness<'static>, at: egui::Pos2) -> bool {
    fn walk(shape: &egui::Shape, at: egui::Pos2) -> bool {
        match shape {
            egui::Shape::Vec(inner) => inner.iter().any(|s| walk(s, at)),
            egui::Shape::Rect(r) => {
                r.rect.contains(at)
                    && r.fill.a() > 0
                    && r.rect.width() < 28.0
                    && r.rect.height() < 28.0
            }
            _ => false,
        }
    }
    h.output().shapes.iter().any(|c| walk(&c.shape, at))
}

/// Closing the window with a tray up hides it rather than quitting, and the
/// apps are then told they are not in front; Open from the tray brings it
/// back; Quit from the tray lets the next close through. Without a tray a
/// close is a close.
#[test]
fn closing_the_window_puts_sigil_in_the_tray_and_quit_lets_it_go() {
    use sigil_platform::tray::TrayAction;
    let unfocused_seen = std::rc::Rc::new(std::cell::Cell::new(false));
    struct Watching {
        seen: std::rc::Rc<std::cell::Cell<bool>>,
    }
    impl App for Watching {
        fn update(&mut self, ctx: &mut AppContext<'_>, _egui_ctx: &egui::Context) {
            self.seen.set(ctx.unfocused);
        }
        fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
            ui.label("watching");
            AppResponse::default()
        }
        fn title(&self) -> &str {
            "Chat"
        }
    }
    let build = |pretend_tray: bool,
                 actions: std::rc::Rc<std::cell::RefCell<Vec<TrayAction>>>,
                 seen: std::rc::Rc<std::cell::Cell<bool>>| {
        let apps: Vec<Box<dyn App>> = vec![Box::new(Watching { seen })];
        let mut shell =
            sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
                sigil::Account::unlocked_for_test([4u8; 32]),
            ]));
        if pretend_tray {
            shell.pretend_tray_for_test();
        }
        Harness::builder()
            .with_size(egui::vec2(900.0, 600.0))
            .build_ui(move |ui| {
                let ctx = ui.ctx().clone();
                theme::install(&ctx, theme::light(), theme::dark());
                shell.tray_actions_for_test(std::mem::take(&mut *actions.borrow_mut()));
                shell.update_all(&ctx, false);
                shell.ui(ui);
            })
    };
    let close = |h: &mut Harness<'static>| {
        h.input_mut()
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .expect("the window")
            .events
            .push(egui::ViewportEvent::Close);
        h.step();
    };

    // With a tray: hidden, not closed, and the app told.
    let actions = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = build(true, actions.clone(), unfocused_seen.clone());
    h.run();
    assert!(!unfocused_seen.get(), "in front to begin with");
    close(&mut h);
    let said = window_asks(&h);
    assert!(said.contains(&"CancelClose".to_string()), "{said:?}");
    assert!(said.contains(&"Visible(false)".to_string()), "{said:?}");
    h.step();
    assert!(unfocused_seen.get(), "closed to the tray is not in front");

    // Open from the tray: back, and in front.
    actions.borrow_mut().push(TrayAction::Open);
    h.step();
    assert!(window_asks(&h).contains(&"Visible(true)".to_string()));
    h.step();
    assert!(!unfocused_seen.get(), "brought back: in front again");

    // Quit from the tray, then a close: let through.
    actions.borrow_mut().push(TrayAction::Quit);
    h.step();
    close(&mut h);
    assert!(
        !window_asks(&h).contains(&"CancelClose".to_string()),
        "{:?}",
        window_asks(&h)
    );

    // No tray: a close is a close.
    let actions = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut h = build(false, actions, unfocused_seen.clone());
    h.run();
    close(&mut h);
    assert!(
        !window_asks(&h).contains(&"CancelClose".to_string()),
        "{:?}",
        window_asks(&h)
    );
}

/// A notification pressed brings the window up and is handed to the apps;
/// the one it belongs to shows it and is switched to. One that belongs to
/// nobody brings the window up and nothing else.
#[test]
fn a_pressed_notification_opens_the_app_it_came_from() {
    use sigil::{Notice, Notify, Target};
    struct Pressing(std::rc::Rc<std::cell::RefCell<Vec<Target>>>);
    impl Notify for Pressing {
        fn notice(&self, _notice: Notice<'_>) -> bool {
            false
        }
        fn pressed(&self) -> Vec<Target> {
            std::mem::take(&mut *self.0.borrow_mut())
        }
    }
    struct Opening {
        title: &'static str,
        mine: bool,
        opened: std::rc::Rc<std::cell::RefCell<Vec<Target>>>,
    }
    impl App for Opening {
        fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
            ui.heading(format!("{} here", self.title));
            AppResponse::default()
        }
        fn title(&self) -> &str {
            self.title
        }
        fn open(&mut self, _ctx: &mut AppContext<'_>, target: &Target) -> bool {
            if self.mine {
                self.opened.borrow_mut().push(target.clone());
            }
            self.mine
        }
    }
    let pressed = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let apps: Vec<Box<dyn App>> = vec![
        Box::new(Opening {
            title: "Calls",
            mine: false,
            opened: opened.clone(),
        }),
        Box::new(Opening {
            title: "Chat",
            mine: true,
            opened: opened.clone(),
        }),
    ];
    let mut shell = sigil_shell::Shell::new(apps, None)
        .with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
        ]))
        .with_notify(Box::new(Pressing(pressed.clone())));
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            shell.update_all(&ctx, false);
            shell.ui(ui);
        });
    h.run();
    assert!(
        said(&h).contains("Calls here"),
        "the first app to begin with"
    );

    let target = Target {
        identity: sqnr_core::PubKey::new([4u8; 32]),
        exchange: String::new(),
        channel: [8u8; 32],
    };
    pressed.borrow_mut().push(target.clone());
    h.step();
    assert!(
        window_asks(&h).contains(&"Visible(true)".to_string()),
        "the window is brought up: {:?}",
        window_asks(&h)
    );
    h.step();
    assert_eq!(
        *opened.borrow(),
        vec![target],
        "handed to the app that owns it"
    );
    assert!(
        said(&h).contains("Chat here"),
        "and switched to it: {}",
        said(&h)
    );
}

/// The Desktop pane's switch for direct calls flips the preference the
/// roster holds -- on by default, off when pressed, and marked as changed
/// so the shell writes it -- and says what turning it on discloses.
#[test]
fn the_desktop_pane_switches_direct_calls_off_and_says_what_they_disclose() {
    use egui_kittest::kittest::Queryable;
    use sigil_platform::Platform;
    use sigil_shell::PlatformApp;

    let mut app = PlatformApp::new(&Platform::new(), "http://127.0.0.1:1", || {});
    let accounts = std::rc::Rc::new(std::cell::RefCell::new(sigil::accounts::Accounts::of(
        vec![sigil::Account::unlocked_for_test([4u8; 32])],
    )));
    let shared = accounts.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 700.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let mut nav = sigil::navigator::Navigator::default();
            let mut accounts = shared.borrow_mut();
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                accounts: &mut accounts,
                unfocused: false,
                away: false,
                notify: &sigil::Silent,
                connections: &Default::default(),
            };
            let _ = app.render(&mut app_ctx, ui);
        });
    h.run();
    assert!(accounts.borrow().prefs.direct_calls, "on by default");
    assert!(
        !accounts.borrow().prefs.take_changed(),
        "nothing changed yet"
    );

    let switch = h.get_by_label("Connect calls directly when possible");
    switch.hover();
    h.run();
    let said = {
        fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
            let n = node.accesskit_node();
            if let Some(l) = n.label() {
                out.push(l.to_string());
            }
            if let Some(v) = n.value() {
                out.push(v.to_string());
            }
            for c in node.children() {
                walk(c, out);
            }
        }
        let mut found = Vec::new();
        walk(h.root(), &mut found);
        found.join(" | ")
    };
    assert!(
        said.contains("learns your address"),
        "the switch does not say what it discloses: {said}"
    );

    h.get_by_label("Connect calls directly when possible")
        .click();
    h.run();
    assert!(!accounts.borrow().prefs.direct_calls, "pressed off");
    assert!(
        accounts.borrow().prefs.take_changed(),
        "and marked as changed, so it is written"
    );
    h.get_by_label("Connect calls directly when possible")
        .click();
    h.run();
    assert!(accounts.borrow().prefs.direct_calls, "and on again");
}

/// An app asking to quit -- an update with a new copy waiting -- ends the
/// process: the window closes and is not put in the tray, which is what a
/// plain close would do with a tray up. Found when Restart after an update
/// left the old copy sitting in the tray and the new one waiting for it.
#[test]
fn an_app_asking_to_quit_is_not_put_in_the_tray() {
    struct Leaving {
        ask: std::rc::Rc<std::cell::Cell<bool>>,
    }
    impl App for Leaving {
        fn update(&mut self, _ctx: &mut AppContext<'_>, _egui_ctx: &egui::Context) {}
        fn asked(&mut self) -> Vec<AppAction> {
            if self.ask.replace(false) {
                vec![AppAction::Quit]
            } else {
                Vec::new()
            }
        }
        fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
            ui.label("leaving");
            AppResponse::default()
        }
        fn title(&self) -> &str {
            "Chat"
        }
    }
    let ask = std::rc::Rc::new(std::cell::Cell::new(false));
    let apps: Vec<Box<dyn App>> = vec![Box::new(Leaving { ask: ask.clone() })];
    let mut shell =
        sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
        ]));
    shell.pretend_tray_for_test();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            shell.update_all(&ctx, false);
            shell.ui(ui);
        });
    h.run();
    let close = |h: &mut Harness<'static>| {
        h.input_mut()
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .expect("the window")
            .events
            .push(egui::ViewportEvent::Close);
        h.step();
    };
    // The control: with a tray up, a close is a hide.
    close(&mut h);
    assert!(
        window_asks(&h).contains(&"CancelClose".to_string()),
        "a close with a tray up should hide: {:?}",
        window_asks(&h)
    );
    // Asked to quit: the window is closed, and the close that follows is
    // let through.
    ask.set(true);
    h.step();
    assert!(
        window_asks(&h).contains(&"Close".to_string()),
        "quitting closes the window: {:?}",
        window_asks(&h)
    );
    close(&mut h);
    assert!(
        !window_asks(&h).contains(&"CancelClose".to_string()),
        "the close after a quit was put in the tray: {:?}",
        window_asks(&h)
    );
}

/// Nobody here for five minutes is away, and the apps are told; a touch
/// of the pointer is somebody back. Time is the harness's, a minute a
/// step, and the window is the only witness -- as on a desktop that
/// cannot say how long since the last keypress anywhere.
#[test]
fn away_is_five_minutes_without_input_and_a_touch_is_back() {
    let seen = std::rc::Rc::new(std::cell::Cell::new(false));
    struct Watching {
        away: std::rc::Rc<std::cell::Cell<bool>>,
    }
    impl App for Watching {
        fn update(&mut self, ctx: &mut AppContext<'_>, _egui_ctx: &egui::Context) {
            self.away.set(ctx.away);
        }
        fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
            ui.label("watching");
            AppResponse::default()
        }
        fn title(&self) -> &str {
            "Chat"
        }
    }
    let apps: Vec<Box<dyn App>> = vec![Box::new(Watching { away: seen.clone() })];
    let mut shell =
        sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
        ]));
    shell.watch_own_input_only_for_test();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .with_step_dt(60.0)
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            shell.update_all(&ctx, false);
            shell.ui(ui);
        });
    // A touch to start the clock, then a minute a step until away: the
    // fifth minute, and not before.
    // The event is queued; the step that carries it is the touch.
    h.event(egui::Event::PointerMoved(egui::pos2(100.0, 100.0)));
    h.step();
    assert!(!seen.get());
    let mut minutes = 0;
    while !seen.get() {
        h.step();
        minutes += 1;
        assert!(minutes <= 10, "never away");
    }
    assert_eq!(
        minutes, 5,
        "away after five minutes without input, not {minutes}"
    );
    // And a touch of the pointer is somebody back, at once.
    h.event(egui::Event::PointerMoved(egui::pos2(120.0, 100.0)));
    h.step();
    assert!(!seen.get(), "back at the pointer's first move");
}

/// Do not disturb from the tray's menu flips the setting the roster holds,
/// which the Desktop pane and every app read; and back again.
#[test]
fn do_not_disturb_from_the_tray_flips_the_setting() {
    use sigil_platform::tray::TrayAction;
    let actions = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let dnd = std::rc::Rc::new(std::cell::Cell::new(false));
    struct Reading {
        dnd: std::rc::Rc<std::cell::Cell<bool>>,
    }
    impl App for Reading {
        fn update(&mut self, ctx: &mut AppContext<'_>, _egui_ctx: &egui::Context) {
            self.dnd.set(ctx.accounts.quiet.dnd);
        }
        fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
            ui.label("reading");
            AppResponse::default()
        }
        fn title(&self) -> &str {
            "Chat"
        }
    }
    let apps: Vec<Box<dyn App>> = vec![Box::new(Reading { dnd: dnd.clone() })];
    let mut shell =
        sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
            sigil::Account::unlocked_for_test([4u8; 32]),
        ]));
    let pending = actions.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(900.0, 600.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            shell.tray_actions_for_test(std::mem::take(&mut *pending.borrow_mut()));
            shell.update_all(&ctx, false);
            shell.ui(ui);
        });
    h.run();
    assert!(!dnd.get());
    actions.borrow_mut().push(TrayAction::QuietToggled);
    h.step();
    h.step();
    assert!(dnd.get(), "on, as every app sees it");
    actions.borrow_mut().push(TrayAction::QuietToggled);
    h.step();
    h.step();
    assert!(!dnd.get(), "and off again");
}
