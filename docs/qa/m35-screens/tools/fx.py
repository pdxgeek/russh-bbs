import sys, re, json, time; sys.path.insert(0, "."); sys.path.insert(0, "/tmp/qa")
from lib import *
PORT = 2722
def conn(name, cols=80, rows=24, user="founder", term="xterm-256color", ct=True, port=PORT):
    return Sess(name, f"ssh -tt -i /tmp/qa/keys/founder -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR {'-o SetEnv=COLORTERM=truecolor' if ct else ''} {user}@127.0.0.1", cols, rows, {"TERM": term}, outdir="/tmp/qa/raw")
def frames(s, secs, every=0.1):
    out = []; t0 = time.time(); last = None
    while time.time() - t0 < secs:
        time.sleep(every); s.feed()
        cells = tuple((s.scr.buffer[y][x].data, s.scr.buffer[y][x].fg, s.scr.buffer[y][x].bg, s.scr.buffer[y][x].bold, s.scr.buffer[y][x].reverse) for y in range(s.rows) for x in range(s.cols))
        out.append((round(time.time() - t0, 2), len(s.raw()), hash(cells)))
    return out

R = {}
def cellsig(s): return tuple((s.scr.buffer[y][x].data, s.scr.buffer[y][x].fg, s.scr.buffer[y][x].bg, s.scr.buffer[y][x].bold, s.scr.buffer[y][x].reverse) for y in range(s.rows) for x in range(s.cols))
def run_intro(name, c, r, term="xterm-256color", ct=True, secs=5.0):
    s = conn(name, c, r, term=term, ct=ct); fr = frames(s, secs)
    raw = s.raw(); st = sgr_stats(raw)
    changes = sum(1 for a, b in zip(fr, fr[1:]) if a[2] != b[2])
    end = max([t for (t, n, h), (t2, n2, h2) in zip(fr, fr[1:]) if n2 != n] or [0])
    last_change = max([t2 for (t, n, h), (t2, n2, h2) in zip(fr, fr[1:]) if h2 != h] or [0])
    return s, dict(bytes=len(raw), stats=st, rev=s.tal.summary(), distinct_frame_changes=changes, last_byte_at=end, last_visual_change_at=last_change)
for (c, r) in ((80, 24), (132, 43)):
    s, d = run_intro(f"fx-i{c}", c, r); d["final_sig"] = hash(cellsig(s)); R[f"intro-{c}x{r}-motion-on"] = d
    open(f"/tmp/qa/fx/intro-{c}-final-on.txt", "w").write(s.text()); R[f"_sig_on_{c}"] = cellsig(s); s.close()
# any key skips: key at 0.5 s, then 's' must reach settings (key handled)
for (c, r) in ((80, 24), (132, 43)):
    s = conn(f"fx-skip{c}", c, r); time.sleep(0.5); s.key(" ", 0.6); s.feed()
    d = dict(menu_after_one_key=s.has("Main menu") or s.has("Forums"), bytes=len(s.raw()))
    s.key("s", 0.6); d["key_after_skip_handled"] = s.has("Terminal settings"); R[f"skip-{c}x{r}"] = d; s.close()
# key skips the intro and the key itself: press 's' at 0.5 s directly
s = conn("fx-skip-s"); time.sleep(0.5); s.key("s", 0.8); R["skip-with-s-at-0.5s"] = dict(settings_open=s.has("Terminal settings"), menu=s.has("Main menu")); s.close()
# motion off
s = conn("fx-m"); s.settle(3); s.key(" ", 0.6); s.key("s", 0.5)
if "Motion: on" in s.text(): s.key("m", 0.5)
R["motion_setting"] = re.search(r"Motion: (\w+)", s.text()).group(0); s.close()
for (c, r) in ((80, 24), (132, 43)):
    s, d = run_intro(f"fx-off{c}", c, r); d["final_sig_equal_to_motion_on_final"] = (cellsig(s) == R[f"_sig_on_{c}"]); R[f"login-motion-off-{c}x{r}"] = d
    open(f"/tmp/qa/fx/login-off-{c}.txt", "w").write(s.text()); s.close()
# back to motion on
s = conn("fx-m2"); s.settle(3); s.key(" ", 0.6); s.key("s", 0.5)
if "Motion: off" in s.text(): s.key("m", 0.5)
s.close()
# menu transition: motion on vs off
for tier_term in (("xterm-256color", True), ("xterm", False), ("vt100", False)):
    s, d = run_intro("fx-t", 80, 24, term=tier_term[0], ct=tier_term[1], secs=3.5); R[f"intro-TERM={tier_term[0]}-ct={tier_term[1]}"] = d; s.close()
R = {k: v for k, v in R.items() if not k.startswith("_sig")}
json.dump(R, open("/tmp/qa/fx/result.json", "w"), indent=1, default=str); print(json.dumps(R, indent=1, default=str))
