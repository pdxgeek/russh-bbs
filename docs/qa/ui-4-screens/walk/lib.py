import os, sys, time
S="/tmp/claude-0/-home-user-russh-bbs/bdc9f6bb-ee06-5437-83d4-9e11c0ff9699/scratchpad"
sys.path.insert(0, S+"/pylib")
sys.path.insert(0, "/home/user/russh-bbs/docs/qa/m4-s3-screens")
from walk import Term
PORT=2290
W=S+"/walk"
OUT="/home/user/russh-bbs/docs/qa/ui-4-screens"
SEND="\x18"
def key(u): return f"{W}/{u}"
def term(user, cols=132, rows=43, keyname=None):
    t=Term(PORT, key(keyname or user), cols, rows, user=user); t.pump(2.5); return t
def line(t, text, quiet=0.7): t.send(text+"\r", quiet=quiet)
def show(t, label=""):
    print("=== "+label); print(t.text())
def join(handle):
    t=term("new", 100, 30, keyname=handle)
    t.send("n", quiet=0.8)
    return t

def settle(t, needle=None, tries=6):
    for _ in range(tries):
        t.pump(0.6, 3.0)
        if needle is None or t.has(needle): return True
    return False

def set_choice(t, what, name, pressed):
    """Open Terminal settings, press `pressed` until the screen says `what: name`, come back."""
    t.pump(0.5, 2.0)
    t.send("s", quiet=1.0)
    if not t.has("Next layout"):
        t.send("s", quiet=1.0)
    for _ in range(16):
        if f"{what}: {name}" in t.text(): break
        t.send(pressed, quiet=0.5)
    ok = f"{what}: {name}" in t.text()
    t.send("\x1b", quiet=1.0)
    return ok

def snap(t, name, out=OUT):
    t.save(out, name)

def at_menu(t):
    for _ in range(4):
        if t.has("Main menu") or t.has("Signed in as"): return
        t.send("\x1b", quiet=0.6)
