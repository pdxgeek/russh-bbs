#!/usr/bin/env python3
"""Walk ART-1 (area art, logoff art and the example board kit) over a real OpenSSH client in tmux.

    python3 walk.py WORKDIR OUTDIR PHASE      PHASE: export | import | shots

Needs tmux, OpenSSH, the built server (target/debug/russh-bbs-server), Pillow and pyte
(`pip install --user pyte`; never commit it). Starts the server on 127.0.0.1:2271 on a scratch
board, claims it as the founder, and installs examples/board/ the way a sysop would.

  export  claim the board, install menus.toml and the pages, export the Paper style from Sysop
          tools (p, e) so the pack can be edited into a seventh style.
  import  import examples/board/packs/*.pack.toml through Sysop tools (p, i).
  shots   for each of the seven styles: restart the server in that style and capture the forum
          list (the `area` header), mail, chat, a page and the logoff screen, at 80x24 and 100x30.
"""
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools" / "screenshots"))
import shot  # noqa: E402

PORT = "2271"
BIN = ROOT / "target" / "debug" / "russh-bbs-server"
KIT = ROOT / "examples" / "board"
SESSION = "art1"


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
    shot.render_pyte(screen, str(outdir / f"{name}.png"), quantize=32)
    (outdir / f"{name}.txt").write_text(text(session))
    print("captured", name)


def wait_for(session, needle, timeout=15):
    end = time.time() + timeout
    while time.time() < end:
        if needle in text(session):
            return
        time.sleep(0.3)
    raise SystemExit(f"timed out waiting for {needle!r}:\n{text(session)}")


def start_server(work, style=None, styles=""):
    env = {
        "RUSSH_BBS_LISTEN": f"127.0.0.1:{PORT}",
        "RUSSH_BBS_HOST_KEY": str(work / "host_key"),
        "RUSSH_BBS_DB": str(work / "bbs.db"),
        "RUSSH_BBS_BOARD_NAME": "Tidepool BBS",
        "RUSSH_BBS_BOARD_LAYOUT": "classic",
        **({"RUSSH_BBS_BOARD_STYLE": style} if style else {}),
        **({"RUSSH_BBS_BOARD_STYLES": styles} if styles else {}),
    }
    envs = " ".join(f"{k}={v!r}" for k, v in env.items())
    tmux("kill-session", "-t", "srv", check=False)
    tmux("new-session", "-d", "-s", "srv", "-x", "200", "-y", "50")
    keys("srv", f"cd {work} && env {envs} {BIN} 2>&1 | tee server.log", "Enter", wait=2.5)


def stop_server():
    tmux("send-keys", "-t", "srv", "C-c")
    time.sleep(1)
    tmux("kill-session", "-t", "srv", check=False)


def ssh(work, size, user):
    tmux("kill-session", "-t", SESSION, check=False)
    cols, rows = size
    tmux("new-session", "-d", "-s", SESSION, "-x", str(cols), "-y", str(rows))
    cmd = (
        f"ssh -tt -p {PORT} -i {work}/id -o IdentitiesOnly=yes -o StrictHostKeyChecking=no "
        f"-o UserKnownHostsFile=/dev/null -o SetEnv='COLORTERM=truecolor' {user}@127.0.0.1"
    )
    keys(SESSION, f"TERM=xterm-256color {cmd}", "Enter", wait=2)


def install_kit(work):
    """What a sysop does: copy the menu file and the pages beside the host key."""
    shutil.copy(KIT / "menus.toml", work / "menus.toml")
    shutil.copytree(KIT / "pages", work / "pages", dirs_exist_ok=True)


def claim(work, size):
    token = re.search(r"^\s{4}(\S+)\s*$", (work / "server.log").read_text(), re.M).group(1)
    ssh(work, size, "claim")
    wait_for(SESSION, "setup")
    keys(SESSION, token, "Enter")
    keys(SESSION, "founder password one", "Enter")
    keys(SESSION, "founder password one", "Enter", wait=1.5)
    keys(SESSION, "x", wait=1)
    for answer in ("n", "y"):
        keys(SESSION, answer, wait=0.8)


def setup(work):
    work.mkdir(parents=True, exist_ok=True)
    if not (work / "id").exists():
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(work / "id")], check=True)
    install_kit(work)


def phase_export(work, out):
    setup(work)
    start_server(work, style="paper")
    time.sleep(2)
    claim(work, (100, 30))
    keys(SESSION, "o", wait=0.8)       # Sysop tools
    keys(SESSION, "p", wait=1)         # Theme packs
    capture(SESSION, out, "01-packs-before-100x30")
    # the list opens on the first style; walk down to Paper by name
    for _ in range(12):
        if re.search(r"^\W*>\W*.*[Pp]aper", text(SESSION), re.M):
            break
        keys(SESSION, "Down", wait=0.3)
    keys(SESSION, "e", wait=1)
    capture(SESSION, out, "02-exported-100x30")
    print("exported:", sorted(p.name for p in (work / "packs").glob("*")))


def sysop_session(work, size=(100, 30), style="paper"):
    start_server(work, style=style)
    time.sleep(2)
    ssh(work, size, "founder")
    keys(SESSION, "x", wait=1)


def phase_import(work, out):
    """Copy the kit's pack into packs/ and import it from Sysop tools, as a sysop would."""
    (work / "packs").mkdir(exist_ok=True)
    shutil.copy(KIT / "packs" / "tidepool.pack.toml", work / "packs" / "tidepool.pack.toml")
    sysop_session(work)
    keys(SESSION, "o", wait=0.8)
    keys(SESSION, "p", wait=1)
    for _ in range(14):
        if re.search(r"^\W*>\W*tidepool\.pack", text(SESSION), re.M):
            break
        keys(SESSION, "Down", wait=0.3)
    capture(SESSION, out, "03-pack-ready-100x30")
    keys(SESSION, "i", wait=1.2)
    capture(SESSION, out, "04-pack-imported-100x30")
    print(text(SESSION))
    stop_server()


STYLES = ["phosphor", "neon", "paper", "amber", "deepspace", "ansi", "tidepool"]


def ssh_logged(work, size, user="founder"):
    """Like `ssh`, but the raw bytes the server sends are also kept, so the logoff frame (which is
    on screen only until the connection closes and the alternate screen is left) can be drawn."""
    tmux("kill-session", "-t", SESSION, check=False)
    cols, rows = size
    tmux("new-session", "-d", "-s", SESSION, "-x", str(cols), "-y", str(rows))
    cmd = (
        f"ssh -tt -p {PORT} -i {work}/id -o IdentitiesOnly=yes -o StrictHostKeyChecking=no "
        f"-o UserKnownHostsFile=/dev/null -o SetEnv='COLORTERM=truecolor' {user}@127.0.0.1 "
        f"2>&1 | tee {work}/raw.bin"
    )
    keys(SESSION, f"TERM=xterm-256color {cmd}", "Enter", wait=2.5)


def render_raw(work, out, name, size):
    """Draw the last frame sent before the client left the alternate screen."""
    raw = (work / "raw.bin").read_bytes()
    cut = raw.rfind(b"\x1b[?1049l")
    frame = raw[: cut if cut >= 0 else len(raw)].decode("utf8", "replace")
    screen = shot.ansi_to_screen(frame, *size)
    shot.render_pyte(screen, str(out / f"{name}.png"), quantize=32)
    rows = ["".join(screen.buffer[y][x].data for x in range(size[0])).rstrip() for y in range(size[1])]
    (out / f"{name}.txt").write_text("\n".join(rows) + "\n")
    print("rendered", name)


def phase_shots(work, out):
    """Each style in turn: the area header on forums, mail and chat, a door list (which draws no
    area header), and the logoff screen."""
    size = (80, 24)
    for style in STYLES:
        start_server(work, style=style)
        time.sleep(2)
        ssh_logged(work, size)
        keys(SESSION, "x", wait=1)
        capture(SESSION, out, f"{style}-1-menu-80x24")
        keys(SESSION, "f", wait=1.2)
        capture(SESSION, out, f"{style}-2-forums-80x24")
        keys(SESSION, "Escape", wait=0.8)
        keys(SESSION, "p", wait=1.2)
        capture(SESSION, out, f"{style}-3-mail-80x24")
        keys(SESSION, "Escape", wait=0.8)
        keys(SESSION, "t", "c", wait=1.5)
        capture(SESSION, out, f"{style}-4-chat-80x24")
        keys(SESSION, "Escape", wait=0.8)
        keys(SESSION, "d", wait=1.2)
        capture(SESSION, out, f"{style}-5-doors-80x24")
        keys(SESSION, "Escape", wait=0.8)
        keys(SESSION, "Escape", wait=0.8)
        keys(SESSION, "l", wait=2)
        render_raw(work, out, f"{style}-6-logoff-80x24", size)
        stop_server()


if __name__ == "__main__":
    work, out, phase = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3]
    out.mkdir(parents=True, exist_ok=True)
    {"export": phase_export, "import": phase_import, "shots": phase_shots}[phase](work, out)
