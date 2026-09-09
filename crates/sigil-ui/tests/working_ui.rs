//! The mark that says something is loading, and what it costs to draw.

use egui_kittest::Harness;
use sigil::theme;

fn drawn(with: impl Fn(&mut egui::Ui) + 'static) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(200.0, 80.0))
        // **A realistic frame time.** egui subtracts the predicted frame time
        // from any delay asked for, so with the harness's default of a quarter
        // of a second every delay shorter than that comes out as "now" — and a
        // test of a delay would be measuring the harness.
        .with_step_dt(1.0 / 60.0)
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            with(ui);
        })
}

fn asked_to_repaint_after(h: &Harness<'static>) -> std::time::Duration {
    h.output()
        .viewport_output
        .values()
        .map(|v| v.repaint_delay)
        .min()
        .expect("a viewport")
}

/// The loading mark asks for its next step, not for the next frame.
///
/// `egui::Spinner` requests a repaint on every pass "because it is animated",
/// which pins the whole window at the display's refresh rate for as long as one
/// is on screen — and sigil's two loading states are on screen for the whole of
/// startup and for every conversation not yet fetched. egui is otherwise
/// reactive: with nothing asking, it sleeps.
///
/// Measured against the spinner it replaced, in the same harness, so this says
/// something about the two rather than about a number I chose.
#[test]
fn the_loading_mark_does_not_hold_the_window_awake() {
    let mut spun = drawn(|ui| {
        ui.spinner();
    });
    spun.step();
    assert_eq!(
        asked_to_repaint_after(&spun),
        std::time::Duration::ZERO,
        "a spinner is supposed to ask for the very next frame; if it has \
         stopped doing that, this test is comparing against nothing"
    );

    let mut marked = drawn(|ui| {
        sigil_ui::working(ui);
    });
    marked.step();
    let after = asked_to_repaint_after(&marked);
    assert!(
        after >= std::time::Duration::from_millis(100),
        "the loading mark asked to be repainted in {after:?}, which is as often \
         as the spinner it replaced"
    );
}

/// And it stops asking when it is not drawn.
#[test]
fn nothing_is_asked_for_when_nothing_is_loading() {
    let mut still = drawn(|ui| {
        ui.label("nothing is happening");
    });
    still.step();
    assert!(
        asked_to_repaint_after(&still) > std::time::Duration::from_secs(1),
        "a window with nothing on it asked to be woken again"
    );
}
