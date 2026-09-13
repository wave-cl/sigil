#!/usr/bin/env python3
"""Draw Sigil's icon as a PNG, at whatever size is asked for.

No dependencies on purpose. An icon file checked into the repository is one
more thing to drift from the tray icon it is supposed to match, and one more
binary blob nobody can diff. This draws the same mark the tray draws, from
the same numbers -- `crates/sigil-platform/src/mark.rs` is the other copy,
and a test there compares the two pixel for pixel.

**The mark.** A seal: a white S drawn as two arcs on a rounded square in
Sigil's accent. The S is the name; the two arcs, each stopping short of the
other, are a sigil's habit of being one stroke that nearly closes. Every
number below is a fraction of the canvas, so the 16-pixel menu bar and the
1024-pixel dock get the same shape.

    icon.py SIZE [OUT]           the app icon: S on the rounded square
    icon.py SIZE [OUT] --glyph   the S alone, in black, for a template icon
"""
import math
import struct
import sys
import zlib

ACCENT = (0x6E, 0x8B, 0xFF)
WHITE = (0xFF, 0xFF, 0xFF)
BLACK = (0x00, 0x00, 0x00)

# The rounded square: Apple's icon grid puts the shape on 80% of the canvas,
# with corners a little over a fifth of its side.
SQUARE = 0.80
CORNER = 0.225

# The S: two arcs of one radius, one above the centre and one below, meeting
# at the centre. Angles are degrees, anticlockwise, with y up -- as on paper.
# The top arc runs from its lower right round the top to the bottom; the
# lower arc from the top round the right and bottom to its upper left. Each
# stops short of where a closed ring would continue.
RADIUS = 0.16
STROKE = 0.085
TOP = (0.5, 0.5 + RADIUS, 15.0, 270.0)  # centre x, centre y, from, to (anticlockwise)
BOTTOM = (0.5, 0.5 - RADIUS, -165.0, 90.0)
SAMPLES = 4


def _in_arc(deg, start, end):
    """Whether `deg` lies on the anticlockwise arc from `start` to `end`."""
    span = (end - start) % 360.0
    return (deg - start) % 360.0 <= span


def glyph_hit(x, y):
    """Whether the point (x, y), in canvas fractions with y up, is on the S."""
    for cx, cy, start, end in (TOP, BOTTOM):
        dx, dy = x - cx, y - cy
        dist = math.hypot(dx, dy)
        if abs(dist - RADIUS) <= STROKE / 2 and _in_arc(math.degrees(math.atan2(dy, dx)), start, end):
            return True
        # Round caps at both ends.
        for deg in (start, end):
            ex = cx + RADIUS * math.cos(math.radians(deg))
            ey = cy + RADIUS * math.sin(math.radians(deg))
            if math.hypot(x - ex, y - ey) <= STROKE / 2:
                return True
    return False


def square_hit(x, y):
    """Whether (x, y) is inside the rounded square."""
    half = SQUARE / 2
    r = CORNER * SQUARE
    dx, dy = abs(x - 0.5), abs(y - 0.5)
    if dx > half or dy > half:
        return False
    if dx <= half - r or dy <= half - r:
        return True
    return math.hypot(dx - (half - r), dy - (half - r)) <= r


def coverage(size, x, y, hit):
    """How much of pixel (x, y) `hit` covers, sampled SAMPLES x SAMPLES."""
    hits = 0
    for sy in range(SAMPLES):
        for sx in range(SAMPLES):
            px = (x + (sx + 0.5) / SAMPLES) / size
            py = 1.0 - (y + (sy + 0.5) / SAMPLES) / size
            if hit(px, py):
                hits += 1
    return hits / (SAMPLES * SAMPLES)


def draw(size, glyph_only=False):
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            g = coverage(size, x, y, glyph_hit)
            if glyph_only:
                row.extend((*BLACK, int(round(255 * g))))
                continue
            s = coverage(size, x, y, square_hit)
            # The S over the square: white where the S is, accent where only
            # the square is, and the square's edge is the icon's edge.
            r, gr, b = (
                int(round(ACCENT[i] + (WHITE[i] - ACCENT[i]) * g)) for i in range(3)
            )
            row.extend((r, gr, b, int(round(255 * s))))
        rows.append(bytes(row))
    return encode(size, rows)


def encode(size, rows):
    """A minimal RGBA PNG."""
    raw = b"".join(b"\x00" + r for r in rows)  # filter byte 0 per scanline

    def chunk(kind, data):
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
        )

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    glyph_only = "--glyph" in sys.argv
    size = int(args[0]) if args else 512
    out = args[1] if len(args) > 1 else "-"
    png = draw(size, glyph_only)
    if out == "-":
        sys.stdout.buffer.write(png)
    else:
        with open(out, "wb") as f:
            f.write(png)
