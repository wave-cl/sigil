//! The icon set, drawn so somebody can look at it.
//!
//! An icon that does not read at the size it is used is worse than the word it
//! replaced, and the only way to know is to look. This snapshot is the sheet.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use sigil::Icon;
use sigil::theme;

const ALL: &[Icon] = &[
    Icon::Call,
    Icon::HangUp,
    Icon::Settings,
    Icon::People,
    Icon::Device,
    Icon::Plus,
    Icon::Compose,
    Icon::Chevron,
    Icon::Menu,
    Icon::Search,
    Icon::Attach,
    Icon::Send,
    Icon::Back,
    Icon::Close,
    Icon::Refresh,
    Icon::Pencil,
    Icon::Public,
    Icon::Reply,
    Icon::React,
    Icon::More,
];

fn harness(dark: bool) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(560.0, 220.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            let t = sigil::ColorTheme::current(&ctx);
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(t.surface_primary)
                        .inner_margin(egui::Margin::same(sigil::tokens::SPACING_LG as i8)),
                )
                .show(ui, |ui| {
                    // At the size they are actually used, and again larger, so
                    // a shape that only works big is obvious.
                    ui.horizontal_wrapped(|ui| {
                        for icon in ALL {
                            sigil::icon_button(ui, *icon);
                        }
                    });
                    ui.add_space(sigil::tokens::SPACING_LG);
                    ui.horizontal_wrapped(|ui| {
                        for icon in ALL {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                            sigil::icon::draw(ui.painter(), rect, *icon, t.text_primary);
                        }
                    });
                });
        })
}

#[test]
fn every_icon_reaches_the_accessibility_tree_by_its_word() {
    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let n = node.accesskit_node();
        if let Some(l) = n.label() {
            out.push(l.to_string());
        }
        for c in node.children() {
            walk(c, out);
        }
    }
    let mut h = harness(true);
    h.run();
    let mut found = Vec::new();
    walk(h.root(), &mut found);
    let said = found.join(" | ");
    for icon in ALL {
        assert!(
            said.contains(icon.word()),
            "{icon:?} is a picture and nothing else: {said}"
        );
    }
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn icons_dark() {
    let mut h = harness(true);
    h.run();
    h.snapshot("icons_dark");
}
