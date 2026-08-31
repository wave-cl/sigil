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

    for row in rows {
        ui.horizontal(|ui| {
            crate::dot(
                ui,
                row.speaking,
                theme.speaking,
                theme.text_muted,
                if row.speaking { "speaking" } else { "silent" },
            );
            ui.add(egui::Label::new(egui::RichText::new(&row.key).monospace()).selectable(true));
            ui.add(
                egui::ProgressBar::new(row.level.clamp(0.0, 1.0))
                    .desired_width(tokens::AVATAR_XL)
                    .fill(if row.speaking {
                        theme.speaking
                    } else {
                        theme.border_default
                    }),
            );
            ui.colored_label(theme.text_muted, &row.detail);
        });
    }

    if connecting > 0 {
        ui.colored_label(
            theme.text_secondary,
            format!("{connecting} more in the room, not yet connected"),
        );
    }
}
