# Hallpass: a school-life door (design)

Status: **Proposed** design; slice 1 (the engine, M4-H1) is **implemented** on branch `m4/hallpass-engine` and described under [Implementation notes](#implementation-notes-m4-h1) at the end. Shared platform rules are in [README.md](README.md).

## Pitch

You transfer mid-year into **Greywater Academy**, an old boarding school on a fog-bound lake. It has crumbling wings, strict prefects, and too many locked doors. You are here to make **mischief**: pull pranks on the staff and the prefects, run schemes, get one over on the rival cliques, and slip away before anyone can prove it was you. One real day is one school day. Each scheme you pull off builds your legend, and each one that goes wrong builds your file in the headmistress's office. Under the mischief there's a mystery: the east wing was closed after something happened there years ago, and your schemes are how you get close to it.

The core tension: **the bigger the prank, the bigger the heat.** Every period goes to setting up, pulling off, or covering up a scheme, or to keeping your grades just high enough that nobody looks too closely.

## Daily loop

1. **Morning bell:**
   - today's timetable;
   - who's angry with you, and what rumours are going round;
   - detention owed;
   - your allowance.
2. **Spend the day's periods.** Seven periods by default: four classes, lunch, one free period and after school. Each period, you pick where to be. Skipping class has a chance to be noticed.
3. **Evening:** homework (or copying someone's), the common room, or slipping out after lights-out (risky, and where the mystery lives).
4. **Night:** grades post, rumours spread, friendships change, and the school's mood shifts.

## Systems

### You

- **Stats:**
  - grades (per subject);
  - **standing** with each clique;
  - **nerve** (courage, used for risky actions);
  - **cool** (how others see you);
  - cash.
- **Traits:** one picked at enrolment and more earned in play (smooth talker, teacher's pet, fast runner, known troublemaker).
- **Notoriety:** how much the prefects and staff watch you. High notoriety makes skipping and pranks harder, and opens some doors with the rougher crowd.

### The school

- **The map:** classrooms, the library, the gym, the dining hall, the dorms, the grounds, the boathouse, the chapel, the closed east wing and the tunnels under the old wing.
- **Places change by period:** who is where, and what you can do there.
- **Locked places:** found keys, learned routines and favours open them. Exploring after lights-out is the main way into the mystery.

### Classes

- Five subjects (maths, sciences, languages, history and art). Each class is a short **minigame**:
  - maths and sciences: logic and number puzzles;
  - languages: word and memory puzzles;
  - history: dates and reasoning;
  - art: a pattern or design choice.
- Good grades open the honour roll, teachers' favours and the library's restricted section. Bad grades bring detention and a meeting with the headmistress.
- Each minigame is a minute or two and readable at 80x24.

### Cliques and characters

- **Cliques:** six original ones.
  - The Rowing Eight: athletes.
  - The Ink Society: writers and the school paper.
  - The Bench: the back-of-the-bus rebels.
  - The Lamplighters: the prefects' circle.
  - The Workshop: tinkerers and the robotics club.
  - The Strays: the kids nobody claims.
- **Characters:** about 40 named non-player students and 12 staff. Each has a personality, a schedule, likes and dislikes, a friendship track, and at least one personal storyline.
- **Friendship tracks** open favours, secrets, help in a fight, and storylines.

### Mischief (the core)

- **Schemes** are the main thing you do. A scheme has a target, a plan, a setup and a getaway:
  - **Target:** a teacher, a prefect, a rival clique, or the school itself (the bell, the menu, the trophy cabinet, the chapel organ, the school paper).
  - **Plan:** chosen from about 60 scheme templates that combine. Examples: swap the staff-room sugar for salt; rig the dining-hall announcement; fill the headmistress's car with rowing-club balls; forge a timetable change; hold a fake fire drill; hijack the school paper's front page.
  - **Setup:** gather the items, learn the target's routine, recruit help, and pick the period.
  - **Getaway:** an alibi, a route, a friend to cover for you, and a place to lie low.
- **Pulling it off** is a short sequence (sneak, act, escape) decided by your stats, your gear, the help you recruited, and how well you scouted. Each phase can go wrong in its own way, and some failures are funny.
- **Heat and the file:** every scheme adds heat with the staff. Getting caught adds a **file entry**, which brings detention, lost privileges, letters home, and in the end the headmistress. A full file at term end means expulsion, which ends that character's term. The file is also why being careful pays.
- **Legend:** pulled-off schemes build **legend**, which is the term's main score. It opens the rougher cliques, the secret club, and the best tools: lockpicks, a master key, a stink bomb kit, and the old janitor's tunnel map.
- **Escalation:** the staff learn. A teacher you prank twice becomes watchful, and the prefects post patrols where pranks keep happening. Chained pranks that build on each other (a campaign) are worth more legend than one-offs.
- **Mischief that matters:** big schemes change the school for everyone on the board. A prank that gets the strict deputy head moved to another wing opens new routes for all players. One that gets the tuck shop shut closes it for everyone for a week. The school paper reports the term's best schemes by name.

### Conflict, kept in the game

- **Rivalries:** the game's antagonists are non-player characters: a rival clique leader, a prefect with a grudge, a bully who picks on the Strays. Standing up to them is a central thread.
- **Player conflict is structured and safe by design:**
  - Players compete for clique standing, the school election, sports results and the end-of-term awards.
  - Schemes against another player's character come from a fixed list (a swapped timetable, a glued locker, a fake note from a teacher). Each costs nerve and has a counterplay, and a player is protected after being targeted. Most mischief targets the school and its non-player characters, not other players.
  - **There is no free-text targeting of another player.** Rumours are chosen from templates, never typed, so the game can't become a way to harass a real person.
  - Any player can opt out of player-versus-player interactions entirely in settings.
- **Fights:** short scuffles with the non-player characters, decided by nerve, stats and allies, drawn as a quick turn-based exchange. They get you detention if a prefect sees.

### Clubs, events and the term

- **Clubs:** chess, drama, rowing, the paper, robotics and the secret one you have to find. Each has a progression and a term-end event.
- **School events:** the house match, the winter play, the school election (players can run), the formal dance, the exam week and the end-of-term awards.
- **Term:** one term lasts 6 weeks, a season. Grades, standing and awards are scored at the end, and the yearbook keeps the results.

### The mystery

- **Shape:** a hand-written storyline in four acts, about 40 steps, under the school's timetable. It covers the east wing, an old student's diary, and a teacher who isn't who they say.
- **How you advance it:** exploring after lights-out, friendships and clues found in places.
- **Endings:** several, depending on whom you trust.
- **Finale:** a term finale that all players on the board share. The east wing opens.

## UI

- **Timetable strip:** the day's periods across the top, and where you are now.
- **School map:** a floor plan per wing, showing which characters are where this period. ASCII tier uses letters and lines.
- **Conversations:** short scenes with two to four choices.
- **Class minigames:** each fits on one screen.
- **Yearbook:** the hall of past terms.
- **Look around:** a text description of the place, who's there and the exits.

## Content and tone

- **Rating:** the tone is mischievous and warm, a school story with a mystery, rated for a general audience. There is no sexual content and no real-world slurs. The mischief is pranks and schemes against authority and rivals, never cruelty: the game has no humiliation of the vulnerable, and the Strays are allies, not targets. It never rewards cruelty to a real player.
- **Characters:** 40 students and 12 staff, each with a personal storyline of 3 to 5 steps.
- **Daily life:** about 60 scheme templates, 150 school-day events, 25 class minigame variants (5 per subject) and 6 clubs.
- **Calendar:** 8 school events and the 4-act mystery.

## Proposed slices

1. **Engine:** timetable, places, characters' schedules, stats, schemes, heat and the file, and the day tick, with replay tests.
2. **Classes and clubs:** class minigames and clubs.
3. **Characters:** conversations, friendships and storylines.
4. **UI:** timetable, map, scenes, minigames and look-around.
5. **Social:** structured pranks, rumours from templates, the election, awards and the opt-out.
6. **Content pass:** the mystery and balance by simulation.

## Open questions

- **Periods:** seven periods a day? *Engine answer (M4-H1, D100): seven day periods plus evening and lights-out, nine slots, all set in `content/timetable.toml`.*
- **Setting:** keep the boarding-school setting, or a day school in a town?
- **Player-versus-player:** should it be on by default, given the opt-out?


## Implementation notes (M4-H1)

Slice 1 of 6 is built in `crates/doors/hallpass` (`russh-bbs-door-hallpass`), registered in `server::doors::Doors::builtin()`. Decision D100 records the choices below that were real alternatives.

### What is implemented (and tested)

| Area | What exists | Where |
|---|---|---|
| Timetable | Nine slots a day (seven periods, evening, lights-out), a timetable per weekday, five subjects with room and teacher, Saturday and Sunday differ. All data. | `content/timetable.toml`, `Game::activity/kind` |
| School map | 31 places in five wings, undirected exits, hours by slot kind, descriptions that change by period, shops, locks that open for any one of a list of conditions (an item, a learned routine, standing, or a world change), walking limited to `moves_per_slot` corridors a period. | `content/places.toml`, `Game::routes/place_open` |
| Characters | Six cliques and 19 characters (10 staff, 2 prefects, 7 students): schedules by slot kind, subject and weekday (first match wins), a world effect can move one for everyone, and each can offer allies roles (`lookout`, `distraction`, `cover`, `gadget`, `keys`) on conditions. | `content/cast.toml`, `Game::npc_place/npcs_at` |
| You | Grades by subject, standing with each clique, nerve (spent to push a phase), cool, cash, notoriety, heat, legend; four skills that grow by use; four starting traits and three earned ones, each of which grants capabilities other tables ask for. | `state::Player`, `content/traits.toml` |
| Schemes | Target (17: staff, prefects, three rival cliques, four school objects), method (9 small, 4 big), up to two twists (6), a getaway (6), setup (items, scouting 0 to 3, allies, a time chosen from where the target's schedule allows), then a three-phase run: sneak, act, escape. | `content/schemes.toml`, `scheme.rs` |
| Consequences | Heat (decays daily, worse odds, easier to trace); the file (levels at 3, 5, 8 and 10 points give detention, lost privileges, a letter home, a summons to the headmistress; a full file is a final warning and one more line, or the end of term, is expulsion; a week of good conduct strikes a point); detention that must be served; staff who learn (wariness raises difficulty and the scouting a scheme needs, and fades); the same trick twice lands softer; campaigns of chained pranks multiply legend; a bust ends the campaign. | `game.rs`, `scheme.rs` |
| Big schemes and the shared world | Four world effects: the deputy head moved to the Old Wing (rest of term; opens the staffroom, moves his patrol), the tuck shop shut for a week, a fire-drill muddle (patrols down for two days), a hijacked paper (legend and heat up for five days). Applied by a pure, idempotent world edit; the paper names the culprit; a hall of legend; a yearbook of past terms. | `world.rs`, `content/world.toml` |
| Term and day | Term of 42 days; the daily tick (idempotent, catches up, rolls the term over); a character's own days apply when they next play (decay of heat, wariness, file; allowance); five endings chosen by what you did. | `world::tick_day`, `Game::sync_day/finish_term` |
| Saves | Hand-written binary codec (the sandbox allows no serde binary format), versioned (`SAVE_VERSION = 1`, `WORLD_VERSION = 1`), every read bounds-checked, a blob newer than the code is refused by the framework. No older version exists yet, so `migrate` returns `Unsupported`. | `wire.rs`, `save.rs` |
| Screen | A minimal text screen: enrol, a main view, go, study, shop, a scheme planner (target, method, twists, getaway, helpers, time), the run with the odds and their reasons, file and stats, news and the hall of legend. Every choice is a `[key] label` line; the same lines are the look-around text. | `session.rs` |

### How the RPG rule is met

Every element changes an outcome, and the tests prove it (`tests/rpg.rs`): traits and skills decide which methods, twists and getaways exist (a talker can forge, a tinkerer can time a bomb, a runner can sprint; a teacher's pet is refused by the back row but not by the Strays); the odds of a phase are the sum of named parts (skill, scouting, helpers, witnesses from the schedules, patrols, wariness, heat, notoriety, cool, nerve) and the screen shows them; the same scheme ends differently for different builds over the same dice; choices close doors (a troublemaker's name, an old trick); consequences persist across days and the save; no content is dead (every method, twist, getaway and trait is reachable by some build, checked).

### Content is data

Eight TOML tables under `crates/doors/hallpass/content/`, parsed with `deny_unknown_fields`, with a condition language that is parsed at load. `tests/content.rs` loads them, breaks each on purpose (unknown field, bad condition, unresolved reference, duplicate id, unplain text, a lock without a key, a missing effect, a role nobody offers, a Stray as a target) and expects the named problem. **Tone rules are checks, not hopes:** no target is a Stray, a target is always a staff member or prefect who is marked prankable, or a rival clique, or a school object, and a target is always an id from the table, never typed; no free-text targeting of a player exists in the engine.

### Seams for the next slices

- **H2 classes, clubs, characters.** `Game::attend` is the single place a lesson is taken: a minigame replaces its flat grade gain there. Clubs add a `club` field to `Player` and a slot-kind activity; conversations hook `loiter` (it already picks a clique member who is present and moves standing) and `Recruit` (the ally asks and the answer is where a scene belongs). Friendship tracks are `standing` plus `ally_used`; storylines are `knowledge` flags that conditions can already test (`know:...`).
- **H3 UI.** `session.rs` is the only file that knows about screens. It builds `(body, items)` and draws them; layouts, the map and the timetable strip can replace the drawing without touching the rules. `Game::describe_place`, `routes`, `npcs_at` and `candidate_times` give a map and a scouting view what they need.
- **H4 social, mystery, balance.** The east wing, the tunnels and the organ loft exist as locked places with no key yet; `know:` flags and `item:` keys are the unlock vocabulary. Structured player schemes and template rumours add `Action` variants and `WorldDelta` variants; the PvP opt-out is a `Player` flag the legal-action list checks. `tests/common/bot.rs` and `tests/sim.rs` are the start of the balance harness (the bots play through the public actions only).

### Not done, deliberately

Class minigames, clubs, conversations and storylines, the school events, the mystery, the election, structured player-against-player schemes, template rumours and awards (H2 to H4). The content budget is a first small cast and 13 methods, not the design's 60 templates, 40 students and 12 staff. Hallpass reads the shared world once when a visit opens: a change another player makes while you are in the door shows at your next visit (the framework has no world-read for a session; noted in `docs/coordination.md`).

### Verified

See the pull request for the commands and their results. In short: unit and integration tests for the engine, schemes, the RPG branches, the shared world through the framework's real world edit and daily tick, replay logs with three pinned branches of one scheme, the 80x24 screen on both transports, the framework's source audit, and a 6-week headless simulation; plus a walk with the OpenSSH client (enrol, the planner, scouting twice, buying, a timed scheme in the staffroom, a day rollover with the server clock moved five days, the missed detention that followed). **Not exercised by hand:** a big scheme over ssh (the legend gate means a long game; the world change is tested through the framework instead), telnet by hand (the same screen is compared on both transports in a test), Postgres.

### Balance (first numbers, from the simulation)

Forty terms of each of four bots (the table below is from the final tuning) on one shared school (`cargo test -p russh-bbs-door-hallpass --test sim -- --nocapture`). The bots play only through the public actions; "reckless" skips class, pushes nerve and starts schemes at any heat, "cautious" only at low heat and scouts to the maximum.

| Bot | Legend | File at term end | Pulled off | Caught | Traced | Expelled |
|---|---|---|---|---|---|---|
| cautious | 48.0 | 0.3 | 8.3 | 1.9 | 0.5 | 0% |
| balanced | 52.4 | 5.8 | 6.8 | 3.3 | 0.5 | 2% |
| reckless | 76.0 | 11.2 | 6.2 | 3.8 | 2.8 | 57% |
| variety (random feasible schemes, for coverage) | 24.0 | 1.5 | 3.4 | 1.1 | 0.1 | 0% |

Endings over the 160 terms: model pupil 60, scraped through 41, infamous 30, expelled 24, phantom 5. Eleven of the thirteen methods were played (the paper headline and the paper hijack sit behind a locked office the bots do not try to open). Cautious is nearly as good as balanced here because the bots scout to the maximum; the sim is a baseline, not a target.

The shape is what the design asks for: boldness pays in legend and costs in the file, and careful play keeps a clean sheet. Real balance is H4's job; these numbers are a baseline, not a target.
