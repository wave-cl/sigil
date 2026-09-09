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
