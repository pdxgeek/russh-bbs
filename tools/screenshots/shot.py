#!/usr/bin/env python3
"""Shared terminal-screenshot renderer for docs (PNG from `.cells`, a pyte screen, tmux or ANSI text).

Box drawing and block characters (U+2500-U+259F) are drawn as exact-size rectangles, never from a
font, so lines join across cells without gaps. Everything else uses a font fallback chain.

    python3 shot.py in.cells out.png [--scale 2]
    python3 shot.py --ansi screen.ans --size 100x30 out.png
    python3 shot.py --tmux-target bbs:0.0 out.png

Only Pillow is required; `pyte` is imported lazily (needed for --ansi, --tmux-target, render_pyte).
"""
import argparse
import dataclasses
import subprocess
import sys
import unicodedata
from typing import Dict, List, Optional, Sequence, Tuple

from PIL import Image, ImageDraw, ImageFont

RGB = Tuple[int, int, int]
Cell = Tuple[str, str, str, str]  # symbol, fg, bg, flags

FONTS = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/freefont/FreeMono.ttf",
]
BOLD_FONTS = [
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/freefont/FreeMonoBold.ttf",
]

# The sixteen ratatui colour names as xterm shows them (what the `.cells` dumps use).
NAMED: Dict[str, RGB] = {
    "Black": (0, 0, 0), "Red": (205, 0, 0), "Green": (0, 205, 0), "Yellow": (205, 205, 0),
    "Blue": (0, 0, 238), "Magenta": (205, 0, 205), "Cyan": (0, 205, 205), "Gray": (229, 229, 229),
    "DarkGray": (127, 127, 127), "LightRed": (255, 0, 0), "LightGreen": (0, 255, 0),
    "LightYellow": (255, 255, 0), "LightBlue": (92, 92, 255), "LightMagenta": (255, 0, 255),
    "LightCyan": (0, 255, 255), "White": (255, 255, 255),
}

# pyte colour names (as in walk.py).
PYTE_NAMES: Dict[str, str] = {
    "black": "#000000", "red": "#cd3131", "green": "#0dbc79", "brown": "#e5e510",
    "blue": "#2472c8", "magenta": "#bc3fbc", "cyan": "#11a8cd", "white": "#e5e5e5",
    "brightblack": "#666666", "brightred": "#f14c4c", "brightgreen": "#23d18b",
    "brightbrown": "#f5f543", "brightblue": "#3b8eea", "brightmagenta": "#d670d6",
    "brightcyan": "#29b8db", "brightwhite": "#ffffff",
}


@dataclasses.dataclass
class Options:
    """Everything overridable. Geometry is at scale 1; `scale` multiplies all of it."""

    scale: int = 1
    cell_w: int = 11
    cell_h: int = 22
    font_size: int = 18
    pad: int = 8
    default_fg: RGB = (205, 210, 208)
    default_bg: RGB = (12, 16, 20)
    named: Dict[str, RGB] = dataclasses.field(default_factory=lambda: dict(NAMED))
    pyte_names: Dict[str, str] = dataclasses.field(default_factory=lambda: dict(PYTE_NAMES))
    fonts: Sequence[str] = tuple(FONTS)
    bold_fonts: Sequence[str] = tuple(BOLD_FONTS)
    quantize: int = 0  # >0: reduce to N colours (smaller PNG, as the old render.py did with 48)


# ---------------------------------------------------------------- `.cells` format

def parse_cells(text: str) -> List[List[Cell]]:
    """Parse a `.cells` dump: `symbol<TAB>fg<TAB>bg<TAB>flags` per cell, a lone `|` ends a row."""
    rows: List[List[Cell]] = []
    row: List[Cell] = []
    for line in text.split("\n"):
        if line == "|":
            rows.append(row)
            row = []
        elif line:
            parts = line.split("\t")
            while len(parts) < 4:
                parts.append("")
            row.append((parts[0], parts[1], parts[2], parts[3]))
    return rows


def format_cells(rows: Sequence[Sequence[Cell]]) -> str:
    out = []
    for row in rows:
        out.extend("\t".join(c) for c in row)
        out.append("|")
    return "\n".join(out) + "\n"


# ---------------------------------------------------------------- colours

def resolve_color(text: str, default: RGB, opts: Options) -> RGB:
    if not text or text == "default":
        return default
    if text.startswith("#") and len(text) == 7:
        return tuple(int(text[i:i + 2], 16) for i in (1, 3, 5))  # type: ignore[return-value]
    if text in opts.named:
        return opts.named[text]
    low = text.lower()
    if text in opts.pyte_names:  # pyte's lowercase names ("red") win over ratatui's ("Red")
        return resolve_color(opts.pyte_names[text], default, opts)
    for k, v in opts.named.items():
        if k.lower() == low:
            return v
    if len(text) == 6:
        try:
            return tuple(int(text[i:i + 2], 16) for i in (0, 2, 4))  # type: ignore[return-value]
        except ValueError:
            pass
    return default


def blend(fg: RGB, bg: RGB, alpha: float) -> RGB:
    return tuple(round(f * alpha + b * (1 - alpha)) for f, b in zip(fg, bg))  # type: ignore[return-value]


# ---------------------------------------------------------------- box drawing tables
# Arms are (left, right, up, down), each 0 none, 1 light, 2 heavy, 3 double.

def _arms() -> Dict[int, Tuple[int, int, int, int]]:
    t: Dict[int, Tuple[int, int, int, int]] = {}
    t[0x2500], t[0x2501] = (1, 1, 0, 0), (2, 2, 0, 0)
    t[0x2502], t[0x2503] = (0, 0, 1, 1), (0, 0, 2, 2)
    # corners 250C..251B: (h style, v style) pairs, four corners x four combinations
    combos = [(1, 1), (2, 1), (1, 2), (2, 2)]  # (horizontal, vertical)
    for i, (h, v) in enumerate(combos):
        t[0x250C + i] = (0, h, 0, v)  # down-right
        t[0x2510 + i] = (h, 0, 0, v)  # down-left
        t[0x2514 + i] = (0, h, v, 0)  # up-right
        t[0x2518 + i] = (h, 0, v, 0)  # up-left
    # tees 251C..253B. Each family lists 8 variants: (vert up, vert down, horiz) patterns.
    # vertical tees: stem on the right (251C) or left (2524); variants (up, down, horizontal)
    vt = [(1, 1, 1), (1, 1, 2), (2, 1, 1), (1, 2, 1), (2, 2, 1), (2, 1, 2), (1, 2, 2), (2, 2, 2)]
    for i, (u, d, h) in enumerate(vt):
        t[0x251C + i] = (0, h, u, d)
        t[0x2524 + i] = (h, 0, u, d)
    # horizontal tees: stem down (252C) or up (2534); variants (left, right, vertical)
    ht = [(1, 1, 1), (2, 1, 1), (1, 2, 1), (2, 2, 1), (1, 1, 2), (2, 1, 2), (1, 2, 2), (2, 2, 2)]
    for i, (l, r, v) in enumerate(ht):
        t[0x252C + i] = (l, r, 0, v)
        t[0x2534 + i] = (l, r, v, 0)
    # crosses 253C..254B
    # The Unicode order of 253C..254B is: all light, L, R, LR, U, D, UD, UL, UR, DL, DR, then
    # the three-heavy and all-heavy forms. Spell it out so the table is checkable by eye.
    cross = [
        (1, 1, 1, 1),  # 253C ┼
        (2, 1, 1, 1),  # 253D ┽
        (1, 2, 1, 1),  # 253E ┾
        (2, 2, 1, 1),  # 253F ┿
        (1, 1, 2, 1),  # 2540 ╀
        (1, 1, 1, 2),  # 2541 ╁
        (1, 1, 2, 2),  # 2542 ╂
        (2, 1, 2, 1),  # 2543 ╃
        (1, 2, 2, 1),  # 2544 ╄
        (2, 1, 1, 2),  # 2545 ╅
        (1, 2, 1, 2),  # 2546 ╆
        (2, 2, 2, 1),  # 2547 ╇
        (2, 2, 1, 2),  # 2548 ╈
        (2, 1, 2, 2),  # 2549 ╉
        (1, 2, 2, 2),  # 254A ╊
        (2, 2, 2, 2),  # 254B ╋
    ]
    for i, a in enumerate(cross):
        t[0x253C + i] = a
    # double and double/single: 2550..256C
    d = {
        0x2550: (3, 3, 0, 0), 0x2551: (0, 0, 3, 3),
        0x2552: (0, 3, 0, 1), 0x2553: (0, 1, 0, 3), 0x2554: (0, 3, 0, 3),
        0x2555: (3, 0, 0, 1), 0x2556: (1, 0, 0, 3), 0x2557: (3, 0, 0, 3),
        0x2558: (0, 3, 1, 0), 0x2559: (0, 1, 3, 0), 0x255A: (0, 3, 3, 0),
        0x255B: (3, 0, 1, 0), 0x255C: (1, 0, 3, 0), 0x255D: (3, 0, 3, 0),
        0x255E: (0, 3, 1, 1), 0x255F: (0, 1, 3, 3), 0x2560: (0, 3, 3, 3),
        0x2561: (3, 0, 1, 1), 0x2562: (1, 0, 3, 3), 0x2563: (3, 0, 3, 3),
        0x2564: (3, 3, 0, 1), 0x2565: (1, 1, 0, 3), 0x2566: (3, 3, 0, 3),
        0x2567: (3, 3, 1, 0), 0x2568: (1, 1, 3, 0), 0x2569: (3, 3, 3, 0),
        0x256A: (3, 3, 1, 1), 0x256B: (1, 1, 3, 3), 0x256C: (3, 3, 3, 3),
        # rounded corners (drawn as arcs, arms recorded for the join)
        0x256D: (0, 1, 0, 1), 0x256E: (1, 0, 0, 1), 0x256F: (1, 0, 1, 0), 0x2570: (0, 1, 1, 0),
        # half lines 2574..257F
        0x2574: (1, 0, 0, 0), 0x2575: (0, 0, 1, 0), 0x2576: (0, 1, 0, 0), 0x2577: (0, 0, 0, 1),
        0x2578: (2, 0, 0, 0), 0x2579: (0, 0, 2, 0), 0x257A: (0, 2, 0, 0), 0x257B: (0, 0, 0, 2),
        0x257C: (1, 2, 0, 0), 0x257D: (0, 0, 1, 2), 0x257E: (2, 1, 0, 0), 0x257F: (0, 0, 2, 1),
    }
    t.update(d)
    return t


ARMS = _arms()
ROUNDED = {0x256D, 0x256E, 0x256F, 0x2570}
# Dashed lines: code -> (horizontal?, dashes, style)
DASHES = {
    0x2504: (True, 3, 1), 0x2505: (True, 3, 2), 0x2506: (False, 3, 1), 0x2507: (False, 3, 2),
    0x2508: (True, 4, 1), 0x2509: (True, 4, 2), 0x250A: (False, 4, 1), 0x250B: (False, 4, 2),
    0x254C: (True, 2, 1), 0x254D: (True, 2, 2), 0x254E: (False, 2, 1), 0x254F: (False, 2, 2),
}
# Blocks: code -> list of (x0, x1, y0, y1) in eighths of the cell (quadrants use halves = 4/8).
BLOCKS: Dict[int, List[Tuple[int, int, int, int]]] = {0x2580: [(0, 8, 0, 4)], 0x2588: [(0, 8, 0, 8)],
                                                       0x2590: [(4, 8, 0, 8)], 0x2594: [(0, 8, 0, 1)],
                                                       0x2595: [(7, 8, 0, 8)]}
for _i in range(1, 8):
    BLOCKS[0x2580 + _i] = [(0, 8, 8 - _i, 8)]  # lower n/8
    BLOCKS[0x2588 + _i] = [(0, 8 - _i, 0, 8)]  # left: 2589 is 7/8 ... 258F is 1/8
_UL, _UR, _LL, _LR = (0, 4, 0, 4), (4, 8, 0, 4), (0, 4, 4, 8), (4, 8, 4, 8)
BLOCKS.update({
    0x2596: [_LL], 0x2597: [_LR], 0x2598: [_UL], 0x2599: [_UL, _LL, _LR], 0x259A: [_UL, _LR],
    0x259B: [_UL, _UR, _LL], 0x259C: [_UL, _UR, _LR], 0x259D: [_UR], 0x259E: [_UR, _LL],
    0x259F: [_UR, _LL, _LR],
})
SHADES = {0x2591: 0.25, 0x2592: 0.5, 0x2593: 0.75}


def is_drawn(ch: str) -> bool:
    """True for characters this module draws itself instead of taking from a font."""
    return len(ch) == 1 and 0x2500 <= ord(ch) <= 0x259F


def _bands(style: int, n: int, s: int) -> List[Tuple[int, int]]:
    """Stroke bands (lo, hi) across an axis of n pixels for an arm style."""
    if style == 3:
        c = (n - s) // 2
        return [(c - 2 * s, c - s), (c + 2 * s, c + 3 * s)]
    t = s if style == 1 else 3 * s
    c = (n - t) // 2
    return [(c, c + t)]


def _rect(draw, x0, y0, x1, y1, fill):
    """Fill the half-open pixel box [x0,x1) x [y0,y1)."""
    if x1 > x0 and y1 > y0:
        draw.rectangle([x0, y0, x1 - 1, y1 - 1], fill=fill)


def _draw_lines(draw, ox, oy, w, h, s, arms, color):
    l, r, u, d = arms
    # Stroke bands of the crossing axis come from the heavier arm, else a centred light line.
    V = _bands(max(u, d) or 1, w, s)
    H = _bands(max(l, r) or 1, h, s)

    # Horizontal arms: strokes (y bands) run from the cell edge to a vertical-stroke edge.
    for arm, right in ((l, False), (r, True)):
        if not arm:
            continue
        bands = _bands(arm, h, s)
        for i, (lo, hi) in enumerate(bands):
            if u and d:
                far = False
            elif u or d:
                far = (i == 0) if d else (i == len(bands) - 1)
            else:
                far = True
            if right:
                x0 = (V[-1][0] if not far else V[0][0]) if (u or d) else V[0][0]
                _rect(draw, ox + x0, oy + lo, ox + w, oy + hi, color)
            else:
                x1 = (V[0][1] if not far else V[-1][1]) if (u or d) else V[-1][1]
                _rect(draw, ox, oy + lo, ox + x1, oy + hi, color)
    # Vertical arms.
    for arm, down in ((u, False), (d, True)):
        if not arm:
            continue
        bands = _bands(arm, w, s)
        for i, (lo, hi) in enumerate(bands):
            if l and r:
                far = False
            elif l or r:
                far = (i == 0) if r else (i == len(bands) - 1)
            else:
                far = True
            if down:
                y0 = (H[-1][0] if not far else H[0][0]) if (l or r) else H[0][0]
                _rect(draw, ox + lo, oy + y0, ox + hi, oy + h, color)
            else:
                y1 = (H[0][1] if not far else H[-1][1]) if (l or r) else H[-1][1]
                _rect(draw, ox + lo, oy, ox + hi, oy + y1, color)


def _draw_rounded(draw, ox, oy, w, h, s, code, color):
    t = s
    cx0, cy0 = (w - t) // 2, (h - t) // 2  # top-left pixel of the centre stroke
    rad = max(2 * s, w // 2 - 1)
    # Straight tails from the edge to where the arc starts, then the arc itself.
    if code == 0x256D:  # right + down
        _rect(draw, ox + cx0 + rad, oy + cy0, ox + w, oy + cy0 + t, color)
        _rect(draw, ox + cx0, oy + cy0 + rad, ox + cx0 + t, oy + h, color)
        box, a0, a1 = (cx0, cy0, cx0 + 2 * rad, cy0 + 2 * rad), 180, 270
    elif code == 0x256E:  # left + down
        _rect(draw, ox, oy + cy0, ox + cx0 + t - rad, oy + cy0 + t, color)
        _rect(draw, ox + cx0, oy + cy0 + rad, ox + cx0 + t, oy + h, color)
        box, a0, a1 = (cx0 + t - 2 * rad, cy0, cx0 + t, cy0 + 2 * rad), 270, 360
    elif code == 0x256F:  # left + up
        _rect(draw, ox, oy + cy0, ox + cx0 + t - rad, oy + cy0 + t, color)
        _rect(draw, ox + cx0, oy, ox + cx0 + t, oy + cy0 + t - rad, color)
        box, a0, a1 = (cx0 + t - 2 * rad, cy0 + t - 2 * rad, cx0 + t, cy0 + t), 0, 90
    else:  # 0x2570 right + up
        _rect(draw, ox + cx0 + rad, oy + cy0, ox + w, oy + cy0 + t, color)
        _rect(draw, ox + cx0, oy, ox + cx0 + t, oy + cy0 + t - rad, color)
        box, a0, a1 = (cx0, cy0 + t - 2 * rad, cx0 + 2 * rad, cy0 + t), 90, 180
    draw.arc([ox + box[0], oy + box[1], ox + box[2] - 1, oy + box[3] - 1], a0, a1, fill=color, width=t)


def _draw_dashes(draw, ox, oy, w, h, s, spec, color):
    horiz, n, style = spec
    t = s if style == 1 else 3 * s
    length = w if horiz else h
    seg = length / n
    gap = seg * 0.2
    other = h if horiz else w
    c = (other - t) // 2
    for i in range(n):
        a = round(i * seg + gap)
        b = round((i + 1) * seg - gap)
        if horiz:
            _rect(draw, ox + a, oy + c, ox + b, oy + c + t, color)
        else:
            _rect(draw, ox + c, oy + a, ox + t + c, oy + b, color)


def draw_box_char(draw, ox, oy, w, h, s, ch, fg: RGB, bg: RGB) -> bool:
    """Draw U+2500-U+259F in the cell at (ox, oy), w x h pixels, at scale s. Returns False if unknown."""
    code = ord(ch)
    if code in BLOCKS:
        for x0, x1, y0, y1 in BLOCKS[code]:
            # Eighths of the cell; (n*k+4)//8 splits the halves the same way for every block.
            xa, xb = (w * x0 + 4) // 8, (w * x1 + 4) // 8
            ya, yb = (h * y0 + 4) // 8, (h * y1 + 4) // 8
            _rect(draw, ox + xa, oy + ya, ox + xb, oy + yb, fg)
        return True
    if code in SHADES:
        _rect(draw, ox, oy, ox + w, oy + h, blend(fg, bg, SHADES[code]))
        return True
    if code in DASHES:
        _draw_dashes(draw, ox, oy, w, h, s, DASHES[code], fg)
        return True
    if code in ROUNDED:
        _draw_rounded(draw, ox, oy, w, h, s, code, fg)
        return True
    if code in ARMS:
        _draw_lines(draw, ox, oy, w, h, s, ARMS[code], fg)
        return True
    if code in (0x2571, 0x2572, 0x2573):
        if code != 0x2572:
            draw.line([ox, oy + h - 1, ox + w - 1, oy], fill=fg, width=s)
        if code != 0x2571:
            draw.line([ox, oy, ox + w - 1, oy + h - 1], fill=fg, width=s)
        return True
    return False


# ---------------------------------------------------------------- rendering

class _Fonts:
    def __init__(self, opts: Options):
        size = opts.font_size * opts.scale
        self.regular = [ImageFont.truetype(f, size) for f in opts.fonts]
        self.bold = [ImageFont.truetype(f, size) for f in opts.bold_fonts]
        self.missing = [bytes(f.getmask("￿")) for f in self.regular]
        self.cache: Dict[Tuple[bool, str], Tuple[ImageFont.FreeTypeFont, int]] = {}

    def pick(self, ch: str, bold: bool):
        key = (bold, ch)
        if key not in self.cache:
            found = (self.regular[0], 0)
            group = self.bold if bold and len(self.bold) == len(self.regular) else self.regular
            for i, f in enumerate(self.regular):
                if bytes(f.getmask(ch)) != self.missing[i]:
                    found = (group[i], i)
                    break
            self.cache[key] = found
        return self.cache[key]


def _is_wide(sym: str) -> bool:
    return len(sym) >= 1 and unicodedata.east_asian_width(sym[0]) in ("W", "F")


def render_cells(rows: Sequence[Sequence[Cell]], path: Optional[str] = None,
                 opts: Optional[Options] = None) -> Image.Image:
    """Render rows of (symbol, fg, bg, flags) cells; save to `path` when given; return the image.

    Flags: b bold, u underline, r reverse, d dim, s strikethrough. Colours are `#rrggbb`, a name
    or empty. A cell with an empty symbol (pyte's spacer) and the cell after a wide character
    draw no glyph.
    """
    opts = opts or Options()
    s, cw, ch_ = opts.scale, opts.cell_w * opts.scale, opts.cell_h * opts.scale
    pad = opts.pad * s
    cols = max((len(r) for r in rows), default=0)
    img = Image.new("RGB", (cols * cw + 2 * pad, len(rows) * ch_ + 2 * pad), opts.default_bg)
    draw = ImageDraw.Draw(img)
    fonts = _Fonts(opts)
    for y, row in enumerate(rows):
        skip = False
        for x, (sym, fgs, bgs, flags) in enumerate(row):
            fg = resolve_color(fgs, opts.default_fg, opts)
            bg = resolve_color(bgs, opts.default_bg, opts)
            if "r" in flags:
                fg, bg = bg, fg
            if "d" in flags:
                fg = tuple(int(c * 0.6) for c in fg)  # type: ignore[assignment]
            px, py = pad + x * cw, pad + y * ch_
            _rect(draw, px, py, px + cw, py + ch_, bg)
            glyph_skip, skip = skip, False
            if glyph_skip or not sym or not sym.strip():
                pass
            elif is_drawn(sym):
                draw_box_char(draw, px, py, cw, ch_, s, sym, fg, bg)
            else:
                wide = _is_wide(sym)
                skip = wide
                font, idx = fonts.pick(sym[0], "b" in flags)
                tx = px
                if wide or idx:
                    avail = cw * (2 if wide else 1)
                    tx = px + (avail - font.getlength(sym)) / 2
                if wide:
                    _rect(draw, px + cw, py, px + 2 * cw, py + ch_, bg)
                draw.text((tx, py + s), sym, font=font, fill=fg)
            span = 2 * cw if (not glyph_skip and sym and _is_wide(sym)) else cw
            if "u" in flags:
                _rect(draw, px, py + ch_ - 3 * s, px + span, py + ch_ - 2 * s, fg)
            if "s" in flags:
                _rect(draw, px, py + ch_ // 2, px + span, py + ch_ // 2 + s, fg)
    if opts.quantize:
        img = img.quantize(colors=opts.quantize, method=Image.Quantize.MEDIANCUT).convert("RGB")
    if path:
        img.save(path, optimize=True)
    return img


def render_cells_file(src: str, path: str, opts: Optional[Options] = None) -> Image.Image:
    with open(src, encoding="utf-8") as f:
        return render_cells(parse_cells(f.read()), path, opts)


def pyte_cells(screen, opts: Optional[Options] = None) -> List[List[Cell]]:
    """Convert a pyte `Screen` to rows of cells (colours resolved to `#rrggbb` where known)."""
    opts = opts or Options()

    def col(name: str) -> str:
        if name == "default":
            return ""
        c = resolve_color(name, (-1, -1, -1), opts)
        return "" if c[0] < 0 else "#%02x%02x%02x" % c

    rows = []
    for y in range(screen.lines):
        line = screen.buffer[y]
        row = []
        for x in range(screen.columns):
            c = line[x]
            flags = ("b" if c.bold else "") + ("u" if c.underscore else "") + \
                    ("r" if c.reverse else "") + ("s" if getattr(c, "strikethrough", False) else "")
            row.append((c.data, col(c.fg), col(c.bg), flags))
        rows.append(row)
    return rows


def render_pyte(screen, path: Optional[str] = None, opts: Optional[Options] = None,
                **kw) -> Image.Image:
    """Render a pyte `Screen`. Keyword args (scale=2, ...) override fields of `opts`."""
    opts = dataclasses.replace(opts or Options(), **kw)
    return render_cells(pyte_cells(screen, opts), path, opts)


def ansi_to_screen(text: str, cols: int, rows: int):
    """Feed ANSI text (as `tmux capture-pane -e -p` prints it) through pyte at a fixed size."""
    import pyte  # lazy: only the ANSI/tmux/pyte paths need it

    screen = pyte.Screen(cols, rows)
    stream = pyte.Stream(screen)
    text = text.replace("\r\n", "\n")
    if text.endswith("\n"):
        text = text[:-1]
    stream.feed(text.replace("\n", "\r\n"))
    return screen


def tmux_capture(target: str) -> Tuple[str, int, int]:
    size = subprocess.run(["tmux", "display-message", "-p", "-t", target, "#{pane_width}x#{pane_height}"],
                          check=True, capture_output=True, text=True).stdout.strip()
    text = subprocess.run(["tmux", "capture-pane", "-e", "-p", "-t", target],
                          check=True, capture_output=True, text=True).stdout
    c, r = size.split("x")
    return text, int(c), int(r)


def _size(text: str) -> Tuple[int, int]:
    c, r = text.lower().split("x")
    return int(c), int(r)


def main(argv: Optional[Sequence[str]] = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("paths", nargs="+", help="[IN.cells] OUT.png")
    p.add_argument("--ansi", metavar="FILE", help="ANSI text file (needs --size and pyte)")
    p.add_argument("--size", metavar="COLSxROWS", help="screen size for --ansi")
    p.add_argument("--tmux-target", metavar="SESSION:WIN.PANE", help="capture this tmux pane (needs pyte)")
    p.add_argument("--scale", type=int, default=1)
    p.add_argument("--font-size", type=int, default=18)
    p.add_argument("--quantize", type=int, default=0, help="reduce to N colours (smaller PNG)")
    a = p.parse_args(argv)
    opts = Options(scale=a.scale, font_size=a.font_size, quantize=a.quantize)
    out = a.paths[-1]
    if a.tmux_target:
        text, c, r = tmux_capture(a.tmux_target)
        render_pyte(ansi_to_screen(text, c, r), out, opts)
    elif a.ansi:
        if not a.size:
            p.error("--ansi needs --size COLSxROWS")
        c, r = _size(a.size)
        with open(a.ansi, encoding="utf-8") as f:
            render_pyte(ansi_to_screen(f.read(), c, r), out, opts)
    else:
        if len(a.paths) != 2:
            p.error("expected IN.cells OUT.png")
        render_cells_file(a.paths[0], out, opts)
    return 0


if __name__ == "__main__":
    sys.exit(main())
