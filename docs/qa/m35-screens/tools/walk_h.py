#!/usr/bin/env python3
"""Hostile-vs-benign walk. usage: walk_h.py KIND PORT USER COLSxROWS TIER MODE [LAYOUT] [THEME]
MODE full: a fixed key script over forums, members, community, chat, doors, settings; menus: the menu in every layout x style."""
import json, re, sys, os
sys.path.insert(0, ".")
sys.path.insert(0, "/tmp/qa")
from lib import *
from drive import ssh_sess, keys
kind, port, user, size, tier, mode = sys.argv[1:7]
layout = sys.argv[7] if len(sys.argv) > 7 else "dashboard"; theme = sys.argv[8] if len(sys.argv) > 8 else "default"
cols, rows = map(int, size.split("x"))
tag = f"{kind}-{user}{'-telnet' if os.environ.get('QA_TELNET') else ''}-{size}-{tier}{'+colour' if os.environ.get('QA_COLOR') == 'on' else ''}-{mode}" + (f"-{layout}-{theme}" if mode == "full" else "")
out = f"/tmp/qa/hw/{tag}"; os.makedirs(out, exist_ok=True)
TN = os.environ.get("QA_TELNET")
if TN:
    Q = "."
    s = Sess(tag, f"python3 {Q}/telnetc.py 127.0.0.1 {TN} {cols} {rows} xterm-256color /tmp/qa/{tag}.wire", cols, rows, outdir="/tmp/qa/raw")
    s.settle(2); s.key("founder\r", 1); s.key("" + os.environ["QA_FOUNDER_PASS"] + "\r", 2.5); s.key(" ", 0.8)
    s.key("s", 0.5); s.key("i", 0.5)
    for k in {"nerd": ["y"], "unicode": ["n", "y"], "ascii": ["n", "n"]}[tier]: s.key(k, 0.5)
    if s.has("Icon check"): s.key("\x1b", 0.4)
    want = "on" if (tier != "ascii" or os.environ.get("QA_COLOR") == "on") else "off"
    if f"Color: {want}" not in s.text(): s.key("c", 0.4)
    s.key("\x1b", 0.6)
else:
    s = ssh_sess(tag, port, user, cols, rows, tier=tier)
def setting(label):
    m = re.search(label + r": (\S+)", s.text()); return m.group(1) if m else None
def pick(key, label, want):
    for _ in range(12):
        if setting(label) == want: return True
        s.key(key, 0.3)
    return False
if tier == "ascii" and "color" in os.environ.get("QA_COLOR", ""):
    pass
s.key("s", 0.5)
if os.environ.get("QA_COLOR") == "on" and setting("Color") != "on": s.key("c", 0.4)
if mode == "full": pick("l", "Layout", layout); pick("t", "Theme", theme)
s.key("\x1b", 0.6)
RES = []
RESERVED = {"lb": "⟦", "rb": "⟧", "minus": "⊖", "f132": "", "e000": "", "f8ff": "", "pua16": "\U0010fffd", "u301a": "〚", "u2609": "☉"}
TOK = {"semi53": b";53m", "osc0": b"\x1b]0;", "bel": b"\x07", "c1_9b": b"\xc2\x9b", "nul": b"\x00", "del": b"\x7f", "gfx": b"\x1bP", "apc": b"\x1b_"}
def snap(name):
    global mark, rmark
    txt = s.text(); raw = s.raw()[mark:]; rev_drawn = s.tal.rev[rmark:]; scr = s.revcells()
    bad_rows = {rows - 1, rows - 2}
    viol = [r for r in rev_drawn + scr if r[0] not in bad_rows or not r[3]]
    glyphs = {k: txt.count(v) for k, v in RESERVED.items()}
    toks = {k: raw.count(v) for k, v in TOK.items()}
    B = set("│┃║┌┐└┘├┤┬┴┼╭╮╯╰╔╗╚╝╠╣╦╩╬╒╓╕╖╘╙╛╜╞╟╡╢╤╥╧╨╪╫┏┓┗┛+|")
    sil = [[x for x in range(cols) if s.scr.buffer[y][x].data in B] for y in range(rows)]
    RES.append(dict(name=name, glyphs=glyphs, toks=toks, viol=len(viol), viol_cells=[list(v[:3]) for v in viol[:4]], rev_screen=[list(r[:3]) for r in scr], bytes=len(raw), sil=sil))
    open(f"{out}/{len(RES):02d}-{name}.txt", "w").write(txt + "\n")
    mark = len(s.raw()); rmark = len(s.tal.rev)
mark = len(s.raw()); rmark = len(s.tal.rev)
def step(name, seq, wait=0.6):
    keys(s, seq, wait); s.settle(0.4); snap(name)
if mode == "menus":
    s.key("s", 0.5)
    layouts = []; themes = []
    for _ in range(9):
        v = setting("Layout")
        if v in layouts: break
        layouts.append(v); s.key("l", 0.3)
    for _ in range(12):
        v = setting("Theme")
        if v in themes: break
        themes.append(v); s.key("t", 0.3)
    for L in layouts:
        pick("l", "Layout", L)
        for T in themes:
            pick("t", "Theme", T); s.key("\x1b", 0.5); snap(f"menu-{L}-{T}"); s.key("s", 0.4)
else:
    snap("menu")
    step("tab-widgets", ["TAB"]); step("esc", ["ESC"])
    step("members", ["u"]); step("members-esc", ["ESC"])
    step("community", ["y"]); step("oneliners", ["o"]); step("o-back", ["ESC"])
    step("wall", ["g"]); step("w-back", ["ESC"]); step("callers", ["l"]); step("c-back", ["ESC"])
    step("polls", ["p"]); step("poll-open", ["1"]); step("poll-back", ["ESC"]); step("polls-back", ["ESC"]); step("comm-back", ["ESC"])
    step("forums", ["f"]); step("board1", ["1"]); step("thread1", ["1"]); step("reader-pg", ["DOWN"]); step("reader-back", ["ESC"]); step("threads-back", ["ESC"])
    step("board2", ["2"]); step("board2-back", ["ESC"]); step("board3", ["3"]); step("board3-back", ["ESC"]); step("board4", ["4"]); step("board4-back", ["ESC"]); step("board5", ["5"]); step("board5-back", ["ESC"])
    step("search", ["s", "w", "o", "r", "d", "ENTER"]); step("search-back", ["ESC", "ESC"])
    step("forums-esc", ["ESC"])
    step("chat", ["c"]); step("chat-who", ["/", "w", "h", "o", "ENTER"]); step("chat-back", ["ESC"])
    step("doors", ["d"]); step("doors-back", ["ESC"])
    step("invites", ["i"]); step("invites-back", ["ESC"])
    step("sysop", ["o"]); step("sysop-back", ["ESC"])
    step("settings", ["s"]); step("widgets", ["g"]); step("widgets-back", ["ESC", "ESC"])
json.dump(RES, open(f"{out}/result.json", "w"))
print(tag, len(RES), "screens", "viol", sum(r["viol"] for r in RES), "bytes", sum(r["bytes"] for r in RES))
s.close()
