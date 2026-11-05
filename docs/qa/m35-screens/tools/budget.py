import sys, time, json, os; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
res = {}
for motion in ("on", "off"):
    s = ssh_sess(f"bud-{motion}", 2922, "bob", 132, 43)
    s.key("s", 0.5)
    if f"Motion: {motion}" not in s.text(): s.key("m", 0.5)
    s.key("\x1b", 1.5)
    s.settle(1.0)
    t0 = time.time(); last = os.path.getsize(s.rawf); ev = []
    while time.time() - t0 < 66:
        time.sleep(0.05); n = os.path.getsize(s.rawf)
        if n != last: ev.append((round(time.time() - t0, 2), n - last)); last = n
    # group events closer than 0.4s into one redraw
    groups = []
    for t, n in ev:
        if groups and t - groups[-1][1] < 0.4: groups[-1][1] = t; groups[-1][2] += n
        else: groups.append([t, t, n])
    gaps = [round(b[0] - a[0], 1) for a, b in zip(groups, groups[1:])]
    res[motion] = dict(seconds=66, redraws=len(groups), bytes=sum(g[2] for g in groups), gaps=gaps, times=[g[0] for g in groups])
    s.close()
json.dump(res, open("/tmp/qa/budget.json", "w"), indent=1); print(json.dumps(res))
