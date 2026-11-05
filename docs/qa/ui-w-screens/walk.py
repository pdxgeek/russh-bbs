#!/usr/bin/env python3
"""Walk the UI-W dock over a real OpenSSH client, with mouse reporting on, and capture it.

    python3 walk.py claim   PORT KEY TOKEN
    python3 walk.py capture PORT KEY OUT [COLSxROWS]

`claim` becomes the founder on a fresh board. `capture` signs in as the founder in a pty of the
given size (`ssh -tt`, TERM=xterm-256color, COLORTERM=truecolor), turns the mouse on in the
settings, then: opens chat beside the menu (`Ctrl-T`), joins the lobby from the pane, moves the
keys (`Ctrl-W`), opens the forums with the chat still beside them, says a line, drags the
divider with SGR mouse reports (press, motion with the button held, release) and resizes it with
`<` `>`, asserting where the divider is each time and that the status row is unchanged. Every
stop is written as `NAME.txt` and a PNG made by `tools/screenshots/shot.py`. Reuses the M4-S3
terminal helper. The key script and the emulator (`pyte`) are not part of the board.
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "m4-s3-screens"))
sys.path.insert(0, os.path.join(HERE, "..", "..", "..", "tools", "screenshots"))
from walk import Term, claim  # noqa: E402
import shot  # noqa: E402

GRIP = "┃"  # the divider's grip (the ASCII tier draws '#')


def divider(term):
    """(column, grip row) of the dock's divider as the emulator shows it, or None."""
    for y in range(1, term.rows - 1):
        row = term.screen.display[y]
        for x, ch in enumerate(row):
            if ch == GRIP and all(
                term.screen.display[r][x] in "│┃" for r in range(2, term.rows - 5)
            ):
                return x, y
    return None


def status_row(term):
    return term.screen.display[term.rows - 1].rstrip()


def snap(term, out, name):
    os.makedirs(out, exist_ok=True)
    term.save(out, name)
    os.remove(f"{out}/{name}.cells")
    shot.render_pyte(term.screen, f"{out}/{name}.png", scale=1)


def mouse(term, kind, col, row):
    """One SGR-1006 report: kind is press, drag or release (left button); col and row 0-based."""
    code, final = {"press": (0, "M"), "drag": (32, "M"), "release": (0, "m")}[kind]
    term.send(f"\x1b[<{code};{col + 1};{row + 1}{final}", quiet=0.35)


def to_menu(term):
    term.pump(2.5)
    if term.has("Any key") or term.has("Signed in as"):
        term.send(" ", quiet=0.8)
    if term.has("Icon check"):
        term.send("y", quiet=0.8)
    term.pump(0.8)


def capture(port, key, out, size):
    cols, rows = size
    term = Term(port, key, cols, rows)
    to_menu(term)
    # The mouse is the member's choice; `o` in the settings turns it on.
    term.send("s", quiet=0.6)
    if "Mouse: off" in term.text() or "Mouse:  off" in term.text():
        term.send("o", quiet=0.6)
    term.send("\x1b", quiet=0.6)
    assert b"\x1b[?1002h" in bytes(term.raw), "the client was never asked for button motion"
    snap(term, out, "01-menu")
    before = status_row(term)

    term.send("\x14", quiet=0.9)  # Ctrl-T
    snap(term, out, "02-chat-beside-menu")
    assert term.has("Chat rooms"), "no chat pane after Ctrl-T"
    assert status_row(term).startswith(before[:8]), (before, status_row(term))
    term.send("\r", quiet=0.9)  # join the lobby from the pane
    snap(term, out, "03-joined-in-the-pane")

    term.send("\x17", quiet=0.5)  # Ctrl-W: the keys go to the screen
    term.send("f", quiet=1.0)
    snap(term, out, "04-forums-beside-chat")
    assert term.has("#lobby"), "the pane went away with the screen"

    term.send("\x17", quiet=0.5)  # and back to the pane
    term.send("hello from the pane\r", quiet=0.9)
    snap(term, out, "05-a-line-in-the-pane")
    assert term.has("hello from the pane")

    # Drag the divider with the mouse: press on it, move with the button held, release.
    start = divider(term)
    assert start, term.text()
    x0, y0 = start
    mouse(term, "press", x0, y0)
    mouse(term, "drag", x0 - 4, y0)
    mouse(term, "drag", x0 - 9, y0)
    mouse(term, "release", x0 - 9, y0)
    snap(term, out, "06-after-the-drag")
    moved = divider(term)
    assert moved and moved[0] < x0, (start, moved)
    print("divider", x0, "->", moved[0], "after a drag of 9")

    # Far past the left edge: the pane stops at its widest and the screen keeps 40 columns.
    mouse(term, "press", moved[0], moved[1])
    mouse(term, "drag", 0, moved[1])
    mouse(term, "release", 0, moved[1])
    widest = divider(term)
    assert widest and widest[0] >= 40, widest
    snap(term, out, "07-dragged-to-the-left-edge")

    # By keys: the screen has the keys, `>` narrows the pane one column at a time.
    term.send("\x17", quiet=0.5)
    for _ in range(6):
        term.send(">", quiet=0.2)
    term.pump(0.6)
    keyed = divider(term)
    assert keyed and keyed[0] == widest[0] + 6, (widest, keyed)
    snap(term, out, "08-after-six-keys")

    # Back to the menu (the screen has the keys), then close the pane: Ctrl-T leaves the room
    # and the screen has its whole width again.
    term.send("\x1b", quiet=0.8)
    term.send("\x14", quiet=0.8)
    assert divider(term) is None
    assert status_row(term).startswith(before[:8])
    snap(term, out, "09-closed")
    term.send("\x03", quiet=0.5)
    term.close()


if __name__ == "__main__":
    mode = sys.argv[1]
    if mode == "claim":
        claim(int(sys.argv[2]), sys.argv[3], sys.argv[4])
    elif mode == "capture":
        size = tuple(int(n) for n in (sys.argv[5] if len(sys.argv) > 5 else "80x24").split("x"))
        capture(int(sys.argv[2]), sys.argv[3], sys.argv[4], size)
