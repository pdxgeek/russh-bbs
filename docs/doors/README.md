# Doors

Status: **Proposed** designs for M4 and later. Nothing here is built. The M4 slices still need the owner's approval.

Every game here is original. Each uses mechanics its genre shares, with its own world, systems, characters and text, and none is modelled on one existing game.

| Game | Genre | One line | Doc |
|---|---|---|---|
| Saltmarch | Clan strategy, daily turns | Clans hold high ground on a tidal salt marsh; the tide is the clock and the map | [saltmarch.md](saltmarch.md) |
| Nullroute | Hacking and heists | Break into procedurally built corporate networks with a rig you build, before the trace finds you | [nullroute.md](nullroute.md) |
| Long Haul | Space trade and exploration | Run a tramp freighter through a frontier where every jump costs fuel and time | [long-haul.md](long-haul.md) |
| Emberdeep | Fantasy delving RPG | One hero, one town, a dungeon that grows deeper every season | [emberdeep.md](emberdeep.md) |
| Hallpass | School mischief | Pull pranks and schemes at a strange old boarding school, and stay one step ahead of the headmistress's file | [hallpass.md](hallpass.md) |

## Design rule: RPG elements with real outcomes (owner, 2026-11-04)

Every door has an RPG layer, and every part of it must change what happens, not just a number on a profile. Concretely:

- **Characters grow by doing.** Skills improve with use, and a character's build (calling, trade, rig, crew mix, traits) decides which options exist in events, fights and dialogue, not only how well they succeed. Two players with different builds see different choices on the same scene.
- **Choices close doors.** Quest branches, faction standing and betrayals lock some content and open other content for that character for the rest of the season. There is no "best" path that sees everything.
- **Consequences persist.** Injuries scar into traits, crew and companions can die or leave, reputation outlasts the deed, and a player's file, heat or debt follows them.
- **The world remembers.** Some outcomes change the shared world for every player on the board for the rest of the season: Saltmarch's causeways and dredged channels, Nullroute's breached networks, Long Haul's outposts and surveyed systems, Emberdeep's repaired lifts and town projects, Hallpass's moved staff and closed tuck shop. Each game lists its world-changing outcomes.
- **Endings differ.** Each game's season or term has more than one ending, depending on what the players collectively did and on each character's choices.
- **Tested.** The replay tests cover branch outcomes, and the balance harness reports how often each ending and each branch occurs, so no branch is dead content.

## What every door shares (the door framework, M4)

The framework is the first M4 slice; every game builds on it.

- **Its own crate.** Each game lives in `crates/doors/<name>` and implements the framework's `Door` trait. The menu hands the session to the door, and the door hands it back. Doors never touch the board's internals.
- **A deterministic engine.** Each game is a pure rules engine: state plus an action gives a new state plus events. It uses a seeded RNG and the board's injected clock. Every game is tested by replay: a recorded action log must reproduce the saved state exactly.
- **Sandbox.** A door has no filesystem or network access (the M4 "Done when" line). It saves only through the framework's save API, which is backed by the store on both backends, with versioned, forward-only save migrations.
- **Daily tick.** A door that has days registers one scheduler job (M3-C). The job is idempotent per day and catches up after downtime by replaying missed days, with a cap.
- **Access.** Each door has a play permission. Staff tools (reset a player, end a season, bar a player from the door) are separate permissions, rank-checked (D69) and audited.
- **Sanitizing.** Every name or text a player types goes through the shared sanitizer and the reserved-name rule. Text one player shows another is rate-limited and comes from templates where the game allows it.
- **UI.** Ratatui on the board's screen framework: classic, lightbar and compact layouts, themes, and the three icon tiers. Every game is playable at 80x24 in the ASCII tier. Every map or board screen has a text "look around" view for screen readers.
- **Transports.** Doors behave the same over ssh and telnet (D94).
- **Content is data.** Events, quests, items and dialogue are RON or TOML tables in the crate, validated at load by a schema test, so writers can add content without touching the engine.
- **Balance.** Each game ships an auto-play harness that simulates many seasons and reports economy and win-rate curves. Balance changes come with its numbers.
- **Credits.** Doors keep their own in-game currency. Whether season results pay board credits (M3-C) is a sysop setting, off by default.
- **Inter-BBS.** Leagues shared between boards wait for federation (M8). Each game keeps a season summary format that could be exchanged later.

## Build order (proposed)

1. Door framework, with a tiny example door.
2. Saltmarch, the flagship, as six slices (see its doc).
3. One more game in M4 if time allows (Emberdeep is the smallest engine). The others follow after the MVP.
