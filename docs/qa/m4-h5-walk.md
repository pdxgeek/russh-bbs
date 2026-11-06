# M4-H5 walk: Hallpass's term events and the East Wing's key over real ssh, two accounts

Date: 2026-11-06. Branch `m4/hallpass-content`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a fresh board, `registration_mode = open`) claimed with the OpenSSH 9.6 client; a second account (`bob`) created with `ssh new@host`; both signed in with `ssh -tt` in a pty of 80x24 (the board sends 16 colours). Phase A ran on day 0; phases B, C and D ran after stopping the server and starting it again on the same database under `faketime -f "+17d"`, `"+18d"` and `"+20d"`. The script is `docs/qa/m4-h5-screens/walk.py` (driver: `walk.sh OUT 80x24`; it needs OpenSSH, `faketime`, `sqlite3`, `pyte` and a built server). Each stop is a `.txt` (what a terminal emulator showed), `.cells` and `.png` (`tools/screenshots/shot.py`).

**One thing was prepared, and it is said here plainly.** The mystery stop (phase D) starts from a pupil the test `crates/doors/hallpass/tests/walk_save.rs` wrote into the database with `sqlite3` (the first seven clues found, the tunnel map, a free period in the grounds on day 20): playing seven clues by hand needs a fortnight of game days. **Everything after that sign-in is real keys over ssh.** The house match (phases B and C) was played from a fresh enrolment with nothing prepared; the pupils rested through the lessons (`r`) to reach after school, which is why a detention shows.

## What the walk showed

- **The calendar is announced (B01, B06).** On day 18 of the term the morning bell's footer reads *On the calendar: The house match (The grounds): Greywater against Hollin House on the lake, three afternoons of it.*
- **A calendar event, two accounts, two builds (B03 to B08).** At after school on the grounds the founder's `t` Talk list gained *The team sheet for the house match* (B03). The scene lists five jobs and says what each shut one needs. The founder, a smooth talker with 48 cool, has **two open**: *Take the touchline and lead the singing* and *Watch from the stands*; *Put your name down for the Eight* (needs rowing), *Offer to run the scoreboard* (needs tinker) and *Slip over to the Hollin House shed* (needs sneak) are shut, each with its reason (B04). Bob, a teacher's pet with 40 cool, has **one open**: the touchline needs *cool at least 48* (B07). The founder took the touchline (B05: *Mr. Gale looks at the size of your voice and the state of the stand, and decides the stand needs it*), and bob the stands (B08).
- **The path is the next day (C01 to C06).** On day 19 the founder's list had *Leading the singing from the touchline* (C01), with the answers a smooth talker has (*Strike up the school song*) and three shut ones, one needing a known troublemaker's name, one bluff 2, one a friendship with Mrs. Tarn (C02); the school song moved the stand (C03: *By the second verse the whole stand is as sharp as you are. It sounds like a school*). Bob's list had *In the stands*, **not** the touchline (C04), whose scene offers a different set (the signals of Hollin House, cocoa for the crew, a bet with Dot shut for want of a friendship) and where he kept Mags's tally (C05, C06). The team sheet was gone from both lists: it is had once.
- **The key (D01 to D06).** The prepared pupil's Journal shows the story at *You need a way into the wing from above: its key* (D02, step 4 of 7). At the caretaker's shed the `e Search` order appears (D03: it appears only where a clue can be found) and the search turned up *On a nail behind the shed door, where a careful hand would hang the one key nobody is meant to use: the East Wing's* (D04); the Journal then reads *You hold the key. The East Wing opens only after lights-out* (D05, step 5 of 7); and the plan shows the East Wing as **shut at this hour, not locked** (D06: `∙ East Wing`, where the Head's office, the staffroom, the greenroom and the paper office are `⊘` locked): it is open to the key, after lights-out.
- No security marker and no lock appears anywhere; nothing is called private.

## Captures

| Stop | 80x24 |
|---|---|
| A01-bob-enrolment | [png](m4-h5-screens/ssh-80x24/A01-bob-enrolment.png) |
| A02-bob-day | [png](m4-h5-screens/ssh-80x24/A02-bob-day.png) |
| A03-founder-day | [png](m4-h5-screens/ssh-80x24/A03-founder-day.png) |
| B01-founder-morning-bell-names-the-house-match | [png](m4-h5-screens/ssh-80x24/B01-founder-morning-bell-names-the-house-match.png) |
| B02-founder-on-the-grounds-after-school | [png](m4-h5-screens/ssh-80x24/B02-founder-on-the-grounds-after-school.png) |
| B03-founder-the-talk-list | [png](m4-h5-screens/ssh-80x24/B03-founder-the-talk-list.png) |
| B04-founder-the-team-sheet-her-options | [png](m4-h5-screens/ssh-80x24/B04-founder-the-team-sheet-her-options.png) |
| B05-founder-leads-the-singing | [png](m4-h5-screens/ssh-80x24/B05-founder-leads-the-singing.png) |
| B06-bob-morning-bell | [png](m4-h5-screens/ssh-80x24/B06-bob-morning-bell.png) |
| B07-bob-the-team-sheet-his-options | [png](m4-h5-screens/ssh-80x24/B07-bob-the-team-sheet-his-options.png) |
| B08-bob-watches-from-the-stands | [png](m4-h5-screens/ssh-80x24/B08-bob-watches-from-the-stands.png) |
| C01-founder-the-talk-list-day-two | [png](m4-h5-screens/ssh-80x24/C01-founder-the-talk-list-day-two.png) |
| C02-founder-leading-the-singing | [png](m4-h5-screens/ssh-80x24/C02-founder-leading-the-singing.png) |
| C03-founder-the-song-and-what-it-did | [png](m4-h5-screens/ssh-80x24/C03-founder-the-song-and-what-it-did.png) |
| C04-bob-the-talk-list-day-two | [png](m4-h5-screens/ssh-80x24/C04-bob-the-talk-list-day-two.png) |
| C05-bob-in-the-stands | [png](m4-h5-screens/ssh-80x24/C05-bob-in-the-stands.png) |
| C06-bob-keeps-mags-tally | [png](m4-h5-screens/ssh-80x24/C06-bob-keeps-mags-tally.png) |
| D01-founder-day-twenty-in-the-grounds | [png](m4-h5-screens/ssh-80x24/D01-founder-day-twenty-in-the-grounds.png) |
| D02-journal-the-key-step | [png](m4-h5-screens/ssh-80x24/D02-journal-the-key-step.png) |
| D03-at-the-caretakers-shed | [png](m4-h5-screens/ssh-80x24/D03-at-the-caretakers-shed.png) |
| D04-search-the-shed | [png](m4-h5-screens/ssh-80x24/D04-search-the-shed.png) |
| D05-journal-the-wing-step | [png](m4-h5-screens/ssh-80x24/D05-journal-the-wing-step.png) |
| D06-the-map-with-the-key-in-hand | [png](m4-h5-screens/ssh-80x24/D06-the-map-with-the-key-in-hand.png) |

## Not walked, and what was not exercised

- **The winter play (days 23 to 26), the formal dance (30 to 32), exam week (34 to 37).** Each is covered by `tests/calendar.rs` and its replays, not by this walk. Only the house match was played over ssh.
- **The East Wing at night.** The map shows it shut at after school; going out after lights-out (a slip-out check, nerve) and the clue behind the bolt were not played over ssh (`tests/mystery.rs` does it through the real store and keys).
- **The first seven clues by hand** (prepared, see above), a vote, an award payout, the finale (day 38), the other three endings of the house match's jobs (the boat, the board, the shed).
- **132x43, the ASCII tier, telnet, Postgres.**
- Early runs of the script failed for ordinary reasons (the first `Esc` on the day screen leaves the door, so the walk pressed it and found itself in another game's menu; a stale server from the first run held the port, so the next run signed in to the wrong board, and `stop` now kills the whole process tree; the script's pattern for a choice did not allow for the lightbar's marker, so it pressed nothing). The run shown is the last, whole, with no hand editing of any capture.
