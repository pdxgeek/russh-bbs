import sys, re, json; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
PORT = int(sys.argv[1]); user = sys.argv[2]
SCREENS = [("menu", []), ("forums", ["f"]), ("board", ["f", "1"]), ("reader", ["f", "1", "1"]), ("doors", ["d"]), ("coin", ["d", "1"]), ("saltmarch", ["d", "2"]), ("settings", ["s"]), ("widgets", ["s", "g"]), ("chat", ["c"]), ("mail", ["m"]), ("members", ["u"]), ("community", ["y"]), ("oneliners", ["y", "o"]), ("polls", ["y", "p"]), ("help", ["?"])]
res = {}
for (c, r) in ((40, 12), (60, 16), (80, 14), (100, 30), (80, 24)):
    s = ssh_sess(f"asc-{c}x{r}", PORT, user, c, r, tier="ascii")
    for name, seq in SCREENS:
        if name != "menu": keys(s, ["ESC"] * 4, 0.2)
        mark = len(s.raw()); keys(s, seq, 0.5)
        raw = s.raw()[mark:]; na = sorted({ch for ch in s.text() if ord(ch) > 127})
        wire_na = sorted({chr(b) for b in raw if b >= 0x80})  # crude: counts bytes only
        res[f"{c}x{r}/{name}"] = dict(nonascii_screen="".join(na), nonascii_wire_bytes=sum(1 for b in raw if b >= 0x80))
    s.close()
json.dump(res, open(f"/tmp/qa/small/asc-{user}.json", "w"), indent=1)
bad = {k: v for k, v in res.items() if v["nonascii_screen"] or v["nonascii_wire_bytes"]}
print(user, "screens", len(res), "with non-ASCII on screen or wire:", len(bad)); 
for k, v in bad.items(): print(" ", k, repr(v["nonascii_screen"]), v["nonascii_wire_bytes"])
