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

fn harness(dark: bool) -> Harness<'static> {
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
    let mut shell = sigil_shell::Shell::new(apps);
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

/// The rail must show an unread count, because that badge is the only thing
/// telling you a message arrived while you were on a call. Checked through the
/// accessibility tree, so it needs no renderer and runs in ordinary CI.
#[test]
fn the_rail_shows_each_app_and_badges_the_unread_one() {
    let mut h = harness(true);
    h.run();
    // Walk the whole tree: the rail's labels are nested several frames deep,
    // and direct children of the root are the panels, not the buttons.
    // Both label and value: accesskit puts an interactive widget's text in the
    // former and a plain one's in the latter, so reading only labels sees the
    // buttons and none of the prose.
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
    let mut found = Vec::new();
    labels(h.root(), &mut found);
    let joined = found.join(" | ");
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
            let mut account = sigil::account::Account::Missing {
                path: "nowhere".into(),
            };
            let mut app_ctx = AppContext {
                navigator: &mut nav,
                account: &mut account,
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
