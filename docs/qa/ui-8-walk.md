# UI-8 walk: effects and graphics over real ssh

Date: 2026-11-05. Branch `m35/effects-graphics`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a fresh board) claimed with the OpenSSH 9.6 client (`ssh claim@127.0.0.1`, a throwaway ed25519 key made for the walk), then signed in as the founder with `ssh -tt` in a pty of 80x24 and 132x43, with `TERM=xterm-256color` and `COLORTERM=truecolor` sent. The script is `ui-8-screens/walk.py` (it records what a terminal emulator, `pyte`, shows every ~110 ms from the moment the session opens, and tallies the bytes and escape sequences the server sent); `ui-8-screens/sheet.py` turns the frames into sheets and GIFs through the M4-S3 `render.py`. Both are kept so the walk can be repeated: `python3 walk.py claim PORT KEY TOKEN`, `python3 walk.py capture PORT KEY OUT`, `python3 sheet.py OUT SHEETS`.

## What was captured

| What | Picture | Result |
|---|---|---|
| The intro, first sign-in (icon check unanswered: plain ASCII, no colour), 80x24 | [sheet](ui-8-screens/intro-ascii-80x24-sheet.png), [gif](ui-8-screens/intro-ascii-80x24.gif) | 17 distinct frames; the board name appears as `R U S S H - B B S`, a few stars, then dissolves into the login screen. 7.7 KB sent. No colour sequence at all. |
| The intro at 132x43, truecolor client | [sheet](ui-8-screens/intro-132x43-sheet.png), [gif](ui-8-screens/intro-132x43.gif) | 19 distinct frames; two-tone plasma, stars, big letters lighting up one by one with a shine and an underline, then the dissolve. **143,636 bytes** for the whole intro. |
| The intro at 80x24, truecolor client | [sheet](ui-8-screens/intro-80x24-sheet.png), [gif](ui-8-screens/intro-80x24.gif) | 18 distinct frames; the letters are tall (1x2) to fit 80 columns. **84,119 bytes.** |
| A key in the middle of the intro (132x43) | [sheet](ui-8-screens/interrupt-132x43-sheet.png), [gif](ui-8-screens/interrupt-132x43.gif) | The key ended the intro at once and was handled: the next frames are the main menu (the short sweep, then static). |
| The typed goodbye after `Log off` | [frames](ui-8-screens/logoff-typed/) | 10 distinct frames: "Goodbye, founde" at about 0.3 s, the full line at about 0.6 s, printed on the main screen. |
| Motion turned off (Terminal settings `m`) | [settings text](ui-8-screens/settings-after-m-132x43.txt), `report.json` | Settings showed `Motion: off`. Returning to the menu drew **0 bytes** and 1 distinct frame (no sweep). A new sign-in drew a static login: **2,097 bytes in 2.6 s**, 2 distinct frames (the first frame and the older repaint that follows the terminal-type message), no timer-driven redraws. |
| The charts, canvases and a PNG logo at 132x43, five tiers and depths | [truecolor, default style](ui-8-screens/charts-132x43-truecolor-default.png), [truecolor, Neon Night](ui-8-screens/charts-132x43-truecolor-neon.png), [256, Phosphor](ui-8-screens/charts-132x43-256-phosphor.png), [16 colours](ui-8-screens/charts-132x43-16.png), [ASCII](ui-8-screens/charts-132x43-ascii.png) | **Drawn by the test harness, not over ssh**: no screen shows a chart yet (UI-4 and UI-10 will). Made by `gallery_dump` in `ui/gallery_tests.rs`; the logo is a PNG generated at test time and decoded by `ui/image`. |

## Counted on the raw bytes the server sent

| Check | Result |
|---|---|
| Sixel, kitty or iTerm graphics sequences | **0** in every capture |
| Bold plus reverse (the reserved marker style) | **0** in every capture (ssh has no transport footer, and no effect draws it) |
| The status row during the intro | Visible in every frame of every sheet (`? help  Esc back  Ctrl-C log off`) |
| Colour in the ASCII capture | 0 truecolor and 0 256-colour sequences |
| Truecolor sequences in the colour intros | 7, all from the static login screen; the scene itself is sent in at most 256 colours (see [ui.md](../ui.md#cost-measured)) |

## What this walk found

- **`Log off` ends the session in the same cycle**, so an effect on the logoff page would never be seen. The goodbye is the text printed on the main screen after the alternate screen is left, so that is what is typed out.
- **An in-process benchmark that did not drive the clock understated the intro's cost about tenfold** (every frame was drawn at progress 0). The ssh capture showed about 200 KB where the benchmark said 40 KB. The benchmark now sets the clock per frame, the scene was reworked (256 colours, 2x2 blocks, seven brightness steps, a field that moves every 200 ms, a 120x36 stage, 36 stars), and the numbers above are what real ssh sends.
- **A wipe that hid cells broke two existing integration tests** that read the screen mid-transition; the menu transition is now a colour sweep that changes no symbol.
- **The ssh and telnet parity test (`tests/telnet.rs`) read a mid-intro frame** because its helper resizes the window after each key and the intro can start after that resize. The helper now resizes once more before reading (a resize ends any effect and repaints the static screen). This is an edit to another slice's test file, noted in coordination.

## Not walked, and what was not exercised

- **A real slow link.** The slow-send guard (`SLOW_SEND`, 250 ms) is unit-tested through `Screen::slow_link` but no throttled connection was run, so its threshold is not tuned.
- **Telnet.** The intro and sweep go through the same `Screen`, but no telnet client was run by this walk (the existing ssh/telnet parity test passes).
- **Terminals other than xterm-like ones.** The pictures are `pyte`'s model, not a real terminal; truecolor was checked by sequence counts, not by eye on a truecolor terminal. Braille and block glyphs are East Asian Ambiguous in some fonts (D18 restricts plain-Unicode glyphs to Neutral width); on such terminals the ASCII tier is the answer.
- **A screen reader**, and the M5 accessibility mode (it does not exist yet; the gate takes it as a parameter and is passed `false`).
- **An image upload.** No sysop screen uploads a logo, so `ui/image` is exercised only by its tests and the gallery.
