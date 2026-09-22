//! A video in a bubble and in the viewer: the one place it plays.

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
    drawn_at(standing, playing, muted, sigil_ui::video::Place::Viewer)
}

fn drawn_at(
    standing: sigil_ui::Standing,
    playing: bool,
    muted: bool,
    place: sigil_ui::video::Place,
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
                still: None,
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
                place,
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

/// Held, the video is its thumbnail with the length on it. In a bubble a
/// press opens it in the viewer; in the viewer a press is the ask to play.
#[test]
fn a_held_video_says_how_long_it_is_and_opens_or_plays_on_a_press() {
    let (mut h, did) = drawn_at(
        sigil_ui::Standing::Held,
        false,
        false,
        sigil_ui::video::Place::Bubble,
    );
    let words = said(&h);
    assert!(words.contains("[video 449s"), "{words}");
    // No bar until it is playable: nothing to pause, nothing to scrub.
    assert!(h.query_by_label("Pause").is_none());
    assert!(h.query_by_label("Mute").is_none());
    h.get_by_label("[video 449s, 46.1 MiB]").click();
    h.run();
    assert!(
        did.get().open && !did.get().toggle,
        "pressing a video in a bubble should open the viewer: {:?}",
        did.get()
    );

    let (mut h, did) = drawn(sigil_ui::Standing::Held, false, false);
    h.get_by_label("[video 449s, 46.1 MiB]").click();
    h.run();
    // **The frame's press is its own answer**, not the bar's play button:
    // a window plays or pauses on it and a phone leaves the viewer, and one
    // flag for both left no way to say which was pressed.
    assert!(
        did.get().tapped && !did.get().toggle,
        "pressing the video in the viewer should say the frame was pressed: {:?}",
        did.get()
    );
}

/// A bubble does not play a video, whatever the player behind it is
/// doing: with one ready and playing, the bubble is still the thumbnail
/// with the play mark and the length on it, no bar under the pointer, and
/// a press opens the viewer rather than pausing anything.
#[test]
fn a_bubble_is_a_thumbnail_even_while_the_video_plays() {
    let (mut h, did) = drawn_at(
        sigil_ui::Standing::Ready,
        true,
        false,
        sigil_ui::video::Place::Bubble,
    );
    let over = h.get_by_label("[video 449s, 46.1 MiB]").rect().center();
    h.hover_at(over);
    h.run();
    assert!(h.query_by_label("Pause").is_none(), "{}", said(&h));
    assert!(h.query_by_label("Mute").is_none());
    assert!(h.query_by_label("See it full size").is_none());
    assert!(h.query_by_role(egui::accesskit::Role::Slider).is_none());
    h.get_by_label("[video 449s, 46.1 MiB]").click();
    h.run();
    assert!(
        did.get().open && !did.get().toggle,
        "a press on a playing video's bubble should open the viewer: {:?}",
        did.get()
    );
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
    // In the viewer, the enlarge control asks for the whole screen; a press
    // on the picture is the frame's own, which the caller reads as play or
    // pause in a window and as the way out on a phone.
    h.get_by_label("Whole screen").click();
    h.run();
    assert!(did.get().fullscreen && !did.get().open);
    did.set(Default::default());
    h.get_by_label("[video 449s, 46.1 MiB]").click();
    h.run();
    assert!(did.get().tapped && !did.get().open && !did.get().toggle);
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

/// **A bubble draws the clip's own first frame when there is one.**
///
/// The sender's thumbnail is capped at eight kilobytes by SIP-18, which
/// makes it 96 pixels across; a phone draws a clip in a bubble over seven
/// hundred device pixels, and the blur is the first thing anybody sees of
/// a video. Once the blob is here the client decodes a still of its own.
/// The frames that *move* are still the viewer's alone -- a bubble does
/// not play -- which is what the `frame` beside this one is.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn a_bubble_draws_the_clips_own_still_when_there_is_one() {
    // **Held across passes.** A `TextureHandle` frees its texture when it is
    // dropped, so one loaded inside the closure is gone before anything is
    // rendered -- and the test then reads a screen with no still on it for
    // a reason that has nothing to do with the code under test.
    let kept: std::cell::RefCell<Option<egui::TextureHandle>> = std::cell::RefCell::new(None);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(400.0, 400.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            sigil::theme::install(&ctx, sigil::theme::light(), sigil::theme::dark());
            // A still of a colour nothing else on this screen is.
            let mut held = kept.borrow_mut();
            let still = held.get_or_insert_with(|| {
                ctx.load_texture(
                    "still",
                    egui::ColorImage::new([8, 8], vec![egui::Color32::from_rgb(9, 200, 9); 64]),
                    egui::TextureOptions::NEAREST,
                )
            });
            let preview: std::sync::Arc<[u8]> = std::sync::Arc::from(Vec::new());
            let v = sigil_ui::Video {
                frame: None,
                still: Some(still),
                preview: &preview,
                id: "clip",
                standing: sigil_ui::Standing::Held,
                position_ms: 0,
                duration_ms: 2_000,
                playing: false,
                ended: false,
                volume: 1.0,
                trouble: None,
                shape: Some((16, 9)),
                described: "[video 2s, 1.2 MiB]",
                place: sigil_ui::video::Place::Bubble,
            };
            sigil_ui::video(ui, &v, 320.0, 320.0);
        });
    h.run();
    h.run();
    // The still is what is on screen: the pixels say so, since a texture
    // draws nothing to the accessibility tree.
    let image = h.render().expect("a renderer");
    let green = image
        .pixels()
        .filter(|p| p.0[1] > 150 && p.0[0] < 60 && p.0[2] < 60)
        .count();
    assert!(
        green > 1000,
        "the clip's own still is not what the bubble drew: {green} pixels of it"
    );
}
