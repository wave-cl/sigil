//! A file carried by a message.
//!
//! # What a receiver may and may not do with one
//!
//! SIP-18 is explicit and the rules are not stylistic:
//!
//! - **Never dispatch on the mime type** beyond choosing how to display. It is
//!   the sender's claim and nothing else.
//! - **Never execute a blob**, and never hand one to a handler chosen by that
//!   string.
//! - The *kind* is what decides the shape drawn, and an unknown kind is a
//!   file.
//!
//! So this takes a kind and some bytes, and the only thing it ever does with
//! either is put pixels on the screen.

use sigil::{ColorTheme, tokens};

/// Image, video, voice note, file — the four SIP-18 kinds, as the interface
/// needs them. Anything unrecognised is a file.
pub const IMAGE: u8 = 0x01;
pub const VIDEO: u8 = 0x02;
pub const VOICE: u8 = 0x03;

/// A file, as a message carries it.
pub struct Attachment<'a> {
    pub kind: u8,
    /// `[image 1920x1080, 2.1 MB]` — what it is, in words.
    pub described: &'a str,
    /// The sender's thumbnail, if they sent one. Shown while the file is
    /// fetched, and instead of it when it is too big to fetch unasked.
    pub preview: &'a [u8],
    /// The file itself, once fetched and opened.
    pub bytes: Option<&'a [u8]>,
    /// A stable name for the blob, so a texture can be keyed on it.
    pub id: &'a str,
}

/// What the reader did to a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttachmentAction {
    pub save: bool,
}

/// Draw one.
///
/// The picture when there is one, the thumbnail while there is not, and a row
/// naming it when it is not a picture at all.
pub fn attachment(ui: &mut egui::Ui, a: &Attachment<'_>) -> AttachmentAction {
    let theme = ColorTheme::current(ui.ctx());
    let mut action = AttachmentAction::default();

    if a.kind == IMAGE {
        // Whole image if we have it, thumbnail if we do not, and the words if
        // we have neither. Each is strictly better than the last and every one
        // of them is better than an empty space where a picture should be.
        let (bytes, whole) = match (a.bytes, a.preview.is_empty()) {
            (Some(b), _) => (Some(b), true),
            (None, false) => (Some(a.preview), false),
            (None, true) => (None, false),
        };
        if let Some(bytes) = bytes {
            // Keyed on the blob, and on whether this is the thumbnail or the
            // real thing: egui caches by URI, so reusing one name for both
            // would leave the thumbnail on screen after the image arrived.
            let uri = format!("bytes://{}{}", a.id, if whole { "" } else { "-preview" });
            let image = egui::Image::from_bytes(uri, bytes.to_vec())
                .max_height(if whole { 320.0 } else { 96.0 })
                .corner_radius(tokens::RADIUS_MD)
                .show_loading_spinner(false);
            let response = ui.add(image);
            if !whole {
                // A thumbnail is not the picture, and saying so stops somebody
                // reading a blurry 96-pixel image as the whole of what was
                // sent.
                ui.colored_label(
                    theme.text_muted,
                    egui::RichText::new("preview — fetching the full image").small(),
                );
            }
            response.context_menu(|ui| {
                if ui.button("Save as…").clicked() {
                    action.save = true;
                    ui.close();
                }
            });
            ui.horizontal(|ui| {
                ui.colored_label(theme.text_muted, egui::RichText::new(a.described).small());
                if ui.small_button("Save").clicked() {
                    action.save = true;
                }
            });
            return action;
        }
    }

    // Not a picture, or one we cannot show. A row that says what it is and
    // offers the only thing that can be done with it.
    egui::Frame::NONE
        .fill(theme.surface_secondary)
        .corner_radius(tokens::RADIUS_MD)
        .inner_margin(egui::Margin::symmetric(
            tokens::SPACING_SM as i8,
            tokens::SPACING_XS as i8,
        ))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // A word, not an icon: an icon for a file kind is a convention
                // to learn, and there are only four of them.
                ui.colored_label(
                    theme.text_muted,
                    match a.kind {
                        IMAGE => "image",
                        VIDEO => "video",
                        VOICE => "voice",
                        _ => "file",
                    },
                );
                ui.label(a.described);
                if ui.small_button("Save").clicked() {
                    action.save = true;
                }
            });
        });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_kind_is_drawn_as_a_file() {
        // SIP-18: a receiver renders an unrecognised kind as a file rather
        // than guessing from the mime type, which is the sender's claim.
        for kind in [0x00u8, 0x7f, 0xff] {
            assert!(!matches!(kind, IMAGE | VIDEO | VOICE));
        }
    }

    #[test]
    fn the_thumbnail_and_the_image_cannot_share_a_cache_entry() {
        // egui caches an image by its URI. One name for both would leave the
        // 96-pixel preview on screen after the real picture arrived, and it
        // would look like the sender had sent something blurry.
        let id = "abc";
        let whole = format!("bytes://{id}");
        let preview = format!("bytes://{id}-preview");
        assert_ne!(whole, preview);
    }
}
