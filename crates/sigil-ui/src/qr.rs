//! A QR code, painted. For the safety code two people compare (SIP-41):
//! the text is short and fixed, so the smallest version that holds it is
//! the one drawn, at whatever size the caller has.

use sigil::tokens;

/// Paint `text` as a QR filling a square of `size`, dark on the surface,
/// with the quiet zone the standard asks for. Returns the response for the
/// square, carrying `text` as its accessibility value so what the picture
/// says is readable without a camera.
pub fn qr(ui: &mut egui::Ui, text: &str, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let said = text.to_string();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, &said));
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else {
        return response;
    };
    if !ui.is_rect_visible(rect) {
        return response;
    }
    // Always dark on light, whatever the theme: a camera reads contrast,
    // and an inverted code reads to fewer of them.
    let painter = ui.painter();
    painter.rect_filled(rect, tokens::RADIUS_SM, egui::Color32::WHITE);
    let width = code.width();
    let quiet = 4;
    let cells = width + 2 * quiet;
    let cell = size / cells as f32;
    let dark = egui::Color32::BLACK;
    for (i, colour) in code.to_colors().iter().enumerate() {
        if *colour != qrcode::Color::Dark {
            continue;
        }
        let x = (i % width + quiet) as f32;
        let y = (i / width + quiet) as f32;
        let min = rect.min + egui::vec2(x * cell, y * cell);
        painter.rect_filled(
            egui::Rect::from_min_size(min, egui::vec2(cell, cell)).expand(0.2),
            0.0,
            dark,
        );
    }
    response
}
