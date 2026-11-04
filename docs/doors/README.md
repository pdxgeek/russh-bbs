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

The framework is the first M4 slice. **It shares plumbing only.** Each game has its own systems and its own engine (the tide and the marsh, the node graph and the trace, the economy and the jump routes, the dungeon and torch-turns, the timetable and schemes), and no game reuses another's combat, economy or progression code. The RPG rule above is a design bar each game meets in its own way, not a shared module. What is shared:

- **Its own crate.** Each game lives in `crates/doors/<name>` and implements the framework's `Door` trait. The menu hands the session to the door, and the door hands it back. Doors never touch the board's internals.
- **A deterministic engine.** Each game is a pure rules engine: state plus an action gives a new state plus events. It uses a seeded RNG and the board's injected clock. Every game is tested by replay: a recorded action log must reproduce the saved state exactly.
- **Sandbox.** A door has no filesystem or network access (the M4 "Done when" line). It saves only through the framework's save API, which is backed by the store on both backends, with versioned, forward-only save migrations.
- **Daily tick.** A door that has days registers one scheduler job (M3-C). The job is idempotent per day and catches up after downtime by replaying missed days, with a cap.
- **Access.** Each door has a play permission. Staff tools (reset a player, end a season, bar a player from the door) are separate permissions, rank-checked (D69) and audited.
- **Sanitizing.** Every name or text a player types goes through the shared sanitizer and the reserved-name rule. Text one player shows another is rate-limited and comes from templates where the game allows it.
- **UI.** Full ratatui (UI-5): each door renders any widget into its own buffer, and the framework cleans every cell (sanitizer, no reserved style, clipped to the door's area) as it composites it. It is built on the board's screen framework: classic, lightbar and compact layouts, themes, and the three icon tiers. Every game is playable at 80x24 in the ASCII tier. Every map or board screen has a text "look around" view for screen readers.
- **Transports.** Doors behave the same over ssh and telnet (D94).
- **Content is data.** Events, quests, items and dialogue are RON or TOML tables in the crate, validated at load by a schema test, so writers can add content without touching the engine.
- **Balance.** Each game ships an auto-play harness that simulates many seasons and reports economy and win-rate curves. Balance changes come with its numbers.
- **Credits.** Doors keep their own in-game currency. Whether season results pay board credits (M3-C) is a sysop setting, off by default.
- **Inter-BBS.** Leagues shared between boards wait for federation (M8). Each game keeps a season summary format that could be exchanged later.

### Framework status (M4-D0, D98)

**Implemented:** everything in the list above except what is named under *Planned*. It lives in `crates/doors/framework` (`russh-bbs-door`), with the persistence and staff tools in `core::doors` and migration `0023`, and the example door (a coin toss with a daily allowance, saved state, a world and a daily tick) in `crates/doors/example`. **Verified:** the shared store suite on SQLite, Postgres 16 and TLS-required Postgres 16; unit and integration tests for the framework and the example; tests over a real russh client and a real telnet client that make the same walk on both transports (`crates/server/tests/doors.rs`); the path test for `doors.run` (`crates/server/tests/access_paths.rs`); and a hand walk with the OpenSSH client and a raw telnet client, reported in the pull request. **Planned, not built:** screens for the staff tools (bar, reset, end a season exist only as audited functions), a key bound to a door's look-around view, a per-door on/off setting, per-door permission grants, and any process boundary for doors by other authors (D98 (7)).

**Writing a door.** A game is a new crate `crates/doors/<name>` that depends on `russh-bbs-door` (plus `serde` and `toml` for content) and nothing else.

1. Implement `Door`: `info()` (a short slug id that never changes, a title, the save and world versions), `open(start, rng)` and, if the game has days, `has_daily_tick()` and `daily_tick()`. Add `migrate()` the day you raise a version; saves are forward-only.
2. Implement `DoorSession`: `key()`, `render()` and `look_around()`. Keep the **engine pure** (state plus an action gives a new state and events, using only `ctx.rng()` and `ctx.now()`) and keep the screen thin, so a recorded `ReplayLog` reproduces the saved state exactly.
3. Save with `ctx.request_save(bytes)` after anything worth keeping (the board writes it before the next key). Change the shared world with `ctx.edit_world(|old| new)`; the closure may run more than once. Never hold a handle to anything else: there is none to hold.
4. Draw with `Canvas::text` and `DoorStyle`. Pass a name a player typed through `ctx.player_name`; pass text another player will see through `ctx.player_text`, or use templates.
5. Put events, items and dialogue in TOML tables embedded with `include_str!`, give each a `content::Table` with `validate()`, and test it with `content::check`.
6. Test: a source audit (`sandbox::audit_crate(env!("CARGO_MANIFEST_DIR"))`), replay logs for each branch, the migrations, and a `sim::Simulation` run with bots for the balance numbers.
7. Register the door in `server::doors::Doors::builtin()`. The board then lists it, gives it a scheduler job if it has a tick, and runs it over ssh and telnet with no further work.

The RPG rule above is the game's to meet, in its own engine; the framework has no character, combat, economy or progression code, and a game must not borrow another game's.

## Build order (proposed)

1. Door framework, with a tiny example door.
2. Saltmarch, the flagship, as six slices (see its doc).
3. One more game in M4 if time allows (Emberdeep is the smallest engine). The others follow after the MVP.
