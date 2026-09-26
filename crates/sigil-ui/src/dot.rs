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
pub fn dot(
    ui: &mut egui::Ui,
    filled: bool,
    on: egui::Color32,
    off: egui::Color32,
    label: &str,
) -> egui::Response {
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
    // Returned so a caller can hang the word off it. A dot that says nothing
    // anywhere is a colour, and a colour is not a message.
    response.on_hover_text(said)
}

/// Draw a state as an icon, at the height of the line it sits on.
///
/// The sibling of [`dot`] for a state that has a shape of its own. A dot says
/// on or off and needs the words beside it to say *what*; a struck-through
/// microphone says which state it is without a legend, which is what a strip
/// with no room for a legend needs.
///
/// **It carries a word too, for the same reason `dot` does.** This is not a
/// control -- it senses hover so it can be explained and nothing more -- so
/// the word is the only thing a reader who cannot see the shape is given, and
/// the only thing a test can assert without comparing pixels.
pub fn state_icon(
    ui: &mut egui::Ui,
    icon: sigil::Icon,
    colour: egui::Color32,
    said: &str,
) -> egui::Response {
    let side = ui.text_style_height(&egui::TextStyle::Body);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    sigil::icon::draw(ui.painter(), rect, icon, colour);
    let word = said.to_string();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &word));
    response.on_hover_text(word)
}
