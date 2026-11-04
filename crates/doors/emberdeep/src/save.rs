//! The player's save, format version 1. Everything the game needs to continue exactly where the
//! hero stopped, the fight included (disconnecting is not an escape) and the hero's own random
//! generator (reopening is not a re-roll). The shared world is a separate blob (`world`).

use russh_bbs_door::DoorRng;

use crate::codec::{Reader, Writer};
use crate::combat::{Enemy, Fight, Stance};
use crate::content::{Content, Intent, Row};
use crate::game::{Game, Place, Today};
use crate::hero::{Hero, derive};
use crate::rules::DEPTH_MAX;
use crate::world::World;

/// The save format version this code writes.
pub const SAVE_VERSION: u32 = 1;

const TAG_TOWN: u8 = 0;
const TAG_DELVE: u8 = 1;
const TAG_DEAD: u8 = 2;

fn intent_code(i: Intent) -> u8 {
    match i {
        Intent::Strike => 0,
        Intent::Heavy => 1,
        Intent::Shoot => 2,
        Intent::Brace => 3,
        Intent::Rally => 4,
        Intent::Advance => 5,
        Intent::Breathe => 6,
    }
}

fn intent_from(code: u8) -> Option<Intent> {
    Some(match code {
        0 => Intent::Strike,
        1 => Intent::Heavy,
        2 => Intent::Shoot,
        3 => Intent::Brace,
        4 => Intent::Rally,
        5 => Intent::Advance,
        6 => Intent::Breathe,
        _ => return None,
    })
}

fn row_code(r: Row) -> u8 {
    match r {
        Row::Front => 0,
        Row::Back => 1,
    }
}

fn row_from(code: u8) -> Option<Row> {
    match code {
        0 => Some(Row::Front),
        1 => Some(Row::Back),
        _ => None,
    }
}

fn stance_code(s: Stance) -> u8 {
    match s {
        Stance::Aggressive => 0,
        Stance::Guarded => 1,
        Stance::Evasive => 2,
    }
}

fn stance_from(code: u8) -> Option<Stance> {
    match code {
        0 => Some(Stance::Aggressive),
        1 => Some(Stance::Guarded),
        2 => Some(Stance::Evasive),
        _ => None,
    }
}

fn write_list(w: &mut Writer, list: &[String]) {
    w.count(list.len());
    for s in list {
        w.str(s);
    }
}

fn read_list(r: &mut Reader<'_>) -> Option<Vec<String>> {
    let n = r.count()?;
    (0..n).map(|_| r.str()).collect()
}

impl Game {
    /// The save bytes.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        let h = &self.hero;
        w.str(&h.name);
        w.str(&h.calling);
        for a in h.attrs {
            w.u8(a);
        }
        w.u8(h.level);
        w.u32(h.xp);
        w.u8(h.skill_points);
        w.u8(h.attr_points);
        w.i32(h.hp);
        w.u8(h.fatigue);
        w.u32(h.turns);
        w.u32(h.coin);
        w.u32(h.salvage);
        w.u16(h.relics);
        write_list(&mut w, &h.nodes);
        write_list(&mut w, &h.traits);
        write_list(&mut w, &h.scars);
        w.count(h.wounds.len());
        for (id, days) in &h.wounds {
            w.str(id);
            w.u8(*days);
        }
        w.str(&h.weapon);
        w.str(&h.armour);
        w.u16(h.deaths);
        w.u8(h.best_depth);
        w.u32(h.kills);
        for word in self.rng.state() {
            w.u64(word);
        }
        match &self.place {
            Place::Town => w.u8(TAG_TOWN),
            Place::Delve { depth, room } => {
                w.u8(TAG_DELVE);
                w.u8(*depth);
                w.u8(*room);
            }
            Place::Dead { depth, room, since } => {
                w.u8(TAG_DEAD);
                w.u8(*depth);
                w.u8(*room);
                w.i64(*since);
            }
        }
        w.bool(self.fight.is_some());
        if let Some(f) = &self.fight {
            w.count(f.enemies.len());
            for e in &f.enemies {
                w.str(&e.id);
                w.i32(e.hp);
                w.u8(row_code(e.row));
                w.u8(intent_code(e.intent));
                w.bool(e.engaged);
                w.u8(e.mark_pct);
                w.u8(e.sunder);
                w.u8(e.bleed);
                w.bool(e.rallied);
                w.bool(e.interrupted);
            }
            w.u8(stance_code(f.stance));
            w.u8(row_code(f.row));
            w.u8(f.focus);
            w.i32(f.ward);
            w.u8(f.round);
            w.bool(f.guardian);
            w.bool(f.paid);
        }
        w.i64(self.day);
        w.u16(self.epoch);
        w.u32(self.season);
        for mask in &self.explored {
            w.u32(*mask);
        }
        w.u8(self.prev_room);
        w.count(self.today.spent.len());
        for (a, b, c) in &self.today.spent {
            w.u8(*a);
            w.u8(*b);
            w.u8(*c);
        }
        w.count(self.today.cleared.len());
        for (a, b) in &self.today.cleared {
            w.u8(*a);
            w.u8(*b);
        }
        w.count(self.today.traps.len());
        for (a, b, c) in &self.today.traps {
            w.u8(*a);
            w.u8(*b);
            w.u8(*c);
        }
        w.0
    }

    /// Read a save, or `None` if the bytes are not one.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        let hero = read_hero(&mut r)?;
        let state = [r.u64()?, r.u64()?, r.u64()?, r.u64()?];
        let place = match r.u8()? {
            TAG_TOWN => Place::Town,
            TAG_DELVE => Place::Delve {
                depth: r.u8()?,
                room: r.u8()?,
            },
            TAG_DEAD => Place::Dead {
                depth: r.u8()?,
                room: r.u8()?,
                since: r.i64()?,
            },
            _ => return None,
        };
        let fight = if r.bool()? {
            Some(read_fight(&mut r)?)
        } else {
            None
        };
        let day = r.i64()?;
        let epoch = r.u16()?;
        let season = r.u32()?;
        let mut explored = Vec::with_capacity(usize::from(DEPTH_MAX));
        for _ in 0..DEPTH_MAX {
            explored.push(r.u32()?);
        }
        let prev_room = r.u8()?;
        let today = read_today(&mut r)?;
        if !r.done() {
            return None;
        }
        Some(Self {
            hero,
            rng: DoorRng::from_state(state),
            place,
            fight,
            day,
            epoch,
            season,
            explored,
            today,
            prev_room,
            world: World::new(season),
            changes: Vec::new(),
        })
    }
}

fn read_hero(r: &mut Reader<'_>) -> Option<Hero> {
    let name = r.str()?;
    let calling = r.str()?;
    let mut attrs = [0u8; 4];
    for a in &mut attrs {
        *a = r.u8()?;
    }
    let level = r.u8()?;
    let xp = r.u32()?;
    let skill_points = r.u8()?;
    let attr_points = r.u8()?;
    let hp = r.i32()?;
    let fatigue = r.u8()?;
    let turns = r.u32()?;
    let coin = r.u32()?;
    let salvage = r.u32()?;
    let relics = r.u16()?;
    let nodes = read_list(r)?;
    let traits = read_list(r)?;
    let scars = read_list(r)?;
    let mut wounds = Vec::new();
    for _ in 0..r.count()? {
        let id = r.str()?;
        wounds.push((id, r.u8()?));
    }
    Some(Hero {
        name,
        calling,
        attrs,
        level,
        xp,
        skill_points,
        attr_points,
        hp,
        fatigue,
        turns,
        coin,
        salvage,
        relics,
        nodes,
        traits,
        scars,
        wounds,
        weapon: r.str()?,
        armour: r.str()?,
        deaths: r.u16()?,
        best_depth: r.u8()?,
        kills: r.u32()?,
    })
}

fn read_fight(r: &mut Reader<'_>) -> Option<Fight> {
    let mut enemies = Vec::new();
    for _ in 0..r.count()? {
        enemies.push(Enemy {
            id: r.str()?,
            hp: r.i32()?,
            row: row_from(r.u8()?)?,
            intent: intent_from(r.u8()?)?,
            engaged: r.bool()?,
            mark_pct: r.u8()?,
            sunder: r.u8()?,
            bleed: r.u8()?,
            rallied: r.bool()?,
            interrupted: r.bool()?,
        });
    }
    Some(Fight {
        enemies,
        stance: stance_from(r.u8()?)?,
        row: row_from(r.u8()?)?,
        focus: r.u8()?,
        ward: r.i32()?,
        round: r.u8()?,
        guardian: r.bool()?,
        paid: r.bool()?,
    })
}

fn read_today(r: &mut Reader<'_>) -> Option<Today> {
    let mut today = Today::default();
    for _ in 0..r.count()? {
        today.spent.push((r.u8()?, r.u8()?, r.u8()?));
    }
    for _ in 0..r.count()? {
        today.cleared.push((r.u8()?, r.u8()?));
    }
    for _ in 0..r.count()? {
        today.traps.push((r.u8()?, r.u8()?, r.u8()?));
    }
    Some(today)
}

/// Where a saved hero is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Whereabouts {
    /// In Cindermere.
    Town,
    /// Below, on this level.
    Below(u8),
    /// Dead, on this level, waiting to be found.
    Dead(u8),
}

/// A saved hero in a few numbers, for tests, the balance harness and the staff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The hero's name.
    pub name: String,
    /// The calling id.
    pub calling: String,
    /// Hero level.
    pub level: u8,
    /// Hit points now.
    pub hp: i32,
    /// Hit points at most.
    pub max_hp: i32,
    /// Where the hero is.
    pub whereabouts: Whereabouts,
    /// Coin held.
    pub coin: u32,
    /// Salvage held.
    pub salvage: u32,
    /// Torch-turns left today.
    pub turns: u32,
    /// Times the hero has died.
    pub deaths: u16,
    /// The deepest level reached.
    pub best_depth: u8,
    /// Monsters killed.
    pub kills: u32,
    /// Skill nodes taken.
    pub nodes: usize,
    /// Scars carried.
    pub scars: usize,
    /// Relics held.
    pub relics: u16,
    /// Whether a fight is in progress.
    pub in_fight: bool,
}

/// Read a save into a [`Summary`].
#[must_use]
pub fn summarize(bytes: &[u8], content: &Content) -> Option<Summary> {
    let game = Game::decode(bytes)?;
    let d = derive(&game.hero, content);
    let h = &game.hero;
    Some(Summary {
        name: h.name.clone(),
        calling: h.calling.clone(),
        level: h.level,
        hp: h.hp,
        max_hp: d.max_hp,
        whereabouts: match game.place {
            Place::Town => Whereabouts::Town,
            Place::Delve { depth, .. } => Whereabouts::Below(depth),
            Place::Dead { depth, .. } => Whereabouts::Dead(depth),
        },
        coin: h.coin,
        salvage: h.salvage,
        turns: h.turns,
        deaths: h.deaths,
        best_depth: h.best_depth,
        kills: h.kills,
        nodes: h.nodes.len(),
        scars: h.scars.len(),
        relics: h.relics,
        in_fight: game.fight.is_some(),
    })
}
