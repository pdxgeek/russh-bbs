import sys, re
sys.path.insert(0, "/tmp/qa")
from lib import *
from drive import *
port = sys.argv[1]
c = ssh_sess("hos2", port, "carol", 80, 24, tier="unicode")
c.key("d", 0.6); c.key("2", 1.0)
print("naming:", c.has("Clan name"))
cands = ["sysop", "Sysop", "SYSOP", "ѕуѕор", "Ｓysop", "admin ", "a​b", "a‮b", "ab", "◉ ok", "[ok]", "x\x07y", "<b>", "a\tb", "éclair", "Clań", "😀", "-hyphen-", "  ", "A" * 40, "LOW-SECURITY", "UNENCRYPTED", "〚ok〛"]
for cand in cands:
    c.key("\x7f" * 45, 0.25)
    c.key(cand, 0.4)
    t = c.text()
    field = re.search(r"Clan name: (.*?)▏", t)
    c.key("\r", 0.7)
    t2 = c.text()
    msg = [l.strip("│ ").strip() for l in t2.splitlines() if "allowed" in l or "not " in l.lower() or "must" in l.lower() or "reserved" in l.lower() or "taken" in l.lower()]
    named = not c.has("Name your clan")
    print(repr(cand), "-> field:", repr(field.group(1) if field else None), "| accepted:", named, "|", msg[:1])
    if named: break
raw = c.raw()
print("bold-rev outside status:", [r for r in c.tal.rev if r[3] and r[0] < 22][:3])
c.close()
