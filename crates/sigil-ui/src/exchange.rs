//! The exchange control in the window's title strip: which exchange an
//! identity is looking at, and the menu to look at another.
//!
//! One control, drawn by every app that acts on an exchange -- the chat and
//! the console -- over one shared answer, which lives with the identity in
//! `Accounts`. Two apps each keeping their own idea of "the exchange" gave
//! the console no way to be pointed anywhere but the default, while the
//! chat beside it was looking somewhere else.

use sigil::{ColorTheme, tokens};

/// One exchange the identity holds, as the menu lists it.
pub struct ExchangeRow {
    /// The roster's name: `""` for the default.
    pub name: String,
    /// What to call it: the domain, or a short key when there is none.
    pub label: String,
    /// Whether it can be taken back from here. The default cannot: it is
    /// not a name in the roster but what the identity resolves to, and there
    /// would be nothing to remove.
    pub removable: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct ExchangeAction {
    /// Look at this one.
    pub chosen: Option<String>,
    /// Stop connecting to this one.
    pub removed: Option<String>,
    /// Open the add-a-domain dialog.
    pub add: bool,
}

/// The control -- the shown exchange's name and a chevron, pressed as one
/// thing -- and its menu. Laid out right to left, so call it from a
/// right-to-left scope at the strip's right-hand end.
pub fn exchange_control(
    ui: &mut egui::Ui,
    theme: &ColorTheme,
    shown: &str,
    selected: &str,
    rows: &[ExchangeRow],
    offer_add: bool,
) -> ExchangeAction {
    let mut action = ExchangeAction::default();
    // The chevron is painted, not typed -- `▾` is in the same block of the
    // font as the diamond that drew as nothing, and the rule since is that a
    // mark is painted. The layout here is right to left, so the chevron is
    // placed first and lands against the window's edge, with the name to
    // its left.
    let control = ui.scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
        // Or the label takes the press for its own text selection.
        ui.style_mut().interaction.selectable_labels = false;
        let side = ui.text_style_height(&egui::TextStyle::Small);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
        if ui.is_rect_visible(rect) {
            sigil::icon::draw(
                ui.painter(),
                rect,
                sigil::Icon::Chevron,
                theme.text_secondary,
            );
        }
        ui.add(
            egui::Label::new(
                egui::RichText::new(shown)
                    .small()
                    .color(theme.text_secondary),
            )
            .truncate(),
        );
    });
    let button = control.response;
    if button.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // Named for the tree: a painted chevron and a domain say nothing to a
    // screen reader about what pressing them does.
    button.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Exchange"));
    let button = button.on_hover_text("The exchange this identity is looking at");

    // Hung from the control's right-hand end, because the control is at the
    // window's: opened from its left edge the menu ran across the pane. And
    // **no wider than it needs**: the Remove control on each row is laid out
    // from the right, and "the right" of a menu with no maximum is wherever
    // the window ends.
    egui::Popup::menu(&button)
        .align(egui::RectAlign::BOTTOM_END)
        .show(|ui| {
            ui.set_min_width(200.0);
            ui.set_max_width(260.0);
            for row in rows {
                let is_selected = row.name == selected;
                let (chosen, removed) =
                    exchange_row(ui, theme, &row.label, is_selected, row.removable);
                if chosen && !is_selected {
                    action.chosen = Some(row.name.clone());
                    ui.close();
                }
                if removed {
                    action.removed = Some(row.name.clone());
                    ui.close();
                }
            }
            if offer_add {
                ui.separator();
                // The same shape as the rows above it, or it reads as a
                // caption under them rather than as one more thing to press.
                if sigil::icon::icon_item(ui, sigil::Icon::Plus, "Add a domain…").clicked() {
                    action.add = true;
                    ui.close();
                }
            }
        });
    action
}

/// One exchange in the menu: a row the width of the menu, and the way to
/// remove it, on the same centre line.
///
/// Not a `selectable_label`, which highlights the words and nothing else --
/// a pill in the corner of a row rather than a row -- and beside which the
/// remove control sat a full button's height lower, because a `horizontal`
/// centres each thing against the height it knew when that thing was placed.
/// One rectangle, allocated first at the height of the taller of the two, and
/// both drawn into it.
///
/// Returns (chosen, removed).
fn exchange_row(
    ui: &mut egui::Ui,
    theme: &ColorTheme,
    label: &str,
    selected: bool,
    removable: bool,
) -> (bool, bool) {
    // A finger's row on a phone, a pointer's in a window: this is a menu
    // reached from the app bar, where the other rows are already the
    // form's size, and a 34-point row between them was the one thing here
    // aimed at a mouse.
    let height = sigil::Form::of(ui.ctx()).button_size();
    let (rect, row) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    row.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label));
    if ui.is_rect_visible(rect) {
        if selected {
            ui.painter()
                .rect_filled(rect, tokens::RADIUS_SM, theme.interactive_hover);
        } else if row.hovered() {
            ui.painter().rect_filled(
                rect,
                tokens::RADIUS_SM,
                theme.interactive_hover.gamma_multiply(0.5),
            );
        }
        let colour = if selected {
            theme.accent
        } else {
            theme.text_primary
        };
        ui.painter().text(
            egui::pos2(rect.left() + tokens::SPACING_SM, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::TextStyle::Body.resolve(ui.style()),
            colour,
        );
    }
    if row.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let mut removed = false;
    if removable {
        // Drawn **into** the row's rectangle, at its right, after the row --
        // so it is on top and wins the press over the row underneath it, and
        // so its centre is the row's centre rather than a centre of its own.
        let square = egui::Rect::from_center_size(
            egui::pos2(rect.right() - height / 2.0, rect.center().y),
            egui::vec2(height, height),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(square), |ui| {
            removed = sigil::icon::icon_button_named(ui, sigil::Icon::Close, "Remove")
                .on_hover_text(
                    "Stop connecting to this exchange. Nothing said there is deleted — the \
                     conversations stay in this store and come back if it is added again.",
                )
                .clicked();
        });
    }
    (row.clicked() && !removed, removed)
}
