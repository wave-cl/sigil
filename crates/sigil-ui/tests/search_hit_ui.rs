//! What a search result looks like, and that the whole of it can be pressed.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use sigil::theme;
use sigil_ui::{SearchHit, search_hit};

/// Three hits: one near the start of a short message, one deep in a long
/// one, and one chosen.
fn harness(clicked: std::rc::Rc<std::cell::Cell<usize>>) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(300.0, 260.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let t = sigil::ColorTheme::current(&ctx);
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(t.surface_primary)
                        .inner_margin(egui::Margin::same(sigil::tokens::SPACING_SM as i8)),
                )
                .show(ui, |ui| {
                    let long = format!(
                        "{}the release check is at noon, bring the notes",
                        "words before it ".repeat(8)
                    );
                    let deep = long.find("release").unwrap();
                    let hits = [
                        (
                            SearchHit {
                                id: "SearchHitFixture1",
                                picture: None,
                                label: "release check",
                                who: "Ada",
                                text: "the release is Thursday",
                                found: 4..11,
                                at: "2m",
                            },
                            false,
                        ),
                        (
                            SearchHit {
                                id: "SearchHitFixture2",
                                picture: None,
                                label: "a very long conversation name that truncates",
                                who: "You",
                                text: &long,
                                found: deep..deep + 7,
                                at: "yesterday",
                            },
                            true,
                        ),
                        (
                            SearchHit {
                                id: "SearchHitFixture3",
                                picture: None,
                                label: "Grace",
                                who: "Grace",
                                text: "no release without the notes\nand the notes are late",
                                found: 3..10,
                                at: "3d",
                            },
                            false,
                        ),
                    ];
                    for (i, (hit, selected)) in hits.iter().enumerate() {
                        if search_hit(ui, hit, *selected).clicked() {
                            clicked.set(i + 1);
                        }
                    }
                });
        })
}

/// A press at a point: down and up, as the pointer does it.
fn press_at(h: &mut Harness<'static>, at: egui::Pos2) {
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
}

/// Pressing the empty right-hand end of a row -- beside the time, where
/// there are no words -- chooses the row. A result is entirely words, and
/// a target only as wide as the words was the hole the conversation list
/// had.
#[test]
fn the_whole_row_is_the_target() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut h = harness(clicked.clone());
    h.run();
    // The first row's words and its time: the gap between them is the
    // ground being tested.
    let words = h.get_by_label_contains("Ada: the release").rect();
    let time = h.get_by_label("2m").rect();
    let gap = egui::pos2((words.right() + time.left()) / 2.0, words.center().y);
    assert!(
        gap.x > words.right() + 8.0,
        "the words reach the time; there is no gap to press: {words:?} {time:?}"
    );
    press_at(&mut h, gap);
    h.run();
    assert_eq!(clicked.get(), 1, "the press beside the words chose nothing");

    // And the ground between the third row's name and its time chooses it,
    // not the first row again.
    let name = h.get_by_label("Grace").rect();
    let time = h.get_by_label("3d").rect();
    press_at(
        &mut h,
        egui::pos2((name.right() + time.left()) / 2.0, name.center().y),
    );
    h.run();
    assert_eq!(clicked.get(), 3);
}

/// The result shows the part of the message the word is in, not its
/// beginning: the second hit's words start with an ellipsis and hold the
/// word, and the first hit is shown from its start.
#[test]
fn the_words_shown_are_around_the_match() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut h = harness(clicked);
    h.run();
    let deep = h.get_by_label_contains("You: …").rect();
    assert!(deep.height() > 0.0);
    assert!(
        h.query_by_label_contains("You: words before it").is_none(),
        "the long message is shown from its start, hiding the match"
    );
    h.get_by_label_contains("Ada: the release is Thursday");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn search_hit_dark() {
    let clicked = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut h = harness(clicked);
    h.run();
    h.snapshot("search_hit_dark");
}
