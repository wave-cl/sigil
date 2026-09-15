//! Sigil's mark, drawn: the same numbers as `packaging/icon.py`.
//!
//! A white hexagon, pointed left and right, on a black rounded square; in
//! it, four black arms of one width meet around the centre without
//! touching, each rounded on one corner where it turns towards the next,
//! so the white between them winds through the middle -- for the tray and
//! the dock. Two copies of one drawing, in two languages, because the app
//! icon is made at packaging time by a script with no dependencies and the
//! tray icon is made at run time by this crate; the test at the bottom
//! renders both and compares them pixel for pixel, so the two cannot drift
//! without somebody noticing. Change a number here, change it there.

const SQUARE: f64 = 0.80;
const CORNER: f64 = 0.146;
/// The hexagon: from the centre to a flat side, and its corner radius.
const HEX: f64 = 0.2706;
const HEX_CORNER: f64 = 0.037;
/// The arms: width, and how far from the centre each runs.
const ARM: f64 = 0.080;
const ARM_IN: f64 = 0.040;
const ARM_OUT: f64 = 0.210;
const ARM_TIP: f64 = 0.012;
/// The emblem alone is drawn larger: the menu bar gives a template a fixed
/// height and the square's margin around it is wasted there, so the
/// hexagon is scaled to nearly the canvas's width.
const GLYPH_SCALE: f64 = 1.55;
/// The mark of something waiting, on the glyph: a solid dot in the top
/// right corner, cut out of the emblem by a clear ring so it reads as a
/// dot on it rather than a lump of it.
const DOT: (f64, f64) = (0.84, 0.84);
const DOT_RADIUS: f64 = 0.13;
const DOT_GAP: f64 = 0.06;
const SAMPLES: u32 = 4;

/// One arm: centre, half width, half height, and a radius per corner in
/// the order top-right, bottom-right, top-left, bottom-left. The outer
/// corners are barely rounded; of the inner ones, the corner facing the
/// next arm anticlockwise is rounded by the arm's whole width and the other
/// is sharp, where it meets the next arm clockwise corner to corner.
struct Arm {
    cx: f64,
    cy: f64,
    hw: f64,
    hh: f64,
    radii: [f64; 4],
}

fn arms() -> [Arm; 4] {
    let half = (ARM_OUT - ARM_IN) / 2.0;
    let mid = ARM_IN + half;
    [
        // Up: rounded at its lower left.
        Arm {
            cx: 0.5,
            cy: 0.5 + mid,
            hw: ARM / 2.0,
            hh: half,
            radii: [ARM_TIP, 0.0, ARM_TIP, ARM],
        },
        // Right: rounded at its lower left.
        Arm {
            cx: 0.5 + mid,
            cy: 0.5,
            hw: half,
            hh: ARM / 2.0,
            radii: [ARM_TIP, ARM_TIP, 0.0, ARM],
        },
        // Down: rounded at its upper right.
        Arm {
            cx: 0.5,
            cy: 0.5 - mid,
            hw: ARM / 2.0,
            hh: half,
            radii: [ARM, ARM_TIP, 0.0, ARM_TIP],
        },
        // Left: rounded at its upper right.
        Arm {
            cx: 0.5 - mid,
            cy: 0.5,
            hw: half,
            hh: ARM / 2.0,
            radii: [ARM, 0.0, ARM_TIP, ARM_TIP],
        },
    ]
}

/// Signed distance to a box with a radius per corner: negative inside.
fn box_margin(x: f64, y: f64, arm: &Arm) -> f64 {
    let (dx, dy) = (x - arm.cx, y - arm.cy);
    let [tr, br, tl, bl] = arm.radii;
    let r = if dx >= 0.0 {
        if dy >= 0.0 { tr } else { br }
    } else if dy >= 0.0 {
        tl
    } else {
        bl
    };
    let (qx, qy) = (dx.abs() - arm.hw + r, dy.abs() - arm.hh + r);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    let inside = qx.max(qy).min(0.0);
    outside + inside - r
}

/// Signed distance to the rounded hexagon: negative inside.
fn hex_margin(x: f64, y: f64) -> f64 {
    // The rounding is applied to a hexagon shrunk by the corner radius, so
    // the flat sides land where HEX says.
    let apothem = HEX - HEX_CORNER;
    let circum = apothem / 30f64.to_radians().cos();
    let verts: Vec<(f64, f64)> = (0..6)
        .map(|i| {
            let a = (60.0 * i as f64).to_radians();
            (0.5 + circum * a.cos(), 0.5 + circum * a.sin())
        })
        .collect();
    let mut inside = true;
    let mut nearest = f64::INFINITY;
    for i in 0..6 {
        let (ax, ay) = verts[i];
        let (bx, by) = verts[(i + 1) % 6];
        let (ex, ey) = (bx - ax, by - ay);
        // The polygon is anticlockwise, so inside is to the left of each edge.
        if ex * (y - ay) - ey * (x - ax) < 0.0 {
            inside = false;
        }
        let t = (((x - ax) * ex + (y - ay) * ey) / (ex * ex + ey * ey)).clamp(0.0, 1.0);
        nearest = nearest.min((x - (ax + t * ex)).hypot(y - (ay + t * ey)));
    }
    (if inside { -nearest } else { nearest }) - HEX_CORNER
}

fn arms_margin(x: f64, y: f64) -> f64 {
    arms()
        .iter()
        .map(|a| box_margin(x, y, a))
        .fold(f64::INFINITY, f64::min)
}

fn dot_margin(x: f64, y: f64) -> f64 {
    (x - DOT.0).hypot(y - DOT.1) - DOT_RADIUS
}

fn dot_ring_margin(x: f64, y: f64) -> f64 {
    (x - DOT.0).hypot(y - DOT.1) - (DOT_RADIUS + DOT_GAP)
}

/// Signed distance to the rounded square: negative inside.
fn square_margin(x: f64, y: f64) -> f64 {
    let half = SQUARE / 2.0;
    let r = CORNER * SQUARE;
    let (dx, dy) = ((x - 0.5).abs() - (half - r), (y - 0.5).abs() - (half - r));
    let outside = dx.max(0.0).hypot(dy.max(0.0));
    let inside = dx.max(dy).min(0.0);
    outside + inside - r
}

/// How much of pixel (x, y) the shape covers, sampled SAMPLES x SAMPLES --
/// unless the signed distance at the pixel's centre says the whole pixel is
/// on one side, which it is for all but a thin band along every edge. Every
/// margin here is an exact distance, so that is safe. `scale` enlarges the
/// shape about the centre, and the distance with it.
fn coverage(size: u32, x: u32, y: u32, margin: fn(f64, f64) -> f64, scale: f64) -> f64 {
    let at = |px: f64, py: f64| margin(0.5 + (px - 0.5) / scale, 0.5 + (py - 0.5) / scale) * scale;
    let px = (x as f64 + 0.5) / size as f64;
    let py = 1.0 - (y as f64 + 0.5) / size as f64;
    let half_diagonal = 2f64.sqrt() / (2.0 * size as f64);
    let m = at(px, py);
    if m > half_diagonal {
        return 0.0;
    }
    if m < -half_diagonal {
        return 1.0;
    }
    let mut hits = 0;
    for sy in 0..SAMPLES {
        for sx in 0..SAMPLES {
            let px = (x as f64 + (sx as f64 + 0.5) / SAMPLES as f64) / size as f64;
            let py = 1.0 - (y as f64 + (sy as f64 + 0.5) / SAMPLES as f64) / size as f64;
            if at(px, py) <= 0.0 {
                hits += 1;
            }
        }
    }
    hits as f64 / (SAMPLES * SAMPLES) as f64
}

/// The white of the emblem at a pixel: the hexagon less the arms, which
/// lie wholly inside it.
fn emblem(size: u32, x: u32, y: u32, scale: f64) -> f64 {
    (coverage(size, x, y, hex_margin, scale) - coverage(size, x, y, arms_margin, scale)).max(0.0)
}

/// The app icon, `size` square, RGBA: the emblem on the rounded square.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let g = emblem(size, x, y, 1.0);
            let s = coverage(size, x, y, square_margin, 1.0);
            // Ties to even, as Python's `round` does, so the two agree exactly.
            let v = (255.0 * g).round_ties_even() as u8;
            out.extend_from_slice(&[v, v, v]);
            out.push((255.0 * s).round_ties_even() as u8);
        }
    }
    out
}

/// The emblem alone, `size` square, RGBA in `colour` with its coverage as
/// alpha: for a menu bar that tints a template, or a tray that wants the
/// mark without the square.
pub fn glyph_rgba(size: u32, colour: [u8; 3]) -> Vec<u8> {
    glyph(size, colour, false)
}

/// The same, with the dot: something is waiting.
pub fn glyph_rgba_marked(size: u32, colour: [u8; 3]) -> Vec<u8> {
    glyph(size, colour, true)
}

fn glyph(size: u32, colour: [u8; 3], marked: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut g = emblem(size, x, y, GLYPH_SCALE);
            if marked {
                // The emblem cleared around the dot, then the dot.
                g = (g * (1.0 - coverage(size, x, y, dot_ring_margin, 1.0))
                    + coverage(size, x, y, dot_margin, 1.0))
                .min(1.0);
            }
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
        python_png_as(size, glyph, false)
    }

    fn python_png_as(size: u32, glyph: bool, marked: bool) -> Vec<u8> {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../packaging/icon.py");
        let mut cmd = std::process::Command::new("python3");
        cmd.arg(script).arg(size.to_string()).arg("-");
        if glyph {
            cmd.arg("--glyph");
        }
        if marked {
            cmd.arg("--marked");
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

    /// And the marked one: the dot is solid, and the ring around it clear,
    /// so it stands off the emblem in a menu bar that only has alpha.
    #[test]
    fn the_marked_glyph_matches_and_has_its_dot() {
        let size = 64;
        let px = glyph_rgba_marked(size, [0, 0, 0]);
        compare(&px, &python_png_as(size, true, true), size);
        let alpha = |x: u32, y: u32| px[((y * size + x) * 4 + 3) as usize];
        assert_eq!(alpha(54, 10), 255, "the dot is solid");
        assert_eq!(alpha(46, 18), 0, "the ring around it is clear");
        let plain = glyph_rgba(size, [0, 0, 0]);
        assert!(
            plain[((18 * size + 46) * 4 + 3) as usize] > 0,
            "where the emblem was, before the ring cleared it"
        );
    }

    /// The glyph fills its canvas: a menu bar draws a template at a fixed
    /// height, so a mark drawn with the square's margin around it was two
    /// thirds the size of every other mark up there.
    #[test]
    fn the_glyph_fills_its_canvas() {
        let size = 64;
        let px = glyph_rgba(size, [0, 0, 0]);
        let alpha = |x: u32, y: u32| px[((y * size + x) * 4 + 3) as usize];
        assert!(alpha(1, 32) > 0, "the left point reaches the edge");
        assert!(alpha(62, 32) > 0, "and the right");
        assert_eq!(
            alpha(32, 3),
            0,
            "the flat top does not: a hexagon is wider than it is tall"
        );
        assert!(alpha(32, 7) > 0, "but it comes close");
    }

    /// The drawing is what it says: a black square with transparent
    /// corners, a white hexagon in it that does not reach the square's
    /// corners, and black arms in the hexagon that stop short of the centre.
    #[test]
    fn the_mark_is_an_emblem_on_a_rounded_square() {
        let size = 64;
        let px = icon_rgba(size);
        let at = |x: u32, y: u32| {
            let i = ((y * size + x) * 4) as usize;
            [px[i], px[i + 1], px[i + 2], px[i + 3]]
        };
        assert_eq!(at(0, 0)[3], 0, "the corner is outside the square");
        assert_eq!(at(32, 7)[3], 255, "the top edge's middle is inside it");
        assert_eq!(
            at(32, 9),
            [0, 0, 0, 255],
            "black between square and hexagon"
        );
        assert_eq!(at(9, 9), [0, 0, 0, 255], "the hexagon has no corner there");
        assert_eq!(at(32, 32), [255, 255, 255, 255], "the centre is white");
        assert_eq!(at(32, 20)[..3], [0, 0, 0], "the up arm");
        assert_eq!(at(44, 32)[..3], [0, 0, 0], "the right arm");
        assert_eq!(at(32, 44)[..3], [0, 0, 0], "the down arm");
        assert_eq!(at(20, 32)[..3], [0, 0, 0], "the left arm");
        assert_eq!(at(24, 24)[..3], [255, 255, 255], "white between the arms");
        assert_eq!(at(14, 32)[..3], [255, 255, 255], "the hexagon's left point");
    }
}
