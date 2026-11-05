# FIX-M35-1 walk: the marker on every screen of a staff session

Slice: FIX-M35-1 (QA M3.5 finding M1, [m35.md](m35.md#m1-no-marker-no-status-row-on-reader-mail-chat-members-menu-door-list-and-running-doors-medium)). Branch `fix/m35-1-marker-everywhere`, measured on `main` at `93a22f8` (UI-4 merged) and again after the fix.

## Re-measured on current main (before the fix)

| Screen | Marker on `main` at `93a22f8` | How measured |
|---|---|---|
| Menu, forums (boards, threads), the reader, mail inbox, chat, members, community, invites, settings, sysop tools | **Present** | `tests/markers.rs` over a real ssh client: all pass on the unfixed code up to the first failure below; UI-4's screens use `FrameSpec` with the session's markers |
| Forum editor (and the mail and poll-description editors, same code path) | **Missing** (new: not in the QA list) | `tests/markers.rs` failed on the unfixed code here: `80x24 forums, editor: no marker in the first cell of the last row` |
| Door list and every running door | **Missing** | `ssh::screen::marker_tests` failed on the unfixed code: `Ascii Classic 80x24 telnet=false door list: marker row 23`. The cause is in the code: `doors::draw` used the empty `MENU_MARKERS` list, so `split_status` reserved no row and the box ran to the last row |

So UI-4 did fix the reader, mail, chat and members screens as its worker said, and the forum reader over ssh shows the marker. What was still missing was the door screens and the editors. The unfixed code was measured only as far as each test's first failure (the integration walk stops at the forum editor); the rest of the list on unfixed code is read from the code, not from a run.

## The cause and the fix

Three places drew with a hard-coded empty marker list instead of the session's: `doors::draw` (list, opening and running door), the forum and mail editors (`EditorView.markers`), and the poll-description editor (`Screen::draw_editor`). Each now takes `Screen::status_markers()` (forums and mail pass it as `DrawCtx.markers`; doors have a new `DrawCtx.markers`). The marker is still drawn only by `chrome::render_status` (`chrome::status_cell`) in the row `chrome::split_status` hands back, so a door cannot reach it and the box (sized by `door_box(layout)`) stays above it. No sanitizer, theme or style change.

## Tests (all fail on the unfixed code)

- `ssh::screen::marker_tests` (unit): replays the bytes a staff `Screen` writes into a cell grid and checks, for the menu, settings, members, community, invites, sysop tools, the door list and a running door of each of the six games (the five games and the coin toss; keys inside the door too), in **every tier x every layout x {80x24, 132x43} x {ssh, telnet footer}** (84 combinations): the first cell of the last row (the last row above the `UNENCRYPTED` footer over telnet) is bold + reverse and holds the marker; the reserved style (bold + reverse) is on no other row (the footer excepted); the telnet footer is still there. A second test runs the same walk for a plain member and for a staff member outside the mode: no marker anywhere, footer unchanged.
- `tests/markers.rs` (integration, real russh client, real store, `low_security` on): a staff session at 80x24 and 132x43 through the menu, forums, a thread (list, editor, reader), mail (inbox, compose), chat (rooms, the lobby), members, community, invites, settings, sysop tools, the door list and each of the six doors (and a key inside it); a plain member through the same walk sees no reserved cell.

## Real OpenSSH 9.6 walk (release binary, scratch board with telnet on)

`docs/qa/fix-m35-1-screens/walk.py`, using the QA M3.5 harness (`docs/qa/m35-screens/tools`): `ssh -tt` in tmux, bytes through `pyte`, a second `pyte` screen that records every cell drawn or erased while reverse video was on. The founder (sysop) claimed the board, then walked: the menu, forums (a board and a thread made in the walk: threads, editor, reader), mail inbox and compose, chat rooms and the lobby, members, community, invites, settings, sysop tools, the door list, and doors 1 to 6 with a key in each, 26 steps per run:

| Window and tier | Result on all 26 steps |
|---|---|
| 80x24, ASCII tier | The reserved cells are exactly the 12 cells of `LOW-SECURITY` at (row 23, col 0), bold + reverse. On the raw bytes, bold + reverse was drawn on row 23 only. |
| 132x43, Nerd Font tier | The reserved cell is the one marker glyph at (row 42, col 0), bold + reverse. On the raw bytes, bold + reverse was drawn on row 42 only. |

Reverse video **without** bold also appears: the selection bars of door 5 (Long Haul: 36 cells at 80x24, 62 at 132x43) and door 6 (Hallpass: 12 cells at 80x24), the editor's cursor. That is QA L1 (door palette), owned by FIX-M35-4, and it is not the reserved style. Captures (tools/screenshots): [fix-m35-1-screens/](fix-m35-1-screens/), for the menu, the reader, the editor, thread list, mail inbox and compose, the lobby, members, the door list and all six doors, at both windows.

## Not verified

- **A telnet staff session cannot exist on a real board**: staff roles are refused over telnet, so `UNENCRYPTED` plus `LOW-SECURITY` on one screen is only reachable in the unit test, which builds the session directly. Telnet was not walked with a real `telnet` client in this slice; a member's telnet footer is covered by the unit test and by `tests/telnet.rs` and `tests/doors.rs`.
- Tiers other than ASCII (80x24) and Nerd Font (132x43) and layouts other than the default were walked over a real client only through the unit table, not OpenSSH.
- Other terminals, a screen reader and a throttled link.
