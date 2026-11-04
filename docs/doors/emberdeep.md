# Emberdeep: a fantasy delving door (design)

Status: **Engine implemented (M4-E1, D99)**; town, crafting, the full UI, social play and the content pass are still **Proposed**. The design below is the target; [Implementation notes (M4-E1)](#implementation-notes-m4-e1) says what exists, how it behaves and what was not built. Shared platform rules are in [README.md](README.md). This is the "small daily-turn RPG" that M4 plans as a first game, and the smallest engine of the set.

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

**Time and randomness.** All time is the board's clock as a calendar day number. All randomness is the **hero's own `DoorRng`, whose state is part of the save** (D99): reopening a save or disconnecting mid-fight is not a re-roll. The session seed is used once, to start a new hero's generator.

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

- **No gear drops or crafting yet**; a hero fights with the calling's starting kit.
- **A lair is not a coin-flip ambush:** entering a pack's room shows the pack and the ways past it, which is how builds change a room.
- **The torch is the turn count**: there is no separate torch item yet.
- **Seasons end by staff action** (`end_season`), not at 6 weeks automatically, and the Cold Furnace's fall collapses the dungeon at the next tick rather than ending the season.
- **Duels, parties and the tavern board contracts** are not built.
