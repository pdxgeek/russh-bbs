//! Boarding: a room-by-room skirmish on a derelict or a wreck, with the crew.
//!
//! A boarding party is the captain plus every fit hand. The captain gives one order a round:
//! move to a room, fight what is in it, breach a sealed door, search for a data cache, patch the
//! wounded, take the loot, or withdraw. What the party can do depends on who is in it: a locked
//! door opens only if the ship's engineering (the captain's, with the crew's help) is up to the
//! lock; a cache is read only by a scientist or a practised surveyor; a gunner doubles the
//! party's punch; a medic keeps the hurt on their feet and the dying alive.
//!
//! Nothing is gained until the party gets out: loot is held in the [`Boarding::haul`] and is
//! lost if the party is wiped out.

use russh_bbs_door::DoorRng;
use russh_bbs_door::content::Table;
use serde::Deserialize;

use crate::codec::{Reader, Writer};
use crate::content::Content;
use crate::crew::Role;
use crate::engine::Env;
use crate::events::{Event, Refusal};
use crate::player::{Cause, MAX_SURVEY, Player, SurveyLot};
use crate::scene::Scene;
use crate::skills::Skill;

/// Longest a boarding lasts, in orders, before the party pulls out on its own.
pub const MAX_ORDERS: u8 = 60;
/// Most rooms a layout has.
pub const MAX_ROOMS: usize = 12;

/// One room of a layout.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomDef {
    /// Name.
    pub name: String,
    /// What the party sees.
    pub text: String,
    /// Hostiles in it.
    #[serde(default)]
    pub foes: u8,
    /// Engineering needed to cut the door open (0 for an open door).
    #[serde(default)]
    pub lock: u8,
    /// Credits in it (low, high).
    #[serde(default)]
    pub credits: [u32; 2],
    /// Cargo in it (a good id), with `tons`.
    #[serde(default)]
    pub good: String,
    /// Tons of that good.
    #[serde(default)]
    pub tons: u16,
    /// Survey value of a data cache, found only by a reader.
    #[serde(default)]
    pub cache: u32,
    /// Hull plating and parts that patch the ship, in hull points.
    #[serde(default)]
    pub parts: u16,
    /// Rooms it opens onto.
    pub exits: Vec<u8>,
}

/// A derelict or wreck layout.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerelictDef {
    /// Stable id.
    pub id: String,
    /// Name.
    pub name: String,
    /// One line.
    pub blurb: String,
    /// What lives aboard.
    pub foe_name: String,
    /// Hit points of each.
    pub foe_hp: u8,
    /// Damage of each, per round.
    pub foe_dmg: u8,
    /// The rooms; room 0 is the airlock.
    pub rooms: Vec<RoomDef>,
}

/// The layouts.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerelictTable {
    /// All layouts.
    pub derelict: Vec<DerelictDef>,
}

impl Table for DerelictTable {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        crate::content::duplicates(
            self.derelict.iter().map(|d| d.id.as_str()),
            "derelict",
            &mut p,
        );
        if self.derelict.len() < 6 || self.derelict.len() > 60 {
            p.push("between 6 and 60 derelict layouts".into());
        }
        for d in &self.derelict {
            if !crate::content::text_ok(&d.name, 32)
                || !crate::content::text_ok(&d.blurb, 120)
                || !crate::content::text_ok(&d.foe_name, 24)
                || d.foe_hp == 0
                || d.foe_hp > 40
                || d.foe_dmg == 0
                || d.foe_dmg > 12
            {
                p.push(format!(
                    "derelict `{}`: a stat or text is out of range",
                    d.id
                ));
            }
            if d.rooms.len() < 3 || d.rooms.len() > MAX_ROOMS {
                p.push(format!("derelict `{}`: 3 to {MAX_ROOMS} rooms", d.id));
            }
            let n = d.rooms.len();
            for (i, r) in d.rooms.iter().enumerate() {
                if !crate::content::text_ok(&r.name, 28) || !crate::content::text_ok(&r.text, 160) {
                    p.push(format!(
                        "derelict `{}` room {i}: name <= 28, text <= 160",
                        d.id
                    ));
                }
                if r.exits.is_empty() || r.exits.iter().any(|&e| usize::from(e) >= n) {
                    p.push(format!("derelict `{}` room {i}: bad exits", d.id));
                }
                if r.lock > 9 || r.foes > 6 || r.credits[0] > r.credits[1] {
                    p.push(format!(
                        "derelict `{}` room {i}: lock, foes or credits out of range",
                        d.id
                    ));
                }
                if r.good.is_empty() != (r.tons == 0) {
                    p.push(format!(
                        "derelict `{}` room {i}: good and tons go together",
                        d.id
                    ));
                }
            }
            if let Some(first) = d.rooms.first()
                && (first.foes != 0 || first.lock != 0)
            {
                p.push(format!(
                    "derelict `{}`: the airlock must be open and empty",
                    d.id
                ));
            }
            // Every room must be reachable from the airlock by exits.
            let mut seen = vec![false; n];
            let mut stack = vec![0usize];
            while let Some(i) = stack.pop() {
                if i >= n || seen[i] {
                    continue;
                }
                seen[i] = true;
                for &e in &d.rooms[i].exits {
                    stack.push(usize::from(e));
                }
            }
            if seen.iter().any(|s| !s) {
                p.push(format!("derelict `{}`: a room cannot be reached", d.id));
            }
        }
        p
    }
}

/// One of the party.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    /// Index into the crew list, or `None` for the captain.
    pub crew: Option<u8>,
    /// Hit points.
    pub hp: i16,
    /// Most hit points.
    pub max: i16,
}

impl Member {
    /// Whether still on their feet.
    #[must_use]
    pub const fn up(&self) -> bool {
        self.hp > 0
    }
}

/// What the party has taken so far.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Haul {
    /// Credits.
    pub credits: u32,
    /// Hull points of parts.
    pub parts: u16,
    /// Cargo: good and tons.
    pub goods: Vec<(u8, u16)>,
    /// Survey value of caches read.
    pub survey: u32,
}

/// A boarding in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boarding {
    /// Index into the content's derelicts.
    pub layout: u8,
    /// The room the party is in.
    pub at: u8,
    /// Rooms whose doors are open (bit per room).
    pub opened: u16,
    /// Rooms whose hostiles are dead.
    pub cleared: u16,
    /// Rooms whose loot has been taken.
    pub looted: u16,
    /// Rooms whose cache has been read.
    pub read: u16,
    /// The party.
    pub party: Vec<Member>,
    /// Hit points of the hostiles in this room, all together.
    pub foe_hp: i16,
    /// Orders given.
    pub orders: u8,
    /// What has been taken.
    pub haul: Haul,
}

/// An order to the party.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drill {
    /// Move to a connected room.
    Move(u8),
    /// Fight the hostiles in the room.
    Fight,
    /// Cut open the sealed door to a connected room.
    Breach(u8),
    /// Search the room for a data cache.
    Search,
    /// Patch the most hurt.
    Patch,
    /// Take what the room holds.
    Take,
    /// Pull out with what has been taken.
    Withdraw,
}

fn bit(room: u8) -> u16 {
    1u16 << u32::from(room.min(15))
}

impl Boarding {
    /// The layout.
    #[must_use]
    pub fn def<'a>(&self, content: &'a Content) -> &'a DerelictDef {
        &content.derelicts.derelict[usize::from(self.layout)]
    }

    /// The room the party is in.
    #[must_use]
    pub fn room<'a>(&self, content: &'a Content) -> &'a RoomDef {
        &self.def(content).rooms[usize::from(self.at)]
    }

    /// Hostiles left in the current room.
    #[must_use]
    pub fn foes_here(&self, content: &Content) -> u8 {
        if self.cleared & bit(self.at) != 0 {
            return 0;
        }
        let def = self.def(content);
        let per = i16::from(def.foe_hp.max(1));
        u8::try_from((self.foe_hp.max(0) + per - 1) / per).unwrap_or(0)
    }

    /// Write to a save.
    pub fn write(&self, w: &mut Writer) {
        w.u8(self.layout);
        w.u8(self.at);
        for v in [self.opened, self.cleared, self.looted, self.read] {
            w.u16(v);
        }
        w.len(self.party.len());
        for m in &self.party {
            w.u8(m.crew.unwrap_or(255));
            w.i32(i32::from(m.hp));
            w.i32(i32::from(m.max));
        }
        w.i32(i32::from(self.foe_hp));
        w.u8(self.orders);
        w.u32(self.haul.credits);
        w.u16(self.haul.parts);
        w.u32(self.haul.survey);
        w.len(self.haul.goods.len());
        for (g, t) in &self.haul.goods {
            w.u8(*g);
            w.u16(*t);
        }
    }

    /// Read from a save, or `None` if anything is out of range.
    pub fn read(r: &mut Reader<'_>, content: &Content, crew_len: usize) -> Option<Self> {
        let layout = r.u8()?;
        let def = content.derelicts.derelict.get(usize::from(layout))?;
        let at = r.u8()?;
        if usize::from(at) >= def.rooms.len() {
            return None;
        }
        let (opened, cleared, looted, read) = (r.u16()?, r.u16()?, r.u16()?, r.u16()?);
        let n = r.len(9, 9)?;
        let mut party = Vec::with_capacity(n);
        for _ in 0..n {
            let crew = match r.u8()? {
                255 => None,
                c if usize::from(c) < crew_len => Some(c),
                _ => return None,
            };
            let hp = i16::try_from(r.i32()?).ok()?;
            let max = i16::try_from(r.i32()?).ok()?;
            if max <= 0 || hp > max {
                return None;
            }
            party.push(Member { crew, hp, max });
        }
        if party.is_empty() {
            return None;
        }
        let foe_hp = i16::try_from(r.i32()?).ok()?;
        let orders = r.u8()?;
        let credits = r.u32()?;
        let plating = r.u16()?;
        let survey = r.u32()?;
        let k = r.len(8, 3)?;
        let mut goods = Vec::with_capacity(k);
        for _ in 0..k {
            let g = r.u8()?;
            if usize::from(g) >= content.goods.len() {
                return None;
            }
            goods.push((g, r.u16()?));
        }
        Some(Self {
            layout,
            at,
            opened,
            cleared,
            looted,
            read,
            party,
            foe_hp,
            orders,
            haul: Haul {
                credits,
                parts: plating,
                goods,
                survey,
            },
        })
    }
}

impl Player {
    /// Put together a party and start a boarding.
    pub fn start_boarding(&mut self, env: &mut Env<'_>, id: &str, events: &mut Vec<Event>) {
        let Some(layout) = env
            .content
            .derelicts
            .derelict
            .iter()
            .position(|d| d.id == id)
        else {
            return;
        };
        let mut party = vec![Member {
            crew: None,
            hp: 20,
            max: 20,
        }];
        for (i, c) in self.crew.iter().enumerate() {
            if c.fit()
                && let Ok(ix) = u8::try_from(i)
            {
                let hp = 12 + 2 * i16::from(c.effective());
                party.push(Member {
                    crew: Some(ix),
                    hp,
                    max: hp,
                });
            }
        }
        let layout = u8::try_from(layout).unwrap_or(0);
        let boarding = Boarding {
            layout,
            at: 0,
            opened: 1,
            cleared: 1,
            looted: 1,
            read: 1,
            party,
            foe_hp: 0,
            orders: 0,
            haul: Haul::default(),
        };
        events.push(Event::BoardStart { layout });
        self.scene = Some(Scene::Boarding(Box::new(boarding)));
    }

    fn take_boarding(&mut self) -> Result<Box<Boarding>, Refusal> {
        match self.scene.take() {
            Some(Scene::Boarding(b)) => Ok(b),
            other => {
                self.scene = other;
                Err(Refusal::NoBoarding)
            }
        }
    }

    /// One member's punch in a round.
    fn punch(&self, m: Member) -> i32 {
        if !m.up() {
            return 0;
        }
        match m.crew.and_then(|i| self.crew.get(usize::from(i))) {
            None => 3,
            Some(c) => match c.role {
                Role::Gunner => 4 + i32::from(c.effective()),
                Role::Medic => 1,
                _ => 2 + i32::from(c.effective() / 2),
            },
        }
    }

    /// Give the party an order.
    pub(crate) fn skirmish(
        &mut self,
        env: &mut Env<'_>,
        order: Drill,
    ) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let mut b = self.take_boarding()?;
        let result = self.run_drill(env, &mut b, order);
        match result {
            Err(refusal) => {
                self.scene = Some(Scene::Boarding(b));
                Err(refusal)
            }
            Ok(mut events) => {
                b.orders = b.orders.saturating_add(1);
                let wiped =
                    !b.party.iter().any(Member::up) || !b.party.first().is_some_and(Member::up);
                let leaving = matches!(order, Drill::Withdraw) || b.orders >= MAX_ORDERS;
                if wiped {
                    self.end_boarding(env, &b, false, &mut events);
                } else if leaving {
                    self.end_boarding(env, &b, true, &mut events);
                } else {
                    self.scene = Some(Scene::Boarding(b));
                }
                let _ = content;
                Ok(events)
            }
        }
    }

    #[allow(clippy::too_many_lines, reason = "one arm per order")]
    fn run_drill(
        &mut self,
        env: &mut Env<'_>,
        b: &mut Boarding,
        order: Drill,
    ) -> Result<Vec<Event>, Refusal> {
        let content = env.content;
        let def = b.def(content);
        let room = b.room(content);
        let foes = b.foes_here(content);
        let mut events = Vec::new();
        match order {
            Drill::Move(to) => {
                if foes > 0 {
                    return Err(Refusal::FoesHere);
                }
                if !room.exits.contains(&to) {
                    return Err(Refusal::NoSuchThing);
                }
                let target = &def.rooms[usize::from(to)];
                if target.lock > 0 && b.opened & bit(to) == 0 {
                    return Err(Refusal::Sealed { lock: target.lock });
                }
                b.at = to;
                b.opened |= bit(to);
                if target.foes > 0 && b.cleared & bit(to) == 0 {
                    b.foe_hp = i16::from(target.foes) * i16::from(def.foe_hp);
                }
                events.push(Event::RoomEntered {
                    layout: b.layout,
                    room: to,
                });
                if target.foes > 0 && b.cleared & bit(to) == 0 {
                    // Hostiles get the first word on a surprised party.
                    self.foes_strike(env.rng, b, &mut events);
                }
            }
            Drill::Breach(to) => {
                if foes > 0 {
                    return Err(Refusal::FoesHere);
                }
                if !room.exits.contains(&to) {
                    return Err(Refusal::NoSuchThing);
                }
                let target = &def.rooms[usize::from(to)];
                if target.lock == 0 || b.opened & bit(to) != 0 {
                    return Err(Refusal::NothingToDo);
                }
                // The party's engineering: the captain's own, with the best fit hand's help.
                let in_party = |role: Role| {
                    b.party.iter().filter(|m| m.up()).any(|m| {
                        m.crew
                            .and_then(|i| self.crew.get(usize::from(i)))
                            .is_some_and(|c| c.role == role)
                    })
                };
                let mut eng = self.level(Skill::Engineer);
                if in_party(Role::Engineer) {
                    eng = (eng + self.crew_bonus(Role::Engineer)).min(10);
                }
                if eng < target.lock {
                    return Err(Refusal::Sealed { lock: target.lock });
                }
                b.opened |= bit(to);
                events.push(Event::Breached { room: to });
                self.train_role(Role::Engineer, &mut events);
                self.gain(Skill::Engineer, 4, &mut events);
            }
            Drill::Fight => {
                if foes == 0 {
                    return Err(Refusal::NothingToDo);
                }
                let punch: i32 = b.party.iter().map(|m| self.punch(*m)).sum();
                let before = foes;
                b.foe_hp = i16::try_from((i32::from(b.foe_hp) - punch.max(1)).max(0)).unwrap_or(0);
                if b.foe_hp == 0 {
                    b.cleared |= bit(b.at);
                }
                let killed = before - b.foes_here(content);
                events.push(Event::Struck {
                    killed,
                    left: b.foes_here(content),
                });
                self.train_role(Role::Gunner, &mut events);
                if b.foes_here(content) > 0 {
                    self.foes_strike(env.rng, b, &mut events);
                }
            }
            Drill::Search => {
                if foes > 0 {
                    return Err(Refusal::FoesHere);
                }
                if room.cache == 0 || b.read & bit(b.at) != 0 {
                    return Err(Refusal::NothingToDo);
                }
                let scientist = b.party.iter().filter(|m| m.up()).any(|m| {
                    m.crew
                        .and_then(|i| self.crew.get(usize::from(i)))
                        .is_some_and(|c| c.role == Role::Scientist)
                });
                if !scientist && self.level(Skill::Survey) < 3 {
                    return Err(Refusal::NoReader);
                }
                b.read |= bit(b.at);
                b.haul.survey = b.haul.survey.saturating_add(room.cache);
                events.push(Event::CacheRead { value: room.cache });
                self.train_role(Role::Scientist, &mut events);
                self.gain(Skill::Survey, 6, &mut events);
            }
            Drill::Patch => {
                let medic = b.party.iter().position(|m| {
                    m.up()
                        && m.crew
                            .and_then(|i| self.crew.get(usize::from(i)))
                            .is_some_and(|c| c.role == Role::Medic)
                });
                let heal: i16 = medic
                    .and_then(|i| b.party[i].crew)
                    .and_then(|ci| self.crew.get(usize::from(ci)))
                    .map_or(3, |c| 6 + i16::from(c.effective()));
                let worst = b
                    .party
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.up() && m.hp < m.max)
                    .min_by_key(|(_, m)| m.hp)
                    .map(|(i, _)| i)
                    .ok_or(Refusal::NothingToDo)?;
                let m = &mut b.party[worst];
                let before = m.hp;
                m.hp = (m.hp + heal).min(m.max);
                events.push(Event::Patched2 {
                    hp: u16::try_from(m.hp - before).unwrap_or(0),
                });
                if medic.is_some() {
                    self.train_role(Role::Medic, &mut events);
                }
                if foes > 0 {
                    self.foes_strike(env.rng, b, &mut events);
                }
            }
            Drill::Take => {
                if foes > 0 {
                    return Err(Refusal::FoesHere);
                }
                if b.looted & bit(b.at) != 0
                    || (room.credits[1] == 0 && room.tons == 0 && room.parts == 0)
                {
                    return Err(Refusal::NothingToDo);
                }
                b.looted |= bit(b.at);
                let credits = env.rng.range(room.credits[0], room.credits[1]);
                b.haul.credits = b.haul.credits.saturating_add(credits);
                b.haul.parts = b.haul.parts.saturating_add(room.parts);
                let mut tons = 0;
                if let Some(g) = content.good_ix(&room.good)
                    && b.haul.goods.len() < 4
                {
                    b.haul.goods.push((u8::try_from(g).unwrap_or(0), room.tons));
                    tons = room.tons;
                }
                events.push(Event::Looted {
                    credits,
                    parts: room.parts,
                    tons,
                });
            }
            Drill::Withdraw => {
                if foes > 0 {
                    // Turning your back on armed hostiles costs a round of fire.
                    self.foes_strike(env.rng, b, &mut events);
                }
            }
        }
        Ok(events)
    }

    /// The hostiles in the room hit back.
    fn foes_strike(&mut self, rng: &mut DoorRng, b: &mut Boarding, events: &mut Vec<Event>) {
        let content_foes = i32::from(b.foe_hp.max(0));
        if content_foes == 0 {
            return;
        }
        let up: Vec<usize> = b
            .party
            .iter()
            .enumerate()
            .filter(|(_, m)| m.up())
            .map(|(i, _)| i)
            .collect();
        if up.is_empty() {
            return;
        }
        // Each hostile (about one per foe_hp) swings at someone. The count comes from the room.
        let swings = u32::try_from((content_foes + 7) / 8)
            .unwrap_or(1)
            .clamp(1, 6);
        let mut total = 0i16;
        for _ in 0..swings {
            if rng.chance(60, 100) {
                let who = up[rng.below(u32::try_from(up.len()).unwrap_or(1)) as usize];
                let dmg = i16::try_from(rng.range(2, 5)).unwrap_or(3);
                let m = &mut b.party[who];
                let was_up = m.up();
                m.hp = (m.hp - dmg).max(-5);
                total += dmg;
                if was_up
                    && !m.up()
                    && let Some(c) = m.crew.and_then(|i| self.crew.get(usize::from(i)))
                {
                    events.push(Event::Downed {
                        name: c.name.clone(),
                    });
                } else if was_up && !m.up() {
                    events.push(Event::CaptainDown);
                }
            }
        }
        events.push(Event::PartyHit {
            hp: total.unsigned_abs(),
        });
    }

    /// Bring the party out. A wiped-out party loses the haul and comes back hurt.
    fn end_boarding(
        &mut self,
        env: &mut Env<'_>,
        b: &Boarding,
        clean: bool,
        events: &mut Vec<Event>,
    ) {
        self.scene = None;
        let rng = &mut *env.rng;
        // Downed hands: hurt, or lost.
        let medic = self.has_role(Role::Medic, 1);
        let mut to_lose: Vec<usize> = Vec::new();
        for m in &b.party {
            let Some(ci) = m.crew.map(usize::from) else {
                continue;
            };
            if !m.up() {
                let death = if medic { 12 } else { 35 };
                if rng.chance(death, 100) && self.crew.len() > to_lose.len() {
                    to_lose.push(ci);
                } else {
                    self.injure(ci, rng, 3, 7, events);
                }
            } else if !clean {
                // The ones still standing drag the rest out, and come back hurt.
                self.injure(ci, rng, 2, 4, events);
            } else if let Some(c) = self.crew.get_mut(ci) {
                c.nudge(crate::crew::DANGER, 1);
            }
        }
        to_lose.sort_unstable();
        for ci in to_lose.into_iter().rev() {
            self.lose_crew(ci, events);
        }
        if !clean {
            events.push(Event::BoardEnd {
                credits: 0,
                parts: 0,
                tons: 0,
                survey: 0,
                clean: false,
            });
            return;
        }
        let credits = i64::from(b.haul.credits);
        self.credit(Cause::Salvage, credits);
        let max_hull = env
            .content
            .hulls
            .get(usize::from(self.ship.hull))
            .map_or(u16::MAX, |h| h.hull_points);
        self.ship.hull_hp = self.ship.hull_hp.saturating_add(b.haul.parts).min(max_hull);
        let mut tons = 0u16;
        // Cargo from the wreck comes aboard free of cost, as far as the hold allows.
        // The hold limit is enforced by the caller through `fit_hold`.
        for &(good, t) in &b.haul.goods {
            tons += self.load_free(env.content, good, t);
        }
        let mut survey = 0;
        if b.haul.survey > 0 && self.survey.len() < MAX_SURVEY {
            let system = self.at;
            self.survey.push(SurveyLot {
                system,
                value: b.haul.survey,
            });
            survey = b.haul.survey;
        }
        self.stats.boardings = self.stats.boardings.saturating_add(1);
        events.push(Event::BoardEnd {
            credits: b.haul.credits,
            parts: b.haul.parts,
            tons,
            survey,
            clean: true,
        });
    }
}
