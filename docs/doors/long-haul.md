# Long Haul: a space trade and exploration door (design)

Status: **Proposed** design; slices **M4-L1 (the engine)** and **M4-L2 (crew, events, combat, jobs)** are implemented in `crates/doors/long-haul` and described under [Implementation notes (M4-L1)](#implementation-notes-m4-l1) and [Implementation notes (M4-L2)](#implementation-notes-m4-l2) at the end. Slice **M4-L3 (the UI)** is implemented too, described under [Implementation notes (M4-L3)](#implementation-notes-m4-l3). The social layer is not built. Shared platform rules are in [README.md](README.md).

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

Slice M4-L1, branch `m4/long-haul-engine`, decision [D103](../decisions.md). Everything here is **Implemented and tested** unless it says otherwise; the last section lists what is not built or not verified.

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

**Time.** A captain gets **10 drive-days per real day** (the design's default, which settles that open question; D103; the loan shark is kept, which settles the debt-pressure one; piracy is not built). No carry-over of unused days. A jump needs one day left and may overdraw; the debt comes off tomorrow's allowance, capped at nine days. Docking costs one day and covers trading, fuel and the loan office for that visit; repairs, skims and scans cost what they cost.

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
* **No contracts, crew, transit events, combat, convoys, piracy or outposts in L1** (crew, events, combat and contracts are L2, below; convoys, piracy and outposts are L4). Seams for them: `Event` and `Refusal` are enums (add variants), `Action` is an enum, `Ship`'s `berths` and `power` are computed and unused, `Player` and `World` have format bytes, and a new `Cause` goes in the ledger array.

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

## Implementation notes (M4-L2)

Slice M4-L2, branch `m4/long-haul-crew`, decision D112 (the number is assigned at merge). Everything here is **Implemented and tested** unless it says otherwise; the last section lists what is not built or not verified. The screens are still the minimal text screens of L1 (L3 builds the real UI).

### What exists

| Area | Where | What it does |
|---|---|---|
| Crew | `crew.rs`, `crewops.rs`, `content/crew.toml` | Up to 8 named hands with a role (pilot, engineer, gunner, medic, scientist, quartermaster, face), a skill that grows by use, a mood (pay, food, danger, treatment), a loyalty and a personal goal (12 goal templates over six kinds). Hired at a dock from three recruits who are a pure function of the season, the station and the three-day epoch; berths limit the crew (hull berths plus cabin modules; passengers share them). |
| Transit events | `transit.rs`, `content/transit.toml` | **60 events** in 11 families (9 distress, 9 pirate, 7 inspection, 6 drive, 8 signal, 4 hazard, 6 passenger, 3 derelict, 3 crew, 3 trade, 2 omen), 184 options of which 81 carry a requirement, 249 outcomes, 20 named story flags. Drawn by weight from the arrival system (hazard, economy, fresh chart), the cargo, standing, the crew and the contracts aboard. |
| Combat | `combat.rs`, `content/enemies.toml` | Three range bands, a reactor of 5 to 8 points split between weapons, shields and engines each round plus a manoeuvre, an enemy whose next move is shown first (and can lie), running always allowed, boarders and grapples, an escorted ship that takes shots. 9 enemy types (pirates, League cutters and gunboats, a Union enforcer, a gate sentinel). Three new weapon modules (`lance2`, `rail1`, `scatter1`) so the band matters. |
| Boarding | `boarding.rs`, `content/derelicts.toml` | A room-by-room skirmish with the captain and every fit hand: move, fight, breach a sealed door, search a data cache, patch, take, withdraw. 9 layouts (4 are the wrecks of boardable enemies). |
| Factions and jobs | `contracts.rs`, `content/jobs.toml` | Five factions (Station League, Miners' Union, Free Captains, Cartographers' Guild, Quiet Church) with standing; five kinds of contract (delivery, passengers, bounty, escort, survey), a per-station board that is a pure function of the season, the station and the epoch, 7 passenger kinds with their own events. |
| Scenes | `scene.rs` | An open event, fight or boarding is saved with the captain; only its own actions are legal until it ends. |
| Saves | `player.rs`, `lib.rs` | Save format 2 (framework `SAVE_VERSION` 2): the L1 body plus an appended tail. `LongHaul::migrate` turns a format-1 save into format 2 by changing the byte and appending an empty tail; the decoder still reads format 1. |
| Screens | `session/screens.rs` | Minimal text screens: the scene (numbered choices, closed ones say why), crew (`c`) and jobs (`b`); the bridge gains `[c] crew`, `[b] jobs`, `[p] rations`. They fit the 72-column box and are also the look-around text. |

### The rules, in numbers

* **Crew bonus.** A fit hand adds half their effective skill rounded up (and one more once sworn) to the matching captain skill (pilot to Piloting, engineer to Engineering, face to Trading, quartermaster to Stealth, scientist to Surveying), capped at 10. A hurt hand adds nothing; each scar costs a point of skill. Gunners and medics have no captain skill; they act in fights, boardings and events.
* **Mood and loyalty.** Four parts, each -10 to 10. Food rises one a day on a stocked galley (rations: `6 cr` a head for a week, a good quartermaster cuts it by up to a fifth) and falls one a day without; pay rises 2 on a paid week and falls 4 on a missed one; danger falls with fights, wounds and losses and heals with calm days; treatment moves with choices and drifts to zero. Loyalty moves daily by `(sum + 3*sign) / 6`, clamped to -3 to +2.
* **Goals.** Visit a system, win fights, chart systems, finish contracts, hold hundreds of credits, dock at new stations. Meeting one makes a hand **sworn**: loyalty +30 and never below 70, +1 effective bonus, they never desert, and events can ask for them (`sworn`). Dismissing a sworn hand sours the rest.
* **Leaving.** An unsworn hand at loyalty 15 or under with a negative mood leaves at the next dock about half the time; two or more together mutiny (60 %), leave together and take a fifth of the purse.
* **Hurt and lost.** Injuries last days (a medic aboard halves them and prevents scars); a heavy hit in a fight can hurt or kill a hand; a downed boarding hand is lost 35 % of the time without a medic and 12 % with one.
* **Combat.** The reactor is 5 (courier), 6 (freighter, prospector) or 8 (gunship), +1 for Engineering 4 and +1 more for 8 (the captain's own with an engineer's help). Damage is the weapons' power, scaled by wear and by how well their `reach` suits the band (100 % on their band, 60 % one off, 25 % two off), times `(weapons points + gunner bonus) / 3`. The enemy's hit chance falls with engines (5 a point) and piloting (2 a level); shields soak `3` a point plus a fifth of the shield modules' percent. An engineer patches half their bonus in hull points a round. A feint's chance is the enemy's `feint_pm` less 60 per scanner grade and 12 per pilot level. Running succeeds at `30 + 7*reactor + 3*pilot + band - 6*enemy speed` percent (band: far +25, mid +5, close -20), clamped 5 to 95, and a failed run gives the enemy its round. A fight ends at a wreck, an escape, a defeat (hull at a tenth or less: 40 % of the cargo and a quarter of the purse go; League ships fine and seize contraband instead) or after 24 rounds.
* **Boarding.** The captain has 20 hit points, a hand has `12 + 2*skill`. A gunner punches `4 + skill`, a medic 1, others `2 + skill/2`, the captain 3. A door with `lock` N opens only if the captain's Engineering plus the best fit engineer in the party's bonus reaches N; a cache is read by a scientist in the party or a captain with Surveying 3; a medic patches `6 + skill`, a captain's kit 3. Loot is kept only if the party pulls out; a wipe-out loses it and hurts or kills hands. Cargo from a wreck comes aboard at no cost and only as far as the hold allows.
* **Standing.** Docks close at -40 for the three station factions (the L1 rule); the Guild's office closes at -40 (no jobs, no survey sale) and pays 10 % more at +25 and 15 % less at -25 for survey data; every office posts nothing at -40; recruits refuse a captain at -25 with their station's faction; bounties need +15 and escorts +10 to be shown, and +25 pays a third more. A deed moves its faction and the rival a third as far the other way (League and Free Captains are rivals, Miners and Church are rivals). Killing a faction's ship costs standing with it (a League cutter 25, a Union enforcer 12, a gate sentinel 8 with the Church).
* **Jobs.** A station's owner always posts; the Guild posts at research stations; the Church at about a third of stations. Four jobs at a time. Delivery cargo counts against the hold, passengers against the berths. A late job fails (standing -4, goods taken back), giving one up costs 3. Client contraband counts in a customs search and, found, ends the job. A passenger's pay runs from 60 % to 140 % of the reward by how content they were (events, rations). An escort pays by the ward's hull percent when it arrives.

### RPG elements and what each changes

Every one has a branch test (`tests/crew.rs`, `tests/events.rs`, `tests/combat.rs`, `tests/boarding.rs`, `tests/contracts.rs`).

| Element | What it changes |
|---|---|
| Crew role (7) | Pilot: a faster, thriftier ship, a harder target, a better getaway. Engineer: cheaper repairs, slower wear, patches the hull in a fight, opens locks. Gunner: more damage, leads a boarding. Medic: faster healing, fewer deaths, better patches. Scientist: dearer survey data, reads caches. Quartermaster: cheaper rations, fewer inspections found (client cargo too). Face: narrower spreads, talks down tolls and bribes. Each also opens options no other mix has. |
| Crew mix | The same scene shows different choices: an engineer of skill 2 opens "fix the drive", a scientist decodes the gate song, a quartermaster hides the hold. A hurt or lost hand closes them. |
| Mood and neglect | Unfed or unpaid crew lose loyalty and walk; in the harness ignored crews leave at 0.15 to 0.26 hands per hire against 0.00 to 0.02 for cared-for ones. |
| Goals | Swear a hand to the ship (and make them immune to leaving) only if the captain actually goes where they asked. |
| Scars and injuries | A scar costs a permanent skill point; a hurt hand is off duty. |
| Standing (5) | See the Standing rule above. Deeds persist: two wrecked League cutters close League docks until the standing thaws a point a day. |
| Flags | A choice in one event unlocks or locks a later one (the ark map, the cartel contact, once-only events). |
| Weapons and bands | A rail driver is a far-band gun, a scatter cannon a close-band gun; the enemy's own preferred band decides what you want to do with the range. |
| Scanner and pilot | See through an enemy's feints. |
| Jobs | Standing gates which jobs are shown and what they pay. |

### Decisions and limitations worth knowing

* **Hull berths and the starter.** Hulls now carry berths (courier 1, freighter 2, prospector 2, gunship 3); a cabin module adds two. Selling a cabin or trading hulls is refused if it would leave people without a berth. Repossession clears the jobs and sends hands without a berth ashore.
* **The event table is addressed by index in saves** (an open scene stores the event's position). Add events at the end; never reorder or delete. The same holds for enemies and derelict layouts.
* **Accept reads the board of the day the key is pressed.** The screen draws the board for the captain's last settled day; across midnight a key can land on a different offer. Offers carry stable keys, so a taken one never returns, but the index is not the key. A framework seam that gave a door the current day at render would close this.
* **Church standing has no dock.** The frontier has no Church stations (changing the generator changes every map, D103 (1)), so the Church moves jobs and events only; the Guild's lever is its office and the survey price.
* **Nothing in L2 changes the shared world.** Kills, rescues and beacons are the captain's alone. L4 owns outposts, convoys, opt-in piracy and the world remembering a kill.
* **Content is a fraction of the launch budget:** 60 of 120 events, 9 of 40 derelicts, 9 of 15 enemy types, 12 goals. The tables take more without code.
* **Combat has no negotiation round and no surrender.** Tolls, bluffs and bribes are events, resolved before a fight starts.
* **Survey jobs count only a scan or chart made after accepting.**
* **L1's own harness numbers moved.** Transit events draw from the same RNG as everything else, so the same seed plays a different L1 sim (`tests/sim.rs`, whose harness waves any scene away): informed bots now end near 43,000, smugglers at about -41,000 (they were bimodal in L1 too), explorers and random flailers negative. The L1 conclusions hold; the exact tables above in the L1 notes are as L1 recorded them.

### Balance harness (60 simulated days, with crews)

`tests/sim_crew.rs` runs 16 bot captains in one shared frontier for 60 days with the nightly tick between days: 6 *haulers* (take jobs, keep three crew fed and paid), 3 *couriers* (jobs, two crew), 3 *hunters* (armed, bounties, seek fights), 2 *neglect* (crew they never feed or look after) and 2 *loners* (no crew, jobs only, run from every fight). Run it with `cargo test -p russh-bbs-door-long-haul --test sim_crew -- --nocapture`. The default seed:

```
== crew loyalty, combat and reputation over sixty days ==

Hauler
day               0      10      20      30      40      50      60
net worth      -500    -182    1897    3564    4871   10178   16703
crew              0      19      23      23      20      17      17
loyalty         0.0    65.2    76.8    90.4    96.8    99.0    96.2
sworn             0       7       9      10       8       7       7
League          7.5    17.7    28.7    30.2    34.3    42.8    46.3
Union           0.0     9.5    14.7    18.2    18.8    20.0    24.3
Captains        0.0     1.0     0.8     3.8     3.8     3.7     3.5
Guild           0.0     0.0     1.8     1.0     2.0     2.0     2.7
Church          0.0    -0.5     0.8     0.5     0.8     1.8     1.3
totals     boardings 13, fights 39, goals met 10, hired 23, hurt 7, jobs done 196, ran 39, scenes 359, seized 2, towed 193

Courier
day               0      10      20      30      40      50      60
net worth     -2200     170    2722    2066    8452   14604   18064
crew              0       7       7       8       8       9      11
loyalty         0.0    76.6    92.1    88.2    95.1    95.8    88.0
sworn             0       5       6       6       7       8       8
League          1.7    13.0    22.0    32.7    43.0    50.0    61.3
Union           0.0     7.7    11.3    19.7    27.7    33.3    35.7
Captains        6.7     4.7     5.0     3.0     1.7     0.0    -2.7
Guild           0.0     2.3     0.0     0.7     3.3     6.0     8.3
Church          0.0    -0.3     1.7     0.7     0.7    -0.3     0.3
totals     boardings 9, fights 21, goals met 8, hired 11, hurt 4, jobs done 124, jobs failed 2, ran 21, scenes 223, seized 2

Hunter
day               0      10      20      30      40      50      60
net worth      2600    2156    3441    1477    4149     769    4052
crew              0      10      11      11      11      11      11
loyalty         0.0    61.4    74.5    87.7    96.5    99.7   100.0
sworn             0       4       4       4       4       4       4
League          3.3    13.0    26.7    26.3    29.0    28.3    29.0
Union           0.0    15.7    16.7    17.3    20.7    24.7    25.7
Captains        3.3     2.0     2.0     2.7     4.0     3.7     3.7
Guild           0.0     2.7     2.7     3.3     3.3     4.3     6.0
Church          0.0    -1.0     2.0     1.7     1.3     0.7     0.7
totals     boardings 11, fights 26, goals met 4, hired 11, hurt 7, jobs done 97, ran 16, scenes 142, seized 1, wrecked 3

Neglect
day               0      10      20      30      40      50      60
net worth      -500    4722    9469   14906   21937   27718   30294
crew              0       6       6       6       7       7       8
loyalty         0.0    65.3    56.0    64.2    58.0    63.7    58.8
sworn             0       4       4       5       5       6       6
League         12.5    24.0    39.0    46.0    60.0    79.5    86.5
Union           0.0     7.0    13.5    23.0    28.0    32.0    45.0
Captains       -5.0    -3.5    -3.0    -0.5    -1.0    -3.5    -1.5
Guild           0.0     1.0     2.0     2.0     3.0     4.0     2.5
Church          0.0     1.5     0.5    -0.5     0.0    -0.5    -1.0
totals     boardings 6, deserted 1, fights 10, goals met 6, hired 9, hurt 4, jobs done 99, ran 10, scenes 145

Loner
day               0      10      20      30      40      50      60
net worth     -1000   -2765   -6330   -7508  -10506  -30081  -58911
crew              0       0       0       1       1       0       0
loyalty         0.0     0.0     0.0    30.0    22.0     0.0     0.0
sworn             0       0       0       0       0       0       0
League         -2.5     6.0     6.0    10.0    10.5     6.5     2.0
Union           0.0     9.5     8.0     8.5    12.0    10.0     9.0
Captains       10.0     9.5    10.5    10.5    11.5    13.5    15.5
Guild           0.0     1.0     1.0     0.0     0.0     0.0     0.0
Church          0.0     3.0     5.0     7.0     7.0     8.0     8.5
totals     boardings 2, deserted 1, fights 6, hired 1, jobs done 23, ran 6, scenes 48, towed 182

losses per hire: hauler 0.00, neglect 0.15
end loyalty: hauler 96.0, neglect 53.6
```

**What the numbers say, honestly.** Loyalty climbs on ships that feed and pay (hauler loyalty rises from about 65 to 96) and the ignored crews end near 50, with the sworn hands holding the average up and the rest leaving; the guard rail asserts that neglect loses at least 0.1 hands per hire more than care. League standing climbs on every job-taker (jobs raise it, its rival the Free Captains slips) and the Union follows. Hunters win a few fights and run from most: a single lance does not beat a raider's shield and hull quickly, which is intended for a starter ship but means **combat income is not yet competitive with jobs** (an L4 balance question). **The loner collapses** (about -59,000 at day 60): with no engineer and no pilot its hull wears down through events, it cannot afford repairs, strands and is towed, and every tow goes on the loan. That is partly the bot (no fuel discipline: the hauler and loner bots were towed 193 and 182 times between them, each tow added to the loan), partly a real flag: an unarmed lone captain bleeds. Crew did not change **net worth** much in this harness (the neglect bots end at 30,000 against the haulers' 17,000) because the bots pick event options at random and do not exploit what a crew opens; crew pays in options and survival more than in credits. None of this is tuned. The guard rails the test asserts: events, hires, fights, jobs and goals all happen; neglect loses crew faster than care; hauler loyalty ends above neglect's; hunters win fights and loners never do; standing stays in range; the ledger balances, the hull, hold and berths stay legal and the save round-trips for every bot every day; the same seed gives the same run.

### Verified, and not

* **Tests** (`cargo test -p russh-bbs-door-long-haul`, 153 tests): the L1 suites unchanged and passing (the harness now waves away a scene that an old test did not expect); `tests/crew.rs` (hiring, upkeep, goals, unrest, injury, the role bonuses); `tests/events.rs` (the table, **every option of every event for a loaded captain across 12 seeds with the invariants checked**, scene blocking and reload, crew/standing/flag branches, weights by system, cargo and standing); `tests/combat.rs` (a property test of random fights plus the branches); `tests/boarding.rs` (a property test of random orders plus the branches); `tests/contracts.rs` (every kind of job end to end, standing as a gate, the Guild price); `tests/saves.rs` (a **frozen L1 save** from commit 68e6cb6 migrates and loads, damaged and newer saves are refused); `tests/screens.rs` (framework replay of a scene, a fight, a boarding, hiring and jobs, and every event and layout and enemy on screen at 71 columns); schema tests that reject a dead-end event, an unknown flag, an unreachable room, wild enemy stats and a missing client line; the economy fuzzer now plays scenes and the new actions (refused actions leave the captain, the world and the RNG untouched, the ledger balances, nothing is created from nothing except contract, salvage and event credits and new survey data); the sandbox audit; the 60-day sim.
* **Not exercised:** a hand walk with a real SSH client (the environment has no OpenSSH client; the screens were exercised through the framework's replay and the shared door path through the server's russh-client tests with the example door); Long Haul over telnet; a real day boundary on a long-lived server; two live captains hunting the same bounty.

## Implementation notes (M4-L3)

Slice M4-L3, branch `m4/long-haul-ui`, decision D118 (the number is assigned at merge). Everything here is **Implemented and tested** unless it says otherwise. The UI lives in `crates/doors/long-haul/src/session/ui/`; the engine is untouched (a few `Session` fields and `Mode` variants were added, and `Mode::Nav` became the chart and the planner).

### The screens

| Screen | Key | What it draws |
|---|---|---|
| Star chart | `v`, or `j` | The frontier as a braille raster: stars, known lanes (solid), lanes to uncharted signals (dotted), **fuel range rings** (a full tank and half a tank, from the ship), stations coloured by the faction that runs them (or by how fresh your price intel is, `o`), `?` for an uncharted signal one lane from a charted system, `▲` for the ship, an amber cursor you walk with the arrow keys (it goes to the nearest system in that direction), three zooms (`+`/`-`), a system card (star, hazard, stations with their economy, yard and repair, the lane cost from here, and a fastest route to the cursor). Labels are placed greedily and never overlap. |
| Route planner | `Enter` on the chart | A path over the **charted** lanes (an uncharted signal may be the last stop) chosen as *Fastest*, *Thriftiest* or *Safest* (`g`): each leg's span, fuel, days and chance of an event; meters for fuel against the tank, days against what is left today, and the chance of meeting anything; a track map; the **expected events by family** (from the same weights the engine draws with). `Enter` flies the **first leg** (an ordinary `Action::Jump`); the plan stays until the target is reached, and a scene that opens takes the screen first. Legs the tank cannot cover are red. |
| Bridge | start | Hull, fuel (with how many short hops it buys), reactor pips, a hold gauge, wear, payday and the voyage date, as gauges; alerts (fuel below the cheapest lane, hull, worn modules, payment you cannot meet, an empty galley, a restless or hurt hand, a job about to be late, a route set); the hold; a local chart; the crew on watch. |
| Combat | a fight | The range between the ships with the **telegraphed** intent as an arrow (`Combat::shown`, which can lie; the real intent is never read), both hulls and the enemy's shield, the **reactor split as three adjustable gauges** (`Up`/`Down` pick, `Left`/`Right` move a point; the total stays the budget; `Enter` fires; `1`-`4` are the old presets and fire at once), the manoeuvre (`c`/`o`/`h`), the run chance, and the fight log. |
| Boarding | a boarding | The wreck's rooms laid out by distance from the airlock with the party's room, hostiles (`!`), sealed doors (`L<n>`, a red link), loot (`$`) and caches (`#`); the party's health and what has been taken; the room and the numbered orders. |
| Crew | `c` | The roster (loyalty, sworn, restless, hurt) and recruits at a dock, with a detail pane: skill pips with scars, the four parts of mood as centred bars, loyalty, the goal and its progress. |
| Market | `m` | Every good with stock, mid price, **how far it sits from base** (a signed percent, so colour is not the only cue), buy, sell, held, and what the station makes or wants; a **sparkline of the selected good's price across every station you have intel for**, nearest first, with the cheapest and dearest named. The prices are as last seen, not live (the pane says so). |
| Jobs | `b` | Carried jobs with a days-left bar, the board as a table, and a detail pane with the client's words, a closed offer's standing need, and the route (jumps, fuel, days) to the destination. |
| Event scenes | an event | A framed vignette (braille, by event family), the story, and the choices; a choice the crew or standing closes stays on the list in dim with `closed: <why>`. |
| Standing | `a` | Five factions on bars that run both ways from zero, with what the engine does at the current value (docks closed at -40, offices shut, better jobs at +25, recruits refusing at -25, military posts turning a smuggler away), read from the engine's own constants. |
| Others | `d` `y` `l` `n` | The dock chooser, the yard, the loan office (debt against the credit limit, payday) and the news, in the same frames. |
| Intro | first visit | The choice of background over a star strip with a freighter crossing, and the perk. |

Colours are the theme's roles (`DoorStyle` through `DoorFrame::style`) plus three accents of the ship's own (hull grey, engine amber, starfield blue) and one accent per faction; the board brings each colour down to the member's depth. Nothing is conveyed by colour alone: values are printed, `closed:` and `NOT ENOUGH FUEL` are words.

### Sizes, tiers and the look-around

* A window of at least **60x15** gets the full screens; a smaller one gets the L1/L2 text screens (also the look-around). Layouts reflow with the area (a side panel on the chart and a third pane on the bridge from 96 columns, taller panels with height).
* **The board's box caps the door at 74 columns** (`MAX_WIDTH` 76 less the border), so at 100x30 and 132x43 the door gets more rows but not more columns. The screens are written for any width and are tested at 130 columns; the cap is a server setting (see [coordination](../coordination.md)).
* **ASCII tier:** the chart is redrawn in plain characters (lanes as `.`, range rings as `:`, a letter per station type, `@` for the ship), bars are `#` and `-`, and the composite swaps the box drawing and block characters. The tests require pure ASCII. The Nerd Font tier draws exactly what the Unicode tier draws (a door has no icon font, D104).
* **Look-around:** every screen has a text view (`tests/ui.rs`): the chart lists every lane from the ship with its spans, fuel and days and a route to the cursor; the planner lists every leg; the boarding view maps the rooms and their exits; combat gives the reactor split; the rest reuse the L1/L2 text. Lines are wrapped to 71 columns.
* The screen holds no rules: it calls `Ship::fuel_for`, `Ship::days_for`, `Player::transit_chance`, `Player::event_weights`, `Player::gear`, `Player::firepower`, `Player::run_chance`, `market_rows`, `contracts::board` and the constants the engine publishes.

### Decisions and limitations worth knowing

* **Route flying is one leg per key.** A multi-jump autopilot would roll several events in one key; one leg lets an event, a fight or a refusal interrupt, and the plan remains.
* **Only charted systems are waypoints.** A route never passes through an uncharted signal; it may end at one. A signal two lanes from the chart is not shown.
* **Market sparklines use remembered prices.** The engine keeps one memory of each station (`Intel`), not a history, so the sparkline is the price landscape across stations, not a price over time. A time series would need the engine to keep one.
* **The fight log and the footer message** come from the session's log, which now keeps 24 lines (the text screens still show the last four).
* **No animation.** Every frame is a pure function of the game state, so a recorded replay draws the same screen. Effects wait for UI-8.
* **Not built:** mouse (UI-9), a heat overlay for one chosen good, a Nerd Font icon set (D104: none for doors).

### Verified, and not

* `cargo test -p russh-bbs-door-long-haul --test ui`: 19 scenes x 3 sizes x 2 tiers as stored snapshots (`tests/snapshots/`, rewrite with `UPDATE_SNAPSHOTS=1`), pure ASCII in the ASCII tier, Nerd Font equals Unicode, wide and tiny windows, a look-around for every screen, planning spends nothing, and a full trip by keys (see the test for what each leg does and where it injects a scene).
* Walked over a real SSH server (`russh-bbs-server` with SQLite) from a russh client at 80x24, 100x30 and 132x43 in the Unicode tier with colour, through every screen above; the screenshots are in the pull request.
* **Not exercised:** the OpenSSH client (none in this environment), telnet, a real terminal emulator's rendering of the ambiguous-width shapes (`▲ ● ○ ◆`), the Nerd Font tier on a real terminal.
