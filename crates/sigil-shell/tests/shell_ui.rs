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
    fn labels(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        if let Some(l) = node.accesskit_node().label() {
            out.push(l.to_string());
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
