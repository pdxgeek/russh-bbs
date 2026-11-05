# FIX-UI-7a walk: the Blocks border and the line-spacing hint

Real server (`target/debug/russh-bbs-server`, scratch SQLite board, `phosphor` style, `classic` layout) driven by OpenSSH 9.6 inside tmux (`TERM=xterm-256color`, `COLORTERM=truecolor`). The key script is [fix-ui-7a-screens/walk.py](fix-ui-7a-screens/walk.py); captures are `tmux capture-pane -e` rendered by `tools/screenshots/shot.py` (PNG) and `-p` (text, beside each PNG).

What was done:

1. Claimed the board as the founder at 80x24. The **icon check shows the hint** ([00](fix-ui-7a-screens/00-icon-check-hint-80x24.png)) and again on the next step ([01](fix-ui-7a-screens/01-icon-check-unicode-80x24.png)).
2. Answered "yes" for Unicode; menu with the style's own thin border ([02](fix-ui-7a-screens/02-menu-phosphor-classic-thin-80x24.png)).
3. Terminal settings, `r` twice: the screen reads `Borders: blocks` ([03](fix-ui-7a-screens/03-settings-blocks-80x24.png)). Menu in Blocks in the first style ([04](fix-ui-7a-screens/04-menu-phosphor-classic-blocks-80x24.png)), a second style ([05](fix-ui-7a-screens/05-menu-second-style-blocks-80x24.png)), a layout with no box ([06](fix-ui-7a-screens/06-menu-third-layout-blocks-80x24.png), compact, nothing to draw) and another ([07](fix-ui-7a-screens/07-menu-another-blocks-80x24.png)).
4. A new session at 132x43: the saved choice applies at the next login ([09 login](fix-ui-7a-screens/09-login-blocks-132x43.png), [10 menu](fix-ui-7a-screens/10-menu-blocks-132x43.png), [11 settings](fix-ui-7a-screens/11-settings-blocks-132x43.png)) and in a pane layout ([12](fix-ui-7a-screens/12-menu-other-layout-blocks-132x43.png)).

Seen: both side edges and the top and bottom are continuous, the corners meet, the status row (marker cell and keys) has no border glyph. In the pane layout the divider between panes is still the thin `│` (not part of a border set).

**Not verified:** macOS Terminal.app. The renderer draws block characters as filled rectangles, so the PNGs show the intended edge, not what a font-drawn terminal shows. Only the owner can check Terminal.app. The ASCII-tier fallback is covered by tests (every layout at both sizes), not walked over ssh.
