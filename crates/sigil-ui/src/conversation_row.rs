//! One line in a conversation list.
//!
//! Plain data, like everything here. The caller decides what a conversation is.

use sigil::{ColorTheme, tokens};

/// A conversation, as a list needs it.
#[derive(Default)]
pub struct ConversationRow<'a> {
    /// What identifies it — a channel id, or a peer's key for a direct
    /// message. Used for the mark, and for selection.
    ///
    /// **Selection is by this, never by index.** The list reorders by last
    /// activity under whatever the cursor is on, so an index selects a
    /// different conversation the moment somebody writes to another one.
    pub id: &'a str,
    /// The name to show. For a group, its name; for a direct message, the
    /// other person's profile name or their key.
    pub label: &'a str,
    /// The full key, when this conversation is with one person. Reachable from
    /// the row, because a name is an assertion (SIP-21) and this is often the
    /// only place the key could appear.
    pub key: Option<&'a str>,
    /// The last thing said, one line.
    pub preview: &'a str,
    /// A short time, already formatted.
    pub at: &'a str,
    pub unread: u32,
    /// Anybody may find and join it, and **nothing in it is encrypted**.
    ///
    /// `None` while nobody has said yet -- a conversation restored from this
    /// machine's own copy, before the exchange has answered. The store keeps
    /// whether a channel is a group and not whether it is public, and neither
    /// guess is safe: calling a private group public claims its contents are
    /// in the clear, and calling a public one private claims the opposite. So
    /// an unanswered kind is drawn as neither.
    pub public: Option<bool>,
    /// More than two people.
    pub group: bool,
    /// Nothing can be sealed to them yet: they have published no prekeys, so
    /// they have never run a client. Not an error, and not the same as being
    /// offline — it is a conversation that cannot start rather than one that
    /// is quiet.
    pub waiting: bool,
    /// Somebody is typing. Replaces the preview while it is true.
    pub typing: bool,
}

/// Draw one row. Returns its response, so the caller decides what a click means.
pub fn conversation_row(
    ui: &mut egui::Ui,
    row: &ConversationRow<'_>,
    selected: bool,
) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    let height = tokens::AVATAR_MD + tokens::SPACING_MD;

    // **The row is the target, not the words in it.** A `Frame` sizes itself
    // to its contents and answers for that rectangle, so the gaps -- beside a
    // short name, under a one-line preview, the whole right-hand end of a
    // narrow row -- were places where pressing a conversation did nothing.
    // A ui with a sense of its own is the row: it takes the full width, and
    // everything drawn inside it is inside the thing that answers.
    let inner = ui.scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
        ui.set_min_width(ui.available_width());
        // Reserved now and painted at the end: the background has to go
        // *under* the contents, and its size is not known until they have
        // been laid out.
        let ground = ui.painter().add(egui::Shape::Noop);
        let response = ui.response();
        let fill = if selected {
            theme.interactive_hover
        } else if response.hovered() {
            // The same colour, quieter: hovering says "this one would be
            // chosen", and choosing is what the full strength means.
            theme.interactive_hover.gamma_multiply(0.5)
        } else {
            egui::Color32::TRANSPARENT
        };
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let drawn = row_body(ui, row, &theme, height);
        ui.painter().set(
            ground,
            egui::epaint::RectShape::filled(drawn, tokens::RADIUS_MD, fill),
        );
    });

    inner.response
}

/// What is in a row: the mark, the name, the time, and a line of what was
/// said. Returns the rectangle it covers, which is the row's own.
fn row_body(
    ui: &mut egui::Ui,
    row: &ConversationRow<'_>,
    theme: &ColorTheme,
    height: f32,
) -> egui::Rect {
    let inner = egui::Frame::NONE
        .corner_radius(tokens::RADIUS_MD)
        .inner_margin(egui::Margin::symmetric(
            tokens::SPACING_SM as i8,
            tokens::SPACING_XS as i8,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // **On the row, not on what is around it.** A minimum height set
                // on the frame's own `ui` is satisfied by empty space underneath,
                // so the row's contents kept their own height and sat above the
                // middle of it -- the mark by a few pixels in every row at once,
                // which reads as a list that is slightly falling over.
                //
                // Set here, the horizontal layout is that tall and its `Center`
                // alignment has the whole row to centre in.
                ui.set_min_height(height);
                crate::identicon(ui, row.id, tokens::AVATAR_MD);
                ui.add_space(tokens::SPACING_SM);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        // A public channel is marked, not merely named. Anybody may
                        // join it and everything in it is in the clear -- that is
                        // the difference that matters about it, and a reader has to
                        // be able to see it before they type.
                        if row.public == Some(true) {
                            ui.colored_label(theme.warning, egui::RichText::new("#").strong())
                                .on_hover_text(
                                    "public — anybody may join, and nothing here is encrypted",
                                );
                        } else if row.group && row.public == Some(false) {
                            ui.colored_label(theme.text_muted, "◇")
                                .on_hover_text("group");
                        }
                        let label = ui.label(egui::RichText::new(row.label).strong());
                        if let Some(key) = row.key {
                            label.on_hover_text(key.to_string());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            crate::unread_pill(ui, row.unread);
                            ui.colored_label(theme.text_muted, egui::RichText::new(row.at).small());
                        });
                    });
                    let (colour, text) = if row.waiting {
                        (
                            theme.text_muted,
                            "waiting for them to run a client".to_string(),
                        )
                    } else if row.typing {
                        (theme.accent, "typing…".to_string())
                    } else {
                        (theme.text_secondary, one_line(row.preview))
                    };
                    ui.colored_label(colour, egui::RichText::new(text).small());
                });
            });
        });

    inner.response.rect
}

/// A preview is one line. A message with newlines in it must not push every
/// other conversation down the list.
fn one_line(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 80 {
        let cut: String = flat.chars().take(79).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_never_carries_a_newline_into_the_list() {
        assert_eq!(one_line("one\ntwo\r\nthree"), "one two three");
        assert!(!one_line("a\nb").contains('\n'));
    }

    #[test]
    fn a_long_preview_is_cut_and_says_it_was() {
        let long = "x".repeat(500);
        let out = one_line(&long);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 80);
    }

    #[test]
    fn cutting_a_preview_counts_characters_not_bytes() {
        // A multi-byte character cut in half is a panic, and somebody writing
        // in a non-latin script would find it first.
        let long = "é".repeat(500);
        let out = one_line(&long);
        assert!(out.chars().count() <= 80);
    }
}
