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
    /// other person's profile name, or what this machine was told to call
    /// them, or the first characters of their key -- decided by the caller,
    /// because only the caller knows whether a profile has arrived.
    pub label: &'a str,
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
        // **Nothing in the row is selectable text.**
        //
        // egui makes labels selectable by default, and a selectable label
        // handles the press itself -- to put a cursor in its text -- so the
        // row underneath never hears it. Which meant the row's own sense,
        // added precisely so the whole thing could be pressed, only answered
        // on the ground *between* the words: beside a short name, under a
        // one-line preview, at the narrow end of the row. Every part of it
        // somebody would actually aim at was dead.
        //
        // Set on the scope rather than on each label, because it is a property
        // of *this row* and not of six widgets that happen to be in it -- a
        // seventh added later inherits it instead of quietly reintroducing the
        // hole.
        //
        // Nothing is lost: there is no text here worth selecting. A name, a
        // time and a truncated preview are all shown in full somewhere the
        // pointer can reach, and the key that used to be worth copying was
        // never selectable anyway.
        ui.style_mut().interaction.selectable_labels = false;
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
                            // **Painted, not typed.** This was `◇`
                            // (U+25C7 WHITE DIAMOND) and egui's fonts do not
                            // have it -- `has_glyph` says so plainly -- so
                            // every private group was marked with nothing at
                            // all, while the public channel beside it kept its
                            // `#` because `#` is ASCII. The same trap as `↳`
                            // in `message.rs`, which is drawn as a painted bar
                            // for exactly this reason.
                            //
                            // Two heads: what a private group *is* from this
                            // row's point of view is more than two people, and
                            // not public.
                            let size = ui.text_style_height(&egui::TextStyle::Body);
                            let (rect, mark) = ui
                                .allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                            if ui.is_rect_visible(rect) {
                                sigil::icon::draw(
                                    ui.painter(),
                                    rect,
                                    sigil::Icon::People,
                                    theme.text_muted,
                                );
                            }
                            mark.on_hover_text("group — more than two people, and not public");
                        }
                        // **No key on hover.** This row used to answer with
                        // the whole base58 key, so running the pointer down a
                        // conversation list -- which is what a pointer does on
                        // its way anywhere -- popped a tooltip of forty-four
                        // characters over the next row down, on every row in
                        // turn. A key is not what somebody is reaching for
                        // here; the conversation is.
                        //
                        // The key is still one gesture away, and now a
                        // deliberate one: open the conversation and press
                        // Members, where every key is in full, in monospace,
                        // and selectable.
                        //
                        // **The time and the count are laid out first, from
                        // the right.** Given the name first it takes the whole
                        // row -- a `Label` has no width to fit into and does
                        // not truncate on its own -- and the right-hand block
                        // is then drawn *on top of it*, so a long name and its
                        // timestamp cross and neither can be read. Seen on a
                        // real list, with a group called "Right?
                        // Wrrrrooooonggggg!" straight through its own 21:48.
                        //
                        // The conversation header above the transcript learned
                        // this first, and its comment says the same thing.
                        // Whatever is left over is the name's, and it
                        // truncates rather than pushing anything off the row.
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            crate::unread_pill(ui, row.unread);
                            ui.colored_label(theme.text_muted, egui::RichText::new(row.at).small());
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(row.label).strong())
                                            .truncate(),
                                    );
                                },
                            );
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
                    // Truncated for the same reason the name is: a label with
                    // no width to fit into keeps drawing, and this one ran
                    // past the end of the column into the transcript.
                    // `one_line` already flattens it to a single line; what it
                    // does not do is make it short.
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).color(colour).small())
                            .truncate(),
                    );
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

#[cfg(test)]
mod glyph_tests {
    /// Every character this crate *draws* must be in the font egui has.
    ///
    /// # Why this test exists
    ///
    /// A private group was marked with `◇` (U+25C7 WHITE DIAMOND), and egui's
    /// bundled fonts do not contain it. There is no error and no warning: the
    /// character simply does not appear, so a private group had no mark while
    /// the public channel on the row above kept its `#` -- because `#` is
    /// ASCII. Nothing in a test or a snapshot said anything, because the
    /// accessibility tree carries the character whether or not a glyph exists
    /// for it, and a missing mark looks exactly like a mark nobody added.
    ///
    /// It is the second time: `message.rs` draws a painted bar rather than
    /// `↳` for the same reason, discovered the same way -- by looking.
    ///
    /// So the rule is: **a mark is painted** ([`sigil::icon::draw`]), and any
    /// character that is typed has to be one the font has. This checks the
    /// second half. `has_glyph` is asked of a real `Context`, so it answers
    /// about the fonts sigil actually runs with rather than a list somebody
    /// kept by hand.
    #[test]
    fn the_characters_we_type_are_in_the_font() {
        // Drawn by this crate and by the chat app's own rows. Add to this
        // when a literal is added, and if it fails, paint it instead.
        const TYPED: &[char] = &[
            '#', // a public channel, in the conversation row
            '…', // a shortened key, a cut preview, a truncated line
            '—', // an em dash, in most of the sentences on screen
            '·', // a separator between small facts
        ];
        // Not in the font, and each one cost somebody a look to find out.
        // Kept as the negative control: if these ever became available the
        // test would stop proving anything, and it would say so.
        const MISSING: &[char] = &['◇', '◆', '●', '↳', '🔒'];

        let ctx = egui::Context::default();
        // The pass's output has to be taken and cleared: dropping a
        // `TexturesDelta` with the font atlas in it panics on the way out,
        // which reads as a failure of whatever the test was doing.
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let id = egui::FontId::proportional(14.0);
            for c in TYPED {
                assert!(
                    ui.ctx().fonts_mut(|f| f.has_glyph(&id, *c)),
                    "U+{:04X} {c:?} is drawn and the font does not have it, so \
                     it is drawn as nothing at all -- paint it instead",
                    *c as u32
                );
            }
            for c in MISSING {
                assert!(
                    !ui.ctx().fonts_mut(|f| f.has_glyph(&id, *c)),
                    "U+{:04X} {c:?} is in the font now. Good news, and this \
                     test's negative control is gone: check the rest of the \
                     list still means something.",
                    *c as u32
                );
            }
        });
        out.textures_delta.clear();
    }
}
