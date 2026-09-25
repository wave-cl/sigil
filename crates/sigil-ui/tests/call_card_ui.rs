//! The call card, on a phone, with nothing around it.
//!
//! # Why the widget is tested before the app is
//!
//! A card reached through a route, inside a navigator, inside a shell that
//! draws its own bar, is three things that can be wrong at once. The layout is
//! only one of them, and it is the one that has been wrong most often here: a
//! row wider than the pane re-lays every row after it, and a control drawn
//! below a scrolling roster ends up off the bottom of the screen. Both are
//! visible with no app at all.
//!
//! # What this asserts that a snapshot cannot
//!
//! A snapshot says two pictures differ. These say *what is wrong*: the card is
//! this many points too wide, the hang-up control is this far below the
//! screen, two controls overlap by this much. And they run in an ordinary
//! `cargo test` rather than in the snapshot job.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
use sigil::theme;
use sigil_ui::{Call, call_card, roster::Row};

/// The phone the app is checked on: a OnePlus NE2213, 360 points across.
const PHONE: f32 = 360.0;
const TALL: f32 = 804.0;

const LONG_NAME: &str = "Alexandra Constantinopoulos-Whitmore";
const KEY: &str = "AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9";

/// A two-party call, up, unmuted, on the earpiece.
fn plain<'a>() -> Call<'a> {
    Call {
        key: KEY,
        named: "Ada",
        picture: None,
        whose: None,
        up: true,
        deaf: false,
        seconds: 75,
        travel: Some(("direct", "")),
        muted: false,
        speaker: Some(false),
        stats: None,
        detail: false,
        present: &[],
        connecting: 0,
        two_party: true,
    }
}

fn rows(n: usize) -> Vec<Row> {
    (0..n)
        .map(|i| Row {
            key: format!("{KEY}{i}"),
            speaking: i % 3 == 0,
            level: 0.4,
            detail: "loss 0% · conceal 0 · buf 3".into(),
        })
        .collect()
}

/// Draw a card in a phone-sized pane and answer what it drew.
///
/// Returns the width the card took and every control's rect by name, so a case
/// can ask both "does it fit" and "is the way out reachable".
fn shown(call: &Call<'_>, size: egui::Vec2) -> (f32, Vec<(String, egui::Rect)>) {
    let width = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let out = width.clone();
    // `Call` borrows, so the closure gets owned copies of what it needs.
    let owned = (
        call.key.to_string(),
        call.named.to_string(),
        call.whose.map(str::to_string),
        call.travel.map(|(w, r)| (w.to_string(), r.to_string())),
        call.stats.map(str::to_string),
        rows(call.present.len()),
    );
    let flags = (
        call.up,
        call.deaf,
        call.seconds,
        call.muted,
        call.speaker,
        call.detail,
        call.connecting,
        call.two_party,
    );
    let mut h = Harness::builder().with_size(size).build_ui(move |ui| {
        let ctx = ui.ctx().clone();
        let form = if size.x <= PHONE + 1.0 {
            sigil::Form::Phone
        } else {
            sigil::Form::Desktop
        };
        sigil::Form::install(&ctx, form);
        theme::install(&ctx, theme::light(), theme::dark());
        ctx.set_theme(egui::Theme::Dark);
        let t = sigil::ColorTheme::current(&ctx);
        let margin = form.body_margin();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(t.surface_primary)
                    .inner_margin(egui::Margin::same(margin as i8)),
            )
            .show(ui, |ui| {
                let (up, deaf, seconds, muted, speaker, detail, connecting, two_party) = flags;
                let call = Call {
                    key: &owned.0,
                    named: &owned.1,
                    picture: None,
                    whose: owned.2.as_deref(),
                    up,
                    deaf,
                    seconds,
                    travel: owned.3.as_ref().map(|(w, r)| (w.as_str(), r.as_str())),
                    muted,
                    speaker,
                    stats: owned.4.as_deref(),
                    detail,
                    present: &owned.5,
                    connecting,
                    two_party,
                };
                call_card(ui, &call);
                out.set(ui.min_rect().width() + 2.0 * margin);
            });
    });
    h.run();
    h.run();

    // Walked rather than queried: this harness has no `get_all_by_label`, and
    // walking is what the other widget tests here do.
    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<(String, egui::Rect)>) {
        if let Some(label) = node.accesskit_node().label() {
            // The controls say a sentence -- "Mute your microphone" -- and are
            // found by the word the card draws under them.
            for word in ["Unmute", "Mute", "Hang up", "Speaker", "Earpiece"] {
                if label.contains(word) {
                    out.push((word.to_string(), node.rect()));
                    break;
                }
            }
        }
        for c in node.children() {
            walk(c, out);
        }
    }
    let mut found = Vec::new();
    walk(h.root(), &mut found);
    (width.get(), found)
}

/// What every case asserts: it fits across, and the way out is on the screen.
fn holds(what: &str, call: &Call<'_>, size: egui::Vec2) {
    let (width, controls) = shown(call, size);
    assert!(width > 0.0, "{what} drew nothing, so this proves nothing");
    assert!(
        width <= size.x + 1.0,
        "{what} draws {width:.0} points wide in a {:.0}-point pane",
        size.x
    );

    let out = controls
        .iter()
        .find(|(word, _)| word == "Hang up")
        .unwrap_or_else(|| panic!("{what} has no way out at all"));
    assert!(
        out.1.bottom() <= size.y + 1.0 && out.1.top() >= -1.0,
        "{what}: the way out is at y {:.0}..{:.0} of {:.0} — a call somebody \
         cannot end is a microphone that stays open",
        out.1.top(),
        out.1.bottom(),
        size.y
    );
    assert!(
        out.1.left() >= -1.0 && out.1.right() <= size.x + 1.0,
        "{what}: the way out is at x {:.0}..{:.0} of {:.0}",
        out.1.left(),
        out.1.right(),
        size.x
    );

    // A target smaller than a finger is a control people miss, and these are
    // pressed without looking.
    assert!(
        out.1.width() >= sigil::tokens::BUTTON_LG && out.1.height() >= sigil::tokens::BUTTON_LG,
        "{what}: the way out is {:.0}x{:.0}, under a thumb's worth",
        out.1.width(),
        out.1.height()
    );

    // **Two controls in one place is one control nobody aimed at.**
    for (i, (a_word, a)) in controls.iter().enumerate() {
        for (b_word, b) in controls.iter().skip(i + 1) {
            assert!(
                !a.intersects(*b),
                "{what}: {a_word:?} and {b_word:?} overlap — a press lands on \
                 whichever egui put last"
            );
        }
    }
}

#[test]
fn a_two_party_call_fits_a_phone() {
    holds("a plain call", &plain(), egui::vec2(PHONE, TALL));
}

#[test]
fn a_long_name_does_not_widen_the_card() {
    holds(
        "a call with a long name",
        &Call {
            named: LONG_NAME,
            ..plain()
        },
        egui::vec2(PHONE, TALL),
    );
}

#[test]
fn a_muted_call_fits() {
    holds(
        "a muted call",
        &Call {
            muted: true,
            speaker: Some(true),
            ..plain()
        },
        egui::vec2(PHONE, TALL),
    );
}

/// **The case the bottom panel is for.** Drawn in the flow, twelve people push
/// the controls past the foot of the screen; sigil-voice's own roster did
/// exactly this and its comment records it.
#[test]
fn a_room_of_twelve_does_not_push_the_way_out_off_the_screen() {
    let present = rows(12);
    holds(
        "a room of twelve",
        &Call {
            present: &present,
            connecting: 2,
            two_party: false,
            ..plain()
        },
        egui::vec2(PHONE, TALL),
    );
}

#[test]
fn a_deaf_call_says_so_and_still_fits() {
    holds(
        "a deaf call",
        &Call {
            deaf: true,
            stats: Some("sent 854 · recv 0 · loss 0.0% · concealed 0 · underruns 1"),
            ..plain()
        },
        egui::vec2(PHONE, TALL),
    );
}

/// A phone held sideways is 360 points tall, and that is where a control drawn
/// below a scrolling body goes off the bottom.
#[test]
fn a_call_fits_a_phone_lying_down() {
    holds("a call lying down", &plain(), egui::vec2(TALL, PHONE));
}

#[test]
fn a_call_without_routing_draws_no_routing_control() {
    let (_, controls) = shown(
        &Call {
            speaker: None,
            ..plain()
        },
        egui::vec2(PHONE, TALL),
    );
    let routing: Vec<_> = controls
        .iter()
        .filter(|(w, _)| w == "Speaker" || w == "Earpiece")
        .collect();
    assert!(
        routing.is_empty(),
        "a device that cannot route drew a routing control anyway: {routing:?}"
    );
    assert!(
        controls.iter().any(|(w, _)| w == "Hang up"),
        "and it lost the way out too, so the case above proves nothing"
    );
}

#[test]
fn the_card_fits_a_desktop_pane() {
    holds("a call on a desktop", &plain(), egui::vec2(1000.0, 700.0));
}
