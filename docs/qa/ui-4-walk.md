# UI-4 walk: the converted screens over real ssh

Date: 2026-11-05. Branch `m35/screens`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a scratch data directory, `[board]` set through environment variables: name "Night Owl BBS", location, since; `registration_mode = open`) with the **OpenSSH 9.6p1** client (`ssh -tt`, `TERM=xterm-256color`) in a pty of 80x24, 100x30 or 132x43, read through `pyte`. The board was claimed with `ssh claim@`, `bob` and `carol` were made through the waiting room (`ssh new@`), and everything on screen was made through the keys: three boards, eight threads with replies and quotes, mail from `bob` to the founder, one-liners and wall lines from `carol`, a poll with a vote, and a chat with `bob` in the lobby (a line each, a whisper). The drivers are in [ui-4-screens/walk/](ui-4-screens/walk/) (they name the walk's scratch directory in `lib.py` and `restart.sh`; edit `S` to repeat it). The PNGs are `docs/qa/m4-s3-screens/render.py` (the shared renderer UI-7's `tools/screenshots/` was **not merged** when this slice finished, so it was not used; its box lines show small gaps), the text copies are beside them.

## Captures

| Stop | Picture(s) |
|---|---|
| Home as the dashboard (Neon Night, 132x43): the menu and ten live widgets | [01](ui-4-screens/01-dashboard-neon-132x43.png) |
| Forum boards and threads under the `area` art; sorted by subject (`o`) | [02](ui-4-screens/02-boards-neon-132x43.png) [03](ui-4-screens/03-threads-neon-132x43.png) [04](ui-4-screens/04-threads-sorted-by-subject-132x43.png) |
| The reader: tab strip, header block, quote bars (nested), scrollbar, next post | [05](ui-4-screens/05-reader-tabs-quotes-132x43.png) [06](ui-4-screens/06-reader-next-post-132x43.png) |
| Chat: the room list, then three panes with a whisper and a half-typed line | [07](ui-4-screens/07-chat-room-list-132x43.png) [08](ui-4-screens/08-chat-three-panes-132x43.png) |
| Mail: the inbox table and a message | [09](ui-4-screens/09-mail-inbox-132x43.png) [10](ui-4-screens/10-mail-reader-132x43.png) |
| Community: one-liners, the wall, last callers, the poll form, the polls table, one poll with bars | [11](ui-4-screens/11-oneliners-132x43.png) [12](ui-4-screens/12-wall-132x43.png) [13](ui-4-screens/13-last-callers-132x43.png) [14](ui-4-screens/14-poll-form-132x43.png) [15](ui-4-screens/15-polls-132x43.png) [16](ui-4-screens/16-poll-with-bars-132x43.png) |
| Members: the list, sorted with the highlight moved, a profile | [17](ui-4-screens/17-members-list-132x43.png) [18](ui-4-screens/18-members-sorted-132x43.png) [19](ui-4-screens/19-profile-132x43.png) |
| The split layout with the chat and mail panes (Neon Night, Phosphor); the wide layout (Paper); the reader in the classic layout (Amber) | [20](ui-4-screens/20-split-chat-and-mail-panes-132x43.png) [21](ui-4-screens/21-split-phosphor-132x43.png) [22](ui-4-screens/22-wide-paper-132x43.png) [23](ui-4-screens/23-reader-classic-amber-132x43.png) |
| Home, threads, reader, chat, mail and polls at 80x24 in Neon Night / dashboard, Phosphor / classic and Amber / split, and at 100x30 in Paper / wide | `30-` to `35-` files in [ui-4-screens/](ui-4-screens/) |

That is four styles (Neon Night, Phosphor, Paper, Amber) and four layouts (dashboard, split, wide, classic) over real ssh, at three sizes.

## What the walk found out

- **Every converted screen drew over real ssh in every layout walked**, with the data a real board had: the dashboard's widgets filled, the area art above the lists at 132x43 (and not at 80x24, where the lists keep the rows), the reader's tab strip and quote bars, three chat panes, the mail table, and the community tables and bars.
- **The marker cell, counted on the terminal's cells** (`marker-cells.json`; the board was started with telnet on, so every staff screen carries the `telnet is on` marker): on the main menu, forums, mail, chat, polls, members and settings, at **80x24 and 132x43**, `pyte` finds **exactly three cells with reverse video, all bold, default colours, in the status bar's first cells, and none anywhere else**.
- **A defect in this slice, found and fixed by the walk's predecessor in the test suite, not by eye:** the first design of the screen change was a wipe that blanked cells until the screen was whole. It fragmented the stream: five integration tests read half a screen. It was dropped for UI-8's engine (a colour sweep that changes no symbol and ends on the static frame), which merged during this slice; UI-4 keeps only the blinking cursor.
- **A walk helper bug, not a product one:** the first key after a sign-in can end the login intro or a menu sweep (UI-8: a key ends an effect) and is not always also handled, so a scripted `s` was lost twice and the first captures were in the wrong style. The scripts now press until the settings screen is up and name each file by the style and layout actually read from the settings screen.
- **Chat nicks:** `bob` and `founder` hash to the same colour (eight hues), so two names can match; the handle is always in the text.

## Not walked, and what was not exercised

- **Telnet.** The screens go through the same `Screen`, the ssh/telnet parity integration test passes, and no telnet client was run by this walk.
- **The ASCII tier and mono over ssh.** Covered by the render matrix (every cell seven-bit and uncoloured in every layout and size), not walked: the walk's members answered the icon check with Unicode.
- **Other terminal emulators, a real 16- or 256-colour client, a Nerd Font session, a screen reader.** The pictures are `pyte`'s model. Braille cells are drawn as the font's glyphs where it has them.
- **A long chat history, a thread of many posts, a mailbox of many messages.** The windowing is unit tested; the walk used a handful of rows.
- **The blinking cursor and a running sweep** were not timed by hand; the cursor's timing is unit tested and the screenshot shows the lit half (`_`).
- **A second sysop-visible difference**: the split layout's chat pane showed `nobody here` in the screenshot because both chat sessions had left before it was taken; its names are tested with data.
