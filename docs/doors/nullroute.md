# Nullroute: a hacking door (design)

Status: **Engine implemented (M4-N1, merged); countermeasures, encounters, coding and script degradation implemented (M4-N2, in review)**; the rest is **Proposed**. Shared platform rules are in [README.md](README.md). Implementation notes are at the end.

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
