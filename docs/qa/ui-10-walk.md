# UI-10 walk: widgets over real ssh

Date: 2026-11-05. Branch `m35/widgets`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a scratch data directory, `[board]` and `[widgets]` set through environment variables: name "Night Owl BBS", style `neon`, layout `dashboard`, a message of the day, `widgets.on = motd`, `widgets.off = wall,doors`, `widgets.hidden = quote`) with the OpenSSH 9.6 client (`ssh -tt`, `TERM=xterm-256color`, `COLORTERM=truecolor` sent with `-o SetEnv`) inside `tmux`. The board was claimed with `ssh claim@127.0.0.1`, a second member (`bob`) created through the waiting room (`registration_mode = open`) and kept signed in so "who is online" has two names, and the database was seeded with SQL (three boards, 52 threads over 14 days, one of the boards a feed board, one-liners, graffiti, a poll with a vote, ten credit ledger rows). The screens below are `tmux capture-pane -e` of the real session rendered to PNG with `pyte` and Pillow (braille cells drawn as dots, because the font has no braille glyphs); the key script and the renderer were not part of the repo. Text copies of every screen are beside the PNGs.

## What the walk found out

- **Widgets draw live over ssh, in every pane layout.** The dashboard at 132x43 is three columns by four rows with ten widgets (who is online with both names, a clock with seven-segment digits, last callers, new posts as bars, mail, a braille activity chart, one-liners, the poll with bars, credits with a braille chart, the sysop's message of the day); the wide layout's right rail, the split pane and the focus pop-over fill from the same lists; at 80x24 the dashboard shows six.
- **A defect found on the way, and fixed here.** "New posts since your last call" showed nothing for a returning member. A trigger on `account_logins` showed why: **one ssh key sign-in recorded two logins**, because `auth_publickey` started the whole member session just to check that the key names its handle, and the shell then started it again, so "previous login" was always this very sign-in. It is older than this slice (it also made the sign-in screen's "Last login" show the current time). Fixed with a read-only `MemberSessions::handle_for_key`, and a regression test (`one_ssh_sign_in_records_exactly_one_login`) that fails on the old handler (2 recorded, wanted 1) and passes now. After the fix the founder's "New posts" counted the four threads active since the previous call, per board (screens 01 and 05); a reconnect minutes later says "All caught up", which is true.
- **The Widgets screen** (Terminal settings, `g`): the list on the left (`[✓]`/`[ ]`, the order numbers, `locked on` and `locked off`; the hidden widget is not listed), a live preview on the right. `Space` switched "This board" off (02 to 03) and `d` moved Clock down; the preview and the list followed at once. **The sysop's locks hold:** the message of the day (`on`) did not switch off with `Space` but moved with `u` (04), and Graffiti and Doors (`off`) cannot be switched on. The screens say "(board default)" until the first edit and "(your own list)" after it.
- **Edits are per layout and persist.** Switching to the wide layout and editing there (Activity moved up the rail, 07 to 09) did not touch the dashboard's list; after logging off and reconnecting at 80x24 the dashboard showed the edited order (13) and the Widgets screen "(your own list)" (14). The lists are in `widget_list` in the store.
- **The focus pop-over** (`Tab`) shows the first widgets' text forms (10).
- **ASCII tier and mono, over ssh** (`bob`, 80x24, icon check answered `n` twice, which stores ASCII and colour off): the dashboard (15) and the Widgets screen (16) contain **no non-ASCII character** and the capture has no colour codes, only bold, dim and underline.
- **The marker cell** (the board started with telnet on, so staff screens carry the `LOW-SECURITY` marker): in the founder's fully populated dashboard and in the Widgets screen, at 80x24 and 132x43, `pyte` finds **exactly three cells with reverse video, all bold, default colours, in the status bar's first cells**, and none anywhere else (20, 21).

## Captures

| Stop | 132x43 | 80x24 |
|---|---|---|
| 01 dashboard, board default list | [png](ui-10-screens/01-dashboard-default-132x43.png) | |
| 02 the Widgets screen, board default | [png](ui-10-screens/02-widgets-screen-132x43.png) | |
| 03 one widget switched off, one moved | [png](ui-10-screens/03-switched-one-off-and-moved-one-132x43.png) | |
| 04 the locked-on widget does not switch off but moves | [png](ui-10-screens/04-locked-on-cannot-be-switched-off-but-moves-132x43.png) | |
| 05 the dashboard after the edit | [png](ui-10-screens/05-dashboard-after-edit-132x43.png) | |
| 06 the wide layout | [png](ui-10-screens/06-wide-default-132x43.png) | |
| 07, 08 the Widgets screen on the wide layout, Activity moved up | [07](ui-10-screens/07-widgets-screen-wide-132x43.png) [08](ui-10-screens/08-wide-activity-moved-up-132x43.png) | |
| 09 the wide layout after the edit | [png](ui-10-screens/09-wide-after-edit-132x43.png) | |
| 10 the focus pop-over | [png](ui-10-screens/10-focus-popover-132x43.png) | |
| 11 the split layout (chat, who is online, mail, new posts, message of the day) | [png](ui-10-screens/11-split-132x43.png) | [12](ui-10-screens/12-reconnect-80x24-split-80x24.png) |
| 13, 14 after a reconnect: the saved order, the Widgets screen | | [13](ui-10-screens/13-dashboard-after-reconnect-80x24.png) [14](ui-10-screens/14-widgets-screen-80x24.png) |
| 15, 16 ASCII tier, mono: dashboard and Widgets screen | | [15](ui-10-screens/15-ascii-mono-dashboard-80x24.png) [16](ui-10-screens/16-ascii-mono-widgets-screen-80x24.png) |
| 20, 21 with the `LOW-SECURITY` marker: dashboard and Widgets screen | [20](ui-10-screens/20-marker-dashboard-132x43.png) [21](ui-10-screens/21-marker-widgets-screen-132x43.png) | [text](ui-10-screens/20-marker-dashboard-80x24.txt) |

## Not walked, and what was not exercised

- **Postgres and telnet sessions.** The store half ran on SQLite, plain Postgres 16 and TLS-required Postgres 16 through the shared suite (see ui.md); the walk used SQLite and ssh only. Telnet runs the same shell (`ShellParts`), but no widget screen was drawn over a telnet client.
- **Other styles.** Only `neon` was walked; every style and layout is covered by the render matrix tests and the goldens.
- **Mail and chat data.** The mail widget showed "No unread mail" (no second member sent mail) and the chat widget "0 in #lobby"; their non-empty forms are covered by the render tests only. A feed board's headlines were seeded as a board with `feed = 1` and shown by the tests' fixtures, not read from a live feed.
- **Doors and status** were hidden or locked off in this walk (`widgets.off = doors`; status is only offered to staff and was left off the default lists); both draw in the render tests.
- **Timer refresh.** The "who is online" count changing without a key press was not timed by hand; the budget logic is unit tested, and the walk's first frame already had data.
- **Another terminal emulator, a real 16-colour or 256-colour client, a Nerd Font session, a screen reader.** The `pyte` renderer is not a terminal emulator that shows what a person would see; braille is drawn as dots there.
- **A hostile member's text in a live widget over ssh** (escape sequences in a handle, a one-liner or a poll option). Covered by `ui::widgets::tests::member_text_in_widgets_is_cleaned_in_every_tier` and the store suite, not walked.
