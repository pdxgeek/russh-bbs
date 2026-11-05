import sys, re, json, time; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
res = {}
for port, rate in ((3101, "960 B/s (~9.6 kbit)"), (3102, "7000 B/s (~56 kbit)"), (3103, "48000 B/s (~384 kbit)")):
    for cols, rows in ((80, 24), (132, 43)):
        s = Sess(f"slow{port}-{cols}", f"ssh -tt -i /tmp/qa/keys/founder -p {port} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o SetEnv=COLORTERM=truecolor founder@127.0.0.1", cols, rows, {"TERM": "xterm-256color"}, outdir="/tmp/qa/raw")
        t0 = time.time(); tl = None; marks = []
        # watch until the login box is visible, up to 120 s
        while time.time() - t0 < 120:
            time.sleep(0.25); s.feed()
            if tl is None and s.has("Any key: continue"): tl = round(time.time() - t0, 1)
            if tl is not None and time.time() - t0 > tl + 1.5: break
        got = len(s.raw())
        # then a key: how long until the menu
        t1 = time.time(); s.key(" ", 0.2); 
        while time.time() - t1 < 60 and not s.has("Main menu"): time.sleep(0.25); s.feed()
        tm = round(time.time() - t1, 1)
        res[f"{rate} {cols}x{rows}"] = dict(login_visible_after_s=tl, bytes_before_login=got, menu_after_key_s=tm, rev=s.tal.summary())
        print(rate, cols, rows, res[f"{rate} {cols}x{rows}"], flush=True)
        s.close()
json.dump(res, open("/tmp/qa/small/slow.json", "w"), indent=1)
