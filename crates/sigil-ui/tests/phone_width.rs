//! Every list row fits a phone, whatever is in it.
//!
//! # Why this is a test and not a snapshot
//!
//! egui grows a `Ui` to whatever is drawn in it, so a row wider than the pane
//! does not merely stick out: every row after it is laid out for a pane that
//! wide. In the chat app that put a member's buttons on top of the member
//! above them and started the section below off the left edge -- a symptom
//! nowhere near its cause. The width of the ui after a pass *is* that fault
//! in one number, and reading it needs no renderer, so it runs in an ordinary
//! `cargo test` rather than in the snapshot job somebody runs before a
//! release.
//!
//! # Why the data is long
//!
//! The fixtures elsewhere are "Ada" and "notes.txt". Nothing in them is
//! longer than a phone, so every pane could pass a width check and still be
//! torn apart by the first person with a long display name. Nothing here is
//! invented for the test: a name is whatever somebody typed, a preview is
//! whatever they said, and a URL is a single word with no break in it, which
//! is the one shape wrapping cannot help.

use egui_kittest::Harness;
use sigil::theme;

/// The phone the app is checked on: a OnePlus NE2213, 360 points across.
/// Not a Pixel's 412 -- at 412 every phone test passed while the phone
/// overflowed.
const PHONE: f32 = 360.0;

const LONG_NAME: &str = "Alexandra Constantinopoulos-Whitmore";
const URL: &str =
    "https://example.org/a/very/long/path/that/never/breaks?and=a&query=string&too=yes";

/// Draw `add` in a phone-wide pane and answer how wide it came out.
fn drawn(add: impl Fn(&mut egui::Ui) + 'static) -> f32 {
    drawn_at(add, 1.0)
}

/// The same, at the reader's text size.
///
/// **The axis this file was missing.** Every case here was drawn at sigil's
/// own size and no other, and the phone honours `Configuration.fontScale` —
/// so a row that fits is a row that fits *once*, and the reader who most
/// needs the words bigger is the one the layout was never checked for.
/// `sigil-ui`'s call card had no scaled case either and overflowed a
/// 360-point pane by 94 points at 1.3x.
fn drawn_at(add: impl Fn(&mut egui::Ui) + 'static, scale: f32) -> f32 {
    let width = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let out = width.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(PHONE, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
            sigil::TextScale::install(&ctx, scale);
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let t = sigil::ColorTheme::current(&ctx);
            let margin = sigil::Form::Phone.body_margin();
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(t.surface_primary)
                        .inner_margin(egui::Margin::same(margin as i8)),
                )
                .show(ui, |ui| {
                    add(ui);
                    out.set(ui.min_rect().width() + 2.0 * margin);
                });
        });
    h.run();
    h.run();
    width.get()
}

/// What every case here asserts, at every size the reader can ask for.
///
/// **The sizes are the reader's, not a sample.** 1.0 is sigil's own; 1.3 is
/// where the phone's slider sits one notch up and where the chat app's panes
/// were once 45 widgets off the screen; 2.0 is the top of `TextScale`'s
/// range, and a row that survives it survives anything between.
const SIZES: [f32; 3] = [1.0, 1.3, 2.0];

fn fits_at_every_size(what: &str, add: impl Fn(&mut egui::Ui) + Clone + 'static) {
    let mut over: Vec<String> = Vec::new();
    for scale in SIZES {
        let width = drawn_at(add.clone(), scale);
        assert!(
            width > 0.0,
            "{what} drew nothing at {scale}x, so this proves nothing about it"
        );
        // A point of slack for the rounding egui does on a margin; the faults
        // this catches were tens of points, not fractions.
        if width > PHONE + 1.0 {
            over.push(format!("{width:.0} points at {scale}x"));
        }
    }
    assert!(
        over.is_empty(),
        "{what} runs off a {PHONE}-point pane: {}",
        over.join(", ")
    );
}

#[test]
fn a_conversation_row_with_a_long_name_fits() {
    fits_at_every_size("a conversation row", |ui| {
        sigil_ui::conversation_row(
            ui,
            &sigil_ui::ConversationRow {
                id: "AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9",
                label: LONG_NAME,
                preview: URL,
                at: "11:59",
                unread: 128,
                public: Some(true),
                group: true,
                waiting: false,
                typing: false,
                mentioned: true,
                muted: true,
                presence: None,
                verified: true,
                picture: None,
            },
            false,
        );
    });
}

#[test]
fn a_search_hit_with_a_long_name_fits() {
    fits_at_every_size("a search hit", |ui| {
        sigil_ui::search_hit(
            ui,
            &sigil_ui::SearchHit {
                id: "SearchHitOnAPhone",
                picture: None,
                label: LONG_NAME,
                who: LONG_NAME,
                text: URL,
                // The match, somewhere in the middle of the unbroken word.
                found: 30..37,
                at: "Thu",
            },
            false,
        );
    });
}

#[test]
fn a_roster_row_with_a_long_detail_fits() {
    fits_at_every_size("a roster", |ui| {
        sigil_ui::roster(
            ui,
            &[sigil_ui::Row {
                key: "AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9".to_string(),
                // A name as long as anybody types, beside a key and a
                // meter: the row now carries three things, and this is the
                // case that says they still fit a phone.
                named: Some(LONG_NAME.to_string()),
                picture: None,
                speaking: true,
                level: 0.7,
                detail: "2.1% lost, 180 ms of buffer, concealing 3 frames in 100".to_string(),
            }],
            2,
            // Open: the long detail line is the whole point of this case.
            true,
        );
    });
}

/// The instrument can say no.
///
/// Every case above passes, which on its own is as consistent with a broken
/// harness as with a sound layout -- and a harness that measures nothing
/// measures nothing quietly. This draws something that genuinely does not
/// fit, and fails if the measurement shrugs.
#[test]
fn the_measurement_notices_something_too_wide() {
    let width = drawn(|ui| {
        ui.horizontal(|ui| {
            for _ in 0..6 {
                ui.label(LONG_NAME);
            }
        });
    });
    assert!(
        width > PHONE,
        "six long names in a row that never wraps measured {width} in a \
         {PHONE}-point pane: the measurement is not seeing what is drawn"
    );
}

/// **And the size axis bites.**
///
/// The cases above run three times, and three green results mean nothing
/// unless a row that fits at sigil's own size and not at the reader's is
/// caught. So: a word with no break in it, short enough for a phone at 1.0
/// and too long at 2.0 — the shape wrapping cannot help, drawn at a size the
/// reader chooses.
///
/// This is the fault the call card had, in miniature: it fitted when it was
/// measured and not when it was read.
#[test]
fn the_measurement_notices_something_that_only_overflows_when_the_text_grows() {
    // Forty characters, no spaces: at sigil's own size that is most of a
    // phone's width, and at twice it is nearly two phones.
    const UNBREAKABLE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let show = |ui: &mut egui::Ui| {
        ui.add(egui::Label::new(UNBREAKABLE).wrap_mode(egui::TextWrapMode::Extend));
    };
    let small = drawn_at(show, 1.0);
    let large = drawn_at(show, 2.0);
    assert!(
        small <= PHONE + 1.0,
        "the control does not fit at sigil's own size either ({small} points), \
         so it says nothing about the size axis"
    );
    assert!(
        large > PHONE + 1.0,
        "a row that only overflows when the text is turned up went unnoticed: \
         {large} points at 2x in a {PHONE}-point pane"
    );
}
