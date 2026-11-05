"""FIX-M35-1 walk: a staff session over real ssh on a board with telnet on.

Run from docs/qa/m35-screens/tools (it uses lib.py / drive.py there) against a scratch board
(start.sh) that was claimed with claim.py:  python3 ../../fix-m35-1-screens/walk.py COLSxROWS TIER OUTDIR

For every screen it counts the reserved cells (reverse video) two ways: on the final screen
(pyte) and on the raw bytes (every cell drawn or erased while reverse was on, since the key).
It asserts they are all on the last row, start at column 0, are bold and are the marker's
"LOW-SECURITY" label, and writes a PNG with tools/screenshots.
"""
import os, sys
sys.path.insert(0, ".")
sys.path.insert(0, "../../../../tools/screenshots")
from lib import *
from drive import ssh_sess
import shot

cols, rows = map(int, sys.argv[1].split("x"))
tier = sys.argv[2]
out = sys.argv[3]
os.makedirs(out, exist_ok=True)
s = ssh_sess("fixm35", 2722, "founder", cols, rows, tier=tier)
last = rows - 1
fails = []
report = []

def check(name, png=False):
    allrev = s.revcells()
    plain_rev = sum(1 for y, x, c, b in allrev if not b)  # reverse without bold: selection bars, cursors
    scr = [t for t in allrev if t[3]]  # the reserved style is bold AND reverse
    cells = sorted((y, x) for y, x, c, b in scr)
    ys = {y for y, x in cells}
    raw = s.tal.summary()
    raw_rows = {k.split("+")[0] for k in raw if k.endswith("+bold")}
    ok = (bool(cells) and ys == {last} and cells[0][1] == 0
          and all(b for y, x, c, b in scr)
          and (tier != "ascii" or "LOW-SECURITY" in "".join(c for y, x, c, b in scr))
          and raw_rows <= {f"row{last}"})
    report.append(f"{name:26s} final={len(cells):2d} cells rows={sorted(ys)} first_col={cells[0][1] if cells else None}  raw bold+reverse rows={sorted(raw_rows)}  reverse-only cells={plain_rev}  {'OK' if ok else 'FAIL'}")
    if not ok:
        fails.append(name)
        print(s.text())
    if png:
        slug = name.replace(" ", "-").replace(",", "")
        shot.render_pyte(s.scr, f"{out}/{cols}x{rows}-{tier}-{slug}.png", scale=1, quantize=48)
    s.tal.rev.clear()

def step(k, wait=0.7):
    s.tal.rev.clear()
    s.key(k, wait)

def to_menu():
    for _ in range(8):
        if s.has("Main menu"): return
        s.key("\x1b", 0.5)
    raise SystemExit("could not get back to the menu:\n" + s.text())

s.tal.rev.clear()
check("menu", png=True)
# A board and a thread, once, for the lists, editor and reader.
if not s.has("General"):
    for k in ["f", "n"]: step(k)
    for k in ["General\r", "Talk about anything\r", "n"]: step(k)
step("1"); check("forums threads", png=True)
if not s.has("Welcome"):
    step("w"); step("Welcome\r"); check("forums editor", png=True)
    step("Hello everyone.\rSecond line."); step("\x18", 1.0)
else:
    step("1", 1.0)
check("forums reader", png=True)
to_menu()
for name, seq in [("mail inbox", ["m"]), ("mail compose", ["m", "c"]), ("chat rooms", ["c"]),
                  ("chat room", ["c", "\r"]), ("members menu", ["u"]), ("community", ["y"]),
                  ("invites", ["i"]), ("settings", ["s"]), ("sysop tools", ["o"]),
                  ("door list", ["d"])]:
    for k in seq: step(k)
    check(name, png=name in ("mail inbox", "chat room", "members menu", "door list", "mail compose"))
    to_menu()
for n in range(1, 7):
    step("d"); step(str(n), 1.0)
    check(f"door {n}", png=True)
    step("1", 0.6); check(f"door {n} after a key")
    for _ in range(6):
        if s.has("Main menu"): break
        s.key("q", 0.4)
        if s.has("Main menu"): break
        s.key("\x1b", 0.5)
    to_menu()
s.close()
print("\n".join(report))
print("FAILED:", fails if fails else "none")
sys.exit(1 if fails else 0)
