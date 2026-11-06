#!/usr/bin/env python3
"""Walk Hallpass's social layer and the mystery over a real OpenSSH client with two accounts.

    python3 walk.py claim PORT FOUNDER_KEY TOKEN
    python3 walk.py join  PORT BOB_KEY                         create the second account (`ssh new@host`)
    python3 walk.py A PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS] day 0: both enrol, a scheme on the other, delivered, heard
    python3 walk.py B PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS] day 11: a rumour, the election, a search, the opt-out

Phase B needs the board's clock moved on (run the server under `faketime -f "+11d"` on the same
database; the east wing's storyline begins on day 6 and the election opens on day 11). `founder`
and `bob` are two accounts on one board; each is an ordinary `ssh -tt` session in a pty. After
each stop the walk writes `NAME.txt` (what a terminal emulator, `pyte`, shows) and `NAME.cells`
(the same screen with the colours the server sent, for `tools/screenshots/shot.py`). The terminal
helper and the map-arrow pathing are the ones from the H3 walk.
"""
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m4-h3-screens"))
from walk import DOWN, ENTER, ESC, Term, claim, open_hallpass, walk_to  # noqa: E402

PASSWORD = "a throwaway password for the walk\r"


def digit(term, pattern):
    """The key (a digit or letter) the screen shows for the order whose text matches `pattern`."""
    m = re.search(r"(?m)(?:^|[^A-Za-z0-9])([0-9a-z])\s+" + pattern, term.text())
    return m.group(1) if m else None


def press(term, pattern, quiet=0.6):
    k = digit(term, pattern)
    if k:
        term.send(k, quiet=quiet)
    else:
        print("no order matching", pattern)
    return k is not None


def join(port, key):
    t = Term(port, key, 100, 30, user="new")
    t.pump(3.0)
    t.send(" ", quiet=1.0)
    t.send("n", quiet=1.5)
    t.send("bob\r", quiet=1.0)
    t.send(PASSWORD, quiet=1.0)
    t.send(PASSWORD, quiet=2.0)
    for _ in range(4):
        if t.has("Doors") or t.has("Welcome"):
            break
        t.send("\r", quiet=1.0)
    print(t.text())
    t.close()


def enter(port, key, cols, rows, user):
    term = Term(port, key, cols, rows, user=user)
    open_hallpass(term, plain=False)
    return term


def enrol(term, trait_pattern):
    if not term.has("Choose") and not term.has("who you want to be"):
        term.pump(1.0)
    press(term, trait_pattern, quiet=1.0)


def in_day(term):
    """Home screen of the day, whatever is on top (messages, a sub-screen)."""
    for _ in range(4):
        if term.has("Go somewhere") or term.has("Lights out"):
            return
        term.send(ESC)


def leave_board(term):
    for _ in range(3):
        term.send("q", quiet=0.5)
    term.close()


def phase_a(port, fkey, bkey, out, cols, rows):
    # bob walks in first, so the school knows him; he is a teacher's pet (trusted, so he spots a swapped timetable).
    bob = enter(port, bkey, cols, rows, "bob")
    bob.save(out, "A01-bob-enrolment")
    enrol(bob, "Teacher's pet")
    bob.save(out, "A02-bob-day")
    bob.send("o", quiet=0.8)
    bob.save(out, "A03-bob-social-hub")
    leave_board(bob)

    # The founder enrols as a smooth talker and finds bob in Classmates.
    f = enter(port, fkey, cols, rows, "founder")
    enrol(f, "Smooth talker")
    f.save(out, "A04-founder-day")
    f.send("o", quiet=0.8)
    f.save(out, "A05-founder-social-hub")
    f.send("c", quiet=0.8)
    f.save(out, "A06-classmates")
    f.send("1", quiet=0.8)
    f.save(out, "A07-schemes-on-bob")
    # The fake note needs a little bluff; laces need nothing. Take laces if offered, else the first.
    if not press(f, "Squeaky laces"):
        f.send("1", quiet=0.8)
    f.save(out, "A08-scheme-sent")
    leave_board(f)

    # bob comes back: the scheme is delivered at the start of his visit.
    bob = enter(port, bkey, cols, rows, "bob")
    bob.save(out, "A09-bob-morning-after")
    bob.send("o", quiet=0.8)
    bob.save(out, "A10-bob-social-hub-lately")
    leave_board(bob)

    # The founder comes back and hears how it went.
    f = enter(port, fkey, cols, rows, "founder")
    f.save(out, "A11-founder-hears-how-it-went")
    leave_board(f)


def period(term):
    """The period the timetable strip has lit (its name, between the markers)."""
    m = re.search(r"[\u2590\[]\s*([A-Za-z]+)\s*[\u258c\]]", term.text())
    return m.group(1) if m else ""


def go_place(term, here, place_id):
    """Walk the map to `place_id` from `here`; returns the place."""
    return walk_to(term, here, place_id)


def phase_b(port, fkey, bkey, out, cols, rows):
    f = enter(port, fkey, cols, rows, "founder")
    f.save(out, "B01-founder-day-eleven")
    # Slot 0: Sol is in the quad; start a rumour there.
    here = go_place(f, "dorm", "quad")
    f.save(out, "B02-in-the-quad")
    f.send("o", quiet=0.8)
    f.send("r", quiet=0.8)
    f.save(out, "B03-rumours")
    f.send("1", quiet=0.8)
    f.save(out, "B04-rumour-about-whom")
    f.send("1", quiet=1.0)
    f.save(out, "B05-rumour-started")
    # The election is open: stand and canvass.
    f.send("o", quiet=0.8)
    f.send("e", quiet=0.8)
    f.save(out, "B06-election-before-standing")
    press(f, "Stand for election")
    f.save(out, "B07-election-standing")
    press(f, "Canvass the corridors")
    f.save(out, "B08-canvassed")
    # (The east wing's first search, in the library in a free period, was not walked over ssh: see the walk notes.)
    leave_board(f)

    # bob: sees the candidate, then opts out, and the founder cannot reach him any more.
    b = enter(port, bkey, cols, rows, "bob")
    b.save(out, "B13-bob-day-eleven")
    b.send("o", quiet=0.8)
    b.send("e", quiet=0.8)
    b.save(out, "B14-bob-the-ballot")
    in_day(b)
    b.send("o", quiet=0.8)
    press(b, "Opt out")
    b.save(out, "B15-bob-opted-out")
    leave_board(b)

    f = enter(port, fkey, cols, rows, "founder")
    f.send("o", quiet=0.8)
    f.send("c", quiet=0.8)
    f.save(out, "B16-founder-sees-bob-opted-out")
    leave_board(f)


def main():
    cmd = sys.argv[1]
    if cmd == "claim":
        claim(int(sys.argv[2]), sys.argv[3], sys.argv[4])
    elif cmd == "join":
        join(int(sys.argv[2]), sys.argv[3])
    else:
        port, fkey, bkey, out = int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
        size = sys.argv[6] if len(sys.argv) > 6 else "80x24"
        cols, rows = (int(v) for v in size.split("x"))
        {"A": phase_a, "B": phase_b}[cmd](port, fkey, bkey, out, cols, rows)


if __name__ == "__main__":
    main()
