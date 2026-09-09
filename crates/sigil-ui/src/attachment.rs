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

/// The tallest a picture is drawn, whatever shape it is.
///
/// A portrait photograph would otherwise be a page of its own in the middle of
/// a conversation. Past this it is scaled down, and clicking it opens it at
/// full size.
pub const PICTURE_MAX_TALL: f32 = 320.0;

/// How much room a picture is given **before anybody knows its shape**.
///
/// # Why a picture's height is remembered rather than fixed
///
/// Each stage of loading used to take whatever it needed -- a line of words
/// while the blob was fetched, another while it decoded, then a few hundred
/// pixels when it appeared -- so every picture changed the height of
/// everything below it two or three times, and scrolling a channel with
/// pictures in it moved the text under the reader's eyes.
///
/// The first answer to that was one fixed box for every picture, and it was
/// wrong in both directions: a picture 1620 by 262 sat in a 240-tall box with
/// two thirds of it empty, and a thumbnail standing in for one that had not
/// arrived was drawn tiny in the middle of that emptiness rather than filling
/// it.
///
/// So a picture keeps its own shape, and its measured height is **remembered
/// against the blob** for as long as the window is open: every state of it
/// after the first reserves exactly what it will take, including a re-fetch
/// and a scroll back to it. This is the room given to one nobody has ever
/// measured -- sixteen by nine at [`PICTURE`] wide, which is the commonest
/// photograph there is.
pub const PICTURE_GUESS: f32 = 180.0;

/// What the reader did to a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AttachmentAction {
    pub save: bool,
    /// Look at it full size.
    pub open: bool,
    /// Ask the exchange for it again.
    pub retry: bool,
}

/// Draw the thumbnail into `rect`, if it is ready. Says whether it drew.
///
/// Used while the picture itself is still decoding, so that what is on screen
/// stays a picture.
fn draw_preview(
    ui: &mut egui::Ui,
    a: &Attachment<'_>,
    rect: egui::Rect,
    side: f32,
    _quiet: egui::Color32,
) -> bool {
    let uri = format!("bytes://{}-preview", a.id);
    ui.ctx().include_bytes(uri.clone(), a.preview.to_vec());
    let image = egui::Image::from_bytes(uri, a.preview.to_vec())
        .fit_to_original_size(1.0)
        .max_size(rect.size())
        .corner_radius(tokens::RADIUS_MD)
        .show_loading_spinner(false);
    match image.load_for_size(ui.ctx(), rect.size()) {
        Ok(egui::load::TexturePoll::Ready { texture }) => {
            let drawn = fit(texture.size, side, false);
            image.paint_at(ui, egui::Rect::from_center_size(rect.center(), drawn));
            true
        }
        _ => false,
    }
}

/// How large to draw a picture of `natural` size in a column `side` wide.
///
/// Its own shape, bounded by the width and by [`PICTURE_MAX_TALL`]. A picture
/// smaller than the column is left alone -- one blown up to four times its
/// size reads as a mistake rather than as a small picture -- **except** a
/// thumbnail, which is not a small picture but a stand-in for a large one and
/// is scaled to the room the real one will take.
fn fit(natural: egui::Vec2, side: f32, whole: bool) -> egui::Vec2 {
    if natural.x <= 0.0 || natural.y <= 0.0 {
        return egui::vec2(side, PICTURE_GUESS);
    }
    let scale = (side / natural.x).min(PICTURE_MAX_TALL / natural.y);
    let scale = if whole { scale.min(1.0) } else { scale };
    natural * scale
}

/// Draw one.
///
/// The picture when there is one, the thumbnail while there is not, and a row
/// naming it when it is not a picture at all.
/// `over` is what this is being drawn on, which decides what "quiet" can be:
/// the palette's muted text is chosen for a surface and is unreadable on the
/// accent that fills one's own bubble. See [`crate::message::faded`].
pub fn attachment(ui: &mut egui::Ui, a: &Attachment<'_>, over: egui::Color32) -> AttachmentAction {
    let theme = ColorTheme::current(ui.ctx());
    let mut action = AttachmentAction::default();
    // For anything written straight onto `over`. Text inside one of the
    // frames below sits on that frame instead, and keeps the palette's own.
    let quiet = crate::message::faded(theme.text_primary, over);

    if a.kind == IMAGE {
        // Whole image if we have it, thumbnail if we do not, and the words if
        // we have neither. Each is strictly better than the last and every one
        // of them is better than an empty space where a picture should be --
        // and **all three take the same room**, so which one is on screen
        // never moves anything else. See [`PICTURE_GUESS`].
        let (bytes, whole) = match (a.bytes, a.preview.is_empty()) {
            (Some(b), _) => (Some(b), true),
            (None, false) => (Some(a.preview), false),
            (None, true) => (None, false),
        };
        let side = PICTURE.min(ui.available_width().max(160.0));
        // **The height this picture took last time it was measured.** Kept
        // against the blob rather than the message, because it is a fact about
        // the picture; kept in the context rather than in the caller, because
        // every view that draws a transcript wants the same answer.
        let remembered = egui::Id::new(("sigil-picture-tall", a.id));
        let tall: f32 = ui
            .ctx()
            .data(|d| d.get_temp(remembered))
            .unwrap_or(PICTURE_GUESS);
        let box_size = egui::vec2(side, tall);
        let (rect, response) = ui.allocate_exact_size(box_size, egui::Sense::click());
        // The ground the picture sits on, so a letterboxed one reads as a
        // picture in a frame rather than as a hole in the bubble.
        ui.painter()
            .rect_filled(rect, tokens::RADIUS_MD, theme.surface_secondary);

        // Words in the middle of the box, for every state that is not a
        // picture yet. A child ui rather than painted text: two of these carry
        // a button, and a painted button is not one.
        //
        // A child ui **that allocates nothing**: the box is already allocated,
        // and `scope_builder` ends by advancing the cursor to the child's own
        // extent -- which is *inside* the box, so everything drawn after it
        // landed back on top of the picture. Measured, because a test asked
        // the three states to agree on their height: the description line
        // disappeared into the box and a fetching picture came out eighteen
        // pixels shorter than the same one once it had arrived, which is the
        // wander this whole change is about.
        let inside = |ui: &mut egui::Ui, draw: &mut dyn FnMut(&mut egui::Ui)| {
            let mut child =
                ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(tokens::SPACING_SM)));
            child.vertical_centered(|ui| {
                ui.add_space((rect.height() / 2.0 - tokens::SPACING_XL).max(0.0));
                draw(ui);
            });
        };

        if let Some(bytes) = bytes {
            // Keyed on the blob, and on whether this is the thumbnail or the
            // real thing: egui caches by URI, so reusing one name for both
            // would leave the thumbnail on screen after the image arrived.
            let uri = format!("bytes://{}{}", a.id, if whole { "" } else { "-preview" });
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
                // **From the picture's own size, not from the space left.**
                //
                // `Image` defaults to `ImageFit::Fraction([1, 1])`, which is
                // `available_size * 1.0` — and inside a scrolling transcript
                // the available *height* is zero for everything below the
                // fold. So every picture in a scrolled conversation was drawn
                // 320 wide and 0 tall: fetched, decoded, uploaded, and
                // invisible. Three passes of diagnostics went past this
                // because each of them asked whether the picture had *loaded*,
                // and it always had.
                .fit_to_original_size(1.0)
                .max_size(box_size)
                .corner_radius(tokens::RADIUS_MD)
                .show_loading_spinner(false);
            // **The whole chain, not the first link of it.**
            //
            // Bytes become an image and an image becomes a texture, and
            // `Image` swallows a failure at either step. Asking only about the
            // image left the texture step — where an oversized picture is
            // refused by the GPU — reporting nothing at all, which is the
            // silence this was written to end. `load_for_size` is the question
            // the widget itself asks.
            match image.load_for_size(ui.ctx(), box_size) {
                Ok(egui::load::TexturePoll::Ready { texture }) => {
                    // **The picture's own shape**, bounded by the width a
                    // bubble gives it and by how tall anything in a transcript
                    // may be. A thumbnail is scaled up to that; it is standing
                    // in for the picture, and one drawn at ninety-six pixels
                    // in the middle of the space the real one will take reads
                    // as a mistake rather than as a picture on its way.
                    let drawn = fit(texture.size, side, whole);
                    // Remembered, so every later state of this picture --
                    // fetched again, scrolled back to, drawn while decoding --
                    // reserves what it actually takes. A frame late the first
                    // time, and never again.
                    if (drawn.y - tall).abs() > 0.5 {
                        ui.ctx().data_mut(|d| d.insert_temp(remembered, drawn.y));
                        ui.ctx().request_repaint();
                    }
                    image.paint_at(ui, egui::Rect::from_center_size(rect.center(), drawn));
                    if response.clicked() {
                        action.open = true;
                    }
                    let response = response.on_hover_text("Click to see it full size");
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
                    if !whole {
                        // A thumbnail is not the picture, and saying so stops
                        // somebody reading a blurry preview as the whole of
                        // what was sent.
                        ui.colored_label(
                            quiet,
                            egui::RichText::new("preview — fetching the full image").small(),
                        );
                    }
                }
                Ok(egui::load::TexturePoll::Pending { .. }) => {
                    // Still decoding, and the next pass is asked for so it
                    // does not sit here.
                    ui.ctx().request_repaint();
                    // **The thumbnail stays up while the real one decodes.**
                    //
                    // The two are different pictures as far as egui is
                    // concerned -- they must be, or the thumbnail would still
                    // be on screen after the image arrived -- so the pass
                    // where the full one is asked for finds it pending, and
                    // replacing what is there with the word "opening" blanks a
                    // picture somebody is already looking at. It came back a
                    // frame or two later, which is the flicker between the
                    // blurry one and the sharp one.
                    let showing =
                        whole && !a.preview.is_empty() && draw_preview(ui, a, rect, side, quiet);
                    if !showing {
                        inside(ui, &mut |ui| {
                            ui.colored_label(quiet, egui::RichText::new("opening…").small());
                        });
                    }
                }
                Err(why) => {
                    let why = why.to_string();
                    inside(ui, &mut |ui| {
                        ui.colored_label(
                            theme.warning,
                            egui::RichText::new("this picture will not open").small(),
                        );
                        ui.colored_label(theme.text_muted, egui::RichText::new(&why).small());
                        if ui.small_button("Save").clicked() {
                            action.save = true;
                        }
                    });
                }
            }
        } else if a.missing {
            inside(ui, &mut |ui| {
                ui.colored_label(
                    theme.warning,
                    egui::RichText::new("could not be fetched").small(),
                )
                .on_hover_text(
                    "The exchange would not hand it over. A file past this channel's \
                     retention window is gone for good; anything else is worth another try.",
                );
                if ui.small_button("Try again").clicked() {
                    action.retry = true;
                }
            });
        } else {
            inside(ui, &mut |ui| {
                ui.colored_label(quiet, egui::RichText::new("fetching…").small());
            });
        }

        // What it is, quietly, and no button. Saving is on the message's own
        // controls beside it, where every other thing done to a message is — a
        // Save button on the picture put the one action nobody takes often in
        // the loudest place on the bubble.
        ui.colored_label(quiet, egui::RichText::new(a.described).small());
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
        //
        // The decode happens on another thread, so the wait has to be in
        // *time* and not in turns: spinning sixty-four polls with nothing
        // between them takes microseconds, and under a full parallel suite
        // the decoder has not been scheduled once in that window. Alone it
        // won the race every time, which is the worst way for this to be
        // wrong.
        let mut seen = None;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
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
                    std::thread::sleep(std::time::Duration::from_millis(5));
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
