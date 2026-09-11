//! A video in a bubble, before and while it plays.

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

/// A harness drawing one video in the given state, recording what was
/// done to it.
fn drawn(
    standing: sigil_ui::Standing,
    playing: bool,
    muted: bool,
) -> (
    Harness<'static>,
    std::rc::Rc<std::cell::Cell<sigil_ui::VideoAction>>,
) {
    let did = std::rc::Rc::new(std::cell::Cell::new(sigil_ui::VideoAction::default()));
    let seen = did.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(500.0, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            let empty = sigil_ui::attachment::no_preview();
            let v = sigil_ui::Video {
                frame: None,
                preview: empty,
                id: "clip",
                standing,
                position_ms: 65_000,
                duration_ms: 449_000,
                playing,
                ended: false,
                volume: if muted { 0.0 } else { 1.0 },
                trouble: None,
                shape: Some((1280, 720)),
                described: "[video 449s, 46.1 MiB]",
            };
            let action = sigil_ui::video(ui, &v, 320.0, 320.0);
            if action != sigil_ui::VideoAction::default() {
                seen.set(action);
            }
        });
    h.run();
    (h, did)
}

/// Its own shape: sixteen by nine at the bubble's width.
#[test]
fn a_video_takes_the_shape_of_the_file() {
    let size = sigil_ui::video::size_for(Some((1280, 720)), 320.0, 320.0);
    assert_eq!(size, egui::vec2(320.0, 180.0));
    let tall = sigil_ui::video::size_for(Some((720, 1280)), 320.0, 320.0);
    assert_eq!(
        tall,
        egui::vec2(180.0, 320.0),
        "a portrait one is bounded by the height"
    );
    assert_eq!(
        sigil_ui::video::size_for(None, 320.0, 320.0),
        egui::vec2(320.0, 180.0),
        "unknown is sixteen by nine"
    );
}

#[test]
fn times_are_said_as_a_clock() {
    use sigil_ui::video::clock;
    assert_eq!(clock(0), "0:00");
    assert_eq!(clock(65_000), "1:05");
    assert_eq!(clock(449_344), "7:29");
    assert_eq!(clock(3_725_000), "1:02:05");
}

/// Held, the video is its thumbnail with the length on it, and pressing it
/// is the ask to fetch and play.
#[test]
fn a_held_video_says_how_long_it_is_and_plays_on_a_press() {
    let (mut h, did) = drawn(sigil_ui::Standing::Held, false, false);
    let words = said(&h);
    assert!(words.contains("[video 449s"), "{words}");
    // No bar until it is playable: nothing to pause, nothing to scrub.
    assert!(h.query_by_label("Pause").is_none());
    assert!(h.query_by_label("Mute").is_none());
    h.get_by_label("[video 449s, 46.1 MiB]").click();
    h.run();
    assert!(did.get().toggle, "pressing the video should ask to play it");
}

/// Playing, the bar carries pause, the clock, mute, full size and a
/// scrubber that seeks.
#[test]
fn a_playing_video_can_be_paused_muted_enlarged_and_scrubbed() {
    let (mut h, did) = drawn(sigil_ui::Standing::Ready, true, false);
    // The bar is there while the pointer is over the picture; a playing
    // video with the pointer elsewhere is just the picture.
    assert!(
        !said(&h).contains("1:05 / 7:29"),
        "the bar is over a playing video nobody is pointing at"
    );
    let over = h.get_by_label("[video 449s, 46.1 MiB]").rect().center();
    h.hover_at(over);
    h.run();
    let words = said(&h);
    assert!(words.contains("1:05 / 7:29"), "{words}");
    h.get_by_label("Pause").click();
    h.run();
    assert!(did.get().toggle);
    did.set(Default::default());
    h.get_by_label("Mute").click();
    h.run();
    assert_eq!(did.get().mute, Some(true));
    did.set(Default::default());
    h.get_by_label("See it full size").click();
    h.run();
    assert!(did.get().open);
    did.set(Default::default());
    // The scrubber: a press at three quarters along goes three quarters in.
    let slider = h.get_by_role(egui::accesskit::Role::Slider);
    let rect = slider.rect();
    let at = egui::pos2(rect.left() + rect.width() * 0.75, rect.center().y);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.step();
    let sought = did.get().seek.expect("no seek");
    let expected = (449_000.0 * 0.75) as u64;
    assert!(
        sought.abs_diff(expected) < 449_000 / 20,
        "pressed three quarters along and sought to {sought} ms of 449000"
    );

    // Muted, the control offers the other word.
    let (mut h, _) = drawn(sigil_ui::Standing::Ready, true, true);
    let over = h.get_by_label("[video 449s, 46.1 MiB]").rect().center();
    h.hover_at(over);
    h.run();
    assert!(h.query_by_label("Unmute").is_some());
    assert!(h.query_by_label("Mute").is_none());
}
