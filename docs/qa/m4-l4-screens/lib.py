import os, re, sys, time
sys.path.insert(0, "/home/user/russh-bbs/docs/qa/m4-s3-screens")
from walk import Term, DOWN, UP, LEFT, RIGHT

PORT = 22222
OUT = os.environ.get("OUT", "/tmp/claude-0/walk/out")

def sign_in(user, cols=80, rows=24):
    t = Term(PORT, f"/tmp/claude-0/walk/{'founder' if user == 'founder' else 'bob'}_key", cols, rows, user=user)
    t.pump(2.5)
    for _ in range(3):
        if t.has("Any key"):
            t.send(" ", quiet=1.0)
        if t.has("Do these look like icons"):
            t.send("y", quiet=1.0)
    return t

def open_lh(t):
    t.send("d", quiet=1.0)
    num = "1"
    for line in t.text().splitlines():
        m = re.search(r"\[(\w+)\] Long Haul", line)
        if m:
            num = m.group(1)
    t.send(num, quiet=1.5)
    if t.has("CAPTAIN") or t.has("background") or t.has("Perk:"):
        pass

def snap(t, name):
    t.save(OUT, name)
