import sys, re, json, random, time; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
PORT = int(sys.argv[1]); out = {}
def marker(s):
    return [(y, x) for y, x, c, b in s.revcells() if b][:3]
SCREENS = [("menu", []), ("forums", ["f"]), ("board", ["f", "1"]), ("reader", ["f", "1", "1"]), ("doors", ["d"]), ("saltmarch", ["d", "2"]), ("settings", ["s"]), ("widgets", ["s", "g"]), ("chat", ["c"]), ("mail", ["m"]), ("members", ["u"]), ("community", ["y"]), ("sysop", ["o"])]
for tier in ("nerd", "ascii"):
    s = ssh_sess(f"sm-{tier}", PORT, "founder", 40, 12, tier=tier)
    s.settle(1.0)
    res = {}
    for name, seq in SCREENS:
        keys(s, ["ESC", "ESC", "ESC", "ESC"], 0.25) if name != "menu" else None
        keys(s, seq, 0.6)
        t = s.text(); lines = t.split("\n")
        res[name] = dict(marker=marker(s), nonblank_rows=sum(1 for l in lines if l.strip()), maxw=max(len(l) for l in lines), text=t if name in ("menu", "forums", "saltmarch", "settings", "widgets") else "")
        if tier == "ascii": res[name]["nonascii"] = sorted({c for c in t if ord(c) > 127})
        open(f"/tmp/qa/small/{tier}-40x12-{name}.txt", "w").write(t)
    out[tier] = res; s.close()
json.dump(out, open("/tmp/qa/small/result.json", "w"), indent=1)
for tier, res in out.items():
    for n, r in res.items(): print(tier, n, "marker", r["marker"], "rows", r["nonblank_rows"], "nonascii", r.get("nonascii"))
