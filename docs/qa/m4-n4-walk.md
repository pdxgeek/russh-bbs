# M4-N4 walk: Nullroute's crews and bounties over real ssh, two accounts

Date: 2026-11-06. Branch `m4/nullroute-social`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a fresh board, `registration_mode = open`) claimed with the OpenSSH 9.6 client, then a second account (`bob`) created with `ssh new@host`; both then signed in as themselves with `ssh -tt` in a 92x25 pty (the board sends 16 colours; the door gets the box the classic layout leaves it). The script is `docs/qa/m4-n4-screens/n4walk.py` (with `nr.py`; the terminal helper is the S3 walk's) and is kept so the walk can be repeated: `python3 n4walk.py day0|day1 PORT FOUNDER_KEY BOB_KEY OUT`. What a terminal emulator (`pyte`) showed after each step is in the `.txt` files; the same screen with the colours the server sent is drawn to a PNG by `tools/screenshots`.

The second day needs the clock moved, so the board was stopped and started again on the same database under `faketime -f "+1d"`: **day 0** (shop, deck, a crew founded, a run played, collected) and **day 1** (a second run, the leader leaves the crew, a bounty, the street).

## What the walk found out

- **A crew run works over ssh as the tests say.** `founder` founded the crew `NightShift`, invited `bob`, opened a run on a contract from the brief (`g`); `bob` accepted and joined (`j`); the leader started it (`s`); both entered (`e`). The map is shared: `bob` opened it and saw **`F founder at AC-6`** on the admin console, with the founder's initial on the node (`B14`).  The shared trace is covered by `tests/crew.rs`; in the walk `bob`'s bar read 0 when he opened the run before moving (`B14`) and 2 after he had walked (`B15`), which is his own share, not a view of the founder's: a mate's trace joins the replay when the log is reopened. `bob` planted; the founder came back and jacked out; the run ended.
- **The debrief shows the crew's split** (`B20`, `B22`): the job paid 53 and the crew's total was 90 (170 percent for two); the founder took 30 marks for 1 point and `bob` 60 for 4. On day 1 a second run (job 62, total 105) split 76 to the founder, who planted, and 28 to `bob`. A lookout guarded the firewall on day 1 (`C06`): the founder put it down with the dictionary script, and `bob` walked through (`C09`).
- **The bounty.** A crewmate cannot be a target: with `bob` in the crew the rivals screen said *Not now: they are in your crew* (`C16`). The founder left the crew (`C17`), then posted a bounty from the rivals screen with a note typed over ssh (`C19`). The 100 marks staked appear as **80** on the board (a 20 percent fee), shown to the founder as *A fixer holds 100 marks and has put a price on bob* (`C20`). `bob`'s next visit began with **A fixer has put a price on your head: 80 marks** (`C21`), and the street's bounty page lists it with the poster's handle and the note (`C24`): the poster is public on the street by design, the in-fiction "fixer" is the cover; nothing is labelled private. The crew `NightShift` then showed on the street's crew page as one member, led by `bob` (`C25`: leadership passed on).
- **Rules the walk ran into, as designed:** a pilot must have visited once before a crew can invite them ("there is no such invitation" for a name the board has not seen: `bob` first visited, then was invited); a bounty needs 100 marks (the starter's 90 were not enough, so the first run was part of the walk); the starter's two jobs said *no tool* until each pilot bought an **implant** (61 marks) and swapped it into the deck for the siphon (`A02`, `A03`); crew names are one handle-shaped word (`Night Shift` with a space was refused: "not a name the board allows"). **The board's own limit of five sign-ins a day** stopped a first attempt, so the walk uses at most four sessions per account a day.
- **No security marker, no lock.** Nothing on the social screens is labelled private; the rivals screen says *Nothing here is private: the board can read every post*.

## Captures

`ssh-92x25/` holds `NAME.png` and `NAME.txt` for every stop (51 pictures, about 2 MB):

| Stop | Screens |
|---|---|
| A00 to A03 first visit: home, the market (an implant), the deck | `A00-bob-home` `A01-founder-home` `A02-market-buy-an-implant` `A03-deck-implant-for-siphon` |
| B01 to B07 the crew screen, naming, founded, an invitation, the contracts, the brief and its gather key, the run opened | `B01` to `B07` |
| B08 to B10 `bob` sees the invitation, joins the crew, joins the run | `B08-bob-sees-the-invitation` `B09` `B10` |
| B11 to B17 the run starts, the founder jacks in and walks to the target, **`bob` sees the founder on the shared map**, follows, plants, jacks out | `B11` to `B17` |
| B18 to B23 the run over; **the debrief with the crew's split** for each; home after | `B20-founder-debrief-crew-split` `B22-bob-debrief-crew-split` |
| C01 to C11 next morning: contracts, a second run opened and joined, the lookout, the plant | `C06-founder-the-lookout-stands-in-the-way` `C09-bob-follows-the-founder-is-on-the-map` `C11-founder-plants` |
| C12 to C15 the second debrief and marks | `C12` `C14` |
| C16 to C20 **the bounty**: refused while crewmates, leaving the crew, the rivals list, the note typed, posted | `C16` to `C20` |
| C21 to C25 **`bob` is told**, rivals, the street's pages (word, bounties, crews) | `C21-bob-home-told-of-the-bounty` `C24-bob-street-bounties` `C25-bob-street-crews` |

## Not walked, and what was not exercised

- **Hacks, honeypots, the rival list's heat, a bounty paid out, arcs and endings, the finale.** The bounty is posted and shown but its payout needs a hack that gets in, and new characters are safe for seven days; those, the arcs, the seasons and the finale are covered by `tests/social.rs`, `tests/crew.rs`, `tests/story.rs` and the balance harness, not by this walk.
- **A third crew member**, a warden waking on a crew run, and a crew run that burns.
- **The ASCII tier and colour off over ssh**; the snapshot tests render every screen in the ASCII tier at three sizes. **Roomy and Grand layouts over a real window** (the classic layout limits a door's box), **telnet**, a truecolor terminal, **the Postgres suites** (this slice touches no store code).
- A crew mate's moves show after reopening the run, not live (FW-1 has not landed); the walk reopens the door between turns, as a player would.
