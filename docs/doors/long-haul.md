# Long Haul: a space trade and exploration door (design)

Status: **Proposed**. Shared platform rules are in [README.md](README.md).

## Pitch

The jump gates failed a generation ago, and the frontier is a scatter of stations, mining rocks and dead colonies linked by slow drive routes. You captain a tramp freighter with a debt, a small crew and a cargo hold. Buy low, haul far, sell high. Take passenger and courier jobs. Salvage derelicts, chart unknown systems, and sometimes outrun pirates or the authorities.

The core tension: **fuel, time and debt.** Every jump burns fuel and days, the loan shark wants a payment each week, and the best profits sit on the longest, riskiest routes.

## Daily loop

1. **Bridge report:** the ship's status, crew mood, debt due, news from the lanes, and cargo prices at known stations.
2. **Spend drive-days.** You get 10 per day by default. A jump costs days by distance and drive; docking, trading and repairs cost a little.
3. **Events in transit:** distress calls, pirates, inspections, drive faults, and strange signals.
4. **Night:** markets move, news spreads, pirates relocate, and interest accrues.

## Systems

### The frontier (procedural)

- About 120 star systems per season, from a seed. Each has stations, planets, belts and hazards.
- **Stations** have an economy type (mining, farming, industrial, research, pirate haven or military) that sets what they make and what they need.
- **Uncharted systems** reveal themselves only when someone visits. Selling **survey data** to the Cartographers' Guild is its own career.

### Economy

- About 30 goods, from water and ore to medicine and contraband. Prices follow supply and demand across the frontier, and every player's trades move them.
- **News events** shift demand: a plague on one station, a strike on another, a war between two factions.
- **Contraband** pays best and risks inspection, fines, seizure and reputation loss with the law.

### The ship

- **Hull classes:** courier, freighter, prospector and gunship. Each has slots for drive, cargo pods, fuel tanks, scanners, weapons, shields, crew cabins, a refinery and a lab.
- **Modules** come in grades and wear out; repairs cost credits and drive-days.
- **Refuelling** happens at stations or by skimming gas giants (slow, and risky with a bad scanner).

### Crew

- Up to 8 named crew, with roles: pilot, engineer, gunner, medic, scientist, quartermaster and face (who trades and talks).
- Each has skills, a **mood** (pay, food, danger, how they're treated) and a personal **goal**. Meeting a goal makes them loyal; ignoring it can end in mutiny or desertion at a station.
- Crew can be injured, hurt in combat, or lost.

### Combat

- **Ship-to-ship:** a short tactical exchange over range bands (far, mid, close). Allocate power between weapons, shields and engines each round. You can always try to run.
- **Boarding** on derelicts and pirate wrecks plays as a short room-by-room skirmish using the crew.

### Jobs and factions

- **Factions:** the Station League, the Miners' Union, the Free Captains (pirates), the Cartographers' Guild and the Quiet Church (who run the old gates and want them back).
- **Reputation** with each faction opens docks, prices and jobs; some factions are at odds.
- **Contracts:** delivery, passenger runs (passengers have their own events), bounties, escorts and surveys.
- **Arcs:** six hand-written arcs, one per faction plus a mystery arc about why the gates failed. Each is 6 to 10 steps.

### Players and the frontier

- **Shared frontier:** every player's trades move the same markets, and charted systems are named after the first captain to survey them.
- **Convoys:** two to four players can fly a route together and share events.
- **Piracy:** opt-in. A captain flagged as a Free Captain can intercept other flagged captains, or unflagged captains carrying contraband. Clean haulers are never attacked by players.
- **Stations:** late in a season, players can fund a new outpost in an uncharted system, which becomes a real market for everyone.

### Seasons

- **Era:** 8 weeks. The season ends when the gates open, which is the finale of the mystery arc.
- **Kept across seasons:** the Hall (richest captains, best surveyors, most-feared pirate), and one heirloom module.

## UI

- **Star chart:** systems as glyphs, lanes as lines, and known prices as a heat overlay. ASCII tier uses letters for station types.
- **Bridge:** ship status bars, crew list, cargo manifest and the route planner, which shows fuel and day cost before you commit.
- **Combat panel:** range bands across the screen, power sliders, and intent arrows for the enemy.
- **Look around:** text lists of nearby systems and lanes, with costs.

## Content budget (launch)

- **Economy:** 30 goods, 6 station economies and 20 news event types.
- **Ships:** 4 hulls, 50 modules and 12 crew backgrounds.
- **Story:** 120 transit events, 40 derelict layouts and 6 arcs (about 45 steps in all).
- **Opposition:** 15 pirate and authority ship types.

## Proposed slices

1. **Engine:** frontier generation, economy, ship and jumps, with replay tests.
2. **Crew and events:** crew, transit events and contracts.
3. **Combat:** ship combat and boarding.
4. **UI:** star chart, bridge, route planner and combat panel.
5. **Social:** convoys, opt-in piracy and player outposts.
6. **Content and balance:** arcs and the economy simulation.

## Open questions

- **Drive-days:** 10 per day?
- **Piracy:** opt-in flagging, as described?
- **Debt pressure:** keep the loan shark (pressure early, freedom later), or start debt-free?
