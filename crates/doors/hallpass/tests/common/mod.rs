//! Shared helpers for the Hallpass tests.
#![allow(
    dead_code,
    unreachable_pub,
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

pub mod bot;

use std::sync::Arc;

use russh_bbs_door::DoorRng;
use russh_bbs_door_hallpass::cond::{Cond, Op, Stat};
use russh_bbs_door_hallpass::content::Content;
use russh_bbs_door_hallpass::game::{Action, Event, Game, Refused};
use russh_bbs_door_hallpass::state::{
    ArcProgress, FileEntry, Membership, Wary, World, map_set, to_u8,
};

pub fn content() -> Arc<Content> {
    static CONTENT: std::sync::OnceLock<Arc<Content>> = std::sync::OnceLock::new();
    Arc::clone(CONTENT.get_or_init(|| Arc::new(Content::embedded().unwrap())))
}

/// The first Monday on or after day 20_000, so the timetable is known.
pub fn monday() -> i64 {
    (20_000..20_007).find(|d| Game::weekday(*d) == 0).unwrap()
}

/// A game with its own RNG, for scripted scenarios.
pub struct Sandbox {
    pub game: Game,
    pub rng: DoorRng,
}

impl Sandbox {
    /// A new character enrolled with `start_trait`, on a Monday morning.
    pub fn new(start_trait: &str, seed: u64) -> Self {
        Self::on(start_trait, seed, monday())
    }

    pub fn on(start_trait: &str, seed: u64, day: i64) -> Self {
        let content = content();
        let (game, _) = Game::open(&content, "ann", None, None, day);
        let mut sb = Self {
            game,
            rng: DoorRng::from_seed(seed),
        };
        sb.ok(Action::Enrol(start_trait.to_owned()));
        sb
    }

    pub fn ok(&mut self, action: Action) -> Vec<Event> {
        match self.game.apply(&action, &mut self.rng) {
            Ok(events) => events,
            Err(Refused(why)) => panic!("{action:?} was refused: {why}"),
        }
    }

    pub fn try_(&mut self, action: Action) -> Result<Vec<Event>, Refused> {
        self.game.apply(&action, &mut self.rng)
    }

    /// Spend `n` periods resting.
    pub fn pass(&mut self, n: usize) {
        for _ in 0..n {
            if self.game.current_slot().is_some() {
                self.ok(Action::Rest);
            }
        }
    }

    pub fn goto(&mut self, place: &str) {
        self.ok(Action::Go(place.to_owned()));
    }

    pub fn give(&mut self, item: &str, n: u8) {
        russh_bbs_door_hallpass::state::map_set(&mut self.game.player.inventory, item, n);
    }

    pub fn world(&self) -> &World {
        &self.game.world
    }

    /// Answer the lesson in progress with `right` puzzles right and the rest wrong (a test may
    /// read the answer; the screen never shows it).
    pub fn take_lesson(&mut self, right: usize) -> Vec<Event> {
        let mut events = Vec::new();
        let mut n = 0;
        while let Some(view) = self.game.lesson_view() {
            let pick = if n < right {
                view.round.answer
            } else {
                (0..view.round.options.len())
                    .find(|i| *i != view.round.answer && view.open[*i])
                    .unwrap()
            };
            events.extend(self.ok(Action::Answer(u8::try_from(pick).unwrap())));
            n += 1;
        }
        events
    }

    /// Go to the lesson (already in the right room) and answer `right` puzzles right.
    pub fn attend(&mut self, right: usize) -> Vec<Event> {
        let mut events = self.ok(Action::Attend);
        events.extend(self.take_lesson(right));
        events
    }
}

/// Count how many events satisfy `f`.
pub fn count(events: &[Event], f: impl Fn(&Event) -> bool) -> usize {
    events.iter().filter(|e| f(e)).count()
}

/// What a scenario gives a scheme before it is run.
#[derive(Default, Clone)]
pub struct Setup {
    pub twists: Vec<&'static str>,
    pub getaway: Option<&'static str>,
    pub scout: u8,
    pub allies: Vec<(&'static str, &'static str)>,
    pub know: Vec<&'static str>,
}

/// Give the character everything a scheme needs, draft it with the engine's own actions, and
/// time it for the first slot the target's schedule allows. Returns the scheme number.
pub fn arrange(sb: &mut Sandbox, target: &str, method: &str, setup: &Setup) -> u16 {
    let m = sb.game.content.method(method).unwrap().clone();
    for item in &m.needs {
        sb.give(item, 1);
    }
    for k in &setup.know {
        sb.game.player.learn(k);
    }
    sb.ok(Action::Draft {
        target: target.into(),
        method: method.into(),
    });
    let id = sb.game.player.schemes.last().unwrap().id;
    for t in &setup.twists {
        for item in sb
            .game
            .content
            .twist(t)
            .map(|_| Vec::<String>::new())
            .unwrap_or_default()
        {
            sb.give(&item, 1);
        }
        sb.ok(Action::AddTwist(id, (*t).into()));
    }
    if let Some(g) = setup.getaway {
        sb.ok(Action::SetGetaway(id, g.into()));
    }
    {
        let s = sb
            .game
            .player
            .schemes
            .iter_mut()
            .find(|s| s.id == id)
            .unwrap();
        s.scout = setup.scout;
        s.allies = setup
            .allies
            .iter()
            .map(|(n, r)| ((*n).to_owned(), (*r).to_owned()))
            .collect();
    }
    let scheme = sb.game.player.scheme(id).unwrap().clone();
    let (day, slot) = *sb
        .game
        .candidate_times(&scheme)
        .first()
        .expect("the schedule allows some time");
    sb.ok(Action::Schedule {
        scheme: id,
        day,
        slot,
    });
    id
}

/// Jump to the scheme's moment, stand where it needs, and run all three phases with `push` nerve
/// each. Returns every event.
pub fn run_scheme(sb: &mut Sandbox, id: u16, push: u8) -> Vec<Event> {
    let scheme = sb.game.player.scheme(id).unwrap().clone();
    let (day, slot) = scheme.when.unwrap();
    if day != sb.game.player.day {
        sb.game.sync_day(day);
    }
    sb.game.player.slot = slot;
    sb.game.player.nerve = sb.game.player.nerve_max;
    sb.game.player.place = sb.game.scheme_place(&scheme).unwrap();
    let mut events = sb.ok(Action::BeginRun(id));
    while sb.game.player.run.is_some() {
        events.extend(sb.ok(Action::RunPhase { push }));
    }
    events
}

pub fn pulled(events: &[Event]) -> bool {
    events.iter().any(|e| matches!(e, Event::Pulled { .. }))
}

pub fn busted(events: &[Event]) -> bool {
    events.iter().any(|e| matches!(e, Event::Busted { .. }))
}

pub fn legend_of(events: &[Event]) -> u32 {
    events
        .iter()
        .find_map(|e| match e {
            Event::Pulled { legend, .. } => Some(*legend),
            _ => None,
        })
        .unwrap_or(0)
}

/// Make `cond` hold for the character, directly in their state. Used to ask "is there any build
/// for which this is open?" without playing days to get there.
pub fn satisfy(game: &mut Game, cond: &Cond) {
    let day = game.player.day;
    match cond {
        Cond::Tag(t) => {
            let id = game
                .content
                .traits
                .iter()
                .find(|x| x.tags.contains(t))
                .map(|x| x.id.clone())
                .unwrap();
            if !game.player.traits.contains(&id) {
                game.player.traits.push(id);
            }
        }
        Cond::Trait(t) => {
            if !game.player.traits.contains(t) {
                game.player.traits.push(t.clone());
            }
        }
        Cond::Item(i) => map_set(&mut game.player.inventory, i, 1),
        Cond::Know(k) => game.player.learn(k),
        Cond::World(w) => game
            .world
            .active
            .push(russh_bbs_door_hallpass::state::Active {
                id: w.clone(),
                since: day,
                until: None,
                by: "x".into(),
            }),
        Cond::Arc { arc, step } => {
            game.player.life.arcs.retain(|a| a.id != *arc);
            game.player.life.arcs.push(ArcProgress {
                id: arc.clone(),
                step: step.clone(),
                since: day - 30,
                ended: None,
            });
        }
        Cond::Ended { arc, ending } => {
            game.player.life.arcs.retain(|a| a.id != *arc);
            game.player.life.arcs.push(ArcProgress {
                id: arc.clone(),
                step: "x".into(),
                since: day - 30,
                ended: Some(ending.clone()),
            });
        }
        Cond::Not(inner) => match inner.as_ref() {
            Cond::Trait(t) => game.player.traits.retain(|x| x != t),
            Cond::Know(k) => game.player.knowledge.retain(|x| x != k),
            _ => {}
        },
        Cond::Expelled => {}
        Cond::Cmp { stat, op, value } => {
            let v = match op {
                Op::Gt => value + 1,
                Op::Lt => value - 1,
                _ => *value,
            };
            let p = &mut game.player;
            match stat {
                Stat::Legend => p.legend = u32::try_from(v.max(0)).unwrap(),
                Stat::Notoriety => p.notoriety = to_u8(v),
                Stat::Heat => p.heat = to_u8(v),
                Stat::File => {
                    p.file.push(FileEntry {
                        day,
                        kind: "caught".into(),
                        sev: to_u8(v),
                    });
                }
                Stat::Cool => p.cool = i16::try_from(v).unwrap(),
                Stat::Nerve => {
                    p.nerve_max = p.nerve_max.max(to_u8(v));
                    p.nerve = to_u8(v);
                }
                Stat::Cash => p.cash = v,
                Stat::Grades => {
                    for (_, g) in &mut p.grades {
                        *g = to_u8(v);
                    }
                }
                Stat::Pulled => p.pulled = u16::try_from(v.max(0)).unwrap(),
                Stat::Caught => p.caught = u16::try_from(v.max(0)).unwrap(),
                Stat::Skill(id) => {
                    map_set(
                        &mut p.xp,
                        id,
                        if matches!(op, Op::Le | Op::Lt) {
                            0
                        } else {
                            100
                        },
                    );
                }
                Stat::Standing(id) => map_set(&mut p.standing, id, i16::try_from(v).unwrap()),
                Stat::Wary(id) => map_set(
                    &mut p.wary,
                    id,
                    Wary {
                        level: to_u8(v),
                        last: day,
                    },
                ),
                Stat::Grade(id) => map_set(&mut p.grades, id, to_u8(v)),
                Stat::Bond(id) => map_set(&mut p.life.bonds, id, i16::try_from(v).unwrap()),
                Stat::Rank(id) => {
                    p.life.clubs.retain(|m| m.club != *id);
                    if v > 0 {
                        p.life.clubs.push(Membership {
                            club: id.clone(),
                            rank: to_u8(v),
                            merit: 99,
                            last_meet: -1,
                        });
                    }
                }
                Stat::Day => game.world.term_start = day - i64::from(v),
                Stat::Clues => {
                    for i in 0..v.max(0) {
                        p.learn(&format!("clue_filler{i}"));
                    }
                }
            }
        }
    }
}

/// Stand the character where `npc` is in the first period of their day.
/// Put the character at a term event: the day of the term is `day` (inside the event's days), the
/// period is one the event runs in, and they stand where it is held.
pub fn at_event(game: &mut Game, event: &str, day: i64) {
    let spec = game
        .content
        .calendar
        .iter()
        .find(|e| e.id == event)
        .unwrap()
        .clone();
    assert!((spec.from..=spec.to).contains(&day), "{event} is not on day {day}");
    game.world.term_start = game.player.day - day;
    let slot = (0..game.periods())
        .find(|s| {
            spec.kinds.iter().any(|k| k == game.kind(game.player.day, *s))
                && game.place_open(&spec.place, game.player.day, *s)
        })
        .expect("the event has a period");
    game.player.slot = u8::try_from(slot).unwrap();
    game.player.place = spec.place;
}

pub fn meet(game: &mut Game, npc: &str) {
    let day = game.player.day;
    let spec = game.content.npc(npc).unwrap().clone();
    game.player.slot = 0;
    game.player.place = game.npc_place(&spec, day, 0).to_owned();
}
