//! What the shell actually looks like, rendered headlessly.
//!
//! Snapshot tests are `#[ignore]`d so an ordinary `cargo test` does not need a
//! GPU. Run them with `scripts/snapshot-test`, which pins the renderer so the
//! pixels are the same on every machine and in CI.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use sigil::app::{App, AppContext, AppResponse};
use sigil::theme;

/// A stand-in app, so this tests the *shell* rather than whatever voice and
/// chat happen to be drawing this week.
struct Stub {
    title: &'static str,
    unread: u32,
}

impl App for Stub {
    fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        ui.heading(self.title);
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
        Box::new(Stub {
            title: "Calls",
            unread: 0,
        }),
        Box::new(Stub {
            title: "Chat",
            unread: 3,
        }),
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
                hidden: false,
                notify: &sigil::Silent,
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
                        hidden: false,
                        notify: &sigil::Silent,
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

/// The switcher appears only when there is a choice, and every account in it
/// is live whether or not it is the one being shown.
#[test]
fn the_rail_offers_a_switcher_once_there_is_more_than_one_identity() {
    fn labels(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let n = node.accesskit_node();
        if let Some(l) = n.label() {
            out.push(l.to_string());
        }
        if let Some(v) = n.value() {
            out.push(v.to_string());
        }
        for child in node.children() {
            labels(child, out);
        }
    }

    fn rail_labels(accounts: sigil::accounts::Accounts) -> String {
        let apps: Vec<Box<dyn App>> = vec![Box::new(Stub {
            title: "Calls",
            unread: 0,
        })];
        let mut shell = sigil_shell::Shell::new(apps, None).with_accounts(accounts);
        let mut h = Harness::builder()
            .with_size(egui::vec2(900.0, 600.0))
            .build_ui(move |ui| {
                let ctx = ui.ctx().clone();
                theme::install(&ctx, theme::light(), theme::dark());
                ctx.set_theme(egui::Theme::Dark);
                shell.ui(ui);
            });
        h.run();
        let mut found = Vec::new();
        labels(h.root(), &mut found);
        found.join(" | ")
    }

    // Fixed seeds: the key is drawn, and a generated one renders differently
    // on every run, which is a snapshot that can never pass twice.
    let one = sigil::Account::unlocked_for_test([1u8; 32]);
    let two = sigil::Account::unlocked_for_test([2u8; 32]);
    let first = one.unlocked().unwrap().me().to_string();

    let alone = rail_labels(sigil::accounts::Accounts::of(vec![
        sigil::Account::unlocked_for_test([1u8; 32]),
    ]));
    assert!(
        !alone.contains(&first[..10]),
        "one account is not a choice, so there is nothing to switch between: {alone}"
    );

    let several = rail_labels(sigil::accounts::Accounts::of(vec![one, two]));
    assert!(
        several.contains(&first[..10]),
        "each held identity is offered: {several}"
    );
}
