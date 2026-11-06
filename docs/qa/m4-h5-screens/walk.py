#!/usr/bin/env python3
"""Walk Hallpass's term events and the mystery's key over a real OpenSSH client with two accounts.

    python3 walk.py claim PORT FOUNDER_KEY TOKEN
    python3 walk.py join  PORT BOB_KEY                        create the second account (`ssh new@host`)
    python3 walk.py A PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]  day 0: both enrol
    python3 walk.py B PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]  day 17: the morning bell names the house match; the team sheet
    python3 walk.py C PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]  day 18: the scenes the two chose, and what each offers
    python3 walk.py D PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]  day 20: the prepared pupil searches the shed and gets the key

Phases B to D need the board's clock moved on (run the server under `faketime -f "+17d"` and so on
on the same database). Phase D starts from a pupil the test `walk_save` wrote into the database (the
first seven clues found); every key after the sign-in is real. After each stop the walk writes
`NAME.txt` (what a terminal emulator, `pyte`, shows) and `NAME.cells` (the same screen with the
colours the server sent, for `tools/screenshots/shot.py`). The terminal helper and the map-arrow
pathing are the ones from the H3 walk.
"""
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "m4-h3-screens"))
from walk import DOWN, ENTER, ESC, Term, claim, open_hallpass, walk_to  # noqa: E402

PASSWORD = "a throwaway password for the walk\r"


def digit(term, pattern):
    """The key (a digit or letter) the screen shows for the order whose text matches `pattern`."""
    m = re.search(r"(?m)(?:^|[^A-Za-z0-9])([0-9a-z])\s+(?:\u25b8\s+)?" + pattern, term.text())
    return m.group(1) if m else None


def press(term, pattern, quiet=0.7):
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


def leave_board(term):
    for _ in range(3):
        term.send("q", quiet=0.5)
    term.close()


def to_after_school(term):
    """Rest through the first six periods (head down) to reach after school."""
    for _ in range(6):
        term.send("r", quiet=0.5)
        if term.has("Lights out"):
            break


def to_free_period(term):
    for _ in range(9):
        if term.has("Free period") and re.search(r"[\[▐]\s*Free", term.text()):
            return
        term.send("r", quiet=0.5)


def to_the_grounds(term):
    # (Esc on the day screen leaves the door, so no Esc here: `g` opens the map from the day.)
    walk_to(term, "dorm", "grounds")


def talk_menu(term):
    term.send("t", quiet=0.8)


def phase_a(port, fkey, bkey, out, cols, rows):
    bob = enter(port, bkey, cols, rows, "bob")
    bob.save(out, "A01-bob-enrolment")
    enrol(bob, "Teacher's pet")
    bob.save(out, "A02-bob-day")
    leave_board(bob)
    f = enter(port, fkey, cols, rows, "founder")
    enrol(f, "Smooth talker")
    f.save(out, "A03-founder-day")
    leave_board(f)


def phase_b(port, fkey, bkey, out, cols, rows):
    # Day 17. The morning bell names the event; at after school the team sheet is up on the grounds.
    f = enter(port, fkey, cols, rows, "founder")
    f.save(out, "B01-founder-morning-bell-names-the-house-match")
    to_after_school(f)
    to_the_grounds(f)
    f.save(out, "B02-founder-on-the-grounds-after-school")
    talk_menu(f)
    f.save(out, "B03-founder-the-talk-list")
    press(f, "The team sheet")
    f.save(out, "B04-founder-the-team-sheet-her-options")
    # A smooth talker with the cool for it takes the touchline.
    press(f, "Take the touchline")
    f.save(out, "B05-founder-leads-the-singing")
    leave_board(f)
    # bob, a teacher's pet, sees the same sheet and a different set of open jobs.
    b = enter(port, bkey, cols, rows, "bob")
    b.save(out, "B06-bob-morning-bell")
    to_after_school(b)
    to_the_grounds(b)
    talk_menu(b)
    press(b, "The team sheet")
    b.save(out, "B07-bob-the-team-sheet-his-options")
    press(b, "Watch from the stands")
    b.save(out, "B08-bob-watches-from-the-stands")
    leave_board(b)


def phase_c(port, fkey, bkey, out, cols, rows):
    # Day 18. The flags each chose yesterday open a different scene today, and shut the other's.
    f = enter(port, fkey, cols, rows, "founder")
    to_after_school(f)
    to_the_grounds(f)
    talk_menu(f)
    f.save(out, "C01-founder-the-talk-list-day-two")
    press(f, "Leading the singing")
    f.save(out, "C02-founder-leading-the-singing")
    press(f, "Strike up the school song")
    f.save(out, "C03-founder-the-song-and-what-it-did")
    leave_board(f)
    b = enter(port, bkey, cols, rows, "bob")
    to_after_school(b)
    to_the_grounds(b)
    talk_menu(b)
    b.save(out, "C04-bob-the-talk-list-day-two")
    press(b, "In the stands")
    b.save(out, "C05-bob-in-the-stands")
    press(b, "Sit with Mags")
    b.save(out, "C06-bob-keeps-mags-tally")
    leave_board(b)


def phase_d(port, fkey, bkey, out, cols, rows):
    # Day 20: the prepared pupil is in the grounds in a free period, seven clues in, with the tunnel map.
    f = enter(port, fkey, cols, rows, "founder")
    f.save(out, "D01-founder-day-twenty-in-the-grounds")
    f.send("j", quiet=0.8)
    f.save(out, "D02-journal-the-key-step")
    f.send(ESC, quiet=0.4)
    walk_to(f, "grounds", "caretaker_shed")
    f.save(out, "D03-at-the-caretakers-shed")
    ok = press(f, r"Search")
    f.save(out, "D04-search-the-shed")
    f.send("j", quiet=0.8)
    f.save(out, "D05-journal-the-wing-step")
    f.send(ESC, quiet=0.4)
    # The map with the key in hand: the East Wing is on the plan, and it is shut at this hour (it
    # opens only after lights-out, to a pupil who holds the key).
    f.send("g", quiet=0.8)
    f.save(out, "D06-the-map-with-the-key-in-hand")
    f.send(ESC, quiet=0.4)
    leave_board(f)
    print("searched:", ok)


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "claim":
        claim(int(sys.argv[2]), sys.argv[3], sys.argv[4])
    elif cmd == "join":
        join(int(sys.argv[2]), sys.argv[3])
    else:
        port, fkey, bkey, out = int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
        cols, rows = map(int, (sys.argv[6] if len(sys.argv) > 6 else "80x24").split("x"))
        {"A": phase_a, "B": phase_b, "C": phase_c, "D": phase_d}[cmd](port, fkey, bkey, out, cols, rows)
