# tools/screenshots

One shared terminal-screenshot renderer for docs. Python 3 + Pillow only; `pyte` is imported
lazily and is needed only for ANSI, tmux and pyte input (`pip install --user pyte`; never commit it).
`.cells` dumps render without it.

Why: drawing every cell from a font leaves gaps in `│` (DejaVu Sans Mono's bar is shorter than the
cell). Box-drawing and block characters here are filled rectangles sized to the cell, so lines join.

## From a walk script

```python
import sys; sys.path.insert(0, "tools/screenshots")
import shot

# `term.screen` is the pyte.Screen a `walk.py`-style pty walk feeds (see docs/qa/m4-s3-screens/walk.py)
term.pump()
shot.render_pyte(term.screen, "docs/qa/my-slice/hold.png", scale=2)   # extra kwargs override Options
shot.render_cells(shot.parse_cells(open("hold.cells").read()), "hold.png")
```

## CLI

```
python3 tools/screenshots/shot.py hold.cells hold.png --scale 2
python3 tools/screenshots/shot.py --ansi screen.ans --size 100x30 out.png
python3 tools/screenshots/shot.py --tmux-target bbs:0.0 out.png     # tmux capture-pane -e -p
```

Other flags: `--font-size N` (18), `--quantize N` (reduce to N colours for smaller PNGs).

## Geometry

Cell 11x22 px, font size 18, 8 px padding, all times `--scale`. Light lines are `scale` px thick,
heavy 3x, double are two light strokes with a 3x gap. Defaults (fg, bg, the 16-colour ratatui and
pyte tables, fonts, sizes) are fields of `shot.Options` and can be overridden.

## Drawn vs font

- Drawn: U+2500-U+257F light/heavy/double lines, every junction and corner, dashes (as real dashes),
  rounded corners (arc), diagonals; U+2580-U+259F half/eighth/quarter blocks and quadrants, and
  `░▒▓` as 25/50/75% blends of fg over bg.
- Font (fallback DejaVu Sans Mono, DejaVu Sans, FreeMono): everything else, including braille and
  symbols. Characters no font has show as tofu (no CJK font is configured). Bold uses the bold
  face; underline, strikethrough, reverse and dim are drawn. Wide (East Asian W/F) characters span
  two cells and the spacer cell is skipped.

Tests: `python3 tools/screenshots/test_shot.py`.
