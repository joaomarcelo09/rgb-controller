#!/usr/bin/env python3
"""Generate Tauri icon set (PNG/ICO/ICNS) without external libraries."""
import struct
import zlib
import os

OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "icons")
os.makedirs(OUT, exist_ok=True)


def px(size, x, y):
    # Diagonal RGB stripes over dark background with a soft center glow.
    d = ((x + y) / (2 * size - 2)) % 1.0
    r, g, b = 0.0, 0.0, 0.0
    if d < 0.33:
        r = 1.0 - (d / 0.33) * 0.45
    elif d < 0.66:
        g = 1.0 - ((d - 0.33) / 0.33) * 0.45
    else:
        b = 1.0 - ((d - 0.66) / 0.34) * 0.45
    r, g, b = r * 0.85 + 0.10, g * 0.85 + 0.10, b * 0.85 + 0.10
    cx, cy = size / 2, size / 2
    dist = (((x - cx) ** 2 + (y - cy) ** 2) ** 0.5) / (size / 2)
    if dist < 0.85:
        glow = (0.85 - dist) / 0.85
        r += glow * 0.25
        g += glow * 0.25
        b += glow * 0.25
    return (
        max(0, min(255, int(r * 255))),
        max(0, min(255, int(g * 255))),
        max(0, min(255, int(b * 255))),
    )


def make_png(size):
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        for x in range(size):
            r, g, b = px(size, x, y)
            raw.extend((r, g, b, 255))

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def make_ico(pngs):
    entries = []
    data = bytearray()
    for p in pngs:
        entries.append(struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(p), len(data)))
        data.extend(p)
    header = struct.pack("<HHH", 0, 1, len(entries))
    return header + b"".join(entries) + bytes(data)


def make_icns(png128):
    chunk = b"ic07" + struct.pack(">I", len(png128) + 8) + png128
    return b"icns" + struct.pack(">I", len(chunk) + 8) + chunk


sizes = [32, 128, 256, 1024]
pngs = {s: make_png(s) for s in sizes}
with open(os.path.join(OUT, "32x32.png"), "wb") as f:
    f.write(pngs[32])
with open(os.path.join(OUT, "128x128.png"), "wb") as f:
    f.write(pngs[128])
with open(os.path.join(OUT, "128x128@2x.png"), "wb") as f:
    f.write(pngs[256])
with open(os.path.join(OUT, "icon.png"), "wb") as f:
    f.write(pngs[1024])
with open(os.path.join(OUT, "icon.ico"), "wb") as f:
    f.write(make_ico([pngs[32], pngs[128], pngs[256]]))
with open(os.path.join(OUT, "icon.icns"), "wb") as f:
    f.write(make_icns(pngs[128]))
print("icons written to", OUT)
