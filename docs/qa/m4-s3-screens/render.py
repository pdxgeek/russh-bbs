#!/usr/bin/env python3
"""Render a `.cells` dump (see `tests/screens.rs`, `SALTMARCH_DUMP`) to a PNG with colours.

    python3 render.py in.cells out.png [cols rows]

Each line of the dump is `symbol <tab> fg <tab> bg <tab> flags`; a line holding only `|` ends a
row. Colours are `#rrggbb` or empty for the terminal default.
"""
import sys
from PIL import Image, ImageDraw, ImageFont

# The first font that has a glyph for a character draws it (the mono font has no braille).
FONTS = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/freefont/FreeMono.ttf",
]
BOLDS = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/freefont/FreeMonoBold.ttf",
]
DEFAULT_FG = (205, 210, 208)
DEFAULT_BG = (12, 16, 20)
CELL_W, CELL_H, SIZE = 11, 22, 18


# The sixteen named colours as xterm shows them (the harness dumps names at 16-colour depth).
NAMED = {
    "Black": (0, 0, 0), "Red": (205, 0, 0), "Green": (0, 205, 0), "Yellow": (205, 205, 0),
    "Blue": (0, 0, 238), "Magenta": (205, 0, 205), "Cyan": (0, 205, 205), "Gray": (229, 229, 229),
    "DarkGray": (127, 127, 127), "LightRed": (255, 0, 0), "LightGreen": (0, 255, 0),
    "LightYellow": (255, 255, 0), "LightBlue": (92, 92, 255), "LightMagenta": (255, 0, 255),
    "LightCyan": (0, 255, 255), "White": (255, 255, 255),
}


def hexcolor(text, default):
    if text.startswith("#") and len(text) == 7:
        return tuple(int(text[i : i + 2], 16) for i in (1, 3, 5))
    return NAMED.get(text, default)


def main():
    src, out = sys.argv[1], sys.argv[2]
    rows, row = [], []
    for line in open(src, encoding="utf-8").read().split("\n"):
        if line == "|":
            rows.append(row)
            row = []
        elif line:
            parts = line.split("\t")
            while len(parts) < 4:
                parts.append("")
            row.append(parts[:4])
    cols = max(len(r) for r in rows)
    img = Image.new("RGB", (cols * CELL_W + 16, len(rows) * CELL_H + 16), DEFAULT_BG)
    draw = ImageDraw.Draw(img)
    fonts = [ImageFont.truetype(f, SIZE) for f in FONTS]
    bolds = [ImageFont.truetype(f, SIZE) for f in BOLDS]
    missing = [bytes(f.getmask("\uffff")) for f in fonts]

    def pick(group, ch):
        for i, f in enumerate(group):
            if bytes(fonts[i].getmask(ch)) != missing[i]:
                return f
        return group[0]

    for y, r in enumerate(rows):
        for x, (sym, fg, bg, flags) in enumerate(r):
            f = hexcolor(fg, DEFAULT_FG)
            b = hexcolor(bg, DEFAULT_BG)
            if "r" in flags:
                f, b = b, f
            if "d" in flags:
                f = tuple(int(c * 0.6) for c in f)
            px, py = 8 + x * CELL_W, 8 + y * CELL_H
            if b != DEFAULT_BG:
                draw.rectangle([px, py, px + CELL_W - 1, py + CELL_H - 1], fill=b)
            if sym.strip():
                draw.text((px, py + 1), sym, font=pick(bolds if "b" in flags else fonts, sym[0]), fill=f)
            if "u" in flags:
                draw.line([px, py + CELL_H - 3, px + CELL_W - 1, py + CELL_H - 3], fill=f)
    # A few colours are all a terminal screen uses: a palette keeps the pictures small.
    img.quantize(colors=48, method=Image.Quantize.MEDIANCUT).save(out, optimize=True)


main()
