import sys, re, json
sys.path.insert(0, "/tmp/qa")
from lib import *
from drive import *
port = sys.argv[1]
HOST = "◉[〚]‮​\x07\u009b31mX퟿⁦"  # lookalike marker, brackets, RLO, ZWSP, Nerd PUA, BEL, C1 CSI, isolate
def sess(name, user):
    return ssh_sess(name, port, user, 80, 24, tier="unicode")
c = sess("hos-carol", "carol")
mark = len(c.raw()); c.key("d", 0.6); c.key("2", 1.0)
print("naming screen:", c.has("Clan name"))
c.key("\x7f" * 30, 0.4)
c.key(HOST, 0.8)
print(c.text())
c.key("\r", 1.2)
home = c.text(); print(home)
raw = c.raw()[mark:]
rev = [r for r in c.tal.rev if r[3] and r[0] not in (23, 22)]
print("bold-reverse off status:", rev[:5])
print("raw bad:", {n: raw.count(b) for n, b in {"BEL": b"\x07", "C2 9B": b"\xc2\x9b", "RLO": b"\xe2\x80\xae", "ZWSP": b"\xe2\x80\x8b", "PUA E0B0": b"\xee\x82\xb0", "U+301A": "〚".encode(), "lookalike": "◉".encode(), "isolate": b"\xe2\x81\xa6"}.items()})
c.key("\x1b", 0.5); c.key("\x1b", 0.5)
c.close()
