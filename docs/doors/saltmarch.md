# Saltmarch: the flagship door (design)

Status: **Engine (M4-S1), expeditions, events and skirmish combat (M4-S2), the screens (M4-S3) and the social layer (M4-S4: raids, pacts, the player exchange, letters, renown and the hall's book) implemented, in review**; quests, factions, contracts, causeways with tolls and the full content budget are **planned** (S5 and later). Owner asked for the game on 2026-11-04. See [Implementation notes (M4-S1)](#implementation-notes-m4-s1), [(M4-S2)](#implementation-notes-m4-s2), [(M4-S3)](#implementation-notes-m4-s3) and [(M4-S4)](#implementation-notes-m4-s4) for what exists; the rest of this document is still the design.

Saltmarch is an original daily-turn clan game. It uses the mechanics the BBS door genre shares (a daily allowance of turns, a home base you build up, a party you send out, raids on other players, a league table that resets by season). Its world, systems, names and text are its own; it is not modelled on any one existing game.

## The pitch

The sea took the old coast. What is left is a salt marsh that floods and drains twice a day: mudflats, reed islands, half-drowned towns, peat bogs, and the bones of the old world under the silt. Each player leads a **clan** that has staked a **hold** on a scrap of high ground. You send crews out across the marsh between tides to cut peat, rake salt, salvage the drowned towns and fight what lives in the reeds. You come back before the water does.

The tide is the heart of it. It is both the clock and the map: what you can reach, how long you can stay, and what you risk all depend on where the water is.

## The daily loop (5 to 15 minutes)

1. **Morning report.** What happened overnight: raids on your hold, what your workers made, injuries healed, a letter from a rival, the tide table for today, the season's standings. One screen, read in ten seconds.
2. **Spend the day's turns.** Each clan gets **tide-turns** per day (default 30, sysop-set). Each action costs turns:
   - **Expeditions** into the marsh (most turns).
   - **Work** at the hold: build, craft, train, trade.
   - **Raids** on other players' holds (capped per day).
3. **Bank or risk.** Crews still out when the flood tide comes in must race back, wait it out on an island (lose turns, gain night events), or be cut off (lose cargo, risk injury). This is the main risk-reward choice of every session.
4. **Night.** The scheduler (M3-C) runs the daily tick: production, healing, aging, events, raid resolution queued in the day, market prices, and the next tide table.

Nobody gains by staying on all day: turns are the whole budget, and the tick, not online time, drives progress. That keeps it fair on a board where people call in once a day.

## Systems

### The tide

- A tide table is generated per day from the board's seed: two highs and two lows at shifting times, with **spring** and **neap** weeks (bigger or smaller tides) on a 14-day cycle and storms as rare events.
- Every marsh tile has an **elevation**. At a given tide a tile is dry, wet (slow, costs extra turns), or drowned (impassable except by boat).
- The current tide is shown on every expedition screen as a strip across the top: water level, time to turn, and a warning band.
- Spring tides open deep salvage (drowned towns stand dry for an hour) and make floods deadlier. Neap weeks are safe and dull. Players plan around the calendar.

### The marsh (map)

- A procedurally generated map per season, from a seed (the same for everyone on the board), about 64 by 64 tiles. It is revealed by exploring, and charted tiles are kept per clan.
- Tile kinds: mudflat, reedbed, peat bog, salt pan, channel, island, drowned town, causeway, wreck, mere, and the **Deep** (the far edge, where the hard content is).
- Points of interest are placed by the generator from templates: camps, shrines, salvage sites, lairs, hermits, traders, ruins with multi-room interiors.
- **Causeways** are built by players, persist for the season and are shared: anyone may use a causeway another clan built, for a toll the builder sets. That gives the map a social layer.

### The clan and its people

- A clan is 3 to 12 named **crew**. Each crew member has:
  - a **trade**: wader, raker, salvager, fighter, healer, boatwright, smith or reed-witch;
  - three **attributes** (grit, wit and reach);
  - **skills** that grow with use;
  - one or two **traits**, good and bad (steady, greedy, sea-born, fearful, lucky, drinks);
  - **injuries**, which last days and can scar into a trait;
  - an **age**. Crew age with the seasons and retire with a short epitaph in the hall.
- Crew are recruited at the hold's gate (a few candidates a day, better ones as the hold grows) or met on the marsh.
- **Bonds:** crew who survive hard expeditions together form bonds that give small bonuses side by side. A bonded crew member's death has a morale cost. This is where the stories come from.

### Expeditions

- Pick 1 to 5 crew, supplies (food, rope, lanterns, boats) and a target. Movement and events cost turns.
- Each tile entered may raise an **event** drawn from weighted tables by tile kind, tide, time of day, season and the clan's history. Events are short choice scenes with two to four options. Outcomes depend on who is in the party: a reed-witch reads omens, a boatwright fixes a hull, a greedy crew member may pocket loot. There are many small events and a few rare, memorable ones.
- **Encounters** with marsh creatures, bandits and rival crews resolve in **skirmish** combat (below).
- **Cargo** is limited by crew and boats; overloading slows you, which matters when the tide turns.

### Skirmish combat

- Short tactical fights on a three-row board (front, middle, back) with initiative order, drawn in ratatui as a compact battle panel.
- Actions: strike, brace, shove (move an enemy between rows), throw, mend, flee. Trades unlock one special action each (a salvager's net, a reed-witch's hex, a smith's guard).
- The terrain and the tide matter: fighting knee-deep penalises everyone, and an incoming tide adds a turn limit. Retreat is always offered, at a cost.
- Each fight is quick (5 to 10 decisions). A **quick resolve** option lets the player auto-fight a routine encounter with predictable results.

### The hold

- A grid of plots on the high ground. Buildings unlock along three tracks:
  - **Shelter:** huts, longhouse, infirmary. More crew, healing.
  - **Work:** salt pans, peat stacks, smokehouse, smithy, boatyard. Production and crafting.
  - **Defence:** palisade, watch-tower, sluice, mudworks. Raid resistance.
- The **sluice** is unique to the setting. It can flood the approach to the hold at a chosen tide, which makes raids at those hours much harder. Building and timing it well is a skill.
- Upkeep (food and peat) is paid at the nightly tick; a starving hold loses morale and crew.

### Economy and crafting

- **Resources:**
  - raw: salt, peat, reed, timber, iron, silt-glass;
  - goods: rope, preserved food, boats, tools, arms;
  - rare: relics from the drowned towns.
- **Recipes** are learned by building workshops, finding plans in ruins, or trading knowledge with other clans.
- **The market** at the Tidecourt (the neutral town) sets prices from supply and demand across the board, updated nightly. Players can sell, buy and post standing orders. Prices are visible, so speculation is a real strategy.
- Credits from M3-C are not used inside the game (the game has its own currency, **tallies**), so door play cannot be farmed for board credits unless the sysop links them.

### Rivals, raids and diplomacy

- **Raids** cost turns and send a crew against another clan's hold at a chosen hour. They resolve at the nightly tick against the defender's buildings, sluice timing and home guard, and the defender gets a battle report. Raiding is capped per day and per target.
- **New-clan protection** lasts the first 7 days, or until the new clan raids first.
- **Pacts:** two clans can sign a pact (no raids, shared causeway tolls, a mutual-defence bonus). Breaking a pact is allowed but public, and it costs reputation with the Tidecourt.
- **Letters** between clan leaders go through the board's own message tools where they exist; in-game short messages are sanitized and rate-limited like every other text.
- **Reputation** with three marsh factions (the Rakers' guild, the Tidecourt and the Reed-folk) opens different quests, prices and endings.

### Quests and story

- **Hand-written quest chains** (at launch, about 12 chains of 3 to 7 steps each), tied to the factions and to places on the map. They have branching outcomes that change faction standing and sometimes the map itself (a channel dredged, a causeway washed out, a lair cleared for everyone).
- **Generated contracts** from the faction boards for daily variety: deliver, clear, salvage, escort.
- **The Deep:** the endgame region, reachable only on spring tides with boats and a seasoned crew. Its ruins hold the season's centrepiece relics.

### Seasons and the hall

- A **season** lasts 8 weeks (sysop-set). At the end, the map, holds and markets reset. Kept across seasons:
  - the **Hall of Tides**: past standings, epitaphs of retired crew, and the clans that reached the Deep;
  - one **heirloom** a clan carries into the next season (a cosmetic title, or a small bonus that cannot stack).
- The league table ranks by **renown**: a mix of hold size, the Deep reached, quests, pacts kept and raids won. Raiding alone cannot top it.
- Inter-BBS leagues (shared seasons across boards) wait for federation in M8. The design keeps a clean boundary so a season summary can be exchanged later.

## UI (ratatui, on the board's screen framework)

- **Layouts:** it uses the board's layouts and themes (M3-A). The classic hotkey layout is the default; lightbar works for every menu.
- **Screens:** morning report, hold (plot grid), crew roster and detail, expedition planner, marsh map (fog of war, tide overlay, causeways), event scene, skirmish panel, market, Tidecourt, letters, league table and hall.
- **Map rendering:** per-tile glyphs from the icon registry in three tiers (Nerd Font, Unicode, ASCII). The tide is drawn as a colour band where colour is on, and as a glyph change where it is off. The game is fully playable in the ASCII tier and at 80x24.
- **Accessibility:** every map screen has a text "look around" view that describes the nearby tiles, so the game works with a screen reader (M5 accessibility mode).
- **Input:** single keys, arrows for map moves, the editor (M3-B) for letters.

## Fit with the platform

- **Rules in core, UI thin.** The game lives in its own crate (`crates/doors/saltmarch`) on the M4 door framework. The rules are a pure, deterministic engine: state plus an action gives a new state plus events, with a seeded RNG. All game time comes from the injected clock. This makes the game testable by replay and lets one save state be checked against a log.
- **Sandbox (M4 rule).** The door has no filesystem or network access. It reads and writes only through the framework's save API, which is backed by the store (both backends, migrations).
- **Scheduler.** The nightly tick is one scheduler job (M3-C). It runs per board, is idempotent per day, and catches up after downtime by replaying missed days with a cap.
- **Access.** Playing needs a door permission; staff tools (reset a clan, end a season early, ban a player from the door) are permissions, rank-checked and audited.
- **Sanitizing.** Clan names, crew names chosen by players and letters go through the shared sanitizer and the reserved-name rule.
- **Limits.** Turns per day, raids per day and letters per day are game settings, separate from the board's limits.
- **Telnet and ssh** behave the same (D94).

## Content budget (launch)

- **Map:** 11 tile kinds, 40 point-of-interest templates, 6 ruin interiors.
- **Events:** about 150 expedition events across tile kinds, tides and seasons, with at least 25 rare ones, all written for this setting.
- **Quests:** 12 quest chains (about 60 steps in total) and 20 contract templates.
- **Crew:** 8 trades, 16 traits, 30 injuries, bonds, aging and epitaphs.
- **Hold:** 18 buildings in 3 tracks, about 40 recipes, 3 factions with 5 reputation tiers each.
- **Combat:** 12 creature and bandit types with distinct behaviour, and 3 Deep guardians.

The content is data, not code: RON or TOML tables in the crate, validated at load by a schema test, so writers can add events without touching the engine.

## Build plan (proposed M4 slices)

1. **Door framework:** the door trait, session hand-off from the menu, the save API on the store, the per-door permission, a scheduler hook, the sandbox rules, and a tiny example door. This is already in M4.
2. **Saltmarch engine:** deterministic core, tide and map generation, the clan, crew, hold and economy models, and replay tests.
3. **Saltmarch expeditions and combat:** events, skirmish, and the content loader with its schema tests.
4. **Saltmarch UI:** all screens on ratatui across the three tiers and layouts, plus the accessibility look-around.
5. **Saltmarch social:** raids, pacts, market, letters, league table and hall.
6. **Content pass:** the full launch budget written and balanced by simulation (an auto-play harness that runs many seasons and reports economy and win-rate curves).

## Open questions for the owner

- **Turns and season length:** 30 turns a day and 8-week seasons as defaults?
- **PvP intensity:** raids on by default, or off unless the sysop enables them?
- **Credits link:** should renown or season placement pay board credits (M3-C)?
- **Name:** "Saltmarch" is a working title.

## Implementation notes (M4-S1)

Crate `crates/doors/saltmarch` (`russh-bbs-door-saltmarch`, door id `saltmarch`, save version 1, world version 1). Decision: D100.

**Implemented and tested** (engine only; the door is not yet registered on the board):

| Design item | Where | Verified by |
|---|---|---|
| Tide table per day, spring/neap on 14 days, storms, three water bands, warning strip data | `tide.rs` | unit tests; property tests for determinism, smoothness and bounds over random seeds |
| Seasonal 64x64 map from a seed: 10 generated tile kinds (+ causeway for players), elevations, hold sites, tracks to the Tidecourt, points of interest from templates, the Deep | `map.rs` | property tests: determinism, well-formedness, spacing, **every hold reachable on foot at a neap low**, POIs on their templates' tiles |
| Clan and crew: 8 trades, grit/wit/reach, skills that grow with use (levels 0 to 10), 13 traits (4 are scars), 8 injuries that heal and can scar, aging by the fortnight with retirement and epitaphs, bonds from shared hardship with morale cost on a death | `crew.rs`, `clan.rs`, `engine.rs` | `engine/tests.rs`: each trait field, skill, injury and bond rule changes an outcome in a test (see below) |
| Hold: 5x4 plots opened by shelter, 3 building tracks with one construction each, requirements, caps, labour sped by the right trades, crew cap, healing, morale, peat upkeep, the sluice | `hold.rs`, `engine.rs` | unit tests |
| Economy: 13 resources, tallies, 7 recipes (workshop-learned or locked), shifts by hand (tide-gated), nightly production by posted crew, the Tidecourt market with a 10%/15% spread and nightly price drift | `types.rs`, `world.rs`, `engine.rs` | unit tests; store test that a sale moves tomorrow's price down |
| Tide-turns: 30 a day, each turn moves the clan's clock 48 minutes; `Wait` lets the water move | `engine.rs` | unit tests |
| Nightly tick: the framework's tick advances the shared world; a clan's own nights run when it next opens (D100) | `lib.rs`, `engine::catch_up` | replay and store tests; nine days at once equals nine days one by one |
| Saves through the framework: versioned, capped, never overwriting what it cannot read | `clan.rs`, `world.rs`, `codec.rs` | `tests/play.rs` |
| Content as data: 7 TOML tables with schema tests | `content/`, `content.rs` | `tests/content.rs` |
| Sandbox audit | all of `src/` | `tests/sandbox.rs` |

**How outcomes are changed (the RPG rule), each with a test:** accident odds fall with *steady* and *sea-born* and rise with *clumsy*, *drinks* and the water; *greedy* pockets part of a shift, *lucky* doubles it; *fearful* and *haunted* refuse the flood; *quick study* trains faster; *hardy* heals faster; an infirmary or a healer on the crew heal faster; a skilled hand out-produces a novice, an own-trade hand an off-trade one; a bonded pair out-produces strangers; hard water forms bonds sooner; losing a bonded partner costs morale; a scar from a broken leg lowers wet-water yield; a tower and a wader that reads the flats chart further; grit wanes with age; low morale drives crew away, and the unbonded go first; an empty larder starves the weakest on the third night and writes an epitaph; a sluice floods the approach only when built, set and the water is up.

**Headless 60-day run** (`tests/sim.rs`, six simulated clans through the framework's simulation; builders, workers and an idler): builders end well ahead of workers, who end well ahead of the idler, and the numbers are in the pull request. They are placeholders for the balance slice (S5).

**Deliberately not in this slice:** expeditions, events and skirmish (S2); the real screens, the three icon tiers and layouts (S3; `session.rs` is a plain debug screen that is also the look-around); raids, pacts, letters, the league, the hall screen and causeway tolls (S4); the full content budget and the auto-play balance harness (S5); a date rule for ending a season; inter-BBS summaries.

**Differences from the design text, on purpose:** a raw `Fish` resource (the smokehouse turns it into preserved food); `reed-garden` and `drift-rack` buildings so reed and timber exist before expeditions do; a new clan starts with a fish weir and a peat stack already staffed; the shifts and the nightly output are in percent of "a standard hand" (see `Crew::power`). Content counts are below the launch budget (18 of 18 buildings but 7 of about 40 recipes, 15 of 40 map templates, 13 of 16 traits, 8 of 30 injuries).

## Implementation notes (M4-S2)

Same crate (`russh-bbs-door-saltmarch`), save version 2 (version 1 saves migrate), world version 1. Decision: D108. The door is registered on the board with a plain text screen; the real screens are S3.

**Implemented and tested:**

| Design item | Where | Verified by |
|---|---|---|
| Expeditions: 1 to 5 crew, a kit of food, rope, lantern peat, tools, arms and up to two boats paid from the stores, a target, the march with the tide, cargo limits and overloading, returning | `trip.rs` | `trip/tests.rs`: refusals change nothing; stores paid and returned; rations and peat eaten by the hours out; overloaded and night-without-light steps cost a turn more; only a boat enters drowned ground |
| The race against the flood: race back (the route avoids water as it will be on arrival), wait it out on high ground (`Camp`), or be cut off (day over) or swept (water over the ground) | `trip.rs` (`route`, `Reach`, `after_action`, `conclude`) | a property test over targets and three tide states: a march always arrives, spends exactly what the route said and is never swept; camp on ground that floods in ten turns sweeps the party (half the cargo, a boat lost) and a boat saves it; the day running out cuts it off; a trip left open overnight is cut off by `catch_up` |
| Gathering (10 kinds of ground) with diminishing returns, boats for open water, finds | `trip.rs`, `content/gather.toml` | trade, skill, luck, injury and hands change the take; greedy hands pocket; a third pass is refused; the hold's own ground is for shifts |
| Events: 62 scenes (7 rare), weighted by tile, place, water, time of day, third of the season and history; 2 to 4 options, offered by the party's trades, skills, traits, scars, bonds, supplies, size, boat and the clan's marks | `lore.rs`, `content/events.toml`, `trip.rs` (`pool`, `View`) | see the RPG rule list below; a test builds a party and a history for every option of every scene and checks it can be offered, and runs every outcome |
| Skirmish: three rows, initiative, strike, brace, shove, throw, mend, flee, one special per trade, terrain and tide, round limit, quick-resolve, odds read, injuries and scars | `combat.rs`, `content/foes.toml` | `combat/tests.rs`: rows gate melee; arms reach a row deeper; every special; cooldown; the water; flight odds; foe behaviours; 300 random fights end within the limit with a verdict the board agrees with; the odds read matches quick-resolve results |
| Content loader: events, foes and groups, gathering as TOML with schema tests and cross-table checks | `lore.rs`, `content.rs` (`Texts`, `Catalog`) | `tests/content.rs`: every table passes; bad edits (a bad effect or condition, risk without a loss, a fight that is not last, a bad mark, too few options, no way through) are named; cross links fail the catalog; the launch shape |
| Saves: the trip, the clan's marks and the counters in save version 2; the trip's dice in the save | `clan.rs`, `trip.rs`, `lib.rs` (`migrate`) | a version 1 save loads through the store and writes back as version 2; a trip in a scene or a fight survives a reopen with the same screen; the same save and key give the same result |
| Sandbox audit | all of `src/` | `tests/sandbox.rs` |

**The RPG rule, each with a test (`trip/tests.rs`, `tests/expeditions.rs`):** the same scene offers a different list to a different party (`sucking-mud`: wader, rope and size; `castaway`: healer); a scar closes one door and opens another (a limp rules out running for it in `storm-warning`; any scar opens an option in `deep-hum`; a hamstrung leg heals into a limp at the table's rate); a bond opens the ferryman's fair coin; skill (8 points a level), luck and clumsiness move the odds shown, and a skill used is a skill learned; a steady hand is hurt less often than a clumsy one; history decides what can be drawn (`water-remembers` needs the dead robbed, `hermit-gift` the hermit fed, `hag-lair` the bell rung; putting it right removes the mark and the scene); 22 marks are each set by one scene and read by another; a rare scene is seen once; the reed-witch makes the whispering reeds three times as likely; a fighter's skill, arms, injuries and the scar traits change a fighter's numbers; the trade of the party decides the special on offer; the water decides which foes are stronger and which shoves work.

**Headless run, 60 days** (`tests/expeditions_sim.rs`, 26 clans over two seeds; the screen is read and answered with keys through the framework's simulation; each bot also works three shifts and shops before going out): the numbers in the pull request. In short: a margin of eight turns brings 74% of trips home and a margin of two turns 55%; a short range with gathering brings 94%; brawlers with arms win 65% of 34 fights, are routed in nine and lose five people. The outcome mix is by design the choice of the player.

**Differences from the design text, on purpose:** lanterns are peat (the lamp burns one peat per six night turns and the dark costs a turn a step), because lanterns would be a thirteenth resource and change the S1 save; the three rows are one set each side (the party stands by trade, the foes by kind) and a player cannot rearrange them, only shove; quick-resolve is the same engine on a fixed policy and not a separate formula, so it is "predictable" in that its result follows the odds read shown; ruins are walked a room at a time (`Delve`) with a scene per room, and the number of rooms comes from the template; a lair that is cleared, a ruin that is looted and a shrine that is robbed are remembered **by the clan**, not by the world (S4 decides what is shared); there is no player-built causeway yet.

**Not done here:** the real screens, the three icon tiers and layouts (S3; `session/trip_ui.rs` is a plain debug view that doubles as the look-around, with tags the bots read); causeways and tolls, quests and chains, factions and reputation, contracts, raids, pacts, letters, the league and the hall screen (S4); the full content budget (150 scenes with 25 rare, 12 foes plus 3 Deep guardians, 40 recipes) and the auto-play balance harness (S5). **Known balance facts for S5:** a shift of S1 earns more common goods per turn than a day of gathering, so expeditions are worth it for iron, silt-glass, rope, relics, charting, bonds and scenes; expedition-only play (no shifts) starves the hold; the water rises about 12 levels a turn at mid-tide, so low ground is a trap twice a day and a party that cuts the return fine is swept or cut off in about 40% of trips, and one that keeps a margin in about 25%.

## Implementation notes (M4-S3)

Same crate, **save version 2, world version 2** (version 1 worlds migrate; saves are unchanged). Decision: D113. All screens are in `src/session/ui/` and draw with ratatui widgets into the door's own buffer (D104); the engine menu in `session.rs` (`Session::entries`) is still the input model, so every key is an entry the simulation bots also press, and `Session::lines` is still the tagged text the bots read. **No game rule is in the UI**: it reads `Clan`, `Tide`, `Map`, `trip::Reach`, `trip::fail_chance`, `Fight` and the catalog.

**Screens, each wired to the engine and covered by `tests/screens.rs`:**

| Screen | What it shows | Source of the numbers |
|---|---|---|
| Title / naming | a picture of the hold at dusk, the clan name box | `art.rs` (decoration only) |
| Every screen | header (clan, day, a turns gauge, tallies) and the **tide strip**: the water as a gauge with the flood line marked, rising or falling, time to the turn, the warning band as a word and a colour, and the day's curve with the clan's clock marked | `Tide::level/next_turn/warning/table`, `Env::minute` |
| Morning report | tiles (nights, made, healed, setbacks, news), what happened (grouped), what did not (raids, letters: none), today's tide as a braille chart with the flood line and the clock, the flood windows and low waters, the season standings | the events `engine::catch_up` returned (`Session::ledger`), `Tide`, `World::standings` |
| Home and Hold | the 5x4 plot grid as a map (built, under construction with labour left, free, locked; the three tracks in three colours, crew posted as dots, what it makes at larger sizes), upkeep gauges in nights of food and peat, morale, crew, the sluice, the stores, the build and work orders | `Hold`, `Catalog`, `Clan` |
| Crew | roster table with a highlight; detail pane with age against retirement, grit/wit/reach bars, traits with scars marked, injuries with days to heal, bonds, skills as bars; the orders that concern that crew member; the gate's candidates at larger sizes | `Crew`, `Catalog::trait_def/injury`, `Clan::partners` |
| Expedition planner | party with ticks, kit presets and what is packed against what is held, the load against what the party can carry, targets with their cost in turns, a cost preview: turns out and back against the turns left, the water and the ground on arrival, rations needed, a verdict, and the set-out key | `trip::Reach` (the search the march uses), `Trip::capacity/load`, `Tide::level` |
| Marsh map (on a trip and from home) | half blocks (two tile rows to a cell, so the marsh is as square as it is on paper) with fog of war, relief, the **tide as a colour band** (dry, knee-deep, under) and, with colour off or in the ASCII tier, as a change of glyph (`-` and `~`); the hold, the Tidecourt, places by kind, the party, the route to the target; a side panel with the party, the load, the ground and when it goes under, the way home, and the orders; arrows walk on a trip and pan the chart from home | `Map`, `Clan::is_charted`, `Access::of`, `trip::safe_level/turns_until_drowned`, `Reach::path_to` |
| Event scene | a picture of the place at that time and water (seeded by the scene, so it is stable and differs from scene to scene), the title (marked when rare), the words, the choices each with its trade, its chance to go badly as a number and a warning when it can start a fight | `Catalog::event`, `trip::fail_chance`, `option_info` |
| Skirmish | initiative order, odds, water, round limit, clan morale; three ranks a side with the fronts facing each other; a card per unit (health gauge, badges, what a foe means to do); the orders as verbs with the key that aims each | `Fight` |
| Market | price table with a fortnight sparkline, the day's change, what is held, the keys to sell and buy; at larger sizes a big sparkline and the spread | `World::prices`, `World::price_trail` |
| Tidecourt, the post, league, hall | the town square with market movers and notices; the post (no letters yet, and the notices that are true today); the league table with a plot of where the holds stand; the hall with the clan's record | `World::standings`, `Clan::hall`, `Clan::stats` |
| Quay | today's tide chart, the highs and lows, the fortnight's springs and neaps as a bar chart | `Tide` |

**Responsive.** `Size` is Tiny (the door's area inside an 80x24 window, about 74x18, once the board has taken its frame: 74x20), Compact (80x24), Roomy (100x30) and Grand (132x40). The tide strip is one row at Tiny, two at Compact and three above; the hold's plots lose their boxes; the market loses its frame; the crew pane puts the three attributes on one line. Below 60x16 the screen is the plain text screen. The ASCII tier draws only ASCII, chosen in the screens (box drawing, blocks and braille are also turned into ASCII by the framework, but a symbol the framework does not know would stay). **The Nerd Font tier draws what the Unicode tier draws**: the board's compositing neutralizes private-use glyphs in a door's cells (D104), so a door cannot use them (a test pins that the two tiers render identically).

**Colour.** The theme's roles (`DoorStyle`) plus a few Saltmarch accents: sea greens, salt whites, peat browns, reed gold and rust (`ui::hue`). The board composites a door at 16 colours today (`server::doors::draw::depth`), so the screens are designed to be legible at 16 and read best in a truecolor terminal once the member setting has a depth (D104). Every cue colour carries is also a glyph or a word: the warning band is `HIGH WATER` / `FLOOD` (with arrows), the water on the map is `-` and `~`, the odds are `++ easy` or `-- grim`, health is a number as well as a bar.

**Keyboard.** Letters `a` to `z` press the entry with that letter (as before). Up and Down move a lightbar and Enter presses the marked entry; on the marsh the arrows walk (the step entries) and on the chart they pan; on the crew screen Up and Down choose the crew member and Left and Right move among their orders. The line at the bottom of every screen spells out the order on the lightbar in full (the lists cut long orders short). No mouse (UI-9).

**Look-around.** `look_around` is the tagged menu text, then `Looking around:` and sentences for the screen: the ground next to the party and the places within six tiles with their direction and distance, the tide in words, the hold plot by plot, the roster with traits, injuries and skills, the prices, the planner's cost, a scene with each choice and its risk, a skirmish rank by rank with what each foe means to do, the league standing. A test requires one for every screen and that no two are the same.

**World version 2.** The price history (the last 14 nights, for the sparklines) and each clan's score (for the league) are in the shared world; both are display-only, no rule reads them, and a version 1 world migrates with no history and scores of zero (`World::upgrade_v1`, a test builds the version 1 bytes by hand). A session writes its clan's score with the same compare-and-swap edit as every other world change, and only when it changed.

**Differences from the design text, on purpose:** the Tidecourt screen is a hub for the market, the post, the league, the hall and the quay; the post shows no letters (none exist until S4) and says so; the league ranks the score `Clan::score` computes (buildings, crew skill, bonds, crew, tallies) because renown needs raids, pacts and quests (S4); the morning report says "Raids: none" because there are none until S4.

**Tests (`tests/screens.rs`):** every screen (16, including a scene and a skirmish reached by play) at 80x24, 100x30 and 132x43 in each tier with colour on and off, plus 74x18, 76x19 and 60x16 (fits, nothing lost, ASCII-only in the ASCII tier, the tide strip on every screen); text snapshots of every screen at the three sizes in the ASCII and Unicode tiers (`tests/snapshots/`, rewritten with `SALTMARCH_BLESS=1`); the Nerd Font tier equals the Unicode tier; a look-around for every screen; the map look-around names the ground, the places, the hold, the Tidecourt and the water; the tide changes the map as glyphs without colour (the glyph count follows the water over a fortnight) and as colour with it; one whole day through the keys (the report, a plan and departure, arrows walking, a scene answered, a skirmish fought, home and the hold again); the sandbox audit; the world migration. `SALTMARCH_SHOW=1 SALTMARCH_DUMP=dir cargo test -p russh-bbs-door-saltmarch --test screens show` writes every screen as a `.cells` file, and `docs/qa/m4-s3-screens/render.py` turns one into a PNG with its colours.

**Verified by a walk with the OpenSSH client:** see [qa/m4-s3-walk.md](../qa/m4-s3-walk.md) (what was and was not walked, and the screenshots).

**Not done here:** mouse (UI-9) and effects (UI-8); a per-member colour depth (the board composites at 16 colours); raids, pacts, letters, quests, causeway tolls and the hall's season lists (S4); the content budget and the balance harness (S5).

## Implementation notes (M4-S4)

Same crate, **save version 3, world version 3** (version 2 saves and worlds migrate; a test builds the old bytes by hand for each). Decision: D-TBD. The rules are in `src/social/`; the orders and the screens that ask for them are `session/social.rs` and `session/ui/social.rs`, `court.rs`, `report.rs`. Nothing changed in the framework: everything uses `Ctx::edit_world`, `Ctx::player_text` and the daily tick as they were.

**The constraint that shapes it.** A session can read only its own clan's save, and it can change anything shared only by asking the board for a compare-and-swap edit of the world (D98); the world it opened with is a snapshot. So nothing in S4 reaches into another player's save, and nothing settles at once:

- **A clan posts what a scout could see** (`Posted`: the home guard as skirmish numbers, the defence rating, the sluice level, the stores in fives, the tallies in tens, buildings, crew) whenever it saves and the post changed. Raiders read the post, never the save.
- **A raid is an order in the world** that the nightly tick settles with the S2 skirmish engine against the defender's post (`social::raid::resolve_all`, called from `daily_tick`). Both clans get the same report as a numbered delivery.
- **Every other action is a dispatch**: a world edit that always leaves a receipt (`social::dispatch::run`). What the clan risked (goods, tallies, tide-turns, the raiding party) stays in its own save as a `Pending` until the receipt is seen; a refusal gives it back; **no receipt at all means the edit never ran, and the clan takes it back too**. A retried edit is not applied twice (its receipt is there).
- **What the world owes a clan is a numbered delivery** (parcels of goods and tallies, raid reports, letters, pact notices). The clan folds each number once and remembers the highest (`ClanSocial::credited_to`), and the world is told to forget what was folded (an acknowledgement edit after the save). A lost acknowledgement cannot pay twice; a lost edit cannot lose goods.
- The only deliberate leak: a raid's loot comes out of the *posted* stock, and the defender gives up what it still has of it. A defender that spent its goods after posting loses less than the raider took (bounded by the caps below). The sim reports the shortfall.

**Implemented and tested:**

| Design item | Where | Verified by |
|---|---|---|
| Raids against another clan's hold, settled by the skirmish engine while the defender is away, using the posted guard and defences; the defender's report; losses persist (injuries and scars through `injure`, raiders who do not come back with an epitaph, stolen stock, morale) | `social/raid.rs`, `social/ledger.rs` (`fold`), `Posted::of` | `raid.rs` and `ledger/tests.rs`; `tests/social.rs`: a raid sent from the gate, settled by the tick, read by both clans, with the posted figures, the loot and the books agreeing; one the guard beat off; both sides replayed |
| Caps and cooldowns per pair; the tide gates when a raid can go | `raid::check` (the one rule: the screen lists targets by it and the world edit applies it), `raid::windows`, `engine::WATER_UP` | two raids a day per clan, two hits a day per hold, three days per pair (and for three days after a pact is broken), two days of peace after a loss, a new clan safe for a week unless it raids first, half loot after four days unseen and no raid after ten, never against a pact partner, a hold under a third of the raider's renown is not worth a raid; high water needs a boat; a flooded approach (the sluice) gives the guard the flood rating, makes the fight knee-deep and ends it after six rounds |
| Pacts: proposals, terms, answering, breaking; reputation that remembers | `social/pact.rs` | `pact.rs`; `tests/social.rs`: offered by the screens, answered from the post, broken with two presses, the memory in the world and in both clans, replayed |
| The player market (the exchange): standing asks and bids, price then age, change back, fees, price history for the sparklines | `social/market.rs` | `market.rs`; `tests/social.rs`: a trade between two clans moves exactly the goods and loses exactly the fees; a dropped edit gives everything back once |
| Letters: templates (a table, `content/letters.toml`), one optional line through `Ctx::player_text`, rate limits, blocking, raid taunts and pact offers carried as letters | `social/letters.rs` | `letters.rs`; `tests/social.rs`: a dirty line is cleaned, six lines a minute, five a day, two unread per sender, a block that tells the sender nothing |
| League by renown, one place (`renown::of`; `Session::save` writes it to the league, `court::standings` reads it) | `social/renown.rs` | `renown.rs` and `tests/social.rs`: raids add at most 18, so raiding alone cannot top the league; a betrayal costs 15; the Deep is worth 20 |
| The hall's book: each season's top, records and ending written when the next season's world is made; each clan writes where it stood in its own hall | `social/hall.rs`, `World::successor`, `Saltmarch::open` | `hall.rs`; `tests/social.rs`: the season is ended in the store, the next visit makes the new world carrying the book, and each clan's line says where it stood |
| Morning report: raids, parcels, pacts, refusals and letters as events from `engine::catch_up_with` | `engine.rs`, `event.rs`, `ui/report.rs` | `tests/screens.rs` (`report-news`); the ledger tests |
| World and save migrations | `World::upgrade_v2`, `Clan::upgrade_v2` | a version 2 world and a version 2 save built by hand |
| Sandbox audit | all of `src/` | `tests/sandbox.rs` |

**Letters and text.** A letter is a template plus at most one line of the sender's, which went through `Ctx::player_text` (sanitized, 140 characters, six a minute per session) before it left the session; the world drops a line that would not survive the board's text rules unchanged. Names in letters are clan names that already passed `Ctx::player_name`. The screens say that letters are readable by the board and by the clan they are for; nothing here is called private and no lock is drawn.

**The RPG rule, each with a test** (a clan's build and history open and close social options): a clan with no raid won cannot gloat (the taunts need a raid won) and one with no raid on its hold cannot say it remembers; an apology needs a pact broken; thanks need a pact or a trade; trade pacts need three finished buildings and nobody on either side ever having broken a pact; mutual defence needs a defence building; shared causeways need a causeway laid; a clan that broke a pact is closed out of trade and defence pacts with *everyone* for the season, cannot offer anything for three days, and with two betrayals cannot offer or accept a truce; an outlaw (standing -60) is refused; two clans that raided each other lately cannot sign; high water needs a boat; the sluice, the palisade and the guard change the odds a raider is shown.

**Headless run, 40 days** (`tests/social_sim.rs`, 12 clans, three of each style, through the framework's simulation; the numbers are in the pull request). Raiders send about 15 raids each; a quarter to two fifths are won, and a raider with three starting hands ends with one or two (the raiders that did not come back are in the hall). Builders are robbed of about 60 tallies' worth over the run and beat a few raids off. The exchange moved about 90 tallies in a quiet economy. The invariants it asserts are the caps' own: the raid log never shows more than two raids a day by a clan, two hits a day on a hold, or a pair inside the cooldown; no new clan and no pact partner was raided; the world stays under the cap. Balance is S5's.

**Differences from the design text, on purpose:**

- A raid is not "at a chosen hour" of the night; it lands in one of the four turns of the water of the **next day** (a low is on foot, a high needs a boat) and the tick settles it. The defender's report is the morning after.
- The exchange settles with the post (the next time the door is opened), not at once, because a session cannot see the answer to an edit it sends. The screens say so.
- Raiders are the strongest fit hands in the hold (the player picks how many, 1 to 4), not hand-picked; they are away until the report, so a raid thins the home guard.
- **Mutual defence** is a bonus to the posted guard (+2 hit points a point of rating, +1 defence) when its partner is raided, not reinforcements from the partner's hold.
- **Shared causeways are not built.** The term exists, is gated by the RPG rule (open only to a clan that has laid a causeway) and is tested as a gate, but nobody can lay a causeway yet and there are no tolls, so it has no other effect and is closed to everyone today. Causeways and tolls need a map overlay and a toll step in the marsh march; they are left for a later slice (see coordination).
- Reputation is one standing with the Tidecourt (-100 to 100) and the betrayal record. The three factions, quests, contracts and the heirloom are not here.
- Letters do not go through the board's mail: they are templates and one line, kept in the door's own world, so the board's seal, reports and moderation tools do not apply to them (the text rules do).

**Not done here:** quests, factions with five tiers, generated contracts, the heirloom, causeways and tolls, inter-BBS leagues (S5 and later); the content budget and the balance harness (S5); a settings screen for the raid and letter limits (they are constants in `social::caps`).
