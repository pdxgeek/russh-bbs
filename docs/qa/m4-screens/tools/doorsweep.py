#!/usr/bin/env python3
"""Door security sweep. usage: doorsweep.py ssh|telnet PORT USER COLSxROWS nerd|unicode|ascii OUT [maxcombos]
For every layout x style: open each door (Coin Toss + the five games), press a fixed key script,
record every bold+reverse / reverse cell drawn (raw-byte Tally) and the status row on each frame."""
import json, re, sys, time
sys.path.insert(0, "/tmp/qa")
from lib import *
tr, port, user, size, tier, out = sys.argv[1:7]
maxc = int(sys.argv[7]) if len(sys.argv) > 7 else 999
cols, rows = map(int, size.split("x"))
pw = os.environ["QA_FOUNDER_PASS"] if user == "founder" else os.environ["QA_PW_PREFIX"] + user
name = f"ds-{tr}-{user}-{size}-{tier}"
os.makedirs(out, exist_ok=True)
if tr == "ssh":
    cmd = f"ssh -tt -i /tmp/qa/keys/{user} -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o SetEnv=COLORTERM=truecolor {user}@127.0.0.1"
else:
    cmd = f"python3 /tmp/qa/telnetc.py 127.0.0.1 {port} {cols} {rows} xterm-256color /tmp/qa/{name}.wire"
s = Sess(name, cmd, cols, rows, {"TERM": "xterm-256color"}, outdir="/tmp/qa/raw")
s.settle(2.5)
if tr == "telnet":
    s.key(user + "\r", 1); s.key(pw + "\r", 2.5)
s.key(" ", 0.8)
if s.has("Icon check"): s.key("y", 0.8)
s.key(" ", 0.6) if s.has("Any key") else None
s.key("s", 0.6)
def setting(label):
    m = re.search(label + r": (\S+)", s.text()); return m.group(1) if m else None
s.key("i", 0.5)
for k in {"nerd": ["y"], "unicode": ["n", "y"], "ascii": ["n", "n"]}[tier]: s.key(k, 0.5)
if s.has("Icon check"): s.key("\x1b", 0.4)
want = "on" if tier != "ascii" else "off"
if f"Color: {want}" not in s.text(): s.key("c", 0.4)
if not s.has("Terminal settings"): s.key("s", 0.5)
def cycle(key, label):
    seen = []
    for _ in range(20):
        v = setting(label)
        if v in seen: break
        seen.append(v); s.key(key, 0.25)
    return seen
layouts = cycle("l", "Layout"); themes = cycle("t", "Theme")
GAMES = ["1", "2", "3", "4", "5", "6"]
GN = {"1": "Coin Toss", "2": "Saltmarch", "3": "Emberdeep", "4": "Nullroute", "5": "Long Haul", "6": "Hallpass"}
SCRIPT = ["\r", "\r", "\x1b[B", "\r", "\x1b[C"]
res = {"layouts": layouts, "themes": themes, "combos": []}
status = {rows - 1, rows - 2}
n = 0
for L in layouts:
    for T in themes:
        if n >= maxc: break
        s.key("t", 0.25)
        lay, th = setting("Layout"), setting("Theme")
        s.key("\x1b", 0.4)
        e = {"layout": lay, "theme": th, "games": {}}
        base_mark = None
        for g in GAMES:
            s.key("d", 0.4)
            mark = len(s.raw()); rm = len(s.tal.rev)
            s.key(g, 0.6)
            frames = [s.text()]
            for k in SCRIPT:
                s.key(k, 0.22); frames.append(s.text())
                if s.has("Main menu"): break
            for _ in range(3):
                if s.has("Main menu"): break
                s.key("\x1b", 0.3)
            raw = s.raw()[mark:]; rev = s.tal.rev[rm:]
            bold = [r for r in rev if r[3]]
            bad_bold = [r for r in bold if r[0] not in status]
            plain = [r for r in rev if not r[3]]
            # last-row marker region must be intact on every frame (col 0 still the marker cell when the board draws one)
            esc_bad = len(re.findall(rb"\x1b[PX^_\]]|\x1b\[\?1049|\x1b\[[0-9;]*[ABCDHJKfsu]\b(?!.)", b"")) # placeholder
            e["games"][g] = {"bold_rev": len(bold), "bold_rev_off_status": len(bad_bold), "plain_rev": len(plain), "bytes": len(raw),
                              "nonascii": sum(1 for b in raw if b >= 0x80) if tier == "ascii" else None,
                              "sixel_kitty": len(re.findall(rb"\x1bP[0-9;]*q|\x1b_G|\x1b\]1337;", raw)),
                              "osc_dcs_apc": len(re.findall(rb"\x1b[\]P_X^]", raw)),
                              "mono_colour": len(re.findall(rb"\x1b\[(?:[0-9;]*;)?(?:3[0-7]|4[0-7]|9[0-7]|10[0-7]|38|48)(?:;[0-9;]*)?m", raw)) if tier == "ascii" else None,
                              "bad_rows": sorted({r[0] for r in bad_bold})[:5]}
            if g == "2" and n % 9 == 0: open(f"{out}/{name}-{lay}-{th}-{GN[g]}.txt", "w").write("\n=====\n".join(frames))
        res["combos"].append(e); n += 1
        s.key("s", 0.4)
    s.key("l", 0.25)
    if n >= maxc: break
res["violations"] = sum(1 for c in res["combos"] for g in c["games"].values() if g["bold_rev_off_status"] or g["sixel_kitty"] or g["osc_dcs_apc"] or (g["nonascii"] or 0) or (g["mono_colour"] or 0))
res["total_bytes"] = len(s.raw()); res["stats"] = sgr_stats(s.raw())
json.dump(res, open(f"{out}/{name}.json", "w"), indent=1)
print(name, "combos", len(res["combos"]), "violations", res["violations"], res["stats"], flush=True)
s.close()
