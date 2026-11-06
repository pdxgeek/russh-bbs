# M4-E4 walk: Emberdeep's social layer over real ssh, two accounts

Date: 2026-11-06. Branch `m4/emberdeep-social`. **What was run:** the real server binary (`target/debug/russh-bbs-server`, SQLite, a scratch data directory, `registration_mode = open`, port 2299) claimed with the OpenSSH 9.6 client (`ssh claim@127.0.0.1`, a throwaway ed25519 key), a second account `bob` made through the waiting room (`ssh new@`, "Create an account"), then both signed in with `ssh -tt` in a pty of 80x24 (the board's box gives a door 74 by 20). The script is `docs/qa/m4-e4-screens/walk.py` (it reuses the S3 walk's terminal helper and draws each stop to a PNG with `tools/screenshots`): `setup`, `party`, `seed`, `rescue`. The board sends 16 colours; the icon check was answered `y` (the Unicode tier).

## What was walked

- **A company by invitation (`party`).** Both accounts made a warden. `founder` opened the Company (`c`), invited `bob`; `bob`'s hub then showed "1 invitation(s)" beside Company, his Company screen listed the invitation, and accepting made the company of two (P08), which the tavern board then announced. In the Duel ring (`u`) both stepped in; the ring's list of foes is empty because the only other ring member is in the company (a company does not duel itself, P12). The Journal (`j`) listed the three lines (two not yet open) and took an epitaph typed over ssh (P13 to P15); the hall's four new tabs were opened (P16 to P19).
- **A rescue (`seed`, `rescue`).** A fall cannot be arranged with keys, so the scratch board's database was given a dead hero (`founder`, fell yesterday on level 3) and a rescuer standing in the same room (`bob`, in a company with them), written with `cargo run -p russh-bbs-door-emberdeep --example rescue_pair`: the same engine builds the bytes, only the dice are missing. `bob` opened the door at the room, saw "founder lies here, dead" and `[1] Carry founder home` at **2 turns** (3 for a stranger), pressed it (R02: "You haul founder onto your back. The town will pay you 45 coin."), and `founder` then opened the door and was home in Cindermere, the board saying "bob carried founder home" (R03).

## What it found

- The walk found nothing wrong with the social rules. It found three things in the walk itself: `make_warden` looked for the wrong case of "new hero" (a script bug); the board's per-day sign-in cap (5) stopped the second pass (the M3 walk raised it in the database; so did this one); and a menu key can be printed as `[1]   Carry` with spaces (the script's pattern now allows it).
- The rescue costs two turns for a companion, as the engine says; the stranger's three and the thanks note are covered by `tests/social.rs`, not this walk.

## Not exercised

- **A duel.** No second ring member outside the company existed on the scratch board; calls, caps and the ladder are covered by `src/tests/social.rs`. The hall's ring tab was walked empty.
- **The thanks note over ssh** (a stranger's rescue), the epitaph reaching the hall (an unfound hero), the Furnace's verdict and any ending: covered by `tests/social.rs` and `src/tests/social.rs`, not walked. No hero reached the Cold Furnace.
- **132x43, the ASCII tier and colour off over ssh, telnet, truecolor.** The snapshot tests render every new screen in both tiers at 80x24, 100x30 and 132x43; the walk is 80x24, Unicode tier, 16 colours.
- Postgres: this slice touches no store code.
