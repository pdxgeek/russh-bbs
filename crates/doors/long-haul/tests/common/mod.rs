//! Shared setup for the engine tests: a frontier, a world, a seeded RNG and a day, and a few
//! ways to put a captain where a test needs one.
#![allow(
    clippy::single_match_else,
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::panic,
    clippy::unused_self,
    missing_docs
)]

pub mod ui;

use russh_bbs_door::DoorRng;
use russh_bbs_door_long_haul::combat::{Maneuver, Orders};
use russh_bbs_door_long_haul::content::Content;
use russh_bbs_door_long_haul::crew::{CrewMember, Goal, GoalKind, Role};
use russh_bbs_door_long_haul::engine::{Action, Env};
use russh_bbs_door_long_haul::events::{Event, Refusal};
use russh_bbs_door_long_haul::frontier::Frontier;
use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::scene::Scene;
use russh_bbs_door_long_haul::world::{Delta, World};

pub const DAY0: i64 = 20_000;

pub struct Harness {
    pub content: Content,
    pub frontier: Frontier,
    pub world: World,
    pub rng: DoorRng,
    pub today: i64,
    pub deltas: Vec<Delta>,
    made: std::cell::Cell<u32>,
}

impl Harness {
    pub fn new(season: u32, seed: u64) -> Self {
        let content = Content::embedded().unwrap();
        let frontier = Frontier::generate(&content, season);
        let world = World::fresh(&content, &frontier, DAY0);
        Self {
            content,
            frontier,
            world,
            rng: DoorRng::from_seed(seed),
            today: DAY0,
            deltas: Vec::new(),
            made: std::cell::Cell::new(0),
        }
    }

    /// A new captain with the background whose id is `background`.
    pub fn captain(&self, background: &str) -> Player {
        let ix = self
            .content
            .backgrounds
            .iter()
            .position(|b| b.id == background)
            .unwrap();
        let n = self.made.get();
        self.made.set(n + 1);
        Player::new(
            &self.content,
            &self.frontier,
            &format!("Tester{n}"),
            ix,
            self.today,
        )
    }

    pub fn reseed(&mut self, seed: u64) {
        self.rng = DoorRng::from_seed(seed);
    }

    /// Settle the day, then do the action.
    pub fn go(&mut self, p: &mut Player, action: &Action) -> Result<Vec<Event>, Refusal> {
        let mut env = Env::new(
            &self.content,
            &self.frontier,
            &mut self.world,
            &mut self.rng,
            self.today,
        );
        p.settle(&mut env);
        // The engine tests that came before transit events act as if an event that opened on
        // arrival had been waved away. Tests of scenes use scene actions, which keep it.
        if !action.is_scene() {
            p.scene = None;
        }
        let result = p.apply(&mut env, action);
        self.deltas.append(&mut env.deltas);
        result
    }

    /// Like [`Harness::go`] but leaves any open scene in place, so `Busy` can be seen.
    pub fn go_strict(&mut self, p: &mut Player, action: &Action) -> Result<Vec<Event>, Refusal> {
        let mut env = Env::new(
            &self.content,
            &self.frontier,
            &mut self.world,
            &mut self.rng,
            self.today,
        );
        p.settle(&mut env);
        let result = p.apply(&mut env, action);
        self.deltas.append(&mut env.deltas);
        result
    }

    /// Settle only (a new day for the captain).
    pub fn settle(&mut self, p: &mut Player) -> Vec<Event> {
        let mut env = Env::new(
            &self.content,
            &self.frontier,
            &mut self.world,
            &mut self.rng,
            self.today,
        );
        p.settle(&mut env)
    }

    pub fn next_day(&mut self) {
        self.today += 1;
        self.world
            .tick_day(&self.content, &self.frontier, self.today, &mut self.rng);
    }

    /// The first station whose economy is `economy` (optionally also with a shipyard).
    pub fn station(&self, economy: &str) -> u16 {
        self.stations(economy)[0]
    }

    pub fn stations(&self, economy: &str) -> Vec<u16> {
        let ix = self.content.economy_ix(economy).unwrap();
        self.frontier
            .stations
            .iter()
            .filter(|s| usize::from(s.economy) == ix)
            .map(|s| s.id)
            .collect()
    }

    /// Put the captain in orbit at a station's system, undocked.
    pub fn orbit(&self, p: &mut Player, station: u16) {
        p.at = self.frontier.station(station).unwrap().system;
        p.docked = None;
    }

    /// Put the captain docked at a station, with no fee, inspection or day charged.
    pub fn dock_free(&self, p: &mut Player, station: u16) {
        p.at = self.frontier.station(station).unwrap().system;
        p.docked = Some(station);
    }

    pub fn good(&self, id: &str) -> u8 {
        u8::try_from(self.content.good_ix(id).unwrap()).unwrap()
    }

    pub fn module(&self, id: &str) -> u8 {
        u8::try_from(self.content.module_ix(id).unwrap()).unwrap()
    }

    pub fn set_level(
        &self,
        p: &mut Player,
        skill: russh_bbs_door_long_haul::skills::Skill,
        level: u8,
    ) {
        p.xp[skill.index()] = russh_bbs_door_long_haul::skills::xp_for(level);
    }

    /// The slot of a good at a station, if it trades it.
    pub fn slot_of(&self, station: u16, good: &str) -> Option<u8> {
        let g = self.good(good);
        self.frontier
            .station(station)
            .unwrap()
            .goods
            .iter()
            .position(|m| m.good == g)
            .map(|i| u8::try_from(i).unwrap())
    }
}

/// A crew member of a role and skill with a plain goal that nothing in a test will meet by
/// accident (dock at 30 new stations).
pub fn member(role: Role, skill: u8) -> CrewMember {
    CrewMember {
        name: format!("{} Test", role.name()),
        role,
        skill,
        uses: 0,
        wage: 60,
        mood: [0; 4],
        loyalty: 50,
        goal: Goal {
            kind: GoalKind::Docks,
            target: 30,
            progress: 0,
            text: 0,
        },
        sworn: false,
        hurt_days: 0,
        scars: 0,
    }
}

pub fn orders(weapons: u8, shields: u8, engines: u8) -> Orders {
    Orders {
        weapons,
        shields,
        engines,
        maneuver: Maneuver::Hold,
    }
}

impl Harness {
    /// Run `f` with an engine environment on this harness.
    pub fn with_env<R>(&mut self, f: impl FnOnce(&mut Env<'_>) -> R) -> R {
        let mut env = Env::new(
            &self.content,
            &self.frontier,
            &mut self.world,
            &mut self.rng,
            self.today,
        );
        let out = f(&mut env);
        self.deltas.append(&mut env.deltas);
        out
    }

    /// Open a fight with the enemy whose id is `enemy`.
    pub fn fight(&mut self, p: &mut Player, enemy: &str) -> Vec<Event> {
        let ix = self
            .content
            .enemies
            .enemy
            .iter()
            .position(|e| e.id == enemy)
            .unwrap();
        let mut events = Vec::new();
        self.with_env(|env| p.start_combat(env, u8::try_from(ix).unwrap(), &mut events));
        events
    }

    /// Open a boarding of the layout whose id is `layout`.
    pub fn board(&mut self, p: &mut Player, layout: &str) -> Vec<Event> {
        let mut events = Vec::new();
        self.with_env(|env| p.start_boarding(env, layout, &mut events));
        events
    }

    /// Open the transit event whose id is `event` as the captain's scene.
    pub fn open_event(&self, p: &mut Player, event: &str) {
        let ix = self.content.transit.event_ix(event).unwrap();
        p.scene = Some(Scene::Event(u16::try_from(ix).unwrap()));
    }

    /// Fit a module by id (no slot check; a test sets up what it needs).
    pub fn fit(&self, p: &mut Player, module: &str) {
        p.ship
            .modules
            .push(russh_bbs_door_long_haul::ship::Installed {
                module: self.module(module),
                wear: 0,
            });
    }

    /// Put a crew member of `role` aboard, at the front of nothing in particular.
    pub fn sign(&self, p: &mut Player, role: Role, skill: u8) -> usize {
        p.crew.push(member(role, skill));
        p.crew.len() - 1
    }

    /// The index of an enemy by id.
    pub fn enemy_ix(&self, id: &str) -> usize {
        self.content
            .enemies
            .enemy
            .iter()
            .position(|e| e.id == id)
            .unwrap()
    }
}

impl Harness {
    /// Play any open scene to its end with a plain policy. Returns every event produced.
    /// Panics if a scene refuses to end (a bug the caller wants to see).
    pub fn finish_scene(&mut self, p: &mut Player) -> Vec<Event> {
        use russh_bbs_door_long_haul::boarding::Drill;
        use russh_bbs_door_long_haul::combat::Phase;
        let mut all = Vec::new();
        for step in 0..400u32 {
            let Some(scene) = p.scene.clone() else {
                return all;
            };
            let action = match &scene {
                Scene::Event(_) => {
                    // The first option this captain may take.
                    let mut done = false;
                    for n in 0..5u8 {
                        if let Ok(mut ev) = self.go_strict(p, &Action::Choose(n)) {
                            all.append(&mut ev);
                            done = true;
                            break;
                        }
                    }
                    assert!(done, "an event with no open option");
                    continue;
                }
                Scene::Combat(c) => {
                    if c.phase == Phase::Won {
                        Action::Wreck {
                            board: step % 2 == 0,
                        }
                    } else if c.round >= 8 {
                        Action::Run
                    } else {
                        Action::Fight(orders(3, 2, 1))
                    }
                }
                Scene::Boarding(b) => {
                    let def = b.def(&self.content);
                    let room = b.room(&self.content);
                    let foes = b.foes_here(&self.content);
                    let bit = |r: u8| 1u16 << u32::from(r);
                    if foes > 0 {
                        Action::Skirmish(Drill::Fight)
                    } else if b.looted & bit(b.at) == 0
                        && (room.credits[1] > 0 || room.tons > 0 || room.parts > 0)
                    {
                        Action::Skirmish(Drill::Take)
                    } else if room.cache > 0 && b.read & bit(b.at) == 0 {
                        Action::Skirmish(Drill::Search)
                    } else if let Some(&to) = room.exits.iter().find(|&&e| {
                        let t = &def.rooms[usize::from(e)];
                        b.looted & bit(e) == 0
                            && (t.lock == 0 || b.opened & bit(e) != 0)
                            && (t.credits[1] > 0 || t.tons > 0 || t.parts > 0)
                    }) {
                        Action::Skirmish(Drill::Move(to))
                    } else if let Some(&to) = room.exits.iter().find(|&&e| {
                        let t = &def.rooms[usize::from(e)];
                        t.lock > 0 && b.opened & bit(e) == 0
                    }) {
                        Action::Skirmish(Drill::Breach(to))
                    } else {
                        Action::Skirmish(Drill::Withdraw)
                    }
                }
            };
            match self.go_strict(p, &action) {
                Ok(mut ev) => all.append(&mut ev),
                Err(_) => {
                    // A refused plain move: pull out (or run) so the scene ends.
                    let out = match &scene {
                        Scene::Event(_) => unreachable!(),
                        Scene::Combat(_) => Action::Run,
                        Scene::Boarding(_) => Action::Skirmish(Drill::Withdraw),
                    };
                    if let Ok(mut ev) = self.go_strict(p, &out) {
                        all.append(&mut ev);
                    }
                }
            }
        }
        panic!("a scene did not end in 400 steps: {:?}", p.scene);
    }
}
