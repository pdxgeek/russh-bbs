#!/usr/bin/env python3
"""Walk Nullroute's social layer over a real OpenSSH client with two accounts on one board.

    python3 n4walk.py day0  PORT FOUNDER_KEY BOB_KEY OUT   a crew run: found, invite, gather, play, collect
    python3 n4walk.py day1  PORT FOUNDER_KEY BOB_KEY OUT   the next day: leave the crew, post a bounty, read the street

`founder` and `bob` are ordinary `ssh -tt` sessions in a 92x25 pty (three per account on day 0: the
board allows five sign-ins a day). After each stop the walk writes NAME.txt and NAME.cells (what a terminal
emulator, pyte, shows, with the colours the server sent). `day1` needs the board's clock moved on: run the
server under `faketime -f "+1d"` on the same database. Helpers: nr.py (the terminal helper is the S3 walk's).
"""
import sys
from nr import Term, keys, open_door

COLS, ROWS = 92, 25
PATH = "1 3 3"  # gateway -> router -> badge reader -> admin console (the plant target) on the board's day-0 network


def term(port, key, user):
    t = Term(port, key, COLS, ROWS, user=user)
    open_door(t)
    return t


def day0(port, fkey, bkey, out):
    # --- bob's first visit (a crew can invite only a pilot the board has seen): shop, deck
    b = term(port, bkey, "bob")
    b.save(out, "A00-bob-home")
    keys(b, "s"); keys(b, "n"); keys(b, "g"); keys(b, "esc"); keys(b, "d"); keys(b, "e"); keys(b, "f"); keys(b, "esc")
    b.close()

    # --- founder, first visit: shop, deck, found the crew, invite, open the run
    f = term(port, fkey, "founder")
    f.save(out, "A01-founder-home")
    keys(f, "s"); keys(f, "n"); keys(f, "g"); f.save(out, "A02-market-buy-an-implant")
    keys(f, "esc"); keys(f, "d"); keys(f, "e"); keys(f, "f"); f.save(out, "A03-deck-implant-for-siphon")
    keys(f, "esc")
    keys(f, "k"); f.save(out, "B01-crew-screen-no-crew")
    keys(f, "n"); f.send("NightShift", quiet=0.4); f.save(out, "B02-naming-the-crew")
    keys(f, "enter"); f.save(out, "B03-crew-founded")
    keys(f, "i"); f.send("bob", quiet=0.4); keys(f, "enter"); f.save(out, "B04-bob-invited")
    keys(f, "esc"); keys(f, "c"); f.save(out, "B05-contracts")
    keys(f, "enter"); f.save(out, "B06-brief-gather-key")
    keys(f, "g"); f.save(out, "B07-run-opened")
    f.close()

    # --- bob: accept the invitation, join the run
    b = term(port, bkey, "bob")
    keys(b, "k"); b.save(out, "B08-bob-sees-the-invitation")
    keys(b, "1"); b.save(out, "B09-bob-joins-the-crew")
    keys(b, "j"); b.save(out, "B10-bob-joins-the-run")
    b.close()

    # --- founder: start the run, jack in, walk to the target, drop the line without jacking out
    f = term(port, fkey, "founder")
    keys(f, "k"); keys(f, "s"); f.save(out, "B11-run-started")
    keys(f, "e"); f.save(out, "B12-founder-jacks-in")
    for k in PATH.split(" "):
        keys(f, k)
    f.save(out, "B13-founder-at-the-target")
    f.close()

    # --- bob: the founder is on his map; he follows and plants
    b = term(port, bkey, "bob")
    keys(b, "k"); keys(b, "e"); b.save(out, "B14-bob-sees-the-founder-on-the-map")
    for k in PATH.split(" "):
        keys(b, k)
    b.save(out, "B15-bob-at-the-target")
    keys(b, "d"); b.save(out, "B16-bob-plants")
    keys(b, "o"); b.save(out, "B17-bob-jacks-out")
    b.close()

    # --- founder: back in, out; collect the debrief with the crew's split
    f = term(port, fkey, "founder")
    keys(f, "k"); keys(f, "e"); f.save(out, "B18-founder-sees-the-job-done")
    keys(f, "o"); f.save(out, "B19-founder-jacks-out-run-over")
    keys(f, "c"); f.save(out, "B20-founder-debrief-crew-split")
    keys(f, "enter"); f.save(out, "B21-founder-home-after")
    f.close()
    b = term(port, bkey, "bob")
    keys(b, "k"); keys(b, "c"); b.save(out, "B22-bob-debrief-crew-split")
    keys(b, "enter"); b.save(out, "B23-bob-home-after")
    b.close()


def go(t, moves, out=None, tag=None):
    """Press the moves; if a countermeasure stops the way, hit it with the dictionary (b) until it is gone."""
    n = 0
    for m in moves.split(" "):
        keys(t, m)
        shots = 0
        while t.has("COUNTERMEASURE") and shots < 8:
            if out and tag and shots == 0:
                t.save(out, f"{tag}-the-lookout-stands-in-the-way")
            keys(t, "b"); shots += 1
        n += shots
    return n


def day1(port, fkey, bkey, out):
    JOB = "down down"  # the third job on the board: the plant at Brackwater Bio, 'ready'
    PATH = "1 2 4"      # gateway -> terminal -> firewall (a lookout) -> admin console
    f = term(port, fkey, "founder")
    f.save(out, "C01-founder-next-morning")
    keys(f, "c"); keys(f, "down"); keys(f, "down"); f.save(out, "C02-contracts-day-1")
    keys(f, "enter"); keys(f, "g"); f.save(out, "C03-second-run-opened")
    f.close()
    b = term(port, bkey, "bob")
    b.save(out, "C04-bob-next-morning")
    keys(b, "k"); keys(b, "j"); b.save(out, "C05-bob-joins-the-second-run")
    b.close()
    f = term(port, fkey, "founder")
    keys(f, "k"); keys(f, "s"); keys(f, "e")
    go(f, "1 2", out, "C06-founder")
    f.save(out, "C07-founder-past-the-lookout")
    f.close()
    b = term(port, bkey, "bob")
    keys(b, "k"); keys(b, "e")
    go(b, "1 2", out, "C08-bob")
    b.save(out, "C09-bob-follows-the-founder-is-on-the-map")
    keys(b, "4"); b.save(out, "C10-bob-at-the-target-leaves-the-plant-to-the-founder")
    b.close()
    f = term(port, fkey, "founder")
    keys(f, "k"); keys(f, "e"); keys(f, "4"); keys(f, "d"); f.save(out, "C11-founder-plants")
    keys(f, "o"); f.close()
    b = term(port, bkey, "bob")
    keys(b, "k"); keys(b, "e"); keys(b, "o")
    keys(b, "c"); b.save(out, "C12-bob-debrief-crew-split")
    keys(b, "enter"); b.save(out, "C13-bob-home-marks")
    b.close()
    f = term(port, fkey, "founder")
    keys(f, "k"); keys(f, "c"); f.save(out, "C14-founder-debrief-crew-split")
    keys(f, "enter"); f.save(out, "C15-founder-home-marks")
    # the bounty: a crewmate cannot be a target, so leave the crew first
    keys(f, "v"); keys(f, "b"); f.save(out, "C16-bounty-refused-while-crewmates")
    keys(f, "esc"); keys(f, "esc") if f.has("TYPE") else None
    keys(f, "k"); keys(f, "l"); f.save(out, "C17-founder-leaves-the-crew")
    keys(f, "esc")
    keys(f, "v"); f.save(out, "C18-rivals-bob-listed")
    keys(f, "b"); f.send("pay the weir toll", quiet=0.4); f.save(out, "C19-typing-the-bounty-note")
    keys(f, "enter"); f.save(out, "C20-bounty-posted")
    keys(f, "esc"); keys(f, "1") if False else None
    f.close()
    b = term(port, bkey, "bob")
    b.save(out, "C21-bob-home-told-of-the-bounty")
    keys(b, "v"); b.save(out, "C22-bob-rivals")
    keys(b, "esc"); keys(b, "w"); b.save(out, "C23-bob-street")
    keys(b, "2"); b.save(out, "C24-bob-street-bounties")
    keys(b, "3"); b.save(out, "C25-bob-street-crews")
    b.close()


if __name__ == "__main__":
    cmd, port, fk, bk, out = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
    {"day0": day0, "day1": day1}[cmd](port, fk, bk, out)
