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
