#!/usr/bin/env python3
"""Walk Saltmarch over a real OpenSSH client and capture every screen as text and as a picture.

    python3 walk.py SERVER_PORT KEY_FILE OUT_DIR [COLSxROWS] [tier] [color]

It signs in as the founder (`ssh -tt`, a pty of the given size), opens Doors, opens Saltmarch,
founds a clan, and presses the keys that reach each screen. After each stop it writes
`NAME.txt` (what a terminal emulator, `pyte`, shows) and `NAME.cells` (the same screen with the
colours the server sent, for `render.py`). The key script and the emulator are not part of the
board; this file is only here so the walk can be repeated.
"""
import fcntl
import os
import pty
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

    def send(self, text, quiet=0.6):
        os.write(self.fd, text.encode() if isinstance(text, str) else text)
        self.pump(quiet)

    def text(self):
        return "\n".join(line.rstrip() for line in self.screen.display)

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

    def close(self):
        try:
            self.proc.terminate()
        except OSError:
            pass


def main():
    port, key, out = int(sys.argv[1]), sys.argv[2], sys.argv[3]
    cols, rows = (int(v) for v in (sys.argv[4] if len(sys.argv) > 4 else "80x24").split("x"))
    term = Term(port, key, cols, rows)
    term.pump(2.0)
    # Main menu, Doors, Saltmarch (the door list is numbered; Saltmarch is the one that says so).
    term.send("d")
    listing = term.text()
    number = "1"
    for line in listing.splitlines():
        if "Saltmarch" in line and "[" in line:
            number = line.split("[")[1].split("]")[0]
    term.send(number)
    stops = [
        ("01-naming", ""),
        ("02-home", "\r"),       # found the clan
        ("03-hold", "a"),        # the hold
        ("04-home-again", "\x1b"),
        ("05-crew", "b"),
        ("06-home-again", "\x1b"),
        ("07-market", "c"),
        ("08-home-again", "\x1b"),
        ("09-planner", "d"),
    ]
    for name, keys in stops:
        if keys:
            term.send(keys)
        term.save(out, name)
    term.close()


if __name__ == "__main__":
    main()
