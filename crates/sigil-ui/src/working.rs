//! Something is happening, said without holding the machine awake.

use sigil::{ColorTheme, tokens};

/// How often the mark moves. Four times a second reads as working; sixty times
/// a second reads the same and costs fifteen times as much.
const STEP: f32 = 0.25;

/// Three dots, one of them lit, going round.
///
/// # Why not `egui::Spinner`
///
/// A spinner calls `request_repaint()` on every pass "because it is animated",
/// which pins the whole window at the display's refresh rate for as long as one
/// is on screen — and sigil's two loading states are on screen for the whole of
/// startup and for every conversation that has not been fetched yet. egui is
/// otherwise reactive: with nothing asking, it sleeps.
///
/// This asks for the *next step* instead of the next frame, so the window wakes
/// four times a second while something is loading and not at all afterwards.
pub fn working(ui: &mut egui::Ui) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    let size = egui::vec2(tokens::SPACING_XL, tokens::SPACING_SM);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        // The step this is on, from the clock rather than from a counter: a
        // counter would need somewhere to live and would run at whatever rate
        // the window happened to be repainting for other reasons.
        let now = ui.input(|i| i.time) as f32;
        let lit = ((now / STEP) as usize) % 3;
        let radius = tokens::SPACING_XS / 2.0;
        for i in 0..3 {
            let at = egui::pos2(
                rect.left() + radius + (rect.width() - radius * 2.0) * (i as f32 / 2.0),
                rect.center().y,
            );
            let colour = if i == lit {
                theme.accent
            } else {
                theme.text_muted
            };
            ui.painter().circle_filled(at, radius, colour);
        }
        // The next step, not the next frame.
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f32(STEP));
    }
    response
}
