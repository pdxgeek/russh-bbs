import json, glob, os, re
RES = []
for h in sorted(glob.glob("/tmp/qa/hw/hostile-*/result.json")):
    d = os.path.dirname(h); b = d.replace("/hw/hostile-", "/hw/benign-")
    if not os.path.exists(b + "/result.json"): continue
    ra, rb = json.load(open(h)), json.load(open(b + "/result.json"))
    n = len(ra); issues = []; art_only = 0; tot_bytes = 0; leak = 0; viol = 0
    for x, y in zip(ra, rb):
        viol += x["viol"]; tot_bytes += x["bytes"]
        pass
        if x["sil"] != y["sil"]:
            rows = [i for i, (p, q) in enumerate(zip(x["sil"], y["sil"])) if p != q]
            if rows == [4] or (rows and max(rows) <= 6 and x["name"] in ("menu","tab-widgets","esc","members-esc","o-back","w-back","c-back","comm-back","community","polls-back","board4-back","board5","board5-back","search-back","forums-esc","invites-back","sysop-back","widgets-back","members","chat-back","doors-back","invites","sysop","board3-back","board2-back","threads-back","reader-back","board1")): art_only += 1
            else: issues.append((x["name"], f"silhouette rows {rows[:6]}"))
        for k in x["glyphs"]:
            if k in ("u301a", "u2609"): continue
            if x["glyphs"][k] != y["glyphs"][k]: issues.append((x["name"], f"glyph {k} hostile {x['glyphs'][k]} benign {y['glyphs'][k]}"))
        for k in x["toks"]:
            if x["toks"][k] != y["toks"][k]: issues.append((x["name"], f"rawtok {k} hostile {x['toks'][k]} benign {y['toks'][k]}"))
        if x["rev_screen"] != y["rev_screen"]: issues.append((x["name"], f"rev cells {x['rev_screen'][:2]} vs {y['rev_screen'][:2]}"))
    # canary check in screens of this run
    txt = ""
    for t in glob.glob(d + "/*.txt"): txt += open(t).read()
    canary = len(re.findall(r"BACKROOM-CANARY-(SUBJECT|BODY|DESC)|Backroom|SECRETFEED|SecretFeed", txt))
    RES.append((os.path.basename(d), n, viol, art_only, issues, canary, tot_bytes))
for name, n, viol, art, issues, canary, tb in RES:
    print(f"{name:62s} screens={n} rev-violations={viol} issues={len(issues)} banner-art-shimmer={art} canary-hits={canary}")
    for i in issues[:6]: print("     ", i)
print("TOTAL runs", len(RES), "screens", sum(r[1] for r in RES), "issues", sum(len(r[4]) for r in RES), "violations", sum(r[2] for r in RES))
