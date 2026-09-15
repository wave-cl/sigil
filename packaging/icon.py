#!/usr/bin/env python3
"""Draw Sigil's icon as a PNG, at whatever size is asked for.

No dependencies on purpose. An icon file checked into the repository is one
more thing to drift from the tray icon it is supposed to match, and one more
binary blob nobody can diff. This draws the same mark the tray draws, from
the same numbers -- `crates/sigil-platform/src/mark.rs` is the other copy,
and a test there compares the two pixel for pixel.

**The mark.** A white hexagon, pointed left and right, on a black rounded
square; in it, four black arms of one width meet around the centre without
touching, each rounded on one corner where it turns towards the next, so
the white between them winds through the middle. Every number below is a
fraction of the canvas, so the 16-pixel menu bar and the 1024-pixel dock
get the same shape.

    icon.py SIZE [OUT]                    the app icon: the emblem on the square
    icon.py SIZE [OUT] --glyph            the emblem alone, in black, for a template
    icon.py SIZE [OUT] --glyph --marked   the same with a dot: something is waiting
"""
import math
import struct
import sys
import zlib

WHITE = (0xFF, 0xFF, 0xFF)
BLACK = (0x00, 0x00, 0x00)

# The rounded square: 80% of the canvas, with corners a seventh of its side.
SQUARE = 0.80
CORNER = 0.146

# The hexagon: a regular one with its points at the left and right, given by
# the distance from the centre to a flat side, and rounded at the corners.
HEX = 0.2706
HEX_CORNER = 0.037

# The arms: each a box `ARM` wide running from `ARM_IN` to `ARM_OUT` of the
# centre. Its outer corners are barely rounded; of its inner corners, the
# one that faces the next arm anticlockwise is rounded by the arm's whole
# width, and the other is sharp, where it meets the next arm clockwise
# corner to corner -- so each arm reads as turning into the next. Given as
# (centre x, centre y, half width, half height, radii), the radii in the
# order top-right, bottom-right, top-left, bottom-left.
ARM = 0.080
ARM_IN = 0.040
ARM_OUT = 0.210
ARM_TIP = 0.012
_HALF = (ARM_OUT - ARM_IN) / 2
_MID = ARM_IN + _HALF
ARMS = (
    # Up: rounded at its lower left, meeting the right arm at its lower right.
    (0.5, 0.5 + _MID, ARM / 2, _HALF, (ARM_TIP, 0.0, ARM_TIP, ARM)),
    # Right: rounded at its lower left, meeting the up arm at its upper left.
    (0.5 + _MID, 0.5, _HALF, ARM / 2, (ARM_TIP, ARM_TIP, 0.0, ARM)),
    # Down: rounded at its upper right, meeting the left arm at its upper left.
    (0.5, 0.5 - _MID, ARM / 2, _HALF, (ARM, ARM_TIP, 0.0, ARM_TIP)),
    # Left: rounded at its upper right, meeting the down arm at its lower right.
    (0.5 - _MID, 0.5, _HALF, ARM / 2, (ARM, 0.0, ARM_TIP, ARM_TIP)),
)
# The emblem alone is drawn larger: the menu bar gives a template a fixed
# height and the square's margin around it is wasted there, so the hexagon
# is scaled to nearly the canvas's width.
GLYPH_SCALE = 1.55
# The mark of something waiting, on the glyph: a solid dot in the top right
# corner, cut out of the emblem by a clear ring so it reads as a dot on it
# rather than a lump of it.
DOT = (0.84, 0.84)
DOT_RADIUS = 0.13
DOT_GAP = 0.06
SAMPLES = 4


def box_margin(x, y, cx, cy, hw, hh, radii):
    """Signed distance to a box with a radius per corner: negative inside."""
    dx, dy = x - cx, y - cy
    tr, br, tl, bl = radii
    if dx >= 0:
        r = tr if dy >= 0 else br
    else:
        r = tl if dy >= 0 else bl
    qx, qy = abs(dx) - hw + r, abs(dy) - hh + r
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    inside = min(max(qx, qy), 0.0)
    return outside + inside - r


def hex_margin(x, y):
    """Signed distance to the rounded hexagon: negative inside."""
    # The rounding is applied to a hexagon shrunk by the corner radius, so
    # the flat sides land where HEX says.
    apothem = HEX - HEX_CORNER
    circum = apothem / math.cos(math.radians(30.0))
    verts = [
        (0.5 + circum * math.cos(math.radians(a)), 0.5 + circum * math.sin(math.radians(a)))
        for a in range(0, 360, 60)
    ]
    inside = True
    nearest = math.inf
    for i, (ax, ay) in enumerate(verts):
        bx, by = verts[(i + 1) % 6]
        ex, ey = bx - ax, by - ay
        # The polygon is anticlockwise, so inside is to the left of each edge.
        if ex * (y - ay) - ey * (x - ax) < 0:
            inside = False
        t = max(0.0, min(1.0, ((x - ax) * ex + (y - ay) * ey) / (ex * ex + ey * ey)))
        nearest = min(nearest, math.hypot(x - (ax + t * ex), y - (ay + t * ey)))
    return (-nearest if inside else nearest) - HEX_CORNER


def arms_margin(x, y):
    return min(box_margin(x, y, *arm) for arm in ARMS)


def dot_margin(x, y):
    return math.hypot(x - DOT[0], y - DOT[1]) - DOT_RADIUS


def dot_ring_margin(x, y):
    return math.hypot(x - DOT[0], y - DOT[1]) - (DOT_RADIUS + DOT_GAP)


def square_margin(x, y):
    """Signed distance to the rounded square: negative inside."""
    half = SQUARE / 2
    r = CORNER * SQUARE
    dx, dy = abs(x - 0.5) - (half - r), abs(y - 0.5) - (half - r)
    outside = math.hypot(max(dx, 0.0), max(dy, 0.0))
    inside = min(max(dx, dy), 0.0)
    return outside + inside - r


def coverage(size, x, y, margin, scale=1.0):
    """How much of pixel (x, y) the shape covers, sampled SAMPLES x SAMPLES
    -- unless the signed distance at the pixel's centre says the whole
    pixel is on one side, which it is for all but a thin band along every
    edge. Every margin here is an exact distance, so that is safe. `scale`
    enlarges the shape about the centre, and the distance with it."""

    def at(px, py):
        return margin(0.5 + (px - 0.5) / scale, 0.5 + (py - 0.5) / scale) * scale

    px = (x + 0.5) / size
    py = 1.0 - (y + 0.5) / size
    half_diagonal = math.sqrt(2.0) / (2.0 * size)
    m = at(px, py)
    if m > half_diagonal:
        return 0.0
    if m < -half_diagonal:
        return 1.0
    hits = 0
    for sy in range(SAMPLES):
        for sx in range(SAMPLES):
            px = (x + (sx + 0.5) / SAMPLES) / size
            py = 1.0 - (y + (sy + 0.5) / SAMPLES) / size
            if at(px, py) <= 0.0:
                hits += 1
    return hits / (SAMPLES * SAMPLES)


def draw(size, glyph_only=False, marked=False):
    scale = GLYPH_SCALE if glyph_only else 1.0
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            # The white of the emblem: the hexagon less the arms, which lie
            # wholly inside it.
            g = max(
                0.0,
                coverage(size, x, y, hex_margin, scale) - coverage(size, x, y, arms_margin, scale),
            )
            if marked:
                # The emblem cleared around the dot, then the dot.
                g = min(1.0, g * (1.0 - coverage(size, x, y, dot_ring_margin)) + coverage(size, x, y, dot_margin))
            if glyph_only:
                row.extend((*BLACK, int(round(255 * g))))
                continue
            s = coverage(size, x, y, square_margin)
            v = int(round(255 * g))
            row.extend((v, v, v, int(round(255 * s))))
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
    marked = "--marked" in sys.argv
    size = int(args[0]) if args else 512
    out = args[1] if len(args) > 1 else "-"
    png = draw(size, glyph_only, marked)
    if out == "-":
        sys.stdout.buffer.write(png)
    else:
        with open(out, "wb") as f:
            f.write(png)
