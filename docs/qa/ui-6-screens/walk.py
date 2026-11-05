#!/usr/bin/env python3
"""Walk UI-6 (sysop menus and pages) over a real OpenSSH client inside tmux.

    python3 walk.py WORKDIR OUTDIR

Needs tmux, OpenSSH, the built server (target/debug/russh-bbs-server), Pillow and pyte. Starts the
server on 127.0.0.1:2263 with a scratch board that has a menus.toml (reorder, rename, hide, a
nested group) and two pages (one with art), claims it as the founder, makes an invite, redeems it
with a second key as a member, then captures the menu as the member and as the sysop, both pages,
the Menus and pages screen (per role, pages, reload) and a hostile file that is refused.
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




MENUS = 'version = 1\n\n[[menu]]\nid = "main"\ntitle = "Night Owl Hall"\n\n  [[menu.item]]\n  target = "page:rules"\n  key = "r"\n  label = "House rules"\n\n  [[menu.item]]\n  target = "main.mail"\n  key = "x"\n  label = "Messages"\n\n  [[menu.item]]\n  target = "group:talk"\n  key = "t"\n  label = "Talk and play"\n\n  [[menu.item]]\n  target = "main.files"\n  hidden = true\n\n[[menu]]\nid = "talk"\ntitle = "Talk and play"\n\n  [[menu.item]]\n  target = "main.chat"\n\n  [[menu.item]]\n  target = "main.doors"\n\n  [[menu.item]]\n  target = "page:history"\n  key = "h"\n'
RULES = 'version = 1\ntitle = "House rules"\ntext = """\n# Welcome to Night Owl\nWe are a small board. Be kind, be brief, and remember that the sysop can read everything except PGP-encrypted mail.\n\n# The rules\n- No spam, no scams, no flooding.\n- Disagree with ideas, not with people.\n- Do not post what you would not say out loud.\n- Ask the sysop if you are unsure.\n\n> The best board is the one people come back to.\n\n---\nQuestions: ask in chat, or mail the sysop.\n"""\n'
HISTORY = 'version = 1\ntitle = "A short history"\nart = "owl.art.toml"\ntext = """\n# Since 1994 (give or take)\nNight Owl began as a single modem on a kitchen table.\n\n- 1994: first call, 2400 baud\n- 2003: moved to a closet\n- 2026: back online, over ssh\n\nThanks to everyone who kept calling.\n"""\n'
ART = 'format = 1\nname = "owl"\n\n[[piece]]\nslot = "area"\nrows = [\n  "   ,___,    N I G H T   O W L",\n  "   (o,o)    ~~~~~~~~~~~~~~~~~~",\n  "   /)_)     est. 1994",\n  \'    " "\',\n]\n'


def install(work):
    (work / "pages").mkdir(parents=True, exist_ok=True)
    (work / "menus.toml").write_text(MENUS)
    (work / "pages" / "rules.toml").write_text(RULES)
    (work / "pages" / "history.toml").write_text(HISTORY)
    (work / "pages" / "owl.art.toml").write_text(ART)


def ssh_as(name, work, size, key, user):
    tmux("kill-session", "-t", name, check=False)
    cols, rows = size
    tmux("new-session", "-d", "-s", name, "-x", str(cols), "-y", str(rows))
    cmd = (
        f"ssh -tt -p {PORT} -i {work}/{key} -o IdentitiesOnly=yes -o StrictHostKeyChecking=no "
        f"-o UserKnownHostsFile=/dev/null -o SetEnv='COLORTERM=truecolor' {user}@127.0.0.1"
    )
    keys(name, f"TERM=xterm-256color {cmd}", "Enter", wait=2)


def main():
    work, out = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
    work.mkdir(parents=True, exist_ok=True)
    out.mkdir(parents=True, exist_ok=True)
    for key in ("id", "id2"):
        if not (work / key).exists():
            subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(work / key)], check=True)
    install(work)
    size = (100, 30)
    start_server(work, "bbs", "")
    time.sleep(2)
    token = re.search(r"^\s{4}(\S+)\s*$", (work / "server.log").read_text(), re.M).group(1)
    ssh_as("sy", work, size, "id", "claim")
    wait_for("sy", "setup")
    keys("sy", token, "Enter")
    keys("sy", "founder password one", "Enter")
    keys("sy", "founder password one", "Enter", wait=1.5)
    keys("sy", "x", wait=1)
    for answer in ("n", "y"):
        keys("sy", answer, wait=0.8)
    capture("sy", out, "10-sysop-menu-100x30")
    keys("sy", "t", wait=0.8)
    capture("sy", out, "11-sysop-group-100x30")
    keys("sy", "h", wait=1)
    capture("sy", out, "12-page-history-100x30")
    keys("sy", "Escape", "Escape", wait=0.8)
    keys("sy", "r", wait=1)
    capture("sy", out, "13-page-rules-100x30")
    keys("sy", "Escape", wait=0.8)
    # an invite for a member
    keys("sy", "i", "c", wait=1.2)
    capture("sy", out, "14-invite-100x30")
    code = re.search(r"\b([A-Za-z0-9]{4,}(?:-[A-Za-z0-9]{3,})+)\b", text("sy"))
    (work / "code.txt").write_text(code.group(1) if code else "")
    print("invite code:", code.group(1) if code else None)


if __name__ == "__main__":
    main()
