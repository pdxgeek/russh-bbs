# Nullroute: a hacking door (design)

Status: **Engine implemented (M4-N1, merged); countermeasures, encounters, coding and script degradation implemented (M4-N2, merged); the UI implemented (M4-N3, in review)**; the rest is **Proposed**. Shared platform rules are in [README.md](README.md). Implementation notes are at the end.

## Pitch

The city runs on private networks owned by a dozen corporations, and you are a freelancer with a secondhand rig and a reputation to build. Each day you take contracts from fixers: steal a file, plant a backdoor, wipe a record, ransom a server room. You break in through a map of nodes, fight countermeasures with scripts you wrote or bought, and get out before the **trace** reaches your rig. Success buys parts, scripts and a name. Getting caught costs you gear, heat and days.

The core tension: every action inside a network moves the trace, and the deeper the prize, the faster it climbs.

## Daily loop

1. **Inbox:** contracts from fixers, market prices, your heat level, and messages from your crew.
2. **Spend cycles.** Each day you get **cycles** (default 40). Runs, rig work, coding scripts and laying low all cost cycles.
3. **Runs:** pick a contract, load your deck (up to 6 scripts), jack in, and work the node map.
4. **Night:** corporations patch holes, heat decays, your bots earn passive income, and new contracts arrive.

## Systems

### Networks (procedural)

- Each target is a **node graph** generated from the corporation's profile and the contract's tier: entry points, routers, firewalls, data vaults, admin consoles, cameras, door locks, and honeypots.
- **Node properties:** security level, what it guards, which countermeasures patrol it, and links that are hidden until scanned.
- **Corporations differ in style:** one stacks firewalls, one runs aggressive tracers, one hides everything behind decoys, and one is badly configured but watched closely by humans.
- A network is **remembered** for the season. Holes you find stay open until the nightly patch rolls the dice, so scouting pays.

### The trace

- A meter from 0 to 100 that climbs with noisy actions, failed scripts and alarms. Countermeasures add more.
- At 100 you are **burned**:
  - you are kicked out;
  - you lose the run's loot;
  - you gain heat, and one rig part is fried.
- Ways to manage the trace: decoy scripts, proxies bought before the run, cutting a camera feed, or a crewmate running interference.

### Scripts (the deck)

- Each script has a cost in memory, a noise level and an effect:
  - **Probe:** scan, map, reveal.
  - **Break:** crack, spoof, overflow.
  - **Hide:** mask, decoy, reroute.
  - **Fight:** jam, fork, wipe.
  - **Utility:** copy, plant, wipe logs.
- **Countermeasures** are the enemies: sentries, tracers, black walls, and the rare **warden**, a corporate AI that hunts you. Each has a pattern you learn.
- **Coding:** combine fragments found on runs into new scripts with a simple module editor (pick an effect, a target type and a modifier). Good builds are a strategy of their own.
- Scripts **degrade** as corporations learn them. A script used too often against one corporation stops working there, which pushes players to vary their decks.

### Encounters

Inside a network, a fight with a countermeasure is a short turn-based exchange. You play scripts from your deck against its pattern while the trace ticks. It's quick (3 to 8 moves) and readable at 80x24.

### Rig and hideout

- **Rig parts:** CPU (cycles per run), memory (deck size), cooling (how much a noisy run fries), deck interface (script speed) and a shield (trace resistance).
- **Hideout:** a bolt-hole that hosts bots (passive income), a workbench (coding), a safe (stash loot) and the fence (sell data).

### Heat and the law

- **Heat** rises with burned runs and high-profile jobs, and decays daily.
- High heat brings raids on your hideout, closes some fixers to you, and raises proxy prices.
- You can lay low (spend cycles, gain nothing, lose heat), or have a crewmate take the heat for a cut.

### Crews and rivals

- **Crews:** form a crew of up to 5 players. Crew runs let several players work one network together, each taking a branch of the map, with a shared trace meter. They run asynchronously across the day.
- **Rivals:** set **honeypots** in your hideout to catch other players. Rival players can hack your hideout's systems to steal stashed data, costing them cycles. A hideout has ICE-free protection for its first week, and attacks are capped daily.
- **Bounties:** a fixer pays out for burning a named rival's rig, but only rivals at your tier or above.

### Story

- **Arcs:** five hand-written arcs, one per major fixer, each 5 to 8 contracts long. Each ends with a choice of whom to betray.
- **Season finale:** the season's centrepiece is a heist on the one network no one has cracked, built to need crews.
- **The Hall:** past seasons, the best runs (the fewest trace points used on the hardest network), and burned legends.

## UI

- **Node map:** nodes drawn as a graph that fits 80x24, with the trace meter as a bar on the top row. ASCII tier: boxes and lines.
- **Run log:** a scrolling feed of what happened, drawn in a terminal-within-a-terminal style that suits the genre.
- **Deck and fight panel:** the deck as cards on the bottom rows, the countermeasure's pattern and intent above.
- **Look around:** a text list of the visible nodes, their links and known properties.

## Content budget (launch)

- **Corporations:** 12, each with a style and a home network template. 30 node types, 20 countermeasures and 3 wardens.
- **Scripts:** 60 to start, plus 30 fragments for coding.
- **Contracts:** 80 contract templates and 5 fixer arcs (about 35 contracts in all).
- **Rig:** 40 rig parts and 15 hideout upgrades.

## Proposed slices

1. **Engine:** network generation, the trace, the deck and runs, with replay tests.
2. **Countermeasures and coding:** encounters, script degradation and the module editor.
3. **UI:** node map, run log and deck panel, plus look-around.
4. **Social:** crews, honeypots, bounties and heat.
5. **Content pass:** fixer arcs, plus balance by simulation.

## Open questions

- **Cycles:** 40 cycles a day?
- **Player-versus-player hacks:** on by default?
- **Tone:** a gritty near-future city, or something stranger?

## Implementation notes (M4-N1: the engine)

Decision: [D102](../decisions.md). Crate: `crates/doors/nullroute` (`russh-bbs-door-nullroute`), registered in `Doors::builtin()`.

### What is implemented and verified

| Area | Implemented | Verified by |
|---|---|---|
| Procedural networks | Layered graph per (season, corporation, tier): entry, `tier + 1` mid layers, an objective floor (vault, admin console, server room) and, by corporation, a decoy vault. Hidden links, locks in three families, sentries, tracers, black walls (tier 2+), live cameras. Regenerated, never stored | `tests/net.rs`: determinism; property tests (400 cases each) for shape, connectivity, security cap, link ranges, unique names, decoys only as vaults, the full toolkit reaching every objective, the fairness guarantee per tier, the starter scripts opening tier 1, holes only widening and alert only narrowing reach |
| Corporation styles | Five corporations: firewall-heavy (Kestrel), aggressive tracers (Halcyon), decoys and hidden links (Mirrorglass), badly configured but watched (Pennywhistle), black walls (Orrery) | `corporations_differ_in_style` counts the differences over 60 seasons |
| The trace and being burned | 0 to 100; climbs with node noise, script noise, failed breaks, patrols, a live camera, a tracer, the alarm, the corporation's watchfulness, the corporation and the tier; falls with a shield, a mask, proxies, decoys, reroutes. At 100: burned | `tests/run.rs` (22 tests) |
| The deck | Up to 6 scripts in the rig's memory; 27 scripts in 5 families; charges; costs in cycles; skill gates | `tests/run.rs`, `tests/player.rs` |
| Runs | Move, scan, map, break, jam, purge, fork, mask, decoy, reroute, copy, plant, shred, cut feed, scrub logs, jack out | `tests/run.rs` |
| Cycles per day | 40 plus 4 per CPU level; jacking in costs 5 | `tests/player.rs` |
| Rig and hideout | Five parts, three levels each, fried parts count as stock until repaired; bots, workbench (no use yet), fence (3 levels), safe, proxies, laying low | `tests/player.rs` |
| Heat and its decay | Added by jobs and burns, falls 4 a night and 12 for 8 cycles laid low; closes tier-3 jobs at 60 and tier-2 at 85; raises proxy prices at 30; raids the hideout at 50 and up (never the safe) | `tests/player.rs` |
| Nightly tick | World half in the tick (patching, alert cooling, market, contracts); player half at the player's next visit (bots, heat, raids), at most 7 nights, seeded so it cannot be rerolled | `tests/world.rs`, `tests/player.rs`, `tests/play.rs` |
| Saves | Hand-written binary, version 1, every length and range checked, pinned byte layouts, no migration yet (nothing older exists) | `tests/world.rs` |
| Playable screen | Contracts, briefing, deck, shop, rig, hideout, the run (node list, trace bar, deck, log), debrief; fits the door area (72 by 19 at 80x24) with the key hints always in view; the same text is the look-around view | `tests/play.rs`; a walk with the OpenSSH client (below) |
| Sandbox | The framework's source audit passes; the manifest has the framework, `serde` and `toml` only (`proptest` and `tokio` are dev-dependencies) | `tests/sandbox.rs` |

### The RPG rule, as implemented

Every element below changes what can happen, and each has a test:

- **Deck against locks.** A lock opens only to its own family at power at least its security. `on_a_known_network_each_deck_reaches_a_different_set_of_nodes` pins the reachable set for six decks on one network; `across_generated_networks_one_family_often_opens_what_another_does_not` shows it on generated tier-3 networks (at least 90 percent differ).
- **Memory, slots and the first deck.** The five starter scripts weigh 9 and a stock rig holds 8, so the first deck is a choice; a schema test pins it.
- **Skills grow by use.** Each family's skill rises with successful uses (thresholds 4, 12, 30, 60, 100) and the best scripts will not load below skill 3. Skill also adds 3 percent success a level.
- **Scouting pays, and the world remembers.** A broken lock is a hole for every player until the nightly patch (by the corporation's diligence); completing an objective raises the corporation's alert (every security level plus one, so a starter deck is shut out of a firewall for a day); a planted backdoor gives later runs a trace buffer.
- **Choices close doors.** A job raises the hirer's standing and lowers the target's patron; at -3 that fixer will not deal with you. Tier 3 wants reputation 40 and standing 2.
- **Consequences persist.** Burns fry a part (cooling lowers the chance from 80 to 20 percent), heat follows you, a dropped line costs heat.
- **Not built yet:** several endings, branch coverage in the balance harness, and the story arcs (N4).

### Numbers: the headless 30-day simulation

`cargo test -p russh-bbs-door-nullroute --test sim -- --nocapture`. A bot plays the engine API like a person (it scouts, jams, breaks, buys what a sensible player buys, one in three plays recklessly). 12 players share a world for 30 days, three seeds:

| Seed | Runs | Done | Burned | Empty-handed | Earned (marks/player/day) | Final rig (of 15) | Final heat |
|---|---|---|---|---|---|---|---|
| 1 | 725 | 492 | 22 | 211 | 111 | 8.6 | 60 |
| 2 | 790 | 641 | 13 | 136 | 147 | 11.8 | 65 |
| 3 | 766 | 561 | 4 | 201 | 128 | 10.8 | 48 |

About 2.2 runs per player a day (contracts, not cycles, are the limit once the board is shared). Tier 1: 80 to 85 percent done, under 1 percent burned. Tier 2: 34 to 65 percent done, up to 9 percent burned. **Tier 3 is never played by the bot in 30 days** (it needs reputation 40, standing 2 and 60-power scripts that the bot buys last); that is a balance finding for N4, not a result. Money arrives at about 110 to 150 marks a day against about 3,000 for the upgrades the bot buys, so a rig is a month's work.

The framework's own `Simulation` also runs six screen-reading bots for 30 days through the real store, tick and save path (`tests/play.rs`): 180 visits, about 2,500 keys, 150 runs, 114 jobs, 114 breaches in the world. A save is under 400 bytes even at its maximum (`a_save_stays_far_under_the_framework_cap`), and the world under 4 KB after a month of breaches.

### What was walked over a real ssh client

OpenSSH 9.6 against the built server on a fresh SQLite board, at 80x24: claim the board, Doors, open Nullroute; contracts and a briefing; buy a proxy; jack in; scan, move, break a lock, walk to a vault, copy, jack out; the debrief; leave and come back with marks, cycles, skills and standing saved; the shop (buy a script), the hideout; start a run and kill the connection, then reconnect and see the dropped-line penalty. Not exercised: telnet by hand (the screen is shown identical on both transports by a test, and the framework walks both), a real day boundary on a long-running server (the tick is tested with a manual clock, and it ran at start-up), Postgres (this crate adds no storage of its own), colour and the Nerd Font tiers (the screen uses only the framework's styles and no symbols).

### Seams left for the next slices

- **N2.** Done; see the next section.
- **N3.** `session.rs` is the only screen code and holds no rules (the fight and workbench screens are in it too); `Run::view`/`NodeView` is the knowledge-respecting view of a node for a map. `look_around` is the same text.
- **N4.** Crews need a shared run (`Run` is per player today); honeypots at hideouts need a per-player world record; fixer arcs build on `Player::standing` and `Contract`; the autoplay harness reports ends of runs by tier and is the base for the branch-coverage report.
- **Content still to write (design budget):** 7 more corporations, 21 more node types, 33 scripts, 30 fragments, about 70 more contract templates, 5 fixer arcs, 25 more rig parts and about 12 more hideout upgrades. The tables and the schema tests are in place; the content is data.


## Implementation notes (M4-N2: countermeasures, encounters, coding, degradation)

Decision: [D111](../decisions.md). Same crate; saves and world are version 2 (N1 saves and worlds migrate forward). The screen is still minimal: N3 builds on UI-5.

### What is implemented and verified

| Area | Implemented | Verified by |
|---|---|---|
| Countermeasures | 20 patrols (7 sentries, 6 tracers, 7 black walls) and 3 wardens as TOML (`countermeasures.toml`): integrity, armor, strike, a weak lock family, tags, a cycle of intents (probe, strike, charge, guard, seal, trace, mend, counter). Which machine patrols a node is a pure function of the corporation, tier and node, so nothing is stored | `tests/content.rs` (schema, counts, a broken edit is refused), `tests/encounters.rs` |
| Corporation styles | `cm_bias` tags triple the weight of matching machines; each corporation names its warden. Kestrel: armored; Halcyon: trace, swift; Mirrorglass: deceptive, mend; Pennywhistle: watcher, slow; Orrery: mend, armored | `corporation_styles_shape_the_mix_of_countermeasures`, `corporation_styles_point_at_real_tags_and_real_wardens` |
| Encounters | Entering a sentry or tracer node, or purging a black wall, starts a fight. The machine telegraphs its beat; you play a script (Break, Jam, Purge hurt it; Fork, Mask, Decoy, Reroute defend; Scan and Map read its pattern), brace for a cycle, or break off. Each move costs one cycle; script noise is halved in a fight. A fight is 3 to 8 moves: no hit takes more than a third of the integrity, and on the eighth move the machine throws you out. Breaking off costs trace and returns you to the previous node; the damage and what you saw stay | `tests/encounters.rs` (20 tests, two properties of 300 cases) |
| Learnable patterns | The current beat is always shown; the next three are shown once you have watched a full cycle or scanned it; the cycle starts on a beat chosen by the node, so a machine behaves the same every time you meet it | `a_pattern_is_learned_by_watching_or_by_scanning`, `the_beats_do_what_the_telegraph_says` |
| Wardens | Tier 3 runs get one 55 percent of the time (tier 2: 15 percent), decided by the character and the contract. It wakes at trace 55 or on an alarm, arrives three moves later wherever you are, and returns one move after you break off. It is hardened: **only a script tuned to wardens hurts it** | `no_stock_script_hurts_a_warden_and_a_tuned_one_does`, `a_coded_script_gets_past_a_warden_a_stock_deck_cannot` |
| Coding | The workbench (N1) now works: 30 fragments (10 effects, 8 target types, 12 modifiers) found in vaults (15 percent) and dropped by defeated machines. A build is effect + optional target + optional modifier; its numbers are the sum of its fragments and must fit **6 memory and 10 noise**; coding burns the fragments and 4 cycles. 16 coded scripts at most; scrap frees a slot | `tests/coding.rs` (12), `tests/n2play.rs` (a coding session replayed through the screen) |
| Script degradation | The world counts, per corporation and script identity, the runs that used it. 10 runs: wary (power -1, noise +50 percent). 20: patched (refused for free, and the brief says so). Counts fall 2 a night; a new season clears them. The starter scripts are exempt; a polymorphic build is learned half as fast. The same coded recipe is the same identity for everyone | `tests/learning.rs` (6) |
| 60 scripts, 30 fragments | 33 new scripts (nums 28 to 60) including tuned jammers and a tuned purge, pierce, and cheap alternates to the single-copy tools | `tests/content.rs` |
| Saves | Save v2 appends fragments and 16 coded slots; world v2 appends the season and the learned rows (96 at most). Both migrate from v1, refuse corrupt bytes, and a new season's first session clears the old season's holes and learning | `tests/n2play.rs`, `tests/world.rs` |
| Replay | A coding session and 60 days of recorded bot visits, with fights in them, replay to the same save | `tests/n2play.rs` |

### The RPG rule, as implemented in N2

- **Stock decks cannot hurt a warden; a coded one can.** `a_coded_script_gets_past_a_warden_a_stock_deck_cannot` runs the same hunted network six times with a shop-bought deck and with a deck holding one coded script (`crack-core` + `warden-lens` + `piercing`): the stock deck never finishes and never downs the warden, the coded one always does and takes the vault.
- **Degradation forces a different deck.** `a_patched_script_does_nothing_and_a_different_deck_gets_through`: on a level 4 lock, heap-spray opens it fresh, is refused once the corporation has learned it, a deck with root-shell still gets through, and the starter deck never could. A wary corporation turns power 4 into 3 and the lock stays shut.
- **Families, armor and seals.** Each machine is weak to one lock family, walls take half from a break script and double from a purge, a guard beat adds armor, pierce ignores it, a seal locks the family you just used, skill adds half a point a level, and a tuned script adds 3. All pinned in `tests/encounters.rs`.
- **Corporations differ in what they field**, so the deck that suits one network does not suit another.
- **Fairness.** `every_tier_has_a_deck_that_can_win_every_fight_in_it`: the starter breaks win every tier 1 fight, power 4 plus a purge every tier 2 fight, and power 6 every tier 3 fight, from every starting beat, by the plainest play (always the best script), inside eight moves. Content that breaks it fails the test.
- **Not built:** several endings, the branch-coverage report, and the arcs (N4).

### Numbers: the headless 30-day simulation

`cargo test -p russh-bbs-door-nullroute --test sim -- --nocapture`: 12 bots, three seeds, one shared world. The bot now fights from the fight screen's information (braces on a guard, rerouts when hot, plays the script that does the most, breaks off when nothing works), avoids scripts the corporation has patched, buys the alternates and a workbench, and codes what its fragments allow.

| Seed | Runs | Done | Burned | Empty-handed | Tier 1 done / burned | Tier 2 done / burned | Earned (marks/player/day) | Fights (won) | Fragments | Coded | Decks with a known script |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 645 | 490 (76%) | 21 (3.3%) | 134 | 447 / 19 of 574 | 43 / 2 of 71 | 106 | 569 (564) | 205 | 11 | 196 |
| 2 | 639 | 492 (77%) | 19 (3.0%) | 128 | 464 / 19 of 604 | 28 / 0 of 35 | 99 | 554 (551) | 187 | 11 | 193 |
| 3 | 634 | 499 (79%) | 20 (3.2%) | 115 | 462 / 19 of 587 | 37 / 1 of 47 | 103 | 538 (535) | 195 | 14 | 199 |

Against N1 (about 750 runs, 74 percent done, 2 percent burned, 110 to 150 marks a day) fights cost cycles and trace, so there are fewer runs and a little less money, with a slightly higher burn rate at tier 1. Findings for the balance slice (N4), not tuned away here:

- Almost every fight is won, as the fairness test promises; the burns come from the trace they cost, and from the N1 honeypot trap in Mirrorglass tier 2 (a bot that never buys a power 3 sweep walks into it).
- **Tier 3 is still never played** by the bot (it needs reputation 40 and standing 2), so the bot woke a warden 0 to 7 times a seed (at tier 2) and never downed one: it codes tuned scripts rarely and the lens drops only from tier 2. The warden numbers come from the targeted tests, not the simulation.
- About a third of decks contain a script a corporation is wary of or has patched by the end of the month; 6 to 7 script identities a seed are patched at the close. The 12 bots are all buying the same few scripts, which a real board would do too; the nightly decay of 2 keeps the world from locking.
- The damage floor (a hit that lands does at least 1) and integrity growing 1 a level came out of the fairness test: without them, a guard-heavy tier 1 machine could not be beaten by the starter deck inside eight moves.

### What was and was not exercised

Unit, property and replay tests, the framework's sandbox audit, the pinned replay log from N1 (still reproduced), the screens drawn at 72 by 19 (the door area at 80x24) with the key hints in view, and the headless simulation. **Not exercised:** a real `ssh` client. This environment has no OpenSSH or telnet client, so the walk N1 did was not repeated; the same `DoorsScreen` code and `Driver` path are what the replay tests use, and the server's own door tests (a real russh client and a raw telnet client, with Coin Toss) pass unchanged. Postgres, colour and the Nerd Font tiers were not exercised either (the screen uses only the framework's styles and no symbols).

### Seams left for the next slices

- **N3.** `Run::encounter()` gives the whole fight state (`Encounter`: integrity, armor, intent, `forecast`, `seen`, `sealed`, `stun`, `charged`); `Run::preview(slot)` is what a script would do now; `Run::warden()` the hunt; `Run::stage(slot)` how the corporation knows a script. `session.rs` has `fight_lines` and `code_lines` as the text versions; replace them and keep `look_around`.
- **N4.** A crew run needs a shared `Run`, so a warden and a fight would have to be per-crew; the sim bot is the base for the branch-coverage report; fixer arcs could hand out specific fragments. Content still to write is the rest of the design budget (7 corporations, 21 node types, about 70 contract templates, 25 rig parts, about 12 hideout upgrades).


## Implementation notes (M4-N3: the UI)

Decision: [D116](../decisions.md). Same crate; no save or world change. Screenshots: [nullroute-ui/](nullroute-ui/README.md). The screens are `crates/doors/nullroute/src/session/ui/`; `session.rs` still holds the state, the keys and the look-around text, and the engine is untouched apart from two read-only seams (`Run::link_known` is public, and the heat thresholds are `player::HEAT_*` constants).

### The screens

| Screen | Key from home | What it draws |
|---|---|---|
| **Run** | (jack in) | The node map (braille edges in the Unicode tiers, `-` `/` `\` in ASCII; nodes by state: you, open, locked, ICE, black wall, honeypot, objective; the edges next to you glow and a packet of dots runs along them), the trace closing in from both ends of the top bar, the node you stand on and its numbered exits, the deck (a chip row at 72x19, a table above that), the scrolling timestamped log coloured by kind, flags (alarm, tracer, camera, warden, soak) and, in `Large`, an intel panel |
| **Encounter** | (a machine in the way) | The machine (art, integrity bar, armor, weak lock family, blind, charged, sealed), the telegraph (what it will do, the next three once the pattern is known or `? ? ?`, the beats you have seen, the move counter as pips), your scripts with their cost and what each would do now as a bar, with state (tuned, wary, PATCHED, SEALED, spent), and the fight log |
| **Debrief** | (after a run) | `EXTRACTED`, `JACKED OUT` or `BURNED`, the run's numbers (peak trace as a gauge, loot, fights, locks, fragments) and what happened |
| **Home** | | Cycles and heat gauges, the menu with live counts, the rig as level pips, the five skills as progress bars toward the next level; from `Medium` the fixers' standing bars; in `Large` the job board and the street word per corporation |
| **Jobs** (`c`) | `c` | A table with a cursor (the status column says `ready`, `no tool` or `closed`) and the selected job's brief beside it (under it at 72x19) |
| **Briefing** | Enter or 1-9 | The job, your deck against the target corporation (what each script can do, and whether the corporation has found it: fresh, wary, patched), the street word as pips |
| **Fixers and standings** (`f`) | `f` | A centred bar for each fixer (red left of the line, green right), who it deals with, its clients, the jobs on the board |
| **Deck** (`d`) | `d` | Memory, slots and noise as gauges, every script owned with its stats and a **degradation matrix** (one letter per corporation: `.` fresh, `w` wary, `X` patched), a detail pane |
| **Market** (`s`) | `s` | The scripts for sale with a cursor, the night's tide as a gauge, and a sparkline of the prices on offer |
| **Black market** (`b`) | `b` | The fence's ladder as bars, the proxies (and the heat surcharge), the safe |
| **Rig** (`r`), **Hideout** (`h`) | | Parts as level pips with the next cost and any fried part; bots, workbench, fence and the case of fragments |
| **Module editor** | `h`, `c` | The pipeline `effect + target + modifier => script` with the active slot lit, the fragments for the step, and a preview with the memory and noise budgets as gauges that go red when over |
| **Heat** (`t`) | `t` | The gauge with a tick for each threshold and the ladder of what heat does, lit as it is passed; lying low |
| **Character** (`p`) | `p` | Reputation, money, runs, jobs and burns, and the skills as a bar chart |

Keyboard only (Up and Down and Enter on the list screens, in addition to every letter and digit of the earlier text screens; Esc backs out). No mouse (UI-9).

### Responsive, in every tier

`Small` is the door area of an 80x24 window (72 by 19), `Medium` is from 88 by 23, `Large` from 120 by 34, and a window under 56 by 14 gets the plain text screen. In the ASCII tier the framework turns boxes, blocks and braille into ASCII, and the map and the node glyphs draw themselves in ASCII; no screen relies on colour alone (a state is also a glyph or a word). The board caps its box at 80 columns today, so over ssh a member gets `Small` with more rows; `Medium` and `Large` are verified at the area sizes of 100x30 and 132x43 windows by the tests and by the same composite path the board uses.

### Look around

The look-around view is the text of every screen (the `lines()` of `session.rs`, unchanged for the earlier views, with the same header and key hints), and the new screens (fixers `STANDINGS`, `HEAT`, `CHARACTER`, `BLACK MARKET`) have theirs. A test checks that every screen has one, that each says its keys, that a map screen names where you are, the ways out and their states, and that no line holds a control character.

### What was verified

| What | How |
|---|---|
| Every screen at 72x19, 92x25 and 124x38 in the ASCII, Unicode and Nerd Font tiers | `tests/ui.rs`: snapshot files in `tests/snapshots/` (regenerate with `UPDATE_SNAPSHOTS=1`); the Nerd Font tier must equal the Unicode one; the ASCII snapshot must be ASCII; every row fits |
| Every screen has a look-around | `every_screen_has_a_look_around` |
| Colours: accents in truecolor, no Rgb at 16 colours, none in mono, still a picture in mono | `colours_are_the_palettes_and_the_accents_and_degrade_to_nothing` |
| A hostile handle reaches no cell | `a_hostile_handle_reaches_no_cell` |
| A whole run through the keys: plan the deck, pick a job with the cursor, the briefing, jack in, work the map, meet a countermeasure, extract; and a reckless run that burns | `a_whole_run_from_the_board_to_the_debrief`, `a_reckless_run_ends_burned_and_the_debrief_says_so` |
| The earlier tests (engine, fights, coding, replay, simulation) | unchanged and passing; the two that read the drawn screen were updated for the new hint text |
| The sandbox audit | `tests/sandbox.rs` (no new dependency) |
| Real ssh | OpenSSH 9.6 against the built server on a fresh board, driven in tmux at 80x24: claim, icons, Doors, Nullroute; every screen above except the module editor; a run from a briefing through a fight to `EXTRACTED`; the screenshots are the captures |

**Not verified:** the module editor over ssh (it needs a workbench and fragments a fresh character does not have), a burn over ssh, the `Medium` and `Large` layouts over ssh (the board's 80-column cap), telnet by hand, Postgres, a real terminal in the ASCII tier, a terminal font without braille (the map would show boxes: the ASCII tier is the fallback).

### Seams left

- **N4.** A shared run for a crew would need the map to draw several positions; `MapView` takes one `Run`. A price history in the world would give the market a real sparkline.
- **UI-8 and UI-9.** The packet that runs along the edges moves with the log; effects (UI-8) could animate it with time. Mouse (UI-9) could pick a node on the map; the node positions are computed in `MapView::render` and would need to be returned.
- **Wider board layouts.** When the board lets a door use more than 72 columns, `Medium` and `Large` start being seen.

## Implementation notes (M4-N4: social, content, balance, endings)

Status: **Implemented (in review); Verified** by schema, unit, property, replay and migration tests, a two-player crew run through the framework's store, caps tests, the sandbox audit, the balance harness (numbers below) and a walk with the OpenSSH 9.6 client with two accounts on one board ([walk](../qa/m4-n4-walk.md), screenshots in [m4-n4-screens/](../qa/m4-n4-screens/)). Decision D143.

### What is implemented

| Area | What it does | Where |
|---|---|---|
| **Crews** | Up to 5 pilots. A leader opens a run on a contract (`g` on the brief), members join (`j`), the leader starts it (`s`), everyone enters (`e`). One shared **trace** (each member adds a share: 100/50/36/28/23 percent for crews of 1 to 5), shared opened locks and scans, **one crew warden** (wakes at the shared threshold or an alarm, lands on whoever moved), per-crew fights. The map shows every mate's initial on its node; the debrief lists the crew's split (points and marks per member). Pay is the job times 100/170/250/330/410 percent for 1 to 5. | `crew.rs`, `session/social.rs`, `session/ui/map.rs` |
| **The Null Vault (finale)** | A heist for 3 or more pilots (reputation 40, heat under 60): a layered network that needs crack, spoof and overflow in turn, so no single script family can open it. | `net.rs` (`generate_finale`), `crew.rs` |
| **Honeypots** | Hideout upgrade (60/120/240 marks, 3 levels). A hack on your hideout has a 25 percent chance per level to trip it and hurt the hacker. | `ties.rs`, `social.rs` |
| **Hacks and bounties** | Hack a rival's posted hideout (6 cycles, settled by the nightly tick, you hear tomorrow). A hit steals 10 percent up to 60 marks. Post a bounty (100 to 400 marks, 20 percent fee) on a pilot at your tier or above; paid to the first hack that gets in, it fries a rig part. | `social.rs` |
| **Heat between players** | Heat persists, decays slowly and has consequences: a rival can see "hot", heat 70 or more draws an automatic fixer bounty, and a bounty hit adds heat. | `social.rs`, `player.rs` |
| **Fixer arcs** | 5 arcs, 35 steps, 15 endings, multi-day, branching on choices and stats (a small condition language, `cond.rs`). Steps hand out contracts; some hand out specific fragments. | `content/arcs.toml`, `story.rs`, `cond.rs`, `ties.rs` |
| **Seasons and endings** | A season closes into the hall with its own ending (7 season endings, 9 epilogues) chosen from the season's tallies; the next season resets holes, learning, heat ties and standings; legends persist. | `content/seasons.toml`, `world.rs` |
| **Market history** | `World.hist` keeps a nightly price trail (saved, migrated, tested). **The market screen does not draw it yet:** it still shows the price ladder, so the sparkline is a seam for the next pass. | `world.rs`, `save.rs` |

**Caps on griefing.** New characters are safe for 7 days. 2 hacks a day, 2 hits taken a day, 3 days between the same two pilots, 2 bounties per poster, 2 per target, 6 fixer bounties on the board, 2 days of peace after being hit. **Opt out:** `pvp` off (a 2-day wait to opt back in) and `crews_open` off (not invitable). A crewmate cannot be a bounty or hack target. Nothing reads another player's save: every social action is an order or a delivery on the world, applied by the nightly tick, with a receipt (nonce) so it applies once, or refused with a refund (a pending order with no receipt is refunded after 14 days).

**Player text.** The only player-to-player free text is the crew name, the invitation handle and the bounty note; each goes through `Ctx::player_text` or `Ctx::player_name` (so the shared sanitizer, 140 characters and 6 a minute), everything else is a template line from `content/social.toml`. The bounty is shown on the street with the poster's handle: the board can read every post and the screens say so.

### The RPG rule, as implemented in N4

Time comes from `Ctx`, randomness from `DoorRng`. A crew run is an **event log** (`Op` in the world: the contract, a snapshot of holes, alert and learning, each member's deck and skills, and a list of packed `u16` actions). Replay is deterministic (`DoorRng::from_label("nullroute-crew", op.id * 1024 + nth)`), so a member's session rebuilds the whole run from the log and the screens always agree; a stale action (the log moved on) is dropped, which is why a mate's move shows after you reopen. The replay tests, the sandbox audit and a two-player run through `MemoryDoorStore` pin it.

### Content against the launch budget

| | Budget | Now | |
|---|---|---|---|
| Corporations | 12 | 12 | met (7 added, 2 fixers) |
| Node types | 30 | 30 | met (21 corporation-specific mid types) |
| Countermeasures and wardens | 20 + 3 | 20 + 3 | met |
| Scripts | 60 | 60 | met |
| Fragments | 30 | 30 | met |
| Contract templates | about 80 | 80 | met (from 11) |
| Fixer arcs | 5 | 5 (35 steps, 15 endings) | met |
| **Rig parts** | 40 | 15 levels of 5 parts | **short** |
| **Hideout upgrades** | 15 | not built toward 15 (honeypot and the N1 set) | **short** |

`perks` and `mods` exist in the save but nothing uses them yet.

### Balance: before and after

Before (N2/N3 harness): about 645 runs and 76 to 79 percent done, 3 percent burned, **tier 3 never played, 0 wardens down**; a first N4 pass had about 96 percent dry runs because 17 contract templates asked for an objective the engine cannot do (copy or shred need a vault; plant needs an admin console or a server): fixed, and a validator now rejects such content at load. Changes: the tier 3 security cap 7 to 6 (the best shop script has power 6; 1 in 5 tier 3 networks was unopenable), warden numbers Bulwark 22/3/12 to 16/2/4, Hound 21/3/11 to 15/2/4, Mimic 20/3/10 to 14/2/4 (integrity, armor, strike), warden wake threshold 55 to 45 and arrival 3 to 4, lens and prism fragment weights 5 to 9 and 5 to 8, and the planner now picks scripts that hurt.

`cargo test -p russh-bbs-door-nullroute --test balance -- --nocapture` (debug, about 45 s): 12 bots, 30 days, 4 seeds, one shared world; bots 0 to 5 are two crews of three, bots 6 to 11 are solo, the last bot opts out of hacks on day 10.

| | Runs | Done | Burned |
|---|---|---|---|
| Solo tier 1 | 1106 | 751 (68%) | 1 (0%) |
| Solo tier 2 | 762 | 400 (52%) | 85 (11%) |
| Solo tier 3 | 169 | 88 (52%) | 37 (22%) |
| Crew tier 2 | 175 | 117 (67%) | 0 |
| Crew tier 3 | 48 | 14 (29%) | 0 |

- **Tier 3 is played:** first tier 3 run on day 7 to 13. **Wardens:** woke 17 to 31 times a seed at tiers 2 and 3. In the 30-day runs 0 are downed, because bots prepare (tuned scripts, lens) rarely; in the 60-day seasons the harness downs 2 to 7 tier 3 wardens a seed. The targeted `the_warden_gate` trial (120 tier 3 runs with a warden): before (cap 7, old numbers) the stock deck met 15, downed 0; a prepared hunter met 39, downed 0, burned 68. After: stock met 13, downed 0, burned 56; prepared met 59, **downed 32** (54 percent of meets), burned 77, done 43. So the gate needs preparation, as designed.
- **Degradation pressure:** 8 to 86 of about 510 runs a seed used a wary or patched script (first row by night 0, up to 3 rows patched at the close).
- **Crew versus solo:** per-bot earnings over 30 days are about 5.5k for a crew bot against 5.6k for a solo bot: comparable on purpose. Crews burn nothing (the shared trace share and the single warden) but finish tier 3 less often (29 percent against 52 percent), because a tier 3 crew run needs the whole crew carrying the tool.
- **Hacks, honeypots, bounties** (4 seeds): 92 hacks filed, 46 got in, **24 caught by a honeypot**; 26 bounties posted, 12 paid, 24 automatic fixer bounties (heat at 70); **0 hits on the opted-out bot**.
- **Finale:** a ready crew cracks it in 5 of 16 seasons (`a_crew_that_is_ready_can_crack_the_finale`); one family alone cannot (60 seasons tried). In the plain 30-day runs 0 of 12.
- **Endings and branches** (`branch_coverage`, 12 seeds, release): 32 of 35 arc steps and 68 of 80 choices reached by the bots, **15 of 15 arc endings**, 4 of 9 epilogues and season endings {2, 5, 7} in simulation; the rest are reached by the test fixtures (every ending, epilogue and season ending has a replay that reaches it, so none is dead content). Some endings are hit once in 12 seeds: rare by design, reachable.

### What was walked over a real ssh client

The server binary on a fresh SQLite board, two accounts (`founder`, `bob`) with the OpenSSH 9.6 client at 92x25: a crew founded and an invitation accepted, a run opened on the brief, both pilots on one shared map (the other's initial on its node), a lookout fight, a plant, two debriefs showing the crew's split, then the next day (the board's clock moved with `faketime`) a second crew run, the leader leaving the crew, a **bounty posted with a typed note**, the target told next visit, and the street's pages. 51 screenshots. See [the walk](../qa/m4-n4-walk.md) for what it found and what it did not cover.

### Seams and what is not built

- **FW-1** (a `Ctx` read of the current world, a render-time "today", world-aware replay logs) had not landed. The workaround is small: the session sees the world as of `open`; every social action goes through `Session::edit` (an `edit_world` closure that may retry) and re-posts the hideout after each key. A crew member sees a mate's moves after reopening the run, not live. When FW-1 lands, `Session::gather` and `edit` can read the world directly.
- **Not built:** drawing the price trail on the market screen; taking the heat for a crewmate for a cut; bespoke social widgets (the social screens are bordered text panels, as the look-around text); a live crew refresh; the 25 missing rig parts and the missing hideout upgrades; `perks` and `mods`.
- Crew names are one handle-shaped word (the framework's name check): no spaces.
- Honeypots, hacks, the rival list and the arcs were exercised by the tests and the simulation; the walk covered the crew run and the bounty only.
