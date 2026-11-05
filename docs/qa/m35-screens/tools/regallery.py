#!/usr/bin/env python3
"""Re-render docs/ui-screenshots/{styles,art,doors-132x43} with tools/screenshots (gap-free lines).
usage: regallery.py PORT [styles|art|doors|all]"""
import sys, os, re, subprocess, time
sys.path.insert(0, "/tmp/qa"); sys.path.insert(0, "."); sys.path.insert(0, "/home/user/russh-bbs/tools/screenshots")
from lib import *
from drive import *
import shot
PORT = int(sys.argv[1]); what = sys.argv[2] if len(sys.argv) > 2 else "all"
OUT = "/home/user/russh-bbs/docs/ui-screenshots"
STYLES = ["phosphor", "neon", "paper", "amber", "deepspace", "ansi"]; LAYOUTS = ["classic", "lightbar", "compact", "dashboard", "wide", "focus", "split"]
def save(s, rel):
    shot.render_pyte(s.scr, f"{OUT}/{rel}.png")
    open(f"{OUT}/{rel}.txt", "w").write(s.text() + "\n")
def pick(s, key, label, want):
    for _ in range(14):
        if f"{label}: {want}" in s.text(): return True
        s.key(key, 0.3)
    return False
def open_settings(s):
    if "Width:" not in s.text(): s.key("s", 0.5)
if what in ("styles", "all"):
    for (c, r) in ((80, 24), (132, 43)):
        s = ssh_sess(f"rg-{c}", PORT, "founder", c, r, tier="nerd")
        for st in STYLES:
            for L in LAYOUTS:
                open_settings(s); pick(s, "t", "Theme", st); pick(s, "l", "Layout", L)
                s.key("\x1b", 0.9); save(s, f"styles/{st}-{L}-{c}x{r}")
        s.close()
if what in ("art", "all"):
    for (c, r) in ((80, 24), (132, 43)):
        for st in STYLES:
            s = ssh_sess(f"ra-{c}", PORT, "founder", c, r, tier="nerd"); open_settings(s); pick(s, "t", "Theme", st); pick(s, "l", "Layout", "classic"); s.key("m", 0.3) if "Motion: on" in s.text() else None; s.key("\x1b", 0.8); save(s, f"art/{st}-menu-{c}x{r}"); s.close()
            s = Sess(f"ra2-{c}", f"ssh -tt -i /tmp/qa/keys/founder -p {PORT} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o SetEnv=COLORTERM=truecolor founder@127.0.0.1", c, r, {"TERM": "xterm-256color"}, outdir="/tmp/qa/raw"); s.settle(2.5); save(s, f"art/{st}-login-{c}x{r}")
            s.key(" ", 0.8); keys(s, ["o", "t"], 0.8); save(s, f"art/{st}-preview-{c}x{r}")
            s.key("z", 0.8); save(s, f"art/{st}-preview-size2-{c}x{r}")
            keys(s, ["ESC"] * 4, 0.3); s.key("l", 1.2); save(s, f"art/{st}-logoff-{c}x{r}"); s.close()
if what in ("doors", "all"):
    names = {"1": "cointoss", "2": "saltmarch", "3": "emberdeep", "4": "nullroute", "5": "longhaul", "6": "hallpass"}
    s = ssh_sess("rd", PORT, "founder", 132, 43, tier="nerd")
    open_settings(s); pick(s, "l", "Layout", "classic"); s.key("\x1b", 0.6)
    for g, n in names.items():
        keys(s, ["ESC"] * 4, 0.2); keys(s, ["d", g], 1.0); keys(s, ["\r", "\r"], 0.8); shot.render_pyte(s.scr, f"{OUT}/doors-132x43/{n}.png")
    s.close()
