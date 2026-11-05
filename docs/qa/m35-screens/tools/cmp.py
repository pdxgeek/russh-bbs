import json, sys, glob, os
a, b = sys.argv[1], sys.argv[2]   # hostile dir, benign dir
ra, rb = json.load(open(f"{a}/result.json")), json.load(open(f"{b}/result.json"))
bad = 0
for x, y in zip(ra, rb):
    assert x["name"] == y["name"]
    probs = []
    if x["sil"] != y["sil"]:
        rows = [i for i, (p, q) in enumerate(zip(x["sil"], y["sil"])) if p != q]
        probs.append(f"silhouette differs rows {rows[:6]}")
    for k in x["glyphs"]:
        if k in ("u301a", "u2609"): continue
        if x["glyphs"][k] != y["glyphs"][k]: probs.append(f"glyph {k}: hostile {x['glyphs'][k]} benign {y['glyphs'][k]}")
    for k in x["toks"]:
        if x["toks"][k] != y["toks"][k]: probs.append(f"rawtok {k}: hostile {x['toks'][k]} benign {y['toks'][k]}")
    if x["viol"] != y["viol"]: probs.append(f"reverse viol hostile {x['viol']} benign {y['viol']}")
    if x["rev_screen"] != y["rev_screen"]: probs.append(f"rev cells on screen differ {x['rev_screen']} vs {y['rev_screen']}")
    if probs: bad += 1; print(x["name"], "|", "; ".join(probs))
print(os.path.basename(a), "screens", len(ra), "with differences", bad)
