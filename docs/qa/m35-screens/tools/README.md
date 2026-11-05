# M3.5-QA phase 1 walk tools

The scripts behind [../../m35.md](../../m35.md). They are the harness I used, kept so the walk can be repeated; they are not part of the board and nothing here is built by CI.

Needs: `tmux`, `openssh-client`, `python3` with `pyte` and `pillow` (for `shots.py`), a release build of `russh-bbs-server`. Run the scripts from this folder (`cd tools`); work files go to `/tmp/qa` (keys, data directories, raw captures). **Keys and passwords are made at test time and are not in this repo:** `ssh-keygen -t ed25519 -N "" -f /tmp/qa/keys/<name>` for `founder`, `bob`, `h1`, `h2`, `h3`, `newbie` and `stranger`, and the founder/new member passwords come from `QA_FOUNDER_PASS` and `QA_NEWBIE_PASS` in the environment.

| Script | What it does |
|---|---|
| `lib.py` | A client in a tmux pane (`ssh` or `telnetc.py`), its bytes tapped by `pipe-pane` and fed to `pyte`; a second `pyte` screen that records every cell **drawn or erased while reverse video was on** (the raw-byte count) |
| `telnetc.py` | A raw telnet client (NAWS, terminal type, no local echo) |
| `start.sh` | Starts a scratch board (own data directory and ports, telnet on, high connection limits) |
| `claim.py` | `ssh claim@` with the printed setup token |
| `matrix.py` | Style x layout matrix: every layout (7) x every style (9) at one size and tier, ssh or telnet, colour or mono; checks the reverse style on every frame and on the final screen, ASCII bytes, colour in mono |
| `seed.py` | Seeds a board with hostile (or length-matched benign) member names, boards, threads, posts, one-liners, wall, polls and a restricted board with canary text |
| `walk_h.py`, `cmp.py`, `cmpall.py` | The same 46-screen key script against the hostile and the benign board; compares box silhouettes, reserved glyphs, raw tokens and reverse cells |
| `art.py` | Hostile `.ans` and `.art.toml` files in `art/`, every style, two sizes |
| `doors.py` | Each door at truecolor, 256 and 16 colours, classic and focus, two sizes |
| `scan.py`, `asc.py`, `small.py`, `storm.py` | Marker presence on every screen, ASCII bytes at small windows, 40x12, resize storms |
| `fx.py`, `budget.py`, `slowproxy.py`, `slow.py` | Intro bytes and frames, motion off, key skip, widget refresh gaps, throttled links |
| `e2e4.py`, `shots.py`, `runh2.sh` | End-to-end regression flow, screenshot renderer glue, the hostile run queue |
