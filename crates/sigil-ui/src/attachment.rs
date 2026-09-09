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
    /// The exchange was asked for it and would not give it.
    ///
    /// **Not the same as "not yet".** A blob past its retention window is
    /// gone and asking again four times a second will not bring it back, so
    /// the fetch is not retried — which left a picture that had failed and one
    /// that had not been reached yet looking identical, and neither of them
    /// said anything at all.
    pub missing: bool,
}

/// How large a picture is drawn in a transcript.
///
/// **The bubble is measured against this**, so the two must agree: a message
/// carrying a file used to ask for infinite width, which made every one of
/// them as wide as the pane allowed — including one whose entire content is a
/// row reading `[image, 28 KiB]`.
pub const PICTURE: f32 = 320.0;

/// What the reader did to a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttachmentAction {
    pub save: bool,
    /// Look at it full size.
    pub open: bool,
    /// Ask the exchange for it again.
    pub retry: bool,
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
            let side = if whole { PICTURE } else { 96.0 };
            // **Registered here, and then asked about.**
            //
            // `Image::from_bytes` registers its bytes when the widget loads
            // them — during `add`, not when it is built — so asking the
            // context beforehand answered *"Bytes not found. Did you forget to
            // call Context::include_bytes?"* for every picture, and the
            // diagnostic that was meant to explain a failure became one.
            //
            // Doing the include ourselves makes the question answerable: from
            // here on, an error is about the bytes and not about the order
            // things happened in.
            ui.ctx().include_bytes(uri.clone(), bytes.to_vec());
            let image = egui::Image::from_bytes(uri.clone(), bytes.to_vec())
                // Both, not only the height. A wide picture given an unbounded
                // width takes the whole pane and pushes the bubble off it.
                .max_size(egui::vec2(side, side))
                .corner_radius(tokens::RADIUS_MD)
                .show_loading_spinner(false)
                .sense(egui::Sense::click());
            // **The whole chain, not the first link of it.**
            //
            // Bytes become an image and an image becomes a texture, and
            // `Image` swallows a failure at either step. Asking only about the
            // image left the texture step — where an oversized picture is
            // refused by the GPU — reporting nothing at all, which is the
            // silence this was written to end. `load_for_size` is the question
            // the widget itself asks.
            let poll = image.load_for_size(ui.ctx(), ui.available_size());
            if let Ok(egui::load::TexturePoll::Pending { .. }) = poll {
                // Still decoding. Saying so beats an empty space, and the next
                // pass is asked for so it does not sit here.
                ui.colored_label(theme.text_muted, egui::RichText::new("opening…").small());
                ui.ctx().request_repaint();
                ui.colored_label(theme.text_muted, egui::RichText::new(a.described).small());
                return action;
            }
            if let Err(why) = poll {
                // In words, where the picture would have been.
                egui::Frame::NONE
                    .fill(theme.surface_secondary)
                    .corner_radius(tokens::RADIUS_MD)
                    .inner_margin(egui::Margin::symmetric(
                        tokens::SPACING_SM as i8,
                        tokens::SPACING_XS as i8,
                    ))
                    .show(ui, |ui| {
                        ui.vertical(|ui| {
                            ui.colored_label(
                                theme.warning,
                                egui::RichText::new("this picture will not open").small(),
                            );
                            ui.colored_label(
                                theme.text_muted,
                                egui::RichText::new(why.to_string()).small(),
                            );
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    theme.text_muted,
                                    egui::RichText::new(a.described).small(),
                                );
                                if ui.small_button("Save").clicked() {
                                    action.save = true;
                                }
                            });
                        });
                    });
                return action;
            }
            let response = ui.add(image);
            // The thumbnail in the transcript is a thumbnail. Clicking it is
            // how anybody expects to see the picture itself.
            if response.clicked() {
                action.open = true;
            }
            let response = response.on_hover_text("Click to see it full size");
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
                if ui.button("See it full size").clicked() {
                    action.open = true;
                    ui.close();
                }
                if ui.button("Save as…").clicked() {
                    action.save = true;
                    ui.close();
                }
            });
            // What it is, quietly, and no button. Saving is on the message's
            // own controls beside it, where every other thing done to a
            // message is — a Save button on the picture put the one action
            // nobody takes often in the loudest place on the bubble.
            ui.colored_label(theme.text_muted, egui::RichText::new(a.described).small());
            return action;
        }
    }

    // A picture that was asked for and refused, or one still on its way. Both
    // drew as a bare filename before, which says nothing about which.
    if a.kind == IMAGE {
        egui::Frame::NONE
            .fill(theme.surface_secondary)
            .corner_radius(tokens::RADIUS_MD)
            .inner_margin(egui::Margin::symmetric(
                tokens::SPACING_SM as i8,
                tokens::SPACING_XS as i8,
            ))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(theme.text_muted, egui::RichText::new(a.described).small());
                    if a.missing {
                        ui.colored_label(
                            theme.warning,
                            egui::RichText::new("could not be fetched").small(),
                        )
                        .on_hover_text(
                            "The exchange would not hand it over. A file past this \
                             channel's retention window is gone for good; anything else \
                             is worth another try.",
                        );
                        if ui.small_button("Try again").clicked() {
                            action.retry = true;
                        }
                    } else {
                        ui.colored_label(
                            theme.text_muted,
                            egui::RichText::new("fetching…").small(),
                        );
                    }
                });
            });
        return action;
    }

    // Not a picture. A row that says what it is and offers the only thing that
    // can be done with it.
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
                // A file that cannot be drawn keeps its button: there is
                // nothing else to do with it, and nothing on screen to click.
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

    /// A one-pixel PNG, written by hand.
    ///
    /// Not a fixture file: what is being tested is that egui was given a
    /// decoder, and a decoder is exactly what would be needed to *produce* a
    /// fixture, so the bytes are spelled out.
    fn a_png() -> Vec<u8> {
        // 1x1 opaque red, stored (uncompressed) deflate, CRCs computed.
        fn crc(bytes: &[&[u8]]) -> u32 {
            let mut c = 0xFFFF_FFFFu32;
            for part in bytes {
                for &x in *part {
                    c ^= x as u32;
                    for _ in 0..8 {
                        c = if c & 1 != 0 {
                            0xEDB8_8320 ^ (c >> 1)
                        } else {
                            c >> 1
                        };
                    }
                }
            }
            c ^ 0xFFFF_FFFF
        }
        fn chunk(kind: &[u8], data: &[u8]) -> Vec<u8> {
            let mut out = (data.len() as u32).to_be_bytes().to_vec();
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            out.extend_from_slice(&crc(&[kind, data]).to_be_bytes());
            out
        }

        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0, 0, 0]));
        // One scanline: a filter byte, then one RGB pixel.
        let raw = [0u8, 255, 0, 0];
        let mut z = vec![0x78, 0x01, 0x01, 4, 0, 0xFB, 0xFF];
        z.extend_from_slice(&raw);
        let (mut a, mut b) = (1u32, 0u32);
        for &x in &raw {
            a = (a + x as u32) % 65521;
            b = (b + a) % 65521;
        }
        z.extend_from_slice(&((b << 16) | a).to_be_bytes());
        png.extend(chunk(b"IDAT", &z));
        png.extend(chunk(b"IEND", b""));
        png
    }

    /// egui was actually given something that can decode a PNG.
    ///
    /// # Why this is not obvious
    ///
    /// `egui::Image::from_bytes` decodes nothing itself: it hands the bytes to
    /// a registered loader, and with none registered it draws a broken-picture
    /// icon — which reads as *this file is damaged* and means only *I have no
    /// decoder*. Nothing called `install_loaders` for months. Asking the
    /// context to load the image is the whole check; drawing it would only
    /// tell us a widget was allocated.
    #[test]
    fn an_image_can_actually_be_decoded() {
        let ctx = egui::Context::default();
        crate::install_loaders(&ctx);
        let uri = "bytes://one.png";
        let png = a_png();

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.add(egui::Image::from_bytes(uri, png.clone()));
        });
        output.textures_delta.clear();

        // Loaders decode on demand and may answer `Pending` the first time, so
        // ask until it settles rather than asserting on one frame.
        let mut seen = None;
        for _ in 0..64 {
            match ctx.try_load_image(uri, egui::SizeHint::Scale(1.0.into())) {
                Ok(egui::load::ImagePoll::Ready { image }) => {
                    seen = Some(image.size);
                    break;
                }
                Ok(egui::load::ImagePoll::Pending { .. }) => {
                    let mut o = ctx.run_ui(egui::RawInput::default(), |ui| {
                        ui.add(egui::Image::from_bytes(uri, png.clone()));
                    });
                    o.textures_delta.clear();
                }
                Err(e) => panic!("nothing can decode a PNG: {e}"),
            }
        }
        assert_eq!(
            seen,
            Some([1, 1]),
            "the image never decoded, so every attachment draws as a broken picture"
        );
    }

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
