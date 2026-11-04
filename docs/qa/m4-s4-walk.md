# M4-S4 walk: Saltmarch's social layer over real ssh, two accounts

Date: 2026-11-04. Branch `m4/saltmarch-social`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a fresh board for each of the two passes, `registration_mode = open`) claimed with the OpenSSH 9.6 client, then a second account (`bob`) created with `ssh new@host`; both then signed in as themselves with `ssh -tt` in a pty (80x24 in the first pass, 132x43 in the second; the board sends 16 colours, as in the S3 walk). The script is `docs/qa/m4-s4-screens/walk.py` (it reuses the terminal helper of the S3 walk and is kept so the walk can be repeated: `python3 walk.py A|B|C PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]`). What a terminal emulator (`pyte`) showed after each step is in the `.txt` files; the same screen with the colours the server sent is drawn to a PNG by `../m4-s3-screens/render.py`.

The raid needs the day to change twice, so the board was stopped and started again on the same database under `faketime`: **day 0** (both clans founded, a trade, a letter), **`+8d`** (the new-clan week is over; both read the post, the founder sends a raid) and **`+9d`** (the scheduler's tick, which runs for the missed days when the server starts, has settled the raid; both read the night's report).

## What the walk found out

- **Everything worked over ssh as the tests say it does.** On day 0 the founder posted an ask for five timber on the exchange, the other account saw it in the book and bought it, and both read the answer at their next visit with the post: the founder's tallies went from 39 to 69 and bob's from 40 to 9 (a price of 6 each is 30, less a 1-tallie listing fee each: 40 - 30 - 1 = 9, 39 + 30 = 69; the levy on 30 is 0), with the five timber in bob's stores. The founder wrote a letter with a line of their own (typed over ssh) and bob read it, line and all, at the post.
- **The raid.** On day 9 the founder sent two raiders at bob from the gate (the screen shows bob's posted guard, rating, stores as a scout would guess them, the tide windows and the odds: *grim*), the raiders left the hold, and the tick settled it overnight. The founder's morning report said *Your raid on bob was beaten off the gate*, with two injuries from it; bob's said *founder came for the hold in the night and was beaten off* with an injury to the guard. The league and the hall carry it (`C02`, `C03`). The founder's odds were bad and the raid failed; a raid that wins is covered by `tests/social.rs`, not by this walk.
- **A pact was not walked.** Bob's pacts screen (`C06`) shows the truce closed for the two of them *because they raided each other within the week*, which is the grudge rule working; offering, answering and breaking a pact is covered by `tests/social.rs` and not walked over ssh.
- **A door in an 80x24 window gets 74 columns by 20 rows** (as the S3 walk measured), so the screens are the Tiny layout; in 132x43 the width is the same and the height grows. Some lines in a panel are cut or wrapped to fit (the orders list shows the full words of the order on the lightbar at the bottom).
- **No security marker, no lock.** Nothing on the social screens is labelled private; the letter screen says *Letters are read by the board and by the clan they are for*. No bold-plus-reverse cell reached the screen from the door (the same check as S3).

## Captures

`ssh-80x24/` and `ssh-132x43/` each hold `NAME.png` and `NAME.txt` for every stop (the same stops in both):

| Stop | 80x24 | 132x43 |
|---|---|---|
| A02 the exchange, timber, before posting | [png](m4-s4-screens/ssh-80x24/A02-exchange-before-posting.png) | [png](m4-s4-screens/ssh-132x43/A02-exchange-before-posting.png) |
| A03 an ask sent ("it settles with the post") | [png](m4-s4-screens/ssh-80x24/A03-exchange-ask-sent.png) | [png](m4-s4-screens/ssh-132x43/A03-exchange-ask-sent.png) |
| A04, A05 the other clan sees the ask and buys | [A04](m4-s4-screens/ssh-80x24/A04-bob-sees-the-ask.png) [A05](m4-s4-screens/ssh-80x24/A05-bob-buys.png) | [A04](m4-s4-screens/ssh-132x43/A04-bob-sees-the-ask.png) [A05](m4-s4-screens/ssh-132x43/A05-bob-buys.png) |
| A06 to A09 a letter: words, a line typed over ssh, sent | [A06](m4-s4-screens/ssh-80x24/A06-compose-letter.png) [A07](m4-s4-screens/ssh-80x24/A07-typing-a-line.png) [A08](m4-s4-screens/ssh-80x24/A08-letter-ready.png) [A09](m4-s4-screens/ssh-80x24/A09-letter-sent.png) | [A06](m4-s4-screens/ssh-132x43/A06-compose-letter.png) [A07](m4-s4-screens/ssh-132x43/A07-typing-a-line.png) [A08](m4-s4-screens/ssh-132x43/A08-letter-ready.png) [A09](m4-s4-screens/ssh-132x43/A09-letter-sent.png) |
| A10 pacts | [png](m4-s4-screens/ssh-80x24/A10-pacts.png) | [png](m4-s4-screens/ssh-132x43/A10-pacts.png) |
| B01, B02 day 8: the morning report (the trade and the letter) and the letter read | [B01](m4-s4-screens/ssh-80x24/B01-bob-morning-report.png) [B02](m4-s4-screens/ssh-80x24/B02-bob-reads-the-letter.png) | [B01](m4-s4-screens/ssh-132x43/B01-bob-morning-report.png) [B02](m4-s4-screens/ssh-132x43/B02-bob-reads-the-letter.png) |
| B03 to B06 the gate: the founder's report, the target, a turn of the water on foot, the raid sent | [B03](m4-s4-screens/ssh-80x24/B03-founder-morning-report.png) [B04](m4-s4-screens/ssh-80x24/B04-the-gate.png) [B05](m4-s4-screens/ssh-80x24/B05-the-gate-on-foot.png) [B06](m4-s4-screens/ssh-80x24/B06-raid-sent.png) | [B03](m4-s4-screens/ssh-132x43/B03-founder-morning-report.png) [B04](m4-s4-screens/ssh-132x43/B04-the-gate.png) [B05](m4-s4-screens/ssh-132x43/B05-the-gate-on-foot.png) [B06](m4-s4-screens/ssh-132x43/B06-raid-sent.png) |
| B07 the league by renown | [png](m4-s4-screens/ssh-80x24/B07-league.png) | [png](m4-s4-screens/ssh-132x43/B07-league.png) |
| C01 the morning after the raid: the raider's report | [png](m4-s4-screens/ssh-80x24/C01-founder-reads-the-night.png) | [png](m4-s4-screens/ssh-132x43/C01-founder-reads-the-night.png) |
| C02 to C04 the league with the renown breakdown, the hall, the post | [C02](m4-s4-screens/ssh-80x24/C02-league.png) [C03](m4-s4-screens/ssh-80x24/C03-hall.png) [C04](m4-s4-screens/ssh-80x24/C04-post-after-the-raid.png) | [C02](m4-s4-screens/ssh-132x43/C02-league.png) [C03](m4-s4-screens/ssh-132x43/C03-hall.png) [C04](m4-s4-screens/ssh-132x43/C04-post-after-the-raid.png) |
| C05 to C07 the defender's report, pacts (closed: a recent raid), the post | [C05](m4-s4-screens/ssh-80x24/C05-bob-reads-the-night.png) [C06](m4-s4-screens/ssh-80x24/C06-bob-pacts.png) [C07](m4-s4-screens/ssh-80x24/C07-bob-post.png) | [C05](m4-s4-screens/ssh-132x43/C05-bob-reads-the-night.png) [C06](m4-s4-screens/ssh-132x43/C06-bob-pacts.png) [C07](m4-s4-screens/ssh-132x43/C07-bob-post.png) |

## Not walked, and what was not exercised

- **The ASCII tier and colour off over ssh.** The snapshot tests render every social screen in the ASCII tier at 80x24, 100x30 and 132x43 and check it is ASCII only; the ssh walk used the Unicode tier with colour.
- **A raid that wins, a pact made and broken, a block, a hold with a sluice, a season ending.** Covered by `tests/social.rs` (through the framework's host and store, with a replay of each visit) and not by this walk.
- **Telnet**, a terminal that is not xterm-like, a truecolor terminal (the board sends 16 colours) and a screen reader driving the look-around (the text is tested, not heard).
- **Roomy and Grand layouts over a real window**: the classic layout limits the door to 74 columns, so they are drawn by the tests only (as in S3).
- **The Postgres suites** (this slice touches no store code).
- Two accounts is the smallest case. The caps against a crowd (a hold struck twice a day, a clan flooded with letters) are exercised by `tests/social.rs` and the 12-clan sim, not by more ssh sessions.
