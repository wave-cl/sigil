//! A video in a bubble: the picture that is due, and the controls.
//!
//! Knows nothing of decoding. The caller hands over the current frame as a
//! texture and the clock; this draws them and hands back what the reader
//! did. Until there is a frame, the sender's thumbnail stands in, with a
//! play mark over it and the length in the corner, which is what a video
//! looks like in every messenger.

use crate::attachment::{PICTURE, PICTURE_MAX_TALL};
use sigil::{ColorTheme, Icon, tokens};

/// Where a video is, as far as the interface knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Not fetched; pressing play asks for it.
    Held,
    /// Asked for, on its way.
    Fetching,
    /// Playable: `Video::frame` is live.
    Ready,
}

/// One video, as the caller sees it.
pub struct Video<'a> {
    /// The picture due now, once there is a player. `None` shows the
    /// thumbnail.
    pub frame: Option<&'a egui::TextureHandle>,
    /// The sender's thumbnail, empty if none.
    pub preview: &'a std::sync::Arc<[u8]>,
    /// A stable name, for the thumbnail's texture.
    pub id: &'a str,
    pub standing: Standing,
    pub position_ms: u64,
    /// Zero when unknown.
    pub duration_ms: u64,
    pub playing: bool,
    pub ended: bool,
    /// Volume, 0 to 1; 0 is muted.
    pub volume: f32,
    /// Why it will not play, if it will not.
    pub trouble: Option<&'a str>,
    /// Width and height, when known from the file; else from the
    /// thumbnail, else sixteen by nine.
    pub shape: Option<(u32, u32)>,
    /// What it is, in words, for the accessibility tree.
    pub described: &'a str,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoAction {
    /// Play or pause -- or, held, fetch.
    pub toggle: bool,
    /// Go to this time.
    pub seek: Option<u64>,
    /// Silence it, or let it be heard again.
    pub mute: Option<bool>,
    /// See it full size.
    pub open: bool,
    pub save: bool,
}

/// `4:32`, or `1:04:32`.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// How large the video is drawn for a box `wide` across: its own shape,
/// bounded by the width and by how tall anything in a transcript may be.
pub fn size_for(shape: Option<(u32, u32)>, wide: f32, tall_max: f32) -> egui::Vec2 {
    let (w, h) = match shape {
        Some((w, h)) if w > 0 && h > 0 => (w as f32, h as f32),
        _ => (16.0, 9.0),
    };
    let scale = (wide / w).min(tall_max / h);
    egui::vec2(w * scale, h * scale)
}

/// Draw one, `wide` across -- [`PICTURE`] in a bubble, the window in the
/// viewer -- and say what was done to it.
pub fn video(ui: &mut egui::Ui, v: &Video<'_>, wide: f32, tall_max: f32) -> VideoAction {
    let theme = ColorTheme::current(ui.ctx());
    let mut action = VideoAction::default();
    let size = size_for(v.shape, wide, tall_max);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Image, true, v.described));
    ui.painter()
        .rect_filled(rect, tokens::RADIUS_MD, egui::Color32::BLACK);

    // The picture: the frame, or the thumbnail until there is one.
    let mut drew = false;
    if let Some(texture) = v.frame {
        let drawn = fit(texture.size_vec2(), rect.size());
        egui::Image::from_texture(egui::load::SizedTexture::from_handle(texture))
            .corner_radius(tokens::RADIUS_MD)
            .paint_at(ui, egui::Rect::from_center_size(rect.center(), drawn));
        drew = true;
    } else if !v.preview.is_empty() {
        let uri = format!("bytes://{}-preview", v.id);
        ui.ctx()
            .include_bytes(uri.clone(), egui::load::Bytes::Shared(v.preview.clone()));
        let image = egui::Image::from_bytes(uri, egui::load::Bytes::Shared(v.preview.clone()))
            .corner_radius(tokens::RADIUS_MD)
            .show_loading_spinner(false);
        if let Ok(egui::load::TexturePoll::Ready { texture }) =
            image.load_for_size(ui.ctx(), rect.size())
        {
            let drawn = fit(texture.size, rect.size());
            image.paint_at(ui, egui::Rect::from_center_size(rect.center(), drawn));
            drew = true;
        }
    }
    if !drew {
        ui.painter()
            .rect_filled(rect, tokens::RADIUS_MD, theme.surface_secondary);
    }

    // Over the picture **including its own controls**: `hovered()` is false
    // while the pointer is on a button drawn on top, and a bar that
    // vanishes the moment the pointer reaches it cannot be pressed.
    let hovered = ui.rect_contains_pointer(rect);
    let idle = !v.playing;
    // **The play mark, big, in the middle**, whenever nothing is playing:
    // the one control everybody looks for first. Dimmed ground behind it so
    // it reads on a bright frame.
    if idle && v.trouble.is_none() {
        let r = (rect.width().min(rect.height()) * 0.18).max(18.0);
        let c = rect.center();
        ui.painter()
            .circle_filled(c, r, egui::Color32::from_black_alpha(140));
        let icon = egui::Rect::from_center_size(c, egui::vec2(r * 1.1, r * 1.1));
        sigil::icon::draw(ui.painter(), icon, Icon::Play, egui::Color32::WHITE);
        if v.standing == Standing::Fetching {
            ui.painter().text(
                c + egui::vec2(0.0, r + tokens::SPACING_MD),
                egui::Align2::CENTER_TOP,
                "fetching…",
                egui::TextStyle::Small.resolve(ui.style()),
                egui::Color32::WHITE,
            );
        }
    }
    if let Some(why) = v.trouble {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            why,
            egui::TextStyle::Small.resolve(ui.style()),
            theme.warning,
        );
    }

    // The length, bottom right, while it is a thumbnail. A video that says
    // how long it is before you press it is one you can decide about.
    if v.standing != Standing::Ready && v.duration_ms > 0 {
        badge(
            ui,
            rect.right_bottom() - egui::vec2(tokens::SPACING_SM, tokens::SPACING_SM),
            &clock(v.duration_ms),
        );
    }

    // Pressing the picture itself: play, pause, or ask for it.
    if response.clicked() {
        action.toggle = true;
    }

    // **The bar, while the pointer is over it or nothing is playing.** A
    // control that is always on top of the picture is a control that is
    // always in the way of it. Painted rather than laid out, so it takes
    // no room in the bubble and the video is the same size with it or
    // without.
    if v.standing == Standing::Ready && (hovered || idle || v.ended) {
        let bar = egui::Rect::from_min_max(
            egui::pos2(
                rect.left(),
                rect.bottom() - tokens::BUTTON_SM - tokens::SPACING_SM,
            ),
            rect.right_bottom(),
        );
        ui.painter().rect_filled(
            bar,
            egui::CornerRadius {
                nw: 0,
                ne: 0,
                sw: tokens::RADIUS_MD as u8,
                se: tokens::RADIUS_MD as u8,
            },
            egui::Color32::from_black_alpha(150),
        );
        let mut inner = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("video-bar", v.id))
                .max_rect(bar.shrink2(egui::vec2(tokens::SPACING_SM, tokens::SPACING_XS)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        inner.spacing_mut().item_spacing.x = tokens::SPACING_XS;
        let white = |ui: &mut egui::Ui| {
            ui.visuals_mut().override_text_color = Some(egui::Color32::WHITE);
            ui.visuals_mut().widgets.inactive.fg_stroke.color = egui::Color32::WHITE;
            ui.visuals_mut().widgets.hovered.fg_stroke.color = egui::Color32::WHITE;
            ui.visuals_mut().widgets.active.fg_stroke.color = egui::Color32::WHITE;
        };
        white(&mut inner);
        let (icon, word) = if v.playing {
            (Icon::Pause, "Pause")
        } else {
            (Icon::Play, "Play")
        };
        if sigil::icon_button_named(&mut inner, icon, word).clicked() {
            action.toggle = true;
        }
        inner.label(
            egui::RichText::new(format!(
                "{} / {}",
                clock(v.position_ms),
                clock(v.duration_ms)
            ))
            .small()
            .color(egui::Color32::WHITE),
        );
        // Right-hand controls first, so the scrubber takes what is left.
        inner.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            white(ui);
            if sigil::icon_button_named(ui, Icon::Enlarge, "See it full size").clicked() {
                action.open = true;
            }
            let (icon, word) = if v.volume > 0.0 {
                (Icon::Sound, "Mute")
            } else {
                (Icon::Muted, "Unmute")
            };
            if sigil::icon_button_named(ui, icon, word).clicked() {
                action.mute = Some(v.volume > 0.0);
            }
            // The scrubber: a line, the part played in the accent, and a
            // press or drag anywhere along it goes there.
            let (track, held) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().max(24.0), tokens::BUTTON_SM),
                egui::Sense::click_and_drag(),
            );
            held.widget_info(|| {
                egui::WidgetInfo::slider(
                    true,
                    v.position_ms as f64 / 1000.0,
                    format!("{} of {}", clock(v.position_ms), clock(v.duration_ms)),
                )
            });
            let line = egui::Rect::from_center_size(track.center(), egui::vec2(track.width(), 4.0));
            ui.painter()
                .rect_filled(line, 2.0, egui::Color32::from_white_alpha(90));
            let played = if v.duration_ms > 0 {
                (v.position_ms as f32 / v.duration_ms as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let done = egui::Rect::from_min_size(
                line.min,
                egui::vec2(line.width() * played, line.height()),
            );
            ui.painter().rect_filled(done, 2.0, theme.accent);
            ui.painter()
                .circle_filled(done.right_center(), 5.0, egui::Color32::WHITE);
            if (held.clicked() || held.dragged())
                && let Some(p) = held.interact_pointer_pos()
                && v.duration_ms > 0
            {
                let t = ((p.x - line.left()) / line.width()).clamp(0.0, 1.0);
                action.seek = Some((t * v.duration_ms as f32) as u64);
            }
        });
    }
    action
}

/// A word in a dark pill, for over a picture.
fn badge(ui: &mut egui::Ui, right_bottom: egui::Pos2, text: &str) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE);
    let pad = egui::vec2(tokens::SPACING_XS, 2.0);
    let rect = egui::Rect::from_min_max(right_bottom - galley.size() - pad * 2.0, right_bottom);
    ui.painter().rect_filled(
        rect,
        tokens::RADIUS_SM,
        egui::Color32::from_black_alpha(160),
    );
    ui.painter()
        .galley(rect.min + pad, galley, egui::Color32::WHITE);
}

/// `natural` scaled to fit inside `room`, keeping its shape.
fn fit(natural: egui::Vec2, room: egui::Vec2) -> egui::Vec2 {
    if natural.x <= 0.0 || natural.y <= 0.0 {
        return room;
    }
    let scale = (room.x / natural.x).min(room.y / natural.y);
    natural * scale
}

/// The room a video takes in a bubble, before anybody knows its shape.
pub fn bubble_size(shape: Option<(u32, u32)>) -> egui::Vec2 {
    size_for(shape, PICTURE, PICTURE_MAX_TALL)
}
