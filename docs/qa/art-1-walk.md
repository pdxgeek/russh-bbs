# ART-1 walk: area art, logoff art and the example board kit over real ssh

Date: 2026-11-06. Branch `art/area-art-and-sample-board`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a scratch directory, layout `classic`, board name "Tidepool BBS") driven by OpenSSH 9.6 (`ssh -tt`, `TERM=xterm-256color`, `COLORTERM=truecolor`) inside `tmux` at 80x24, screens rendered by `tools/screenshots/shot.py` (quantized to 32 colours; the folder is about 1.4 MB). Script: [art-1-screens/walk.py](art-1-screens/walk.py) (phases `export`, `import`, `shots`).

## What the walk did

1. **Installed the kit the way a sysop would:** `menus.toml` and `pages/` copied beside the host key. The board claimed by the founder over ssh.
2. **Exported a style with UI-7's screen** (Sysop tools, `p`, `e` on Paper: "Exported paper to packs/paper.pack.toml"). That file was edited into the `tidepool` style (new palette, `rule` titles, `diamonds` header, four original art pieces) and saved as `examples/board/packs/tidepool.pack.toml`.
3. **Imported it through the real screen** (`p`, `i`: "Imported tidepool (1 art files). It loads when the server restarts."), then restarted the server once per style (`RUSSH_BBS_BOARD_STYLE`) and captured, for each of the seven styles at 80x24: the main menu (`NN-1`), forums (`-2`), mail (`-3`), chat (`-4`), the doors list (`-5`) and the logoff screen (`-6`).

## What it showed

- **Menu (all styles).** "The Front Hall": Notice board, Message boards, Post office, The Commons, then the unlisted built-in items in their usual order. Reordered, renamed, re-keyed and grouped as `menus.toml` says.
- **Area art.** The style's own three-row header draws above the forum list, the mail list and the chat rooms in all seven styles, above the table and below the header bar, and the table keeps its rows.
- **Logoff.** The picture draws above the "Logged off" box. The frame is on screen only until the connection closes and the client leaves the alternate screen, so `walk.py` keeps the bytes the server sent (`tee`) and draws the last frame before `ESC[?1049l`. A person sees it as a brief frame; that timing is existing behaviour and was not changed.
- **Doors list.** It draws **no** area header (the `-5-doors` shots). Files is a placeholder ("not built yet"). So "an area header for doors and files" is not possible without engine work: FIX-ART-1.
- **Art on Paper.** Art cells with no background colour draw on the terminal's own background, not the style's surface, so Paper's header sits on a dark strip in these shots, as the existing Paper menu header does. The new Paper pieces use bright blue so they read on either. That is existing behaviour of the art slot and was not changed.

## Not walked or not verified

- Only 80x24, only the Unicode tier in tmux, and 256/truecolor. The ASCII tier, 16-colour, mono and other sizes are covered by unit tests (`ui/kit_tests.rs` asks the real chooser for every style in the Unicode and ASCII tiers and for the narrow fallback) and not by this walk.
- No telnet, no Terminal.app, no Nerd Font terminal.
- The pages were loaded and tested (`kit_tests.rs`) and the menu was walked; the page screens themselves were not opened over ssh in this walk (UI-6's walk did that for pages with art).
- The Files and Doors screens were not changed.
