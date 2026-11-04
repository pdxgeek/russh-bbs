#!/usr/bin/env python3
"""Walk the Saltmarch social layer over a real OpenSSH client with two accounts.

    python3 walk.py A PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]   day 0: both found clans, trade, write
    python3 walk.py B PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]   day 8: read the post, send a raid
    python3 walk.py C PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]   day 9: read the night's reports

Phase B and C need the board's clock moved on (run the server under `faketime -f "+8d"` and
`"+9d"` on the same database; the new-clan week is over by day 8 and the nightly tick that
settles the raid has run by day 9). `founder` and `bob` are two accounts on one board; each is
an ordinary `ssh -tt` session in a pty. After each stop the walk writes `NAME.txt` (what a
terminal emulator, `pyte`, shows) and `NAME.cells` (the same screen with the colours the server
sent, for `render.py` in `../m4-s3-screens/`). The terminal helper is the one from the S3 walk.
"""
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m4-s3-screens"))
from walk import DOWN, Term, back_home, open_saltmarch  # noqa: E402


def court(term):
    """Home, then the Tidecourt (the seventh order on the home screen, below the fold at 80x24)."""
    back_home(term)
    term.send("g", quiet=0.8)
    return term.has("Where to")


def room(term, label):
    """From the Tidecourt into a room."""
    if not term.has("Where to"):
        court(term)
    ok = term.press(re.escape(label))
    if not ok:
        print("no entry for", label)
    return ok


def leave_room(term):
    term.send("\x1b")


def found_or_report(term):
    """Open Saltmarch; found the clan if it is a first visit; read past the morning report."""
    open_saltmarch(term)
    if term.has("Name your clan") or term.has("Clan name"):
        term.send("\r", quiet=1.0)
    elif term.has("While you were away") or term.has("News from the Tidecourt"):
        pass  # captured by the caller before going on


def dismiss_report(term):
    if term.has("While you were away"):
        term.send(" ", quiet=1.0)


def turn_to(term, label, key_pattern, limit=14):
    """Press the 'next' order until the screen shows `label`."""
    for _ in range(limit):
        if term.has(label):
            return True
        if not term.press(key_pattern):
            return False
    return term.has(label)


def phase_a(port, fkey, bkey, out, cols, rows):
    f = Term(port, fkey, cols, rows, user="founder")
    found_or_report(f)
    f.save(out, "A01-founder-naming-or-home")
    f.send("\r", quiet=1.0) if f.has("Clan name") else None
    court(f)
    room(f, "The exchange")
    turn_to(f, "The exchange: timber", r"Next good")
    f.save(out, "A02-exchange-before-posting")
    f.press(re.escape("Post an ask: 5 timber")) or f.press("Post an ask")
    f.save(out, "A03-exchange-ask-sent")
    f.close()

    b = Term(port, bkey, cols, rows, user="bob")
    found_or_report(b)
    b.send("\r", quiet=1.0) if b.has("Clan name") else None
    court(b)
    room(b, "The exchange")
    turn_to(b, "The exchange: timber", r"Next good")
    b.save(out, "A04-bob-sees-the-ask")
    b.press("Buy 5 timber")
    b.save(out, "A05-bob-buys")
    b.close()

    f = Term(port, fkey, cols, rows, user="founder")
    found_or_report(f)
    dismiss_report(f)
    court(f)
    room(f, "The post")
    f.press("Write a letter")
    f.save(out, "A06-compose-letter")
    f.press("Next words")
    f.press("Add a line of your own")
    f.send("see you at the weir at low water")
    f.save(out, "A07-typing-a-line")
    f.send("\r", quiet=0.8)
    f.save(out, "A08-letter-ready")
    f.press("Send the letter to")
    f.save(out, "A09-letter-sent")
    court(f)
    room(f, "Pacts and diplomacy")
    f.save(out, "A10-pacts")
    f.close()


def walkable_window(term):
    for _ in range(5):
        m = re.search(r">\s*(\d\d:\d\d) +(?:low|high)[^\n]*?(needs a boat|on foot)", term.text())
        if m and "boat" not in m.group(2):
            return True
        if not term.press("Next turn of the water"):
            return False
    return False


def phase_b(port, fkey, bkey, out, cols, rows):
    b = Term(port, bkey, cols, rows, user="bob")
    found_or_report(b)
    b.save(out, "B01-bob-morning-report")
    dismiss_report(b)
    court(b)
    room(b, "The post")
    b.save(out, "B02-bob-reads-the-letter")
    back_home(b)
    b.press("Chart the marsh around")  # a save, so the clan posts its guard again
    b.close()

    f = Term(port, fkey, cols, rows, user="founder")
    found_or_report(f)
    f.save(out, "B03-founder-morning-report")
    dismiss_report(f)
    back_home(f)
    f.press("Chart the marsh around")
    court(f)
    room(f, "The gate")
    f.save(out, "B04-the-gate")
    walkable_window(f)
    f.save(out, "B05-the-gate-on-foot")
    f.press("Send the raid on")
    f.save(out, "B06-raid-sent")
    court(f)
    room(f, "The league table")
    f.save(out, "B07-league")
    f.close()


def phase_c(port, fkey, bkey, out, cols, rows):
    f = Term(port, fkey, cols, rows, user="founder")
    found_or_report(f)
    f.save(out, "C01-founder-reads-the-night")
    dismiss_report(f)
    court(f)
    room(f, "The league table")
    f.save(out, "C02-league")
    leave_room(f)
    room(f, "The Hall of Tides")
    f.save(out, "C03-hall")
    leave_room(f)
    room(f, "The post")
    f.save(out, "C04-post-after-the-raid")
    f.close()

    b = Term(port, bkey, cols, rows, user="bob")
    found_or_report(b)
    b.save(out, "C05-bob-reads-the-night")
    dismiss_report(b)
    court(b)
    room(b, "Pacts and diplomacy")
    b.save(out, "C06-bob-pacts")
    leave_room(b)
    room(b, "The post")
    b.save(out, "C07-bob-post")
    b.close()


def main():
    phase, port, fkey, bkey, out = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
    size = sys.argv[6] if len(sys.argv) > 6 else "80x24"
    cols, rows = (int(v) for v in size.split("x"))
    {"A": phase_a, "B": phase_b, "C": phase_c}[phase](port, fkey, bkey, out, cols, rows)


if __name__ == "__main__":
    main()
