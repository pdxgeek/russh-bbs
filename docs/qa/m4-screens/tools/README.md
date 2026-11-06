# M4-QA phase 1 walk tools

The scripts behind [../../m4.md](../../m4.md). They are the harness I used, kept so the walk can be repeated; they are not part of the board and nothing here is built by CI. They build on the M3.5 tools in [../../m35-screens/tools/](../../m35-screens/tools/README.md): copy `lib.py`, `drive.py`, `telnetc.py` and `claim.py` from there next to these files (and into `/tmp/qa`, where the scripts look), and `pip install pyte`.

Needs: `tmux`, `openssh-client`, `faketime`, `python3` with `pyte`, a release build of `russh-bbs-server`. **Passwords are made at test time**: `export QA_FOUNDER_PASS=$(openssl rand -hex 12) QA_PW_PREFIX=$(openssl rand -hex 8)-` before anything here; nothing in this folder is a credential. Keys are made with `ssh-keygen` into `/tmp/qa/keys/` (founder, alice, bob, carol, dave, erin, frank).

| Script | What it does |
|---|---|
| `bstart.sh DIR PORT TPORT [faketime -f +Nd]` | Starts the release binary on a scratch board (open registration, telnet with password login on TPORT, high rate limits); the optional prefix runs it under `faketime` |
| `join.py PORT USER` | `ssh new@` with the user's key: creates the account (open registration allows five per network per run; restart the server for more) |
| `doorsweep.py ssh\|telnet PORT USER COLSxROWS nerd\|unicode\|ascii OUT` | For every layout x style (63): opens Coin Toss and the five games, presses a short key script, and records every reverse and bold-reverse cell drawn (raw-byte Tally from `lib.py`), OSC/DCS/APC and graphics sequences, non-ASCII bytes and colour SGR (ASCII tier) |
| `monkey.py`, `monkey2.py` | A seeded "monkey" in each game (hotkeys read from the screen, Enter, arrows, digits, an occasional Esc), 100 to 160 keys a game; `monkey2.py` also checks that the status row's first 16 cells are unchanged on every key and that cell 0 is still bold-reverse |
| `revext.py ssh\|telnet PORT USER` | Where exactly the reverse cells are: row and column extent per row across all six doors |
| `run_walk.sh s4\|h4` | The M4-S4 and M4-H4 authors' two-account walks (`docs/qa/m4-s4-screens/walk.py`, `m4-h4-screens/walk.py`) on a fresh board, restarting under `faketime +8d`, `+9d` or `+11d` between phases |
| `letter_hostile.py`, `hostile.py`, `hostile2.py` | Hostile text typed into the clan-name field and a letter line (lookalike marker, U+301A/B brackets, RLO, ZWSP, Nerd PUA, BEL, C1 CSI) and read back by a second account |
| `letters_cap.py`, `readpost.py`, `carolrep.py` | Sends eight letters in a row and reads the recipient's post and the sender's next morning report |
| `concur.py` | Two ssh sessions of one account in Saltmarch |
| `kill9b.py` | Three Coin Toss calls, `SIGKILL` of the server with the session open, restart, reopen |
| `gap.py` | Opens a door after a long server downtime (run the server under `faketime +60d` and so on) |
| `sims.sh` | The five games' headless simulations in release mode, with `--nocapture` |

`../data/` holds the compact result files (`doorsweep-merged-main.json`: totals for six sweeps of 63 layout x style combinations x six doors; `mk-*.json`: the monkey's per-game counters on a staff ssh session and a telnet member).
