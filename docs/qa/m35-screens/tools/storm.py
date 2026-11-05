import sys, re, json, random, time, subprocess; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
PORT = int(sys.argv[1]); tier = sys.argv[2]; random.seed(7)
SCREENS = [("menu", []), ("forums", ["f"]), ("reader", ["f", "1", "1"]), ("saltmarch", ["d", "2"]), ("settings", ["s"]), ("widgets", ["s", "g"]), ("chat", ["c"]), ("mail", ["m"]), ("editor", ["f", "1", "w"])]
SIZES = [(40, 12), (30, 8), (20, 5), (132, 43), (44, 12), (80, 24), (200, 60), (10, 3), (1, 1), (80, 24), (120, 40), (25, 7)]
def norm(t): return re.sub(r"\d\d:\d\d", "hh:mm", re.sub(r"20\d\d-\d\d-\d\d \d\d:\d\d", "DATE", t))
res = {}
for name, seq in SCREENS:
    s = ssh_sess(f"st-{name}", PORT, "founder", 80, 24, tier=tier)
    keys(s, seq, 0.6); t0 = time.time()
    for i in range(60):
        c, r = random.choice(SIZES) if i % 2 == 0 else (random.randint(1, 160), random.randint(1, 60))
        s.resize(c, r); time.sleep(0.08)
    s.resize(80, 24); s.settle(1.2)
    alive = subprocess.run(["tmux", "list-panes", "-t", f"st-{name}", "-F", "#{pane_dead}"], capture_output=True, text=True).stdout.strip()
    # fresh reference
    f = ssh_sess(f"stf-{name}", PORT, "founder", 80, 24, tier=tier); keys(f, seq, 0.8)
    same = norm(s.text()) == norm(f.text())
    mk = [(y, x) for y, x, c, b in s.revcells() if b][:1]; mkf = [(y, x) for y, x, c, b in f.revcells() if b][:1]
    res[name] = dict(alive=(alive == "0"), equals_fresh_screen=same, marker_after=mk, marker_fresh=mkf, seconds=round(time.time() - t0, 1))
    if not same:
        open(f"/tmp/qa/small/storm-{tier}-{name}-after.txt", "w").write(s.text()); open(f"/tmp/qa/small/storm-{tier}-{name}-fresh.txt", "w").write(f.text())
    s.close(); f.close()
for k, v in res.items(): print(tier, k, v)
json.dump(res, open(f"/tmp/qa/small/storm-{tier}.json", "w"), indent=1)
