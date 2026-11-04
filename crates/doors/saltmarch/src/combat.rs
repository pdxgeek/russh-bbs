//! Skirmish combat: short fights on a three-row board (front, middle, back) with initiative,
//! six actions (strike, brace, shove, throw, mend, flee) and one special per trade. Terrain and
//! tide matter: in knee-deep water everyone fights worse, the aquatic better, and a rising tide
//! puts a limit on the rounds. The fight is plain data (it is saved mid-fight) and every roll
//! comes from the `DoorRng` the caller passes.

use russh_bbs_door::DoorRng;

use crate::codec::{Reader, Writer};
use crate::content::Catalog;
use crate::crew::Crew;
use crate::lore::Behaviour;
use crate::types::{Attr, Trade};

/// Rounds after which a fight is a stalemate and both sides break off.
pub const MAX_ROUNDS: u8 = 14;

/// Rounds between uses of a special.
pub const SPECIAL_COOL: u8 = 3;

/// Rounds that make up one tide-turn of the clan's day.
pub const ROUNDS_PER_TURN: u8 = 4;

/// Water level from which the tidal foes are stronger.
pub const TIDAL_LEVEL: i32 = 60;

/// `ratio` from which a fight reads as favourable, even and against you.
pub const ODDS_EASY: i64 = 200;
/// See [`ODDS_EASY`].
pub const ODDS_FAIR: i64 = 125;
/// See [`ODDS_EASY`].
pub const ODDS_HARD: i64 = 95;

/// The most units on the board.
pub const MAX_UNITS: usize = 12;

/// Which side a unit is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The clan's party.
    Party,
    /// The foes.
    Foe,
}

/// One fighter.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // status flags, saved and read together
pub struct Unit {
    /// Which side.
    pub side: Side,
    /// The crew id (party units).
    pub crew: u16,
    /// The foe's id (foe units).
    pub foe: String,
    /// The name shown.
    pub name: String,
    /// The trade (party units), which decides the special.
    pub trade: Option<Trade>,
    /// Hit points now.
    pub hp: i16,
    /// Hit points at the start.
    pub max_hp: i16,
    /// Attack.
    pub atk: i16,
    /// Armour.
    pub def: i16,
    /// Speed.
    pub spd: i16,
    /// How hard to shove.
    pub mass: u8,
    /// 0 front, 1 middle, 2 back.
    pub row: u8,
    /// Carries arms: can reach one row deeper.
    pub armed: bool,
    /// Braced until its next turn.
    pub braced: bool,
    /// Rounds it loses its turns for.
    pub netted: u8,
    /// Rounds it is hexed for.
    pub hexed: u8,
    /// Rounds it is guarded for.
    pub guarded: u8,
    /// Dropped its guard (a cleave) until its next turn.
    pub exposed: bool,
    /// Rounds until its special is ready.
    pub cool: u8,
    /// Better in water.
    pub aquatic: bool,
    /// Stronger at high water.
    pub tidal: bool,
    /// How a foe fights.
    pub behaviour: Behaviour,
    /// Fled or swept away (neither alive nor downed).
    pub gone: bool,
}

impl Unit {
    /// Still on the board and standing.
    #[must_use]
    pub const fn alive(&self) -> bool {
        self.hp > 0 && !self.gone
    }

    /// Down: at zero hit points and not gone.
    #[must_use]
    pub const fn downed(&self) -> bool {
        self.hp <= 0 && !self.gone
    }

    fn blank(side: Side, name: String) -> Self {
        Self {
            side,
            crew: 0,
            foe: String::new(),
            name,
            trade: None,
            hp: 1,
            max_hp: 1,
            atk: 1,
            def: 0,
            spd: 1,
            mass: 5,
            row: 0,
            armed: false,
            braced: false,
            netted: 0,
            hexed: 0,
            guarded: 0,
            exposed: false,
            cool: 0,
            aquatic: false,
            tidal: false,
            behaviour: Behaviour::Bruiser,
            gone: false,
        }
    }

    /// A unit from numbers posted for a raid: the raiders on the party side and the home guard
    /// on the foe side (a guard fights as a bruiser: it holds its ground and hits what is in
    /// reach).
    #[must_use]
    pub fn posted(side: Side, d: &crate::social::Defender) -> Self {
        let mut u = Self::blank(side, d.name.clone());
        u.crew = d.id;
        u.trade = Some(d.trade);
        u.hp = d.hp.max(1);
        u.max_hp = u.hp;
        u.atk = d.atk.max(1);
        u.def = d.def.max(0);
        u.spd = d.spd.max(1);
        u.mass = d.mass.clamp(1, 10);
        u.row = d.row.min(2);
        u.aquatic = d.aquatic;
        u.armed = d.armed;
        u
    }

    /// A party unit from a crew member. `armed` is whether they carry arms from the kit.
    #[must_use]
    pub fn from_crew(cat: &Catalog, c: &Crew, armed: bool) -> Self {
        let grit = c.attr(Attr::Grit);
        let wit = c.attr(Attr::Wit);
        let reach = c.attr(Attr::Reach);
        let fighter = i32::from(c.level(Trade::Fighter));
        let smith = i32::from(c.level(Trade::Smith));
        let pen = i32::try_from(100 - c.penalty(cat)).unwrap_or(0);
        let hp = ((8 + 2 * grit + fighter + c.trait_sum(cat, |t| t.fight_hp)) * pen / 100).max(4);
        let atk = (2
            + grit / 2
            + fighter / 2
            + i32::from(c.trade == Trade::Fighter)
            + 2 * i32::from(armed)
            + c.trait_sum(cat, |t| t.fight_atk))
            * pen
            / 100;
        let def = i32::from(matches!(c.trade, Trade::Fighter | Trade::Smith))
            + smith / 3
            + c.trait_sum(cat, |t| t.fight_def);
        let spd = 2 + reach / 2 + wit / 3 + c.trait_sum(cat, |t| t.fight_spd);
        let row = match c.trade {
            Trade::Fighter | Trade::Smith | Trade::Boatwright => 0,
            Trade::Wader | Trade::Raker | Trade::Salvager => 1,
            Trade::Healer | Trade::ReedWitch => 2,
        };
        let mut u = Self::blank(Side::Party, c.name.clone());
        u.crew = c.id;
        u.trade = Some(c.trade);
        u.hp = narrow(hp);
        u.max_hp = narrow(hp);
        u.atk = narrow(atk.max(1));
        u.def = narrow(def.max(0));
        u.spd = narrow(spd.max(1));
        u.mass = u8::try_from(grit.clamp(1, 10)).unwrap_or(5);
        u.row = row;
        u.armed = armed;
        u.aquatic = c.trait_any(cat, |t| t.wet_pct > 0);
        u
    }

    fn write(&self, w: &mut Writer) {
        w.u8(u8::from(self.side == Side::Foe));
        w.u16(self.crew);
        w.str(&self.foe);
        w.str(&self.name);
        w.u8(self
            .trade
            .map_or(u8::MAX, |t| u8::try_from(t.index()).unwrap_or(0)));
        for v in [self.hp, self.max_hp, self.atk, self.def, self.spd] {
            w.u16(u16::from_le_bytes(v.to_le_bytes()));
        }
        w.u8(self.mass);
        w.u8(self.row);
        for b in [
            self.armed,
            self.braced,
            self.exposed,
            self.aquatic,
            self.tidal,
            self.gone,
        ] {
            w.bool(b);
        }
        w.u8(self.netted);
        w.u8(self.hexed);
        w.u8(self.guarded);
        w.u8(self.cool);
        w.u8(match self.behaviour {
            Behaviour::Bruiser => 0,
            Behaviour::Lurker => 1,
            Behaviour::Hurler => 2,
            Behaviour::Skittish => 3,
            Behaviour::Swarm => 4,
        });
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let side = if r.bool()? { Side::Foe } else { Side::Party };
        let crew = r.u16()?;
        let foe = r.str()?;
        let name = r.str()?;
        let trade = match r.u8()? {
            u8::MAX => None,
            i => Some(*Trade::ALL.get(usize::from(i))?),
        };
        let mut nums = [0i16; 5];
        for n in &mut nums {
            *n = i16::from_le_bytes(r.u16()?.to_le_bytes());
        }
        let (mass, row) = (r.u8()?, r.u8()?);
        let (armed, braced, exposed, aquatic, tidal, gone) = (
            r.bool()?,
            r.bool()?,
            r.bool()?,
            r.bool()?,
            r.bool()?,
            r.bool()?,
        );
        let (netted, hexed, guarded, cool) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
        let behaviour = match r.u8()? {
            0 => Behaviour::Bruiser,
            1 => Behaviour::Lurker,
            2 => Behaviour::Hurler,
            3 => Behaviour::Skittish,
            4 => Behaviour::Swarm,
            _ => return None,
        };
        (row <= 2).then_some(Self {
            side,
            crew,
            foe,
            name,
            trade,
            hp: nums[0],
            max_hp: nums[1],
            atk: nums[2],
            def: nums[3],
            spd: nums[4],
            mass,
            row,
            armed,
            braced,
            netted,
            hexed,
            guarded,
            exposed,
            cool,
            aquatic,
            tidal,
            behaviour,
            gone,
        })
    }
}

fn narrow(v: i32) -> i16 {
    i16::try_from(v).unwrap_or(i16::MAX)
}

/// What the party does on one unit's turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FightAction {
    /// Hit a foe hand to hand (front rank only, or one row deeper with arms).
    Strike {
        /// The unit index of the foe.
        target: u8,
    },
    /// Guard until the next turn.
    Brace,
    /// Push a foe back a row (or into the water).
    Shove {
        /// The unit index of the foe.
        target: u8,
    },
    /// Throw something at any foe.
    Throw {
        /// The unit index of the foe.
        target: u8,
    },
    /// Patch up an ally (or lift one who has fallen).
    Mend {
        /// The unit index of the ally.
        ally: u8,
    },
    /// The trade's special.
    Special {
        /// The unit index of the target, for specials that need one.
        target: Option<u8>,
    },
    /// Run for it, at a cost.
    Flee,
}

/// How a fight ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Every foe is down or gone.
    Won,
    /// Every one of the party is down.
    Lost,
    /// The party ran.
    Fled,
    /// The rising water ended it.
    Washed,
    /// Neither side could finish it.
    Stalemate,
}

/// Where the fight stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// It is this unit's turn and the player must choose.
    Turn(usize),
    /// It is over.
    Over(Verdict),
}

/// A rough read on a fight before it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Odds {
    /// The party should win without much trouble.
    Easy,
    /// A fair fight.
    Fair,
    /// The party is the underdog.
    Hard,
    /// The party is outmatched.
    Grim,
}

impl Odds {
    /// A word for the screen.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Easy => "easy",
            Self::Fair => "you should win",
            Self::Hard => "a toss-up",
            Self::Grim => "grim",
        }
    }
}

/// A fight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fight {
    /// The group's id.
    pub group: String,
    /// Everyone on the board.
    pub units: Vec<Unit>,
    /// The round now (0 before the first).
    pub round: u8,
    /// This round's turn order (unit indices).
    pub order: Vec<u8>,
    /// Whose turn in the order.
    pub cursor: u8,
    /// Knee-deep: everyone but the aquatic fights worse.
    pub wet: bool,
    /// The water level (the tidal foes read it).
    pub level: i32,
    /// The round at which the water ends the fight (0: it does not).
    pub limit: u8,
    /// Added to the chance to flee (negative when overloaded).
    pub flee_mod: i16,
}

impl Fight {
    /// A fight between `party` and the foes of `group`.
    #[must_use]
    pub fn begin(
        cat: &Catalog,
        party: Vec<Unit>,
        group: &str,
        wet: bool,
        level: i32,
        limit: u8,
        flee_mod: i16,
    ) -> Self {
        let mut units = party;
        if let Some(g) = cat.group(group) {
            for id in &g.foes {
                let Some(def) = cat.foe(id) else { continue };
                let mut u = Unit::blank(Side::Foe, def.name.clone());
                u.foe.clone_from(&def.id);
                u.hp = i16::from(def.hp);
                u.max_hp = i16::from(def.hp);
                u.atk = i16::from(def.atk);
                u.def = i16::from(def.def);
                u.spd = i16::from(def.spd);
                u.mass = def.mass;
                u.row = def.row.row();
                u.aquatic = def.aquatic;
                u.tidal = def.tidal;
                u.behaviour = def.behaviour;
                units.push(u);
            }
        }
        units.truncate(MAX_UNITS);
        let mut f = Self {
            group: group.to_owned(),
            units,
            round: 0,
            order: Vec::new(),
            cursor: 0,
            wet,
            level,
            limit,
            flee_mod,
        };
        f.compact();
        f
    }

    /// The indices of the living on a side.
    pub fn alive(&self, side: Side) -> impl Iterator<Item = usize> + '_ {
        self.units
            .iter()
            .enumerate()
            .filter(move |(_, u)| u.side == side && u.alive())
            .map(|(i, _)| i)
    }

    /// The front-most occupied row of a side.
    #[must_use]
    pub fn front(&self, side: Side) -> Option<u8> {
        self.alive(side).map(|i| self.units[i].row).min()
    }

    /// Whether a unit can fight hand to hand: it stands in its side's front rank, or one row
    /// behind it with arms.
    #[must_use]
    pub fn in_reach(&self, idx: usize) -> bool {
        let u = &self.units[idx];
        self.front(u.side)
            .is_some_and(|f| u.row == f || (u.armed && u.row == f + 1))
    }

    /// The foes (or allies' foes) that unit `idx` can hit hand to hand.
    #[must_use]
    pub fn melee_targets(&self, idx: usize) -> Vec<usize> {
        let u = &self.units[idx];
        let other = if u.side == Side::Party {
            Side::Foe
        } else {
            Side::Party
        };
        let Some(front) = self.front(other) else {
            return Vec::new();
        };
        let reach = u8::from(u.armed);
        self.alive(other)
            .filter(|&i| self.units[i].row <= front + reach)
            .collect()
    }

    /// Close ranks: rows with nobody in them are squeezed out, front first.
    fn compact(&mut self) {
        for side in [Side::Party, Side::Foe] {
            let mut rows: Vec<u8> = self.alive(side).map(|i| self.units[i].row).collect();
            rows.sort_unstable();
            rows.dedup();
            for u in self
                .units
                .iter_mut()
                .filter(|u| u.side == side && u.alive())
            {
                u.row =
                    u8::try_from(rows.iter().position(|r| *r == u.row).unwrap_or(0)).unwrap_or(0);
            }
        }
    }

    /// Whether either side is finished.
    fn verdict(&self) -> Option<Verdict> {
        if self.alive(Side::Foe).next().is_none() {
            Some(Verdict::Won)
        } else if self.alive(Side::Party).next().is_none() {
            Some(Verdict::Lost)
        } else {
            None
        }
    }

    fn atk_of(&self, u: &Unit) -> i16 {
        let mut atk = u.atk;
        if u.hexed > 0 {
            atk -= 2;
        }
        if self.wet {
            atk += if u.aquatic { 1 } else { -1 };
        }
        if u.tidal && self.level >= TIDAL_LEVEL {
            atk += 2;
        }
        atk.max(1)
    }

    fn def_of(&self, u: &Unit) -> i16 {
        let mut def = u.def;
        if u.braced {
            def += 3;
        }
        if u.guarded > 0 {
            def += 2;
        }
        if u.hexed > 0 {
            def -= 1;
        }
        if u.exposed {
            def -= 2;
        }
        if self.wet && u.aquatic {
            def += 1;
        }
        def.max(0)
    }

    /// Roll an attack by `from` on `to`: true damage dealt (0 on a miss). `ignore` skips armour.
    fn hit(
        &mut self,
        from: usize,
        to: usize,
        bonus: i16,
        ignore: bool,
        rng: &mut DoorRng,
        lines: &mut Vec<String>,
    ) -> i16 {
        let miss = 10
            + if self.wet && !self.units[from].aquatic {
                10
            } else {
                0
            };
        let (a, t) = (&self.units[from], &self.units[to]);
        if rng.chance(miss, 100) {
            lines.push(format!("{} swings at {} and misses.", a.name, t.name));
            return 0;
        }
        let atk = self.atk_of(a) + bonus;
        let armour = if ignore { 0 } else { self.def_of(t) };
        let roll = i16::try_from(rng.below(3)).unwrap_or(0);
        let dmg = (atk + roll - armour).max(1);
        let (an, tn) = (a.name.clone(), t.name.clone());
        let target = &mut self.units[to];
        target.hp -= dmg;
        let down = target.hp <= 0;
        lines.push(format!("{an} hits {tn} for {dmg}."));
        if down {
            lines.push(format!(
                "{tn} {}.",
                if self.units[to].side == Side::Party {
                    "goes down"
                } else {
                    "falls"
                }
            ));
        }
        dmg
    }

    fn start_round(&mut self, rng: &mut DoorRng) {
        self.round += 1;
        for u in &mut self.units {
            u.hexed = u.hexed.saturating_sub(1);
            u.guarded = u.guarded.saturating_sub(1);
            u.cool = u.cool.saturating_sub(1);
        }
        self.compact();
        let mut keyed: Vec<(i16, usize)> = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.alive())
            .map(|(i, u)| {
                let wet = i16::from(self.wet && !u.aquatic);
                let init = u.spd - wet + i16::try_from(rng.below(3)).unwrap_or(0);
                (init, i)
            })
            .collect();
        keyed.sort_by_key(|&(init, i)| (std::cmp::Reverse(init), i));
        self.order = keyed
            .into_iter()
            .map(|(_, i)| u8::try_from(i).unwrap_or(0))
            .collect();
        self.cursor = 0;
    }

    /// Run foe turns and round changes until a party unit must choose, or the fight is over.
    pub fn advance(&mut self, rng: &mut DoorRng, lines: &mut Vec<String>) -> Progress {
        loop {
            if let Some(v) = self.verdict() {
                return Progress::Over(v);
            }
            if usize::from(self.cursor) >= self.order.len() {
                if self.round >= MAX_ROUNDS {
                    return Progress::Over(Verdict::Stalemate);
                }
                if self.limit > 0 && self.round >= self.limit {
                    return Progress::Over(Verdict::Washed);
                }
                self.start_round(rng);
                continue;
            }
            let idx = usize::from(self.order[usize::from(self.cursor)]);
            if !self.units[idx].alive() {
                self.cursor += 1;
                continue;
            }
            let side = self.units[idx].side;
            self.units[idx].braced = false;
            self.units[idx].exposed = false;
            if side == Side::Foe {
                self.foe_turn(idx, rng, lines);
                self.cursor += 1;
                continue;
            }
            if self.units[idx].netted > 0 {
                self.units[idx].netted -= 1;
                lines.push(format!(
                    "{} is tangled and loses the turn.",
                    self.units[idx].name
                ));
                self.cursor += 1;
                continue;
            }
            return Progress::Turn(idx);
        }
    }

    fn foe_turn(&mut self, idx: usize, rng: &mut DoorRng, lines: &mut Vec<String>) {
        if self.units[idx].netted > 0 {
            self.units[idx].netted -= 1;
            lines.push(format!(
                "{} is tangled and loses the turn.",
                self.units[idx].name
            ));
            return;
        }
        let u = &self.units[idx];
        if u.behaviour == Behaviour::Skittish && i32::from(u.hp) * 100 / i32::from(u.max_hp) < 35 {
            lines.push(format!("{} bolts.", u.name));
            self.units[idx].gone = true;
            return;
        }
        let party: Vec<usize> = self.alive(Side::Party).collect();
        let Some(&weakest) = party.iter().min_by_key(|&&i| (self.units[i].hp, i)) else {
            return;
        };
        match self.units[idx].behaviour {
            Behaviour::Hurler => {
                let name = self.units[idx].name.clone();
                let tn = self.units[weakest].name.clone();
                if rng.chance(15, 100) {
                    lines.push(format!("{name} throws wide of {tn}."));
                    return;
                }
                let roll = i16::try_from(rng.below(3)).unwrap_or(0);
                let dmg = (self.atk_of(&self.units[idx]) - 1 + roll
                    - self.def_of(&self.units[weakest]))
                .max(1);
                self.units[weakest].hp -= dmg;
                lines.push(format!("{name} slings at {tn} for {dmg}."));
                if self.units[weakest].hp <= 0 {
                    lines.push(format!("{tn} goes down."));
                }
            }
            Behaviour::Lurker => {
                self.hit(idx, weakest, 0, false, rng, lines);
            }
            Behaviour::Bruiser | Behaviour::Skittish | Behaviour::Swarm => {
                if self.in_reach(idx) {
                    let targets = self.melee_targets(idx);
                    if targets.is_empty() {
                        return;
                    }
                    let pick = targets[usize::try_from(
                        rng.below(u32::try_from(targets.len()).unwrap_or(1)),
                    )
                    .unwrap_or(0)];
                    self.hit(idx, pick, 0, false, rng, lines);
                } else {
                    self.units[idx].row = self.units[idx].row.saturating_sub(1);
                    lines.push(format!("{} presses forward.", self.units[idx].name));
                }
            }
        }
    }

    /// The foe a special should go after, by what the party can see: the strongest unaffected.
    fn threat(&self, skip: impl Fn(&Unit) -> bool) -> Option<usize> {
        self.alive(Side::Foe)
            .filter(|&i| !skip(&self.units[i]))
            .max_by_key(|&i| (self.units[i].atk, std::cmp::Reverse(i)))
    }

    /// What a quick-resolve does on this unit's turn: a fixed policy, so results follow the odds.
    #[must_use]
    pub fn auto(&self, idx: usize) -> FightAction {
        let me = &self.units[idx];
        let pct = |u: &Unit| i32::from(u.hp.max(0)) * 100 / i32::from(u.max_hp.max(1));
        let party_hp: i32 = self
            .alive(Side::Party)
            .map(|i| i32::from(self.units[i].hp))
            .sum();
        let party_max: i32 = self
            .units
            .iter()
            .filter(|u| u.side == Side::Party)
            .map(|u| i32::from(u.max_hp))
            .sum();
        let foe_hp: i32 = self
            .alive(Side::Foe)
            .map(|i| i32::from(self.units[i].hp))
            .sum();
        let foe_max: i32 = self
            .units
            .iter()
            .filter(|u| u.side == Side::Foe)
            .map(|u| i32::from(u.max_hp))
            .sum();
        if party_hp * 100 < party_max * 25 && foe_hp * 100 > foe_max * 50 {
            return FightAction::Flee;
        }
        let hurt = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| u.side == Side::Party && !u.gone && pct(u) < 40)
            .min_by_key(|(i, u)| (u.hp, *i))
            .map(|(i, _)| i);
        if me.trade == Some(Trade::Healer)
            && let Some(h) = hurt
        {
            return FightAction::Mend {
                ally: u8::try_from(h).unwrap_or(0),
            };
        }
        if me.cool == 0
            && let Some(special) = self.auto_special(idx)
        {
            return special;
        }
        let melee = if self.in_reach(idx) {
            self.melee_targets(idx)
        } else {
            Vec::new()
        };
        if let Some(&t) = melee.iter().min_by_key(|&&i| (self.units[i].hp, i)) {
            return FightAction::Strike {
                target: u8::try_from(t).unwrap_or(0),
            };
        }
        if let Some(t) = self.alive(Side::Foe).min_by_key(|&i| (self.units[i].hp, i)) {
            return FightAction::Throw {
                target: u8::try_from(t).unwrap_or(0),
            };
        }
        FightAction::Brace
    }

    fn auto_special(&self, idx: usize) -> Option<FightAction> {
        let me = &self.units[idx];
        let foes = self.alive(Side::Foe).count();
        let target = |i: Option<usize>| i.map(|i| u8::try_from(i).unwrap_or(0));
        match me.trade? {
            Trade::Smith => {
                (foes >= 2 && me.guarded == 0).then_some(FightAction::Special { target: None })
            }
            Trade::Healer => {
                let hurt = self
                    .alive(Side::Party)
                    .filter(|&i| {
                        i32::from(self.units[i].hp) * 100 < i32::from(self.units[i].max_hp) * 70
                    })
                    .count();
                (hurt >= 2).then_some(FightAction::Special { target: None })
            }
            Trade::ReedWitch => self.threat(|u| u.hexed > 0).map(|t| FightAction::Special {
                target: target(Some(t)),
            }),
            Trade::Salvager | Trade::Wader => {
                self.threat(|u| u.netted > 0).map(|t| FightAction::Special {
                    target: target(Some(t)),
                })
            }
            Trade::Fighter => {
                let t = self
                    .melee_targets(idx)
                    .into_iter()
                    .max_by_key(|&i| (self.units[i].hp, std::cmp::Reverse(i)))?;
                (self.in_reach(idx) && self.units[t].hp > 8).then_some(FightAction::Special {
                    target: target(Some(t)),
                })
            }
            Trade::Raker => {
                let front = self.front(Side::Foe)?;
                let n = self
                    .alive(Side::Foe)
                    .filter(|&i| self.units[i].row == front)
                    .count();
                (n >= 2 && self.in_reach(idx)).then_some(FightAction::Special { target: None })
            }
            Trade::Boatwright => self
                .alive(Side::Foe)
                .find(|&i| self.units[i].row > 0 && self.units[i].behaviour == Behaviour::Hurler)
                .map(|t| FightAction::Special {
                    target: target(Some(t)),
                }),
        }
    }

    /// The party's staying power over the foes', in percent: each side's hit points times what
    /// a hit is worth.
    #[must_use]
    pub fn ratio(&self) -> i64 {
        let power = |side: Side| -> i64 {
            self.units
                .iter()
                .filter(|u| u.side == side && u.alive())
                .map(|u| i64::from(u.hp) * i64::from(self.atk_of(u) + 1))
                .sum::<i64>()
        };
        power(Side::Party) * 100 / power(Side::Foe).max(1)
    }

    /// A read on the odds from the units as they stand.
    #[must_use]
    pub fn odds(&self) -> Odds {
        match self.ratio() {
            r if r >= ODDS_EASY => Odds::Easy,
            r if r >= ODDS_FAIR => Odds::Fair,
            r if r >= ODDS_HARD => Odds::Hard,
            _ => Odds::Grim,
        }
    }

    /// Do the current unit's action. Returns the new progress.
    ///
    /// # Errors
    /// Why the action cannot be done (a plain sentence); nothing changes.
    #[allow(clippy::too_many_lines)]
    pub fn act(
        &mut self,
        action: FightAction,
        rng: &mut DoorRng,
        lines: &mut Vec<String>,
    ) -> Result<Progress, String> {
        let Some(&slot) = self.order.get(usize::from(self.cursor)) else {
            return Err("It is nobody's turn.".to_owned());
        };
        let me = usize::from(slot);
        if self.units[me].side != Side::Party || !self.units[me].alive() {
            return Err("It is not your turn.".to_owned());
        }
        let foe_ok = |f: &Self, t: u8| {
            let t = usize::from(t);
            f.units
                .get(t)
                .is_some_and(|u| u.side == Side::Foe && u.alive())
        };
        match action {
            FightAction::Strike { target } => {
                let t = usize::from(target);
                if !foe_ok(self, target) {
                    return Err("There is nobody there.".to_owned());
                }
                if !self.in_reach(me) {
                    return Err(
                        "Only the front rank fights hand to hand. Throw, or mend, from here."
                            .to_owned(),
                    );
                }
                if !self.melee_targets(me).contains(&t) {
                    return Err("That one is behind the front rank.".to_owned());
                }
                self.hit(me, t, 0, false, rng, lines);
            }
            FightAction::Brace => {
                self.units[me].braced = true;
                lines.push(format!("{} braces.", self.units[me].name));
            }
            FightAction::Shove { target } => {
                let t = usize::from(target);
                if !foe_ok(self, target) {
                    return Err("There is nobody there.".to_owned());
                }
                if !self.in_reach(me) || !self.melee_targets(me).contains(&t) {
                    return Err("You can only shove one you can reach.".to_owned());
                }
                self.shove(me, t, rng, lines);
            }
            FightAction::Throw { target } => {
                if !foe_ok(self, target) {
                    return Err("There is nobody there.".to_owned());
                }
                let t = usize::from(target);
                if rng.chance(15, 100) {
                    lines.push(format!("{} throws wide.", self.units[me].name));
                } else {
                    let reach = self.units[me].spd / 3;
                    let dmg = (1 + reach + i16::from(self.units[me].armed) - self.units[t].def / 2)
                        .max(1);
                    self.units[t].hp -= dmg;
                    lines.push(format!(
                        "{} throws at {} for {dmg}.",
                        self.units[me].name, self.units[t].name
                    ));
                    if self.units[t].hp <= 0 {
                        lines.push(format!("{} falls.", self.units[t].name));
                    }
                }
            }
            FightAction::Mend { ally } => {
                let a = usize::from(ally);
                let ok = self
                    .units
                    .get(a)
                    .is_some_and(|u| u.side == Side::Party && !u.gone && u.hp < u.max_hp);
                if !ok {
                    return Err("They do not need mending.".to_owned());
                }
                let skill = if self.units[me].trade == Some(Trade::Healer) {
                    3
                } else {
                    0
                };
                let heal = 2 + skill + self.units[me].spd / 3;
                let was_down = self.units[a].hp <= 0;
                self.units[a].hp = (self.units[a].hp.max(0) + heal).min(self.units[a].max_hp);
                lines.push(format!(
                    "{} {} {} ({} hit points).",
                    self.units[me].name,
                    if was_down { "lifts" } else { "patches up" },
                    self.units[a].name,
                    heal
                ));
            }
            FightAction::Special { target } => self.special(me, target, rng, lines)?,
            FightAction::Flee => {
                let mine: i16 = {
                    let n = i16::try_from(self.alive(Side::Party).count())
                        .unwrap_or(1)
                        .max(1);
                    self.alive(Side::Party)
                        .map(|i| self.units[i].spd)
                        .sum::<i16>()
                        / n
                };
                let theirs = self
                    .alive(Side::Foe)
                    .map(|i| self.units[i].spd)
                    .max()
                    .unwrap_or(0);
                let wet = if self.wet { -5 } else { 0 };
                let chance = (55 + 5 * (mine - theirs) + wet + self.flee_mod).clamp(15, 90);
                if rng.chance(u32::try_from(chance).unwrap_or(15), 100) {
                    lines.push("You break away and run.".to_owned());
                    return Ok(Progress::Over(Verdict::Fled));
                }
                lines.push(format!(
                    "{} tries to run and is cut off.",
                    self.units[me].name
                ));
            }
        }
        self.cursor += 1;
        Ok(self.advance(rng, lines))
    }

    fn shove(&mut self, me: usize, t: usize, rng: &mut DoorRng, lines: &mut Vec<String>) {
        let grit = i32::from(self.units[me].mass);
        let mass = i32::from(self.units[t].mass);
        let resist = if self.wet && self.units[t].aquatic {
            20
        } else {
            0
        };
        let chance = (50 + 6 * (grit - mass) - resist).clamp(15, 90);
        let (an, tn) = (self.units[me].name.clone(), self.units[t].name.clone());
        if !rng.chance(u32::try_from(chance).unwrap_or(15), 100) {
            lines.push(format!("{an} heaves at {tn} and it does not budge."));
            return;
        }
        let row = self.units[t].row;
        let last = self
            .alive(Side::Foe)
            .map(|i| self.units[i].row)
            .max()
            .unwrap_or(row);
        if row >= last {
            // Nowhere behind it to go. In the water a light land-bound foe goes under.
            if self.wet && self.units[t].mass <= 4 && !self.units[t].aquatic {
                self.units[t].gone = true;
                lines.push(format!(
                    "{an} shoves {tn} into the water, and the current takes it."
                ));
            } else {
                lines.push(format!("{an} shoves {tn}, but there is no room behind it."));
            }
            return;
        }
        // Swap with whoever stands one row back, so the board stays as it was but for the two.
        let behind: Vec<usize> = self
            .alive(Side::Foe)
            .filter(|&i| i != t && self.units[i].row == row + 1)
            .collect();
        for i in behind {
            self.units[i].row = row;
        }
        self.units[t].row = row + 1;
        lines.push(format!("{an} shoves {tn} back a rank."));
    }

    #[allow(clippy::too_many_lines)]
    fn special(
        &mut self,
        me: usize,
        target: Option<u8>,
        rng: &mut DoorRng,
        lines: &mut Vec<String>,
    ) -> Result<(), String> {
        let Some(trade) = self.units[me].trade else {
            return Err("You have no special.".to_owned());
        };
        if self.units[me].cool > 0 {
            return Err(format!(
                "Not ready for {} more round{}.",
                self.units[me].cool,
                if self.units[me].cool == 1 { "" } else { "s" }
            ));
        }
        let need = |f: &Self| -> Result<usize, String> {
            let t = usize::from(target.ok_or("That needs a target.")?);
            if f.units
                .get(t)
                .is_some_and(|u| u.side == Side::Foe && u.alive())
            {
                Ok(t)
            } else {
                Err("There is nobody there.".to_owned())
            }
        };
        let name = self.units[me].name.clone();
        match trade {
            Trade::Wader => {
                let t = need(self)?;
                let rounds = if self.wet { 2 } else { 1 };
                self.units[t].netted = self.units[t].netted.max(rounds);
                lines.push(format!(
                    "{name} drags {} into the mire{}.",
                    self.units[t].name,
                    if self.wet { ", and it sticks fast" } else { "" }
                ));
            }
            Trade::Raker => {
                if !self.in_reach(me) {
                    return Err("The sweep needs a place in the front rank.".to_owned());
                }
                let front = self.front(Side::Foe).unwrap_or(0);
                let hit: Vec<usize> = self
                    .alive(Side::Foe)
                    .filter(|&i| self.units[i].row == front)
                    .collect();
                lines.push(format!("{name} sweeps the rake through the front rank."));
                for t in hit {
                    let half = self.atk_of(&self.units[me]) / 2;
                    let dmg = (half - self.def_of(&self.units[t]) / 2).max(1);
                    self.units[t].hp -= dmg;
                    lines.push(format!("{} takes {dmg}.", self.units[t].name));
                    if self.units[t].hp <= 0 {
                        lines.push(format!("{} falls.", self.units[t].name));
                    }
                }
            }
            Trade::Salvager => {
                let t = need(self)?;
                self.units[t].netted = self.units[t].netted.max(2);
                lines.push(format!(
                    "{name} throws the net over {}.",
                    self.units[t].name
                ));
            }
            Trade::Fighter => {
                let t = need(self)?;
                if !self.in_reach(me) || !self.melee_targets(me).contains(&t) {
                    return Err("You can only cleave one you can reach.".to_owned());
                }
                let before = self.units[me].atk;
                self.units[me].atk = before * 3 / 2;
                lines.push(format!("{name} swings with everything."));
                self.hit(me, t, 0, true, rng, lines);
                self.units[me].atk = before;
                self.units[me].exposed = true;
            }
            Trade::Healer => {
                let heal = 1 + self.units[me].spd / 4 + 2;
                let allies: Vec<usize> = self
                    .units
                    .iter()
                    .enumerate()
                    .filter(|(_, u)| u.side == Side::Party && !u.gone)
                    .map(|(i, _)| i)
                    .collect();
                for a in allies {
                    let u = &mut self.units[a];
                    u.hp = (u.hp.max(0) + heal).min(u.max_hp);
                }
                lines.push(format!(
                    "{name} passes a salve down the line ({heal} each)."
                ));
            }
            Trade::Boatwright => {
                let t = need(self)?;
                let row = self.units[t].row;
                if row > 0 {
                    let front: Vec<usize> = self
                        .alive(Side::Foe)
                        .filter(|&i| i != t && self.units[i].row == row - 1)
                        .collect();
                    for i in front {
                        self.units[i].row = row;
                    }
                    self.units[t].row = row - 1;
                }
                let dmg =
                    (self.atk_of(&self.units[me]) / 2 - self.def_of(&self.units[t]) / 2).max(1);
                self.units[t].hp -= dmg;
                lines.push(format!(
                    "{name} hooks {} forward and clips it for {dmg}.",
                    self.units[t].name
                ));
                if self.units[t].hp <= 0 {
                    lines.push(format!("{} falls.", self.units[t].name));
                }
            }
            Trade::Smith => {
                for u in self
                    .units
                    .iter_mut()
                    .filter(|u| u.side == Side::Party && u.alive())
                {
                    u.guarded = 3;
                }
                lines.push(format!("{name} sets a shield-wall. The party is guarded."));
            }
            Trade::ReedWitch => {
                let t = need(self)?;
                self.units[t].hexed = 3;
                lines.push(format!(
                    "{name} speaks a word to the reeds, and {} falters.",
                    self.units[t].name
                ));
            }
        }
        self.units[me].cool = SPECIAL_COOL + 1;
        Ok(())
    }

    /// Write to a save.
    pub fn write(&self, w: &mut Writer) {
        w.str(&self.group);
        w.count(self.units.len());
        for u in &self.units {
            u.write(w);
        }
        w.u8(self.round);
        w.count(self.order.len());
        for o in &self.order {
            w.u8(*o);
        }
        w.u8(self.cursor);
        w.bool(self.wet);
        w.u32(u32::from_le_bytes(self.level.to_le_bytes()));
        w.u8(self.limit);
        w.u16(u16::from_le_bytes(self.flee_mod.to_le_bytes()));
    }

    /// Read from a save.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        let group = r.str()?;
        let units = (0..r.u8()?)
            .map(|_| Unit::read(r))
            .collect::<Option<Vec<_>>>()?;
        let round = r.u8()?;
        let order = (0..r.u8()?).map(|_| r.u8()).collect::<Option<Vec<_>>>()?;
        let cursor = r.u8()?;
        let wet = r.bool()?;
        let level = i32::from_le_bytes(r.u32()?.to_le_bytes());
        let limit = r.u8()?;
        let flee_mod = i16::from_le_bytes(r.u16()?.to_le_bytes());
        let ok = units.len() <= MAX_UNITS && order.iter().all(|o| usize::from(*o) < units.len());
        ok.then_some(Self {
            group,
            units,
            round,
            order,
            cursor,
            wet,
            level,
            limit,
            flee_mod,
        })
    }
}

#[cfg(test)]
mod tests;
