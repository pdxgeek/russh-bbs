import sys, subprocess, os; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,"."); sys.path.insert(0,"/home/user/russh-bbs/docs/qa/m4-s3-screens")
from lib import *
from drive import *
from walk import color
def dump(s, path):
    lines = []
    for y in range(s.rows):
        row = s.scr.buffer[y]
        for x in range(s.cols):
            c = row[x]; flags = ("b" if c.bold else "") + ("r" if c.reverse else "") + ("u" if c.underscore else "")
            lines.append(f"{c.data or ' '}\t{color(c.fg)}\t{color(c.bg)}\t{flags}")
        lines.append("|")
    open(path, "w", encoding="utf-8").write("\n".join(lines) + "\n")
def png(s, name):
    dump(s, f"/tmp/qa/shots/{name}.cells")
    subprocess.run(["python3", "/home/user/russh-bbs/docs/qa/m4-s3-screens/render.py", f"/tmp/qa/shots/{name}.cells", f"/tmp/qa/shots/{name}.png", str(s.cols), str(s.rows)], check=True)
s = ssh_sess("shot", 2722, "founder", 80, 24, tier="nerd")
# make sure classic layout
s.key("s", 0.5)
for _ in range(9):
    if "Layout: classic" in s.text(): break
    s.key("l", 0.3)
s.key("\x1b", 0.8); png(s, "f1-menu-marker-present")
for name, seq in (("f1-mail-no-marker", ["m"]), ("f1-chat-no-marker", ["c"]), ("f1-doors-no-marker", ["d"]), ("f1-door-saltmarch-no-marker", ["d", "2", "\r"]), ("f1-reader-no-marker", ["f", "1", "1"])):
    keys(s, ["ESC"] * 5, 0.2); keys(s, seq, 0.9); png(s, name)
s.close()
# ASCII-tier scrollbar
a = ssh_sess("shota", 2722, "founder", 80, 14, tier="ascii")
a.key("s", 0.5)
for _ in range(9):
    if "Layout: classic" in a.text(): break
    a.key("l", 0.3)
a.key("\x1b", 0.8); png(a, "f2-ascii-menu-scrollbar-80x14")
a.close()
