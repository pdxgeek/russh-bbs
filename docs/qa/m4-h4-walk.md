# M4-H4 walk: Hallpass's social layer over real ssh, two accounts

Date: 2026-11-06. Branch `m4/hallpass-social`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a fresh board, `registration_mode = open`) claimed with the OpenSSH 9.6 client; a second account (`bob`) created with `ssh new@host`; both signed in with `ssh -tt` in a pty of 80x24 (the board sends 16 colours). Phase A ran on day 0; phase B ran after stopping the server and starting it again on the same database under `faketime -f "+11d"`. The script is `docs/qa/m4-h4-screens/walk.py` (driver: `walk.sh OUT 80x24`; needs OpenSSH, `faketime`, `pyte`). Each stop is a `.txt` (what a terminal emulator showed), `.cells` and `.png` (`tools/screenshots/shot.py`).

## What the walk showed

- **A scheme on the other player, end to end (A04 to A11).** Bob (a teacher's pet) opened Hallpass first, so the school knew him. The founder opened `o` Social, `c` Classmates (bob, with the note that a name cannot be typed), picked bob, saw the four mischief and three kind schemes on offer (A07) and played squeaky laces. Bob signed in again: the first line of his visit was *Your laces squeaked at every step all morning. It was founder, probably.* (A09), and Social showed it under *Lately* (A10). The founder signed in again and read *News of your squeaky laces on bob: it landed.* with legend 1 (A11). Playing it cost the founder a period (it was class time: *Nobody noticed you were not in class*) and nerve.
- **A rumour (B03 to B05).** On day 12 in the Quad, where Sol was, the founder started a rumour from the list of templates about a character; it spent the period and the line shows in the footer.
- **The election (B06 to B08, B13, B14).** Open from day 12 of the term: the ballot lists Bramley, Wren and Hollis with their platforms and backing; the founder stood and canvassed; bob's ballot then listed the founder as a candidate.
- **The opt-out (B15, B16).** Bob opted out; his Social screen says so and that rumours, the election and awards carry on; the founder's Classmates screen then said *bob has opted out and cannot be reached* and offered no scheme.
- No security marker and no lock appears anywhere; nothing is called private. The screens say the school can read everything.

## Captures

| Stop | 80x24 |
|---|---|
| A01-bob-enrolment | [png](m4-h4-screens/ssh-80x24/A01-bob-enrolment.png) |
| A02-bob-day | [png](m4-h4-screens/ssh-80x24/A02-bob-day.png) |
| A03-bob-social-hub | [png](m4-h4-screens/ssh-80x24/A03-bob-social-hub.png) |
| A04-founder-day | [png](m4-h4-screens/ssh-80x24/A04-founder-day.png) |
| A05-founder-social-hub | [png](m4-h4-screens/ssh-80x24/A05-founder-social-hub.png) |
| A06-classmates | [png](m4-h4-screens/ssh-80x24/A06-classmates.png) |
| A07-schemes-on-bob | [png](m4-h4-screens/ssh-80x24/A07-schemes-on-bob.png) |
| A08-scheme-sent | [png](m4-h4-screens/ssh-80x24/A08-scheme-sent.png) |
| A09-bob-morning-after | [png](m4-h4-screens/ssh-80x24/A09-bob-morning-after.png) |
| A10-bob-social-hub-lately | [png](m4-h4-screens/ssh-80x24/A10-bob-social-hub-lately.png) |
| A11-founder-hears-how-it-went | [png](m4-h4-screens/ssh-80x24/A11-founder-hears-how-it-went.png) |
| B01-founder-day-eleven | [png](m4-h4-screens/ssh-80x24/B01-founder-day-eleven.png) |
| B02-in-the-quad | [png](m4-h4-screens/ssh-80x24/B02-in-the-quad.png) |
| B03-rumours | [png](m4-h4-screens/ssh-80x24/B03-rumours.png) |
| B04-rumour-about-whom | [png](m4-h4-screens/ssh-80x24/B04-rumour-about-whom.png) |
| B05-rumour-started | [png](m4-h4-screens/ssh-80x24/B05-rumour-started.png) |
| B06-election-before-standing | [png](m4-h4-screens/ssh-80x24/B06-election-before-standing.png) |
| B07-election-standing | [png](m4-h4-screens/ssh-80x24/B07-election-standing.png) |
| B08-canvassed | [png](m4-h4-screens/ssh-80x24/B08-canvassed.png) |
| B13-bob-day-eleven | [png](m4-h4-screens/ssh-80x24/B13-bob-day-eleven.png) |
| B14-bob-the-ballot | [png](m4-h4-screens/ssh-80x24/B14-bob-the-ballot.png) |
| B15-bob-opted-out | [png](m4-h4-screens/ssh-80x24/B15-bob-opted-out.png) |
| B16-founder-sees-bob-opted-out | [png](m4-h4-screens/ssh-80x24/B16-founder-sees-bob-opted-out.png) |

## Not walked, and what was not exercised

- **The east wing's first search over ssh.** The planned step (rest to the free period, go to the library, press `e`) failed in the script: the sequence of Esc keys left the door and the session ended, twice. The search, every clue and every ending are covered by `tests/mystery.rs` through the real store and keys, not by this walk. The journal line and `e` order were not seen over ssh.
- **A vote, the count (day 27), the finale (day 38), an award payout.** They need later days; tested through the board's daily tick, not walked.
- **132x43, the ASCII tier, telnet, Postgres.**
- The first full run of the script also lost its phase B login once (`Permission denied`) and was discarded; the second run is what is shown.
