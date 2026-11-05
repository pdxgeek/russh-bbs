import sys, re, json, os; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
TOK = {"hostile_sgr_7_53": b"\x1b[1;7;53m", "hostile_sgr_7_31_44": b"\x1b[1;7;31;44m", "tc_1_2_3": b"\x1b[38;2;1;2;3m", "bg196": b"\x1b[48;5;196m", "osc": b"\x1b]", "bel": b"\x07", "c1_9b": b"\xc2\x9b", "dcs": b"\x1bP", "apc": b"\x1b_", "1049": b"\x1b[?1049", "mouse": b"\x1b[?1000", "DSR": b"\x1b[6n", "QA": b"QAPWN", "QADCS": b"QADCS", "nul": b"\x00", "del": b"\x7f", "clip": b"]52"}
out = {}
for (c, r) in ((80, 24), (132, 43)):
    for tier in ("ascii",):
        for style in ("phosphor", "neon", "paper", "amber", "deepspace", "ansi", "default"):
            s = Sess(f"art-{c}-{tier}-{style}", f"ssh -tt -i /tmp/qa/keys/founder -p 2822 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o SetEnv=COLORTERM=truecolor founder@127.0.0.1", c, r, {"TERM": "xterm-256color"}, outdir="/tmp/qa/raw")
            s.settle(3.0)  # the animated intro then login with art
            login = s.text()
            if s.has("Icon check") or True:
                s.key(" ", 0.7)
                if s.has("Icon check"): s.key({"nerd": "y", "ascii": "n"}[tier], 0.5); 
                if tier == "ascii" and s.has("Icon check"): s.key("n", 0.5)
            s.key("s", 0.5)
            s.key("i", 0.5)
            for k in {"nerd": ["y"], "ascii": ["n", "n"]}[tier]: s.key(k, 0.5)
            if s.has("Icon check"): s.key("\x1b", 0.4)
            if tier == "ascii" and "Color: off" in s.text(): s.key("c", 0.4)
            for _ in range(12):
                if f"Theme: {style}" in s.text(): break
                s.key("t", 0.3)
            s.key("\x1b", 1.0); menu = s.text()
            raw = s.raw(); toks = {k: raw.count(v) for k, v in TOK.items()}
            rows_rev = [(y, x, ch, b) for y, x, ch, b in s.tal.rev if not (y in (r - 1, r - 2) and b)]
            key = f"{c}x{r}-{tier}-{style}"
            out[key] = dict(art_shown_login=('RESERVED' in login or 'ASCII-ok' in login or 'SELECT' in login), art_shown_menu=('RESERVED' in menu or 'SELECT' in menu or 'ASCII-ok' in menu), toks=toks, rev_outside_status=len(rows_rev), rev_screen=[(y, x) for y, x, *_ in s.revcells()][:4], bytes=len(raw))
            open(f"/tmp/qa/art/{key}-login.txt", "w").write(login); open(f"/tmp/qa/art/{key}-menu.txt", "w").write(menu)
            s.close()
json.dump(out, open("/tmp/qa/art/result-ascii.json", "w"), indent=1)
import glob
bad = {}
for f in sorted(glob.glob("/tmp/qa/art/*ascii*-*.txt")):
    t = open(f).read(); na = sorted({c for c in t if ord(c) > 127})
    if na: bad[os.path.basename(f)] = "".join(na)
print("NONASCII", len(bad), "of", len(glob.glob("/tmp/qa/art/*ascii*-*.txt")), list(bad.items())[:4])
for k, v in out.items(): print(k, v)
