//! What the roster looks like, with state a live call would be needed to reach.
//!
//! Keeping the widget a pure function of plain data is what makes this
//! possible: a room with somebody speaking, somebody silent and somebody still
//! connecting is three lines to construct here and a five-person meeting to
//! arrange otherwise.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use egui_kittest::kittest::Queryable;
use sigil::theme;
use sigil_ui::{Row, roster};

/// **One with a name and one without**, because both happen in the same room:
/// the chat app fills a name for everybody it has a profile for and nobody
/// else, and sigil-voice fills none at all. A fixture where every row is named
/// would let the unnamed branch rot.
fn rows() -> Vec<Row> {
    vec![
        Row {
            key: "3yMhjNhZ8kLpQr2vWx7TnBcDfGhJkLmNpQrStUvWxYz1".into(),
            named: Some("Ada".into()),
            picture: None,
            speaking: true,
            level: 0.42,
            detail: "loss 0% · conceal 0 · buf 3".into(),
        },
        Row {
            key: "GkpAfVhY4jNmRt6uXz9QwErTyUiOpAsDfGhJkLzXcVbN".into(),
            named: None,
            picture: None,
            speaking: false,
            level: 0.01,
            detail: "loss 2% · conceal 4 · buf 3".into(),
        },
    ]
}

fn harness(rows: Vec<Row>, connecting: usize) -> Harness<'static> {
    sized(
        rows,
        connecting,
        egui::vec2(900.0, 260.0),
        sigil::Form::Desktop,
    )
}

/// The same roster on a phone. Its rows carry a key, a meter and a sentence
/// about the path, which at 360 points is more than a row holds.
fn harness_phone(rows: Vec<Row>, connecting: usize) -> Harness<'static> {
    sized(
        rows,
        connecting,
        egui::vec2(360.0, 320.0),
        sigil::Form::Phone,
    )
}

fn sized(
    rows: Vec<Row>,
    connecting: usize,
    size: egui::Vec2,
    form: sigil::Form,
) -> Harness<'static> {
    with_detail(rows, connecting, size, form, true)
}

/// The same, with the per-row numbers open or shut. Every case above wants
/// them open -- the pane they were written for always shows them, and the
/// row-height rule below is about a row that is two lines tall -- so `sized`
/// keeps that and this is here for the one case that is about them being shut.
fn with_detail(
    rows: Vec<Row>,
    connecting: usize,
    size: egui::Vec2,
    form: sigil::Form,
    detail: bool,
) -> Harness<'static> {
    Harness::builder().with_size(size).build_ui(move |ui| {
        let ctx = ui.ctx().clone();
        sigil::Form::install(&ctx, form);
        theme::install(&ctx, theme::light(), theme::dark());
        ctx.set_theme(egui::Theme::Dark);
        let t = sigil::ColorTheme::current(&ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(t.surface_primary)
                    .inner_margin(egui::Margin::same(form.body_margin() as i8)),
            )
            .show(ui, |ui| roster(ui, &rows, connecting, detail));
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

/// The roster on a phone: the path's detail under its row rather than after
/// it, because after it was 572 points in a 360-point pane.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn roster_phone() {
    let mut rows = rows();
    // What the engine really reports, rather than the abbreviation the
    // desktop fixture uses: this is the string that did not fit.
    rows[0].detail = "2.1% lost, 180 ms of buffer, concealing 3 frames in 100".into();
    let mut h = harness_phone(rows, 1);
    h.run();
    h.snapshot("roster_phone");
}

/// **The mark costs width, not height.** A phone's roster rows are brought
/// down to a single line on purpose: a room of eight is eight rows on a
/// 360-point screen, and the rule that does it is two lines above the one
/// that draws the mark. A mark at `AVATAR_SM` would quietly undo it for every
/// row in the room, which is the mistake this pins.
#[test]
fn a_mark_does_not_make_the_rows_taller() {
    let rows = rows();
    let (a, b) = (sigil_ui::short(&rows[0].key), sigil_ui::short(&rows[1].key));
    let h = harness_phone(rows, 0);
    let first = h.get_by_label(&a).rect();
    let second = h.get_by_label(&b).rect();
    let pitch = second.min.y - first.min.y;
    // Measured both ways before this number was chosen: 38.4 points apart
    // with the mark at the line's height, 45.2 with it at `AVATAR_SM`, which
    // grows the key line from 15 points to 24. The bound sits between them,
    // and the 45.2 was read from this assertion failing, not predicted.
    assert!(
        pitch > 0.0 && pitch < 42.0,
        "roster rows {pitch} points apart on a phone. A row here is two lines \
         of text and the spacings around them; a mark taller than the line it \
         sits beside makes every row in the room taller."
    );
}

/// **A roster drawn where the numbers are shut does not show them.**
///
/// `roster` is drawn in two places. sigil-voice's pane is where somebody goes
/// to ask how the path is holding up, and there the numbers are the point. A
/// call card is not that screen: it already draws the call's own engine line
/// only when the numbers are open or nothing is arriving, and it drew a
/// per-person copy of the same numbers under every face in the room whatever
/// that rule said -- twelve people, twelve lines of loss and buffer depth, at
/// twice the height of the roster itself.
///
/// Both halves matter, so both are asserted: the line is gone from the tree,
/// and the rows it was padding out are shorter for it.
#[test]
fn the_numbers_are_the_callers_to_open() {
    let fixture = rows();
    let (a, b) = (
        sigil_ui::short(&fixture[0].key),
        sigil_ui::short(&fixture[1].key),
    );
    let detail = fixture[0].detail.clone();

    let open = harness_phone(fixture, 0);
    assert!(
        text_of(&open).contains(&detail),
        "with the numbers open the row says {detail:?}: {}",
        text_of(&open)
    );
    let open_pitch = open.get_by_label(&b).rect().min.y - open.get_by_label(&a).rect().min.y;

    let shut = with_detail(
        rows(),
        0,
        egui::vec2(360.0, 320.0),
        sigil::Form::Phone,
        false,
    );
    assert!(
        !text_of(&shut).contains(&detail),
        "with them shut it does not: {}",
        text_of(&shut)
    );
    let shut_pitch = shut.get_by_label(&b).rect().min.y - shut.get_by_label(&a).rect().min.y;
    assert!(
        shut_pitch > 0.0 && shut_pitch < open_pitch,
        "rows {shut_pitch} points apart with the numbers shut and {open_pitch} with them \
         open. Hiding the line has to give the height back, or the room still fits as \
         few people on a screen as it did."
    );
}

/// **A name where there is one, and the key either way.**
///
/// The row type is shared with sigil-voice, which joins a room by key and
/// keeps no profiles, so it carried a key and nothing else -- and the chat
/// app, which has a name and a face for everybody in the room, filled it the
/// same way. A room call named its channel at the top of the card and spelt
/// its members in base58 six rows down.
///
/// The key does not go away when a name arrives. A name is an assertion and a
/// key is not (SIP-21), and on a phone this is one of the few places the
/// person's own key is written down at all.
#[test]
fn a_name_joins_the_key_rather_than_replacing_it() {
    let fixture = rows();
    let (named, anonymous) = (fixture[0].key.clone(), fixture[1].key.clone());
    let h = harness_phone(fixture, 0);
    let said = text_of(&h);

    assert!(
        said.contains("Ada"),
        "the one with a profile is named: {said}"
    );
    assert!(
        said.contains(&sigil_ui::short(&named)),
        "and still carries their key: {said}"
    );
    assert!(
        said.contains(&sigil_ui::short(&anonymous)),
        "the one without a profile is still their key: {said}"
    );
    // Not the name of somebody else's row: the two are zipped together in the
    // caller and a fencepost there would name the wrong person to their face.
    let ada = h.get_by_label("Ada").rect();
    let theirs = h.get_by_label(&sigil_ui::short(&named)).rect();
    let other = h.get_by_label(&sigil_ui::short(&anonymous)).rect();
    assert!(
        (ada.center().y - theirs.center().y).abs() < 1.0,
        "the name sits on its own row: name at {}, key at {}",
        ada.center().y,
        theirs.center().y
    );
    assert!(
        ada.center().y < other.center().y,
        "and not on the next one down"
    );
}
