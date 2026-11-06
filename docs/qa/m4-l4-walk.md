# M4-L4 walk: Long Haul's social layer over real ssh, two accounts

Date: 2026-11-06. Branch `m4/long-haul-social`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a fresh board for each of the two passes, `registration_mode = open`) claimed with the OpenSSH 9.6 client (`ssh claim@host`, the token from the console), a second account (`bob`) created with `ssh new@host`, then both signed in with `ssh -tt` in a pty (80x24 in one pass, 132x43 in the other; the board sends 16 colours). Member limits for calls and minutes a day were raised in the scratch database, as in the earlier walks. The scripts are in `m4-l4-screens/` (`fresh.sh`, `restart.sh`, `phaseA.py`, `phaseB.py`, `phaseC.py`, `lib.py`; they reuse the terminal helper of the S3 walk). What a terminal emulator (`pyte`) showed after each step is in the `.txt` files and the same screen with the colours the server sent is the `.png`.

The board's clock was moved with `faketime` (the server stopped and started again on the same database): **day 0** (both captains sign on, a convoy, notes, colours), **+22 days** (the founder borrows, flies to a charted system with no station and pledges 12,000 credits) and **+23 days** (the nightly tick has opened the station; the founder docks and reads its market, bob reads the Outposts tab).

## What the walk showed

- **A convoy over ssh, two accounts.** The founder formed the *Patient Caravan* (a name from the tables), posted quick note 3 and typed a line (`hold at the next dock, bob`, sent with Enter through `Ctx::player_text`); bob, on his own account, saw the open convoy with the founder in it, joined with Enter and read both lines in the log (`A04` to `A06`, `A12`, `A13`).
- **Opting in is public and says so.** Both raised the colours; the Piracy tab then listed the other captain with their strength and worth band (`A14`). The screens say *The server can read all of this*; no lock or "private" appears anywhere on them. The hunt itself was **not** walked (it needs a gun from the yard, a day of colours and the night): it is covered by `tests/social.rs` and `tests/piracy.rs`.
- **An idle convoy is dropped.** After 22 days nobody had flown, so the convoy was gone when the founder came back (`B01`, *Your convoy has broken up*). That is the ten-day rule working; it was not planned as a screenshot.
- **An outpost, start to finish.** Nothing opens early (`A09` says outposts open on day 21). At +22 days the founder borrowed 15,000 credits at the loan office (`B02`), undocked, moved the chart cursor onto Kestrel Hold (a charted system with no station, `B03`), planned and flew the leg (`B04`, `B05`, a 4-day jump), and pledged 5,000 twice and 500 four times (`B07`, `B08`: *site at Kestrel Hold: 12,000 of 12,000 cr*). After the night the station existed: **founder Landing**, a farming station at level 1 (`C02`), the founder docked there and read its market (`C03`, `C04`: stock, prices and roles like any station), and bob, who never went near it, saw it in his Outposts tab (`C08`). The shared map had changed for both.
- **The Hall** listed the founder as the richest captain from the daily records (`C05`).
- **A door in an 80x24 window gets 74 columns by 20 rows** (as the earlier walks measured); the society screen fits it, with the keys line wrapping to two lines. At 132x43 the width is the same and the height grows.

## Captures

Each folder has `NAME.png` and `NAME.txt` for these stops:

| Stop | 80x24 | 132x43 |
|---|---|---|
| A01-founder-intro | [png](m4-l4-screens/ssh-80x24/A01-founder-intro.png) | [png](m4-l4-screens/ssh-132x43/A01-founder-intro.png) |
| A02-founder-bridge | [png](m4-l4-screens/ssh-80x24/A02-founder-bridge.png) | [png](m4-l4-screens/ssh-132x43/A02-founder-bridge.png) |
| A03-founder-society-no-convoy | [png](m4-l4-screens/ssh-80x24/A03-founder-society-no-convoy.png) | [png](m4-l4-screens/ssh-132x43/A03-founder-society-no-convoy.png) |
| A04-founder-forms-a-convoy | [png](m4-l4-screens/ssh-80x24/A04-founder-forms-a-convoy.png) | [png](m4-l4-screens/ssh-132x43/A04-founder-forms-a-convoy.png) |
| A05-founder-typing-a-line | [png](m4-l4-screens/ssh-80x24/A05-founder-typing-a-line.png) | [png](m4-l4-screens/ssh-132x43/A05-founder-typing-a-line.png) |
| A06-founder-line-sent | [png](m4-l4-screens/ssh-80x24/A06-founder-line-sent.png) | [png](m4-l4-screens/ssh-132x43/A06-founder-line-sent.png) |
| A07-founder-piracy-before | [png](m4-l4-screens/ssh-80x24/A07-founder-piracy-before.png) | [png](m4-l4-screens/ssh-132x43/A07-founder-piracy-before.png) |
| A08-founder-colours-up | [png](m4-l4-screens/ssh-80x24/A08-founder-colours-up.png) | [png](m4-l4-screens/ssh-132x43/A08-founder-colours-up.png) |
| A09-founder-outposts-early | [png](m4-l4-screens/ssh-80x24/A09-founder-outposts-early.png) | [png](m4-l4-screens/ssh-132x43/A09-founder-outposts-early.png) |
| A10-founder-hall | [png](m4-l4-screens/ssh-80x24/A10-founder-hall.png) | [png](m4-l4-screens/ssh-132x43/A10-founder-hall.png) |
| A11-founder-journal | [png](m4-l4-screens/ssh-80x24/A11-founder-journal.png) | [png](m4-l4-screens/ssh-132x43/A11-founder-journal.png) |
| A12-bob-sees-the-open-convoy | [png](m4-l4-screens/ssh-80x24/A12-bob-sees-the-open-convoy.png) | [png](m4-l4-screens/ssh-132x43/A12-bob-sees-the-open-convoy.png) |
| A13-bob-joined-and-reads-the-log | [png](m4-l4-screens/ssh-80x24/A13-bob-joined-and-reads-the-log.png) | [png](m4-l4-screens/ssh-132x43/A13-bob-joined-and-reads-the-log.png) |
| A14-bob-colours-up-roster | [png](m4-l4-screens/ssh-80x24/A14-bob-colours-up-roster.png) | [png](m4-l4-screens/ssh-132x43/A14-bob-colours-up-roster.png) |
| B00-founder-after-three-weeks | [png](m4-l4-screens/ssh-80x24/B00-founder-after-three-weeks.png) | [png](m4-l4-screens/ssh-132x43/B00-founder-after-three-weeks.png) |
| B01-founder-convoy-gone-idle | [png](m4-l4-screens/ssh-80x24/B01-founder-convoy-gone-idle.png) | [png](m4-l4-screens/ssh-132x43/B01-founder-convoy-gone-idle.png) |
| B02-founder-borrows-for-the-outpost | [png](m4-l4-screens/ssh-80x24/B02-founder-borrows-for-the-outpost.png) | [png](m4-l4-screens/ssh-132x43/B02-founder-borrows-for-the-outpost.png) |
| B03-chart-cursor-on-a-charted-system-with-no-station | [png](m4-l4-screens/ssh-80x24/B03-chart-cursor-on-a-charted-system-with-no-station.png) | [png](m4-l4-screens/ssh-132x43/B03-chart-cursor-on-a-charted-system-with-no-station.png) |
| B04-route-planner | [png](m4-l4-screens/ssh-80x24/B04-route-planner.png) | [png](m4-l4-screens/ssh-132x43/B04-route-planner.png) |
| B05-arrived | [png](m4-l4-screens/ssh-80x24/B05-arrived.png) | [png](m4-l4-screens/ssh-132x43/B05-arrived.png) |
| B06-outposts-before-pledge | [png](m4-l4-screens/ssh-80x24/B06-outposts-before-pledge.png) | [png](m4-l4-screens/ssh-132x43/B06-outposts-before-pledge.png) |
| B07-pledged-ten-thousand | [png](m4-l4-screens/ssh-80x24/B07-pledged-ten-thousand.png) | [png](m4-l4-screens/ssh-132x43/B07-pledged-ten-thousand.png) |
| B08-pledged-twelve-thousand | [png](m4-l4-screens/ssh-80x24/B08-pledged-twelve-thousand.png) | [png](m4-l4-screens/ssh-132x43/B08-pledged-twelve-thousand.png) |
| B09-bob-sees-the-site | [png](m4-l4-screens/ssh-80x24/B09-bob-sees-the-site.png) | [png](m4-l4-screens/ssh-132x43/B09-bob-sees-the-site.png) |
| C01-founder-the-morning-after | [png](m4-l4-screens/ssh-80x24/C01-founder-the-morning-after.png) | [png](m4-l4-screens/ssh-132x43/C01-founder-the-morning-after.png) |
| C02-founder-the-outpost-is-open | [png](m4-l4-screens/ssh-80x24/C02-founder-the-outpost-is-open.png) | [png](m4-l4-screens/ssh-132x43/C02-founder-the-outpost-is-open.png) |
| C03-founder-docked-at-the-new-station | [png](m4-l4-screens/ssh-80x24/C03-founder-docked-at-the-new-station.png) | [png](m4-l4-screens/ssh-132x43/C03-founder-docked-at-the-new-station.png) |
| C04-founder-the-new-station-market | [png](m4-l4-screens/ssh-80x24/C04-founder-the-new-station-market.png) | [png](m4-l4-screens/ssh-132x43/C04-founder-the-new-station-market.png) |
| C05-founder-the-hall | [png](m4-l4-screens/ssh-80x24/C05-founder-the-hall.png) | [png](m4-l4-screens/ssh-132x43/C05-founder-the-hall.png) |
| C06-bob-the-morning-after | [png](m4-l4-screens/ssh-80x24/C06-bob-the-morning-after.png) | [png](m4-l4-screens/ssh-132x43/C06-bob-the-morning-after.png) |
| C07-bob-convoy-after-three-weeks | [png](m4-l4-screens/ssh-80x24/C07-bob-convoy-after-three-weeks.png) | [png](m4-l4-screens/ssh-132x43/C07-bob-convoy-after-three-weeks.png) |
| C08-bob-sees-the-outpost-too | [png](m4-l4-screens/ssh-80x24/C08-bob-sees-the-outpost-too.png) | [png](m4-l4-screens/ssh-132x43/C08-bob-sees-the-outpost-too.png) |

## Not walked, and what was not exercised

- **A hunt, a raid and its report; the Journal with an arc under way.** Covered by `tests/social.rs`, `tests/piracy.rs`, `tests/arcs.rs` and the season simulation, not by this walk.
- **The ASCII tier and colour off over ssh.** The snapshot tests render every society tab in the ASCII tier at three sizes and check it is ASCII only; the walk used the Unicode tier with colour.
- **Telnet**, Postgres, a long-running server across real midnights (the clock was moved), a terminal that draws `▲ ●` double width, and more than two accounts.
