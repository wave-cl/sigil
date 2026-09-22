//! Emoji on screen: one as a picture, the strip that hangs off a message,
//! and the picker behind its ➕.
//!
//! # Pictures, not glyphs
//!
//! egui draws text from monochrome fonts, and the emoji font it bundles is a
//! 2014 subset of Noto — 887 glyphs, none of them yellow, one of the old
//! picker's six (🧡) not among them and drawn as a box. So an emoji here is
//! looked up in `sigil-emoji` and painted from its Twemoji SVG through the
//! image loader that `install_loaders` registers. **The string is still the
//! thing**: it is what the button says to the accessibility tree, what a
//! click returns, and what goes over the wire. The picture is how it looks.
//!
//! When there is no picture — a string this table does not know, or no svg
//! loader, which is every kittest harness — the string is drawn as text, so
//! a test sees the same widgets as the application and a stranger's emoji is
//! still something rather than nothing.
//!
//! # The strip
//!
//! The quick reactions and a ➕, on a pill that hangs off the top-outer
//! corner of the message under the pointer, with More beside them -- and
//! Reply too where the pane is wide enough for it. The one place everything
//! to do *to* a message lives, and one region for the reveal rule to reason
//! about. Drawn as a foreground `Area` so it can overlap the message above
//! without that message noticing: the reveal test is per layer, and the
//! strip is its own.
//!
//! **The cells are the form's, not a number.** A phone's pane is the budget:
//! the cell is as big as a finger wants, the row is measured against the
//! pane rather than assumed to fit, and what does not fit goes in the menu.

use sigil::{ColorTheme, tokens};

use crate::message::BubbleAction;

/// A cell on the strip and in the picker: a finger's worth on a phone, a
/// pointer's worth on a desktop.
///
/// **A phone's cells are the finger's, not the pointer's**: at a desktop
/// 28 points they were hit two presses in three on the device, which is
/// how a reaction "does nothing".
pub fn cell(ctx: &egui::Context) -> f32 {
    if sigil::Form::of(ctx).is_phone() {
        tokens::BUTTON_LG
    } else {
        tokens::BUTTON_MD
    }
}
/// How much of a cell the picture inside it takes. A share rather than a
/// size: the cell is the form's, and a 20-point emoji in a 40-point cell is
/// a big button with a small picture rattling around in it.
const PICTURE: f32 = 0.66;
/// How far the strip reaches back over the bubble's corner.
const OVERLAP: f32 = tokens::SPACING_LG;
/// Cells across in the picker.
const COLS: usize = 8;
/// How many the picker's "Frequently used" row shows.
pub const FREQUENT: usize = 5;

/// One emoji, as a button: its picture, or its text when there is none.
///
/// Says the emoji itself to the accessibility tree — the label is the string,
/// so a test can find "🎉" whether or not a picture was painted — and shows
/// its Unicode name on hover.
///
/// `ours` is "you have already sent this one": it is drawn held down, and
/// said as a pressed button to the accessibility tree. **Pressing it again
/// takes the reaction back**, which is only obvious if the one you sent
/// looks different from the five you did not.
pub fn glyph(ui: &mut egui::Ui, chars: &str, cell: f32, ours: bool) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(cell, cell), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), ours, chars)
    });
    if ui.is_rect_visible(rect) {
        if ours {
            // The same two marks a chip of one's own carries -- filled and
            // outlined in the accent -- so the cell and the chip under the
            // message are recognisably the same reaction.
            ui.painter()
                .rect_filled(rect, tokens::RADIUS_SM, theme.interactive_hover);
            ui.painter().rect_stroke(
                rect,
                tokens::RADIUS_SM,
                egui::Stroke::new(tokens::STROKE_THIN, theme.accent),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            ui.painter()
                .rect_filled(rect, tokens::RADIUS_SM, theme.interactive_hover);
        }
        let side = (cell * PICTURE).min(cell - 2.0 * tokens::SPACING_XXS);
        let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(side, side));
        paint(ui, chars, inner, theme.text_primary);
    }
    match sigil_emoji::find(chars) {
        Some(e) => response.on_hover_text(e.name),
        None => response,
    }
}

/// Paint one into `rect`: the picture if it loads, the text if not.
///
/// Returns whether a picture was drawn. Text is sized to the rect, which is
/// what the bundled font makes of it — a box, for an emoji it lacks — and is
/// the fallback, not the design.
pub fn paint(ui: &mut egui::Ui, chars: &str, rect: egui::Rect, colour: egui::Color32) -> bool {
    if let Some(e) = sigil_emoji::find(chars) {
        let image = egui::Image::from_bytes(e.uri(), egui::load::Bytes::Static(e.svg()))
            .fit_to_exact_size(rect.size())
            .show_loading_spinner(false);
        if let Ok(egui::load::TexturePoll::Ready { .. }) =
            image.load_for_size(ui.ctx(), rect.size())
        {
            image.paint_at(ui, rect);
            return true;
        }
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        chars,
        egui::FontId::proportional(rect.height() * 0.8),
        colour,
    );
    false
}

/// One emoji and how many people sent it, under a message. Ours is outlined.
/// How tall a chip is, for whoever has to make room for one before it is
/// drawn.
pub fn chip_height(_ui: &egui::Ui) -> f32 {
    tokens::ICON_SM + 2.0 * tokens::SPACING_XS
}

pub fn chip(ui: &mut egui::Ui, chars: &str, count: usize, ours: bool) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    let picture = tokens::ICON_SM;
    let pad = tokens::SPACING_SM;
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = (count > 1).then(|| {
        ui.painter()
            .layout_no_wrap(count.to_string(), font, theme.text_primary)
    });
    let width = pad
        + picture
        + galley
            .as_ref()
            .map_or(0.0, |g| tokens::SPACING_XS + g.size().x)
        + pad;
    let height = chip_height(ui);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    // What the chip means, in words: the emoji and the count, so a reader of
    // the tree gets "🎉 2" and not a picture and a number.
    let label = match count {
        0 | 1 => chars.to_string(),
        n => format!("{chars} {n}"),
    };
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &label)
    });
    if ui.is_rect_visible(rect) {
        let fill = if ours {
            theme.interactive_hover
        } else {
            theme.surface_secondary
        };
        ui.painter().rect_filled(rect, tokens::RADIUS_PILL, fill);
        if ours {
            // Outlined as well as filled: "I reacted" and "somebody reacted"
            // must not be one shade apart.
            ui.painter().rect_stroke(
                rect,
                tokens::RADIUS_PILL,
                egui::Stroke::new(tokens::STROKE_THIN, theme.accent),
                egui::StrokeKind::Inside,
            );
        }
        let at = egui::Rect::from_min_size(
            egui::pos2(rect.left() + pad, rect.center().y - picture / 2.0),
            egui::vec2(picture, picture),
        );
        paint(ui, chars, at, theme.text_primary);
        if let Some(g) = galley {
            let pos = egui::pos2(
                at.right() + tokens::SPACING_XS,
                rect.center().y - g.size().y / 2.0,
            );
            ui.painter().galley(pos, g, theme.text_primary);
        }
    }
    response.on_hover_text(if ours {
        "you reacted — click to take it back"
    } else {
        "react"
    })
}

/// Where a strip goes, and whose it is.
pub struct Strip<'a> {
    /// The message's own id; the strip, its picker and its menu hang off it.
    pub id: egui::Id,
    /// The bubble's rect, whose top-outer corner the strip hangs off.
    pub bubble: egui::Rect,
    /// One's own message, so the outer corner is the left one.
    pub mine: bool,
    /// The transcript's clip rect: the strip stays inside it sideways and is
    /// not drawn at all once the corner has scrolled out of it.
    pub clip: egui::Rect,
    /// The person's most-used, for the picker's own row.
    pub frequent: &'a [String],
    /// What this person has already sent on this message: those cells are
    /// drawn held down, and pressing one takes that reaction back.
    pub chosen: &'a [&'a str],
    /// How many cells `more` draws after the emoji and the ➕, so the width
    /// is known before the row is: a strip wider than it thought reaches
    /// across the message it hangs off and swallows what was meant for it.
    pub extras: usize,
}

/// The layer the strip for `id` draws on, for the reveal test.
pub fn strip_layer(id: egui::Id) -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, id)
}

/// The strip: the quick reactions, ➕ for the rest, and whatever `more`
/// draws after them.
///
/// Hangs off the top-outer corner of `bubble` — the right corner of somebody
/// else's message, the left of one's own — reaching `OVERLAP` back over it
/// and out into the space beside, where there is room. Kept inside `clip`
/// sideways, so a wide bubble does not push it off the pane; dropped to the
/// bottom corner when the top one is at the top of the transcript; and not
/// drawn at all when the bubble is scrolled out of `clip`: a foreground
/// area is not clipped by the transcript, and a strip floating over the
/// header for a message that is not on screen is a strip for nothing.
///
/// `more` draws the rest of the row after the emoji -- More, and Reply
/// where there is room for it -- and is handed the cell size, so what it
/// draws is the same size as what the strip drew. It is the caller's
/// because what it offers depends on the message; `extras` is how many
/// cells it will take. An emoji chosen from the strip or the picker lands
/// in `action.react`.
/// Returns the rectangle it drew in: a press there is the strip's, and the
/// message underneath must not read it as a press elsewhere and put the
/// strip away before the control under the finger has been drawn -- which
/// is exactly what a reaction that did nothing was. Empty where the strip
/// was not drawn at all.
pub fn strip(
    ui: &mut egui::Ui,
    Strip {
        id,
        bubble,
        mine,
        clip,
        frequent,
        chosen,
        extras,
    }: Strip<'_>,
    action: &mut BubbleAction,
    more: impl FnOnce(&mut egui::Ui, &mut BubbleAction, f32),
) -> egui::Rect {
    if bubble.bottom() < clip.top() || bubble.top() > clip.bottom() {
        return egui::Rect::NOTHING;
    }
    let theme = ColorTheme::current(ui.ctx());
    let gap = tokens::SPACING_XXS;
    let pad = tokens::SPACING_XS;
    // The quick reactions, then ➕, Reply and More. **Counted, not written
    // down**: it said eight while there were five quick ones, and a sixth
    // made the strip wider than the placement thought -- so it reached
    // further across the message, over the pointer that had revealed it,
    // and swallowed the wheel that should have scrolled the transcript.
    let cells = (sigil_emoji::QUICK.len() + 1 + extras) as f32;
    // As big as the form wants, and never wider than the pane: a strip that
    // does not fit is clamped back over the message, where it takes the
    // presses meant for the message. Measured, so neither the count nor the
    // size has to be guessed right twice.
    let room = (clip.width() - 2.0 * pad - (cells - 1.0) * gap) / cells;
    let cell = cell(ui.ctx()).min(room).max(tokens::BUTTON_SM);
    let width = cells * cell + (cells - 1.0) * gap + 2.0 * pad;
    let height = cell + 2.0 * pad;
    let x = if mine {
        bubble.left() + OVERLAP - width
    } else {
        bubble.right() - OVERLAP
    };
    let x = if clip.width() > width {
        x.clamp(clip.left(), clip.right() - width)
    } else {
        clip.left()
    };
    // Off the top corner — unless the top corner is at the top of the
    // transcript, where half the strip would be under the header: painted
    // clipped and, since a widget outside its clip cannot be pressed,
    // unreachable. The bottom corner then.
    let y = bubble.top() - height / 2.0;
    let y = if y < clip.top() {
        bubble.bottom() - height / 2.0
    } else {
        y
    };
    let pos = egui::pos2(x, y);

    let picker_id = id.with("picker");
    let mut open_picker = false;
    let drawn = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .constrain(false)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(clip);
            egui::Frame::NONE
                .fill(theme.surface_elevated)
                .stroke(egui::Stroke::new(tokens::STROKE_THIN, theme.border_default))
                .corner_radius(tokens::RADIUS_PILL)
                .inner_margin(pad)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                    ui.horizontal(|ui| {
                        for quick in sigil_emoji::QUICK {
                            let ours = chosen.contains(&quick);
                            if glyph(ui, quick, cell, ours).clicked() {
                                action.react = Some(quick.to_string());
                            }
                        }
                        if cell_icon(ui, sigil::Icon::Plus, "More emoji", cell).clicked() {
                            open_picker = true;
                        }
                        more(ui, action, cell);
                    });
                });
        });

    if let Some(picked) = picker(ui, picker_id, bubble, frequent, chosen, open_picker) {
        action.react = Some(picked);
    }
    // The rectangle as it was just drawn, not the one remembered from a
    // pass when the strip was somewhere else.
    drawn.response.rect
}

/// A painted icon in a strip cell, the size of an emoji cell rather than of
/// `icon_button` — the strip is one row of like-sized things.
pub fn cell_icon(ui: &mut egui::Ui, icon: sigil::Icon, word: &str, cell: f32) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(egui::vec2(cell, cell), egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), word));
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, tokens::RADIUS_SM, theme.interactive_hover);
        }
        sigil::icon::draw(
            ui.painter(),
            rect.shrink(tokens::SPACING_XS),
            icon,
            theme.text_primary,
        );
    }
    response.on_hover_text(word)
}

/// A menu row that is an emoji and some words: the picture where an icon
/// would be in [`sigil::icon_item`], and the words beside it, as one thing
/// to press.
///
/// Says "❤️ Ana, You" to the accessibility tree -- the emoji as itself, then
/// the words -- so a reader is told which reaction the names belong to.
pub fn emoji_item(ui: &mut egui::Ui, chars: &str, text: &str) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    let gap = tokens::SPACING_SM;
    let square = tokens::BUTTON_MD;
    let font = egui::TextStyle::Body.resolve(ui.style());
    // Wrapped: a list of names is as long as the people are, and a menu
    // that grows sideways off a phone shows the first name and no more.
    let wrap = (ui.available_width() - square - gap).max(80.0);
    let galley = ui
        .painter()
        .layout(text.to_owned(), font, theme.text_primary, wrap);
    let height = square.max(galley.size().y + tokens::SPACING_XS);
    let width = ui.available_width().max(square + gap + galley.size().x);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let said = format!("{chars} {text}");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &said)
    });
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, tokens::RADIUS_SM, theme.interactive_hover);
        }
        let picture = egui::Rect::from_center_size(
            egui::pos2(rect.left() + square / 2.0, rect.center().y),
            egui::Vec2::splat(tokens::ICON_SM),
        );
        paint(ui, chars, picture, theme.text_primary);
        let at = egui::pos2(
            rect.left() + square + gap,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(at, galley, theme.text_primary);
    }
    response
}

/// A row of the picker: a heading, or up to `COLS` emoji.
enum Row {
    Head(&'static str),
    Cells(Vec<&'static str>),
}

/// The full picker, anchored to the message: a search box, the person's
/// own most-used, and every group.
///
/// Opens when `open_now`; closes on a choice, on Escape, or on a click
/// outside — not on a click inside, which a search box needs. Only the rows
/// on screen are laid out, so opening it does not rasterise nineteen hundred
/// pictures at once. Returns the emoji chosen, if one was.
pub fn picker(
    ui: &mut egui::Ui,
    id: egui::Id,
    anchor: egui::Rect,
    frequent: &[String],
    ours: &[&str],
    open_now: bool,
) -> Option<String> {
    let ctx = ui.ctx().clone();
    let search_id = id.with("search");
    if open_now {
        ctx.data_mut(|d| d.insert_temp(search_id, String::new()));
    }
    let mut chosen = None;
    egui::Popup::new(
        id,
        ctx.clone(),
        egui::PopupAnchor::ParentRect(anchor),
        ui.layer_id(),
    )
    .open_memory(open_now.then_some(egui::SetOpenCommand::Bool(true)))
    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
    .show(|ui| {
        let theme = ColorTheme::current(ui.ctx());
        let gap = tokens::SPACING_XXS;
        let room =
            (ui.ctx().content_rect().width() - 2.0 * tokens::SPACING_MD - (COLS - 1) as f32 * gap)
                / COLS as f32;
        let side = cell(ui.ctx()).min(room).max(tokens::BUTTON_SM);
        let width = COLS as f32 * side + (COLS - 1) as f32 * gap;
        ui.set_width(width);

        let mut query: String = ui.data(|d| d.get_temp(search_id)).unwrap_or_default();
        let field = ui.add(
            egui::TextEdit::singleline(&mut query)
                .hint_text("Search")
                .desired_width(width),
        );
        if open_now {
            field.request_focus();
        }
        if field.changed() {
            ui.data_mut(|d| d.insert_temp(search_id, query.clone()));
        }
        ui.add_space(tokens::SPACING_XS);

        let rows = rows(&query, frequent);
        ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
        egui::ScrollArea::vertical()
            .max_height(10.0 * (side + gap))
            .auto_shrink([false, false])
            .show_rows(ui, side, rows.len(), |ui, range| {
                for row in &rows[range] {
                    match row {
                        Row::Head(text) => {
                            ui.allocate_ui(egui::vec2(width, side), |ui| {
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            egui::RichText::new(*text)
                                                .small()
                                                .color(theme.text_secondary),
                                        );
                                    },
                                );
                            });
                        }
                        Row::Cells(cells) => {
                            ui.horizontal(|ui| {
                                for chars in cells {
                                    if glyph(ui, chars, side, ours.contains(chars)).clicked() {
                                        chosen = Some((*chars).to_string());
                                    }
                                }
                            });
                        }
                    }
                }
            });
        if rows.is_empty() {
            ui.label(egui::RichText::new("Nothing by that name").color(theme.text_muted));
        }
    });
    if chosen.is_some() {
        egui::Popup::close_id(&ctx, id);
    }
    chosen
}

/// What the picker lists: a search's matches, flat; or the frequent row and
/// every group under its heading.
fn rows(query: &str, frequent: &[String]) -> Vec<Row> {
    let mut rows = Vec::new();
    let push = |rows: &mut Vec<Row>, cells: Vec<&'static str>| {
        for chunk in cells.chunks(COLS) {
            rows.push(Row::Cells(chunk.to_vec()));
        }
    };
    if !query.trim().is_empty() {
        let found: Vec<&'static str> = sigil_emoji::search(query).map(|e| e.chars).collect();
        push(&mut rows, found);
        return rows;
    }
    // The person's own, by the string they sent: only the ones this table
    // has a picture for, which is every one this picker can have offered.
    let theirs: Vec<&'static str> = frequent
        .iter()
        .filter_map(|s| sigil_emoji::find(s))
        .map(|e| e.chars)
        .take(FREQUENT)
        .collect();
    if !theirs.is_empty() {
        rows.push(Row::Head("Frequently used"));
        push(&mut rows, theirs);
    }
    for group in sigil_emoji::Group::ALL {
        rows.push(Row::Head(group.label()));
        push(&mut rows, group.emoji().map(|e| e.chars).collect());
    }
    rows
}
