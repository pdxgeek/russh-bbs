# Emberdeep: a fantasy delving door (design)

Status: **Proposed**. Shared platform rules are in [README.md](README.md). This is the "small daily-turn RPG" that M4 plans as a first game, and the smallest engine of the set.

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
