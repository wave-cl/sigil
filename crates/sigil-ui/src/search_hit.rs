//! One result of searching what this client holds.
//!
//! A conversation row's cousin: the same shape of row -- the same mark, in the
//! same place, at the same size -- so the two lists read as one family. What
//! it shows is the *message*: who said it, and the part of it the word was
//! found in, with the word marked. A result that
//! shows the first sixty characters of a long message hides the one thing
//! the reader searched for, and has to be opened to be rejected.

use std::ops::Range;

use sigil::{ColorTheme, tokens};

/// A hit, as a list needs it. Plain data; the caller decides what matched.
pub struct SearchHit<'a> {
    /// The conversation's identifier, for the mark. **The conversation's and
    /// not the speaker's**: a result is a place to go back to, and the mark
    /// beside it is the one the chats list draws for that same conversation,
    /// so finding something here and then finding it there is one gesture
    /// rather than a second search.
    pub id: &'a str,
    /// That conversation's picture, where it has one. `None` draws the
    /// identicon, as everywhere else.
    pub picture: Option<&'a egui::TextureHandle>,
    /// What the conversation it was found in is called.
    pub label: &'a str,
    /// Who said it, as the transcript names them.
    pub who: &'a str,
    /// The whole message. The row shows the part around `found`.
    pub text: &'a str,
    /// Where in `text` the match is, in bytes, on character boundaries.
    pub found: Range<usize>,
    /// A short time, already formatted.
    pub at: &'a str,
}

/// How much of the message is shown before the match, in characters.
const BEFORE: usize = 40;

/// Draw one result. The whole row answers to a press.
pub fn search_hit(ui: &mut egui::Ui, hit: &SearchHit<'_>, selected: bool) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());

    // The row is the target, and nothing in it is selectable text: the same
    // two rules as `conversation_row`, for the same reason -- a result is
    // entirely words, and a selectable label takes the press for itself.
    let inner = ui.scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| {
        ui.set_min_width(ui.available_width());
        ui.style_mut().interaction.selectable_labels = false;
        let ground = ui.painter().add(egui::Shape::Noop);
        let response = ui.response();
        let fill = if selected {
            theme.interactive_hover
        } else if response.hovered() {
            theme.interactive_hover.gamma_multiply(0.5)
        } else {
            egui::Color32::TRANSPARENT
        };
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let drawn = body(ui, hit, &theme);
        ui.painter().set(
            ground,
            egui::epaint::RectShape::filled(drawn, tokens::RADIUS_MD, fill),
        );
    });

    inner.response
}

/// The conversation and the time on one line, the words under it. Returns
/// the rectangle it covers.
fn body(ui: &mut egui::Ui, hit: &SearchHit<'_>, theme: &ColorTheme) -> egui::Rect {
    let inner = egui::Frame::NONE
        .corner_radius(tokens::RADIUS_MD)
        .inner_margin(egui::Margin::symmetric(
            tokens::SPACING_SM as i8,
            tokens::SPACING_XS as i8,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Left of both lines, as the chats list has it: a mark beside
                // only the first line would sit against the conversation's
                // name and read as belonging to it rather than to the row.
                crate::avatar(ui, hit.id, hit.picture, tokens::AVATAR_MD);
                ui.add_space(tokens::SPACING_SM);
                ui.vertical(|ui| {
                    ui.set_min_width(ui.available_width());
                    // The time first, from the right, and the name in what is
                    // left -- the conversation row's arrangement, and for its
                    // reason: a label given the row first takes all of it.
                    //
                    // Inside a row one line tall -- allocated, not a
                    // `horizontal`: that is at least `interact_size` high, a
                    // finger on a phone, with the name centred in it and the
                    // words a line's height under the name they belong to. A
                    // right-to-left layout put straight into a vertical ui is
                    // worse still: given all the height there is, it put the
                    // first result two hundred pixels down an empty column.
                    let line = ui.text_style_height(&egui::TextStyle::Body);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), line),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.colored_label(
                                        theme.text_muted,
                                        egui::RichText::new(hit.at).small(),
                                    );
                                    ui.with_layout(
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(hit.label).strong(),
                                                )
                                                .truncate(),
                                            );
                                        },
                                    );
                                },
                            );
                        },
                    );
                    let (shown, marked) = excerpt(hit.text, hit.found.clone(), BEFORE);
                    let job = excerpt_job(
                        ui,
                        hit.who,
                        &shown,
                        marked,
                        ui.available_width(),
                        theme.text_secondary,
                        theme.text_primary,
                        theme.accent,
                    );
                    ui.add(egui::Label::new(job));
                });
            });
        });
    inner.response.rect
}

/// The name, then the words with the match in the accent, laid out to at
/// most two rows of `wrap` with an ellipsis where they stop.
#[allow(clippy::too_many_arguments)]
fn excerpt_job(
    ui: &egui::Ui,
    who: &str,
    shown: &str,
    marked: Range<usize>,
    wrap: f32,
    name: egui::Color32,
    plain: egui::Color32,
    accent: egui::Color32,
) -> egui::text::LayoutJob {
    let small = egui::TextStyle::Small.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap;
    job.wrap.max_rows = 2;
    job.wrap.break_anywhere = false;
    job.wrap.overflow_character = Some('…');
    job.append(
        &format!("{who}: "),
        0.0,
        egui::TextFormat::simple(small.clone(), name),
    );
    job.append(
        &shown[..marked.start],
        0.0,
        egui::TextFormat::simple(small.clone(), plain),
    );
    job.append(
        &shown[marked.clone()],
        0.0,
        egui::TextFormat {
            font_id: small.clone(),
            color: accent,
            underline: egui::Stroke::new(1.0, accent),
            ..Default::default()
        },
    );
    job.append(
        &shown[marked.end..],
        0.0,
        egui::TextFormat::simple(small, plain),
    );
    job
}

/// The part of `text` around `found`: at most `before` characters ahead of
/// the match, cut at a word when one is near, with `…` where the text was
/// cut. Returns the excerpt and where the match is in *it*. Control
/// characters are flattened to spaces, as a preview's are, so a message
/// with newlines in it stays on its rows.
///
/// Only the start is cut here. The end is the layout's to cut, at the width
/// it has, which this function does not know.
pub fn excerpt(text: &str, found: Range<usize>, before: usize) -> (String, Range<usize>) {
    let found = clamp(text, found);
    // Character boundaries at most `before` characters ahead of the match.
    let ahead: Vec<usize> = text[..found.start]
        .char_indices()
        .map(|(i, _)| i)
        .rev()
        .take(before)
        .collect();
    let Some(&furthest) = ahead.last() else {
        // The match is at the start: nothing to cut.
        return (flat(text), found);
    };
    if furthest == 0 {
        return (flat(text), found);
    }
    // Prefer to start at a word: the first space at or after the furthest
    // point, if there is one before the match, else the furthest point.
    let start = text[furthest..found.start]
        .find(char::is_whitespace)
        .map(|i| furthest + i + 1)
        .filter(|&i| i < found.start)
        .unwrap_or(furthest);
    let cut = "…";
    let shown = format!("{cut}{}", flat(&text[start..]));
    let marked = found.start - start + cut.len()..found.end - start + cut.len();
    (shown, marked)
}

/// Newlines and tabs as spaces, byte for byte: an ASCII control character
/// is one byte and so is a space, so every index into the text stays where
/// it was. The C1 controls are two bytes and are left alone -- a mark that
/// has moved is worse than a stray U+0085.
fn flat(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_control() { ' ' } else { c })
        .collect()
}

/// A range that is inside `text` and on character boundaries, whatever was
/// passed: a range from elsewhere is a claim, not a fact.
fn clamp(text: &str, found: Range<usize>) -> Range<usize> {
    let mut start = found.start.min(text.len());
    while !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = found.end.clamp(start, text.len());
    while !text.is_char_boundary(end) {
        end += 1;
    }
    start..end
}

#[cfg(test)]
mod tests {
    use super::excerpt;

    /// A match near the start is shown from the start, whole.
    #[test]
    fn a_match_near_the_start_cuts_nothing() {
        let (shown, marked) = excerpt("the release check is at noon", 4..11, 40);
        assert_eq!(shown, "the release check is at noon");
        assert_eq!(&shown[marked], "release");
    }

    /// A match deep in a long message is shown from a word not far ahead of
    /// it, with an ellipsis for what was cut, and the mark still lands on
    /// the word.
    #[test]
    fn a_match_deep_in_a_message_is_shown_from_a_word_before_it() {
        let text = format!("{} release check", "word ".repeat(30));
        let found = text.find("release").unwrap();
        let (shown, marked) = excerpt(&text, found..found + 7, 20);
        assert!(shown.starts_with("…word "), "cut at a word: {shown:?}");
        assert!(
            shown.chars().count() < 20 + 7 + 8,
            "about twenty characters ahead, not the whole message: {shown:?}"
        );
        assert_eq!(&shown[marked], "release");
    }

    /// Letters that take more than one byte do not move the mark.
    #[test]
    fn multibyte_letters_before_the_match_do_not_move_the_mark() {
        let text = "über über über über über über über über über über Brücke";
        let found = text.find("Brücke").unwrap();
        let (shown, marked) = excerpt(text, found..found + "Brücke".len(), 12);
        assert_eq!(&shown[marked], "Brücke", "{shown:?}");
        assert!(shown.starts_with('…'));
    }

    /// Newlines in the message are spaces in the excerpt, and the mark is
    /// where it was.
    #[test]
    fn newlines_are_flattened_without_moving_the_mark() {
        let (shown, marked) = excerpt("one\ntwo\nthree", 8..13, 40);
        assert_eq!(shown, "one two three");
        assert_eq!(&shown[marked], "three");
    }

    /// A range that is not on character boundaries, or runs past the end,
    /// is brought inside rather than panicking the row.
    #[test]
    fn a_bad_range_is_brought_inside() {
        let (shown, marked) = excerpt("Brücke", 3..99, 40);
        assert_eq!(shown, "Brücke");
        assert_eq!(&shown[marked], "ücke");
    }
}
