//! What the roster looks like, with state a live call would be needed to reach.
//!
//! Keeping the widget a pure function of plain data is what makes this
//! possible: a room with somebody speaking, somebody silent and somebody still
//! connecting is three lines to construct here and a five-person meeting to
//! arrange otherwise.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use sigil::theme;
use sigil_ui::{Row, roster};

fn rows() -> Vec<Row> {
    vec![
        Row {
            key: "3yMhjNhZ8kLpQr2vWx7TnBcDfGhJkLmNpQrStUvWxYz1".into(),
            speaking: true,
            level: 0.42,
            detail: "loss 0% · conceal 0 · buf 3".into(),
        },
        Row {
            key: "GkpAfVhY4jNmRt6uXz9QwErTyUiOpAsDfGhJkLzXcVbN".into(),
            speaking: false,
            level: 0.01,
            detail: "loss 2% · conceal 4 · buf 3".into(),
        },
    ]
}

fn harness(rows: Vec<Row>, connecting: usize) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(900.0, 260.0))
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
                .show(ui, |ui| roster(ui, &rows, connecting));
        })
}

fn text_of(h: &Harness<'static>) -> String {
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
}

#[test]
fn every_key_is_shown_in_full() {
    let expected = rows();
    let mut h = harness(rows(), 0);
    h.run();
    let said = text_of(&h);
    for row in &expected {
        assert!(
            said.contains(&row.key),
            "keys are never abbreviated away (SIP-21): {said}"
        );
    }
}

/// Somebody in the room who cannot yet be heard is a different thing from
/// somebody absent, and it is exactly what you want to know when you cannot
/// hear a person you were told was here.
#[test]
fn members_who_are_not_yet_connected_are_counted_separately() {
    let mut h = harness(rows(), 2);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("2 more in the room, not yet connected"),
        "{said}"
    );
}

#[test]
fn an_empty_room_says_which_kind_of_empty_it_is() {
    let mut h = harness(Vec::new(), 0);
    h.run();
    assert!(text_of(&h).contains("Nobody else here yet"));

    let mut h = harness(Vec::new(), 3);
    h.run();
    assert!(
        text_of(&h).contains("Connecting to 3"),
        "waiting to connect is not the same as being alone"
    );
}

/// The speaking state must survive being read without colour — filled versus
/// hollow, not green versus grey — and it must say so in words, because a
/// screen reader announcing "black circle" helps nobody.
#[test]
fn speaking_is_said_in_words_not_only_in_colour() {
    let mut h = harness(rows(), 0);
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("speaking"),
        "the talker is named as such: {said}"
    );
    assert!(said.contains("silent"), "and so is the listener: {said}");
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn roster_dark() {
    let mut h = harness(rows(), 1);
    h.run();
    h.snapshot("roster_dark");
}
