# Nullroute UI screenshots (M4-N3)

Colour captures of the screens, rendered to PNG from ANSI text with a monospace font (a font with braille is used for the map's edges).

- **`ssh-80x24/`**: captured over a real `ssh` session (OpenSSH 9.6 client, tmux 80x24) against the built server on a fresh board, in the Unicode icon tier with colour on. These are what a member sees today (the door area is 72 by 19). `run*` and `fight*` are one run in order: scan, move, scan, a tracer fight, put it down, find the vault, copy, jack out (`debrief`).
- **`wide/`**: the 92x25 and 124x38 door areas of 100x30 and 132x43 windows, composited by the same framework path the board uses (`DoorSurface::composite`, truecolor). The board caps its box at 80 columns today, so these are not yet reachable over ssh; see D-TBD (2).
- **`ascii/`**: the ASCII icon tier at 72x19 and 124x38.

Regenerate the ANSI captures with `SHOTS_DIR=/some/dir cargo test -p russh-bbs-door-nullroute --test ui shots -- --ignored`.
