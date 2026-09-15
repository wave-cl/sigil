//! Emoji as pictures: that a picture is what gets drawn when there is a
//! loader to draw it, and text when there is not.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use sigil::theme;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether `paint` drew a picture, with or without the svg loader.
fn painted(with_loaders: bool) -> bool {
    let drew = Arc::new(AtomicBool::new(false));
    let seen = drew.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(200.0, 100.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            if with_loaders {
                sigil_ui::install_loaders(&ctx);
            }
            let rect = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(20.0, 20.0));
            let did = sigil_ui::emoji::paint(ui, "\u{1f602}", rect, egui::Color32::WHITE);
            if did {
                seen.store(true, Ordering::Relaxed);
            }
        });
    // Several passes: the svg is rasterised on the pass that asks for it and
    // ready on a later one.
    for _ in 0..4 {
        h.run();
    }
    drew.load(Ordering::Relaxed)
}

/// With the loaders installed the picture is drawn; the whole point.
#[test]
fn a_picture_is_drawn_when_there_is_a_loader() {
    assert!(
        painted(true),
        "no picture was ever drawn through the svg loader"
    );
}

/// Without them — every kittest harness that does not install them — it
/// falls back to text and says so. The negative control for the test above:
/// a `paint` that returned true for any reason would pass both.
#[test]
fn without_a_loader_it_falls_back_to_text() {
    assert!(
        !painted(false),
        "a picture was reported drawn with no loader to draw it"
    );
}

/// A string the table does not know is drawn as text and said as itself.
#[test]
fn an_unknown_string_is_still_a_button_saying_itself() {
    let h = Harness::builder()
        .with_size(egui::vec2(200.0, 100.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            sigil_ui::install_loaders(&ctx);
            sigil_ui::emoji::glyph(ui, "xyz", sigil_ui::emoji::CELL);
            sigil_ui::emoji::chip(ui, "\u{1f389}", 3, true);
        });
    h.get_by_label("xyz");
    h.get_by_label("\u{1f389} 3");
}

/// Where the strip goes: off the top corner, and off the bottom one when
/// the top is at the top of the transcript.
///
/// The first message on screen has its top edge at the header, so a strip
/// off its top corner was half under the header: painted clipped, and —
/// since a widget outside its clip cannot be pressed — unreachable. Found
/// by a test that replied to the top message and got no reply.
fn strip_at(clip_top: f32, mine: bool) -> (egui::Rect, egui::Rect, bool) {
    let pressed = Arc::new(AtomicBool::new(false));
    let seen = pressed.clone();
    // Room on the side the strip reaches into: theirs to the right, one's
    // own to the left. (Without it the strip is clamped back over the
    // bubble, which is right on a narrow pane and not what this measures.)
    let left = if mine { 240.0 } else { 20.0 };
    let bubble = egui::Rect::from_min_size(egui::pos2(left, 100.0), egui::vec2(160.0, 44.0));
    let mut h = Harness::builder()
        .with_size(egui::vec2(420.0, 300.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            let clip =
                egui::Rect::from_min_max(egui::pos2(0.0, clip_top), egui::pos2(420.0, 300.0));
            let mut action = sigil_ui::BubbleAction::default();
            let seen = seen.clone();
            sigil_ui::emoji::strip(
                ui,
                sigil_ui::emoji::Strip {
                    id: egui::Id::new("strip-at"),
                    bubble,
                    mine,
                    clip,
                    frequent: &[],
                },
                &mut action,
                move |ui, _| {
                    if sigil_ui::emoji::cell_icon(ui, sigil_ui::Icon::Reply, "Reply").clicked() {
                        seen.store(true, Ordering::Relaxed);
                    }
                },
            );
        });
    h.run();
    h.run();
    let reply = h.get_by_label("Reply").rect();
    h.get_by_label("Reply").click();
    h.run();
    (bubble, reply, pressed.load(Ordering::Relaxed))
}

#[test]
fn the_strip_hangs_off_the_top_corner_and_can_be_pressed() {
    let (bubble, reply, pressed) = strip_at(0.0, false);
    assert!(
        reply.top() < bubble.top() && reply.bottom() > bubble.top(),
        "{reply:?} against {bubble:?}"
    );
    assert!(
        reply.left() > bubble.right(),
        "theirs: off the right corner, {reply:?} against {bubble:?}"
    );
    assert!(pressed, "the strip could not be pressed");
    let (bubble, reply, _) = strip_at(0.0, true);
    assert!(
        reply.right() < bubble.left(),
        "mine: off the left corner, {reply:?} against {bubble:?}"
    );
}

#[test]
fn at_the_top_of_the_transcript_it_hangs_off_the_bottom_corner_instead() {
    // The clip begins a little below the bubble's top: the top strip would
    // be under whatever is above the transcript.
    let (bubble, reply, pressed) = strip_at(bubble_top() + 4.0, false);
    assert!(
        reply.top() < bubble.bottom() && reply.bottom() > bubble.bottom(),
        "{reply:?} against {bubble:?}"
    );
    assert!(pressed, "the strip could not be pressed");
}

fn bubble_top() -> f32 {
    100.0
}

/// The strip and the open picker, drawn with pictures.
///
/// The one place the colour is looked at: everything else asserts labels,
/// which are the same whether the picture or its fallback was painted.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn emoji_picker_dark() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(420.0, 520.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            sigil_ui::install_loaders(&ctx);
            let theme = sigil::ColorTheme::current(&ctx);
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, theme.surface_primary);
            let bubble = egui::Rect::from_min_size(egui::pos2(40.0, 60.0), egui::vec2(160.0, 44.0));
            ui.painter()
                .rect_filled(bubble, sigil::tokens::RADIUS_PILL, theme.surface_elevated);
            let first = ctx.cumulative_pass_nr() == 0;
            let mut action = sigil_ui::BubbleAction::default();
            let frequent = ["\u{1f389}".to_string(), "\u{1f44d}".to_string()];
            sigil_ui::emoji::strip(
                ui,
                sigil_ui::emoji::Strip {
                    id: egui::Id::new("snapshot-strip"),
                    bubble,
                    mine: false,
                    clip: ui.max_rect(),
                    frequent: &frequent,
                },
                &mut action,
                |ui, _| {
                    sigil_ui::emoji::cell_icon(ui, sigil_ui::Icon::Reply, "Reply");
                    sigil_ui::emoji::cell_icon(ui, sigil_ui::Icon::More, "More");
                },
            );
            if first {
                egui::Popup::open_id(&ctx, egui::Id::new("snapshot-strip").with("picker"));
            }
            ui.horizontal(|ui| {
                sigil_ui::emoji::chip(ui, "\u{1f389}", 2, true);
                sigil_ui::emoji::chip(ui, "\u{2764}\u{fe0f}", 1, false);
            });
        });
    for _ in 0..6 {
        h.run();
    }
    h.snapshot("emoji_picker_dark");
}
