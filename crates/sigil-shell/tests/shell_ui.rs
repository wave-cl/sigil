//! What the shell actually looks like, rendered headlessly.
//!
//! Snapshot tests are `#[ignore]`d so an ordinary `cargo test` does not need a
//! GPU. Run them with `scripts/snapshot-test`, which pins the renderer so the
//! pixels are the same on every machine and in CI.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::app::{App, AppContext, AppResponse};
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
}

impl Stub {
    fn named(title: &'static str, unread: u32) -> Self {
        Stub {
            title,
            unread,
            asks_to_switch: false,
        }
    }
}

impl App for Stub {
    fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        ui.heading(self.title);
        if std::mem::take(&mut self.asks_to_switch) {
            return AppResponse::action(sigil::app::AppAction::ChooseIdentity);
        }
        AppResponse::default()
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
fn asking_to_switch() -> Harness<'static> {
    let apps: Vec<Box<dyn App>> = vec![Box::new(Stub {
        title: "Calls",
        unread: 0,
        asks_to_switch: true,
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

    assert!(
        with >= without + inset,
        "the rail moved down by {}, and the buttons need {inset}",
        with - without
    );
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

/// The rail must show an unread count, because that badge is the only thing
/// telling you a message arrived while you were on a call. Checked through the
/// accessibility tree, so it needs no renderer and runs in ordinary CI.
#[test]
fn the_rail_shows_each_app_and_badges_the_unread_one() {
    let mut h = harness(true);
    h.run();
    let joined = said(&h);
    assert!(
        joined.contains("Calls"),
        "the rail lists every app: {joined}"
    );
    assert!(
        joined.contains("Chat (3)"),
        "an app with unread messages is badged in the rail: {joined}"
    );
}

/// The desktop pane must say what each capability is *for*, so an unavailable
/// row tells somebody what they lose rather than only that something is
/// missing — and must give the reason, not merely the fact.
#[test]
fn the_desktop_pane_explains_what_is_missing_and_why() {
    use sigil_platform::Platform;
    use sigil_shell::PlatformApp;

    let mut app = PlatformApp::new(Platform::new());
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
    }
}

fn platform_harness() -> Harness<'static> {
    use sigil_shell::PlatformApp;
    let mut app = PlatformApp::from_report(a_report());
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
