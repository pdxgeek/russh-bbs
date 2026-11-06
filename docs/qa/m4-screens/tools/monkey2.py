#!/usr/bin/env python3
"""Door monkey: random but plausible keys in each game; checks per frame. usage: monkey.py ssh|telnet PORT USER COLSxROWS TIER OUT STEPS [games]"""
import json, random, re, sys
sys.path.insert(0, "/tmp/qa")
from lib import *
tr, port, user, size, tier, out, steps = sys.argv[1:8]
games = sys.argv[8].split(",") if len(sys.argv) > 8 else ["2", "3", "4", "5", "6"]
steps = int(steps); cols, rows = map(int, size.split("x"))
pw = os.environ["QA_PW_PREFIX"] + user; name = f"mk-{tr}-{user}-{size}"
os.makedirs(out, exist_ok=True)
rng = random.Random(f"{user}{size}")
if tr == "ssh":
    cmd = f"ssh -tt -i /tmp/qa/keys/{user} -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o SetEnv=COLORTERM=truecolor {user}@127.0.0.1"
else:
    cmd = f"python3 /tmp/qa/telnetc.py 127.0.0.1 {port} {cols} {rows} xterm-256color /tmp/qa/{name}.wire"
s = Sess(name, cmd, cols, rows, {"TERM": "xterm-256color"}, outdir="/tmp/qa/raw")
s.settle(2.5)
if tr == "telnet": s.key(user + "\r", 1); s.key(pw + "\r", 2.5)
s.key(" ", 0.8)
if s.has("Icon check"): s.key("y", 0.8)
s.key(" ", 0.6) if s.has("Any key") else None
s.key("s", 0.6); s.key("i", 0.5)
for k in {"nerd": ["y"], "unicode": ["n", "y"], "ascii": ["n", "n"]}[tier]: s.key(k, 0.5)
if s.has("Icon check"): s.key("\x1b", 0.4)
want = "on" if tier != "ascii" else "off"
if f"Color: {want}" not in s.text(): s.key("c", 0.4)
s.key("\x1b", 0.5)
GN = {"1": "Coin Toss", "2": "Saltmarch", "3": "Emberdeep", "4": "Nullroute", "5": "Long Haul", "6": "Hallpass"}
BAD = re.compile(r"panic|unreadable|internal error|something went wrong|backtrace|Connection closed|not implemented|todo!", re.I)
res = {}
if not s.has("Main menu"): s.key("\x1b", 0.5)
BASE = "".join(s.scr.buffer[rows-1][x].data for x in range(cols))[:16].rstrip()
BASE_REV = s.scr.buffer[rows-1][0].reverse and s.scr.buffer[rows-1][0].bold
print("baseline last row:", repr(BASE), "marker reverse+bold:", BASE_REV, flush=True)
for g in games:
    seen = set(); bad_texts = []; boldrev = 0; entered = False; stuck = 0; last = ""; keys_used = 0; deaths = 0; marker_bad = 0; marker_ex = None
    mark = len(s.raw()); rm = len(s.tal.rev)
    for i in range(steps):
        lr = "".join(s.scr.buffer[rows-1][x].data for x in range(cols))[:16].rstrip()
        c0 = s.scr.buffer[rows-1][0]
        if lr != BASE or (BASE_REV and not (c0.reverse and c0.bold)): marker_bad += 1; marker_ex = (i, lr)
        t = s.text()
        if BAD.search(t): bad_texts.append((i, [l for l in t.splitlines() if BAD.search(l)][:2]))
        if "Main menu" in t and entered is False or ("Main menu" in t and "Hold" not in t and GN[g] not in t.split("\n")[1] if len(t.split("\n")) > 1 else False):
            s.key("d", 0.5); s.key(g, 1.0); entered = True; continue
        if "Main menu" in t:
            s.key("d", 0.5); s.key(g, 1.0); continue
        hot = re.findall(r"(?m)(?:^|[^A-Za-z0-9])([a-z0-9])[\s.\])] ?[A-Z→]", t)
        r = rng.random()
        if r < 0.55 and hot: k = rng.choice(hot)
        elif r < 0.75: k = rng.choice(["\r", "\r", "\x1b[B", "\x1b[A", "\x1b[C", "\x1b[D", " "])
        elif r < 0.93: k = rng.choice(list("123456789abcdefghjklmnopqrstuvwxyz"))
        else: k = "\x1b"
        s.key(k, 0.22); keys_used += 1
        seen.add(s.text()[:2000])
        if tr == "ssh" and subprocess.run(["tmux", "list-panes", "-t", name, "-F", "#{pane_dead}"], capture_output=True, text=True).stdout.strip() == "1": deaths += 1; break
    rev = s.tal.rev[rm:]
    status = {rows - 1, rows - 2}
    boldrev_bad = [r for r in rev if r[3] and r[0] not in status]
    raw = s.raw()[mark:]
    res[GN[g]] = {"keys": keys_used, "distinct_screens": len(seen), "bad_text_hits": bad_texts[:3], "bold_rev_off_status": len(boldrev_bad), "bytes": len(raw),
                  "osc_dcs_apc": len(re.findall(rb"\x1b[\]P_X^]", raw)), "nonascii_in_ascii_tier": sum(1 for b in raw if b >= 0x80) if tier == "ascii" else None, "deaths": deaths, "marker_row_mismatch": marker_bad, "marker_example": marker_ex}
    open(f"{out}/{name}-{GN[g]}.txt", "w").write(s.text())
    print(name, GN[g], json.dumps(res[GN[g]]), flush=True)
    for _ in range(4):
        if s.has("Main menu"): break
        s.key("\x1b", 0.4)
json.dump(res, open(f"{out}/{name}.json", "w"), indent=1)
s.close()
