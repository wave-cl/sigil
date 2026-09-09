//! What a message's file looks like when it will not open.

use egui_kittest::Harness;
use egui_kittest::kittest::NodeT;
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
fn tall(bytes: Option<&'static [u8]>, height: Option<f32>) -> f32 {
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
                            preview: &[],
                            bytes,
                            missing: false,
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

fn drawn(bytes: &'static [u8]) -> Harness<'static> {
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
                    preview: &[],
                    bytes: Some(bytes),
                    missing: false,
                    id: "notapicture",
                },
                sigil::ColorTheme::current(&ctx).surface_elevated,
            );
        })
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
    let mut h = drawn(a_png());
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
    let mut h = drawn(b"this is not a picture");
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
    let mut h = drawn(a_png());
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
    let with_room = tall(Some(a_png()), Some(400.0));
    let with_none = tall(Some(a_png()), None);
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

/// A picture takes the same room before it arrives as after.
///
/// It used to take whatever each stage needed: one line of words while the
/// blob was fetched, another while it decoded, then a few hundred pixels when
/// it appeared. So every picture changed the height of everything below it two
/// or three times as it loaded, and scrolling through a channel with pictures
/// in it moved the text under the reader's eyes -- the transcript's content
/// height was measured wandering by forty to two hundred pixels at a time
/// while nobody had touched anything.
///
/// The three states are the three this can be in: nothing fetched, bytes that
/// will not decode, and a picture. They have to agree to the pixel.
#[test]
fn every_stage_of_a_picture_takes_the_same_room() {
    let fetching = tall(None, Some(400.0));
    let broken = tall(Some(b"this is not a picture"), Some(400.0));
    let drawn = tall(Some(a_png()), Some(400.0));

    assert!(fetching > 0.0, "a picture on its way takes no room at all");
    assert_eq!(
        fetching, drawn,
        "the transcript moves when a picture arrives: {fetching} before, {drawn} after"
    );
    assert_eq!(
        broken, drawn,
        "the transcript moves when a picture turns out to be unopenable: \
         {broken} against {drawn}"
    );
}
