import sys, time, json, subprocess, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lib import *
PORT = int(sys.argv[1]); label = sys.argv[2]
rates = {"56kbit": 7000, "1Mbit": 125000}
res = {}
proxies = []
for i, (rn, rate) in enumerate(rates.items()):
    pp = PORT + 10 + i
    proxies.append(subprocess.Popen(["python3", "slowproxy.py", str(pp), str(PORT), str(rate)]))
    time.sleep(0.5)
    for cols, rows in ((80, 24), (132, 43)):
        for keyat in ("late", "early"):
            s = Sess(f"m{label}{rn}{cols}{keyat}", f"ssh -tt -i /tmp/qa/keys/founder -p {pp} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR founder@127.0.0.1", cols, rows, {"TERM": "xterm-256color", "COLORTERM": "truecolor"}, outdir="/tmp/qa/raw")
            t0 = time.time(); tl = None
            # first screens: icon check may appear once; answer it if so
            while time.time() - t0 < 90:
                time.sleep(0.1); s.feed()
                if s.has("look like icons"):
                    s.key("x", 0.1); s.key("n", 0.1); s.key("y", 0.1)
                if tl is None and s.has("Any key: continue"): tl = round(time.time() - t0, 1); break
                if keyat == "early" and time.time() - t0 > 0.5 and len(s.raw()) > 3000 and tl is None:
                    break
            if keyat == "late" and tl is not None:
                time.sleep(1.5)
            got = len(s.raw())
            t1 = time.time(); s.key(" ", 0.0)
            while time.time() - t1 < 90 and not s.has("Main menu"):
                time.sleep(0.05); s.feed()
            tm = round(time.time() - t1, 2)
            total = len(s.raw())
            key = f"{rn} {cols}x{rows} key_{keyat}"
            res[key] = dict(login_visible_after_s=tl, bytes_at_key=got, menu_after_key_s=tm, total_bytes=total)
            print(key, res[key], flush=True)
            subprocess.run(["tmux", "kill-session", "-t", s.name], capture_output=True)
for p in proxies: p.kill()
json.dump(res, open(f"/tmp/qa/out-{label}.json", "w"), indent=1)
