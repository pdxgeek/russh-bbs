#!/usr/bin/env python3
"""Walk Emberdeep's social layer over a real OpenSSH client with two accounts.

    python3 walk.py setup PORT ANN_KEY BOB_KEY TOKEN        claim the board as `founder`; make `bob`
    python3 walk.py party PORT ANN_KEY BOB_KEY OUT [COLSxROWS]
    python3 walk.py seed  DB TODAY                          put a fallen hero and a rescuer in the board
    python3 walk.py rescue PORT ANN_KEY BOB_KEY OUT [COLSxROWS]

`party`: both make a warden; founder invites bob to a company, bob accepts; both step into the
ring and founder calls bob out; the journal and an epitaph; the hall's tabs. `seed` writes a
dead hero (`founder`, fell yesterday) and a rescuer standing in the same room (`bob`, in the
same company) straight into the scratch board's database, as `cargo run -p
russh-bbs-door-emberdeep --example rescue_pair` prints them; `rescue`: bob carries founder home,
founder opens the door and is home. Each stop writes NAME.txt and NAME.png (drawn with
tools/screenshots). The terminal helper is the S3 walk's.
"""
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "m4-s3-screens"))
sys.path.insert(0, os.path.join(HERE, "..", "..", "..", "tools", "screenshots"))
from walk import Term  # noqa: E402
import shot  # noqa: E402

ESC, DOWN, RIGHT, LEFT = "\x1b", "\x1b[B", "\x1b[C", "\x1b[D"


def snap(term, out, name):
    os.makedirs(out, exist_ok=True)
    open(f"{out}/{name}.txt", "w", encoding="utf-8").write(term.text() + "\n")
    shot.render_pyte(term.screen, f"{out}/{name}.png", scale=1)
    print("captured", name)


def size(args):
    if args:
        c, r = args[0].split("x")
        return int(c), int(r)
    return 80, 24


def open_emberdeep(term):
    term.pump(2.0)
    if term.has("Any key"):
        term.send(" ")
    if term.has("Icon check"):
        term.send("y")
    term.send("d", quiet=1.0)
    number = "3"
    for line in term.text().splitlines():
        m = re.search(r"\[(\d+)\] Emberdeep", line)
        if m:
            number = m.group(1)
    term.send(number, quiet=1.0)


def hub(term):
    for _ in range(4):
        if term.has("[d] Descend") or term.has("Descend into"):
            return
        term.send(ESC, quiet=0.5)


def key_for(term, label):
    m = re.search(r"\[([0-9a-z])\]\s+" + re.escape(label), term.text())
    return m.group(1) if m else None


def make_warden(term):
    if term.has("new hero"):
        term.send("1", quiet=0.5)  # the warden
        term.send("a", quiet=0.3)
        term.send("b", quiet=0.3)
        term.send("\r", quiet=0.6)
        for k in "1123":  # four points
            term.send(k, quiet=0.2)
        term.send("\r", quiet=1.0)


def setup(port, ann_key, bob_key, token):
    from walk import claim
    claim(port, ann_key, token)
    t = Term(port, bob_key, 100, 30, user="new")
    t.pump(2.0)
    t.send("n", quiet=1.0)
    for line in ["bob", "a throwaway password for bob", "a throwaway password for bob"]:
        t.send(line + "\r", quiet=1.0)
    print(t.text())
    t.close()


def party(port, ann_key, bob_key, out, cols, rows):
    a = Term(port, ann_key, cols, rows, user="founder")
    open_emberdeep(a)
    snap(a, out, "P01-founder-new-hero")
    make_warden(a)
    snap(a, out, "P02-founder-town")
    a.close()
    b = Term(port, bob_key, cols, rows, user="bob")
    open_emberdeep(b)
    make_warden(b)
    snap(b, out, "P03-bob-town")
    b.close()

    a = Term(port, ann_key, cols, rows, user="founder")
    open_emberdeep(a)
    hub(a)
    a.send("c", quiet=1.0)
    snap(a, out, "P04-founder-company-before")
    k = key_for(a, "Invite bob")
    if k:
        a.send(k, quiet=1.0)
    snap(a, out, "P05-founder-invites-bob")
    a.close()

    b = Term(port, bob_key, cols, rows, user="bob")
    open_emberdeep(b)
    hub(b)
    snap(b, out, "P06-bob-hub-with-an-invitation")
    b.send("c", quiet=1.0)
    snap(b, out, "P07-bob-company-invitation")
    k = key_for(b, "Accept founder")
    if k:
        b.send(k, quiet=1.0)
    snap(b, out, "P08-bob-joined")
    b.send(ESC, quiet=0.5)
    b.send("u", quiet=1.0)
    snap(b, out, "P09-bob-ring-before")
    k = key_for(b, "Step into the ring")
    if k:
        b.send(k, quiet=1.0)
    snap(b, out, "P10-bob-in-the-ring")
    b.close()

    a = Term(port, ann_key, cols, rows, user="founder")
    open_emberdeep(a)
    hub(a)
    snap(a, out, "P11-founder-hub-company")
    a.send("u", quiet=1.0)
    k = key_for(a, "Step into the ring")
    if k:
        a.send(k, quiet=1.0)
    snap(a, out, "P12-founder-ring-with-companion-not-listed")
    a.send(ESC, quiet=0.5)
    a.send("j", quiet=1.0)
    snap(a, out, "P13-journal")
    a.send("e", quiet=0.6)
    a.send("She read the walls to the end.", quiet=0.6)
    snap(a, out, "P14-epitaph-typing")
    a.send("\r", quiet=1.0)
    snap(a, out, "P15-journal-with-epitaph")
    a.send(ESC, quiet=0.5)
    a.send("r", quiet=1.0)
    for i in range(3):
        a.send(RIGHT, quiet=0.4)
    snap(a, out, "P16-hall-companies")
    a.send(RIGHT, quiet=0.4)
    snap(a, out, "P17-hall-ring")
    a.send(RIGHT, quiet=0.4)
    snap(a, out, "P18-hall-donors")
    a.send(RIGHT, quiet=0.4)
    snap(a, out, "P19-hall-chronicle")
    a.close()


def seed(db, today):
    import sqlite3
    out = subprocess.check_output(
        ["cargo", "run", "-q", "-p", "russh-bbs-door-emberdeep", "--example", "rescue_pair",
         "--", str(today), "1", "founder", "bob"], text=True).split()
    fallen, rescuer, world = (bytes.fromhex(h) for h in out)
    con = sqlite3.connect(db)
    ids = dict(con.execute("select handle, id from accounts").fetchall()) if False else None
    rows = con.execute("select id, username from accounts").fetchall()
    ids = {u: i for i, u in rows}
    now = today * 86400 + 3600
    for who, blob in (("founder", fallen), ("bob", rescuer)):
        con.execute(
            "update door_save set data=?, version=3, updated_at=?, revision=revision+1 where door='emberdeep' and account_id=?",
            (blob, now, ids[who]))
    con.execute(
        "update door_state set world=?, world_version=3, world_revision=world_revision+1, last_tick_day=? where door='emberdeep'",
        (world, today))
    con.commit()
    print("seeded", ids)


def rescue(port, ann_key, bob_key, out, cols, rows):
    b = Term(port, bob_key, cols, rows, user="bob")
    open_emberdeep(b)
    snap(b, out, "R01-bob-where-founder-lies")
    k = key_for(b, "Carry founder home")
    print("carry key", k)
    if k:
        b.send(k, quiet=1.0)
    snap(b, out, "R02-bob-carries-founder-home")
    b.close()
    a = Term(port, ann_key, cols, rows, user="founder")
    open_emberdeep(a)
    snap(a, out, "R03-founder-wakes-at-home")
    a.close()


if __name__ == "__main__":
    cmd = sys.argv[1]
    if cmd == "setup":
        setup(int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5])
    elif cmd == "seed":
        seed(sys.argv[2], int(sys.argv[3]))
    else:
        port, ak, bk, out = int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5]
        c, r = size(sys.argv[6:])
        {"party": party, "rescue": rescue}[cmd](port, ak, bk, out, c, r)
