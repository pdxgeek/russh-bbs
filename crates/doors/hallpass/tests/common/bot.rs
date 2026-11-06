//! A planner bot that plays the engine through its public actions only, for the term simulation.
#![allow(
    dead_code,
    unreachable_pub,
    missing_docs,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::collapsible_if,
    clippy::struct_excessive_bools
)]

use russh_bbs_door::DoorRng;
use russh_bbs_door_hallpass::game::{Action, Event, Game};
use russh_bbs_door_hallpass::state::Scheme;

/// How a bot plays.
#[derive(Debug, Clone)]
pub struct Policy {
    pub name: &'static str,
    /// Start no new scheme while heat is above this.
    pub max_heat: u8,
    /// Start no new scheme while the file is above this.
    pub max_file: i32,
    /// Nerve pushed on each phase.
    pub push: u8,
    /// Whether to go for big schemes.
    pub big: bool,
    /// Whether to skip class to set up.
    pub skip_class: bool,
    /// Extra legend appetite: add the calling card twist.
    pub flashy: bool,
    /// How many days of setup to leave before the act.
    pub lead_days: i64,
    /// Scouting level to reach before acting, where the place allows.
    pub scout_to: u8,
    /// Whether to recruit a lookout when one is willing.
    pub helpers: bool,
    /// Pick any feasible scheme at random, to exercise the content.
    pub variety: bool,
    /// Answer conversations at random among the open answers, to exercise the content.
    pub explore: bool,
    /// Percent of puzzles answered right (the bot may read the answer; a pupil would work it out).
    pub accuracy: u32,
    /// Percent of free periods spent on clubs and conversations.
    pub social: u32,
    /// Clubs to join, in order of preference.
    pub clubs: &'static [&'static str],
    /// Whether to give up a helper when the headmistress asks.
    pub informer: bool,
    /// Percent of free, evening and night periods spent searching for the east wing's clues.
    pub sleuth: u32,
    /// Percent of free periods spent on a scheme against another pupil.
    pub plots: u32,
    /// Percent of free periods spent on a rumour.
    pub rumours: u32,
    /// Whether to stand in the school election (and canvass), and to vote.
    pub politics: bool,
    /// Whether to opt out of player-versus-player play.
    pub opt_out: bool,
    /// Whether schemes on other pupils are mischief (true) or kindness (false).
    pub mean: bool,
    /// The verdicts this pupil will end the mystery with, in order of preference.
    pub verdicts: &'static [&'static str],
}

pub fn cautious() -> Policy {
    Policy {
        name: "cautious",
        max_heat: 8,
        max_file: 2,
        push: 0,
        big: false,
        skip_class: false,
        flashy: false,
        lead_days: 1,
        scout_to: 3,
        helpers: true,
        variety: false,
        explore: false,
        accuracy: 75,
        social: 30,
        clubs: &["chess", "paper"],
        informer: false,
        sleuth: 12,
        plots: 0,
        rumours: 10,
        politics: false,
        opt_out: false,
        mean: false,
        verdicts: &["fenn", "soames", "vane"],
    }
}

/// A pupil who lives at the boathouse and the school's events, and makes little mischief: the
/// one who takes the boat in the house match and the part in the play.
pub fn athlete() -> Policy {
    Policy {
        name: "athlete",
        max_heat: 8,
        max_file: 3,
        push: 1,
        big: false,
        skip_class: false,
        flashy: false,
        lead_days: 1,
        scout_to: 2,
        helpers: true,
        variety: false,
        explore: true,
        accuracy: 70,
        social: 80,
        clubs: &["rowing", "drama"],
        informer: false,
        sleuth: 5,
        plots: 0,
        rumours: 10,
        politics: false,
        opt_out: false,
        mean: false,
        verdicts: &["fenn", "walk_away"],
    }
}

pub fn balanced() -> Policy {
    Policy {
        name: "balanced",
        max_heat: 20,
        max_file: 5,
        push: 1,
        big: true,
        skip_class: false,
        flashy: false,
        lead_days: 1,
        scout_to: 2,
        helpers: true,
        variety: false,
        explore: false,
        accuracy: 70,
        social: 30,
        clubs: &["robotics", "paper", "chess"],
        informer: false,
        sleuth: 20,
        plots: 8,
        rumours: 10,
        politics: false,
        opt_out: false,
        mean: true,
        verdicts: &["wren", "fenn", "soames"],
    }
}

pub fn reckless() -> Policy {
    Policy {
        name: "reckless",
        max_heat: 70,
        max_file: 9,
        push: 2,
        big: true,
        skip_class: true,
        flashy: true,
        lead_days: 0,
        scout_to: 0,
        helpers: false,
        variety: false,
        explore: true,
        accuracy: 55,
        social: 15,
        clubs: &["rowing"],
        informer: true,
        sleuth: 15,
        plots: 15,
        rumours: 15,
        politics: false,
        opt_out: false,
        mean: true,
        verdicts: &["bonfire", "wren"],
    }
}

/// Tries everything it can, for coverage rather than for a score.
pub fn variety() -> Policy {
    Policy {
        name: "variety",
        max_heat: 60,
        max_file: 7,
        push: 1,
        big: true,
        skip_class: false,
        flashy: false,
        lead_days: 1,
        scout_to: 2,
        helpers: true,
        variety: true,
        explore: false,
        accuracy: 65,
        social: 70,
        clubs: &["chess", "drama", "rowing", "paper", "robotics", "candle"],
        informer: true,
        sleuth: 30,
        plots: 12,
        rumours: 15,
        politics: true,
        opt_out: false,
        mean: true,
        verdicts: &["vane", "walk_away", "soames", "fenn", "wren", "bonfire"],
    }
}

/// A pupil who does no mischief at all: lessons, clubs and conversations.
pub fn scholar() -> Policy {
    Policy {
        name: "scholar",
        max_heat: 0,
        max_file: -1,
        push: 0,
        big: false,
        skip_class: false,
        flashy: false,
        lead_days: 1,
        scout_to: 0,
        helpers: false,
        variety: false,
        explore: false,
        accuracy: 90,
        social: 70,
        clubs: &["chess", "paper", "robotics"],
        informer: false,
        sleuth: 8,
        plots: 0,
        rumours: 0,
        politics: true,
        opt_out: false,
        mean: false,
        verdicts: &["vane", "soames"],
    }
}

/// An investigator: spends most free evenings and nights on the east wing.
pub fn sleuth() -> Policy {
    Policy {
        name: "sleuth",
        max_heat: 12,
        max_file: 3,
        push: 1,
        big: false,
        skip_class: false,
        flashy: false,
        lead_days: 1,
        scout_to: 2,
        helpers: true,
        variety: false,
        explore: true,
        accuracy: 75,
        social: 40,
        clubs: &["candle", "paper"],
        informer: false,
        sleuth: 85,
        plots: 0,
        rumours: 8,
        politics: false,
        opt_out: false,
        mean: false,
        verdicts: &["fenn", "soames", "wren", "vane", "bonfire", "walk_away"],
    }
}

/// A campaigner: stands for Head of School, canvasses, votes and spreads word.
pub fn politician() -> Policy {
    Policy {
        name: "politician",
        max_heat: 6,
        max_file: 3,
        push: 0,
        big: false,
        skip_class: false,
        flashy: false,
        lead_days: 1,
        scout_to: 2,
        helpers: true,
        variety: false,
        explore: true,
        accuracy: 70,
        social: 60,
        clubs: &["paper", "drama"],
        informer: false,
        sleuth: 5,
        plots: 10,
        rumours: 30,
        politics: true,
        opt_out: false,
        mean: false,
        verdicts: &["wren", "vane"],
    }
}

/// A prankster who plays schemes on other pupils, and spreads gossip.
pub fn rival() -> Policy {
    Policy {
        name: "rival",
        max_heat: 20,
        max_file: 5,
        push: 1,
        big: false,
        skip_class: false,
        flashy: false,
        lead_days: 1,
        scout_to: 2,
        helpers: true,
        variety: false,
        explore: true,
        accuracy: 70,
        social: 40,
        clubs: &["candle", "robotics", "chess"],
        informer: false,
        sleuth: 10,
        plots: 45,
        rumours: 25,
        politics: false,
        opt_out: false,
        mean: true,
        verdicts: &["bonfire", "walk_away"],
    }
}

/// A pupil who opted out of player-versus-player play and just gets on with school.
pub fn recluse() -> Policy {
    Policy {
        name: "recluse",
        max_heat: 10,
        max_file: 3,
        push: 0,
        big: false,
        skip_class: false,
        flashy: false,
        lead_days: 1,
        scout_to: 2,
        helpers: true,
        variety: false,
        explore: false,
        accuracy: 80,
        social: 50,
        clubs: &["chess", "robotics"],
        informer: false,
        sleuth: 25,
        plots: 0,
        rumours: 5,
        politics: true,
        opt_out: true,
        mean: false,
        verdicts: &["soames", "walk_away"],
    }
}

// ---------------------------------------------------------------- branch coverage

thread_local! {
    static COVERAGE: std::cell::RefCell<std::collections::BTreeMap<String, u32>> =
        const { std::cell::RefCell::new(std::collections::BTreeMap::new()) };
}

/// Note that a branch was taken (per test thread).
pub fn note(what: String) {
    COVERAGE.with(|c| *c.borrow_mut().entry(what).or_default() += 1);
}

/// Take what this thread noted.
pub fn take_coverage() -> std::collections::BTreeMap<String, u32> {
    COVERAGE.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

fn answer_lesson(game: &mut Game, policy: &Policy, rng: &mut DoorRng, events: &mut Vec<Event>) {
    let Some(view) = game.lesson_view() else {
        return;
    };
    let right = rng.below(100) < policy.accuracy;
    let wrong: Vec<usize> = (0..view.round.options.len())
        .filter(|i| *i != view.round.answer && view.open[*i])
        .collect();
    let pick = if right || wrong.is_empty() {
        view.round.answer
    } else {
        wrong[usize::try_from(rng.below(u32::try_from(wrong.len()).unwrap())).unwrap()]
    };
    if let Ok(e) = game.apply(&Action::Answer(u8::try_from(pick).unwrap()), rng) {
        events.extend(e);
    }
}

fn answer_scene(game: &mut Game, policy: &Policy, rng: &mut DoorRng, events: &mut Vec<Event>) {
    let Some(view) = game.scene_view() else {
        return;
    };
    let open: Vec<usize> = view
        .choices
        .iter()
        .enumerate()
        .filter(|(_, c)| c.open && (policy.informer || !c.label.starts_with("Name the")))
        .map(|(i, _)| i)
        .collect();
    // An explorer wants to try something: choices that do nothing (the ways out) come last.
    let doing: Vec<usize> = if policy.explore {
        let node = game.scene_view().and_then(|_| {
            let scene = game.player.life.scene.as_ref()?;
            let talk = game.content.talk(&scene.talk)?;
            talk.node.iter().find(|n| n.id == scene.node).cloned()
        });
        node.map_or_else(Vec::new, |n| {
            open.iter()
                .copied()
                .filter(|i| n.choice.get(*i).is_some_and(|c| !c.effects.is_empty()))
                .collect()
        })
    } else {
        Vec::new()
    };
    // An explorer also wants what nobody has tried yet: answers no pupil has given come first.
    let doing: Vec<usize> = if policy.explore {
        let untried: Vec<usize> = game
            .player
            .life
            .scene
            .as_ref()
            .map(|scene| {
                open.iter()
                    .copied()
                    .filter(|i| {
                        COVERAGE.with(|c| {
                            !c.borrow()
                                .contains_key(&format!("choice:{}/{}/{i}", scene.talk, scene.node))
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if untried.is_empty() { doing } else { untried }
    } else {
        doing
    };
    // (One time in four it takes any open answer, the ways out included.)
    let open = if doing.is_empty() || rng.below(100) < 25 {
        open
    } else {
        doing
    };
    let pick = match open.as_slice() {
        [] => None,
        [only] => Some(*only),
        many if policy.variety || policy.explore || rng.below(100) < 35 => {
            Some(many[usize::try_from(rng.below(u32::try_from(many.len()).unwrap())).unwrap()])
        }
        many => Some(many[0]),
    };
    if let (Some(i), Some(scene)) = (pick, game.player.life.scene.as_ref()) {
        note(format!("choice:{}/{}/{i}", scene.talk, scene.node));
    }
    let action = pick.map_or(Action::Abandon, |i| {
        Action::Choose(u8::try_from(i).unwrap())
    });
    if let Ok(e) = game.apply(&action, rng) {
        events.extend(e);
    } else if let Ok(e) = game.apply(&Action::Abandon, rng) {
        events.extend(e);
    }
}

// ---------------------------------------------------------------- the social layer and the mystery

/// Distance in corridors from every place to `to`, over the school's doors (locks ignored).
fn distances(game: &Game, to: &str) -> std::collections::HashMap<String, u32> {
    let mut out = std::collections::HashMap::new();
    out.insert(to.to_owned(), 0u32);
    let mut queue = std::collections::VecDeque::from([to.to_owned()]);
    while let Some(at) = queue.pop_front() {
        let d = out[&at];
        for p in &game.content.places {
            let touches = p.exits.contains(&at)
                || game
                    .content
                    .place(&at)
                    .is_some_and(|a| a.exits.contains(&p.id));
            if touches && !out.contains_key(&p.id) {
                out.insert(p.id.clone(), d + 1);
                queue.push_back(p.id.clone());
            }
        }
    }
    out
}

/// Walk toward `place` as far as this period's corridors allow. True if the bot is there.
fn travel(game: &mut Game, place: &str, rng: &mut DoorRng, events: &mut Vec<Event>) -> bool {
    let dist = distances(game, place);
    for _ in 0..6 {
        if game.player.place == place {
            return true;
        }
        let Some(d_here) = dist.get(&game.player.place).copied() else {
            return false;
        };
        let best = game
            .routes()
            .into_iter()
            .filter_map(|(p, _)| dist.get(&p).map(|d| (p, *d)))
            .filter(|(_, d)| *d < d_here)
            .min_by_key(|(_, d)| *d);
        let Some((next, _)) = best else {
            return false;
        };
        match game.apply(&Action::Go(next), rng) {
            Ok(e) => events.extend(e),
            Err(_) => return false,
        }
    }
    game.player.place == place
}

/// One step of the east wing's mystery: walk to where the next clue is and search. True if the
/// bot did anything with the period.
fn sleuth_step(game: &mut Game, rng: &mut DoorRng, events: &mut Vec<Event>) -> bool {
    let Some(slot) = game.current_slot() else {
        return false;
    };
    let day = game.player.day;
    let clue = game
        .content
        .mystery
        .clues
        .iter()
        .find(|c| {
            !game.player.knows(&format!("clue_{}", c.id))
                && c.after
                    .iter()
                    .all(|a| game.player.knows(&format!("clue_{a}")))
                && game.all_hold(&c.needs)
        })
        .cloned();
    let Some(clue) = clue else {
        return befriend_for_clue(game, rng, events);
    };
    let now_ok = clue.kinds.iter().any(|k| k == game.kind(day, slot))
        && game.place_open(&clue.place, day, slot);
    let later_ok = (slot + 1..9).any(|s| {
        clue.kinds.iter().any(|k| k == game.kind(day, s)) && game.place_open(&clue.place, day, s)
    });
    let before = (game.player.place.clone(), game.player.slot);
    if now_ok {
        if travel(game, &clue.place, rng, events) {
            if let Ok(e) = game.apply(&Action::Search, rng) {
                events.extend(e);
                if game.player.knows(&format!("clue_{}", clue.id)) {
                    note(format!("clue:{}", clue.id));
                }
                return true;
            }
        }
    } else if later_ok {
        let _ = travel(game, &clue.place, rng, events);
    }
    (game.player.place.clone(), game.player.slot) != before
}

/// A term event is on and a scene of it is open to the pupil: go to the event and take part.
/// True if the bot did anything with the period.
fn event_step(game: &mut Game, rng: &mut DoorRng, events: &mut Vec<Event>) -> bool {
    let Some(slot) = game.current_slot() else {
        return false;
    };
    let day = game.player.day;
    let events_on: Vec<_> = game
        .content
        .calendar
        .iter()
        .filter(|e| game.event_open_at(e, slot, &e.place))
        .cloned()
        .collect();
    for event in events_on {
        let mut scenes: Vec<_> = game
            .content
            .talks
            .iter()
            .filter(|t| t.event.as_deref() == Some(event.id.as_str()) && game.talk_ready(t))
            .cloned()
            .collect();
        if scenes.is_empty() || !game.place_open(&event.place, day, slot) {
            continue;
        }
        for i in (1..scenes.len()).rev() {
            let j = usize::try_from(rng.below(u32::try_from(i + 1).unwrap())).unwrap();
            scenes.swap(i, j);
        }
        if !go_then(game, &event.place, rng, events) {
            continue;
        }
        for scene in scenes {
            if game.talk_open(&scene) {
                if let Ok(e) = game.apply(&Action::Talk(scene.id.clone()), rng) {
                    events.extend(e);
                    return true;
                }
            }
        }
    }
    false
}

/// The next clue is shut only by a friendship: go and talk to that character, if they have
/// something to say. True if the bot did anything with the period.
fn befriend_for_clue(game: &mut Game, rng: &mut DoorRng, events: &mut Vec<Event>) -> bool {
    use russh_bbs_door_hallpass::cond::{Cond, Stat};
    let Some(slot) = game.current_slot() else {
        return false;
    };
    let day = game.player.day;
    let wanted: Vec<String> = game
        .content
        .mystery
        .clues
        .iter()
        .filter(|c| {
            !game.player.knows(&format!("clue_{}", c.id))
                && c.after
                    .iter()
                    .all(|a| game.player.knows(&format!("clue_{a}")))
        })
        .flat_map(|c| {
            let others_hold = c.needs.iter().all(|n| {
                matches!(
                    n,
                    Cond::Cmp {
                        stat: Stat::Bond(_),
                        ..
                    }
                ) || game.holds(n)
            });
            c.needs
                .iter()
                .filter_map(move |n| match n {
                    Cond::Cmp {
                        stat: Stat::Bond(npc),
                        ..
                    } if others_hold => Some(npc.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .filter(|npc| {
            let need = game
                .content
                .mystery
                .clues
                .iter()
                .flat_map(|c| c.needs.iter())
                .filter_map(|n| match n {
                    Cond::Cmp {
                        stat: Stat::Bond(x),
                        value,
                        ..
                    } if x == npc => Some(*value),
                    _ => None,
                })
                .max()
                .unwrap_or(0);
            game.bond(npc) < need
        })
        .collect();
    for npc in wanted {
        let talk = game
            .content
            .talks
            .iter()
            .find(|t| t.npc == npc && t.event.is_none() && game.talk_ready(t))
            .cloned();
        let Some(talk) = talk else { continue };
        let Some(spec) = game.content.npc(&npc).cloned() else {
            continue;
        };
        let at = game.npc_place(&spec, day, slot).to_owned();
        if game.place_open(&at, day, slot)
            && go_then(game, &at, rng, events)
            && game.talk_open(&talk)
        {
            if let Ok(e) = game.apply(&Action::Talk(talk.id.clone()), rng) {
                events.extend(e);
                return true;
            }
        }
    }
    false
}

/// A scheme on another pupil, if one is open to the bot.
fn plot_step(game: &mut Game, policy: &Policy, rng: &mut DoorRng, events: &mut Vec<Event>) -> bool {
    let mut options: Vec<Action> = Vec::new();
    for spec in &game.content.social.plots {
        if (spec.kind == "mischief") != policy.mean {
            continue;
        }
        for mate in game.classmates() {
            if game.plot_block(&spec.id, &mate.name).is_none() {
                options.push(Action::Plot {
                    kind: spec.id.clone(),
                    to: mate.name,
                });
            }
        }
    }
    if options.is_empty() {
        return false;
    }
    let pick = usize::try_from(rng.below(u32::try_from(options.len()).unwrap())).unwrap();
    if let Action::Plot { kind, .. } = &options[pick] {
        note(format!("plot:{kind}"));
    }
    match game.apply(&options[pick], rng) {
        Ok(e) => {
            events.extend(e);
            true
        }
        Err(_) => false,
    }
}

/// A rumour, if one can be started here and now.
fn rumour_step(
    game: &mut Game,
    policy: &Policy,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> bool {
    let mut options: Vec<Action> = Vec::new();
    for spec in &game.content.social.rumours {
        let nasty = spec.kind == "gossip";
        if nasty && !policy.mean {
            continue;
        }
        for about in game.content.rumour_subjects(spec) {
            if game.rumour_block(&spec.id, &about).is_none() {
                options.push(Action::Spread {
                    rumour: spec.id.clone(),
                    about,
                });
            }
        }
    }
    if options.is_empty() {
        return false;
    }
    let pick = usize::try_from(rng.below(u32::try_from(options.len()).unwrap())).unwrap();
    if let Action::Spread { rumour, .. } = &options[pick] {
        note(format!("rumour:{rumour}"));
    }
    match game.apply(&options[pick], rng) {
        Ok(e) => {
            events.extend(e);
            true
        }
        Err(_) => false,
    }
}

/// Stand, canvass and vote, as the policy's politics go. True if it used the period.
fn politics_step(game: &mut Game, rng: &mut DoorRng, events: &mut Vec<Event>) -> bool {
    // About half of the politically minded decide, once, to stand; the rest only vote.
    if game.can(&Action::Nominate) && !game.player.knows("declined_election") {
        if rng.below(100) < 50 {
            if let Ok(e) = game.apply(&Action::Nominate, rng) {
                events.extend(e);
            }
        } else {
            game.player.learn("declined_election");
        }
    }
    if game.voting_open() && game.player.social.voted_term != game.player.term {
        let ballot = game.ballot();
        let me = game.player.name.clone();
        let choice = if game.player.social.ran_term == game.player.term {
            Some(me)
        } else {
            let n = u32::try_from(ballot.len()).unwrap();
            (n > 0).then(|| ballot[usize::try_from(rng.below(n)).unwrap()].id.clone())
        };
        if let Some(who) = choice {
            if let Ok(e) = game.apply(&Action::Vote(who), rng) {
                events.extend(e);
            }
        }
    }
    if game.can(&Action::Canvass) && rng.below(100) < 60 {
        if let Ok(e) = game.apply(&Action::Canvass, rng) {
            events.extend(e);
            return true;
        }
    }
    false
}

/// Which verdict a talk at the choice casts (or `walk_away`).
fn verdict_of_talk(talk: &str) -> Option<&'static str> {
    Some(match talk {
        "wing_fenn_choice" => "fenn",
        "wing_soames_choice" => "soames",
        "wing_vane_choice" => "vane",
        "wing_wren_choice" => "wren",
        "wing_sol_choice" => "bonfire",
        "wing_tilly_walk_away" => "walk_away",
        _ => return None,
    })
}

/// A club meeting, a sign-up or a conversation, whichever is on offer. True if anything happened.
fn social_step(
    game: &mut Game,
    policy: &Policy,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> bool {
    let Some(slot) = game.current_slot() else {
        return false;
    };
    let day = game.player.day;
    let kind = game.kind(day, slot).to_owned();
    if kind == "class" {
        return false;
    }
    let weekday = u8::try_from(Game::weekday(day)).unwrap();
    let meetings: Vec<_> = game
        .player
        .life
        .clubs
        .iter()
        .filter_map(|m| game.content.club(&m.club).map(|c| (c.clone(), m.last_meet)))
        .collect();
    for (club, last) in meetings {
        if club.kind == kind
            && (club.days.is_empty() || club.days.contains(&weekday))
            && last != day
            && game.place_open(&club.place, day, slot)
            && game.player.due == 0
            && go_then(game, &club.place, rng, events)
        {
            if let Ok(e) = game.apply(&Action::Meet, rng) {
                events.extend(e);
                return true;
            }
        }
    }
    // After lights-out only a club that meets then (the Candle Society) is worth the risk.
    if kind == "night" {
        return false;
    }
    if game.player.life.clubs.len() < game.content.rules.max_clubs {
        for id in policy.clubs {
            let Some(club) = game.content.club(id).cloned() else {
                continue;
            };
            let sign = club
                .join_place
                .clone()
                .unwrap_or_else(|| club.place.clone());
            if game.player.life.rank_in(id) == 0
                && game.all_hold(&club.join)
                && game.place_open(&sign, day, slot)
                && go_then(game, &sign, rng, events)
                && game.join_block(&club).is_none()
            {
                if let Ok(e) = game.apply(&Action::Join((*id).to_owned()), rng) {
                    events.extend(e);
                    return true;
                }
            }
        }
    }
    let talks: Vec<_> = game
        .content
        .talks
        .iter()
        .filter(|t| game.talk_ready(t))
        .cloned()
        .collect();
    // A pupil follows a story they are in before they chat: the storylines' scenes first, in the
    // table's order; then half the time the rest in the table's order and half the time in any.
    // (Whoever wants the Candle Society goes to Sol for the knock first: it is the way in.)
    let (mut story, mut chat): (Vec<_>, Vec<_>) = talks.into_iter().partition(|t| {
        t.arc.is_some() || (t.id == "sol_candle" && policy.clubs.contains(&"candle"))
    });
    if rng.below(100) < 50 {
        for i in (1..chat.len()).rev() {
            let j = usize::try_from(rng.below(u32::try_from(i + 1).unwrap())).unwrap();
            chat.swap(i, j);
        }
    }
    story.extend(chat);
    let mut talks = story;
    // At the end of the mystery a pupil only takes the conversations they would end it with, in
    // the order they would prefer.
    talks.retain(|t| verdict_of_talk(&t.id).is_none_or(|v| policy.verdicts.contains(&v)));
    talks.sort_by_key(|t| {
        verdict_of_talk(&t.id)
            .and_then(|v| policy.verdicts.iter().position(|x| *x == v))
            .unwrap_or(usize::MAX)
    });
    for talk in talks {
        // A scene of a term event is held at the event's place, whoever is about.
        if let Some(event) = talk
            .event
            .as_ref()
            .and_then(|e| game.content.calendar.iter().find(|c| c.id == *e))
            .cloned()
        {
            if game.event_open_at(&event, slot, &event.place)
                && game.place_open(&event.place, day, slot)
                && go_then(game, &event.place, rng, events)
                && game.talk_open(&talk)
            {
                if let Ok(e) = game.apply(&Action::Talk(talk.id.clone()), rng) {
                    events.extend(e);
                    return true;
                }
            }
            continue;
        }
        let Some(npc) = game.content.npc(&talk.npc).cloned() else {
            continue;
        };
        let at = game.npc_place(&npc, day, slot).to_owned();
        if game.place_open(&at, day, slot)
            && go_then(game, &at, rng, events)
            && game.talk_open(&talk)
        {
            if let Ok(e) = game.apply(&Action::Talk(talk.id.clone()), rng) {
                events.extend(e);
                return true;
            }
        }
    }
    false
}

fn go_then(game: &mut Game, place: &str, rng: &mut DoorRng, events: &mut Vec<Event>) -> bool {
    if game.player.place == place {
        return true;
    }
    match game.apply(&Action::Go(place.to_owned()), rng) {
        Ok(e) => {
            events.extend(e);
            game.player.place == place
        }
        Err(_) => false,
    }
}

fn shop_place(game: &Game, item: &str) -> Option<String> {
    let shop = &game.content.item(item)?.shop;
    game.content
        .places
        .iter()
        .find(|p| p.shop.as_ref() == Some(shop))
        .map(|p| p.id.clone())
}

/// Roles a scheme is missing that someone can in principle supply to this character.
fn feasible(game: &Game, method: &str) -> bool {
    let Some(m) = game.content.method(method) else {
        return false;
    };
    let cost: i32 = m
        .needs
        .iter()
        .filter_map(|i| game.content.item(i))
        .map(|i| i.price)
        .sum();
    let roles_ok = m.roles.iter().all(|role| {
        game.content.npcs.iter().any(|n| {
            n.ally
                .iter()
                .any(|a| a.role == *role && game.all_hold(&a.needs))
        })
    });
    let items_ok = m.needs.iter().all(|i| {
        game.content
            .item(i)
            .is_none_or(|it| game.all_hold(&it.needs))
    });
    // The place must be one this character can get into, or one scouting teaches the way into.
    let place_ok = game.content.method(method).is_some_and(|m| {
        game.content.place(&m.place).is_some_and(|p| {
            p.lock.is_none()
                || p.unlock.iter().any(|c| {
                    game.holds(c) || matches!(c, russh_bbs_door_hallpass::cond::Cond::Know(_))
                })
        })
    });
    roles_ok && items_ok && place_ok && cost <= game.player.cash + 2 * game.content.rules.allowance
}

fn pick_draft(game: &Game, policy: &Policy, rng: &mut DoorRng) -> Option<(String, String)> {
    let mut pool: Vec<(String, String)> = Vec::new();
    let mut best: Option<(i32, (String, String))> = None;
    for (target, method) in game.drafts() {
        let m = game.content.method(&method)?;
        if m.big && !policy.big {
            continue;
        }
        if !feasible(game, &method)
            || game
                .player
                .schemes
                .iter()
                .any(|s| s.target == target && s.method == method)
        {
            continue;
        }
        let t = game.content.target(&target)?;
        let wary = t.npc.as_deref().map_or(0, |n| game.player.wary_of(n));
        let done = i32::from(
            russh_bbs_door_hallpass::state::map_get(
                &game.player.done,
                &format!("{target}/{method}"),
            )
            .copied()
            .unwrap_or(0),
        );
        pool.push((target.clone(), method.clone()));
        let campaign = game
            .player
            .campaign
            .as_ref()
            .is_some_and(|c| c.theme == target);
        let score = m.legend * 3 - m.difficulty - 10 * wary - 25 * done
            + if campaign { 12 } else { 0 }
            - 2 * m.heat;
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, (target, method)));
        }
    }
    if policy.variety && !pool.is_empty() {
        // For coverage: methods nobody has drafted yet come first, so the long tail is reached.
        let fresh: Vec<(String, String)> = pool
            .iter()
            .filter(|(_, m)| COVERAGE.with(|c| !c.borrow().contains_key(&format!("method:{m}"))))
            .cloned()
            .collect();
        let from = if fresh.is_empty() { pool } else { fresh };
        let pick = usize::try_from(rng.below(u32::try_from(from.len()).unwrap_or(1))).unwrap_or(0);
        return from.into_iter().nth(pick);
    }
    best.map(|(_, d)| d)
}

fn setup_step(
    game: &mut Game,
    policy: &Policy,
    scheme: &Scheme,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> bool {
    let ready = game.readiness(scheme);
    let place = game.scheme_place(scheme).unwrap_or_default();
    let day = game.player.day;
    let slot = game.current_slot().unwrap_or(0);
    // Items first.
    if let Some(item) = ready.missing_items.first() {
        let Some(shop) = shop_place(game, item) else {
            return false;
        };
        let price = game.content.item(item).map_or(0, |i| i.price);
        if game.player.cash < price {
            let _ = game.apply(&Action::Drop(scheme.id), rng);
            return true;
        }
        if !game.place_open(&shop, day, slot) {
            return false;
        }
        if go_then(game, &shop, rng, events) {
            if let Ok(e) = game.apply(&Action::Buy(item.clone()), rng) {
                events.extend(e);
                return true;
            }
        }
        return false;
    }
    // Then helpers.
    if let Some(role) = ready.missing_roles.first() {
        if let Some((npc, r, _)) = game
            .recruitable(scheme)
            .into_iter()
            .find(|(_, r, _)| r == role)
        {
            if let Ok(e) = game.apply(
                &Action::Recruit {
                    scheme: scheme.id,
                    npc,
                    role: r,
                },
                rng,
            ) {
                events.extend(e);
                return true;
            }
        }
        // Find someone who offers the role and is somewhere we can walk to.
        let target = game.content.npcs.iter().find(|n| {
            n.ally
                .iter()
                .any(|a| a.role == *role && game.all_hold(&a.needs))
                && game.player.ally_used.iter().all(|(a, _)| *a != n.id)
        });
        if let Some(npc) = target {
            let at = game.npc_place(npc, day, slot).to_owned();
            if go_then(game, &at, rng, events) {
                // We are with them: ask on the next pass of the loop.
                return game.recruitable(scheme).iter().any(|(_, r, _)| r == role);
            }
        }
        return false;
    }
    // Optional helpers: a lookout, when one is willing.
    if policy.helpers
        && !scheme.allies.iter().any(|(_, r)| r == "lookout")
        && scheme.allies.len() < 3
    {
        if let Some((npc, r, cost)) = game
            .recruitable(scheme)
            .into_iter()
            .find(|(_, r, c)| r == "lookout" && *c <= game.player.cash / 2)
        {
            let _ = cost;
            if let Ok(e) = game.apply(
                &Action::Recruit {
                    scheme: scheme.id,
                    npc,
                    role: r,
                },
                rng,
            ) {
                events.extend(e);
                return true;
            }
        }
    }
    // Then scouting.
    let want_scout = ready.scout_need.max(policy.scout_to.min(3));
    if ready.scout_have < want_scout {
        if game.near(&game.player.place.clone(), &place) {
            if let Ok(e) = game.apply(&Action::Scout(scheme.id), rng) {
                events.extend(e);
                return true;
            }
        }
        // Walk to the place or next door.
        if go_then(game, &place, rng, events) {
            if let Ok(e) = game.apply(&Action::Scout(scheme.id), rng) {
                events.extend(e);
                return true;
            }
        }
        return false;
    }
    // Then a time.
    if scheme.when.is_none() {
        let earliest = day + policy.lead_days;
        let place_witnesses = |d: i64, s: u8| game.witnesses_at(&place, d, usize::from(s));
        let pick = game
            .candidate_times(scheme)
            .into_iter()
            .filter(|(d, s)| *d > earliest || (*d == earliest && usize::from(*s) > slot))
            .filter(|t| game.player.schemes.iter().all(|o| o.when != Some(*t)))
            .min_by_key(|(d, s)| (place_witnesses(*d, *s), *d, *s));
        if let Some((d, s)) = pick {
            if let Ok(e) = game.apply(
                &Action::Schedule {
                    scheme: scheme.id,
                    day: d,
                    slot: s,
                },
                rng,
            ) {
                events.extend(e);
                return true;
            }
        } else if game.candidate_times(scheme).is_empty() {
            let _ = game.apply(&Action::Drop(scheme.id), rng);
            return true;
        }
    }
    false
}

/// Play one school day. Returns the events.
pub fn play_day(game: &mut Game, policy: &Policy, rng: &mut DoorRng) -> Vec<Event> {
    let mut events = Vec::new();
    let mut guard = 0;
    if policy.opt_out && !game.player.social.opt_out {
        if let Ok(e) = game.apply(&Action::OptOut(true), rng) {
            events.extend(e);
        }
    }
    while game.playing() && game.current_slot().is_some() && guard < 80 {
        guard += 1;
        if game.player.life.lesson.is_some() {
            answer_lesson(game, policy, rng, &mut events);
            continue;
        }
        if game.player.life.scene.is_some() {
            answer_scene(game, policy, rng, &mut events);
            continue;
        }
        if let Some(run) = game.player.run.clone() {
            let push = policy.push.min(game.player.nerve);
            if let Ok(e) = game.apply(&Action::RunPhase { push }, rng) {
                events.extend(e);
            }
            let _ = run;
            continue;
        }
        let slot = game.current_slot().unwrap();
        let day = game.player.day;
        // A scheme whose moment has come.
        let moment: Vec<Scheme> = game
            .player
            .schemes
            .iter()
            .filter(|s| {
                s.when == Some((day, u8::try_from(slot).unwrap())) && game.readiness(s).ready()
            })
            .cloned()
            .collect();
        if let Some(s) = moment.first() {
            let place = game.scheme_place(s).unwrap();
            if go_then(game, &place, rng, &mut events) {
                if let Ok(e) = game.apply(&Action::BeginRun(s.id), rng) {
                    events.extend(e);
                    continue;
                }
            }
        }
        // Detention.
        if game.player.due > 0 && game.kind(day, slot) == "after" {
            if go_then(game, "detention_room", rng, &mut events) {
                if let Ok(e) = game.apply(&Action::Attend, rng) {
                    events.extend(e);
                    continue;
                }
            }
        }
        // Setup, unless it is class time and we want to be a good student.
        let in_class = game.kind(day, slot) == "class";
        let schemes = game.player.schemes.clone();
        if !in_class || policy.skip_class {
            let mut worked = false;
            for s in &schemes {
                if setup_step(game, policy, s, rng, &mut events) {
                    worked = true;
                    break;
                }
            }
            if worked && game.current_slot() != Some(slot) {
                continue;
            }
            if worked {
                continue;
            }
        }
        // The mystery, the other pupils, the election.
        if (!in_class || policy.skip_class) && game.player.due == 0 {
            let night = game.kind(day, slot) == "night";
            if rng.below(100) < 45 + policy.social / 2 && event_step(game, rng, &mut events) {
                continue;
            }
            if policy.sleuth > 0
                && rng.below(100) < policy.sleuth
                && sleuth_step(game, rng, &mut events)
            {
                continue;
            }
            if !night && policy.politics && politics_step(game, rng, &mut events) {
                continue;
            }
            if !night
                && policy.plots > 0
                && rng.below(100) < policy.plots
                && plot_step(game, policy, rng, &mut events)
            {
                continue;
            }
            if !night
                && policy.rumours > 0
                && rng.below(100) < policy.rumours
                && rumour_step(game, policy, rng, &mut events)
            {
                continue;
            }
        }
        // A new scheme.
        let p = &game.player;
        if p.schemes.len() < 2
            && p.heat <= policy.max_heat
            && game.player.file_points() <= policy.max_file
            && game
                .player
                .schemes
                .iter()
                .all(|s| game.readiness(s).scheduled)
        {
            if let Some((target, method)) = pick_draft(game, policy, rng) {
                let drafted = method.clone();
                if game.apply(&Action::Draft { target, method }, rng).is_ok() {
                    note(format!("method:{drafted}"));
                    let id = game.player.schemes.last().unwrap().id;
                    if policy.flashy {
                        let _ = game.apply(&Action::AddTwist(id, "calling_card".into()), rng);
                    } else if game.skill_level("sneak") > 0 {
                        let _ = game.apply(&Action::AddTwist(id, "no_trace".into()), rng);
                    }
                    for g in ["lie_low", "back_stairs", "sprint", "blend_in"] {
                        if !policy.flashy
                            && game.apply(&Action::SetGetaway(id, g.into()), rng).is_ok()
                        {
                            break;
                        }
                    }
                    continue;
                }
            }
        }
        // Clubs and conversations, when the period is free for them.
        if !in_class
            && rng.below(100) < policy.social
            && social_step(game, policy, rng, &mut events)
        {
            continue;
        }
        // Otherwise be a student.
        if in_class {
            let subject = game
                .activity(day, slot)
                .strip_prefix("class:")
                .unwrap_or("")
                .to_owned();
            if let Some(room) = game.content.subject(&subject).map(|s| s.room.clone()) {
                if go_then(game, &room, rng, &mut events) {
                    if let Ok(e) = game.apply(&Action::Attend, rng) {
                        events.extend(e);
                        continue;
                    }
                }
            }
        }
        let low_grade = game
            .content
            .subjects
            .iter()
            .map(|s| s.id.clone())
            .min_by_key(|s| game.stat(&russh_bbs_door_hallpass::cond::Stat::Grade(s.clone())))
            .unwrap();
        if game.kind(day, slot) != "night"
            && game.kind(day, slot) != "evening"
            && go_then(game, "library", rng, &mut events)
        {
            if let Ok(e) = game.apply(&Action::Study(low_grade), rng) {
                events.extend(e);
                continue;
            }
        }
        if let Ok(e) = game.apply(&Action::Rest, rng) {
            events.extend(e);
        } else if let Ok(e) = game.apply(&Action::Loiter, rng) {
            events.extend(e);
        } else {
            break;
        }
    }
    // Whatever is left of the day.
    while game.playing() && game.current_slot().is_some() {
        if game.player.run.is_some() {
            let push = policy.push.min(game.player.nerve);
            let _ = game.apply(&Action::RunPhase { push }, rng);
        } else if game.apply(&Action::Rest, rng).is_err() {
            break;
        }
    }
    events
}
