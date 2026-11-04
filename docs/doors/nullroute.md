# Nullroute: a hacking door (design)

Status: **Proposed**. Shared platform rules are in [README.md](README.md).

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
