# UI-3 board art screenshots

The real server binary (SQLite, a scratch board named "Night Owl BBS" with two logoff quotes, one style locked per run through `RUSSH_BBS_BOARD_STYLE` and `RUSSH_BBS_BOARD_STYLES`) driven by a real OpenSSH 9.6 client in a pty (`TERM=xterm-256color`, `COLORTERM=truecolor`), read through the `pyte` terminal emulator and drawn to PNG from its cells. Each stop has a `.png` and a `.txt` (the screen as text). Two sizes: `80x24` and `132x43`.

| Style | Login banner | Main-menu header |
|---|---|---|
| phosphor | [80x24](phosphor-login-80x24.png) [132x43](phosphor-login-132x43.png) | [80x24](phosphor-menu-80x24.png) [132x43](phosphor-menu-132x43.png) |
| neon | [80x24](neon-login-80x24.png) [132x43](neon-login-132x43.png) | [80x24](neon-menu-80x24.png) [132x43](neon-menu-132x43.png) |
| paper | [80x24](paper-login-80x24.png) [132x43](paper-login-132x43.png) | [80x24](paper-menu-80x24.png) [132x43](paper-menu-132x43.png) |
| amber | [80x24](amber-login-80x24.png) [132x43](amber-login-132x43.png) | [80x24](amber-menu-80x24.png) [132x43](amber-menu-132x43.png) |
| deepspace | [80x24](deepspace-login-80x24.png) [132x43](deepspace-login-132x43.png) | [80x24](deepspace-menu-80x24.png) [132x43](deepspace-menu-132x43.png) |
| ansi | [80x24](ansi-login-80x24.png) [132x43](ansi-login-132x43.png) | [80x24](ansi-menu-80x24.png) [132x43](ansi-menu-132x43.png) |

Theme preview (Sysop tools, `t`) and the logoff screen for `neon` and `ansi`: `*-preview-*`, `*-preview-size2-*` (after `z`: 100x30, drawn in the real window, clipped with a note at 80x24) and `*-logoff-*`.

Honest notes:

- **Terminal emulator.** `pyte` does not implement the alternate screen, so on the logoff captures the farewell line the server prints after leaving it lands over the first line of the box; the quote is on the screen underneath. It is an emulator artifact, not what a real terminal shows.
- **The menu header at 80x24** is the 3-row art; at that size the screen has no room for large letters, so a board with no art (the `default` style) shows none, by design.
- Not captured: telnet, a real 16-colour or 256-colour client (the depth fallback is covered by unit tests on cells), a Nerd Font session.
