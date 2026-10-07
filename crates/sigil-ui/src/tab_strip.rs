//! One row of named places, with the one you are in marked.
//!
//! A timeline has more than one thing to show and no room for a second
//! heading, which is what a tab strip is for: the places are named together,
//! so a reader can see that the other one exists without going looking, and
//! moving between them is one press rather than a menu.
//!
//! **Selected is drawn and said.** The mark is an underline, because a
//! colour alone leaves somebody who cannot see it unable to tell which of
//! them they are looking at — the same rule `icon_button_as_named` follows,
//! and for the same reason.

use sigil::theme::ColorTheme;
use sigil::tokens;

/// How thick the mark under the chosen tab is.
const UNDERLINE: f32 = 2.0;

/// How tall a tab is, before the mark.
///
/// A tab is tapped, so what decides this is a phone's smallest comfortable
/// target rather than the height of the words.
const TALL: f32 = 44.0;

/// Draw the strip. Returns the index pressed, if one was.
///
/// `chosen` is the index currently shown; it is drawn marked and still
/// answers a press, because pressing the tab you are on is how a timeline is
/// sent back to the top on every phone.
pub fn tab_strip(ui: &mut egui::Ui, names: &[&str], chosen: usize) -> Option<usize> {
    if names.is_empty() {
        return None;
    }
    let theme = ColorTheme::current(ui.ctx());
    let mut pressed = None;
    // **Equal shares of the row, measured once.** Laying them out left to
    // right with their own widths puts the mark under a different place for
    // every set of names, and a strip whose geometry depends on the words is
    // one that moves when a translation lands.
    let room = ui.available_width();
    let each = room / names.len() as f32;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, name) in names.iter().enumerate() {
            let (rect, press) =
                ui.allocate_exact_size(egui::vec2(each, TALL), egui::Sense::click());
            let here = i == chosen;
            press.widget_info(|| {
                // `WidgetType::Button`, which is what `icon_button_as_named`
                // uses and what actually reaches the tree as selected. A
                // `SelectableLabel` carries the state as `toggled` instead,
                // and the widget test that reads `is_selected` found the
                // strip saying nothing about which tab it was on.
                egui::WidgetInfo::selected(egui::WidgetType::Button, true, here, *name)
            });
            if ui.is_rect_visible(rect) {
                let ink = if here {
                    theme.text_primary
                } else {
                    theme.text_secondary
                };
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    name,
                    egui::FontId::proportional(ui.style().text_styles[&egui::TextStyle::Body].size),
                    ink,
                );
                if here {
                    // Under the words rather than across the whole share, so
                    // the mark points at the name and not at the column.
                    let wide = (each * 0.6).min(rect.width());
                    let y = rect.bottom() - UNDERLINE;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_size(
                            egui::pos2(rect.center().x - wide / 2.0, y),
                            egui::vec2(wide, UNDERLINE),
                        ),
                        tokens::RADIUS_SM,
                        theme.accent,
                    );
                }
            }
            if press.clicked() {
                pressed = Some(i);
            }
        }
    });
    ui.separator();
    pressed
}
