# Long Haul: a space trade and exploration door (design)

Status: **Proposed** design; slice **M4-L1 (the engine) is implemented** in `crates/doors/long-haul` and described under [Implementation notes (M4-L1)](#implementation-notes-m4-l1) at the end. Crew, events, combat, the full UI and the social layer are not built. Shared platform rules are in [README.md](README.md).

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

## Implementation notes (M4-L1)

Slice M4-L1, branch `m4/long-haul-engine`, decision [D100](../decisions.md). Everything here is **Implemented and tested** unless it says otherwise; the last section lists what is not built or not verified.

### What exists

| Area | Where | What it does |
|---|---|---|
| Frontier | `frontier.rs` | 120 systems, lanes, stations, economies, hazards, charted and uncharted systems, generated from the **season number alone** (`DoorRng::from_label`). Nothing of the map is stored; every session and the tick rebuild it. Changing the generator changes the map, so `GEN_VERSION` is written into the world and a mismatch is treated as a different world. |
| Economy | `market.rs`, `world.rs` | About 30 goods, six station economies, the board-wide stock per station and good, the nightly tick (stock recovers 16 % of its gap a night with noise; news starts, spreads one hop a night and ends), contraband and inspections. |
| Ship | `ship.rs`, `trade.rs` | Four hull classes with module slots, 17 modules, wear in tenths of a percent, repairs that cost credits and drive-days, fuel, refuelling and gas-giant skimming. |
| Jumps | `engine.rs` | Cost in drive-days (by lane length, drive speed, wear and Piloting) and fuel (by length, drive burn and Piloting); hazards on arrival; first arrival charts a system for everyone and yields survey data. |
| Loan | `engine.rs` | Daily interest, a weekly payment, penalties, repossession, borrowing within a limit, a tow for a stranded ship. |
| Saves | `player.rs`, `world.rs`, `codec.rs` | A hand-rolled binary save (format byte `1`, framework `SAVE_VERSION` 1) and world (`WORLD_VERSION` 1). Hand-rolled because the door sandbox allows `serde` and `toml` only and `toml` here parses but does not write. A save that fails to decode opens an *unreadable* screen and **never** overwrites the bytes. |
| Tick | `lib.rs` | `daily_tick` decodes the world, applies the day, encodes it. Idempotent and capped by the framework. |
| Screen | `session.rs` | A minimal playable text screen (bridge, market, lanes, yard, loan, news). Not the L3 UI. It reads the same lines for `look_around`. |
| Content | `content/*.toml`, `content.rs` | Goods, economies, news, ships, backgrounds and name parts as TOML with a schema test (`tests/content.rs`) and cross-table link checks. |

**Time.** A captain gets **10 drive-days per real day** (the design's default, which settles that open question; D100; the loan shark is kept, which settles the debt-pressure one; piracy is not built). No carry-over of unused days. A jump needs one day left and may overdraw; the debt comes off tomorrow's allowance, capped at nine days. Docking costs one day and covers trading, fuel and the loan office for that visit; repairs, skims and scans cost what they cost.

**Prices.** A market's price depends only on its stock. The price of the *k*-th ton is the same whether it is being bought or sold, with a spread on each side, so any sequence of buys and sells at one station loses money (property test). A good's multiplier stays between 0.4 and 3.0 of its base. A station that *makes* a good aims to hold 140 % of its depth and one that *needs* it 65 %, which makes producers cheap and consumers dear, and every captain's trade moves the shared stock.

**What a captain can know.** Prices are known only as last seen at a dock (`Player::intel`), which is the information game: stale memory against live stock. A station's economy is public (the lane list shows its letter, and the bridge says what each type makes and wants). News reaches a system one hop a night, so a captain far from an event learns of it late.

**Money.** Every credit that enters or leaves the purse goes through `Player::credit` and `Player::debit`, keyed by a `Cause`, and the fuzz test checks `ledger.net() == cash` after every step. Fuel, repairs, fines, tows, dock fees and loan payments can only cost credits; only trade, module and hull sales, survey sales, borrowing and the starting purse add them. The same test checks that **no action raises net worth** except by the profit on cargo sold or by new survey data.

**RPG elements and what each changes** (each has a branch test in `tests/branches_rpg.rs` or `tests/branches_ship.rs`):

| Element | What it changes |
|---|---|
| Background (4) | Starting skills, purse, loan and standing, and one perk that opens an option: *Known Face* (a tenth off League fuel), *Old Codes* (no inspection and no dock fee at military posts), *Guild Contact* (a scanner at the start and survey data sells for a quarter more), *Fence* (havens pay a tenth more for contraband and customs look less). A navy washout is also turned away by a haven's black market until their Free Captains standing rises. |
| Trading | Narrows the spread on every trade (6 % a side down to 3 %). |
| Piloting | Less fuel burned, shorter jumps, reads a nebula from level 5, safer skimming. |
| Engineering | Cheaper repairs (down to half) and slower wear. |
| Stealth | Fewer inspections found (a level-10 sneak is never found at a 30 % League dock with up to 33 t of contraband aboard; a level-0 captain is found about a third of the time). |
| Surveying | Dearer survey data. |
| Skills grow by use | Only their own actions train them (`tests/branches_rpg.rs`). |
| Standing (3 factions) | At -40 a faction's docks refuse the ship until it thaws (a point a day); at +50 with the Free Captains the military posts refuse it. Seizure costs the station's faction 8 points; legal trade earns standing; contraband raises Free Captains standing. |
| Modules and hulls | Slot limits are hard; a scanner reads debris and nebulae, a shield soaks hazards, a refinery adds skim yield, a worn drive is slower and can fault. |
| Debt | Three missed payments in a row and the ship is repossessed: back to a hand-me-down courier, a fee on the loan and a smaller credit limit for good (`repossessions`). |
| The shared world | The first captain to reach an uncharted system charts it for everyone and their name is kept in the world. Trades move prices for everyone. News changes targets and inspection chances where it reaches. |

### Decisions and limitations worth knowing

* **The session reads the world once, when the door opens.** The framework gives a door no fresher view (`Ctx` has no read of the stored world; the closure given to `edit_world` sees the current bytes but cannot report back). The world edit is clamped (stock never goes below zero or above its limit), but a long-lived session could trade on stale stock. Long Haul therefore refuses buying, selling and jumping 15 minutes after the door opened and asks the captain to leave and come back. A framework addition that lets a door read the current world each key would remove the need; it is noted in [coordination](../coordination.md).
* **Interest and payments are applied lazily** on a captain's first key of a new real day (the tick cannot reach a player's save). After an absence at most 28 days of interest and two weekly payments are chased. The nightly tick moves markets and news only.
* **A new season starts a new captain.** The previous season's captain is overwritten by the first save (the Hall, heirloom and kept records are slice L4). A save from a **newer** build is refused by the framework before the door sees it.
* **Replay logs start from a fresh deterministic world**, because the framework's replay helper gives a door no world. Branch tests build the world and captain directly.
* **Survey data** is valued by richness, planets, belts, hazard, scanner grade, Surveying and a lab; a ship with no scanner still charts a system but gets a quarter of the value. A deep scan of a charted system pays a third and can be done once per system per captain.
* **No contracts, crew, transit events, combat, convoys, piracy or outposts.** Seams for them: `Event` and `Refusal` are enums (add variants), `Action` is an enum, `Ship`'s `berths` and `power` are computed and unused, `Player` and `World` have format bytes, and a new `Cause` goes in the ledger array.

### Balance harness (60 simulated days)

`tests/sim.rs` runs 16 bot captains in one shared frontier for 60 days with the nightly tick between days: 6 *informed* (they know only what they have seen and what a station type makes and wants), 3 *oracle* (live prices everywhere), 3 *smugglers*, 2 *explorers* and 2 *random* flailers. Run it with `cargo test -p russh-bbs-door-long-haul --test sim -- --nocapture`. A run with the default seed:

```
median net worth (credits) by strategy, every 10 days
day               0       10       20       30       40       50       60
Informed      -2950     8268    18763    24755    24164    22949    24035
Oracle        -2950    10139    21569    31914    51394    53620    82152
Smuggler      -5550     8033     7878    32781    58946    56272    61845
Explorer      -1000      889    -2878    -5381    -7082   -23429   -25686
Random        -2950    -5017    -7016   -23656   -26413   -34409   -38068

price index (mean mid/base, per-mille), min and max across all markets
day   0: mean  1080  min   633  max  2000
day  30: mean  1080  min   646  max  3000
day  60: mean  1081  min   559  max  2654
```

Net worth is purse, cargo at cost, modules and hull at resale and unsold survey data, less the debt, so a new captain starts near -3,000. The guard rails the test asserts: the price level stays between 0.8 and 1.5 of base and no price leaves 0.4 to 3.0; a plan beats flailing by 20,000 credits in each of four seasons; live prices beat remembered prices on average; growth is neither a runaway (under 20,000 a day) nor a non-living (over 100 a day); the same seed gives the same sixty days.

**What the numbers say, honestly.** Trading pays: the informed bots' median net worth turns positive within ten days (the loan is still owed, but they are worth more than they owe) and reaches about 24,000 at day 60. Across eight seeds the informed median ends between 21,000 and 45,000. They **plateau after day 20**, and the oracle bots keep growing, so either the bots' stale memory or market saturation is the limit; L4 should find out which. **Explorers lose money in this harness** (survey data is a finite pool and the explorer bot is simple), and **smugglers are bimodal** (a good run pays most, a stranded one is repossessed). Neither is tuned; both are L4 balance questions, listed in [coordination](../coordination.md).

### Verified, and not

* **Tests** (`cargo test -p russh-bbs-door-long-haul`): 60+ tests: unit tests for the codec, pricing and skills; property tests for generation (every season well formed, connected, with every economy, shipyards, a charted home ring and enough start stations), for prices (always in band, no sequence of trades at one station makes money) and for random play (500 random actions per case across seasons and backgrounds, checking the ledger, capacity, wear, slots, saves round-tripping, a refused action changing nothing including the RNG, and net worth never rising except by sale profit or new survey data); replay tests through the framework; 33 branch tests; the framework's sandbox audit; the nightly tick through `run_due` (idempotent, capped catch-up, a pure function of world and day); a headless run of bots through the real open/flush/tick path; the 60-day sim.
* **A hand walk with the OpenSSH client** over a real board (`russh-bbs-server`, SQLite, 80x24): Doors, choose Long Haul, pick a captain, buy 24 t of Kiln Ceramics, look at the lanes, jump, dock at a station with two docks (the chooser), borrow, find that the first station did not trade the cargo, carry it to a research station that wanted it and sell at a profit, leave and re-enter (state kept), restart the server (state kept, `door-long-haul` job registered).
* **Not exercised:** Long Haul over telnet (the framework's tests cover the shared code path with the example door); a real day boundary on a long-running server (interest, the weekly payment and the allowance reset are tested with a manual clock); two live captains trading in the same market at once; Postgres (the door touches the store only through the framework's tested save API).

