//! The strip that names where else a pane could be.
//!
//! What is worth proving about a tab strip is not that it draws: it is that
//! **which one you are in reaches somebody who cannot see the mark**. A
//! strip whose only signal is an underline is a strip that says nothing at
//! all to a screen reader, and the one piece of information it exists to
//! carry is exactly that.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::theme;

const PHONE: f32 = 360.0;

/// Draw the strip and report every (name, selected) the tree carries, the
/// width it took, and the harness, so a press can follow.
fn drawn(names: &'static [&'static str], chosen: usize) -> (Vec<(String, bool)>, f32) {
    let width = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let took = width.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(PHONE, 120.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            sigil_ui::tab_strip(ui, names, chosen);
            took.set(ui.min_rect().width());
        });
    h.run();
    h.run();

    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<(String, bool)>) {
        let n = node.accesskit_node();
        if let Some(l) = n.label() {
            // **`toggled`, not `is_selected`.** egui carries a widget's
            // chosen state in accesskit's toggled field whatever
            // `WidgetInfo::selected` is given as a type, and a test reading
            // `is_selected` gets `None` from a strip that marks its tab
            // perfectly — an instrument that cannot see the thing, which
            // reads exactly like the thing being absent.
            out.push((
                l.to_string(),
                n.toggled() == Some(egui::accesskit::Toggled::True),
            ));
        }
        for c in node.children() {
            walk(c, out);
        }
    }
    let mut found = Vec::new();
    walk(h.root(), &mut found);
    (found, width.get())
}

/// **Selected is said and not only drawn.**
#[test]
fn the_tab_you_are_on_is_marked_in_the_tree_and_the_others_are_not() {
    let (said, _) = drawn(&["Following", "Feeds here"], 0);
    assert_eq!(
        said.iter().find(|(n, _)| n == "Following").map(|(_, s)| *s),
        Some(true),
        "the tab being shown is not marked: {said:?}"
    );
    assert_eq!(
        said.iter()
            .find(|(n, _)| n == "Feeds here")
            .map(|(_, s)| *s),
        Some(false),
        "a tab that is not being shown is marked: {said:?}"
    );

    // And it moves with the argument. Without this the test above would hold
    // on a strip that marked the first one whatever it was given.
    let (said, _) = drawn(&["Following", "Feeds here"], 1);
    assert_eq!(
        said.iter()
            .find(|(n, _)| n == "Feeds here")
            .map(|(_, s)| *s),
        Some(true),
        "the mark did not follow the tab being shown: {said:?}"
    );
}

/// Both names are there to be read, and the strip fits the phone it is on.
#[test]
fn every_tab_is_named_and_the_strip_fits_a_phone() {
    let (said, wide) = drawn(&["Following", "Feeds here"], 0);
    assert_eq!(
        said.len(),
        2,
        "a two-tab strip drew {} named things: {said:?}",
        said.len()
    );
    assert!(
        wide <= PHONE + 1.0,
        "the strip drew {wide} points wide in a {PHONE}-point pane"
    );
}

/// A press answers the tab that was pressed, including the one already shown
/// — which is how a timeline is sent back to the top everywhere else, and is
/// the caller's to interpret rather than this widget's to swallow.
#[test]
fn a_press_answers_which_tab_it_was() {
    let answered = std::rc::Rc::new(std::cell::Cell::new(None));
    let got = answered.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(PHONE, 120.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            if let Some(i) = sigil_ui::tab_strip(ui, &["Following", "Feeds here"], 0) {
                got.set(Some(i));
            }
        });
    h.run();
    h.run();
    assert_eq!(answered.get(), None, "it answered a press nobody made");
    h.get_by_label("Feeds here").click();
    h.run();
    h.run();
    assert_eq!(
        answered.get(),
        Some(1),
        "pressing the second tab did not answer the second tab"
    );
}
