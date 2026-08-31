#!/usr/bin/env python3
"""Draw sigil's icon as a PNG, at whatever size is asked for.

No dependencies on purpose. An icon file checked into the repository is one
more thing to drift from the tray icon it is supposed to match, and one more
binary blob nobody can diff. This draws the same mark the tray draws -- a disc
in sigil's accent -- from the same numbers.
"""
import struct
import sys
import zlib

ACCENT = (0x6E, 0x8B, 0xFF)


def draw(size: int) -> bytes:
    """A filled disc, antialiased by sampling, on transparency."""
    centre = (size - 1) / 2.0
    # A little inset so the disc does not touch the edge; macOS gives icons
    # their own padding and a disc flush to the bounds looks larger than
    # everything beside it in the dock.
    radius = centre * 0.86
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            # Four samples a pixel: enough to take the jaggedness off a curve
            # at 16px, cheap enough not to matter at 1024.
            hits = 0
            for dx, dy in ((0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)):
                px, py = x + dx - 0.5, y + dy - 0.5
                if ((px - centre) ** 2 + (py - centre) ** 2) ** 0.5 <= radius:
                    hits += 1
            alpha = int(255 * hits / 4)
            row.extend((*ACCENT, alpha))
        rows.append(bytes(row))
    return encode(size, rows)


def encode(size: int, rows) -> bytes:
    """A minimal RGBA PNG."""
    raw = b"".join(b"\x00" + r for r in rows)  # filter byte 0 per scanline

    def chunk(kind: bytes, data: bytes) -> bytes:
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
    size = int(sys.argv[1]) if len(sys.argv) > 1 else 512
    out = sys.argv[2] if len(sys.argv) > 2 else "-"
    png = draw(size)
    if out == "-":
        sys.stdout.buffer.write(png)
    else:
        with open(out, "wb") as f:
            f.write(png)
