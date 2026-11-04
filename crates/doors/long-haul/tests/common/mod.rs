//! Shared setup for the engine tests: a frontier, a world, a seeded RNG and a day, and a few
//! ways to put a captain where a test needs one.
#![allow(dead_code, clippy::unwrap_used, clippy::panic, missing_docs)]

use russh_bbs_door::DoorRng;
use russh_bbs_door_long_haul::content::Content;
use russh_bbs_door_long_haul::engine::{Action, Env};
use russh_bbs_door_long_haul::events::{Event, Refusal};
use russh_bbs_door_long_haul::frontier::Frontier;
use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::world::{Delta, World};

pub const DAY0: i64 = 20_000;

pub struct Harness {
    pub content: Content,
    pub frontier: Frontier,
    pub world: World,
    pub rng: DoorRng,
    pub today: i64,
    pub deltas: Vec<Delta>,
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
        Player::new(&self.content, &self.frontier, "Tester", ix, self.today)
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
