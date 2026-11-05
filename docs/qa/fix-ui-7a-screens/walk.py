#!/usr/bin/env python3
"""Walk FIX-UI-7a (the Blocks border and the line-spacing hint) over a real OpenSSH client in tmux.

    python3 walk.py WORKDIR OUTDIR

Needs: tmux, OpenSSH, the built server (target/debug/russh-bbs-server), Pillow and pyte.
Starts the server on 127.0.0.1:2263 with a scratch board, claims it as the founder, then:
  1. the icon check (the hint line) at 80x24 and 132x43;
  2. Terminal settings: `r` cycles the border (style's own, ascii, blocks); the menu in three
     styles/layouts with Blocks at 80x24, then at 132x43 in a second session;
  3. the same member in the ASCII tier (the icon check answered "no" to every style) falls back to
     `+ - |`.
Captures are `tmux capture-pane -e` rendered by tools/screenshots/shot.py (box and block characters
are drawn as filled rectangles, so what the PNG shows is the ideal edge, not Terminal.app).
"""
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools" / "screenshots"))
import shot  # noqa: E402

PORT = "2263"
BIN = ROOT / "target" / "debug" / "russh-bbs-server"


def tmux(*args, check=True):
    return subprocess.run(["tmux", *args], check=check, capture_output=True, text=True).stdout


def keys(session, *k, wait=0.6):
    tmux("send-keys", "-t", session, *k)
    time.sleep(wait)


def text(session):
    return tmux("capture-pane", "-p", "-t", session)


def capture(session, outdir, name):
    ansi, cols, rows = shot.tmux_capture(session)
    screen = shot.ansi_to_screen(ansi, cols, rows)
    shot.render_pyte(screen, str(outdir / f"{name}.png"))
    (outdir / f"{name}.txt").write_text(text(session))
    print("captured", name)


def wait_for(session, needle, timeout=15):
    end = time.time() + timeout
    while time.time() < end:
        if needle in text(session):
            return
        time.sleep(0.3)
    raise SystemExit(f"timed out waiting for {needle!r}:\n{text(session)}")


def start_server(work):
    env = {
        "RUSSH_BBS_LISTEN": f"127.0.0.1:{PORT}",
        "RUSSH_BBS_HOST_KEY": str(work / "host_key"),
        "RUSSH_BBS_DB": str(work / "bbs.db"),
        "RUSSH_BBS_BOARD_NAME": "Night Owl BBS",
        "RUSSH_BBS_BOARD_STYLE": "phosphor",
        "RUSSH_BBS_BOARD_LAYOUT": "classic",
    }
    envs = " ".join(f"{k}={v!r}" for k, v in env.items())
    tmux("kill-session", "-t", "bbs", check=False)
    tmux("new-session", "-d", "-s", "bbs", "-x", "200", "-y", "50")
    keys("bbs", f"cd {work} && env {envs} {BIN} 2>&1 | tee server.log", "Enter", wait=2)


def ssh(name, work, size, user="claim"):
    tmux("kill-session", "-t", name, check=False)
    cols, rows = size
    tmux("new-session", "-d", "-s", name, "-x", str(cols), "-y", str(rows))
    cmd = (
        f"ssh -tt -p {PORT} -i {work}/id -o IdentitiesOnly=yes -o StrictHostKeyChecking=no "
        f"-o UserKnownHostsFile=/dev/null -o SetEnv='COLORTERM=truecolor' {user}@127.0.0.1"
    )
    keys(name, f"TERM=xterm-256color {cmd}", "Enter", wait=2)


def main():
    work, out = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    work.mkdir(parents=True, exist_ok=True)
    out.mkdir(parents=True, exist_ok=True)
    if not (work / "id").exists():
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(work / "id")], check=True)

    start_server(work)
    time.sleep(2)
    token = re.search(r"^\s{4}(\S+)\s*$", (work / "server.log").read_text(), re.M).group(1)

    # ---- 80x24: claim, icon check with the hint, then settings --------------------------------
    ssh("c1", work, (80, 24))
    wait_for("c1", "setup")
    keys("c1", token, "Enter")
    keys("c1", "founder password one", "Enter")
    keys("c1", "founder password one", "Enter", wait=1.5)
    keys("c1", "x", wait=1)  # past the login page
    wait_for("c1", "Box lines look broken?")
    capture("c1", out, "00-icon-check-hint-80x24")
    keys("c1", "n", wait=0.8)
    capture("c1", out, "01-icon-check-unicode-80x24")
    keys("c1", "y", wait=1)  # Unicode tier
    capture("c1", out, "02-menu-phosphor-classic-thin-80x24")
    keys("c1", "s", wait=0.8)
    keys("c1", "r", wait=0.6)  # ascii
    keys("c1", "r", wait=0.6)  # blocks
    capture("c1", out, "03-settings-blocks-80x24")
    keys("c1", "b", wait=0.8)
    capture("c1", out, "04-menu-phosphor-classic-blocks-80x24")
    keys("c1", "s", "t", wait=0.8)  # next style
    keys("c1", "b", wait=0.8)
    capture("c1", out, "05-menu-second-style-blocks-80x24")
    keys("c1", "s", "l", "l", wait=0.8)  # next layouts
    keys("c1", "b", wait=0.8)
    capture("c1", out, "06-menu-third-layout-blocks-80x24")
    keys("c1", "s", "t", "t", "l", wait=0.8)
    keys("c1", "b", wait=0.8)
    capture("c1", out, "07-menu-another-blocks-80x24")
    keys("c1", "C-c", wait=1)

    # ---- 132x43: the saved choice (Blocks) applies at the next login ---------------------------
    ssh("c2", work, (132, 43), user="founder")
    wait_for("c2", "Any key: continue", timeout=20)
    capture("c2", out, "09-login-blocks-132x43")
    keys("c2", "x", wait=1)
    capture("c2", out, "10-menu-blocks-132x43")
    keys("c2", "s", wait=0.8)
    capture("c2", out, "11-settings-blocks-132x43")
    keys("c2", "l", "l", "l", wait=0.8)
    keys("c2", "b", wait=0.8)
    capture("c2", out, "12-menu-other-layout-blocks-132x43")
    keys("c2", "C-c", wait=1)
    tmux("kill-session", "-t", "bbs", check=False)


if __name__ == "__main__":
    main()
