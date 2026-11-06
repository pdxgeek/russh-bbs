# M4-S5 walk: Saltmarch's boards, journal, toll setting and away crew over real ssh, two accounts

Date: 2026-11-06. Branch `m4/saltmarch-content`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, a fresh SQLite board, `registration_mode = open`) claimed with the OpenSSH 9.6 client (`founder`), a second account created with `ssh new@host` (`bob`); both then signed in with `ssh -tt` in an 80x24 pty. The script is `docs/qa/m4-s5-screens/walk.py` (`python3 walk.py A|B|C PORT FOUNDER_KEY BOB_KEY OUT [COLSxROWS]`; it reuses the S3 terminal helper and `tools/screenshots/shot.py` for the PNGs). The board's clock was moved with `faketime`: **day 0** (phase A), **+8d** (B) and **+9d** (C) on the same database. What a terminal emulator (`pyte`) showed is in the `.txt` files next to each PNG in `m4-s5-screens/ssh-80x24/`.

## What the walk showed

- **The boards and the journal are rooms of the Tidecourt** (A01, A02, A04). The boards show the clan's standing with each faction (all Stranger, 0, on day 0), what is in hand and the four offers of the day with pay and standing; taking one (A03) puts it in hand, with the poster's words, the days left and the progress. The journal says no chain has begun (A04, C03).
- **The toll setting** is an order at the Tidecourt: two presses moved it from the default 2 to 4 tallies (A05).
- **Bob sees his own board** (A07); the day's offers come from the season's seed and the clan's standing.
- **The crew screen shows the party away** (B02, the S4 open item): after the founder sent a raid at a low water (B01), two crew are marked `»` in the roster and the detail pane says *Away on a raid: back with the report*.
- **The night's reports** (C01, C04): the raid was beaten off, two injuries are named, and the post is empty; the league (C02) shows the renown line with the new `work` part (0 here: no contract finished, no chain, no causeway).

## Captures (80x24, 16 colours, Unicode tier)

| Stop | Picture |
|---|---|
| A01 the Tidecourt with the new rooms | [png](m4-s5-screens/ssh-80x24/A01-tidecourt-with-the-boards.png) |
| A02 the boards: standing and today's offers | [png](m4-s5-screens/ssh-80x24/A02-boards-standing-and-offers.png) |
| A03 a contract taken | [png](m4-s5-screens/ssh-80x24/A03-a-contract-taken.png) |
| A04 the journal | [png](m4-s5-screens/ssh-80x24/A04-journal.png) |
| A05 the causeway toll set | [png](m4-s5-screens/ssh-80x24/A05-causeway-toll-set.png) |
| A06 the planner | [png](m4-s5-screens/ssh-80x24/A06-planner.png) |
| A07 bob's boards | [png](m4-s5-screens/ssh-80x24/A07-bob-boards.png) |
| B01 the raid sent | [png](m4-s5-screens/ssh-80x24/B01-raid-sent.png) |
| B02 the crew with the party away | [png](m4-s5-screens/ssh-80x24/B02-crew-with-the-party-away.png) |
| C01 to C04 the reports, the league with `work` in renown, the journal, bob's report | [C01](m4-s5-screens/ssh-80x24/C01-founder-reads-the-night.png) [C02](m4-s5-screens/ssh-80x24/C02-league-with-work-in-renown.png) [C03](m4-s5-screens/ssh-80x24/C03-journal-after.png) [C04](m4-s5-screens/ssh-80x24/C04-bob-reads-the-night.png) |

## Not walked, and what was not exercised

- **A causeway laid and crossed.** The walk reached the planner but did not set out with rope and lay a tile, and a second account never crossed a span. Laying, the toll step, the parcel to the builder, the pact term and the overlay are covered by `trip/tests.rs`, `social/dispatch/tests.rs` and `social/ledger` tests, not by ssh.
- **A quest scene, a finished contract, a season ending and the next visit's legacy and heirloom.** Covered by the arc walk (`every_chain_can_be_walked_to_each_of_its_ends_and_each_end_closes_the_others`), `board.rs` and `legacy.rs` tests, not by ssh. The founder's grain levy (3 days) lapsed unseen between phases B and C; the report does not mention it in the capture, which was cut off at the first screen of the report.
- **132x43, the ASCII tier with colour off, telnet**, a truecolor terminal and a screen reader. The snapshot tests render the two new rooms in the ASCII and Unicode tiers at the three sizes.
- Offers are cut with `~` at 80x24 (the lightbar line spells the order in full); nothing is called private and no lock is drawn.
