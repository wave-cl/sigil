//! A mark for a key, so somebody is recognisable before they are read.
//!
//! # Why it agrees with the terminal client
//!
//! `sqex-chat` draws identicons too, and the same person appearing as a
//! different colour in the two clients would make the mark worse than none —
//! the whole value of one is that it is stable. So the hash and the two hues
//! here are **deliberately the same arithmetic** as `sqex_chat::ui::identicon`:
//! FNV-1a over the identifier's bytes, two hues taken independently and pushed
//! apart when they land too close.
//!
//! Independently, because deriving the second from the first makes the pair a
//! function of one byte, and two accounts whose first hue is near each other
//! then get near-identical marks — which is the thing these exist to prevent.
//!
//! What is different is everything below that. A terminal has two cells to
//! work with; a window has as many pixels as it likes, so this draws the
//! familiar symmetric grid instead.
//!
//! # It is not identity
//!
//! An identicon is a **hint for the eye, not a check**. Two keys can collide
//! here and nothing stops somebody choosing a key that looks like another's.
//! It sits *beside* the key, never instead of it (SIP-21).

use sigil::tokens;

/// How many cells across. Odd, so there is a centre column to mirror about.
const GRID: usize = 5;

/// FNV-1a. The same constants as the terminal client's, so the same identifier
/// produces the same mark in both.
fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A byte to a colour, going round the wheel at a fixed saturation so no key
/// lands on something unreadably dark or washed out.
fn hue(byte: u8) -> egui::Color32 {
    let sector = byte as u32 * 6 / 256;
    let within = (byte as u32 * 6 % 256) * 255 / 256;
    let (lo, hi) = (70u8, 220u8);
    let up = (lo as u32 + within * (hi - lo) as u32 / 255) as u8;
    let down = (hi as u32 - within * (hi - lo) as u32 / 255) as u8;
    let (r, g, b) = match sector {
        0 => (hi, up, lo),
        1 => (down, hi, lo),
        2 => (lo, hi, up),
        3 => (lo, down, hi),
        4 => (up, lo, hi),
        _ => (hi, lo, down),
    };
    egui::Color32::from_rgb(r, g, b)
}

/// The two colours a given identifier gets.
pub fn colours(id: &[u8]) -> (egui::Color32, egui::Color32) {
    let h = fnv(id);
    let first = (h & 0xFF) as u8;
    let mut second = ((h >> 8) & 0xFF) as u8;
    // Distance round the wheel, which is a circle: 250 and 5 are ten apart.
    let apart = |x: u8, y: u8| x.wrapping_sub(y).min(y.wrapping_sub(x));
    if apart(first, second) < 60 {
        second = second.wrapping_add(85);
    }
    (hue(first), hue(second))
}

/// Draw the mark for `id` at `size` square.
///
/// `id` is whatever identifies the thing — an account key, a channel
/// identifier. A channel has an identifier as good as a key has and no reason
/// to go without a mark of its own.
pub fn identicon_of(ui: &mut egui::Ui, id: &[u8], size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let (back, fore) = colours(id);
    let h = fnv(id);
    let painter = ui.painter();
    let centre = rect.center();
    let radius = size / 2.0;
    painter.circle_filled(centre, radius, back);

    // Mirrored about the centre column, which is what makes these read as a
    // face or a glyph rather than as noise. Only the left half plus the middle
    // is drawn from the hash; the rest is its reflection.
    let cell = size / GRID as f32;
    let half = GRID.div_ceil(2);
    for col in 0..half {
        for row in 0..GRID {
            let bit = col * GRID + row;
            if (h >> (bit % 64)) & 1 == 0 {
                continue;
            }
            for c in [col, GRID - 1 - col] {
                let at = egui::Rect::from_min_size(
                    rect.min + egui::vec2(c as f32 * cell, row as f32 * cell),
                    egui::vec2(cell, cell),
                );
                // **The circle, clipped to the cell** -- not the cell, which
                // would spill past the outline at the corners.
                //
                // The union of (circle ∩ cell) over the cells that are set is
                // exactly (circle ∩ pattern), so the mark keeps a true
                // circular edge using nothing but rectangular clipping, which
                // is all egui offers. Drawing the cells and masking afterwards
                // would need the colour of whatever is behind, and these sit
                // on four different surfaces.
                painter
                    .with_clip_rect(at.intersect(rect))
                    .circle_filled(centre, radius, fore);
            }
        }
    }
    response
}

/// The mark for an account or channel named by a base58 string.
pub fn identicon(ui: &mut egui::Ui, key: &str, size: f32) -> egui::Response {
    identicon_of(ui, key.as_bytes(), size)
}

/// An avatar: a picture if there is one, and the identicon if there is not.
///
/// Everybody has a mark from the moment they exist, which is what stops a
/// conversation list being a column of identical grey circles until people get
/// round to setting a picture.
pub fn avatar(
    ui: &mut egui::Ui,
    key: &str,
    picture: Option<&egui::TextureHandle>,
    size: f32,
) -> egui::Response {
    match picture {
        Some(texture) => {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            if ui.is_rect_visible(rect) {
                // The same shape as the mark it stands in for, or a picture
                // and a generated mark would be two different things in one
                // column.
                egui::Image::new(texture)
                    .corner_radius(size / 2.0)
                    .paint_at(ui, rect);
            }
            response
        }
        None => identicon(ui, key, size),
    }
    // The key stays reachable from the mark, wherever it is drawn. A picture
    // is something somebody chose; a key is not.
    .on_hover_text(key.to_string())
}

/// A ring drawn round an avatar, for presence or speaking.
pub fn ring(ui: &egui::Ui, around: egui::Rect, colour: egui::Color32) {
    ui.painter().circle_stroke(
        around.center(),
        around.width() / 2.0 + tokens::STROKE_THICK,
        egui::Stroke::new(tokens::STROKE_THICK, colour),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_hues_are_never_confusable() {
        // Every mark carries two colours and the point of the pair is that it
        // says more than one would. A pair that landed on the same shade would
        // be a mark with half the information it claims.
        let apart = |a: egui::Color32, b: egui::Color32| {
            let d = |x: u8, y: u8| (x as i32 - y as i32).abs();
            d(a.r(), b.r()) + d(a.g(), b.g()) + d(a.b(), b.b())
        };
        for n in 0u16..2000 {
            let id = format!("account-{n}");
            let (a, b) = colours(id.as_bytes());
            assert!(
                apart(a, b) > 60,
                "{id} got two colours too close to tell apart: {a:?} vs {b:?}"
            );
        }
    }

    /// Everything the mark paints in its own colours is a circle.
    ///
    /// # Why this is not left to the snapshot
    ///
    /// "It is round" is exactly the sort of thing a picture shows and nobody
    /// checks: the snapshots are looked at when something else changed, and a
    /// mark quietly back to a rounded square would pass a threshold that
    /// forgives a few hundred pixels. This reads the shapes egui was actually
    /// given, so a `rect_filled` in the identicon's own colours fails here and
    /// names itself.
    #[test]
    fn the_mark_is_drawn_as_circles_and_never_as_squares() {
        let id = b"a-key";
        let (back, fore) = colours(id);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            identicon_of(ui, id, 40.0);
        });
        // epaint panics on a dropped `TexturesDelta`, which is its way of
        // saying a real backend must upload the font atlas. There is no
        // backend here and nothing to upload it to.
        output.textures_delta.clear();

        let mut circles = 0;
        let mut rects = 0;
        for clipped in &output.shapes {
            match &clipped.shape {
                egui::Shape::Circle(c) if c.fill == back || c.fill == fore => circles += 1,
                egui::Shape::Rect(r) if r.fill == back || r.fill == fore => rects += 1,
                _ => {}
            }
        }
        assert_eq!(rects, 0, "the mark paints {rects} rectangles of its own");
        // The ground and at least one cell of the pattern. A hash that set no
        // bits at all would make this vacuous, so the count is checked rather
        // than merely being non-zero.
        assert!(
            circles > 1,
            "the mark painted {circles} circles, so there is no pattern in it"
        );
    }

    #[test]
    fn the_same_identifier_always_gets_the_same_mark() {
        let (a1, b1) = colours(b"a-key");
        let (a2, b2) = colours(b"a-key");
        assert_eq!((a1, b1), (a2, b2));
        assert_ne!(colours(b"a-key"), colours(b"another-key"));
    }

    #[test]
    fn keys_differing_in_one_byte_do_not_look_alike() {
        // Two accounts whose keys share a prefix are exactly the pair somebody
        // is most likely to confuse, so the mark has to move a lot when the
        // key moves a little.
        let (a, _) = colours(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
        let (b, _) = colours(b"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB");
        assert_ne!(a, b, "a one-byte difference must change the mark");
    }
}
