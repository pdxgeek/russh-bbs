//! The player's save, format version 2 (version 1 is the same bytes without the trailing town
//! section; [`upgrade_v1`] appends an empty one). Everything the game needs to continue exactly where the
//! hero stopped, the fight included (disconnecting is not an escape) and the hero's own random
//! generator (reopening is not a re-roll). The shared world is a separate blob (`world`).

use russh_bbs_door::DoorRng;

use crate::codec::{Reader, Writer};
use crate::combat::{Enemy, Fight, Stance};
use crate::content::{Content, Intent, Row};
use crate::game::{Game, Place, Today};
use crate::hero::{Bond, Hero, Item, Kit, Taken, derive};
use crate::rules::{CORES_MAX, DEPTH_MAX, FOUND_MAX, PACK_MAX};
use crate::world::World;

/// The save format version this code writes.
pub const SAVE_VERSION: u32 = 2;

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
        w.u32(h.stash);
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
        write_town(&mut w, self);
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
                depth: read_depth(&mut r)?,
                room: read_room(&mut r)?,
            },
            TAG_DEAD => Place::Dead {
                depth: read_depth(&mut r)?,
                room: read_room(&mut r)?,
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
        let prev_room = read_room(&mut r)?;
        let mut today = read_today(&mut r)?;
        let mut hero = hero;
        read_town(&mut r, &mut hero, &mut today)?;
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

const LIST_MAX: usize = 40;

fn write_kit(w: &mut Writer, kit: &Kit) {
    write_list(w, &kit.affixes);
    w.bool(kit.core.is_some());
    if let Some(core) = &kit.core {
        w.str(core);
    }
    w.u8(kit.wear);
}

fn read_kit(r: &mut Reader<'_>) -> Option<Kit> {
    let affixes = read_list(r)?;
    let core = if r.bool()? { Some(r.str()?) } else { None };
    let wear = r.u8()?;
    (affixes.len() <= 3 && wear <= 100).then_some(Kit {
        affixes,
        core,
        wear,
    })
}

fn write_items(w: &mut Writer, items: &[Item]) {
    w.count(items.len());
    for it in items {
        w.str(&it.base);
        write_kit(w, &it.kit);
    }
}

fn read_items(r: &mut Reader<'_>, max: usize) -> Option<Vec<Item>> {
    let n = r.count()?;
    if n > max {
        return None;
    }
    (0..n)
        .map(|_| {
            Some(Item {
                base: r.str()?,
                kit: read_kit(r)?,
            })
        })
        .collect()
}

/// The format-2 section: everything the town slice added, after the format-1 bytes.
fn write_town(w: &mut Writer, g: &Game) {
    let h = &g.hero;
    write_kit(w, &h.wkit);
    write_kit(w, &h.akit);
    write_items(w, &h.pack);
    write_items(w, &h.found);
    write_list(w, &h.cores);
    write_list(w, &h.recipes);
    w.u32(h.debt);
    w.bool(h.bond.is_some());
    if let Some(b) = &h.bond {
        w.str(&b.id);
        w.u8(b.level);
        w.u8(b.progress);
        w.i64(b.last);
    }
    write_list(w, &h.closed);
    w.count(h.given.len());
    for (project, coin, salvage) in &h.given {
        w.str(project);
        w.u32(*coin);
        w.u32(*salvage);
    }
    w.bool(h.contract.is_some());
    if let Some(c) = &h.contract {
        w.str(&c.id);
        w.i64(c.day);
        w.bool(c.done);
    }
    w.u16(h.contracts_done);
    let t = &g.today;
    w.u8(t.drinks);
    w.u8(t.gambles);
    w.u8(t.trained);
    w.bool(t.courted);
    w.count(t.bought.len());
    for b in &t.bought {
        w.u8(*b);
    }
    w.count(t.tips.len());
    for b in &t.tips {
        w.u8(*b);
    }
    w.bool(t.shut_out);
}

fn read_town(r: &mut Reader<'_>, h: &mut Hero, today: &mut Today) -> Option<()> {
    h.wkit = read_kit(r)?;
    h.akit = read_kit(r)?;
    h.pack = read_items(r, PACK_MAX)?;
    h.found = read_items(r, FOUND_MAX)?;
    h.cores = read_list(r)?;
    h.recipes = read_list(r)?;
    if h.cores.len() > CORES_MAX || h.recipes.len() > LIST_MAX {
        return None;
    }
    h.debt = r.u32()?;
    h.bond = if r.bool()? {
        let bond = Bond {
            id: r.str()?,
            level: r.u8()?,
            progress: r.u8()?,
            last: r.i64()?,
        };
        (bond.level <= 3).then_some(bond)
    } else {
        None
    };
    h.closed = read_list(r)?;
    let n = r.count()?;
    if n > LIST_MAX {
        return None;
    }
    h.given = (0..n)
        .map(|_| Some((r.str()?, r.u32()?, r.u32()?)))
        .collect::<Option<Vec<_>>>()?;
    h.contract = if r.bool()? {
        Some(Taken {
            id: r.str()?,
            day: r.i64()?,
            done: r.bool()?,
        })
    } else {
        None
    };
    h.contracts_done = r.u16()?;
    today.drinks = r.u8()?;
    today.gambles = r.u8()?;
    today.trained = r.u8()?;
    today.courted = r.bool()?;
    let n = r.count()?;
    today.bought = (0..n).map(|_| r.u8()).collect::<Option<Vec<_>>>()?;
    let n = r.count()?;
    today.tips = (0..n).map(|_| r.u8()).collect::<Option<Vec<_>>>()?;
    today.shut_out = r.bool()?;
    Some(())
}

/// Bring a format-1 save up to format 2: the same hero with an empty town section.
pub(crate) fn upgrade_v1(bytes: &[u8]) -> Option<Vec<u8>> {
    // Parse the old bytes by decoding a copy that has the empty section appended; a v1 save
    // that was damaged does not decode and is refused (never guessed at).
    let mut out = bytes.to_vec();
    let empty = Game::empty_town_section();
    out.extend_from_slice(&empty);
    Game::decode(&out).map(|_| out)
}

impl Game {
    /// The bytes of a town section for a hero with no gear marks and nothing in the town.
    fn empty_town_section() -> Vec<u8> {
        let mut w = Writer::default();
        write_kit(&mut w, &Kit::default());
        write_kit(&mut w, &Kit::default());
        w.count(0); // pack
        w.count(0); // found
        w.count(0); // cores
        w.count(0); // recipes
        w.u32(0); // debt
        w.bool(false); // bond
        w.count(0); // closed
        w.count(0); // given
        w.bool(false); // contract
        w.u16(0); // contracts done
        w.u8(0); // drinks
        w.u8(0); // gambles
        w.u8(0); // trained
        w.bool(false); // courted
        w.count(0); // bought
        w.count(0); // tips
        w.bool(false); // shut out
        w.0
    }
}

/// A level number, 1 to the bottom: a save can never put a hero where there is no level.
fn read_depth(r: &mut Reader<'_>) -> Option<u8> {
    r.u8().filter(|d| (1..=DEPTH_MAX).contains(d))
}

/// A room index; a level has at most 20 rooms and the explored map is a 32-bit mask.
fn read_room(r: &mut Reader<'_>) -> Option<u8> {
    r.u8().filter(|room| *room < 32)
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
    let stash = r.u32()?;
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
        stash,
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
        wkit: Kit::default(),
        akit: Kit::default(),
        pack: Vec::new(),
        found: Vec::new(),
        cores: Vec::new(),
        recipes: Vec::new(),
        debt: 0,
        bond: None,
        closed: Vec::new(),
        given: Vec::new(),
        contract: None,
        contracts_done: 0,
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
    /// Salvage carried.
    pub salvage: u32,
    /// Salvage banked in town.
    pub stash: u32,
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
        stash: h.stash,
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
