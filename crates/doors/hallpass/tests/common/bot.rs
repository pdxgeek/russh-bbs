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
    /// Percent of puzzles answered right (the bot may read the answer; a pupil would work it out).
    pub accuracy: u32,
    /// Percent of free periods spent on clubs and conversations.
    pub social: u32,
    /// Clubs to join, in order of preference.
    pub clubs: &'static [&'static str],
    /// Whether to give up a helper when the headmistress asks.
    pub informer: bool,
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
        accuracy: 75,
        social: 30,
        clubs: &["chess", "paper"],
        informer: false,
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
        accuracy: 70,
        social: 30,
        clubs: &["robotics", "paper", "chess"],
        informer: false,
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
        accuracy: 55,
        social: 15,
        clubs: &["rowing"],
        informer: true,
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
        accuracy: 65,
        social: 40,
        clubs: &["chess", "drama", "rowing", "paper", "robotics", "candle"],
        informer: false,
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
        accuracy: 90,
        social: 70,
        clubs: &["chess", "paper", "robotics"],
        informer: false,
    }
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
    let pick = match open.as_slice() {
        [] => None,
        [only] => Some(*only),
        many if rng.below(100) < 35 => {
            Some(many[usize::try_from(rng.below(u32::try_from(many.len()).unwrap())).unwrap()])
        }
        many => Some(many[0]),
    };
    let action = pick.map_or(Action::Abandon, |i| {
        Action::Choose(u8::try_from(i).unwrap())
    });
    if let Ok(e) = game.apply(&action, rng) {
        events.extend(e);
    } else if let Ok(e) = game.apply(&Action::Abandon, rng) {
        events.extend(e);
    }
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
    if kind == "class" || kind == "night" {
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
    for talk in talks {
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
        let pick = usize::try_from(rng.below(u32::try_from(pool.len()).unwrap_or(1))).unwrap_or(0);
        return pool.into_iter().nth(pick);
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
                if game.apply(&Action::Draft { target, method }, rng).is_ok() {
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
