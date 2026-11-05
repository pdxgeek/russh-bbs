# UI-7 walk: the style gallery and theme packs over real ssh

Date: 2026-11-05. Branch `m35/theme-gallery`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a scratch directory, board name "Night Owl BBS", style `neon`, layout `dashboard`, set through environment variables) driven by OpenSSH 9.6 (`ssh -tt`, `TERM=xterm-256color`, `COLORTERM=truecolor`) inside `tmux`. The board was claimed as the founder. Each screen is `tmux capture-pane -e` rendered to PNG by the shared renderer, `tools/screenshots/shot.py`, with a text copy beside it. The key script is [ui-7-screens/walk.py](ui-7-screens/walk.py).

## What the walk showed

- **Gallery, three styles and a pick (02, 03, 04, 05, 06, 07).** Terminal settings, `y`: the strip of all nine styles and a live sample of Neon Night, then Paper, then Deep Space (a starfield backdrop, its own header art and a diamond header decoration). `s` switches the sample to the login screen (05). `Enter` on Deep Space changed the line to "in use" (06) and the real menu, after `Esc Esc`, was in Deep Space (07).
- **Packs, export, edit, import (10 to 14).** Sysop tools, `p`: the installed styles and no files. `e` on Neon Night wrote `packs/neon.pack.toml` ("Exported neon to packs/neon.pack.toml", 11). The file was edited outside the board (the name `neonx`, the title "Neon Night X", the art `style`) and saved as `neonx.pack.toml`; `r` listed it as "ready: style neonx" (12); `i` imported it ("Imported neonx (1 art files). It loads when the server restarts.", 13). A hostile pack with `[style.marker] reverse = true` was listed as refused with its reason (14).
- **After a restart (20)** the gallery offers "Neon Night X". The audit rows `theme.exported` and `theme.imported` are asserted by the in-process ssh test (`crates/server/tests/access_paths.rs`), not read off this walk.
- **A lock (30).** With `board.styles = neon` the gallery shows one style and says "the sysop has set this board's style"; there is no "Enter" hint.

## Not walked

- Another terminal emulator, a real 16-colour or 256-colour client, Terminal.app, a Nerd Font session, a screen reader. The renderer is not a terminal emulator.
- The sample screen carries its own status bar at its bottom edge, drawn by the real menu code; the gallery's own status bar (and its marker cell) is the last row. The walk board had no marker to show (telnet off); the marker-cell tests cover it.
- Telnet sessions and Postgres. The store was not touched.
- A hot reload: imports load at the next start by design (D126).
