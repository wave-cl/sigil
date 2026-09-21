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
    let width = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let out = width.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(PHONE, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::Form::install(&ctx, sigil::Form::Phone);
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

/// What every case here asserts.
fn fits(what: &str, width: f32) {
    assert!(
        width > 0.0,
        "{what} drew nothing, so this proves nothing about it"
    );
    // A point of slack for the rounding egui does on a margin; the faults
    // this catches were tens of points, not fractions.
    assert!(
        width <= PHONE + 1.0,
        "{what} draws {width} points wide in a {PHONE}-point pane"
    );
}

#[test]
fn a_conversation_row_with_a_long_name_fits() {
    fits(
        "a conversation row",
        drawn(|ui| {
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
                },
                false,
            );
        }),
    );
}

#[test]
fn a_search_hit_with_a_long_name_fits() {
    fits(
        "a search hit",
        drawn(|ui| {
            sigil_ui::search_hit(
                ui,
                &sigil_ui::SearchHit {
                    label: LONG_NAME,
                    who: LONG_NAME,
                    text: URL,
                    // The match, somewhere in the middle of the unbroken word.
                    found: 30..37,
                    at: "Thu",
                },
                false,
            );
        }),
    );
}

#[test]
fn a_roster_row_with_a_long_detail_fits() {
    fits(
        "a roster",
        drawn(|ui| {
            sigil_ui::roster(
                ui,
                &[sigil_ui::Row {
                    key: "AKnL4NNf3DGWZJS6cPknBuEGnVsV4A4m5tgebLHaRSZ9".to_string(),
                    speaking: true,
                    level: 0.7,
                    detail: "2.1% lost, 180 ms of buffer, concealing 3 frames in 100".to_string(),
                }],
                2,
            );
        }),
    );
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
