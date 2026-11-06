# Emberdeep: a fantasy delving door (design)

Status: **Engine implemented (M4-E1, D101); town and crafting implemented (M4-E2, D110); the full UI implemented (M4-E3, D115); social play, content and balance implemented (M4-E4, D-TBD, in review)**. The design below is the target; the implementation notes ([E1](#implementation-notes-m4-e1), [E2](#implementation-notes-m4-e2), [E3](#implementation-notes-m4-e3), [E4](#implementation-notes-m4-e4)) say what exists, how it behaves and what was not built. Shared platform rules are in [README.md](README.md). This is the "small daily-turn RPG" that M4 plans as a first game, and the smallest engine of the set.

## Pitch

Under the hill town of Cindermere is the Emberdeep: a dungeon of old forges that never went cold, built by a people no one remembers. Its levels shift each season as the fires below breathe. You play one hero who lives in the town, goes down each day, and comes back up, if they can, with coin, salvage and stories. The town grows as heroes bring things back. Deeper levels hold older forges, stranger things, and the reason the fires still burn.

The core tension: **how deep do you go today?** Each level down is richer and more dangerous, and coming back up costs torches and time.

## Daily loop

1. **Morning in Cindermere:**
   - news from the tavern board;
   - your hero's wounds;
   - who else went down yesterday and who didn't come back;
   - the town's projects.
2. **Spend torch-turns** (default 25). Each room explored, each fight and each rest costs turns. **Going back up** costs one turn per level, and if your torch runs out below, the dark gets you.
3. **Town actions:** shop, craft, train at the guilds, donate to the town's projects, drink, gamble, court.
4. **Night:** the town's projects progress, wounds heal, the market restocks, and the dungeon shifts a little.

## Systems

### The hero

- **Callings:** six of them (warden, cutthroat, ember-priest, runesmith, wanderer and duelist), each with a skill tree of 15 nodes.
- **Attributes and traits:** four attributes, and two traits picked from 24.
- **Fatigue and wounds:** wounds last days and some leave scars, which are traits.
- **Death:** a hero who dies below can be recovered by another player (for a reward) within three days. If no one comes, the hero's gear is lost and they wake in the temple, weakened.

### The dungeon (procedural)

- Each **level** is generated from a seed per season. Rooms come from about 60 room templates with variations: corridors, vaults, forges, flooded halls, shrines and lairs.
- **Depth bands:** every 5 levels share a theme (the Upper Works, the Slag Galleries, the Glass Halls, the Hearth of Kings and the Cold Furnace). Each band has its own monsters, traps and a guardian.
- **Shared dungeon:** all players explore the same seasonal dungeon. Shortcuts (lifts, rope lines and bridges) that one hero repairs stay open for everyone.

### Combat

- Turn-based, against 1 to 4 enemies, with **stances** (aggressive, guarded, evasive), positioning (front and back) and calling skills.
- **Enemies telegraph:** each enemy shows its next action, so reading intent is the core skill.
- Fights are quick (4 to 10 turns), with a quick-resolve option for routine fights.

### Gear and crafting

- **Gear:** weapons and armour with affixes, and **ember-cores** you slot into gear to change how it behaves.
- **Crafting:** runesmiths and the town forge turn salvage into gear. Recipes are found below.
- **Wear:** gear wears down and is repaired in town.

### The town (shared)

- Cindermere grows as players donate salvage and coin to **projects**: a better forge, a temple that heals faster, a lift down to a deeper band, a library that identifies relics.
- Projects are shared progress across all players on the board. Their state is the town's story for the season.

### Players

- **Parties:** up to 3 players can descend together. Each plays their own turns on the same level, asynchronously during the day, with shared doors and shortcuts.
- **Rescues:** fetching a fallen hero earns a reward from them and the town.
- **The duel ring** in town: opt-in, non-lethal duels for wagers and a ranking.
- There is no player killing in the dungeon.

### Story

- **The ember-priests' chain:** three questlines, about 30 steps in all, that answer what the fires are.
- **The tavern board:** posts contracts every day (rescue a lost NPC, recover a heirloom, kill a named beast).
- **The season finale:** the Cold Furnace guardian. When someone kills it, the dungeon collapses into a new layout for the next season.

### Seasons and the hall

- **Length:** a season lasts 6 weeks.
- **The hall:** the deepest heroes, the guardian slayers, the town's project roll, and epitaphs for heroes who were not recovered.
- **Carry-over:** one heirloom item crosses seasons.

## UI

- **Dungeon view:** a room-at-a-time view with exits, features and enemies, plus a small map of the explored level. ASCII tier: letters and lines.
- **Combat panel:** enemies with their next actions, your stance and skills.
- **Town:** a menu of places, each with a few lines of character.
- **Look around:** a text description of the room, its exits and what's in it.

## Content budget (launch)

- **Dungeon:** 60 room templates, 5 depth bands, 40 monsters, 5 guardians and 30 traps.
- **Hero:** 6 callings with 90 skill nodes, 120 item bases, 50 affixes and 20 ember-cores.
- **Story:** 100 dungeon events, 40 tavern contracts, 3 questlines and 12 town projects.

## Proposed slices

1. **Engine:** dungeon generation, the hero, combat and the torch-turn economy, with replay tests.
2. **Town and crafting:** town, gear and crafting.
3. **UI:** dungeon view, combat panel and town.
4. **Social:** parties, rescues, duels and shared projects.
5. **Content and balance:** content pass and balance by simulation.

## Open questions

- **Torch-turns:** 25 per day?
- **Duels:** should the opt-in duel ring be the only player-versus-player?
- **Size:** should this be the second M4 game, because it is the smallest?

## Implementation notes (M4-E1)

**Status.** The deterministic engine and a minimal playable text screen are built in `crates/doors/emberdeep` (`russh-bbs-door-emberdeep`, door id `emberdeep`, registered in `server::doors::Doors::builtin()` after Coin Toss). **Implemented:** the hero, the seasonal shared dungeon, combat, the torch-turn economy, death and recovery, the nightly tick, versioned saves and the shared world. **Verified:** by unit tests of every rule, replay tests through the framework, a content schema test with cross-checks, the framework's sandbox audit, a 30-day headless simulation, and a walk with the OpenSSH client (see the pull request for what was and was not exercised). **Planned, not built:** everything in the later slices below.

### What is built, and where

| Part | Module | Notes |
|---|---|---|
| Content tables (TOML, schema-checked, cross-checked) | `content.rs`, `content/*.toml` | 6 callings, 90 skill nodes, 24 traits, 8 scars, 12 wounds, 21 moves, 20 monsters (15 ordinary and 5 guardians), 10 traps, 6 weapons, 6 armours, 34 room templates |
| Hero: attributes, callings, traits, scars, wounds, skill trees, levels | `hero.rs` | one `derive()` folds everything into the numbers the rules read |
| Seasonal shared dungeon | `dungeon.rs` | a level is a pure function of `(season, epoch, depth)`; packs and traps are a pure function of the day too |
| The game (actions, rooms, options, economy, death) | `game.rs` | state plus an `Action` gives new state plus `Event`s; no I/O |
| Combat | `combat.rs` | stances, rows, telegraphed intents, moves, quick-resolve |
| Shared world and the nightly tick | `world.rs`, `lib.rs` | shortcuts, slain guardians, the fallen, the hall, news, the collapse |
| Saves | `save.rs`, `codec.rs` | hand-written binary format version 1 |
| Playable text screen | `session.rs` | every command is written `[k] label`; one text view is both the drawing and the look-around |

### Rules as built

**Time and randomness.** All time is the board's clock as a calendar day number. All randomness is the **hero's own `DoorRng`, whose state is part of the save** (D101): reopening a save or disconnecting mid-fight is not a re-roll. The session seed is used once, to start a new hero's generator.

**The day.** 25 torch-turns, refilled each calendar day. Walking into a room costs 1 (2 into a flooded hall or onto a collapsed stair, 1 there with the `pathfind` capability), going down a stair 1, a played fight 2, a quick-resolved fight 1, a camp rest 2, carrying a fallen hero home 3, and an option its own cost. With no turns left below, the only action is the climb.

**The climb back.** One turn per level, or less with a shortcut: from level `L`, with a shortcut at level `S <= L`, the cost is `(L - S) + flat(S)`, where a lift is 1, a bridge 2, a rope line 3. Then `ClimbEase` effects come off (a scar such as a limp adds), never below 1; salvage over the hero's carry adds one turn per 10 over; fatigue of 75 or more adds 1. A short torch means **the dark gets you**: each missing turn costs `3 + 2*band` hit points (plus 0 to 2), 10 fatigue, and a death save if it reaches zero. Salvage is banked in the town store (`stash`) on arrival; the town slice crafts from it.

**The dungeon.** 25 levels in 5 bands of 5; a guardian holds the way down of levels 5, 10, 15, 20 and 25. A level is a 5 by 4 grid with 10 to 13 rooms: a random spanning tree with up to three loops, the entry at the start, the way down at the farthest room, a shaft room (lift, rope line or bridge) about two thirds of the way out, 2 or 3 lairs, a shrine, and a mix of corridors, vaults, forges and flooded halls. Traps and packs depend on the day, so the dungeon shifts every night but is the same for every hero on a given day. A lair's pack **bars the way** out of its room until the hero fights it, sneaks past it, stares it down or lures it off (each only for builds that have the capability); a cleared room offers none of those again that day.

**Rooms and the RPG rule.** A room template lists options; each has `need` conditions (a capability tag, a missing tag, an attribute minimum, a calling, trait, node or level), a turn cost, an optional d20 check against an attribute, and outcomes. Two builds see different options in the same room (tested in `tests/branches.rs`): the broken lift offers a runesmith a rune-work repair (a lift, no roll), a warden a hand-hauled rope line (strength roll, injury on a failure), a tinker a roll on wits, and everyone else only a look. What a hero opens **persists for everyone**: the shortcut goes into the shared world and cuts every other hero's climb and descent.

**Combat.** 1 to 4 enemies in a front and a back row. The hero stands in a stance (aggressive: deal and take 125%; guarded: deal 75%, take 50%; evasive: deal 90%, never caught by a heavy blow, and slips strikes by `35 + evade + 2*finesse` percent) and a row (melee reaches the front row only; ranged reaches both). **Every enemy announces its next action before the hero chooses**, and executes exactly that (tested over 240 seeded fights): a strike, a heavy blow (double, dodged outright by an evasive hero), a shot (80%, any row), a brace (half damage taken, no attack), a rally (allies hit 125% next round), an advance (closing on a hero in the back row) or a guardian's breath (150%, any row). The hero acts first each round, so killing or interrupting an enemy stops its announced action. Moves cost focus (regained one a round, up to 5) and interrupt, mend, ward, mark, sunder, bleed or shove. Quick-resolve plays the fight with no moves and no reading of intents (guard when hurt, else attack), costs 1 turn, and is not offered against guardians. Armour takes a flat amount off every hit (a hit does at least 1).

**Growth.** Experience comes from kills and from options. Level `n` needs `20*n*(n-1)` in total, to level 20; a skill point per level up to level 12 (so no tree of 15 is ever finished), an attribute point at levels 4, 8, 12, 16 and 20. Each calling has a 15-node tree of 5 tiers (tier `t` needs level `2t-1`); **forks close paths for good**. Two traits are chosen at creation, and four attribute points are spent.

**Wounds and scars.** A hit that takes 35% of maximum hit points or more may leave a wound of the blow's kind; a trap, a failed option or a survived death may leave another. A wound lasts days and applies its penalties while it lasts; when it runs out it may leave a **scar**, a permanent trait that is usually a trade, and sometimes opens an option (a grim visage can stare a pack down). At most 3 scars.

**Death.** At zero hit points a **death save** (`25 + 2*spirit + effects - depth` percent, 5 to 95) decides: success leaves the hero at 1 hit point at the brink (a wound, and a coin-flip scar), out of the fight; failure leaves a body at that room. Another hero in the same room can **carry the body home** (3 turns; the town pays coin and experience); the dead hero then wakes in town the next day with their gear and pack, wounded. If nobody comes within **3 days**, or the player gives up, they wake in the temple weakened with the calling's starting gear, a quarter of their coin and no carried salvage (the stash is safe in town). A rescue is remembered for 14 days so a late return is still honoured.

**A night below.** A hero who leaves the door below stays there. The next day they have `25 - 8` turns, recover a quarter of their hit points, and meet something at their camp 2 times in 5, resolved as a quick fight.

**The shared world** (`World`, one blob per door, well under 2 KiB): the epoch, shortcuts, slain guardians, the fallen and rescues, the hall (deepest heroes) and the last six news lines. Every change is an idempotent function used twice, on the session's own copy and inside `Ctx::edit_world`, so two heroes' edits merge in any order. The nightly tick expires old bodies and, if the Cold Furnace fell, **collapses the dungeon**: the epoch rises, so every level is re-seeded, shortcuts and guardians reset, and heroes below are thrown out with what they carry. A new season (staff action) starts a clean world and a new hero.

**Saves.** Format version 1: the hero, the hero's generator state, the place, a fight in progress, the day, epoch and season, a per-level bitmask of explored rooms, and what was done today. A fresh hero is about 0.3 KiB and a mid-game hero well under 1 KiB (the cap is 64 KiB); a damaged save is refused (the screen says so and nothing is written), never guessed at. `Door::migrate` is the place a version 2 will hook in.

**Text.** The hero's name is the member's handle (already sanitized by the board). Nothing a player types is stored or shown to another player in this slice: the names in the hall, the news and the fallen list are handles, and every string is drawn through `Canvas::text`, which sanitizes. Free text (epitaphs, party chat) waits for E4 and must go through `Ctx::player_text`.

### Content shipped now, against the launch budget

| | Shipped (E1) | Launch budget |
|---|---|---|
| Room templates | 34 (76 options) | 60 |
| Monsters (and guardians) | 15 (and 5) | 40 (and 5) |
| Traps | 10 | 30 |
| Callings / skill nodes | 6 / 90 | 6 / 90 |
| Traits / scars / wounds | 24 / 8 / 12 | 24 |
| Moves | 21 | |
| Item bases, affixes, ember-cores | 6 weapons, 6 armours; no affixes or cores | 120, 50, 20 |
| Dungeon events, tavern contracts, questlines, town projects | none | 100, 40, 3, 12 |

### Seams for the later slices

- **Town and crafting (E2).** `Hero::stash` is the town store; `Hero::coin`, `relics` and the gear ids are already saved. Gear is the two starting ids per calling; drops, affixes, ember-cores, repair and wear need a save version 2. The town screen has the places for a shop, a forge, a temple and the projects; the world has room for a project table (add it with a world version 2).
- **UI (E3).** `session.rs` is the only file that draws. `Session::lines` returns head, log and keys as plain strings, so a richer layout replaces one function; the board's box is 74 columns wide at 80x24 and a test checks every screen any bot sees fits it.
- **Social, content and balance (E4).** `Fallen`, rescue and the world's news are the first social layer: parties, rewards from the rescued hero, duels and shared project rolls build on `World`. The 30-day simulation is the balance harness; its numbers are in the pull request.

### Differences from the design above

- **No gear drops or crafting yet** (E1 only; M4-E2 built them, see below); a hero fought with the calling's starting kit.
- **A lair is not a coin-flip ambush:** entering a pack's room shows the pack and the ways past it, which is how builds change a room.
- **The torch is the turn count**: there is no separate torch item yet.
- **Seasons end by staff action** (`end_season`), not at 6 weeks automatically, and the Cold Furnace's fall collapses the dungeon at the next tick rather than ending the season.
- **Duels and parties** are not built (the tavern board is built in M4-E2).

## Implementation notes (M4-E2)

**Status.** Cindermere, gear and crafting are built in the same crate (D110). **Implemented:** the market, the forge (repair, craft, slot cores), the temple, the guild hall, the tavern (drink, dice, a debt, one companion thread, the contract board), the library and the town works (shared projects), gear with affixes, ember-cores and wear, gear drops, recipes, and save and world format 2 with a migration from E1's format 1. **Verified:** by unit tests of every rule, branch tests of the RPG rule, the no-double-credit invariants, content reachability tests, schema tests (including broken tables), a pinned real E1 save and world that must keep migrating, replay tests through the framework, the framework's sandbox audit, a 30-day headless run with project progress and gear curves (numbers are in the pull request), and a walk with the OpenSSH client (what was and was not exercised is in the pull request). **Planned, not built:** the full UI (E3), parties, rescues beyond E1's, duels, the content budget and balance (E4).

### What is built, and where

| Part | Module | Notes |
|---|---|---|
| Town rules (market, forge, temple, guild, tavern, board, works, library, nightly debts and bonds) | `town.rs` | pure rules on `Game`; the screen only maps keys to `TownAct` |
| Town content (affixes, ember-cores, recipes, projects, contracts, companions, rumours) | `content_town.rs`, `content/{affixes,recipes,projects,town}.toml` | schema-checked and cross-checked; `gear.toml` and `wounds.toml` grew (bases with a band and a price, boons) |
| Gear on the hero (kits, pack, found, cores, recipes, debt, bond, contract, gifts) | `hero.rs`, `save.rs` | save format 2 (version 1 plus a trailing section) |
| Shared projects (pledges, finished-ness, ways down) | `world.rs` | world format 2; completion is derived, never stored |
| The town's text screens | `session/places.rs` | each place is a head and a menu; the menu draws the keys and answers them |

### Rules as built

**Gear.** Weapons and armour have a band (1 to 5) and a market price; a band-n piece needs hero level `3n - 2`. A `Kit` rides on each worn piece: up to three affixes (lists of the engine's effects), one ember-core, and wear 0 to 100. Wear takes 1 off weapon damage or armour per 25. A won fight wears each piece 2 (a guardian's fight 4). Spare gear (6) is kept at home and survives a death; gear found below (4) is carried like salvage, banked on arrival, and lost with the hero. A hero who wakes in the temple loses the worn gear and any core in it, except the weapon once the town has built the reliquary.

**Ember-cores.** A core is a list of effects: it can grant a combat move (`Cinder Core`: a ranged bolt; `Anvil Core`: a sundering strike; `Hearth Core`: a mending breath; `King's Core`: a finisher) or a capability that opens room options (`Tinder Core`: the lantern in a lair; `Lodestone Core`: the deep map and a shorter climb). A guardian drops its band's core. A test fights with and without the same core and shows the move refused, then offered. Slotting costs 8 coin; slotting over a core cracks the old one unless the hero is a runesmith.

**Recipes.** Known by calling (every calling has one), by skill node (`wa-pack-mule` teaches the hauler's coat, `rs-etching` the ward core), found below (a forge's chalked pattern, a strongroom's book, some contracts) or read out of a relic at the library. A recipe needs the forge level it names, the salvage in the town store, coin and, for a core, the `runecraft` capability. The runesmith's edge is the `smithcraft` tag that only that calling starts with: a quarter less salvage, a chance of a masterwork extra affix that rises with wits, cores lifted out whole, half-price repairs.

**The market** has the same four stalls for every hero on a day (a pure function of season, epoch and day), plus two stalls and a core for each market level. What is on the shelves follows the shared world: the deepest hero in the hall raises the top band on sale by one band for every three levels, up to band 4. Buying and selling are one-way tickets: a core inside gear must come out before it can be sold, a stall sells once a day.

**The temple.** Being tended: `2 * missing hp + fatigue/10` coin and 2 turns (none with the roof). A cure of one wound: 10 coin a day left, no scar (this is how the weakened wound after a death is lifted at once). The temple choir and an ember-priest each take a quarter off.

**The guild hall** drills a hero up to level 8: `8 * level` coin and a turn for a quarter of the way to the next level and a one-day boon, once a day (twice with the annex).

**The tavern.** A drink (4 coin, three a day) buys a rumour: a one-day boon, a little coin, or the whole map of one level on the next descent; the third brings a hangover. Dice for 5, 20 or 50: win on d20 plus the wits modifier of 11 or more, paying nine tenths; a stake the hero cannot cover goes on the crew's tab. A debt grows a tenth a night and, past 50, the collectors take the coin and a wound. A hero with the `sneak` capability can play loaded dice (a bonus, caught on a 1 to 3: a fine of twice the stake, the door and a wound). A hero courts one companion at a time: each call costs a gift and rolls an attribute; two good days climb a rung and each of the three rungs gives the hero something (a trap sense, a lantern, a death save, the deep map). A bond cools after five days unvisited; a stranger left alone moves on and does not come back this season.

**The board** posts three contracts a day, drawn from the ones the hero's build and depth fit (so a sneak is offered the quiet work, a runesmith the maker's plate, a priest the locket). A beast contract is done by killing that kind of monster at that depth or deeper, an heirloom or a rescue by walking into that kind of room there; the pay comes on turning in at the board; a rescue adds a turn to the climb for the soul on your back. A posting lapses after its days.

**The works.** Eleven projects share the town's coin and salvage; a gift is clamped to what the project still needs. A finished project changes the options of every hero: a better forge, a roofed temple, bigger markets, a second drill, a reading room, the reliquary, and lifts from the town to level 6 and level 11. The lifts join the way-down list for everyone and survive the collapse of the dungeon.

**No double credit.** The world keeps, per hero and project, the largest running total that hero has pledged; applying a pledge twice, in any order, or again when a session opens changes nothing. The hero's own total (`given`) and the world's agree after any run (the headless run asserts it). A full ledger (3000 pledges) refuses a new donor before any coin moves.

**Saves.** Format 2 is format 1 with a section appended: kits, pack, found, cores, recipes, debt, bond, closed companions, gifts, the contract, and the day's town state. A mid-game save is about 0.6 KiB (the cap is 64 KiB). `Door::migrate` 1 to 2 appends an empty section and re-reads the result; a damaged version 1 save is refused. The world's pledges are appended the same way. A real E1 hero and world are pinned as bytes in the tests.

### Content shipped now, against the launch budget

| | Shipped (E1) | Shipped (E2) | Launch budget |
|---|---|---|---|
| Item bases | 6 weapons, 6 armours | 29 weapons, 17 armours | 120 |
| Affixes, ember-cores | none | 25, 10 | 50, 20 |
| Recipes | none | 18 | |
| Town projects | none | 11 | 12 |
| Tavern contracts | none | 16 | 40 |
| Companions, rumours | none | 3, 10 | |
| Room options that teach a recipe | none | 4 | |

### Seams for the later slices

- **UI (E3).** `session/places.rs` draws every place: `spot_view` returns a head and a menu (`Entry { key, label, action }`), so a richer layout replaces drawing and keeps the actions. Menu keys are numbered `1` to `9` then `a` to `z` in order, and some numbers shift when an item leaves the list (repair gone after a repair); a layout may want stable keys. The bots in `tests/sim.rs` read `[k] label`.
- **Social, content and balance (E4).** The works already take gifts from every hero and the hall of donors is `World::pledges`; parties, duels, a donors' roll screen and the content budget build on them. Prices, costs and rates are in `rules.rs`, `town.rs` and the tables; the 30-day run (12 bots) is the harness. The descent-tuning handoff from E1 still stands.

## Implementation notes (M4-E3)

**Status.** Every screen is drawn with ratatui into the door's own buffer (D104), over the same engine and the same keys (D115). **Implemented:** the dungeon view and its map, the combat panel, the hero sheet and the skill tree, the town hub and each of its eight places, event scenes, the hall, hero creation, the death screen, and a look-around for all of them. **Verified:** by render tests of every screen at 80x24, 100x30 and 132x43 in each icon tier and each colour depth (shape, ASCII-only in the ASCII tier, no marker style), pinned text snapshots, a look-around test for every screen, an interaction test of one whole day, the framework's sandbox audit, and a walk over a real OpenSSH client with a screenshot of each screen (attached to the pull request). **Not built:** mouse (UI-9), effects and motion (UI-8), and the social slice.

### What is built, and where

| Part | Module | Notes |
|---|---|---|
| Entry point, layout choice, header, gauges, log | `session/ui/mod.rs` | `ui::draw` picks a layout by area: below 64x16 the old text screen, the compact layouts at the board's 74x19, a third column at 100 columns, taller art at 28 rows |
| Palette, meters, panels, key chips | `session/ui/kit.rs` | theme roles for text, Emberdeep accents (ember, cold stone, deep blue, blood, moss, bone) for art; every meter prints its numbers |
| The lit map | `session/ui/map.rs` | fog over unseen rooms, a light radius that shrinks with the torch, remembered rooms in blue; two scales; `describe` is the same map in words |
| Dungeon view and event scenes | `session/ui/delve.rs` | the room's vignette, features, options with cost and roll, the options this build cannot take with the reason, exits |
| Combat panel | `session/ui/fight.rs` | enemy cards with health, row, marks and the announced action; the order of the round; focus, ward, stance and row; actions with costs |
| Hero sheet, skill tree | `session/ui/sheet.rs` | list-and-detail with tabs; the skill tree shows what a node needs, what it closes for good and why it is locked |
| Town hub, death screen | `session/ui/town.rs` | skyline, tavern board, the places with a line of today's state, the way down |
| The eight places | `session/ui/places.rs` | a table (stalls, gear, recipes, postings, projects) beside a preview that shows the cost and what is left; the menu is still `Session::spot_view` |
| Hall | `session/ui/hall.rs` | standings (a bar chart of the deepest, the five guardians), the dead (a memorial), the world (shortcuts, finished works, news) |
| Hero creation | `session/ui/create.rs` | a card for the calling under the cursor, a three-column trait list, attribute bars |
| Scenery | `session/ui/art.rs` | room vignettes (sparks over a forge, candles, eyes in the dark), the skyline, the temple flame |
| Fixtures for tests and pictures | `showcase.rs` | saves in known states (hidden from the docs); the tests open the door on them and press keys |

### How it behaves

- **Keys are the old keys.** Every command still works as before (the bots in `tests/sim.rs` and the replay logs pass unchanged). New: `Up`/`Down` move a cursor on every list and in the town's list of places, `Enter` takes the row under it (a skill, a menu entry) or opens the place, `Left`/`Right` change the tab on the sheet and the hall, `Space` toggles a trait while making a hero. The cursor and the tab are session-only: they are never saved and never reach the engine.
- **The light is a picture.** How far the torch reaches (0 to 3 rooms) and how warm it burns come from the turns left; they change how the map is drawn and nothing else. The fog is drawn over the whole grid, so it never shows where an unseen room is.
- **Two builds, one room.** A room option the build cannot take is listed under the ones it can, with the first condition it fails (`needs runecraft`, `needs Might 12`, `a runesmith only`). The look-around says the same ("Closed to you: ..."), so a screen reader hears what a sighted player sees. A `◆` marks an option that exists only because of the build.
- **Prices that move.** The market shows the average ask of the shelf over the last fourteen days as a sparkline; it asks the engine for each day (the market is a pure function of the day), it does not invent a curve.
- **Colour.** Text uses the theme's roles; art uses Emberdeep's own colours. The board composites a door at the client's depth (D109), so the ramps are made of saturated stops that degrade to black, red, yellow and white and not to grey at 16 colours, and are smooth gradients at 256 colours and truecolor (both were walked over ssh). Nothing relies on colour alone: every meter prints its numbers, every threat has a word, the stance and the cursor have a mark.
- **ASCII tier.** The framework maps box drawing, blocks and arrows; a last pass maps the few icons it does not (ellipsis, the dagger, the lock-and-key marks). The ASCII tier is pure ASCII at every size (tested).
- **Size.** At the board's 74 by 19 every screen fits (the map and the scene side by side, two rows of gauges, three rows of log, two rows of keys). More height gives a bigger map with boxed rooms, a longer log and room vignettes; at 100 columns and more the dungeon view adds the hero's card. The board's box is capped at 76 columns, so the wide layouts show only where the window is wider than that box can be today; they are tested at full window size.

### Differences from the design above

- **"Skills growing by use"** is as the engine has it: experience comes from kills and from options, and levels give points to spend on the tree. The sheet shows the experience bar and the tree; there is no per-skill use counter, because the engine has none.
- **Event scenes** are the room's options (the design's "100 dungeon events" are still content for M4-E4); the scene is the same screen as the room, with a vignette at taller sizes.
- **The detail pane** is on the sheet, the skill tree and every place; the dungeon view has no detail pane, the room's text is the detail.

### Seams for the later slices

- **Mouse (UI-9)** is a new default-bodied method on `DoorSession`; the lists are `ratatui` `List`s with a cursor in `UiState`, which is where a click would set it.
- **Effects (UI-8)** run on the surface after `draw`; the screens draw from the state alone (no clock), so a fade between screens needs nothing from them.
- **A stored colour cap** (UI-2) changes nothing here: the ramps already degrade at every depth the board composites.
- **Social (E4).** The hall already has a tab for the dead and for the world; parties and duels add rows to the same list-and-detail widget (`sheet::list_detail`).

## Implementation notes (M4-E4)

**Status.** Parties, rescues with a reward from the rescued, the opt-in duel ring, shared town projects with a hall of donors, the story tables (100+ dungeon events, three questlines of 10 steps), the Cold Furnace's verdict and six collective endings with eight epilogues, and a content pass are built in the same crate (D-TBD). **Verified:** by unit tests of every rule (`src/tests/social.rs`, `balance.rs`), a two-account test of a company, a rescue by a stranger and by a companion, and an unfound hero's epitaph through the framework's store and nightly tick (`tests/social.rs`), a replay of a social branch, the migrations (a real E1 hero and world walk 1 to 2 to 3 through the host), pinned screen snapshots of every new screen in both icon tiers and all sizes, the framework's sandbox audit, and the 18-hero season simulation. **Not verified:** see "What was not exercised" at the end of this section.

### What is built, and where

| Part | Module | Notes |
|---|---|---|
| The commons (roster, companies, invitations, notes, the ring's ladder, epitaphs, chronicle, verdict) | `social.rs` | world format 3; every change idempotent and capped; also the duel resolver |
| The hero's half (invite, accept, leave, opt in and out, call out, collect notes, epitaph) | `company.rs` | checked against the world as the hero last saw it |
| Questlines, goals, choices and the verdict | `story.rs`, `content_story.rs`, `content/quests.toml` | goals read the hero's own tallies; a choice sets a flag for good |
| Endings and epilogues | `endings.rs` | six collective endings, eight epilogues |
| Dungeon events | `dungeon.rs` (`lay_events`), `content/events.toml` | an overlay on ordinary rooms; options are room options |
| Screens | `session/circle.rs`, `session/ui/places.rs`, `session/ui/hall.rs` | the Company, the Duel ring, the Journal and the Furnace's question are places; the hall has four new tabs (Companies, The ring, Donors, Chronicle) |
| Formats | `save.rs`, `world.rs` | save 3 = save 2 + story and social section; world 3 = world 2 + commons; `Door::migrate` 2 to 3 appends an empty one |

### Rules as built

**No player's save is written by another player's session.** Anything owed *to* a hero is a **note** in the world, collected (once, guarded by the ids the hero's own save keeps) the next time that hero opens the door. A rescue is thanked by a note out of the rescued hero's own purse; a bruise from the ring arrives the same way.

**Companies (parties).** Up to three heroes. Only an invitation brings a hero in (the world refuses a join without one); an invitation stands three days; a hero may have two outstanding; a hero who has closed the door to invitations (the opt-out, on by default) is not listed. The shared state: **a pack any companion dealt with today is dealt with for every companion today** (the world holds the list for the day; each companion's own save still says what *they* did). A companion who walks into the den takes **two thirds of the spoils** (xp, coin, salvage; once a day), so riding together is not poorer than riding alone. The company's deepest level and the companions it carried home are on the hall. The sharing is asynchronous by design: a hero sees what their company did before they opened the door (see Differences).

**Rescues.** The 3-day window and the carry from E1 stand. New: the rescuer's count of heroes carried home (a questline goal); the rescued hero, waking, sends **a fifth of their purse, at most 10 + 3 per level of the fall**, to the rescuer as a note. A **companion** carries a fallen companion for 2 turns instead of 3, the companion wakes with three quarters of their health and no wound, owes no thanks, and the carry counts for the company.

**The duel ring.** Opt-in (off by default, and a hero who steps out can no longer be called out). A hero calls out a ring member within 4 levels who is not in their company, for a stake of 0, 5, 20 or 50 coin. It costs a turn; each hero may call twice a day, never the same foe twice in a day, and a foe may be called out three times a day. The fight is the **foe's last shown strength**, fought with ring rules (hit points and armour from level alone, damage pulled halfway to the level's norm; what remains to tell heroes apart is crit, evasion and the blow), at most 12 rounds. A win returns the stake and half again, adds ladder points and gives a two-day boon; a loss gives the stake to the town works (the night's tick pledges it as "the Ring" to the first open project) and a day's bruise; the foe earns points for a defence and, if the challenger won, a bruise note. **There is no player killing.** The ladder keeps 20.

**Shared projects.** The works, the pledges roll and what a finished project changes are as in E2. New: the **hall of donors** tab (largest givers, and the largest gift to each finished project), a **12th project** (a third drill a day), ring stakes feeding the works, and the Furnace's ending reading how many are finished.

**Questlines (the ember-priests' chain).** Three lines of ten steps (30 steps): the Low Flame (from hero level 1), the Smiths' Debt (3), the Quiet Watch (5). Goals: reach a level, slay N of a monster, enter N new rooms of a kind, slay a guardian, turn in contracts, give coin to the works, reach a hero level, carry heroes home, win duels. Each line has a choice mid-way and a last choice that sets a flag; flags open and close room options and events (17 event options read one) and decide which verdicts the slayer may give.

**The Furnace's verdict and the endings.** The hero who fells the Cold Furnace's guardian is asked at once: *quench* (always open), *bank* (needs `keeps-the-flame`), *claim the heart* (needs `keeps-the-key`) or *open the gate* (needs `opens-the-gate`). That night's tick turns the verdict and what the whole town did into one of six endings, written in the chronicle before the dungeon collapses into its next epoch: **The Hearth Rekindled** (bank, at least 8 projects finished), **Embers Banked** (bank, fewer), **The Ember Crown** (claim), **The Open Gate** (open, at least 6 ways down), **The Long Table** (a quenched or opened fire, at least 5 heroes carried home and a company that carried one of its own), **The Cold Furnace** (otherwise). Each hero also has an **epilogue** from their own deeds and choices (furnace-bearer, ring champion, hearth-bearer, patron, flame-keeper, deep delver, scarred, or a hand in the dark).

**Epitaphs.** The only free text one player shows another. A hero may write one line (80 characters) in the Journal; it goes through `Ctx::player_text` (sanitized, one line, 6 a minute) and is printed in the hall only if the hero is not found (otherwise the hall prints "Lost on level N."). Everything else (an invitation, a challenge, a thanks) is a fixed template: no words pass between players.

**Dungeon events.** 40% of corridors, vaults, forges, flooded halls and shrines carry an event on a given level (a pure function of the season, epoch and level, so everyone sees the same ones), chosen from the events of that band and kind. Their options are room options, so two builds see different choices.

### Content shipped now, against the launch budget

| | Shipped (E1+E2) | Shipped (E4) | Launch budget | Met |
|---|---|---|---|---|
| Room templates | 34 | 70 | 60 | yes |
| Monsters (ordinary / guardians) | 15 / 5 | 40 / 5 | 40 / 5 | yes |
| Traps | 10 | 30 | 30 | yes |
| Callings / skill nodes | 6 / 90 | 6 / 90 | 6 / 90 | yes |
| Item bases | 46 | 120 | 120 | yes |
| Affixes / ember-cores | 25 / 10 | 60 / 22 | 50 / 20 | yes |
| Dungeon events | 0 | 101 | 100 | yes |
| Tavern contracts | 16 | 45 | 40 | yes |
| Questlines (steps) | 0 | 3 (30) | 3 (about 30) | yes |
| Town projects | 11 | 12 | 12 | yes |
| Depth bands / levels | 5 / 25 | 5 / 25 | 5 | yes |

Skill nodes stay at the design's 90 (15 per calling). Content test `tests/balance.rs` pins these numbers.

### Balance

Harness: `cargo test -p russh-bbs-door-emberdeep --test sim -- --nocapture` (env `EMBERDEEP_SEED`, and `EMBERDEEP_SIM_LEGACY=1` for the E3 shape of 12 bots over 30 days). It plays **18 bots for a 42-day season** through the real screens: each of the six callings as a *careful*, a *greedy* and a *reckless* player, who also join companies, step into the ring (wardens, cutthroats and duelists), take questline choices and answer the Furnace. It prints survival and deaths by calling and by style, depth and level curves, the economy, project completion, rescues, the ring's ladder, quest progress, the endings and gear. The pull request carries the numbers before and after.

### What was changed to balance it, and why

- **The ring was owned by one calling.** With the hero's real hit points and armour, three wardens won 205, 193 and 176 duels against 4, 15 and 33 losses while cutthroats won two of 201. The ring now uses ring rules (above). After: the best warden is 151-57, the worst cutthroat 16-194, duelists remain the strongest in the ring (about 85% wins; they are the calling that is meant to win by timing), and no calling is shut out of the ladder.
- **Wanderer and cutthroat hit points** 32 to 36 and 30 to 33: they were the two callings with deaths and the lowest level in a season (wanderer level 6.7, cutthroat 8.3 over three seeds); the cutthroat now averages 9.3 to 11.7. The wanderer still lags (5.3 to 7.3 against 10 to 12 for the best). It is viable (alive, level 5 or more, depth 5 or more in a season) but it is the weakest calling and is recorded as open for the content pass that follows.
- **Dungeon events and the descent.** More options per room cost the careful bots more turns; the harness now lets a bot skip cheap options on the way down. (A harness bug found on the way: a bot read the word "sealed" in a room's text as "the door is sealed" and quit; events made that common. Fixed in the bot.)
- **Companions' spoils.** Sharing cleared packs alone made a company poorer than riding alone (one fight pays three heroes' worth of nothing but the fighter). A companion now takes two thirds of the spoils of a den a companion cleared.

### Balance numbers

Seed 4242 unless stated; `cargo test -p russh-bbs-door-emberdeep --test sim -- --nocapture`.

| Run | Before (E3 `main`, 12 bots, 30 days) | After, same shape, no social chores (`EMBERDEEP_SIM_LEGACY=1 EMBERDEEP_SIM_NO_SOCIAL=1`) |
|---|---|---|
| Mean hero level, day 30 | 5.9 | 5.8 |
| Projects finished | 10 of 11 | 9 of 12 |
| Coin / salvage given to the works | 4095 / 1271 | 3295 / 1100 |
| Deepest level | 7 | 3 (the lifts to 6 and 11 were not finished in time) |
| Deaths | 4 | 1 |

The content pass (more rooms, 40 monsters, events) is nearly neutral on levels and slightly slower on the works.

| 18 bots, 42 days (the season shape) | Day 6 | Day 12 | Day 24 | Day 42 |
|---|---|---|---|---|
| Mean hero level | 3.4 | 4.5 | 6.3 | 9.8 |
| Projects finished (of 12) | 1 | 4 | 10 | 11 |
| Coin / salvage given | 625 / 290 | 1535 / 564 | 3545 / 1400 | 4780 / 1553 |
| Mean blow / armour | 10.7 / 2.6 | 11.4 / 3.0 | 12.3 / 3.4 | 17.4 / 6.3 |

By calling (three heroes each, careful / greedy / reckless, mean over the season; level, deepest level, heroes who died at least once): warden 11.0, 8.3, 0/3; cutthroat 10.0, 7.3, 0/3; ember-priest 11.7, 7.3, 0/3; runesmith 8.0, 7.0, 0/3; wanderer 6.7, 6.7, 1/3; duelist 11.7, 8.3, 0/3. Over three seeds the wanderer is the lowest (5.3 to 7.3) and the ember-priest, duelist and warden the highest (10 to 12); every calling is alive, past level 5 and below level 6 in a season. By style: careful level 7.3, depth 3.7; greedy 11.2, 6.8; reckless 11.0, 12.0 (the only deaths). Before the ring and hit-point changes the season had 3 deaths (a cutthroat, a runesmith, a wanderer); after, 0 to 1 per seed: **deaths are rare**, so rescues are rare in a bot season (0 to 3) and the rescue rates are measured by the two-account tests, not the harness. The ring: before ring rules a warden went 205-4 and a cutthroat 2-199; after, wardens are between 148-60 and 65-145, duelists about 80-5, cutthroats 20-190 (the cutthroat is still weak in the ring: it relies on tricks the duel does not model).

**An open finding.** With the bots joining companies (the 12-bot, 30-day shape with the Company chore) the works finish 3 to 4 of 12 against 9 of 12 without; in the 18-bot, 42-day season they finish 11 of 12. The wanderer and the duelist, in a company, fight about a fifth as often (kills 85 to 9, 93 to 16), so the company is leaving loot on the table in the harness: a hero whose companion cleared the den takes two thirds of its spoils (raising it from a third did not change the 12-bot number), so the cause is not isolated. It is recorded for the next balance pass, not fixed.

### Differences from the design above

- **Parties are asynchronous only.** The framework gives a door no read of the *current* stored world (FW-1 is a separate slice): a session sees its company as of when it opened the door, and its own changes at once. A companion who clears a pack after you opened the door is seen next time you open it. When FW-1 lands, `Game::world` can be refreshed on each key.
- **No shared party chat, and no free text between players except the epitaph.**
- **"Skills growing by use" and a 6-week automatic season** are unchanged from E1 (staff end a season).
- **Questline goals are cumulative tallies** (kills, rooms, contracts), not counted from the moment a step opens, so a hero who has done the work already catches up in one go.
- **Shortcuts for the whole company**, a shared party stash and a party chat are not built.

### What was not exercised

- The Postgres store suites (this slice touches no store code).
- A player who actually reaches and fells the Cold Furnace: no bot gets there in a season. The verdict, the six endings, the collapse and the chronicle are covered by unit tests of the rules and by a Monte Carlo over random seasons (every ending shows), not by a played run.
- A rescue or a death in the bot season is rare (0 to 3 deaths in 18 heroes): the rescue rates come from the two-account tests, not from the harness.
- Telnet, and the ASCII tier and colour off over ssh (the snapshot tests cover them in the tiers).
