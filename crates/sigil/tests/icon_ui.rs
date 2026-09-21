//! The icon set, drawn so somebody can look at it.
//!
//! An icon that does not read at the size it is used is worse than the word it
//! replaced, and the only way to know is to look. This snapshot is the sheet.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use sigil::Icon;
use sigil::theme;

/// Every icon, from the declaration itself rather than a copy of it.
///
/// This list *was* a copy, and it stopped at `Switch`: the nine icons added
/// after it were neither drawn on the sheet nor checked for their word, and
/// both tests passed the whole time by not asking about them. See the
/// `icons!` macro in `sigil::icon`.
const ALL: &[Icon] = Icon::ALL;

/// How tall the sheet is allowed to be. The harness is built at this, and
/// `every_icon_is_on_the_sheet` checks what was actually drawn fits inside it.
const SHEET: f32 = 360.0;

fn harness(dark: bool) -> Harness<'static> {
    harness_measured(dark).0
}

/// The sheet, and how far down the page it reached -- which is the number
/// nobody was looking at when the set outgrew the window.
fn harness_measured(dark: bool) -> (Harness<'static>, std::rc::Rc<std::cell::Cell<f32>>) {
    let drawn = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let reached = drawn.clone();
    let h = Harness::builder()
        // Tall enough for both sizes of the whole set. It was 220, then 280,
        // and each time the set grew the sheet clipped the newest shapes in
        // silence -- the one thing this picture is for is seeing whether a
        // shape works big. `every_icon_is_on_the_sheet` is what now says so
        // out loud instead of leaving it to whoever looks.
        .with_size(egui::vec2(560.0, SHEET))
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
                    // The bottom of everything drawn, not of the buttons: the
                    // larger marks are painted into allocated space and are
                    // not widgets, so a check that queried the tree would miss
                    // exactly the row most likely to be cut.
                    reached.set(ui.min_rect().bottom());
                });
        });
    (h, drawn)
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

/// The sheet has room for every icon, and does not quietly clip the newest.
///
/// A snapshot that cuts off the last row still renders, still passes, and
/// still looks like a picture of the icon set. Twice now the set outgrew the
/// harness and nobody noticed until an icon was wanted and was not there.
#[test]
fn every_icon_is_on_the_sheet() {
    let (mut h, reached) = harness_measured(true);
    h.run();
    let bottom = reached.get();
    assert!(bottom > 0.0, "nothing was drawn at all");
    assert!(
        bottom <= SHEET,
        "the sheet draws down to {bottom} and is {SHEET} tall: {} icons have \
         outgrown it and the snapshot is clipping the newest. Raise SHEET.",
        ALL.len()
    );
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn icons_dark() {
    let mut h = harness(true);
    h.run();
    h.snapshot("icons_dark");
}
