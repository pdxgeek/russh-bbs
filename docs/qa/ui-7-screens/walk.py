#!/usr/bin/env python3
"""Walk UI-7 over a real OpenSSH client inside tmux and capture each screen as text and PNG.

    python3 walk.py WORKDIR OUTDIR

Needs: tmux, OpenSSH, the built server (target/debug/russh-bbs-server), Pillow and pyte.
It starts the server on 127.0.0.1:2262 with a scratch board (neon default, `board.styles` set),
claims it as the founder, then:
  1. opens Terminal settings, the gallery (`y`), previews three styles, picks one, backs out;
  2. as sysop opens Sysop tools, Theme packs: exports a style, edits the file, imports it back;
  3. restarts the server and shows the imported style in the gallery.
A second board with `board.styles = neon` shows the lock. Captures are `tmux capture-pane -e`
rendered by tools/screenshots/shot.py. The key script is here so the walk can be repeated.
"""
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools" / "screenshots"))
import shot  # noqa: E402

PORT = "2262"
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


def start_server(work, name, styles, extra=None):
    env = {
        "RUSSH_BBS_LISTEN": f"127.0.0.1:{PORT}",
        "RUSSH_BBS_HOST_KEY": str(work / "host_key"),
        "RUSSH_BBS_DB": str(work / "bbs.db"),
        "RUSSH_BBS_BOARD_NAME": "Night Owl BBS",
        "RUSSH_BBS_BOARD_STYLE": "neon",
        "RUSSH_BBS_BOARD_LAYOUT": "dashboard",
        **({"RUSSH_BBS_BOARD_STYLES": styles} if styles else {}),
        **(extra or {}),
    }
    envs = " ".join(f"{k}={v!r}" for k, v in env.items())
    tmux("kill-session", "-t", name, check=False)
    tmux("new-session", "-d", "-s", name, "-x", "200", "-y", "50")
    keys(name, f"cd {work} && env {envs} {BIN} 2>&1 | tee server.log", "Enter", wait=2)


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

    # ---- board 1: every style allowed -------------------------------------------------------
    start_server(work, "bbs", "")
    time.sleep(2)
    token = re.search(r"^\s{4}(\S+)\s*$", (work / "server.log").read_text(), re.M).group(1)
    ssh("c1", work, (132, 43))
    wait_for("c1", "setup")
    keys("c1", token, "Enter")
    keys("c1", "founder password one", "Enter")
    keys("c1", "founder password one", "Enter", wait=1.5)
    keys("c1", "x", wait=1)  # past the login page
    for answer in ("n", "y"):  # the icon check: ASCII for the first, Unicode for the second
        keys("c1", answer, wait=0.8)
    capture("c1", out, "00-menu-132x43")
    keys("c1", "s")  # Terminal settings
    capture("c1", out, "01-settings-132x43")
    keys("c1", "y", wait=1)
    capture("c1", out, "02-gallery-neon-132x43")
    keys("c1", "Down", wait=0.8)
    capture("c1", out, "03-gallery-paper-132x43")
    keys("c1", "Down", "Down", wait=0.8)
    capture("c1", out, "04-gallery-deepspace-132x43")
    keys("c1", "s", wait=0.8)  # the login sample
    capture("c1", out, "05-gallery-deepspace-login-132x43")
    keys("c1", "s", wait=0.5)
    keys("c1", "Enter", wait=1.2)  # use it
    capture("c1", out, "06-gallery-deepspace-in-use-132x43")
    keys("c1", "Escape", "Escape", wait=1)
    capture("c1", out, "07-menu-in-deepspace-132x43")

    # ---- the sysop: export, edit, import ---------------------------------------------------
    keys("c1", "o", wait=0.8)
    keys("c1", "p", wait=1.2)
    capture("c1", out, "10-packs-132x43")
    keys("c1", "Down", "Down", wait=0.5)  # phosphor is row 1; neon is row 2
    keys("c1", "e", wait=1.2)
    capture("c1", out, "11-packs-exported-132x43")
    packs = work / "packs"
    exported = next(packs.glob("*.pack.toml"))
    edited = (
        exported.read_text()
        .replace('name = "neon"', 'name = "neonx"')
        .replace('style = "neon"', 'style = "neonx"')
        .replace('title = "Neon Night"', 'title = "Neon Night X"')
    )
    (packs / "neonx.pack.toml").write_text(edited)
    keys("c1", "r", wait=1.2)
    for _ in range(8):
        keys("c1", "Down", wait=0.15)
    capture("c1", out, "12-packs-two-files-132x43")
    keys("c1", "Down", wait=0.4)  # neonx
    keys("c1", "i", wait=1.5)
    capture("c1", out, "13-packs-imported-132x43")
    # A hostile pack is refused with a reason.
    (packs / "evil.pack.toml").write_text(
        "pack = 1\ntheme = '''\nname = \"evil\"\n[style.marker]\nreverse = true\n'''\n"
    )
    keys("c1", "r", wait=1.2)
    capture("c1", out, "14-packs-hostile-refused-132x43")
    keys("c1", "C-c", wait=1)
    tmux("kill-session", "-t", "bbs", check=False)

    # ---- restart: the imported style is in the gallery --------------------------------------
    start_server(work, "bbs", "")
    time.sleep(2)
    ssh("c2", work, (100, 30), user="founder")
    time.sleep(1.5)
    keys("c2", "x", wait=1)
    keys("c2", "s", wait=0.8)
    keys("c2", "y", wait=1)
    for _ in range(12):  # until the imported style is under the cursor
        if "Neon Night X" in text("c2").splitlines()[2]:
            break
        keys("c2", "Down", wait=0.3)
    capture("c2", out, "20-gallery-imported-style-100x30")
    keys("c2", "C-c", wait=1)
    tmux("kill-session", "-t", "bbs", check=False)

    # ---- board 2: one style allowed (a lock) ------------------------------------------------
    start_server(work, "bbs", "neon")
    time.sleep(2)
    ssh("c3", work, (100, 30), user="founder")
    time.sleep(1.5)
    keys("c3", "x", wait=1)
    keys("c3", "s", wait=0.8)
    keys("c3", "y", wait=1)
    capture("c3", out, "30-gallery-locked-100x30")
    keys("c3", "C-c", wait=1)
    tmux("kill-session", "-t", "bbs", check=False)


if __name__ == "__main__":
    main()
