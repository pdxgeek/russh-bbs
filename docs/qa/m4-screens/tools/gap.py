import sys, re
sys.path.insert(0, "/tmp/qa")
from lib import *
from drive import *
port, user, game = sys.argv[1], sys.argv[2], sys.argv[3]
s = ssh_sess("gap", port, user, 80, 24, tier="unicode")
s.key("d", 0.6); s.key(game, 2.0)
print("\n".join(l.rstrip() for l in s.text().splitlines() if l.strip())[:2200])
for i in range(3):
    s.key(" ", 1.0); print("-- key", i); print("\n".join(l.rstrip() for l in s.text().splitlines() if l.strip())[:1400])
s.close()
