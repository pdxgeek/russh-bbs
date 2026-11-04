#!/usr/bin/env python3
"""Walk Hallpass over a real OpenSSH client and capture every screen as text and as a picture.

    python3 walk.py claim PORT KEY TOKEN
    python3 walk.py play  PORT KEY OUT_DIR [COLSxROWS] [plain]

`claim` becomes the founder on a fresh board (`ssh claim@host`). `play` signs in as the founder
(`ssh -tt`, a pty of the given size), opens Doors and Hallpass, enrols, and presses the keys that
reach each screen: the day, the map (arrows move the cursor, Enter walks), two lessons, the
scheme planner and its lists, the file, clubs, the journal, the paper, the hall of legend and the
yearbook. With `plain` the icon check is answered for plain ASCII with no colour. After each stop
it writes `NAME.txt` (what a terminal emulator, `pyte`, shows) and `NAME.cells` (the same screen
with the colours the server sent, for `render.py`). The key script and the emulator are not part
of the board; this file is here so the walk can be repeated. It reads the map grid from
`crates/doors/hallpass/src/session/ui/map.rs` to work out which arrow keys reach a place.
"""
import fcntl
import os
import pty
import re
import select
import struct
import subprocess
import sys
import termios
import time

import pyte

NAMES = {
    "black": "#000000", "red": "#cd3131", "green": "#0dbc79", "brown": "#e5e510",
    "blue": "#2472c8", "magenta": "#bc3fbc", "cyan": "#11a8cd", "white": "#e5e5e5",
    "brightblack": "#666666", "brightred": "#f14c4c", "brightgreen": "#23d18b",
    "brightbrown": "#f5f543", "brightblue": "#3b8eea", "brightmagenta": "#d670d6",
    "brightcyan": "#29b8db", "brightwhite": "#ffffff",
}


def color(name):
    if name == "default":
        return ""
    if name in NAMES:
        return NAMES[name]
    if len(name) == 6:
        return "#" + name
    return ""


class Term:
    def __init__(self, port, key, cols, rows, user="founder"):
        self.cols, self.rows = cols, rows
        self.screen = pyte.Screen(cols, rows)
        self.stream = pyte.ByteStream(self.screen)
        self.raw = bytearray()
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        env = dict(os.environ, TERM="xterm-256color")
        self.proc = subprocess.Popen(
            ["ssh", "-tt", "-i", key, "-p", str(port),
             "-o", "StrictHostKeyChecking=no", "-o", "UserKnownHostsFile=/dev/null",
             "-o", "IdentitiesOnly=yes", "-o", "LogLevel=ERROR", f"{user}@127.0.0.1"],
            stdin=slave, stdout=slave, stderr=slave, env=env, close_fds=True,
        )
        os.close(slave)
        self.fd = master

    def pump(self, quiet=0.6, limit=8.0):
        end, last = time.time() + limit, time.time()
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.1)
            if r:
                try:
                    data = os.read(self.fd, 65536)
                except OSError:
                    return
                if not data:
                    return
                self.raw += data
                self.stream.feed(data)
                last = time.time()
            elif time.time() - last > quiet:
                return

    def send(self, text, quiet=0.5):
        os.write(self.fd, text.encode() if isinstance(text, str) else text)
        self.pump(quiet)

    def text(self):
        return "\n".join(line.rstrip() for line in self.screen.display)

    def has(self, needle):
        return needle in self.text()

    def letter(self, pattern):
        """The key the screen shows for the order whose text matches `pattern`."""
        m = re.search(r"(?m)(?:^|[^A-Za-z])([a-z]) " + pattern, self.text())
        return m.group(1) if m else None

    def press(self, pattern):
        k = self.letter(pattern)
        if k:
            self.send(k)
        return k is not None

    def save(self, out, name):
        os.makedirs(out, exist_ok=True)
        open(f"{out}/{name}.txt", "w", encoding="utf-8").write(self.text() + "\n")
        lines = []
        for y in range(self.rows):
            row = self.screen.buffer[y]
            for x in range(self.cols):
                c = row[x]
                fg, bg = color(c.fg), color(c.bg)
                flags = ("b" if c.bold else "") + ("r" if c.reverse else "") + ("u" if c.underscore else "")
                lines.append(f"{c.data or ' '}\t{fg}\t{bg}\t{flags}")
            lines.append("|")
        open(f"{out}/{name}.cells", "w", encoding="utf-8").write("\n".join(lines) + "\n")
        print("captured", name)

    def sgr_counts(self):
        raw = bytes(self.raw)
        return {
            "truecolor": len(re.findall(rb"\x1b\[[0-9;]*[34]8;2;", raw)),
            "256": len(re.findall(rb"\x1b\[[0-9;]*[34]8;5;", raw)),
            "bold+reverse": len(re.findall(rb"\x1b\[(?:[0-9;]*;)?1;7(?:;[0-9;]*)?m", raw)),
        }

    def close(self):
        try:
            self.proc.terminate()
        except OSError:
            pass


def claim(port, key, token):
    term = Term(port, key, 100, 30, user="claim")
    term.pump(2.0)
    term.send(token + "\r", quiet=1.5)
    # The claim also asks for a password (twice) for the founder's account.
    term.send("a throwaway password for the walk\r", quiet=1.0)
    term.send("a throwaway password for the walk\r", quiet=2.0)
    print(term.text())
    term.close()


import math
import pathlib
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[3]
HALLPASS = ROOT / "crates/doors/hallpass"
DOWN, RIGHT, LEFT, UP = "\x1b[B", "\x1b[C", "\x1b[D", "\x1b[A"
ENTER, ESC = "\r", "\x1b"


def grid():
    """(id, col, row) for each place on the plan, in the order the cursor logic scans them."""
    src = (HALLPASS / "src/session/ui/map.rs").read_text()
    block = src[src.index("const GRID"): src.index("];", src.index("const GRID"))]
    return [(m[0], int(m[1]), int(m[2])) for m in re.findall(r'\("(\w+)", (\d+), (\d+), "[^"]*"\)', block)]


def step(places, frm, key):
    """The place the cursor moves to from `frm` on `key` (the same scan as `step_cursor`)."""
    pos = {p: (c * 12, r * 2) for p, c, r in places}
    fx, fy = pos[frm]
    best = None
    for p, _, _ in places:
        if p == frm:
            continue
        x, y = pos[p]
        dx, dy = math.trunc((x - fx) / 2), (y - fy) * 2
        along, across = {
            UP: (-dy, abs(dx)), DOWN: (dy, abs(dx)), LEFT: (-dx, abs(dy)), RIGHT: (dx, abs(dy)),
        }[key]
        if along <= 0:
            continue
        score = along + across * 2
        if best is None or score < best[0]:
            best = (score, p)
    return best[1] if best else None


def path(frm, to):
    """The arrow keys that take the map cursor from `frm` to `to`."""
    places = grid()
    seen, queue = {frm: []}, [frm]
    while queue:
        here = queue.pop(0)
        if here == to:
            return seen[here]
        for key in (UP, DOWN, LEFT, RIGHT):
            nxt = step(places, here, key)
            if nxt and nxt not in seen:
                seen[nxt] = seen[here] + [key]
                queue.append(nxt)
    return None


def names():
    data = tomllib.loads((HALLPASS / "content/places.toml").read_text())
    return {p["name"]: p["id"] for p in data["place"]}


def open_hallpass(term, plain):
    term.pump(2.0)
    if term.has("Signed in as") and term.has("Any key"):
        term.send(" ")
    if term.has("Icon check"):
        if plain:
            term.send("n")
            term.send("n")
        else:
            term.send("y")
    term.send("d")
    number = "3"
    for line in term.text().splitlines():
        m = re.search(r"\[(\d+)\] Hallpass", line)
        if m:
            number = m.group(1)
    term.send(number, quiet=1.0)


def walk_to(term, here, place_id):
    """Open the map, move the cursor to `place_id` with arrows, press Enter; returns the new place."""
    term.send("g")
    keys = path(here, place_id)
    if keys is None:
        print("no path to", place_id)
        term.send(ESC)
        return here
    for k in keys:
        term.send(k, quiet=0.25)
    term.send(ENTER, quiet=0.8)
    return place_id


def home(term):
    """Back to the day screen with Esc, never past it (Esc on the day leaves the door)."""
    for _ in range(5):
        if term.has("q Leave") and term.has("p Schemes"):
            return
        term.send(ESC, quiet=0.4)


def play(port, key, out, cols, rows, plain):
    term = Term(port, key, cols, rows)
    open_hallpass(term, plain)
    term.save(out, "01-enrol")
    term.send("1", quiet=1.0)  # the first trait
    term.save(out, "02-day")
    here = "dorm"

    # The map: the cursor moves with the arrows, and the detail follows it.
    term.send("g")
    term.save(out, "03-map")
    term.send(RIGHT, quiet=0.3)
    term.send(DOWN, quiet=0.3)
    term.save(out, "03b-map-cursor-moved")
    term.send(ESC)

    # First period: to the lesson, and take it.
    subject_room = {"history": "room_history", "maths": "room_maths"}
    for n, (what, label) in enumerate([("history", "04"), ("maths", "07")]):
        here = walk_to(term, here, subject_room[what])
        term.save(out, f"{label}-arrived-{what}")
        if term.press(r"Attend"):
            term.save(out, f"{int(label) + 1:02d}-lesson-{what}")
            for _ in range(3):
                term.send(ENTER, quiet=0.8)
            term.save(out, f"{int(label) + 2:02d}-after-lesson-{what}")
        else:
            print("no lesson to attend for", what)

    # Schemes: start one, and look at each list.
    home(term)
    term.send("p")
    term.save(out, "10-schemes")
    term.send("1")
    term.save(out, "11-target")
    term.send("1")
    term.save(out, "12-method")
    term.send("1")
    term.save(out, "13-planner")
    term.send(RIGHT, quiet=0.3)
    term.save(out, "14-planner-other-phase")
    for key_, name in [("t", "15-twists"), ("y", "16-getaway"), ("d", "17-schedule")]:
        term.send(key_)
        term.save(out, name)
        term.send(ESC)
    home(term)

    # The file, clubs, journal, paper, hall and yearbook.
    for key_, name, more in [
        ("f", "18-file", RIGHT),
        ("c", "20-clubs", DOWN),
        ("j", "22-journal", None),
        ("n", "23-paper", None),
    ]:
        term.send(key_)
        term.save(out, name)
        if more:
            term.send(more, quiet=0.3)
            term.save(out, name + "-next")
        if key_ == "n":
            term.send("h")
            term.save(out, "24-hall")
            term.send(ESC)
            term.send("y")
            term.save(out, "25-yearbook")
        home(term)
    print("sgr", term.sgr_counts())
    term.close()


def main():
    cmd = sys.argv[1]
    if cmd == "claim":
        claim(int(sys.argv[2]), sys.argv[3], sys.argv[4])
        return
    port, key, out = int(sys.argv[2]), sys.argv[3], sys.argv[4]
    size = sys.argv[5] if len(sys.argv) > 5 and "x" in sys.argv[5] else "80x24"
    cols, rows = (int(v) for v in size.split("x"))
    plain = "plain" in sys.argv[5:]
    if cmd == "play":
        play(port, key, out, cols, rows, plain)


if __name__ == "__main__":
    main()
