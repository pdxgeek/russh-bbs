# M4-H3 walk: the Hallpass screens over real ssh

Date: 2026-11-04. Branch `m4/hallpass-ui`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a fresh board each time) claimed with the OpenSSH 9.6 client (`ssh claim@127.0.0.1` with a throwaway ed25519 key, the token printed on the console), then signed in as the founder with `ssh -tt` in a pty of 80x24 and of 132x43. After the icon check (`y` for the Nerd Font tier with colour, or `n` twice for plain ASCII with no colour) the walk opened `d` Doors, Hallpass, enrolled, and pressed the keys that reach each screen. What a terminal emulator (`pyte`) showed after each step is in the `.txt` files; the same screen with the colours the server sent is drawn to a PNG by `render.py` from the emulator's cells. The script is `docs/qa/m4-h3-screens/walk.py` (it works out the arrow keys that take the map cursor to a room from the plan in `ui/map.rs`; `make_pngs.sh` draws the pictures): `python3 walk.py claim PORT KEY TOKEN`, then `python3 walk.py play PORT KEY OUT_DIR [COLSxROWS] [plain]`.

## What the walk found out about the board

- **A door in an 80x24 window gets 74 columns by 20 rows**, as the Saltmarch walk found; in a 132x43 window it is still 74 columns wide and 37 rows tall (the classic layout's box is 76 wide). So over `ssh` the screens draw at their *Tiny* size at 80x24 and the same width in the big window, where the map's plan uses taller cells (4 rows) to fill the height. The *Roomy* and *Grand* layouts (the three-column day, the map with a "who is where" panel beside it, the file with three columns) are drawn only by the tests and the truecolor pictures until another layout gives a door more room. The tests cover 74x20, 80x24, 100x30 and 132x43.
- **The board composites doors at 16 colours whatever the terminal.** The captures held no truecolor sequence (0 `38;2`); the 80x24 and 132x43 walks together show nine distinct colour values (a deep ink band, ink blue, lamplight, chalk white, detention red, brass, moss, slate and the board's own cyan), all indexed (`38;5`) values the sixteen-colour composite maps to. The accents were chosen for that (D-TBD (4)); the pictures in `design-*` are the same screens at truecolor, which is how they will look when the member setting has a depth.
- **ASCII tier, colour off:** every capture is ASCII inside the board's own frame, and the raw stream held no colour sequence at all (0 `38;5`, 0 `38;2`). The map's glyphs are `@ o . - X` and the staff mark is `!`; the stamps and `READY` / `NOT READY` are words.
- **No bold-plus-reverse (the reserved marker style) reached the screen** from the door in any capture (0 in each walk).
- **Esc is back, and leaves the door only from the day screen.** The first run of the script pressed Esc once too often and left the door; the script now goes home with Esc only while the day screen is not showing.

## Captures

Each folder has `NAME.png` and `NAME.txt` for these stops (the numbers are the order they were reached in):

| Stop | 80x24, 16 colours | 80x24, ASCII, no colour | 132x43, 16 colours |
|---|---|---|---|
| 01 the enrolment card | [png](m4-h3-screens/ssh-80x24/01-enrol.png) | [png](m4-h3-screens/ssh-ascii-80x24/01-enrol.png) | [png](m4-h3-screens/ssh-132x43/01-enrol.png) |
| 02 the day | [png](m4-h3-screens/ssh-80x24/02-day.png) | [png](m4-h3-screens/ssh-ascii-80x24/02-day.png) | [png](m4-h3-screens/ssh-132x43/02-day.png) |
| 03 the school map | [png](m4-h3-screens/ssh-80x24/03-map.png) | [png](m4-h3-screens/ssh-ascii-80x24/03-map.png) | [png](m4-h3-screens/ssh-132x43/03-map.png) |
| 03b the map after two arrow keys | [png](m4-h3-screens/ssh-80x24/03b-map-cursor-moved.png) | [png](m4-h3-screens/ssh-ascii-80x24/03b-map-cursor-moved.png) | [png](m4-h3-screens/ssh-132x43/03b-map-cursor-moved.png) |
| 04 in the History room | [png](m4-h3-screens/ssh-80x24/04-arrived-history.png) | [png](m4-h3-screens/ssh-ascii-80x24/04-arrived-history.png) | [png](m4-h3-screens/ssh-132x43/04-arrived-history.png) |
| 05 a history lesson | [png](m4-h3-screens/ssh-80x24/05-lesson-history.png) | [png](m4-h3-screens/ssh-ascii-80x24/05-lesson-history.png) | [png](m4-h3-screens/ssh-132x43/05-lesson-history.png) |
| 06 after it | [png](m4-h3-screens/ssh-80x24/06-after-lesson-history.png) | [png](m4-h3-screens/ssh-ascii-80x24/06-after-lesson-history.png) | [png](m4-h3-screens/ssh-132x43/06-after-lesson-history.png) |
| 07 in the Maths room | [png](m4-h3-screens/ssh-80x24/07-arrived-maths.png) | [png](m4-h3-screens/ssh-ascii-80x24/07-arrived-maths.png) | [png](m4-h3-screens/ssh-132x43/07-arrived-maths.png) |
| 08 a maths lesson | [png](m4-h3-screens/ssh-80x24/08-lesson-maths.png) | [png](m4-h3-screens/ssh-ascii-80x24/08-lesson-maths.png) | [png](m4-h3-screens/ssh-132x43/08-lesson-maths.png) |
| 09 after it | [png](m4-h3-screens/ssh-80x24/09-after-lesson-maths.png) | [png](m4-h3-screens/ssh-ascii-80x24/09-after-lesson-maths.png) | [png](m4-h3-screens/ssh-132x43/09-after-lesson-maths.png) |
| 10 your schemes | [png](m4-h3-screens/ssh-80x24/10-schemes.png) | [png](m4-h3-screens/ssh-ascii-80x24/10-schemes.png) | [png](m4-h3-screens/ssh-132x43/10-schemes.png) |
| 11 the target list | [png](m4-h3-screens/ssh-80x24/11-target.png) | [png](m4-h3-screens/ssh-ascii-80x24/11-target.png) | [png](m4-h3-screens/ssh-132x43/11-target.png) |
| 12 the method list | [png](m4-h3-screens/ssh-80x24/12-method.png) | [png](m4-h3-screens/ssh-ascii-80x24/12-method.png) | [png](m4-h3-screens/ssh-132x43/12-method.png) |
| 13 the planner | [png](m4-h3-screens/ssh-80x24/13-planner.png) | [png](m4-h3-screens/ssh-ascii-80x24/13-planner.png) | [png](m4-h3-screens/ssh-132x43/13-planner.png) |
| 14 the planner, another phase (Right) | [png](m4-h3-screens/ssh-80x24/14-planner-other-phase.png) | [png](m4-h3-screens/ssh-ascii-80x24/14-planner-other-phase.png) | [png](m4-h3-screens/ssh-132x43/14-planner-other-phase.png) |
| 15 twists | [png](m4-h3-screens/ssh-80x24/15-twists.png) | [png](m4-h3-screens/ssh-ascii-80x24/15-twists.png) | [png](m4-h3-screens/ssh-132x43/15-twists.png) |
| 16 getaway | [png](m4-h3-screens/ssh-80x24/16-getaway.png) | [png](m4-h3-screens/ssh-ascii-80x24/16-getaway.png) | [png](m4-h3-screens/ssh-132x43/16-getaway.png) |
| 17 the time | [png](m4-h3-screens/ssh-80x24/17-schedule.png) | [png](m4-h3-screens/ssh-ascii-80x24/17-schedule.png) | [png](m4-h3-screens/ssh-132x43/17-schedule.png) |
| 18 the file | [png](m4-h3-screens/ssh-80x24/18-file.png) | [png](m4-h3-screens/ssh-ascii-80x24/18-file.png) | [png](m4-h3-screens/ssh-132x43/18-file.png) |
| 18b the file, next tab (Right) | [png](m4-h3-screens/ssh-80x24/18-file-next.png) | [png](m4-h3-screens/ssh-ascii-80x24/18-file-next.png) | [png](m4-h3-screens/ssh-132x43/18-file-next.png) |
| 20 clubs | [png](m4-h3-screens/ssh-80x24/20-clubs.png) | [png](m4-h3-screens/ssh-ascii-80x24/20-clubs.png) | [png](m4-h3-screens/ssh-132x43/20-clubs.png) |
| 20b clubs, next club (Down) | [png](m4-h3-screens/ssh-80x24/20-clubs-next.png) | [png](m4-h3-screens/ssh-ascii-80x24/20-clubs-next.png) | [png](m4-h3-screens/ssh-132x43/20-clubs-next.png) |
| 22 the journal | [png](m4-h3-screens/ssh-80x24/22-journal.png) | [png](m4-h3-screens/ssh-ascii-80x24/22-journal.png) | [png](m4-h3-screens/ssh-132x43/22-journal.png) |
| 23 the paper | [png](m4-h3-screens/ssh-80x24/23-paper.png) | [png](m4-h3-screens/ssh-ascii-80x24/23-paper.png) | [png](m4-h3-screens/ssh-132x43/23-paper.png) |
| 24 the hall of legend | [png](m4-h3-screens/ssh-80x24/24-hall.png) | [png](m4-h3-screens/ssh-ascii-80x24/24-hall.png) | [png](m4-h3-screens/ssh-132x43/24-hall.png) |
| 25 the yearbook | [png](m4-h3-screens/ssh-80x24/25-yearbook.png) | [png](m4-h3-screens/ssh-ascii-80x24/25-yearbook.png) | [png](m4-h3-screens/ssh-132x43/25-yearbook.png) |

Truecolor designs from the test harness (the same screens at the three sizes the owner named plus the 74x20 a door gets; not captured over ssh): [design-132x43/](m4-h3-screens/design-132x43/) has all 29 states, [design-100x30/](m4-h3-screens/design-100x30/) and [design-74x20/](m4-h3-screens/design-74x20/) the same. The ones the walk could not reach: [the run](m4-h3-screens/design-132x43/run.png) and [mid-run](m4-h3-screens/design-132x43/run-act.png), [a conversation](m4-h3-screens/design-132x43/scene.png) and [the talk list](m4-h3-screens/design-132x43/talk.png), [a file with entries](m4-h3-screens/design-132x43/file.png), [a lesson in sciences](m4-h3-screens/design-132x43/lesson-sciences.png), [the map with the cursor moved](m4-h3-screens/design-132x43/map-moved.png), [the end of a term](m4-h3-screens/design-132x43/over.png) and [the enrolment card](m4-h3-screens/design-132x43/enrol.png).

## Things the walk checked

- **The map.** The cursor moves with the arrows (the place panel and the footer name the place under it); Enter on a place in reach walks there (the dorm to the History room in three corridors, then to the Maths room), and the period's lesson is then on offer. The places you cannot enter show as locked with a glyph (`⊘`, `X` in ASCII) and the legend says so in words.
- **Lessons.** `a` in the room opens the lesson; the board shows the subject's own picture (the timeline for history, the sequence strip for maths), the answers are on the lightbar, Enter takes the highlighted one three times, and the day screen says how many were right and what the grade did.
- **The planner.** `p`, a new scheme, the target (with how wary each is), the method (difficulty, legend and heat as bars, what it needs), the planner with odds for sneak, act and escape and the reasons behind them (Left and Right change which phase's reasons show), twists, getaway and the time. A scheme with no items and no time says `NOT READY` and what is missing.
- **The file, clubs, journal, paper, hall and yearbook.** The file shows the thin folder of a new pupil (no entries) and the consequences it will reach; the club cards say why each is shut and the week of meetings; the paper links to the hall of legend and the yearbook.
- **The same keys work as in the text screens:** letters, digits, Esc; the lightbar and the cursor only press the same menu items.

## Not walked, and what was not exercised

- **A scheme run, a conversation and the end of a term over `ssh`.** A run needs a scheme that is ready (its items, a time the target allows, the room); a conversation needs someone who has something to say; the end of a term needs six weeks. The run, the conversation (with its portrait and a shut choice) and the end-of-term report are drawn by the test harness in each size and tier (`tests/ui.rs`) and are in `design-*/` as pictures.
- **Telnet.** The screens draw through the same `Driver` as ssh, and a test compares the two transports; the repo's telnet client was not used.
- **A truecolor terminal.** The board sends 16 colours; the truecolor pictures are from the test harness.
- **A terminal that is not xterm-like**, a real Nerd Font installed (the Nerd Font tier draws the Unicode glyphs, D113 (2)) and a screen reader driving the look-around (no key is bound to it yet; the text is tested, not heard).
- **A door area larger than 74x37.** See the first section.
- **The Postgres suites** (this slice touches no store code).
- **A morning after:** the day roll-over (`faketime`) was not repeated for this slice; the screens read the same engine state the H1 and H2 walks exercised.
