#!/usr/bin/env python3
"""Walk Saltmarch over a real OpenSSH client and capture every screen as text and as a picture.

    python3 walk.py claim  PORT KEY TOKEN
    python3 walk.py play   PORT KEY OUT_DIR [COLSxROWS]
    python3 walk.py report PORT KEY OUT_DIR [COLSxROWS]

`claim` becomes the founder on a fresh board (`ssh claim@host`). `play` signs in as the founder
(`ssh -tt`, a pty of the given size), opens Doors and Saltmarch, founds a clan, and presses the
keys that reach each screen: the hold, the crew, the market, the quay, the chart, the Tidecourt
and its rooms, the planner, a march (arrows walk), and whatever scene or skirmish the marsh
brings. `report` signs in again after the day has changed (run the server under `faketime`) and
captures the morning report. After each stop it writes `NAME.txt` (what a terminal emulator,
`pyte`, shows) and `NAME.cells` (the same screen with the colours the server sent, for
`render.py`). The key script and the emulator are not part of the board; this file is here so
the walk can be repeated.
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


def open_saltmarch(term):
    term.pump(2.0)
    if term.has("Signed in as") and term.has("Any key"):
        term.send(" ")
    if term.has("Icon check"):
        term.send("y")
    term.send("d")
    number = "2"
    for line in term.text().splitlines():
        m = re.search(r"\[(\d+)\] Saltmarch", line)
        if m:
            number = m.group(1)
    term.send(number, quiet=1.0)


def back_home(term):
    for _ in range(3):
        if term.has("The hold of") and term.has("Orders"):
            return
        term.send("\x1b")


DOWN, RIGHT, LEFT, UP = "\x1b[B", "\x1b[C", "\x1b[D", "\x1b[A"


def play(port, key, out, cols, rows):
    term = Term(port, key, cols, rows)
    open_saltmarch(term)
    term.save(out, "01-naming")
    term.send("\r", quiet=1.0)  # found the clan, named for the founder
    term.save(out, "02-home")

    # The rooms off the home screen, by the letter the screen shows for each.
    for name, label in [
        ("03-hold", "The hold: build"),
        ("04-crew", "The crew and the gate"),
        ("06-market", "The Tidecourt market"),
        ("07-quay", "Today's tide table"),
        ("08-chart", "The marsh chart"),
    ]:
        back_home(term)
        if not term.press(re.escape(label)):
            print("no entry for", label)
            continue
        if name == "04-crew":
            term.save(out, name)
            term.send(DOWN)  # the next crew member
            name = "05-crew-detail"
        if name == "08-chart":
            term.send(RIGHT)
            term.send(RIGHT)
        term.save(out, name)
    back_home(term)
    term.press(re.escape("The Tidecourt: league"))
    term.save(out, "09-tidecourt")
    for name, label in [
        ("10-league", "The league table"),
        ("11-hall", "The Hall of Tides"),
        ("12-post", "The post: letters"),
    ]:
        term.press(re.escape(label))
        term.save(out, name)
        term.send("\x1b")

    # Chart the ground round the hold, so there are places to aim for.
    back_home(term)
    term.send("h")  # "Chart the marsh around the hold (2 turns)": the 8th order, below the fold
    term.send("h")
    # The planner: two crew, the standard kit, the nearest charted place.
    back_home(term)
    term.press(re.escape("Send a party out"))
    term.send("a")  # the first crew member (before the set-out order appears)
    term.send("c")  # the second (the orders have moved down one)
    m = re.search(r"([a-z]) +[◆ ]?[A-Z][^│]*? (\d+)t\s*│", term.text())
    if m:  # the first charted place the party can reach today
        term.send(m.group(1))
    term.save(out, "13-planner")
    term.press("Set out with")
    term.save(out, "14-marsh")

    # The marsh: arrows walk (a few steps, to show it), then strike out in each direction in
    # turn until a scene and a skirmish have both come up, or the trip is over.
    met = set()
    for k in [RIGHT, DOWN]:
        term.send(k)
    term.save(out, "14b-marsh-walking")
    legs = ["north", "east", "south", "west"]
    for leg in range(40):
        if term.has("What do you do?"):
            if "scene" not in met:
                met.add("scene")
                term.save(out, "15-scene")
                # Take the way that starts a fight, if the scene offers one: a skirmish is rarer
                # than a scene, and the screen says which choice can start one.
                term.send(term.letter(r"[^│\n]*\((?:a bad turn )?starts a fight") or "a", quiet=1.0)
                term.save(out, "15b-scene-after")
            else:
                term.send(term.letter(r"[^│\n]*\((?:a bad turn )?starts a fight") or "a", quiet=1.0)
            continue
        if term.has("SKIRMISH"):
            if "fight" not in met:
                met.add("fight")
                term.save(out, "16-skirmish")
                term.press("quick-resolve") or term.send("a")
                term.save(out, "16b-skirmish-after")
            else:
                term.press("quick-resolve") or term.send("a")
            continue
        if not term.has("The marsh"):
            break
        if len(met) == 2:
            break
        term.press("Strike out " + legs[leg % 4]) or term.send([RIGHT, DOWN, LEFT, UP][leg % 4])
    term.save(out, "17-marsh-later")
    # Home again.
    for _ in range(8):
        if term.has("The hold of"):
            break
        if term.has("What do you do?") or term.has("SKIRMISH"):
            term.press("quick-resolve") or term.send("a", quiet=1.0)
            continue
        term.press("March home") or term.press("Stand down")
    term.save(out, "18-home-after")
    print("sgr", term.sgr_counts(), "met", sorted(met))
    term.close()


def report(port, key, out, cols, rows):
    term = Term(port, key, cols, rows)
    open_saltmarch(term)
    term.save(out, "19-morning-report")
    term.send(" ", quiet=1.0)
    term.save(out, "20-home-next-morning")
    term.close()


def main():
    cmd = sys.argv[1]
    if cmd == "claim":
        claim(int(sys.argv[2]), sys.argv[3], sys.argv[4])
        return
    port, key, out = int(sys.argv[2]), sys.argv[3], sys.argv[4]
    size = sys.argv[5] if len(sys.argv) > 5 else "80x24"
    cols, rows = (int(v) for v in size.split("x"))
    if cmd == "play":
        play(port, key, out, cols, rows)
    elif cmd == "report":
        report(port, key, out, cols, rows)


if __name__ == "__main__":
    main()
