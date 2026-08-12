#!/usr/bin/env python3
#
# Build the splash and icon assets from the source art.
#
# Output is raw RGBA, not PNG, so the Rust side needs no image decoder and no
# new dependency. Each file is width * height * 4 bytes, top row first, and the
# dimensions live in assets/sizes.txt beside them.
#
# The source art is neon on pure black, so brightness is alpha. Anything else
# needs reconstruction and will not look as good: an earlier source had the
# transparency checkerboard painted into the pixels and cost a lot of work to
# undo.
#
# Run from the repo root:  python3 hack/logo.py

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import png

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(HERE, "assets", "source")
OUT = os.path.join(HERE, "assets")

NEON = os.path.join(SRC, "logo-neon.png")
TILE = os.path.join(SRC, "logo-tile.png")

SPLASH_SIZE = 512
ICON_SIZES = (256, 48, 32, 16)

ALPHA_BOOST = 1.30
BLACK_FLOOR = 6
TILE_RADIUS = 0.13
WATERMARK = (0.82, 0.99)


def cut_watermark(w, h, px):
    lo, hi = WATERMARK
    for y in range(int(h * lo), int(h * hi)):
        for x in range(int(w * lo), int(w * hi)):
            px[(y * w + x) * 4 + 3] = 0
    return px


def patch_watermark(w, h, px):
    lo, hi = WATERMARK
    for y in range(int(h * lo), int(h * hi)):
        for x in range(int(w * lo), int(w * hi)):
            o = (y * w + x) * 4
            s = (y * w + (w - 1 - x)) * 4
            px[o:o+3] = px[s:s+3]
    return px


def alpha_from_brightness(w, h, px):
    out = bytearray(w * h * 4)
    for i in range(0, len(px), 4):
        r, g, b = px[i], px[i+1], px[i+2]
        m = r if r > g else g
        if b > m:
            m = b
        if m <= BLACK_FLOOR:
            continue
        a = int(m * ALPHA_BOOST)
        out[i] = r
        out[i+1] = g
        out[i+2] = b
        out[i+3] = 255 if a > 255 else a
    return out


def trim(w, h, px, thresh=30, pad=10):
    minx, miny, maxx, maxy = w, h, 0, 0
    for y in range(h):
        row = y * w * 4
        for x in range(w):
            if px[row + x * 4 + 3] > thresh:
                minx = min(minx, x); maxx = max(maxx, x)
                miny = min(miny, y); maxy = max(maxy, y)
    side = max(maxx - minx, maxy - miny) + 1 + pad * 2
    cx, cy = (minx + maxx) // 2, (miny + maxy) // 2
    x0 = max(0, min(cx - side // 2, w - side))
    y0 = max(0, min(cy - side // 2, h - side))
    out = bytearray(side * side * 4)
    for y in range(side):
        s = ((y0 + y) * w + x0) * 4
        out[y*side*4:(y+1)*side*4] = px[s:s+side*4]
    return side, side, out


def square_tile(path):
    w, h, px = png.decode(path)

    minx, miny, maxx, maxy = w, h, 0, 0
    for y in range(h):
        for x in range(w):
            o = (y * w + x) * 4
            if not (px[o] > 235 and px[o+1] > 235 and px[o+2] > 235):
                minx = min(minx, x); maxx = max(maxx, x)
                miny = min(miny, y); maxy = max(maxy, y)

    side = min(maxx - minx + 1, maxy - miny + 1)
    tile = bytearray(side * side * 4)
    for y in range(side):
        s = ((miny + y) * w + minx) * 4
        tile[y*side*4:(y+1)*side*4] = px[s:s+side*4]

    tile = patch_watermark(side, side, tile)

    r = int(side * TILE_RADIUS)
    for y in range(side):
        for x in range(side):
            o = (y * side + x) * 4
            cx = min(max(x, r), side - 1 - r)
            cy = min(max(y, r), side - 1 - r)
            d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
            if d > r:
                tile[o+3] = 0
            elif d > r - 2:
                tile[o+3] = int(tile[o+3] * max(0.0, (r - d) / 2))
    return side, side, tile


def write_raw(name, w, h, px, sizes):
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(bytes(px))
    sizes.append(f"{name} {w} {h}")
    print(f"  {name:24} {w}x{h}  {len(px)//1024} KB")


def main():
    os.makedirs(OUT, exist_ok=True)
    sizes = []

    w, h, px = png.decode(NEON)
    neon = cut_watermark(w, h, alpha_from_brightness(w, h, px))
    nw, nh, neon = trim(w, h, neon)

    splash = png.resize(nw, nh, neon, SPLASH_SIZE, SPLASH_SIZE)
    write_raw("splash.rgba", SPLASH_SIZE, SPLASH_SIZE, splash, sizes)

    for size in ICON_SIZES:
        write_raw(f"icon-neon-{size}.rgba", size, size,
                  png.resize(nw, nh, neon, size, size), sizes)

    tw, th, tile = square_tile(TILE)
    for size in ICON_SIZES:
        write_raw(f"icon-tile-{size}.rgba", size, size,
                  png.resize(tw, th, tile, size, size), sizes)

    with open(os.path.join(OUT, "sizes.txt"), "w") as f:
        f.write("\n".join(sizes) + "\n")


if __name__ == "__main__":
    main()
