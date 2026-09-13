//! Sigil's mark, drawn: the same numbers as `packaging/icon.py`.
//!
//! A seal: a white S of two arcs, its free ends finished with a point, in
//! a ring broken where the stroke leaves it, on a rounded square in the
//! accent -- for the tray and the dock. Two copies of one drawing, in two languages,
//! because the app icon is made at packaging time by a script with no
//! dependencies and the tray icon is made at run time by this crate; the
//! test at the bottom renders both and compares them pixel for pixel, so
//! the two cannot drift without somebody noticing. Change a number here,
//! change it there.

/// `#6E8BFF`.
pub const ACCENT: [u8; 3] = [0x6E, 0x8B, 0xFF];

const SQUARE: f64 = 0.80;
const CORNER: f64 = 0.225;
const RADIUS: f64 = 0.14;
const STROKE: f64 = 0.075;
/// Centre x, centre y, from, to -- degrees, anticlockwise, y up.
const TOP: (f64, f64, f64, f64) = (0.5, 0.5 + RADIUS, 15.0, 270.0);
const BOTTOM: (f64, f64, f64, f64) = (0.5, 0.5 - RADIUS, -165.0, 90.0);
/// A sigil's strokes end in a point: a disc at each free end of the S.
const TERMINAL: f64 = 0.062;
/// And a seal is drawn in a ring, broken where the S's free ends point out
/// of it, so the S reads as one stroke leaving the ring and coming back.
const RING: f64 = 0.355;
const RING_STROKE: f64 = 0.04;
const RING_GAPS: [(f64, f64); 2] = [(30.0, 78.0), (210.0, 258.0)];
const SAMPLES: u32 = 4;

fn in_arc(deg: f64, start: f64, end: f64) -> bool {
    let span = (end - start).rem_euclid(360.0);
    (deg - start).rem_euclid(360.0) <= span
}

/// Whether (x, y) is on the arc `(cx, cy, from, to)` of `radius`, drawn
/// `stroke` wide.
fn on_arc(x: f64, y: f64, arc: (f64, f64, f64, f64), radius: f64, stroke: f64) -> bool {
    let (cx, cy, start, end) = arc;
    let (dx, dy) = (x - cx, y - cy);
    if (dx.hypot(dy) - radius).abs() > stroke / 2.0 {
        return false;
    }
    in_arc(dy.atan2(dx).to_degrees(), start, end)
}

fn end(cx: f64, cy: f64, radius: f64, deg: f64) -> (f64, f64) {
    (
        cx + radius * deg.to_radians().cos(),
        cy + radius * deg.to_radians().sin(),
    )
}

/// Whether (x, y), in canvas fractions with y up, is on the mark.
fn glyph_hit(x: f64, y: f64) -> bool {
    // The S, with a round cap where the two arcs meet and a terminal disc
    // at each free end: the top arc's start, the bottom arc's end.
    for (arc, free_is_start) in [(TOP, true), (BOTTOM, false)] {
        let (cx, cy, start, stop) = arc;
        if on_arc(x, y, arc, RADIUS, STROKE) {
            return true;
        }
        for (deg, is_start) in [(start, true), (stop, false)] {
            let (ex, ey) = end(cx, cy, RADIUS, deg);
            let r = if is_start == free_is_start {
                TERMINAL
            } else {
                STROKE / 2.0
            };
            if (x - ex).hypot(y - ey) <= r {
                return true;
            }
        }
    }
    // The ring, less its gaps, with round caps at each break.
    let (dx, dy) = (x - 0.5, y - 0.5);
    if (dx.hypot(dy) - RING).abs() <= RING_STROKE / 2.0 {
        let deg = dy.atan2(dx).to_degrees();
        if !RING_GAPS.iter().any(|&(a, b)| in_arc(deg, a, b)) {
            return true;
        }
    }
    for (a, b) in RING_GAPS {
        for deg in [a, b] {
            let (ex, ey) = end(0.5, 0.5, RING, deg);
            if (x - ex).hypot(y - ey) <= RING_STROKE / 2.0 {
                return true;
            }
        }
    }
    false
}

/// Whether (x, y) is inside the rounded square.
fn square_hit(x: f64, y: f64) -> bool {
    let half = SQUARE / 2.0;
    let r = CORNER * SQUARE;
    let (dx, dy) = ((x - 0.5).abs(), (y - 0.5).abs());
    if dx > half || dy > half {
        return false;
    }
    if dx <= half - r || dy <= half - r {
        return true;
    }
    (dx - (half - r)).hypot(dy - (half - r)) <= r
}

fn coverage(size: u32, x: u32, y: u32, hit: fn(f64, f64) -> bool) -> f64 {
    let mut hits = 0;
    for sy in 0..SAMPLES {
        for sx in 0..SAMPLES {
            let px = (x as f64 + (sx as f64 + 0.5) / SAMPLES as f64) / size as f64;
            let py = 1.0 - (y as f64 + (sy as f64 + 0.5) / SAMPLES as f64) / size as f64;
            if hit(px, py) {
                hits += 1;
            }
        }
    }
    hits as f64 / (SAMPLES * SAMPLES) as f64
}

/// The app icon, `size` square, RGBA: the S on the rounded square.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let g = coverage(size, x, y, glyph_hit);
            let s = coverage(size, x, y, square_hit);
            for c in ACCENT {
                // Ties to even, as Python's `round` does, so the two agree exactly.
                out.push((c as f64 + (255.0 - c as f64) * g).round_ties_even() as u8);
            }
            out.push((255.0 * s).round_ties_even() as u8);
        }
    }
    out
}

/// The S alone, `size` square, RGBA in `colour` with its coverage as alpha:
/// for a menu bar that tints a template, or a tray that wants the glyph
/// without the square.
pub fn glyph_rgba(size: u32, colour: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let g = coverage(size, x, y, glyph_hit);
            out.extend_from_slice(&colour);
            out.push((255.0 * g).round_ties_even() as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `packaging/icon.py`, run for real, decoded, and compared: the tray
    /// and the dock are one drawing or they are two.
    fn python_png(size: u32, glyph: bool) -> Vec<u8> {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packaging/icon.py");
        let mut cmd = std::process::Command::new("python3");
        cmd.arg(script).arg(size.to_string()).arg("-");
        if glyph {
            cmd.arg("--glyph");
        }
        let out = cmd.output().expect("python3 runs packaging/icon.py");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let img = image::load_from_memory(&out.stdout).expect("a png");
        img.to_rgba8().into_raw()
    }

    fn compare(ours: &[u8], theirs: &[u8], size: u32) {
        assert_eq!(ours.len(), theirs.len());
        // Exactly, every channel: the same numbers, the same sampling, the
        // same rounding (ties to even, both sides) leave nothing to differ.
        for (i, (a, b)) in ours.iter().zip(theirs).enumerate() {
            assert_eq!(
                a,
                b,
                "pixel ({}, {}) channel {} differs: rust {a}, python {b}",
                (i / 4) % size as usize,
                (i / 4) / size as usize,
                i % 4
            );
        }
    }

    #[test]
    fn the_tray_draws_the_icon_the_package_draws() {
        for size in [16, 32, 64] {
            compare(&icon_rgba(size), &python_png(size, false), size);
        }
    }

    #[test]
    fn the_glyph_alone_matches_too() {
        compare(&glyph_rgba(32, [0, 0, 0]), &python_png(32, true), 32);
    }

    /// The drawing is not empty, not a disc, and has the S in it: the middle
    /// row of the icon crosses the stroke, so it has white in it, and the
    /// corners are transparent while the edges' middles are not.
    #[test]
    fn the_mark_is_an_s_on_a_rounded_square() {
        let size = 64;
        let px = icon_rgba(size);
        let at = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(0, 0)[3], 0, "the corner is outside the square");
        assert_eq!(at(32, 7)[3], 255, "the top edge's middle is inside it");
        assert_eq!(
            at(32, 32),
            [255, 255, 255, 255],
            "the centre is the S's waist"
        );
        assert_eq!(at(32, 14)[..3], [255, 255, 255], "the top of the S");
        assert_eq!(at(32, 50)[..3], [255, 255, 255], "the bottom of the S");
        assert_eq!(at(32, 23)[..3], ACCENT, "the top bowl is open");
        assert_eq!(at(32, 41)[..3], ACCENT, "and so is the bottom one");
        assert_eq!(at(32, 9)[..3], [255, 255, 255], "the ring, above the S");
        assert_eq!(
            at(54, 20)[..3],
            ACCENT,
            "broken where the S's end points out"
        );
    }
}
