#!/usr/bin/env python3
"""Style x layout matrix. usage: matrix.py ssh|telnet PORT COLSxROWS nerd|unicode|ascii [color] OUTDIR"""
import json, re, sys, time
sys.path.insert(0, ".")
from lib import *
Q = "."
tr, port, size, tier = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
mono = "mono" in sys.argv[5:6]; color = not mono; out = sys.argv[-1]
cols, rows = map(int, size.split("x"))
name = f"blocks-{tr}-{size}-{tier}{'-mono' if mono else '-colour'}{'-term-' + os.environ['QA_TERM'] if 'QA_TERM' in os.environ else ''}{'-noct' if os.environ.get('QA_CT','1') == '0' else ''}"
os.makedirs(out, exist_ok=True)
if tr == "ssh":
    cmd = f"ssh -tt -i /tmp/qa/keys/founder -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR {'-o SetEnv=COLORTERM=truecolor' if os.environ.get('QA_CT', '1') == '1' else ''} founder@127.0.0.1"
else:
    cmd = f"python3 {Q}/telnetc.py 127.0.0.1 {port} {cols} {rows} xterm-256color /tmp/qa/{name}.wire"
s = Sess(name, cmd, cols, rows, {"TERM": os.environ.get("QA_TERM", "xterm-256color")}, outdir="/tmp/qa/raw")
s.settle(2.5)
if tr == "telnet":
    s.key("founder\r", 1); s.key("" + os.environ["QA_FOUNDER_PASS"] + "\r", 2.5)
s.key(" ", 0.8)
if s.has("Icon check"): s.key("y", 0.8)
s.key(" ", 0.6) if s.has("Any key") else None
s.key("s", 0.6)
def setting(label):
    m = re.search(label + r": (\S+)", s.text()); return m.group(1) if m else None
# tier
s.key("i", 0.5)
for k in {"nerd": ["y"], "unicode": ["n", "y"], "ascii": ["n", "n"]}[tier]: s.key(k, 0.5)
if s.has("Icon check"): s.key("\x1b", 0.4)
want_color = color
cur = setting("Color")
if (cur == "on") != want_color: s.key("c", 0.4)
if not s.has("Terminal settings"): s.key("s", 0.5)
# if the window is 132 wide the layouts/themes list is the same
def cycle(key, label):
    seen = []
    for _ in range(20):
        v = setting(label)
        if v in seen: break
        seen.append(v); s.key(key, 0.3)
    return seen
for _ in range(4):
    if "Borders: blocks" in s.text(): break
    s.key("r", 0.4)
layouts = cycle("l", "Layout"); themes = cycle("t", "Theme")
res = {"layouts": layouts, "themes": themes, "combos": []}
bad = 0
status_rows = {rows - 1, rows - 2}
sigs = {}
for li, L in enumerate(layouts):
    for ti, T in enumerate(themes):
        # now settings screen shows layout L (after loop), step theme
        s.key("t", 0.3)
        lay, th = setting("Layout"), setting("Theme")
        mark = len(s.raw()); rmark = len(s.tal.rev)
        s.key("\x1b", 0.45)
        txt = s.text(); raw = s.raw()[mark:]
        rev = list(s.tal.rev[rmark:]); scr = s.revcells(); rev_all = rev + [(y, x, c, b) for y, x, c, b in scr]
        viol = [r for r in rev_all if r[0] not in status_rows or not r[3]]
        sig = tuple(sorted({(y, min(x for yy, x, *_ in scr if yy == y), max(x for yy, x, *_ in scr if yy == y)) for y in {r[0] for r in scr}}))
        sigs[sig] = sigs.get(sig, 0) + 1
        entry = {"layout": lay, "theme": th, "rev_cells_drawn": len(rev), "rev_cells_on_screen": len(scr), "violations": len(viol), "bytes": len(raw), "marker_on_screen": any(y in status_rows and x == 0 for y, x, *_ in scr)}
        if tier == "ascii":
            entry["nonascii_wire_bytes"] = sum(1 for b in raw if b >= 0x80)
        entry["colour_sgr_in_window"] = len(re.findall(rb"\x1b\[(?:[0-9;]*;)?(?:3[0-7]|4[0-7]|9[0-7]|10[0-7]|38|48)(?:;[0-9;]*)?m", raw))
        if viol: bad += 1; entry["viol"] = [list(v[:3]) for v in viol[:5]]
        if tier != "ascii" or True:
            entry["screen_nonascii_cells"] = sum(1 for ch in txt if ord(ch) >= 0x80) if tier == "ascii" else None
        res["combos"].append(entry)
        open(f"{out}/{name}__{lay}__{th}.txt", "w").write(txt + "\n"); entry["block_chars"] = sum(txt.count(c) for c in "▀▄█▌▐▛▜▙▟▖▗▘▝")
        s.key("s", 0.4)
    s.key("l", 0.3)
res["signatures"] = {str(k): v for k, v in sigs.items()}
idx = [int(m.group(2)) for m in re.finditer(rb"\x1b\[[0-9;]*?(38|48);5;(\d+)", s.raw())]; res["max_palette_index"] = max(idx) if idx else None; res["palette_idx_over_15"] = sum(1 for i in idx if i > 15)
res["total_bytes"] = len(s.raw()); res["stats"] = sgr_stats(s.raw())
res["violations"] = bad
json.dump(res, open(f"{out}/{name}.json", "w"), indent=1)
print(name, "combos", len(res["combos"]), "violations", bad, "sigs", res["signatures"])
s.close()
