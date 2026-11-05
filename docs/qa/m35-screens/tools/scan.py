import sys; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
cols, rows = map(int, sys.argv[1].split("x")); tier = sys.argv[2]; layout = sys.argv[3] if len(sys.argv) > 3 else None
s = ssh_sess("scan", 2722, "founder", cols, rows, tier=tier)
if layout:
    s.key("s", 0.5)
    for _ in range(9):
        if f"Layout: {layout}" in s.text(): break
        s.key("l", 0.3)
    s.key("\x1b", 0.5)
def mk():
    rc = [(y, x) for y, x, c, b in s.revcells()]
    return "marker@" + ",".join(f"({y},{x})" for y, x in rc[:3]) if rc else "NO MARKER"
def show(n):
    print(f"{n:28s} {mk():22s} last row: {s.text().split(chr(10))[rows-1][:50]!r}")
show("menu")
for name, seq in [("forums","f"),("board1","1"),("thread list ->reader","1"),("reader back","\x1b"),("write (editor)","w"),("editor esc","\x1b"),("esc","\x1b"),("esc2","\x1b"),("mail","m"),("mail compose","c"),("compose esc","\x1b"),("mail esc","\x1b"),("chat","c"),("chat esc","\x1b"),("doors","d"),("door list 1","1"),("door esc","\x1b"),("door 2","2"),("door2 enter","\r"),("door2 esc","\x1b"),("door2 esc2","\x1b"),("door2 q","q"),("members","u"),("members esc","\x1b"),("community","y"),("oneliners","o"),("oneliners esc","\x1b"),("polls","p"),("polls esc","\x1b"),("community esc","\x1b"),("invites","i"),("inv esc","\x1b"),("sysop","o"),("sysop esc","\x1b"),("settings","s"),("widgets","g"),("w esc","\x1b"),("settings esc","\x1b")]:
    s.key(seq, 0.7); show(name)
s.close()
