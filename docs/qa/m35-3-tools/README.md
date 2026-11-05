# FIX-M35-3 throttled-link measurement

The same approach as QA M3.5 M3: a userspace proxy (`slowproxy.py` from `qa/m35-gate`, `docs/qa/m35-screens/tools/`) passes the server's bytes to a real OpenSSH client inside tmux at a fixed rate, and `pyte` reads the screen. `lib.py`, `slowproxy.py` and `claim.py` are QA's and are not copied here; put them beside these files (`git show origin/qa/m35-gate:docs/qa/m35-screens/tools/<file>`).

| File | What it does |
|---|---|
| `run.sh BIN LABEL` | Fresh scratch board with `BIN`, claims it as the founder |
| `prep.py PORT` | One unthrottled sign-in to answer the icon check, so the measured sign-in shows the login screen and its intro |
| `measure.py PORT LABEL` | For 56 kbit (7,000 B/s) and 1 Mbit (125,000 B/s), at 80x24 and 132x43, with the key pressed 1.5 s after the login box (`late`) and about 0.5 s into the intro (`early`): time to the login box, bytes received when the key went in, time from the key to `Main menu`, total bytes |
| `result-before.json`, `result-after.json` | The numbers in `docs/ui.md` (Cost), before (`main` at `ce14e69`) and after |

Keys are made with `ssh-keygen` at test time (`/tmp/qa/keys/founder`); nothing here is a credential. Needs `tmux`, `openssh-client`, Python with `pyte`.
