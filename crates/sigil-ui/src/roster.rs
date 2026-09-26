//! Who is in a room, and who is talking.
//!
//! Takes plain data rather than anything from the protocol, so this crate stays
//! a set of widgets rather than a second place the wire format is understood.
//! The caller maps its own types onto [`Row`].

use sigil::{ColorTheme, tokens};

/// One person in the roster.
pub struct Row {
    /// Their key, in full. Never abbreviated away: a name is an assertion and a
    /// key is not (SIP-21), and this is often the only place the whole thing
    /// appears.
    pub key: String,
    /// What to call them, when the caller knows. **`None` is not "they have no
    /// name"** -- it is a caller with nowhere to look one up, which is what
    /// sigil-voice is: a voice client with no profile store, and the reason
    /// this row carried a key and nothing else for as long as it did. The chat
    /// app has had a name and a face for every one of these people all along
    /// and was handing over base58.
    ///
    /// Drawn *beside* the key rather than instead of it, for the reason the
    /// field above gives.
    pub named: Option<String>,
    /// Their picture, when the caller has one decoded. `None` draws the
    /// identicon, which is what a key alone can say.
    pub picture: Option<egui::TextureHandle>,
    /// Whether they are speaking, smoothed so it follows a conversation rather
    /// than flickering between syllables.
    pub speaking: bool,
    /// Loudness, roughly 0..1, for the meter.
    pub level: f32,
    /// How their path is holding up — loss, concealment, buffer depth.
    pub detail: String,
}

/// Draw the roster.
///
/// `connecting` counts members whose session is not up yet. They are in the
/// room and cannot be heard, and that is a different thing from not being
/// there — it is precisely what somebody wants to know when they cannot hear a
/// person they were told is present.
///
/// `detail` is whether each row's numbers are open. **The caller decides,
/// because a roster is drawn in two places that mean different things by it.**
/// In sigil-voice's pane the numbers are the pane: it is where somebody goes
/// to ask how the path is holding up. On a call card they are a second line
/// of engineering under every person in the room, which is not what that
/// screen is for and is twice the height of what is — and that card already
/// has a rule for when numbers are open, which it applied to its own line and
/// not to the roster inside it.
pub fn roster(ui: &mut egui::Ui, rows: &[Row], connecting: usize, detail: bool) {
    let theme = ColorTheme::current(ui.ctx());

    if rows.is_empty() {
        ui.colored_label(
            theme.text_secondary,
            match connecting {
                0 => "Nobody else here yet.".to_string(),
                n => format!("Connecting to {n}…"),
            },
        );
        return;
    }

    // A full key and an eighty-point meter beside it do not fit a phone's
    // width; there the key is shortened -- the full one is still the row's
    // name for the tree, and the tooltip -- and the meter takes a share.
    let narrow = ui.available_width() < tokens::NARROW_WIDTH;
    // A phone's theme makes every row a control's height for the buttons'
    // sake; here there are none, and a finger's height between a key and
    // its detail line reads as two rows. A line tall, as the conversation
    // row does.
    let line = ui.text_style_height(&egui::TextStyle::Body);
    if narrow {
        ui.spacing_mut().interact_size.y = line;
        ui.spacing_mut().item_spacing.y = tokens::SPACING_XXS;
    }
    // **A mark beside each key**, as everywhere else in the app that somebody
    // appears. A roster is read to find one person in it, and a column of
    // base58 stems is the hardest thing here to scan; the mark is what a
    // reader compares at a glance.
    //
    // The height of the row, so it costs width and not height: the rule just
    // above deliberately brings a phone's row down to a single line, and a
    // mark at `AVATAR_SM` would undo it for every row in the room.
    let mark = if narrow { line } else { tokens::AVATAR_SM };

    for row in rows {
        ui.horizontal(|ui| {
            crate::dot(
                ui,
                row.speaking,
                theme.speaking,
                theme.text_muted,
                if row.speaking { "speaking" } else { "silent" },
            );
            crate::avatar(ui, &row.key, row.picture.as_ref(), mark);
            // **The name and the key, in that order** -- the same shape the
            // card above this roster uses for the person on a two-party call,
            // so one screen does not name somebody at the top and spell them
            // in base58 six rows down. The key stays because a name is an
            // assertion and a key is not; it goes quiet and small when there
            // is a name to read first.
            let keyed = |ui: &mut egui::Ui| {
                let text = egui::RichText::new(if narrow {
                    crate::short(&row.key)
                } else {
                    row.key.clone()
                })
                .monospace();
                let text = match row.named {
                    Some(_) => text.small().color(theme.text_muted),
                    None => text,
                };
                if narrow {
                    ui.add(egui::Label::new(text)).on_hover_text(&row.key);
                } else {
                    ui.add(egui::Label::new(text).selectable(true));
                }
            };
            let meter = |ui: &mut egui::Ui, width: f32| {
                ui.add(
                    egui::ProgressBar::new(row.level.clamp(0.0, 1.0))
                        .desired_width(width)
                        .fill(if row.speaking {
                            theme.speaking
                        } else {
                            theme.border_default
                        }),
                );
            };

            if narrow {
                // **The name is given a budget, because it is the one of the
                // three that can give way.** A `horizontal` never wraps and
                // `Label::truncate` shortens to whatever is free *at the
                // moment it is drawn*, which -- added left to right -- is the
                // whole rest of the pane, because the key and the meter after
                // it have not asked for theirs yet. A long name pushed the
                // meter clean off the edge: 417 points in a 360-point pane,
                // which is what `phone_width` measured. So both are measured
                // first and taken off what the name may have.
                //
                // Laying the row out from the right instead was tried and is
                // worse to look at: it right-aligns the whole group, so every
                // name floats away from the mark it belongs to and the column
                // is ragged down the left.
                let key_width = {
                    let size = ui.style().text_styles[&egui::TextStyle::Small].size;
                    let text = crate::short(&row.key);
                    ui.ctx().fonts_mut(|f| {
                        f.layout_no_wrap(text, egui::FontId::monospace(size), theme.text_muted)
                            .size()
                            .x
                    })
                };
                let meter_width = tokens::AVATAR_XL.min(ui.available_width() * 0.3);
                if let Some(named) = &row.named {
                    let budget = (ui.available_width()
                        - key_width
                        - meter_width
                        - ui.spacing().item_spacing.x * 2.0)
                        .max(tokens::AVATAR_SM);
                    ui.scope(|ui| {
                        ui.set_max_width(budget);
                        ui.add(egui::Label::new(named).truncate())
                            .on_hover_text(&row.key);
                    });
                }
                keyed(ui);
                // Against the right edge, so the meters make a column rather
                // than stepping in and out with the length of each name.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    meter(ui, meter_width);
                });
            } else {
                if let Some(named) = &row.named {
                    ui.add(egui::Label::new(named).truncate())
                        .on_hover_text(&row.key);
                }
                keyed(ui);
                meter(ui, tokens::AVATAR_XL);
            }
            // **On a phone the detail goes under the row, not after it.**
            // It is a sentence about the path -- "2.1% lost, 180 ms of
            // buffer, concealing 3 frames in 100" -- and a `horizontal` never
            // wraps, so after a key and a meter it took a 360-point pane out
            // to 572 and every row after it with it. There is no shortening
            // it either: each clause is a separate fact somebody is reading
            // it for.
            if !narrow && detail {
                ui.colored_label(theme.text_muted, &row.detail);
            }
        });
        if narrow && detail && !row.detail.is_empty() {
            ui.horizontal(|ui| {
                // Indented to the key, so it reads as belonging to the row
                // above rather than as a line of its own.
                ui.add_space(tokens::SPACING_MD + tokens::SPACING_SM);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&row.detail)
                            .small()
                            .color(theme.text_muted),
                    )
                    .wrap(),
                );
            });
        }
    }

    if connecting > 0 {
        ui.colored_label(
            theme.text_secondary,
            format!("{connecting} more in the room, not yet connected"),
        );
    }
}
