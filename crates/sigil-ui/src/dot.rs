//! A small status disc: filled or hollow, and always said in words.

use sigil::tokens;

/// Draw a status dot.
///
/// **Painted, not written.** The obvious spelling is a `●` and a `○`, and the
/// first is not in the default font — it renders as a tofu box. The
/// accessibility assertions cannot see that, so it passed twice before a
/// snapshot caught it.
///
/// **Filled versus hollow, not one colour versus another.** The state has to
/// survive being read by somebody who cannot tell the two colours apart.
///
/// **And it carries a word.** A screen reader announcing "black circle" helps
/// nobody, and every caller here has something specific to say — "speaking",
/// "connected" — that is more useful than the shape.
pub fn dot(ui: &mut egui::Ui, filled: bool, on: egui::Color32, off: egui::Color32, label: &str) {
    let size = egui::vec2(tokens::SPACING_MD, tokens::SPACING_MD);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let radius = tokens::SPACING_XS + 1.0;
    if filled {
        ui.painter().circle_filled(rect.center(), radius, on);
    } else {
        ui.painter().circle_stroke(
            rect.center(),
            radius,
            egui::Stroke::new(tokens::STROKE_MEDIUM, off),
        );
    }
    let said = label.to_string();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &said));
}
