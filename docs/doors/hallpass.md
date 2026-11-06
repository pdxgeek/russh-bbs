# Hallpass: a school-life door (design)

Status: **Proposed** design; slice 1 (the engine, M4-H1), slice 2 (classes, clubs and characters, M4-H2) and slice 3 (the UI, M4-H3) are merged, and slice 4 (social, mystery and balance, M4-H4, in review on branch `m4/hallpass-social`) is the last: all four are **implemented** and described under [Implementation notes (M4-H1)](#implementation-notes-m4-h1), [(M4-H2)](#implementation-notes-m4-h2), [(M4-H3)](#implementation-notes-m4-h3) and [(M4-H4)](#implementation-notes-m4-h4) at the end. Shared platform rules are in [README.md](README.md).

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
- **Player conflict is structured and safe by design** (built in M4-H4):
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

Built in M4-H4 (see [Implementation notes (M4-H4)](#implementation-notes-m4-h4)): twelve clues in four acts, three ways to the key, six endings of a pupil's story, and a finale on day 38 where the school's verdicts decide one of five endings that change the wing for everyone. **That is 12 clues, not the 40 steps the design names.**

## UI

Built in M4-H3: see [Implementation notes (M4-H3)](#implementation-notes-m4-h3). The list below is the original design.

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

- **Periods:** seven periods a day? *Engine answer (M4-H1, D107): seven day periods plus evening and lights-out, nine slots, all set in `content/timetable.toml`.*
- **Setting:** keep the boarding-school setting, or a day school in a town?
- **Player-versus-player:** should it be on by default, given the opt-out?


## Implementation notes (M4-H1)

Slice 1 of 6 is built in `crates/doors/hallpass` (`russh-bbs-door-hallpass`), registered in `server::doors::Doors::builtin()`. Decision D107 records the choices below that were real alternatives.

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

## Implementation notes (M4-H2)

Slice 2 builds on the H1 engine in the same crate. Decision D114 records the choices that were real alternatives. The ownership split of the code: `lesson.rs` (class minigames), `club.rs` (clubs), `talk.rs` (conversations, friendships, storylines), `effect.rs` (the effect language), `check.rs` (cross-table checks), and four new tables, `lessons.toml`, `clubs.toml`, `talks.toml` and `arcs.toml`.

### What is implemented (and tested)

| Area | What exists | Where |
|---|---|---|
| Class minigames | Six subjects (games is new), each with its own short deterministic puzzle: number sequences (maths), balance the flasks (sciences), Greywater Latin vocabulary (languages), earliest or latest event (history), finish the border (art), who wins the relay (games). Three puzzles to a lesson, harder as the subject grade rises, with the answer to each a pure function of the lesson's stored seed, so a lesson survives leaving the door and replays exactly. The score sets the grade change and the skill the subject trains, and the teacher notices perfect and empty work. | `src/lesson.rs`, `content/lessons.toml` |
| Grades matter | A good history mark opens the restricted stacks; a teacher lends a key (Mr. Alder) or covers for you (Mr. Pell) only for a pupil with the mark and the regard; a missed class is noticed more when the mark is poor and the teacher is no friend; the chance of being traced shifts by the average grade; Friday's report files a poor mark and rewards the honour roll; a mark slips a point a day when it is not kept up. | `scheme.rs`, `game.rs`, `rules.toml` |
| Clubs | Six clubs (chess, drama, rowing, the paper, robotics, and the secret Candle Society found through a flag and the cellars). Joining needs the sign-up place, the conditions, a free place (two clubs at most) and no rival club; leaving is remembered for the term. Meetings on set days earn merit (more with the club's skill), ranks come with merit, the advisor and the members present warm to a regular, and the term ends with each club's event. Rank is a condition anywhere (`rank:CLUB>=N`): it opens the green room, the paper office, the cellars and the candle room; sells greasepaint and servos to members only; and unlocks two twists and a getaway. | `src/club.rs`, `content/clubs.toml`, places, items, schemes |
| Conversations | 34 conversations with 20 characters (some have a second beat): data-driven trees with conditions on stats, skills, traits, standing, the file, bonds, club rank, world state and storyline step. A choice that fails its conditions is shown as shut with what it wants. Choices apply effects (standing, bond, grades, file, items, flags, steps, endings, world edits, betrayal). A conversation costs the period when it ends; leaving at the first line costs nothing; nothing else can be done in the middle; a scene survives leaving the door. | `src/talk.rs`, `src/effect.rs`, `content/talks.toml` |
| Friendships and rivalries | A bond per character, -100 to 100, grown by doing: talking, loitering together, club meetings, recruiting and then a clean job, tutoring, favours. At 12 a character is a friend, at -12 a rival (a rival who is at the deed adds to the odds against you, a friend in your crew adds), at -30 closed. Roles an ally offers can ask for a bond (Rafe will distract for a friend, Cordelia will cover for a friend who is in with the Lamplighters). A betrayal (the headmistress asks who helped, and you name them) closes the helper, sets the bond at -40, flags `betrayed_...`, and costs standing with their clique; they will talk only about making it up, and amends reopen them on a cooler footing. The Strays are never given up in a script (checked in the content and at runtime). | `talk.rs`, `scheme.rs`, content |
| Storylines | Two arcs a term, each with five steps and five endings: **The prefects' ledger** (begins on day 4) and **The light in the East Wing** (day 8). A step is open only while the story is there and some days have passed; a choice that moves the story shuts the other branch's conversations. The ledger can end with the prefects stood down for the term, a **shared-world change** through the framework's world edit (patrols thinner and Cordelia's rival Bramley moved for every player, the paper names the culprit); another ending is a hijacked paper for five days. The journal shows where each story stands. | `content/arcs.toml`, `talks.toml`, `world.toml` |
| Saves | `SAVE_VERSION = 2`: the character gains `SchoolLife` (clubs, bonds, storylines, a lesson or a conversation under way, memory of the latest helpers). `save::migrate` brings a version 1 save forward by appending the empty value and refuses bytes that were never a version 1 character. The world is unchanged (`WORLD_VERSION = 1`). | `state.rs`, `save.rs` |
| Screen | The minimal text screen gains the lesson screen (the puzzle and its answers), the conversation screen (the node and its choices, shut ones listed with what they want), Talk, Clubs and Journal, and the main view marks friends and rivals. Every choice is still a `[key] label` line; no rule is in the screen. | `session.rs` |

### How the RPG rule is met

Tests in `tests/life.rs` and `tests/story.rs` show it directly. The same lesson shows a practised pupil fewer wrong answers than a novice (the skill the subject trains strikes them out). The same scene, `marsh_clipboard`, offers a smooth talker an apology, a teacher's pet his marks and a plain runner only the floor, and each ends differently. The ledger's shakedown is shut to a teacher's pet and open to a troublemaker. A known troublemaker is not offered the head prefect's rounds. Clubs open different places, items, twists and getaways for their members and shut their rivals. Friends, rivals and betrayed helpers change who will help and what the odds are. Storyline choices close the other branch. Every conversation, and every choice in it, is checked to be open to some build (`satisfy` in the test helpers builds that build), and each ending of each story has a recorded way to it.

### Content is data

Four new tables (and additions to rules, timetable, places, cast, items, schemes and world), parsed with `deny_unknown_fields`, with conditions (new: `bond:`, `rank:`, `day`, `arc:ID@STEP`, `ended:ID@ENDING`) and effects parsed when they load. `check.rs` makes the following load errors, and `tests/content.rs` breaks each on purpose: an unknown id in any condition or effect; a conversation node nobody can reach, a choice that leads nowhere, a node whose every choice asks for something, a repeating conversation with no cooldown; a storyline step that cannot be entered or has nothing to do, an ending nobody can reach, a story with fewer than two endings; a flag a condition tests that nothing grants; clubs that are not mutual rivals, an advisor who is not staff, ranks that do not rise; a subject with no lesson; a script that costs the Strays standing or betrays a Stray. `east_wing_key` is the one item a lock asks for that no shop sells; the mystery (H4) hands it out and the check names it.

### Seams for the next slices

- **H3 UI.** The new screens are `lesson_screen`, `scene_screen`, `talk_view`, `clubs_view` and `journal_view` in `session.rs`; they take everything from `Game::lesson_view` (the puzzle, struck-out answers), `Game::scene_view` (node text, choices with `open` and `wants`), `Game::join_block` (why a club is closed), `Game::journal`, `Game::regard` and `Game::talks_here`. The look-around text is the same lines.
- **H4 social, mystery, balance.** The East Wing story ends before the wing opens: `know:diary_page` and `know:soames_trust` are flags the mystery can read, and `east_wing_key` still has no giver. More storylines are rows in `arcs.toml` and `talks.toml` and need no code; a new effect is a variant in `effect.rs`. The sim bots (`tests/common/bot.rs`) now answer lessons, join clubs and talk; the grade, club and storyline tables are in `tests/sim.rs`. Grades saturate at 100 in H1's study numbers unless something pulls them down; H2 adds a daily slip (see below), but the numbers are a baseline.

### Not done, deliberately

Per-character personal storylines for all 40 students and 12 staff (two shared arcs and 34 conversations are the system and a sample), the 25 minigame variants (six kinds, each with levels), the school events (house match, play, election, dance, exam week, awards beyond each club's term-end event), structured player-versus-player schemes, template rumours, the opt-out and the mystery (H4). A full-screen ratatui showcase of any of this is H3.

### Verified

See the pull request for the commands and their results. In short: unit tests of the puzzle generators across every kind and level, the condition and effect parsers and the content checks; integration tests of lessons, clubs, friendships, conversations and the RPG rule (`tests/life.rs`), replay logs for every branch of both storylines and the shared-world change through the framework (`tests/story.rs`), the version 1 to 2 save migration (`tests/migrate.rs`, with the old layout pinned as a file), breaking each new table on purpose (`tests/content.rs`), the framework's source audit, and the 6-week simulation. Walked with the OpenSSH client at 80x24 (in tmux, against a real server): enrol, a History lesson (three puzzles, one wrong, the result), tutoring with Mr. Pell (a conversation with a shut choice that says what it wants), a server restart in the middle of the day (the character comes back to the same period), the Clubs screen (why each is closed) and joining Robotics, then the server clock moved six days (`faketime`): the morning bell, the allowance, the ledger story beginning, the Journal, and the first ledger conversation with Tilly moving the story on. A bug found this way and fixed: the main menu had grown past 24 rows and cut the room description, so the labels were shortened to keep two columns (now a test). **Not exercised by hand:** a club meeting and a rank-up, a betrayal, the storyline ending that changes the school (tested through the framework instead), telnet, Postgres.

### Balance (first numbers, from the simulation)

Forty terms of each of five bots on one shared school (`cargo test -p russh-bbs-door-hallpass --test sim -- --nocapture`). The bots answer lessons right with a set chance (scholar 90, cautious 75, balanced 70, variety 65, reckless 55 percent) and spend a share of free periods on clubs and talk (scholar 70, variety 40, cautious and balanced 30, reckless 15). The "scholar" pupil makes no mischief at all.

| Bot | Average grade | Lessons taken | Perfect lessons | Clubs | Friends | Honour roll |
|---|---|---|---|---|---|---|
| scholar | 99 | 144 | 121 | 2.0 | 10.9 | 100% |
| cautious | 83 | 129 | 97 | 1.0 | 8.6 | 85% |
| balanced | 80 | 130 | 91 | 2.0 | 8.4 | 65% |
| variety | 89 | 142 | 86 | 2.0 | 10.0 | 92% |
| reckless | 75 | 104 | 57 | 0.6 | 4.6 | 57% |

Club ranks reached by the 200 players of 40 terms (players at ranks 1 / 2 / 3 / 4): chess 26 / 46 / 20 / 45, robotics 3 / 17 / 41 / 49, rowing 8 / 16 / 7 / 4, drama 2 / 14 / 6 / (none at 4). Nobody joined the School Paper or the Candle Society: their gates (standing 6 with the Ink Society, and the knock that Sol teaches) need more social play than the bots do, and a test shows each is open to some build. Storyline endings at term end (players): the ledger was left open 113 times, owed 31, shrugged 24, returned 16, purged 16 (the shared-world ending), headline 0 (it needs a School Paper card, which no bot had); the East Wing light was left open 120 times, shared 31, ignored 15, quiet 14, secret 11, informer 9. Treat these as a baseline: the chess ladder is the easiest to climb because Tobias Bramley plays at the board every four days, and the scholar sits on the grade ceiling. Real balance is H4's job.

## Implementation notes (M4-H3)

Slice 3 gives the engine its screens. Decision D119 (number assigned at merge) records the choices that were real alternatives. The code is `crates/doors/hallpass/src/session/ui/` (one module per family of screens); `session.rs` keeps the menu, the keys and the text screen, and the new modules only read the engine. The walk over `ssh` and the pictures are in [qa/m4-h3-walk.md](../qa/m4-h3-walk.md).

### What is implemented (and tested)

| Screen | What it shows | Where |
|---|---|---|
| The timetable strip (every screen) | The day's nine periods across the top, the one now lit with brackets, the ones gone dimmed, a rule under each, and (from 80x24) a sky from dawn gold to night ink with the bell's position. | `ui/day.rs` `strip` |
| Day | Where you are (wing, description, patrol mark), who is here with a mark for friend or rival, what the period is and the next one, who is in the corridors next door, heat, file, nerve pips, legend and cash, a stamp for detention owed or a summons, the schemes in hand with `READY` or `setup`, and the orders as hotkey chips. Roomy and Grand add the whole day (`Today`) and a `Nearby` panel. | `ui/day.rs` |
| School map | The 34 places as a plan of corridors with a state for each (you, reachable with its corridor count, open, shut at this hour, locked, locked but yours), the staff mark where you can see staff, a cursor moved with the arrows, a detail for the place under it (hours, doors, who is there, patrol level) and, from 100 columns, who is where as far as you know. Enter walks (`Action::Go`). | `ui/map.rs` |
| Scheme planner | The lists (schemes, target with how wary they are, method with its difficulty, legend and heat as bars, twists, getaway, helpers with their bond, the time with who will be about) each with a panel that says what the highlighted choice means; and the dossier: target, method, where (and whether the way in is open), twists, getaway, items (have or need), allies, time, scouting; the odds of sneak, act and escape as bars, the chance of being traced, the reasons behind the phase chosen with Left and Right, and a `READY` or `NOT READY` verdict that says what is missing. | `ui/plan.rs` |
| Scheme run | Three stages along the top (clear, now, ahead, each with its odds), the nerve gauge and the slips, each way to spend nerve with the odds it buys, and a log of what happened. | `ui/run.rs` |
| The file | The headmistress's file as a document: a typed line per entry with its weight, how thick the folder is, the stamps for detention and a summons, the consequences the file has reached (`3, 5, 8, 10` and expulsion at 12), and (as tabs, or side by side from 100 columns) the record, marks and skills, and standing with each clique and who is watching you. | `ui/file.rs` |
| Lessons | A blackboard in ink with the subject's own emblem (sequences, flasks, Latin tiles, a timeline, a border pattern, a relay track), the puzzle in chalk, puzzles taken as pips, and the answers with the ones a practised pupil has ruled out struck. | `ui/lesson.rs` |
| Conversations | A speaker's portrait (a face made from the character's id, half blocks in colour and an ASCII bust otherwise), what they say, the bond meter with the friend and rival marks, and the answers, shut ones saying what they need. | `ui/people.rs` |
| Clubs, friendships, journal | Club cards with rank pips, rank name, the week with meeting days lit and today marked, merit towards the next rank, why a club is shut; friends and rivals on bond meters; storylines as step pips with where each stands. | `ui/people.rs` |
| Paper, hall of legend, yearbook | The paper with its headlines and what is in force with the days left; the ranked hall with bars; the yearbook of past terms and your own. The enrolment card with the school's name in block letters and the end-of-term report. | `ui/paper.rs` |
| Look-around | `look_around` is the engine's `[key] label` text, then `Looking around:` and sentences for every screen: the map names where you are, what you can walk to and in how many corridors, what is locked or shut, the patrols you know of, who is about and what the place under the cursor is; the planner gives each phase's odds and reasons; lessons and conversations read the puzzle or the speech and every answer, shut ones with what they need. | `ui/describe.rs` |

**Keys.** Letters and digits as before; Up, Down and Enter move the lightbar and press its item; on the map the arrows move the cursor and Enter walks; on the planner and the file Left and Right change the phase shown or the tab; on the clubs screen Up and Down choose a club and Enter joins or leaves it (or says why it is shut); Esc is back, and leaves the door only from the day screen. Mouse comes with UI-9.

**Sizes and tiers.** Tiny (the 74x20 a door gets inside an 80x24 board), Compact (80x24), Roomy (100x30) and Grand (132x43); below 60x16 the plain text screen. The plan of the school gets wider and taller cells as the window grows. The ASCII tier is ASCII only; the Nerd Font tier is the Unicode picture (the board neutralizes private-use glyphs in a door, D104).

**Colour.** Ink blue, chalk white, detention red, lamplight, moss, brass and slate over the theme's roles, each chosen to survive the board's sixteen colours; with colour off every cue is a word or a glyph (the stamps, `READY`, `NOT READY`, the glyph and word for locked and shut). No cell is bold plus reverse.

### How the RPG rule is met

The screens add no rule, so the RPG rule is unchanged from H1 and H2; what the screens do is show it. The planner puts the engine's named parts of each odd on screen (skill, scouting, helpers, witnesses, patrols, wariness, heat), so a build that changes the odds changes the numbers a player sees; a closed club says what it wants; a shut conversation choice says what it needs; the map shows a place as locked until you hold one of the ways in, and what opens it only once you have scouted it.

### Verified

Tests in `tests/ui.rs` and `session/ui/map.rs`: every screen (29 states: the enrolment card, the day, the map with the cursor moved, study, a shop, the scheme lists and planner on both phases, twists, getaway, the time, a lesson in three subjects, a talk list and a conversation with a shut choice, a run at two stages, the file on two tabs, clubs, the journal, the paper, the hall, the yearbook and the end of term) rendered at 80x24, 100x30 and 132x43 in the ASCII, Unicode and Nerd Font tiers with colour on and off, and again at 74x20; snapshots of every screen at every size in the ASCII and Unicode tiers (`HALLPASS_BLESS=1` rewrites them); the Nerd Font tier equals the Unicode tier and has no private-use glyph; the ASCII tier is ASCII; nothing is coloured with colour off; at sixteen colours no rgb value survives and the accents are all there; no bold plus reverse; the plain text screen under 60x16; a look-around for every screen that differs from every other and is the same in every tier; the map's look-around names the walkable places, the locked and the shut; the planner's and the file's numbers in words; the lightbar, the map cursor and Enter; every place has one cell on the plan, no two share one, no more than the two named long doors, and the cursor reaches every place; one whole day through the keys (the enrolment card, a lesson answered with Enter, the file, a scheme planned to the planner, clubs); the framework's source audit (`tests/sandbox.rs`) with no new dependency. The older text-screen tests (`tests/play.rs`, `tests/screens.rs`) now read the words of a screen from the look-around, which still holds every line they asserted on. The walk over a real `ssh` client, with a picture of each stop, is [qa/m4-h3-walk.md](../qa/m4-h3-walk.md).

### Not done, deliberately

A scheme run reached by hand over `ssh` (it needs a scheme that is ready, the items and a moment the target allows; the run, the conversation and the end-of-term screens are drawn and tested through the harness and pictured as designs), mouse (UI-9), animation and effects (UI-8), the Nerd Font glyphs (the board neutralizes them, D104), per-member colour depth (the board composites doors at sixteen colours today), the school events, the social layer and the mystery (H4).


## Implementation notes (M4-H4)

Slice 4 is the last Hallpass slice: the social layer, the east wing's mystery, a content pass and the balance pass. Decision D-TBD (number assigned at merge) records the choices that were real alternatives. New code: `src/commons.rs` (what the social layer keeps: a character's `Social` and the school's `Commons`), `src/social.rs` (the rules: schemes on pupils, rumours, the election, the awards, the search), `src/social_spec.rs` (the two new tables and their checks), `src/session/social_views.rs` (the screens); new tables `content/social.toml` and `content/mystery.toml`, and additions to `arcs.toml`, `talks.toml`, `world.toml`, `items.toml`, `places.toml`, `traits.toml`, `clubs.toml`.

### What is implemented (and tested)

| Area | What exists | Where |
|---|---|---|
| Schemes on another pupil | Seven templates (four mischief: a swapped timetable, a glued locker, a fake note from a teacher, squeaky laces; three kind: a good word, a pep talk, spare change), aimed at a name from the roster (you cannot type one). Each costs nerve (and some cash), needs a build or kit to be offered (the swap needs a forgery kit, the glue a tinkerer's hands, the note some bluff), and a mischief scheme has a **counterplay** that depends on the target (a spare timetable card or a trusted pupil's good name stops the swap, a padlock or a tinkerer the glue, bluff 3 or charm the note, a fast exit or sneak 3 the laces). What lands is only a small change of cool, nerve or cash; the load check refuses anything that reaches the file, the world, a friendship or a standing. A target is left alone for two days after one reaches them. | `social.toml`, `social.rs` (`plot_block`, `send_plot`, `deliver_plots`) |
| How it travels | The sender posts a record to the shared world; the target's own session delivers it when their visit opens (an opt-out, the protection window and the counterplay each stop it) and writes the outcome back; the sender collects it at their next visit (a scheme that landed pays legend, one seen through costs a little cool). The **stored** world re-checks both opt-outs and every cap when it accepts the record, so a session working from an old view cannot get past them. | `world.rs` (`post_plot`), `social.rs` |
| Caps against griefing | Two schemes a day per sender, a rest of three days between the same two pupils, three waiting for any one target, sixteen in the post; a scheme costs a period as well as nerve; plots expire after six days. | `social.toml` rules, `commons.rs` caps |
| The opt-out | One flag on the character, in `Social`, shown on the roster. An opted-out pupil can neither send nor receive anything aimed at a player; it follows them from term to term. Rumours, the election and the awards are not aimed at a person and carry on. | `social.rs` (`set_opt_out`), `world.rs` |
| Rumours | Eight templates about a character, a clique or yourself (praise, gossip about staff and prefects and rival cliques only, tall tales about yourself); never about a player, never gossip about a Stray or a pupil (checked at load). A rumour moves your standing, a friendship, your cool and legend, sometimes the heat or the patrols where the character is found, shows in everyone's paper for a week, and moves a candidate's backing in the election. One a day, and only with an audience present. | `social.toml`, `social.rs` (`spread`) |
| The election | Head of School. Three characters run (Bramley, Wren, Hollis); a pupil may stand from day 11 (a record that is not poor, some cool), canvass twice a day (each door knocked on wins less as the campaign saturates, at 44 backing at most), and vote once from day 20; the votes are counted by the daily tick on day 27. The characters campaign too, the school's mood favours a different one each term, rumours and what the school has done (the prefects stood down, the paper hijacked) swing them, and a pupil's vote counts three times a point of backing. The winner (a character or a pupil) changes the school for everyone through the world edit; a winning pupil takes legend and the trait Head of School. | `social.rs` (`ballot_of`, `tally`, `count_election`), `world.rs` |
| The awards | Seven, scored from the roster at term end (legend, grades, schemes, clues, club merit, friends, a clean getaway) with a minimum to win anything; the paper prints them, the winners are told and paid at their next visit (also between terms), and the list outlives the term. | `social.toml`, `social.rs` (`give_awards`) |
| The mystery | Twelve clues in four acts, each found by searching a place (mostly after lights-out or in the free periods), gated by day, by earlier clues, by the storyline's step and by build (items, skills, bonds); a search can be noticed and costs a line on the file. **Three ways to the East Wing's key:** lift it from the caretaker's shed (sneak), have Mr. Fenn give it to a friend who found the locket, or have the chaplain give it to a pupil he likes (the choir; the East Light story's `shared` ending still gives him a reason to help, and a shortcut past the organ's key). `diary_page` from the H2 story is the evidence the paper and the bonfire ask for. | `mystery.toml`, `arcs.toml` (`the_wing`), `talks.toml`, `social.rs` (`search`) |
| The endings | Six endings of a pupil's story, from conversations only some builds are offered (Fenn needs a friendship, the chaplain one, the headmistress a clean file and no troublemaker's name, the paper standing with the Ink Society, the bonfire the diary; walking away needs only to have read it). Five cast a **verdict**; on day 38 the daily tick counts them and one of five comes about for **everyone**: the wing opened by the caretaker, a vigil, the diary in the paper (the wing and tunnels open, for different prices), the wing sealed for good, or a bonfire (both closed even to a key). A tie goes by a fixed order; nobody settling it is a sixth outcome (the wing stays as it was). | `social.rs` (`finale`), `world.toml`, `places.toml` |
| Saves | `SAVE_VERSION = 3` appends `Social` to the character; `WORLD_VERSION = 2` appends `Commons` to the school. `save::migrate` brings a version 1 or 2 character and a version 1 school forward, each by appending the empty value after checking the bytes decode; all three are pinned against hand-kept old bytes (`tests/fixtures/*.hex`). | `commons.rs`, `save.rs` |
| Screens | An `o` Social hub from the day screen (classmates and the schemes you can play on each, rumours, the election, the opt-out, awards and what was done to you lately) and an `e` search order that appears only where a clue can be found. They are the same `[key] label` lists as every screen (so the bots, the replay logs and the look-around all work) and draw in the scheme planner's list style at every size and tier. | `session/social_views.rs`, `session/ui/*` |

### How the RPG rule is met

Every part changes what happens (`tests/social.rs`, `tests/mystery.rs`). **Options depend on build:** a forger is offered the swapped timetable, a tinkerer the glue and a plain runner only the laces and the kind schemes; which of three routes finds the key, and which of five endings is on offer, depend on a pupil's skills, bonds, standing and file. **Branches close doors:** casting a verdict ends the pupil's story and no other ending is offered after; a choice in the East Light story (the chaplain's trust, the diary) opens or shuts a route into this one. **Consequences persist:** an opt-out follows the pupil across terms; awards, the roster and the verdicts are in the shared world; a scheme that landed protects its target and pays its sender. **The world changes:** the election's winner and the east wing's ending are applied to everyone through the one idempotent world edit, at the daily tick. **Several endings:** six for a pupil, six for the school (five verdicts and "nobody settled it"), five election winners (three characters, a pupil, and who the school's mood favours).

### Content is data

Two new tables (`social.toml`, `mystery.toml`), parsed with `deny_unknown_fields` and checked when they load (`social_spec.rs`): a scheme's effect on a player may only be a small change of cool, nerve or cash with the right sign; a mischief scheme needs a counterplay; a kind scheme cannot cost its target; a rumour template needs `{about}`, may be gossip only about staff, prefects and rival cliques, and is never about a Stray or a pupil; a tall tale is about the speaker; no Stray runs for election or is gossiped about; an election's days rise; every clue has a real place that is open when it can be found and follows an earlier clue; every act has a clue; every verdict is cast by some ending and brings about a real world effect; the finale falls inside the term. `tests/content.rs` breaks each on purpose and expects the named problem. **Tone rules are checks, not hopes** (they are tests): no target is a Stray and none can be typed (the targets are ids), and a scheme on a pupil never touches their file.

### Seams for what comes next

- **FW-1 (a read of the current world in `Ctx`).** Delivery happens when a visit opens, because a session sees the world as it was then. With a current read, a scheme could be delivered mid-visit (the target's screen would change as it landed) and the roster would be fresh. The workaround is small: `Session::key` flushes `Game::take_world_changes()` on every key. Until FW-1's world-aware replay log lands, the mystery's talks (open late in the term) are replayed through `Running` against a store seeded with a world (`tests/mystery.rs`), because a `ReplayLog` has no world to say what day of the term it is.
- **More mystery.** More clues are rows in `mystery.toml`; more endings are a verdict row, a world effect, and an arc ending with `verdict:ID`; the checks refuse a verdict nothing casts.
- **The design's 60 scheme templates, 40 students and 12 staff.** See below.

### Content budget, said plainly

The design asks for about 60 scheme templates, 40 students, 12 staff, 150 school-day events, 25 minigame variants, 8 school events and a 40-step mystery. **This slice falls short of that**, and the pull request says so: see the counts in the next paragraph. The school events are partly there (the election, the awards, each club's term-end event, the Friday report, the big schemes); the house match, the winter play, the formal dance and exam week are not (`docs/features.md`).

CONTENT_COUNTS_PLACEHOLDER

### Not done, deliberately

Free-text player messages (the design forbids them where templates serve), a typed target of any kind, applying an effect to another pupil's save from the sender's session (the framework gives a session no write to another save), delivery in the middle of a visit (needs FW-1), the house match, the winter play, the formal dance and exam week, per-character storylines for all 40 students, 25 minigame variants (six kinds with levels), the design's 40 mystery steps (12 clues, 7 steps, 3 key routes and 6 endings), pacts or alliances between pupils, and a Hallpass-specific staff tool to reset one pupil's opt-out (staff cannot undo it; the pupil can).

### Verified

See the pull request for the commands and their results. In short: `tests/social.rs` (a scheme posted, delivered and its outcome heard, each counterplay, the list never offering what the engine refuses, the caps and the protection, kind schemes, the opt-out both ways and against a stale snapshot, the opt-out across a term, rumours and their limits, the election from standing to the winner's effect and through the rule of the mood, a close race by votes, the awards and their payment, and the whole two-player flow through the framework's real store and keys); `tests/mystery.rs` (the twelve clues in order with every gate, the day gate and a risky search, three routes to the key and a build with none, each of the six endings replayed twice through the store and the keys, the doors it shuts, the endings closed to builds that have not earned them, the five finales and what each does to the wing for a pupil with and without the key, the count and its tie-break, the finale through the real board and tick); `tests/migrate.rs` (versions 1 and 2 of a character and version 1 of the school against pinned bytes, and the store opening an old save and an old school); `tests/content.rs` (each new table broken on purpose); the framework's source audit (`tests/sandbox.rs`) with no new dependency; `tests/ui.rs` (the six new screens at every size, tier and colour setting, with a look-around, and snapshots); the simulation below. The walk over a real `ssh` client with two accounts is [qa/m4-h4-walk.md](../qa/m4-h4-walk.md).

BALANCE_PLACEHOLDER
