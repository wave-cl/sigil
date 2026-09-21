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
            if !lit(h, col, row) {
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

/// The key the **product's own** mark is drawn from: thirty-two 0x01 bytes,
/// spelt as the app spells a key.
///
/// A mark like every other in the app and nobody's in particular. It is what
/// `scripts/launcher-icon` puts on an Android home screen, so it is what a
/// phone's first screen shows too -- the thing somebody just tapped. Named
/// here because two copies of a constant that has to match a committed
/// picture is one copy too many.
pub const MARK_KEY: &str = "4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi";

/// Whether the cell at `col`, `row` of the mark for a hash is lit: the
/// left half from the hash's bits, the right half a mirror of it.
fn lit(h: u64, col: usize, row: usize) -> bool {
    let col = col.min(GRID - 1 - col);
    let bit = col * GRID + row;
    (h >> (bit % 64)) & 1 == 1
}

/// The mark as an Android launcher layer: `size` square, RGBA, with the
/// mark's circle two thirds of the side across, centred, and the mark's
/// background colour everywhere else. A launcher masks an adaptive icon
/// to the middle two thirds of its layers -- a circle, on most phones --
/// so a mark drawn this way fills the whole of the icon's circle, and
/// a launcher that cuts a squircle instead shows its colour, not a hole.
pub fn identicon_layer(id: &[u8], size: usize) -> Vec<u8> {
    let (back, _) = colours(id);
    let inner = size * 2 / 3;
    let mark = identicon_raster(id, inner);
    let offset = (size - inner) / 2;
    let mut out = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let (mx, my) = (x as isize - offset as isize, y as isize - offset as isize);
            let px = if mx >= 0 && my >= 0 && (mx as usize) < inner && (my as usize) < inner {
                let i = (my as usize * inner + mx as usize) * 4;
                let p = &mark[i..i + 4];
                // Over the background colour, so the circle's soft edge
                // blends into it rather than into nothing.
                let a = p[3] as u32;
                [
                    ((p[0] as u32 * a + back.r() as u32 * (255 - a)) / 255) as u8,
                    ((p[1] as u32 * a + back.g() as u32 * (255 - a)) / 255) as u8,
                    ((p[2] as u32 * a + back.b() as u32 * (255 - a)) / 255) as u8,
                    255,
                ]
            } else {
                [back.r(), back.g(), back.b(), 255]
            };
            out.extend_from_slice(&px);
        }
    }
    out
}

/// The mark as pixels: `size` square, RGBA, row by row, with the corners
/// outside the circle clear. The same rule as [`identicon_of`] draws by,
/// for where there is no painter -- a phone's launcher icon, which is
/// nobody's mark until an identity is made.
pub fn identicon_raster(id: &[u8], size: usize) -> Vec<u8> {
    const SUB: usize = 4;
    let (back, fore) = colours(id);
    let h = fnv(id);
    let radius = size as f32 / 2.0;
    let cell = size as f32 / GRID as f32;
    let mut out = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            // Supersampled: how much of the pixel is inside the circle,
            // and of that how much is in a lit cell.
            let mut inside = 0u32;
            let mut on = 0u32;
            for sy in 0..SUB {
                for sx in 0..SUB {
                    let px = x as f32 + (sx as f32 + 0.5) / SUB as f32;
                    let py = y as f32 + (sy as f32 + 0.5) / SUB as f32;
                    let (dx, dy) = (px - radius, py - radius);
                    if dx * dx + dy * dy > radius * radius {
                        continue;
                    }
                    inside += 1;
                    let (col, row) = ((px / cell) as usize, (py / cell) as usize);
                    if lit(h, col.min(GRID - 1), row.min(GRID - 1)) {
                        on += 1;
                    }
                }
            }
            let total = (SUB * SUB) as u32;
            if inside == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let mix = |a: u8, b: u8| ((a as u32 * (inside - on) + b as u32 * on) / inside) as u8;
            let alpha = (255 * inside / total) as u8;
            out.extend_from_slice(&[
                mix(back.r(), fore.r()),
                mix(back.g(), fore.g()),
                mix(back.b(), fore.b()),
                alpha,
            ]);
        }
    }
    out
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

/// Whether somebody is there, in three words. Drawn as a dot on the
/// corner of their mark by [`presence`], and, standing alone, by
/// [`Presence::dot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Presence {
    /// Connected, and at the keyboard.
    Active,
    /// Connected, and nobody there.
    Away,
    /// Not connected, or not for long enough to say.
    #[default]
    Offline,
}

impl Presence {
    /// The word, the one every consumer says.
    pub fn word(self) -> &'static str {
        match self {
            Presence::Active => "active",
            Presence::Away => "away",
            Presence::Offline => "offline",
        }
    }

    /// Filled, and in what colour; hollow for offline. **Filled versus
    /// hollow, not one colour versus another**: the state survives
    /// somebody who cannot tell green from amber.
    pub fn look(self, theme: &sigil::ColorTheme) -> (bool, egui::Color32) {
        match self {
            Presence::Active => (true, theme.link_up),
            Presence::Away => (true, theme.warning),
            Presence::Offline => (false, theme.text_muted),
        }
    }

    /// The dot alone, with the word and a hover.
    pub fn dot(self, ui: &mut egui::Ui, hover: &str) -> egui::Response {
        let theme = sigil::ColorTheme::current(ui.ctx());
        let (filled, colour) = self.look(&theme);
        let response = crate::dot(ui, filled, colour, colour, self.word());
        if hover.is_empty() {
            response
        } else {
            response.on_hover_text(hover)
        }
    }
}

/// An avatar with a status dot on its corner: whose it is, and whether
/// they are there, in one mark.
///
/// The dot is [`dot`](crate::dot)'s -- filled or hollow, so the state
/// survives somebody who cannot tell the colours apart -- with a ring of
/// the surface behind it so it reads over a picture, and it carries
/// `word` the same way, on the mark's own accessibility node: the
/// presence's own word, or the link's for one's own mark with the link
/// down ("reconnecting…"). `hover` is what a pointer learns -- last seen,
/// the key.
pub fn presence(
    ui: &mut egui::Ui,
    key: &str,
    picture: Option<&egui::TextureHandle>,
    size: f32,
    seen: Presence,
    word: &str,
    hover: &str,
) -> egui::Response {
    let theme = sigil::ColorTheme::current(ui.ctx());
    let (filled, colour) = seen.look(&theme);
    // The link's own colour when the word is the link's: a mark that says
    // "reconnecting…" in the offline grey would be saying two things.
    let colour = if seen == Presence::Offline && word != seen.word() {
        theme.link_retrying
    } else {
        colour
    };
    let response = avatar(ui, key, picture, size);
    let rect = response.rect;
    let radius = tokens::SPACING_XS + 1.0;
    // On the corner, half inside the mark: a dot wholly inside is lost in
    // a busy picture, and one wholly outside belongs to nothing.
    let at = rect.right_bottom() - egui::vec2(radius, radius);
    ui.painter()
        .circle_filled(at, radius + tokens::STROKE_MEDIUM, theme.surface_primary);
    if filled {
        ui.painter().circle_filled(at, radius, colour);
    } else {
        ui.painter()
            .circle_stroke(at, radius, egui::Stroke::new(tokens::STROKE_MEDIUM, colour));
    }
    let said = word.to_string();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &said));
    response.on_hover_text(hover.to_string())
}

/// What a pointer learns from a mark: the state, when they were last
/// there, and the key -- a name is nobody's to vouch for (SIP-21).
///
/// `last_seen` is the exchange's clock, `now` this machine's reading of it;
/// nought is never seen.
pub fn presence_hover(seen: Presence, last_seen: u64, now: u64, key: &str) -> String {
    let when = match (seen, last_seen) {
        (Presence::Active, _) => String::new(),
        (_, 0) => " — never seen".to_string(),
        (Presence::Away, at) => format!(" — last active {}", crate::brief(at, now)),
        (Presence::Offline, at) => format!(" — last seen {}", crate::brief(at, now)),
    };
    format!("{}{when}\n{key}", seen.word())
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

    /// The raster is the mark: mirrored left to right, clear outside the
    /// circle, and not one colour.
    #[test]
    fn the_raster_is_a_mirrored_mark_in_a_circle() {
        let size = 48;
        let px = identicon_raster(b"11111111111111111111111111111111", size);
        assert_eq!(px.len(), size * size * 4);
        let at = |x: usize, y: usize| &px[(y * size + x) * 4..(y * size + x) * 4 + 4];
        assert_eq!(at(0, 0)[3], 0, "the corner is clear");
        assert_eq!(at(size / 2, size / 2)[3], 255, "the middle is solid");
        for y in 0..size {
            for x in 0..size / 2 {
                assert_eq!(at(x, y), at(size - 1 - x, y), "mirrored at {x},{y}");
            }
        }
        let distinct: std::collections::HashSet<&[u8]> = (0..size)
            .flat_map(|y| (0..size).map(move |x| (x, y)))
            .filter(|&(x, y)| at(x, y)[3] == 255)
            .map(|(x, y)| &at(x, y)[..3])
            .collect();
        assert!(distinct.len() >= 2, "one colour: {distinct:?}");
    }

    /// The launcher layer is opaque everywhere, the mark's circle spans
    /// the middle two thirds, and outside it is the mark's own background.
    #[test]
    fn the_launcher_layer_fills_its_square_with_the_mark_in_the_middle_two_thirds() {
        let size = 108;
        let id = b"11111111111111111111111111111111";
        let px = identicon_layer(id, size);
        assert_eq!(px.len(), size * size * 4);
        let at = |x: usize, y: usize| &px[(y * size + x) * 4..(y * size + x) * 4 + 4];
        let (back, _) = colours(id);
        let back = [back.r(), back.g(), back.b(), 255];
        assert_eq!(at(0, 0), &back, "the corner is the background colour");
        assert_eq!(
            at(1, size / 2),
            &back,
            "just inside the edge, left of the circle"
        );
        assert!(
            px.iter().skip(3).step_by(4).all(|&a| a == 255),
            "opaque throughout"
        );
        // The mark's circle reaches two thirds across: a point at 1/6 from
        // the edge on the middle row is on the circle's rim.
        let mark = identicon_raster(id, size * 2 / 3);
        let m = |x: usize, y: usize| {
            &mark[(y * (size * 2 / 3) + x) * 4..(y * (size * 2 / 3) + x) * 4 + 4]
        };
        assert_eq!(
            &at(size / 2, size / 2)[..3],
            &m(size / 3, size / 3)[..3],
            "the middle is the mark's middle"
        );
    }

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
