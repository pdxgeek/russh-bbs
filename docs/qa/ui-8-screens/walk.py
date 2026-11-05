#!/usr/bin/env python3
"""Walk the UI-8 effects over a real OpenSSH client and capture them as timed frames.

    python3 walk.py claim   PORT KEY TOKEN
    python3 walk.py capture PORT KEY OUT

`claim` becomes the founder on a fresh board. `capture` signs in as the founder several times
and, each time, records what a terminal emulator (`pyte`) shows every ~110 ms from the moment
the session opens: the intro (first sign-in in plain ASCII with no colour, then after the icon
check at 80x24 and 132x43 with truecolor), a key pressed in the middle of the intro, the menu
wipe, the logoff reveal and a session with motion turned off. For each frame it writes
`NAME/frame-NN.txt` and `.cells` (for `../m4-s3-screens/render.py`) and tallies the bytes the
server sent. Reuses the terminal helper of the M4-S3 walk.
"""
import json
import os
import select
import subprocess
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m4-s3-screens"))
from walk import Term, claim  # noqa: E402

DOWN = "\x1b[B"


def snapshot(term):
    cells = []
    for y in range(term.rows):
        row = term.screen.buffer[y]
        for x in range(term.cols):
            c = row[x]
            flags = ("b" if c.bold else "") + ("r" if c.reverse else "")
            from walk import color  # noqa: E402

            cells.append((c.data or " ", color(c.fg), color(c.bg), flags))
    return term.text(), cells


def record(term, seconds, every=0.11, key_at=None, key=None):
    """Frames for `seconds`, optionally sending `key` at `key_at` seconds."""
    frames, started, next_at, sent = [], time.time(), 0.0, False
    raw_start = len(term.raw)
    while True:
        now = time.time() - started
        if now >= seconds:
            break
        if key is not None and not sent and key_at is not None and now >= key_at:
            os.write(term.fd, key.encode())
            sent = True
        ready, _, _ = select.select([term.fd], [], [], 0.01)
        if ready:
            try:
                data = os.read(term.fd, 65536)
            except OSError:
                break
            if not data:
                break
            term.raw += data
            term.stream.feed(data)
        if now >= next_at:
            text, cells = snapshot(term)
            frames.append((round(now, 2), text, cells))
            next_at += every
    return frames, len(term.raw) - raw_start


def save(out, name, frames):
    folder = os.path.join(out, name)
    os.makedirs(folder, exist_ok=True)
    for i, (t, text, cells) in enumerate(frames):
        open(f"{folder}/frame-{i:02d}.txt", "w", encoding="utf-8").write(f"t={t}s\n{text}\n")
        lines = []
        per_row = len(cells) // max(1, len(text.split("\n")))
        for n, (ch, fg, bg, flags) in enumerate(cells):
            lines.append(f"{ch}\t{fg}\t{bg}\t{flags}")
            if (n + 1) % per_row == 0:
                lines.append("|")
        open(f"{folder}/frame-{i:02d}.cells", "w", encoding="utf-8").write("\n".join(lines) + "\n")


def distinct(frames):
    seen, changes = set(), 0
    for _, text, cells in frames:
        key = (text, tuple(cells))
        if key not in seen:
            changes += 1
        seen.add(key)
    return changes


def sgr(raw):
    import re

    raw = bytes(raw)
    return {
        "bytes": len(raw),
        "truecolor_sgr": len(re.findall(rb"\x1b\[[0-9;]*[34]8;2;", raw)),
        "256_sgr": len(re.findall(rb"\x1b\[[0-9;]*[34]8;5;", raw)),
        "bold_reverse_sgr": len(re.findall(rb"\x1b\[(?:[0-9;]*;)?1;7(?:;[0-9;]*)?m", raw)),
        "sixel_kitty_iterm": len(re.findall(rb"\x1bP[0-9;]*q|\x1b_G|\x1b\]1337;", raw)),
    }


def capture(port, key, out):
    report = {}
    os.environ["COLORTERM"] = "truecolor"

    # 1. The very first sign-in: the icon check is unanswered, so the board draws plain ASCII
    #    with no colour. The intro runs in that tier too.
    term = Term(port, key, 80, 24)
    frames, nbytes = record(term, 2.6)
    save(out, "intro-ascii-80x24", frames)
    report["intro-ascii-80x24"] = {"frames_that_differ": distinct(frames), "sgr": sgr(term.raw)}
    term.send(" ", quiet=0.8)  # any key: on to the icon check
    if term.has("Icon check"):
        term.send("y", quiet=0.8)
    term.pump(0.8)
    term.send("l", quiet=0.8)
    term.close()

    # 2. After the icon check: Nerd Font tier with colour, truecolor client.
    for cols, rows in ((132, 43), (80, 24)):
        term = Term(port, key, cols, rows)
        frames, nbytes = record(term, 2.6)
        name = f"intro-{cols}x{rows}"
        save(out, name, frames)
        report[name] = {"frames_that_differ": distinct(frames), "sgr": sgr(term.raw), "bytes_total": nbytes}
        term.close()

    # 3. A key in the middle of the intro: it ends at once, and the key is handled.
    term = Term(port, key, 132, 43)
    frames, nbytes = record(term, 2.0, key_at=0.55, key=" ")
    save(out, "interrupt-132x43", frames)
    report["interrupt-132x43"] = {
        "last_frame_has_menu": "Main menu" in frames[-1][1] or "Log off" in frames[-1][1],
        "sgr": sgr(term.raw),
    }
    # 4. The menu wipe happened on that key; now the logoff reveal.
    frames, nbytes = record(term, 1.6, key_at=0.1, key="l")
    save(out, "logoff-132x43", frames)
    report["logoff-132x43"] = {
        "final_frame_has_goodbye": any(w in frames[-1][1] for w in ("Goodbye", "goodbye", "Bye", "bye")),
        "frames_that_differ": distinct(frames),
    }
    term.close()

    # 5. Motion off (Terminal settings, `m`): one static frame, no timer, no wipe.
    term = Term(port, key, 132, 43)
    record(term, 0.4)
    term.send(" ", quiet=0.8)  # skip the intro, on to the menu
    term.pump(0.6)
    term.send("s", quiet=0.6)  # Terminal settings
    term.send("m", quiet=0.6)  # motion off
    import re

    settings_text = term.text()
    saved = bool(re.search(r"(?i)motion\W+off", settings_text))
    os.makedirs(os.path.join(out, "motion-off-menu-132x43"), exist_ok=True)
    open(os.path.join(out, "settings-after-m-132x43.txt"), "w", encoding="utf-8").write(settings_text + "\n")
    term.send("\x1b", quiet=0.2)  # back to the menu: this would wipe if motion were on
    frames, nbytes = record(term, 1.2)
    save(out, "motion-off-menu-132x43", frames)
    report["motion-off-menu-132x43"] = {
        "settings_showed_motion_off": saved,
        "frames_that_differ": distinct(frames),
        "bytes_after_the_menu_returned": nbytes,
    }
    term.send("l", quiet=0.5)
    term.close()
    term = Term(port, key, 132, 43)
    frames, nbytes = record(term, 2.6)
    save(out, "motion-off-login-132x43", frames)
    report["motion-off-login-132x43"] = {
        "frames_that_differ": distinct(frames),
        "bytes_total_in_2.6s": nbytes,
        "sgr": sgr(term.raw),
    }
    term.send(" ", quiet=0.5)
    term.send("s", quiet=0.4)
    term.send("m", quiet=0.4)  # put motion back on for the next run
    term.close()
    json.dump(report, open(os.path.join(out, "report.json"), "w"), indent=2)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    mode = sys.argv[1]
    if mode == "claim":
        claim(int(sys.argv[2]), sys.argv[3], sys.argv[4])
    elif mode == "capture":
        capture(int(sys.argv[2]), sys.argv[3], sys.argv[4])
