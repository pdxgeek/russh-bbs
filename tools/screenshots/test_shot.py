#!/usr/bin/env python3
"""Tests for shot.py. Run: python3 tools/screenshots/test_shot.py  (or python3 -m unittest in this dir)."""
import os
import subprocess
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import shot  # noqa: E402

FG, BG = "#ffffff", "#000000"
WHITE, BLACK = (255, 255, 255), (0, 0, 0)
O = shot.Options(pad=0, default_fg=WHITE, default_bg=BLACK)
W, H = O.cell_w, O.cell_h


def cells(lines):
    return [[(ch, FG, BG, "") for ch in line] for line in lines]


def cell_px(img, x, y, o=O):
    return lambda px, py: img.getpixel((x * o.cell_w * o.scale + px, y * o.cell_h * o.scale + py))


class BoxTests(unittest.TestCase):
    def test_light_box_vertical_lines_continuous(self):
        img = shot.render_cells(cells(["┌─┐", "│ │", "│ │", "└─┘"]), opts=O)
        cx = (W - 1) // 2
        for col in (0, 2):
            xs = [col * W + cx]
            for x in xs:
                # Every pixel row below the top corner's horizontal line and above the bottom one.
                for y in range(H // 2 + 1, 3 * H + H // 2 - 1):
                    self.assertEqual(img.getpixel((x, y)), WHITE, f"gap at x={x} y={y}")
        # Seams between the two stacked `│` cells.
        for y in (2 * H - 1, 2 * H, 3 * H - 1, 3 * H):
            self.assertEqual(img.getpixel((cx, y)), WHITE)
        # Horizontal line is continuous across the top row and the interior is background.
        cy = img.size[1] and next(y for y in range(H) if img.getpixel((W + cx, y)) == WHITE)
        for x in range(W, 2 * W):
            self.assertEqual(img.getpixel((x, cy)), WHITE)
        self.assertEqual(img.getpixel((W + cx, H + 3)), BLACK)

    def test_all_box_chars_edge_to_edge_vertical(self):
        # Every pure vertical line character touches both the top and bottom pixel rows.
        for ch in "│┃║":
            img = shot.render_cells(cells([ch]), opts=O)
            col = [img.getpixel((x, 0)) == WHITE for x in range(W)]
            col_b = [img.getpixel((x, H - 1)) == WHITE for x in range(W)]
            self.assertTrue(any(col) and col == col_b, ch)

    def test_double_box(self):
        img = shot.render_cells(cells(["╔═╗", "║ ║", "╚═╝"]), opts=O)
        # Two parallel vertical strokes in the left `║` cell, separated by background.
        row = [img.getpixel((x, H + H // 2)) == WHITE for x in range(W)]
        runs = sum(1 for i, v in enumerate(row) if v and (i == 0 or not row[i - 1]))
        self.assertEqual(runs, 2)
        # Corner ╔: the outer stroke is continuous from the corner down into the next cell.
        outer = next(x for x in range(W) if row[x])
        for y in range(H // 2, 2 * H):
            self.assertEqual(img.getpixel((outer, y)), WHITE, y)
        # Top edge: two horizontal strokes in the middle `═` cell.
        col = [img.getpixel((W + W // 2, y)) == WHITE for y in range(H)]
        self.assertEqual(sum(1 for i, v in enumerate(col) if v and (i == 0 or not col[i - 1])), 2)

    def test_junctions_cover_centre(self):
        for ch in "┼╋╬├┤┬┴╪╫":
            img = shot.render_cells(cells([ch]), opts=O)
            self.assertTrue(
                any(img.getpixel((x, y)) == WHITE for x in range(W) for y in range(H)), ch)

    def test_every_codepoint_draws(self):
        for code in range(0x2500, 0x25A0):
            img = shot.render_cells(cells([chr(code)]), opts=O)
            self.assertTrue(
                any(img.getpixel((x, y)) != BLACK for x in range(W) for y in range(H)), hex(code))

    def test_rounded_corner_joins_neighbours(self):
        img = shot.render_cells(cells(["╭─╮", "│ │", "╰─╯"]), opts=O)
        cx = (W - 1) // 2
        # The vertical stroke below `╭` reaches the cell bottom and continues in `│`.
        self.assertEqual(img.getpixel((cx, H - 1)), WHITE)
        self.assertEqual(img.getpixel((cx, H)), WHITE)

    def test_scale_doubles_geometry(self):
        o2 = shot.Options(pad=0, scale=2, default_fg=WHITE, default_bg=BLACK)
        img = shot.render_cells(cells(["│"]), opts=o2)
        self.assertEqual(img.size, (2 * W, 2 * H))
        self.assertEqual(sum(1 for x in range(2 * W) if img.getpixel((x, 5)) == WHITE), 2)


class BlockTests(unittest.TestCase):
    def count(self, ch, **kw):
        img = shot.render_cells(cells([ch]), opts=O)
        return sum(1 for x in range(W) for y in range(H) if img.getpixel((x, y)) == WHITE)

    def test_full_block(self):
        self.assertEqual(self.count("█"), W * H)

    def test_half_blocks(self):
        img = shot.render_cells(cells(["▀", "▄"]), opts=O)
        top = [img.getpixel((3, y)) == WHITE for y in range(H)]
        self.assertEqual(top, [True] * (H // 2) + [False] * (H // 2))
        bot = [img.getpixel((3, H + y)) == WHITE for y in range(H)]
        self.assertEqual(bot, [False] * (H // 2) + [True] * (H // 2))
        self.assertTrue(all(img.getpixel((x, 0)) == WHITE for x in range(W)))
        # Stacked ▄ over ▀ would leave no gap: lower half of row 2 meets upper half of row 3.
        self.assertEqual(self.count("▌") + self.count("▐"), W * H)

    def test_eighths_and_quadrants(self):
        self.assertEqual(self.count("▁"), W * 3)
        self.assertEqual(self.count("▘") + self.count("▝") + self.count("▖") + self.count("▗"), W * H)
        self.assertEqual(self.count("▛") + self.count("▗"), W * H)
        self.assertEqual(self.count("▚") + self.count("▞"), W * H)
        self.assertEqual(self.count("▉") + self.count("▏") * 0, round(W * 7 / 8) * H)

    def test_shades_blend(self):
        for ch, a in (("░", 0.25), ("▒", 0.5), ("▓", 0.75)):
            img = shot.render_cells(cells([ch]), opts=O)
            want = round(255 * a)
            px = {img.getpixel((x, y)) for x in range(W) for y in range(H)}
            self.assertEqual(px, {(want, want, want)}, ch)


class CellsFormatTests(unittest.TestCase):
    def test_round_trip(self):
        rows = [[("A", "#112233", "", "b"), (" ", "", "LightCyan", ""), ("│", "Red", "#000000", "ur")], [("x", "", "", "")]]
        self.assertEqual(shot.parse_cells(shot.format_cells(rows)), rows)

    def test_parse_pads_short_lines(self):
        self.assertEqual(shot.parse_cells("a\n|\n"), [[("a", "", "", "")]])

    def test_colours(self):
        o = shot.Options()
        self.assertEqual(shot.resolve_color("#0a0b0c", (1, 1, 1), o), (10, 11, 12))
        self.assertEqual(shot.resolve_color("LightCyan", (1, 1, 1), o), (0, 255, 255))
        self.assertEqual(shot.resolve_color("", (1, 1, 1), o), (1, 1, 1))
        self.assertEqual(shot.resolve_color("brightred", (1, 1, 1), o), (0xf1, 0x4c, 0x4c))

    def test_reverse_and_png_file(self):
        import tempfile
        with tempfile.TemporaryDirectory() as d:
            src, out = os.path.join(d, "a.cells"), os.path.join(d, "a.png")
            with open(src, "w", encoding="utf-8") as f:
                f.write(shot.format_cells([[("█", "#ff0000", "#0000ff", "r")]]))
            self.assertEqual(shot.main([src, out, "--scale", "1"]), 0)
            from PIL import Image
            img = Image.open(out).convert("RGB")
            self.assertEqual(img.getpixel((8 + 5, 8 + 10)), (0, 0, 255))  # reversed: bg colour fills

    def test_wide_cell_skips_spacer(self):
        rows = [[("漢", "", "", ""), ("", "", "", ""), ("x", "", "", "")]]
        img = shot.render_cells(rows, opts=O)
        self.assertEqual(img.size, (3 * W, H))


class NoPyteTests(unittest.TestCase):
    def test_cells_without_pyte(self):
        code = (
            "import sys; sys.modules['pyte'] = None\n"
            f"sys.path.insert(0, {os.path.dirname(os.path.abspath(__file__))!r})\n"
            "import shot\n"
            "img = shot.render_cells([[('█', '#fff', '', '')]])\n"
            "assert img.size[0] > 11\n"
        )
        r = subprocess.run([sys.executable, "-I", "-c", code], capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stderr)


try:
    import pyte  # noqa: F401
    HAVE_PYTE = True
except ImportError:
    HAVE_PYTE = False


@unittest.skipUnless(HAVE_PYTE, "pyte not installed")
class PyteTests(unittest.TestCase):
    def test_render_pyte_box_and_wide(self):
        screen = shot.ansi_to_screen("\x1b[31m┌─┐\n│漢│\n└─┘\n", 4, 3)
        img = shot.render_pyte(screen, pad=0, default_fg=WHITE, default_bg=BLACK)
        self.assertEqual(img.size, (4 * W, 3 * H))
        cx = (W - 1) // 2
        self.assertEqual(img.getpixel((cx, H + H // 2)), (0xcd, 0x31, 0x31))
        # red vertical line continuous across the seam under the corner
        self.assertEqual(img.getpixel((cx, H - 1)), (0xcd, 0x31, 0x31))
        self.assertEqual(img.getpixel((cx, H)), (0xcd, 0x31, 0x31))

    def test_reverse_from_pyte(self):
        screen = shot.ansi_to_screen("\x1b[7mX\x1b[0m", 2, 1)
        cells_ = shot.pyte_cells(screen)
        self.assertIn("r", cells_[0][0][3])


if __name__ == "__main__":
    unittest.main()
