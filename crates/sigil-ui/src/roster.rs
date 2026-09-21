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
pub fn roster(ui: &mut egui::Ui, rows: &[Row], connecting: usize) {
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
    for row in rows {
        ui.horizontal(|ui| {
            crate::dot(
                ui,
                row.speaking,
                theme.speaking,
                theme.text_muted,
                if row.speaking { "speaking" } else { "silent" },
            );
            if narrow {
                ui.add(egui::Label::new(
                    egui::RichText::new(crate::short(&row.key)).monospace(),
                ))
                .on_hover_text(&row.key);
            } else {
                ui.add(
                    egui::Label::new(egui::RichText::new(&row.key).monospace()).selectable(true),
                );
            }
            ui.add(
                egui::ProgressBar::new(row.level.clamp(0.0, 1.0))
                    .desired_width(if narrow {
                        tokens::AVATAR_XL.min(ui.available_width() * 0.3)
                    } else {
                        tokens::AVATAR_XL
                    })
                    .fill(if row.speaking {
                        theme.speaking
                    } else {
                        theme.border_default
                    }),
            );
            // **On a phone the detail goes under the row, not after it.**
            // It is a sentence about the path -- "2.1% lost, 180 ms of
            // buffer, concealing 3 frames in 100" -- and a `horizontal` never
            // wraps, so after a key and a meter it took a 360-point pane out
            // to 572 and every row after it with it. There is no shortening
            // it either: each clause is a separate fact somebody is reading
            // it for.
            if !narrow {
                ui.colored_label(theme.text_muted, &row.detail);
            }
        });
        if narrow && !row.detail.is_empty() {
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
