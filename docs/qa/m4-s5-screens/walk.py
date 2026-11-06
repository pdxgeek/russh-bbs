#!/usr/bin/env python3
"""Walk Saltmarch's S5 additions over a real OpenSSH client with two accounts.

    python3 walk.py A PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]   day 0: boards, a contract, the journal, the toll, a causeway
    python3 walk.py B PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]   day 8: a raid sent, the crew screen shows the party away
    python3 walk.py C PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]   day 9: the night's reports, the league with the new renown

Phase B and C need the board's clock moved on (run the server under `faketime -f "+8d"` and
`"+9d"` on the same database). `founder` and `bob` are two accounts on one board; each is an
ordinary `ssh -tt` session in a pty. The terminal helper is the one from the S3 and S4 walks.
After each stop the walk writes `NAME.txt` (what a terminal emulator, `pyte`, shows), `NAME.cells`
(the colours the server sent) and `NAME.png` (drawn by `tools/screenshots/shot.py`).
"""
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "..", "..", "tools", "screenshots"))
import importlib.util  # noqa: E402


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


_s3 = _load("s3walk", os.path.join(HERE, "..", "m4-s3-screens", "walk.py"))
Term, back_home, open_saltmarch = _s3.Term, _s3.back_home, _s3.open_saltmarch

try:
    import shot  # noqa: E402
except ImportError:  # the renderer is optional
    shot = None


def court(term):
    """Home, then the Tidecourt."""
    back_home(term)
    term.send("g", quiet=0.8)
    return term.has("Where to")


def room(term, label):
    if not term.has("Where to"):
        court(term)
    ok = term.press(re.escape(label))
    if not ok:
        print("no entry for", label)
    return ok


def leave_room(term):
    term.send("\x1b")


def found_or_report(term):
    open_saltmarch(term)
    if term.has("Name your clan") or term.has("Clan name"):
        term.send("\r", quiet=1.0)


def dismiss_report(term):
    if term.has("While you were away"):
        term.send(" ", quiet=1.0)


def turn_to(term, label, key_pattern, limit=14):
    for _ in range(limit):
        if term.has(label):
            return True
        if not term.press(key_pattern):
            return False
    return term.has(label)


def snap(term, out, name):
    term.save(out, name)
    if shot is not None:
        shot.render_cells(shot.parse_cells(open(f"{out}/{name}.cells").read()), f"{out}/{name}.png")


def walkable_window(term):
    """Turn to a window of the water a party can walk (a low), not one that needs a boat."""
    for _ in range(5):
        m = re.search(r"[▸>]\s*(\d\d:\d\d) +(?:low|high)[^\n]*?(needs a boat|on foot)", term.text())
        if m and "boat" not in m.group(2):
            return True
        if not term.press("Next turn of the water"):
            return False
    return False


def phase_a(port, fkey, bkey, out, cols, rows):
    f = Term(port, fkey, cols, rows, user="founder")
    found_or_report(f)
    f.send("\r", quiet=1.0) if f.has("Clan name") else None
    dismiss_report(f)
    court(f)
    snap(f, out, "A01-tidecourt-with-the-boards")
    room(f, "The boards")
    snap(f, out, "A02-boards-standing-and-offers")
    f.press("Take:")
    snap(f, out, "A03-a-contract-taken")
    leave_room(f)
    room(f, "The journal")
    snap(f, out, "A04-journal")
    leave_room(f)
    for _ in range(2):  # the toll goes 2 -> 3 -> 4
        f.press("Causeway toll")
    snap(f, out, "A05-causeway-toll-set")
    back_home(f)
    f.press("Send a party out")
    snap(f, out, "A06-planner")
    f.close()

    b = Term(port, bkey, cols, rows, user="bob")
    found_or_report(b)
    b.send("\r", quiet=1.0) if b.has("Clan name") else None
    dismiss_report(b)
    court(b)
    room(b, "The boards")
    snap(b, out, "A07-bob-boards")
    b.close()


def phase_b(port, fkey, bkey, out, cols, rows):
    f = Term(port, fkey, cols, rows, user="founder")
    found_or_report(f)
    dismiss_report(f)
    back_home(f)
    f.send("h", quiet=0.8)
    court(f)
    room(f, "The gate")
    walkable_window(f)
    f.press("Send the raid on")
    snap(f, out, "B01-raid-sent")
    back_home(f)
    f.press("The crew and the gate")
    snap(f, out, "B02-crew-with-the-party-away")
    f.close()


def phase_c(port, fkey, bkey, out, cols, rows):
    f = Term(port, fkey, cols, rows, user="founder")
    found_or_report(f)
    snap(f, out, "C01-founder-reads-the-night")
    dismiss_report(f)
    court(f)
    room(f, "The league table")
    snap(f, out, "C02-league-with-work-in-renown")
    leave_room(f)
    room(f, "The journal")
    snap(f, out, "C03-journal-after")
    f.close()
    b = Term(port, bkey, cols, rows, user="bob")
    found_or_report(b)
    snap(b, out, "C04-bob-reads-the-night")
    b.close()


def main():
    phase, port, fkey, bkey, out = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
    size = sys.argv[6] if len(sys.argv) > 6 else "80x24"
    cols, rows = (int(v) for v in size.split("x"))
    {"A": phase_a, "B": phase_b, "C": phase_c}[phase](port, fkey, bkey, out, cols, rows)


if __name__ == "__main__":
    main()
