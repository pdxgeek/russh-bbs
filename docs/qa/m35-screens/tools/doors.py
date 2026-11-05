import sys, json, re; sys.path.insert(0,"/tmp/qa"); sys.path.insert(0,".")
from lib import *
from drive import *
DEPTHS = {"truecolor": ("xterm-256color", True), "256": ("xterm-256color", False), "16": ("xterm", False)}
GAMES = {"1": "Coin Toss", "2": "Saltmarch", "3": "Emberdeep", "4": "Nullroute", "5": "Long Haul", "6": "Hallpass"}
B = set("│┃║┌┐└┘├┤┬┴┼╭╮╯╰╔╗╚╝╠╣╦╩╬╒╓╕╖╘╙╛╜╞╟╡╢╤╥╧╨╪╫┏┓┗┛")
def run(depth, game, layout, cols, rows, tier="nerd"):
    term, ct = DEPTHS[depth]
    s = ssh_sess(f"d-{depth}-{game}", 2722, "founder", cols, rows, tier=tier, env={"TERM": term}, ct=ct)
    if not ct: pass
    s.key("s", 0.4)
    for _ in range(9):
        if f"Layout: {layout}" in s.text(): break
        s.key("l", 0.3)
    s.key("\x1b", 0.5); keys(s, ["d", game], 0.9); mark = len(s.raw()); rm = len(s.tal.rev)
    shots = []
    seq = ["\r", "\r", "DOWN", "ENTER", "RIGHT", "ESC", "h", "1", "2", "ESC"]
    ext = None
    for i, k in enumerate(seq):
        if k == "RIGHT": s.key("\x1b[C", 0.5)
        else: keys(s, [k], 0.5)
        shots.append(s.text())
        if i == 1:
            rb = [y for y in range(rows) if any(s.scr.buffer[y][x].data in B for x in range(cols))]
            cb = [x for x in range(cols) if any(s.scr.buffer[y][x].data in B for y in range(rows))]
            ext = ((min(rb), max(rb)), (min(cb), max(cb)), [(y, x) for y, x, c, b in s.revcells() if b][:3])
    raw = s.raw()[mark:]
    sg = sgr_stats(raw)
    idx = [int(m.group(2)) for m in re.finditer(rb"\x1b\[[0-9;]*?(38|48);5;(\d+)", raw)]
    rev = s.tal.rev[rm:] ; bad_rev = [r for r in rev if r[3] and r[0] not in (rows - 1, rows - 2)]
    boldrev_anywhere = [r for r in rev if r[3]]
    # door box extents on the final in-door shot: use the shot with most border chars
    best = max(shots, key=lambda t: sum(ch in B for ch in t))
    lines = best.split("\n")
    # the grid-based extents
    # (text() has wide-char padding removed; use screen buffer for the last state only)
    rowsb = [y for y in range(rows) if any(s.scr.buffer[y][x].data in B for x in range(cols))]
    colsb = [x for x in range(cols) if any(s.scr.buffer[y][x].data in B for y in range(rows))]
    res = dict(depth=depth, game=GAMES[game], layout=layout, size=f"{cols}x{rows}", bytes=len(raw), truecolor=sg["truecolor"], c256=sg["c256"], max_idx=max(idx) if idx else None,
               n_idx_over15=sum(1 for i in idx if i > 15), bold_reverse_drawn=len(boldrev_anywhere), bold_reverse_off_status=len(bad_rev), rev_only=len([r for r in rev if not r[3]]),
               box_rows=ext[0], box_cols=ext[1], marker_in_door=ext[2],
               title_ok=GAMES[game] in "".join(shots) or game == "1")
    open(f"/tmp/qa/doors2/{depth}-{GAMES[game].replace(' ','')}-{layout}-{cols}x{rows}.txt", "w").write("\n=====\n".join(shots))
    s.close(); return res
if __name__ == "__main__":
    import os; os.makedirs("/tmp/qa/doors", exist_ok=True); os.makedirs("/tmp/qa/doors2", exist_ok=True)
    out = []
    cfgs = [(d, g, L, c, r) for (c, r) in ((80, 24), (132, 43)) for L in ("classic", "focus") for d in DEPTHS for g in GAMES]
    for cfg in cfgs:
        try: out.append(run(*cfg)); print(json.dumps(out[-1]), flush=True)
        except Exception as e: print("ERR", cfg, e, flush=True)
    json.dump(out, open("/tmp/qa/doors/result2.json", "w"), indent=1)
