import sys, re, time
sys.path.insert(0, "/tmp/qa")
from lib import *
from drive import *
port = sys.argv[1]; user = sys.argv[2]
a = ssh_sess("cc-a", port, user, 80, 24, tier="unicode")
b = ssh_sess("cc-b", port, user, 80, 24, tier="unicode")
print("both signed in:", a.has("Main menu"), b.has("Main menu"))
for s in (a, b): s.key("d", 0.6); s.key("2", 1.2)
def show(tag, s):
    t = s.text(); m = re.search(r"(\d+) tallies", t); tm = re.search(r"turns .*? (\d+)/30", t)
    print(tag, "| tallies", m.group(1) if m else None, "| turns", tm.group(1) if tm else None, "|", [l.strip("│ ") for l in t.splitlines() if "already" in l.lower() or "another" in l.lower()][:2])
show("A open", a); show("B open", b)
# A: work in the hold (a); B: survey the marsh (h) -- different spends of turns
a.key("a", 0.8); print("A in hold:", a.has("Hold") or a.has("hold"))
a.key("\x1b", 0.5)
b.key("h", 0.8)
show("A after", a); show("B after", b)
a.key("h", 0.8); show("A after h", a)
# leave both, reopen as a third
for s in (a, b):
    for _ in range(4): s.key("\x1b", 0.4)
c = ssh_sess("cc-c", port, user, 80, 24, tier="unicode"); c.key("d", 0.6); c.key("2", 1.2); show("C fresh", c)
for s in (a, b, c): s.close()
