# Saltmarch: the flagship door (design)

Status: **Proposed** (owner asked for it on 2026-11-04). Planned for M4 on the door framework. Nothing here is built.

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
