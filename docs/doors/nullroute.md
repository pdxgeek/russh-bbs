# Nullroute: a hacking door (design)

Status: **Engine implemented (M4-N1, in review)**; the rest is **Proposed**. Shared platform rules are in [README.md](README.md). Implementation notes are at the end.

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

- **N2.** `Run::step` resolves a patrol in one step (the `Patrol` enum and `Run::add_trace` are the hook points); replace it with encounters. Script degradation: a per-corporation use count belongs beside `NetState` in the world. The module editor needs the workbench (`Player::workbench`) and fragments (not in the content yet).
- **N3.** `session.rs` is the only screen code and holds no rules; `Run::view`/`NodeView` is the knowledge-respecting view of a node for a map. `look_around` is the same text.
- **N4.** Crews need a shared run (`Run` is per player today); honeypots at hideouts need a per-player world record; fixer arcs build on `Player::standing` and `Contract`; the autoplay harness reports ends of runs by tier and is the base for the branch-coverage report.
- **Content still to write (design budget):** 7 more corporations, 21 more node types, 33 scripts, 30 fragments, about 70 more contract templates, 5 fixer arcs, 25 more rig parts and about 12 more hideout upgrades. The tables and the schema tests are in place; the content is data.
