//! The icon set, drawn so somebody can look at it.
//!
//! An icon that does not read at the size it is used is worse than the word it
//! replaced, and the only way to know is to look. This snapshot is the sheet.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use egui_kittest::kittest::Queryable;
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

/// **No two icons are the same picture.**
///
/// `Forward` was added as the mirror of `Back`, for the picture viewer's Next
/// -- which had been a *downward* chevron beside a left-pointing Previous.
/// The mirror of `Back` is an arrow to the right, and so was `Send`: two
/// icons, two words, one picture. Nothing was ambiguous in use, because they
/// never appear on the same screen, but a set where two names draw the same
/// thing is a set that will eventually put them side by side. The sheet
/// showed it; nothing failed.
///
/// So: draw each one on its own and compare what was actually painted. Every
/// icon gets the same rectangle, colour and stroke, so two that come out
/// equal are the same shape in the same place -- which is the whole claim.
#[test]
fn no_two_icons_draw_the_same_shape() {
    fn painted(icon: Icon) -> String {
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(40.0, 40.0));
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().clone();
            sigil::icon::draw(&painter, rect, icon, egui::Color32::WHITE);
        });
        // Dropping a `TexturesDelta` with the font atlas in it panics on the
        // way out, which reads as a failure of whatever the test was doing.
        out.textures_delta.clear();
        format!("{:?}", out.shapes)
    }

    let mut seen: std::collections::HashMap<String, Icon> = std::collections::HashMap::new();
    let mut same: Vec<String> = Vec::new();
    for icon in ALL {
        let shape = painted(*icon);
        // The instrument can say nothing: an icon that paints nothing at all
        // would collide with every other one that paints nothing, which is a
        // different fault and worth its own words.
        assert!(shape.len() > 20, "{icon:?} painted almost nothing: {shape}");
        if let Some(other) = seen.insert(shape, *icon) {
            same.push(format!("{other:?} and {icon:?}"));
        }
    }
    assert!(
        same.is_empty(),
        "{} pair(s) of icons draw the same picture:\n  {}",
        same.len(),
        same.join("\n  ")
    );
    // And that it can say yes: the same icon twice must collide with itself.
    assert_eq!(
        painted(Icon::Back),
        painted(Icon::Back),
        "the same icon drew differently twice, so this comparison means nothing"
    );
}

/// A count on a menu row is a **pill**, not brackets in the words.
///
/// The row is the navigation rail on a phone, and it drew `Chat (3)` --
/// the only count in sigil written out, next to a chats list, a rail and a
/// window badge that all draw a filled pill. What is drawn here is the
/// word and the number apart; what is *said* is still the brackets,
/// because the tree takes words and not shapes.
#[test]
fn a_count_on_a_menu_row_is_a_pill_and_not_brackets() {
    fn drawn(count: u32) -> (Vec<String>, usize) {
        let ctx = egui::Context::default();
        theme::install(&ctx, theme::light(), theme::dark());
        ctx.set_theme(egui::Theme::Dark);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_max_width(240.0);
            sigil::icon::icon_item_counted(ui, Icon::Compose, "Chat", false, count);
        });
        let mut words = Vec::new();
        let mut rects = 0;
        fn walk(s: &egui::epaint::Shape, words: &mut Vec<String>, rects: &mut usize) {
            match s {
                egui::epaint::Shape::Text(t) => words.push(t.galley.text().to_owned()),
                egui::epaint::Shape::Rect(_) => *rects += 1,
                egui::epaint::Shape::Vec(v) => {
                    for s in v {
                        walk(s, words, rects);
                    }
                }
                _ => {}
            }
        }
        for c in &out.shapes {
            walk(&c.shape, &mut words, &mut rects);
        }
        // epaint panics on a dropped texture delta it was never given back.
        out.textures_delta.clear();
        (words, rects)
    }

    let (plain, plain_rects) = drawn(0);
    let (counted, counted_rects) = drawn(3);
    assert!(
        counted.iter().any(|w| w == "Chat"),
        "the word lost its own galley: {counted:?}"
    );
    assert!(
        counted.iter().any(|w| w == "3"),
        "the count was not drawn: {counted:?}"
    );
    assert!(
        !counted.iter().any(|w| w.contains("(3)")),
        "the count is still in the words: {counted:?}"
    );
    assert_eq!(plain.iter().filter(|w| *w == "Chat").count(), 1);
    assert_eq!(
        counted_rects,
        plain_rects + 1,
        "the pill behind the number was not filled: {plain_rects} without, {counted_rects} with"
    );
}

/// And the count is still spoken, because a pill says nothing to a reader
/// that cannot see it.
#[test]
fn the_count_is_still_said_in_words() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(240.0, 80.0))
        .build_ui(|ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            sigil::icon::icon_item_counted(ui, Icon::Compose, "Chat", false, 3);
            sigil::icon::icon_item_counted(ui, Icon::Call, "Phone", false, 0);
            sigil::icon::icon_item_counted(ui, Icon::Settings, "Exchange", false, 140);
        });
    h.run();
    h.get_by_label("Chat (3)");
    h.get_by_label("Phone");
    // Past a hundred the pill says "99+" and the words say the number:
    // the shape has a width to keep, and the tree does not.
    h.get_by_label("Exchange (140)");
}
