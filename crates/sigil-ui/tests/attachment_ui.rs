//! What a message's file looks like when it will not open.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::theme;

fn said(h: &Harness<'static>) -> String {
    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let n = node.accesskit_node();
        if let Some(l) = n.label() {
            out.push(l.to_string());
        }
        if let Some(v) = n.value() {
            out.push(v.to_string());
        }
        for c in node.children() {
            walk(c, out);
        }
    }
    let mut found = Vec::new();
    walk(h.root(), &mut found);
    found.join(" | ")
}

/// How tall the attachment drew, in a ui with the height it is given.
///
/// `None` for the height means what a scrolling transcript actually gives a
/// message below the fold: **zero**. `bytes` of `None` is a picture that has
/// not been fetched yet.
fn tall(bytes: Option<std::sync::Arc<[u8]>>, height: Option<f32>) -> f32 {
    let took = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let seen = took.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(500.0, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            ui.allocate_ui_with_layout(
                egui::vec2(320.0, height.unwrap_or(0.0)),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let before = ui.min_rect().height();
                    sigil_ui::attachment(
                        ui,
                        &sigil_ui::Attachment {
                            kind: sigil_ui::attachment::IMAGE,
                            described: "[image, 28 KiB]",
                            preview: sigil_ui::attachment::no_preview(),
                            bytes: bytes.as_ref(),
                            missing: false,
                            held: false,
                            size: 0,
                            video: None,
                            sending: false,
                            coming: None,
                            waveform: &[],
                            duration_ms: None,
                            voice: None,
                            id: "sized",
                        },
                        sigil::ColorTheme::current(&ctx).surface_elevated,
                    );
                    let grew = ui.min_rect().height() - before;
                    seen.store(grew.max(0.0) as u32, std::sync::atomic::Ordering::Relaxed);
                },
            );
        });
    // Twice: the texture is not ready on the pass that asks for it.
    h.run();
    h.run();
    took.load(std::sync::atomic::Ordering::Relaxed) as f32
}

fn drawn(bytes: std::sync::Arc<[u8]>) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(500.0, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            sigil_ui::install_loaders(&ctx);
            sigil_ui::attachment(
                ui,
                &sigil_ui::Attachment {
                    kind: sigil_ui::attachment::IMAGE,
                    described: "[image, 28 KiB]",
                    preview: sigil_ui::attachment::no_preview(),
                    bytes: Some(&bytes),
                    missing: false,
                    held: false,
                    size: 0,
                    video: None,
                    sending: false,
                    coming: None,
                    waveform: &[],
                    duration_ms: None,
                    voice: None,
                    id: "notapicture",
                },
                sigil::ColorTheme::current(&ctx).surface_elevated,
            );
        })
}

/// A PNG of any size, spelled out.
///
/// A decoder is exactly what would be needed to read a fixture file, and a
/// decoder is part of what these tests are about. The zlib stream is stored
/// blocks -- uncompressed, which is a legal deflate stream and needs no
/// compressor here.
fn png_of(w: u32, h: u32) -> Vec<u8> {
    fn crc(parts: &[&[u8]]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for part in parts {
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

    let mut header = w.to_be_bytes().to_vec();
    header.extend_from_slice(&h.to_be_bytes());
    // 8 bits per channel, colour type 2 (rgb), no interlace.
    header.extend_from_slice(&[8, 2, 0, 0, 0]);

    // One filter byte per row, then three bytes a pixel.
    let mut raw = Vec::with_capacity((h * (1 + w * 3)) as usize);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend_from_slice(&[(x % 251) as u8, (y % 251) as u8, 128]);
        }
    }

    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65_535).enumerate() {
        let last = (i + 1) * 65_535 >= raw.len();
        z.push(u8::from(last));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &header));
    png.extend(chunk(b"IDAT", &z));
    png.extend(chunk(b"IEND", b""));
    png
}

/// A one-pixel PNG, spelled out.
///
/// A decoder is exactly what would be needed to produce a fixture file, and a
/// decoder is what this is testing.
fn a_png() -> &'static [u8] {
    static PNG: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    PNG.get_or_init(|| {
        fn crc(parts: &[&[u8]]) -> u32 {
            let mut c = 0xFFFF_FFFFu32;
            for part in parts {
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
    })
    .as_slice()
}

/// **A picture that is fine says nothing.**
///
/// The control the failure test did not have, and needed: its first version
/// passed on any bytes at all, because the diagnostic asked the context about
/// a URI whose bytes had not been registered yet and got *"Bytes not found"*
/// every time. It reported a decode failure for every picture in the
/// application, including the ones that worked.
#[test]
fn a_picture_that_is_fine_is_not_reported_as_broken() {
    let mut h = drawn(a_png().into());
    h.run();
    h.run();
    let said = said(&h);
    assert!(
        !said.contains("will not open"),
        "a perfectly good PNG is reported as broken: {said}"
    );
}

/// Bytes that are not a picture say what went wrong, rather than drawing
/// nothing.
///
/// The loader answers an error, `Image` swallows it, and the bubble is a
/// filename with an empty space where a picture should be — which looks
/// exactly like a picture that has not arrived, a bubble drawn too small, and
/// a loader that was never installed. All three were suspected in turn, and
/// the interface knew which the whole time.
#[test]
fn something_that_is_not_a_picture_says_what_went_wrong() {
    let mut h = drawn(std::sync::Arc::from(&b"this is not a picture"[..]));
    h.run();
    let said = said(&h);
    assert!(
        said.contains("will not open"),
        "a picture that cannot be decoded draws nothing at all: {said}"
    );
    // And what it is stays on screen, with the one thing still worth doing to
    // it: the bytes are here, so they can still be written to a file.
    assert!(said.contains("[image, 28 KiB]"), "{said}");
    assert!(said.contains("Save"), "{said}");
}

/// A picture the texture step refuses is reported too.
///
/// Bytes become an image and an image becomes a texture, and `Image` swallows
/// a failure at either step. The first version of this diagnostic asked only
/// about the image, so the texture step — where an oversized picture is
/// refused — went on reporting nothing at all, which is the silence the whole
/// thing was written to end.
#[test]
fn the_whole_chain_is_asked_about_and_not_only_the_first_link() {
    // A harness has no GPU, so a texture never becomes ready here: what this
    // pins is that the widget asks `load_for_size` — the question covering
    // both steps — rather than `try_load_image`, which covers one.
    let mut h = drawn(a_png().into());
    h.run();
    let said = said(&h);
    assert!(
        !said.contains("Bytes not found"),
        "the bytes are still being asked about before they are registered: {said}"
    );
    // Either it is on its way or it is here; neither is silence.
    assert!(
        said.contains("opening") || !said.contains("will not open"),
        "a good picture is neither drawn nor explained: {said}"
    );
}

/// A picture is sized by the picture, not by the room left below it.
///
/// `Image` defaults to `ImageFit::Fraction([1, 1])` — `available_size * 1.0` —
/// and inside a scrolling transcript the available *height* is zero for
/// everything below the fold. Every picture in a scrolled conversation was
/// therefore drawn 320 wide and **0 tall**: fetched, decoded, uploaded to the
/// GPU, and invisible.
///
/// Three rounds of diagnostics went straight past it, because each of them
/// asked whether the picture had *loaded*, and it always had. This asks the
/// only question that was failing: how much room did it take.
#[test]
fn a_picture_takes_room_even_where_there_is_none_left() {
    let with_room = tall(Some(a_png().into()), Some(400.0));
    let with_none = tall(Some(a_png().into()), None);
    assert!(
        with_none > 0.0,
        "a picture below the fold takes no height at all, which is how it \
         becomes invisible"
    );
    // And the same picture either way: its size is its own.
    assert_eq!(
        with_none, with_room,
        "the space left over changes how big the picture is"
    );
}

/// A picture keeps its own shape.
///
/// One fixed box for every picture was wrong in both directions: a picture
/// 1620 by 262 sat in a 240-tall box with two thirds of it empty.
///
/// Self-calibrating, so it measures the shape and not the furniture around it:
/// two pictures the same width and twice the height apart must differ in drawn
/// height by exactly that, scaled by the width they are given. Everything else
/// on the row -- the description underneath, the spacing -- is the same in
/// both and cancels.
#[test]
fn a_picture_is_drawn_in_its_own_shape() {
    let wide: std::sync::Arc<[u8]> = png_of(400, 100).into();
    let taller: std::sync::Arc<[u8]> = png_of(400, 200).into();

    let short = tall(Some(wide.clone()), Some(400.0));
    let deep = tall(Some(taller.clone()), Some(400.0));
    // 400 wide into a column of 320 is a scale of 0.8, so 100 more pixels of
    // picture is 80 more pixels on screen.
    assert!(
        (deep - short - 80.0).abs() <= 4.0,
        "a picture twice as tall drew {deep} against {short}, a difference of \
         {} where the shape says 80",
        deep - short
    );
    // And the wide one is nowhere near the tallest a picture may be: that was
    // the letterbox.
    assert!(
        short < 160.0,
        "a picture four times as wide as it is tall took {short} pixels"
    );
}

/// Every state of a picture reserves what the picture takes, once anything has
/// measured it.
///
/// Each stage used to take whatever it needed -- a line of words while the
/// blob was fetched, another while it decoded, then a few hundred pixels when
/// it appeared -- so every picture changed the height of everything below it
/// two or three times as it loaded, and scrolling a channel with pictures in
/// it moved the text under the reader's eyes. The height is remembered against
/// the blob, so a re-fetch or a scroll back to it reserves the same room.
///
/// **One context throughout**, because that is where the memory lives: a fresh
/// harness per state would be a fresh window, and a fresh window has never
/// seen the picture.
#[test]
fn a_picture_reserves_what_it_took_last_time() {
    let bytes: std::sync::Arc<[u8]> = png_of(400, 100).into();
    let shown = std::rc::Rc::new(std::cell::RefCell::new(Some(bytes.clone())));
    let took = std::rc::Rc::new(std::cell::Cell::new(0.0f32));

    let (state, seen) = (shown.clone(), took.clone());
    let mut h = Harness::builder()
        .with_size(egui::vec2(500.0, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            ui.allocate_ui_with_layout(
                egui::vec2(320.0, 400.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let before = ui.min_rect().height();
                    sigil_ui::attachment(
                        ui,
                        &sigil_ui::Attachment {
                            kind: sigil_ui::attachment::IMAGE,
                            described: "[image, 28 KiB]",
                            preview: sigil_ui::attachment::no_preview(),
                            bytes: state.borrow().as_ref(),
                            missing: false,
                            held: false,
                            size: 0,
                            video: None,
                            sending: false,
                            coming: None,
                            waveform: &[],
                            duration_ms: None,
                            voice: None,
                            id: "remembered",
                        },
                        sigil::ColorTheme::current(&ctx).surface_elevated,
                    );
                    seen.set(ui.min_rect().height() - before);
                },
            );
        });
    // Three: the texture is not ready on the pass that asks for it, and the
    // height it settles on is reserved the pass after that.
    h.run();
    h.run();
    h.run();
    let drawn = took.get();
    assert!(drawn > 60.0, "the picture drew nothing: {drawn}");
    // **The room follows the picture.** Without that, the two halves of this
    // test agree at whatever was guessed and prove nothing: a box that never
    // changes is trivially the same before and after. 400 by 100 into a column
    // of 320 is 80 pixels of picture, and the guess for one nobody has
    // measured is 180.
    assert!(
        drawn < 140.0,
        "the picture is 80 pixels tall and it reserved {drawn}, which is the \
         guess rather than the measurement"
    );

    // The blob goes away -- fetched again, or scrolled far enough that it was
    // dropped. The room it takes must not.
    *shown.borrow_mut() = None;
    h.run();
    h.run();
    assert!(
        (took.get() - drawn).abs() < 1.0,
        "a picture on its way took {} where the same picture takes {drawn}",
        took.get()
    );
}

/// The thumbnail stays up while the picture itself decodes.
///
/// The two are different pictures as far as egui is concerned -- they have to
/// be, or the thumbnail would still be on screen after the real one arrived --
/// so the pass that first asks for the full image finds it pending. Putting
/// the word "opening" there blanks a picture somebody is already looking at,
/// and it comes back a frame or two later: the flicker between the blurry one
/// and the sharp one.
#[test]
fn the_thumbnail_stays_up_while_the_picture_decodes() {
    let preview: std::sync::Arc<[u8]> = png_of(40, 10).into();
    let full: std::sync::Arc<[u8]> = png_of(400, 100).into();
    let bytes = std::rc::Rc::new(std::cell::RefCell::new(None::<std::sync::Arc<[u8]>>));
    let words = std::rc::Rc::new(std::cell::RefCell::new(String::new()));

    let (shown, said_now) = (bytes.clone(), words.clone());
    let mut h = Harness::builder()
        .with_size(egui::vec2(500.0, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            sigil_ui::attachment(
                ui,
                &sigil_ui::Attachment {
                    kind: sigil_ui::attachment::IMAGE,
                    described: "[image, 28 KiB]",
                    preview: &preview,
                    bytes: shown.borrow().as_ref(),
                    missing: false,
                    held: false,
                    size: 0,
                    video: None,
                    sending: false,
                    coming: None,
                    waveform: &[],
                    duration_ms: None,
                    voice: None,
                    id: "swapping",
                },
                sigil::ColorTheme::current(&ctx).surface_elevated,
            );
            said_now.borrow_mut().clear();
        });
    // The thumbnail, on screen and settled.
    h.run();
    h.run();

    // The picture arrives. On this very pass it has not decoded yet -- that is
    // the pass this is about.
    *bytes.borrow_mut() = Some(full.clone());
    h.step();
    let _ = words;
    assert!(
        !said(&h).contains("opening"),
        "the thumbnail was replaced by words while the picture decoded: {}",
        said(&h)
    );
}

/// What the caption under a thumbnail says, and what its button does, for
/// each reason the picture itself is not here.
fn captioned(
    missing: bool,
    held: bool,
) -> (
    Harness<'static>,
    std::rc::Rc<std::cell::Cell<sigil_ui::AttachmentAction>>,
) {
    let preview: std::sync::Arc<[u8]> = png_of(40, 10).into();
    let did = std::rc::Rc::new(std::cell::Cell::new(sigil_ui::AttachmentAction::default()));
    let seen = did.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(500.0, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            let action = sigil_ui::attachment(
                ui,
                &sigil_ui::Attachment {
                    kind: sigil_ui::attachment::IMAGE,
                    described: "[image, 4.1 MiB]",
                    preview: &preview,
                    bytes: None,
                    missing,
                    held,
                    size: 4_300_000,
                    video: None,
                    sending: false,
                    coming: None,
                    waveform: &[],
                    duration_ms: None,
                    voice: None,
                    id: "captioned",
                },
                sigil::ColorTheme::current(&ctx).surface_elevated,
            );
            if action != sigil_ui::AttachmentAction::default() {
                seen.set(action);
            }
        });
    h.run();
    h.run();
    (h, did)
}

/// A thumbnail under a picture too big to fetch unasked says how big, and
/// offers to fetch it; one whose fetch failed says so and offers to try
/// again. Neither says "fetching".
///
/// Both did: the caption was one string for every reason the picture was
/// not there, and "fetching the full image" over a fetch that would never
/// start is what a reader waited on for a gif.
#[test]
fn the_caption_under_a_thumbnail_says_why_the_picture_is_not_here() {
    let (mut h, did) = captioned(false, true);
    let words = said(&h);
    assert!(
        words.contains("4.1 MiB") && !words.contains("fetching"),
        "a held picture should say its size and not promise a fetch: {words}"
    );
    h.get_by_label("Fetch").click();
    h.run();
    assert!(did.get().fetch, "Fetch should ask for the picture");
    assert!(!did.get().retry);

    let (mut h, did) = captioned(true, false);
    let words = said(&h);
    assert!(
        words.contains("could not be fetched") && !words.contains("fetching"),
        "a failed fetch should say so: {words}"
    );
    h.get_by_label("Try again").click();
    h.run();
    assert!(did.get().retry, "Try again should ask again");
    assert!(!did.get().fetch);

    let (h, _) = captioned(false, false);
    assert!(
        said(&h).contains("fetching the full image"),
        "one on its way still says so: {}",
        said(&h)
    );
}

#[test]
fn sizes_are_said_in_round_units() {
    use sigil_ui::attachment::human;
    assert_eq!(human(900), "900 B");
    assert_eq!(human(28 * 1024), "28 KiB");
    assert_eq!(human(4_300_000), "4.1 MiB");
}

/// A gif of two frames, each one colour, `delay` centiseconds apart.
fn gif_of(delay: u16) -> Vec<u8> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, RgbaImage};
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for colour in [[255u8, 0, 0, 255], [0, 0, 255, 255]] {
            let img = RgbaImage::from_pixel(8, 8, image::Rgba(colour));
            enc.encode_frame(Frame::from_parts(
                img,
                0,
                0,
                Delay::from_numer_denom_ms(delay as u32 * 10, 1),
            ))
            .unwrap();
        }
    }
    out
}

/// A gif is decoded off the thread that asked for it, and plays once it is.
///
/// `egui_extras`'s loader decodes every frame on the calling thread while
/// holding its cache lock, and that is a window frozen for as long as a
/// multi-megabyte gif takes -- once per launch. sigil's answers "pending"
/// from the first ask and fills in on a thread; that first answer is what
/// tells the two loaders apart, so it is what is asserted, along with the
/// frames actually arriving and differing.
#[test]
fn a_gif_is_decoded_off_the_interface_thread_and_then_plays() {
    let bytes: std::sync::Arc<[u8]> = gif_of(10).into();
    let h = Harness::builder()
        .with_size(egui::vec2(200.0, 200.0))
        .build_ui(|ui| {
            sigil_ui::install_loaders(ui.ctx());
        });
    let ctx = h.ctx.clone();
    ctx.include_bytes("bytes://two-frames", egui::load::Bytes::Shared(bytes));
    let hint = egui::load::SizeHint::default();

    // The first ask does not decode: it hands the bytes to a thread and says
    // so. The loader that ships with egui_extras would say Ready here.
    let first = ctx.try_load_image("bytes://two-frames#0", hint);
    let pending = match &first {
        Ok(egui::load::ImagePoll::Pending { .. }) => "pending",
        Ok(egui::load::ImagePoll::Ready { .. }) => "ready",
        Err(e) => panic!("the first ask failed: {e}"),
    };
    assert_eq!(
        pending, "pending",
        "the first ask should be answered pending, not decoded on this thread"
    );

    // Then the frames come, and they are different frames.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let frame = |i: usize| loop {
        match ctx.try_load_image(&format!("bytes://two-frames#{i}"), hint) {
            Ok(egui::load::ImagePoll::Ready { image }) => break image,
            Ok(egui::load::ImagePoll::Pending { .. }) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Ok(_) => panic!("frame {i} never arrived"),
            Err(e) => panic!("frame {i} failed: {e}"),
        }
    };
    let (red, blue) = (frame(0), frame(1));
    assert_eq!(red.pixels[0], egui::Color32::from_rgb(255, 0, 0));
    assert_eq!(blue.pixels[0], egui::Color32::from_rgb(0, 0, 255));

    // And the timing the widget animates by was put where it reads it.
    let durations: Option<egui::FrameDurations> =
        ctx.data(|d| d.get_temp(egui::Id::new("bytes://two-frames")));
    let all: Vec<_> = durations
        .expect("no frame durations")
        .all()
        .copied()
        .collect();
    assert_eq!(all, vec![std::time::Duration::from_millis(100); 2]);
}

/// A voice note, drawn: the bars the **row itself** painted, and what it said.
///
/// Counted off the shapes the frame produced rather than by calling the
/// drawing function again beside it -- what is in question is whether the
/// row draws a waveform, and a second call proves only that the function
/// works when something calls it.
fn voice(levels: &'static [u8], duration_ms: Option<u64>, wide: f32) -> (usize, String) {
    fn bars(shape: &egui::Shape, found: &mut usize) {
        match shape {
            egui::Shape::Rect(r) => {
                // A bar: a few points wide and no taller than the row.
                if r.rect.width() > 0.0
                    && r.rect.width() <= 3.5
                    && r.rect.height() <= sigil::tokens::ICON_MD
                {
                    *found += 1;
                }
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| bars(s, found)),
            _ => {}
        }
    }
    let ctx = egui::Context::default();
    theme::install(&ctx, theme::light(), theme::dark());
    let mut found = 0;
    let mut said = String::new();
    // Twice: the first pass is where a ui learns what it is given.
    for _ in 0..2 {
        found = 0;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(wide, 60.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        sigil_ui::attachment(
                            ui,
                            &sigil_ui::Attachment {
                                kind: sigil_ui::attachment::VOICE,
                                described: "[voice note 12s]",
                                preview: sigil_ui::attachment::no_preview(),
                                bytes: None,
                                missing: false,
                                held: false,
                                size: 40_000,
                                video: None,
                                sending: false,
                                coming: None,
                                waveform: levels,
                                duration_ms,
                                voice: None,
                                id: "voice",
                            },
                            sigil::ColorTheme::current(ui.ctx()).surface_elevated,
                        );
                    },
                );
            });
        });
        // Nobody is uploading these anywhere; epaint panics on a dropped
        // delta that was never handled.
        out.textures_delta.clear();
        for clipped in &out.shapes {
            bars(&clipped.shape, &mut found);
        }
        said = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ");
    }
    (found, said)
}
/// SIP-18 puts the levels in the message so a voice note has a shape before
/// any audio is fetched — and nothing here fetches any.
#[test]
fn a_voice_note_draws_its_waveform_with_no_audio() {
    let levels: &[u8] = &[0, 40, 120, 200, 255, 200, 120, 40];
    let (bars, said) = voice(levels, Some(12_000), 240.0);
    assert_eq!(bars, levels.len(), "every level got a bar");
    assert!(
        said.contains("0:12"),
        "the row says how long it runs: {said}"
    );
}

/// The negative control: no levels, no bars. A sender may send none, and a
/// row that drew something anyway would pass the test above for free.
#[test]
fn a_voice_note_without_levels_draws_no_bars() {
    let (bars, said) = voice(&[], Some(12_000), 240.0);
    assert_eq!(bars, 0, "bars were drawn for a note with no waveform");
    assert!(
        said.contains("0:12"),
        "it still says how long it runs: {said}"
    );
}

/// A note longer than the row has pitches for is read at a coarser pitch
/// rather than squeezed: a comb of hairlines is a texture, not a shape.
#[test]
fn a_long_waveform_thins_rather_than_smears() {
    let levels: &[u8] = &[80; 200];
    let (bars, _) = voice(levels, Some(200_000), 140.0);
    assert!(
        bars > 0 && bars < levels.len(),
        "{bars} of {}",
        levels.len()
    );
    assert!(bars as f32 <= 140.0 / 5.0, "{bars} bars across 140 points");
}

/// A voice note nobody has pressed has no play control and no fetch: SIP-18
/// puts the shape and the length in the message, and the audio stays at the
/// exchange until somebody asks for it.
#[test]
fn a_voice_note_nobody_pressed_is_a_shape_and_a_length() {
    let (_, said) = voice(&[0, 40, 120, 200, 255], Some(12_000), 240.0);
    assert!(said.contains("0:12"), "{said}");
    assert!(!said.contains("Pause"), "{said}");
}

/// Pressing it asks to play, and the row says which way it is about to go.
#[test]
fn the_control_says_play_and_then_pause() {
    for (playing, word, other) in [(false, "Play", "Pause"), (true, "Pause", "Play")] {
        let mut h = a_note_row(sigil_ui::attachment::Voice {
            playing,
            ..Default::default()
        });
        h.run();
        h.get_by_label(word);
        assert!(
            h.query_by_label(other).is_none(),
            "both controls were drawn at once"
        );
    }
}

/// While it plays the clock counts up beside the whole length, which is the
/// pair of numbers somebody actually wants.
#[test]
fn a_playing_note_says_where_it_is_and_how_long_it_is() {
    let mut h = a_note_row(sigil_ui::attachment::Voice {
        playing: true,
        done: 0.25,
        position_ms: 3_000,
        ..Default::default()
    });
    h.run();
    let said = said(&h);
    assert!(said.contains("0:03 / 0:12"), "{said}");
}

/// Asked for and not here: a spinner where the control is, because there is
/// nothing to play until the blob arrives.
#[test]
fn a_note_being_fetched_shows_no_control() {
    let mut h = a_note_row(sigil_ui::attachment::Voice {
        fetching: true,
        ..Default::default()
    });
    // `run_steps`, not `run`: a spinner asks for the next frame forever,
    // and `run` waits for the ui to settle.
    h.run_steps(2);
    assert!(h.query_by_label("Play").is_none());
    assert!(h.query_by_label("Pause").is_none());
}

/// One that will not decode says so where the control was, rather than
/// offering a press that does nothing.
#[test]
fn a_note_that_will_not_decode_says_so() {
    let mut h = a_note_row(sigil_ui::attachment::Voice {
        trouble: Some("this voice note will not play"),
        ..Default::default()
    });
    h.run();
    assert!(
        h.query_by_label("Play").is_none(),
        "a press that does nothing"
    );
    let said = said(&h);
    assert!(said.contains("0:12"), "the length stays: {said}");
}

/// A voice row with a player attached to it.
fn a_note_row(voice: sigil_ui::attachment::Voice) -> Harness<'static> {
    a_note_row_coming(voice, None)
}

fn a_note_row_coming(
    voice: sigil_ui::attachment::Voice,
    coming: Option<(u64, u64)>,
) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(360.0, 200.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            ui.allocate_ui_with_layout(
                egui::vec2(300.0, 60.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    sigil_ui::attachment(
                        ui,
                        &sigil_ui::Attachment {
                            kind: sigil_ui::attachment::VOICE,
                            described: "[voice note 12s]",
                            preview: sigil_ui::attachment::no_preview(),
                            bytes: None,
                            missing: false,
                            held: false,
                            size: 40_000,
                            video: None,
                            sending: false,
                            coming,
                            waveform: &[0, 40, 120, 200, 255, 200, 120, 40],
                            duration_ms: Some(12_000),
                            voice: Some(voice),
                            id: "voice",
                        },
                        sigil::ColorTheme::current(&ctx).surface_elevated,
                    );
                },
            );
        })
}

/// A picture coming down, drawn at a given fraction of the way through.
fn coming_at(done: u64, all: u64) -> Harness<'static> {
    let preview: std::sync::Arc<[u8]> = png_of(40, 40).into();
    let mut h = Harness::builder()
        .with_size(egui::vec2(400.0, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            sigil_ui::attachment(
                ui,
                &sigil_ui::Attachment {
                    kind: sigil_ui::attachment::IMAGE,
                    described: "[image, 4.1 MiB]",
                    preview: &preview,
                    bytes: None,
                    missing: false,
                    held: false,
                    size: all,
                    video: None,
                    sending: false,
                    coming: Some((done, all)),
                    waveform: &[],
                    duration_ms: None,
                    voice: None,
                    id: "coming",
                },
                sigil::ColorTheme::current(&ctx).surface_elevated,
            );
        });
    // Twice: the thumbnail is not a texture on the pass that asks for it,
    // and the ring is drawn over where it lands.
    h.run();
    h.run();
    h
}

/// How much of the frame the ring's own colour covers.
///
/// The arc is the accent and nothing else drawn here is, so counting those
/// pixels counts the arc -- which is a measure of how far round it went,
/// without needing to know where egui laid the picture out.
fn arc_pixels(image: &image::RgbaImage) -> usize {
    let accents = [theme::light().accent, theme::dark().accent];
    image
        .pixels()
        .filter(|p| {
            let [r, g, b, a] = p.0;
            a > 200
                && accents.iter().any(|c| {
                    let near = |x: u8, y: u8| x.abs_diff(y) <= 24;
                    near(r, c.r()) && near(g, c.g()) && near(b, c.b())
                })
        })
        .count()
}

/// The ring over a thumbnail is a **measure**, not a mark: a quarter of the
/// way through and three quarters of the way through are different pictures.
///
/// # What this is really testing
///
/// That the arc is drawn *from the fraction*. A ring drawn always-full, or
/// always-empty, or from a constant, would satisfy "there is a ring over the
/// thumbnail" and say nothing at all about the download -- which is the only
/// reason the ring exists. So the assertion is on how much of the accent is
/// on screen, which is how far round the arc went and nothing else.
///
/// `sqex`'s `a_download_says_how_far_along_it_is` is the other half of this,
/// and proves the numbers reaching it are real.
#[test]
fn the_ring_over_a_coming_picture_is_drawn_from_how_far_along_it_is() {
    let all = 4_300_000u64;
    let nothing = arc_pixels(&coming_at(0, all).render().expect("a renderer"));
    let quarter = arc_pixels(&coming_at(all / 4, all).render().expect("a renderer"));
    let most = arc_pixels(&coming_at(all * 3 / 4, all).render().expect("a renderer"));

    assert!(
        quarter > 0,
        "no arc was drawn at all for a picture a quarter of the way down"
    );
    assert!(
        most > quarter * 2,
        "three quarters of the way down drew {most} arc pixels against \
         {quarter} at a quarter: the arc does not grow with the fraction"
    );
    // **Nought draws no arc.** The dim full circle is still there -- it is
    // not the accent -- so this is the arc alone, and an arc with a floor
    // under it would show a sliver of accent for a fetch that has not
    // started.
    assert!(
        nothing < quarter / 4,
        "a fetch at nought per cent drew {nothing} arc pixels against \
         {quarter} at a quarter: the arc has a floor under it"
    );
}

/// The words beside the ring say both numbers, so anything that reads
/// rather than looks gets the same fact.
#[test]
fn a_coming_picture_says_how_much_of_it_is_here() {
    let words = said(&coming_at(1024 * 1024, 4 * 1024 * 1024));
    assert!(
        words.contains("1.0 MiB of 4.0 MiB"),
        "the caption should say how much of how much: {words}"
    );
    // Not the old sentence, which said nothing about either number and is
    // what this replaced.
    assert!(
        !words.contains("fetching the full image"),
        "a picture with a measure on it still says the measureless thing: {words}"
    );
}

/// A voice note coming down says how much of it is here, in the clock --
/// which is where that row says how long or how large it is otherwise.
///
/// **The length is the wrong number while it is arriving.** Twelve seconds
/// was on screen before anybody pressed anything; what a reader is waiting
/// on after pressing is the bytes. The control beside it is a ring rather
/// than a spinner, so the row has a measure too and not only words.
#[test]
fn a_voice_note_coming_down_counts_bytes_in_the_clock() {
    let mut h = a_note_row_coming(
        sigil_ui::attachment::Voice {
            fetching: true,
            ..Default::default()
        },
        Some((10_240, 40_960)),
    );
    h.run_steps(2);
    let words = said(&h);
    assert!(
        words.contains("10 KiB / 40 KiB"),
        "the clock should count the bytes while they arrive: {words}"
    );
    assert!(
        !words.contains("0:12"),
        "the length is still where the progress should be: {words}"
    );
    // Nothing to play yet, and the ring is not a button.
    assert!(h.query_by_label("Play").is_none());
    assert!(h.query_by_label("Pause").is_none());
    // The arc is drawn, which a spinner in its place would not be: a
    // quarter of the way round draws accent where a whole one draws more.
    let quarter = arc_pixels(&h.render().expect("a renderer"));
    let mut whole = a_note_row_coming(
        sigil_ui::attachment::Voice {
            fetching: true,
            ..Default::default()
        },
        Some((40_960, 40_960)),
    );
    whole.run_steps(2);
    assert!(
        quarter > 0 && arc_pixels(&whole.render().expect("a renderer")) > quarter,
        "the control beside the clock is not a ring that grows: {quarter} \
         pixels of accent at a quarter"
    );
}

/// A phone-wide pane holding the bar for a file going up, and what it said.
fn going(name: &str, done: u64, all: u64) -> (String, f32) {
    const PHONE: f32 = 360.0;
    let words = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let width = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
    let (out, took) = (words.clone(), width.clone());
    let name = name.to_string();
    let mut h = Harness::builder()
        .with_size(egui::vec2(PHONE, 120.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            sigil_ui::going_bar(ui, &name, done, all);
            took.set(ui.min_rect().width());
        });
    h.run();
    h.run();
    // **This file's own `said`, not a second walk.** A plain `Label` reports
    // its words as the node's *value* and leaves its label empty, so a walk
    // that reads only labels comes back with nothing and the test fails
    // saying the bar drew no words — which it had.
    *out.borrow_mut() = said(&h);
    let text = words.borrow().clone();
    (text, width.get())
}

/// **A file going up says how far along it is**, which is the whole reason
/// the bar exists: "Sending clip.mp4…" is the same sentence for a minute
/// over a video on a phone's uplink.
#[test]
fn a_file_going_up_says_how_far_along_it_is() {
    let (said, _) = going("clip.mp4", 3 * 1024 * 1024, 12 * 1024 * 1024);
    assert!(
        said.contains("clip.mp4"),
        "it does not say what is going: {said}"
    );
    assert!(
        said.contains("25%"),
        "it does not say how far along it is: {said}"
    );
    assert!(
        said.contains("12.0 MiB"),
        "it does not say how big the file is, which is what somebody \
         deciding whether to wait is deciding about: {said}"
    );
}

/// **Floored, not rounded.** A file one chunk short of done must not read as
/// finished: over a bar that is about to vanish, that is the one lie it
/// could tell. The ring on an arriving file floors for the same reason.
#[test]
fn a_file_all_but_done_does_not_say_it_is_finished() {
    let (said, _) = going("clip.mp4", 9_999, 10_000);
    assert!(
        said.contains("99%"),
        "a file one chunk short read as finished: {said}"
    );
}

/// **A file name is not a licence to widen the pane.** A ui grows to what is
/// drawn in it, so a bar measured off the words rather than the room would
/// take the strip — and the composer with it — off the side of a phone.
#[test]
fn a_long_name_does_not_widen_the_strip() {
    const PHONE: f32 = 360.0;
    let (_, wide) = going(
        "a recording of the whole afternoon, named at length.mp4",
        1,
        2,
    );
    assert!(wide > 0.0, "it drew nothing, so this proves nothing");
    assert!(
        wide <= PHONE + 1.0,
        "the strip draws {wide} points wide in a {PHONE}-point pane"
    );
}

/// **Nought of nought is drawn, not refused.** A progress bar is the wrong
/// place to panic, and a sender that understated its own size should show a
/// full bar rather than none.
#[test]
fn a_file_of_no_size_draws_rather_than_panicking() {
    let (said, _) = going("empty.bin", 0, 0);
    assert!(said.contains("100%"), "nought of nought drew {said}");
}
