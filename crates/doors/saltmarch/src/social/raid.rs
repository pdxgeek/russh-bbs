//! Raids: a clan sends a party against another clan's hold. The order goes into the world and
//! the nightly tick settles it with the skirmish engine against the defender's *posted* home
//! guard and defences, while the defender is away. Both clans get a report; losses persist.
//!
//! **Caps** (all in [`caps`], enforced by [`check`] inside the world edit so several sessions
//! cannot get around them): two raids a day per clan; two hits a day per hold; three days
//! between the same two clans; a raided hold is left alone for two days; a new clan is safe for
//! a week unless it raids first; a clan unseen for four days yields half the loot and for ten
//! days cannot be raided; a hold less than a third of the raider's renown is not worth a raid;
//! never against a pact partner, nor for three days after a pact is broken.
//!
//! **The tide gates the order.** A raid lands in one of the day's four turns of the water
//! ([`windows`]). On foot a party can cross only when the water is at or below
//! [`engine::WATER_UP`]; at a high it needs a boat. The defender's sluice floods the approach
//! when the water is at or over its set level: the guard gets the flood rating, the fight is
//! knee-deep and ends after six rounds.

use russh_bbs_door::DoorRng;

use super::{
    Body, ClanIx, DeedKind, Defender, Refusal, Wire, caps, read_list, read_resource, write_list,
    write_resource,
};
use crate::codec::{Reader, Writer};
use crate::combat::{Fight, FightAction, Progress, Side, Unit, Verdict};
use crate::content::Catalog;
use crate::engine::WATER_UP;
use crate::tide::Tide;
use crate::types::Resource;
use crate::world::World;

/// A raid waiting for the tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    /// Its id.
    pub id: u32,
    /// The raider.
    pub from: ClanIx,
    /// The hold raided.
    pub to: ClanIx,
    /// The day it was sent. It lands on the next day, in `window`.
    pub day: i64,
    /// Which turn of the water of the landing day (index into [`windows`]).
    pub window: u8,
    /// Boats taken along.
    pub boats: u8,
    /// The raiding party as it stood when sent.
    pub raiders: Vec<Defender>,
    /// A taunt template left for the defender (a letters table id), or empty.
    pub taunt: String,
}

impl Wire for Order {
    fn write(&self, w: &mut Writer) {
        w.u32(self.id);
        w.u8(self.from);
        w.u8(self.to);
        w.i64(self.day);
        w.u8(self.window);
        w.u8(self.boats);
        write_list(w, &self.raiders);
        w.str(&self.taunt);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            id: r.u32()?,
            from: r.u8()?,
            to: r.u8()?,
            day: r.i64()?,
            window: r.u8()?,
            boats: r.u8()?,
            raiders: read_list(r)?,
            taunt: r.str()?,
        })
    }
}

/// A raid that has been settled, remembered for cooldowns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogEntry {
    /// The raider.
    pub from: ClanIx,
    /// The hold.
    pub to: ClanIx,
    /// The day it was sent.
    pub day: i64,
}

impl Wire for LogEntry {
    fn write(&self, w: &mut Writer) {
        w.u8(self.from);
        w.u8(self.to);
        w.i64(self.day);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            from: r.u8()?,
            to: r.u8()?,
            day: r.i64()?,
        })
    }
}

/// How a raid ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The guard was beaten and the raiders took what they could carry.
    Won,
    /// The guard held.
    Repelled,
    /// A pact (or a vanished hold) called the raid off before the fight.
    Recalled,
}

/// What became of one raider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    /// Back unhurt.
    Unhurt,
    /// Back hurt.
    Hurt,
    /// Not back.
    Dead,
}

/// Which side a report is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The report goes to the raider.
    Raider,
    /// The report goes to the defender.
    Defender,
}

/// A raid's result, delivered to both clans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The order.
    pub order: u32,
    /// Whose copy.
    pub role: Role,
    /// How it ended.
    pub outcome: Outcome,
    /// The raider's name.
    pub raider: String,
    /// The defender's name.
    pub defender: String,
    /// The minute of the day the raid landed.
    pub minute: u16,
    /// The water then.
    pub level: i16,
    /// Whether the sluice flooded the approach.
    pub flooded: bool,
    /// Rounds fought.
    pub rounds: u8,
    /// Goods taken.
    pub loot: Vec<(Resource, u32)>,
    /// Tallies taken.
    pub tallies: u32,
    /// What became of each raider: `(crew id, fate)`.
    pub fates: Vec<(u16, Fate)>,
    /// The guard members who were hurt (crew ids).
    pub hurt: Vec<u16>,
    /// A taunt left with the raid (a letters table id), or empty.
    pub taunt: String,
}

impl Wire for Report {
    fn write(&self, w: &mut Writer) {
        w.u32(self.order);
        w.u8(self.role as u8);
        w.u8(self.outcome as u8);
        w.str(&self.raider);
        w.str(&self.defender);
        w.u16(self.minute);
        w.u16(u16::from_le_bytes(self.level.to_le_bytes()));
        w.bool(self.flooded);
        w.u8(self.rounds);
        w.count(self.loot.len());
        for (r, n) in &self.loot {
            write_resource(w, *r);
            w.u32(*n);
        }
        w.u32(self.tallies);
        w.count(self.fates.len());
        for (id, f) in &self.fates {
            w.u16(*id);
            w.u8(*f as u8);
        }
        w.count(self.hurt.len());
        for id in &self.hurt {
            w.u16(*id);
        }
        w.str(&self.taunt);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let order = r.u32()?;
        let role = match r.u8()? {
            0 => Role::Raider,
            1 => Role::Defender,
            _ => return None,
        };
        let outcome = match r.u8()? {
            0 => Outcome::Won,
            1 => Outcome::Repelled,
            2 => Outcome::Recalled,
            _ => return None,
        };
        let (raider, defender) = (r.str()?, r.str()?);
        let (minute, level) = (r.u16()?, i16::from_le_bytes(r.u16()?.to_le_bytes()));
        let (flooded, rounds) = (r.bool()?, r.u8()?);
        let loot = (0..r.u8()?)
            .map(|_| Some((read_resource(r)?, r.u32()?)))
            .collect::<Option<Vec<_>>>()?;
        let tallies = r.u32()?;
        let fates = (0..r.u8()?)
            .map(|_| {
                let id = r.u16()?;
                let f = match r.u8()? {
                    0 => Fate::Unhurt,
                    1 => Fate::Hurt,
                    2 => Fate::Dead,
                    _ => return None,
                };
                Some((id, f))
            })
            .collect::<Option<Vec<_>>>()?;
        let hurt = (0..r.u8()?).map(|_| r.u16()).collect::<Option<Vec<_>>>()?;
        Some(Self {
            order,
            role,
            outcome,
            raider,
            defender,
            minute,
            level,
            flooded,
            rounds,
            loot,
            tallies,
            fates,
            hurt,
            taunt: r.str()?,
        })
    }
}

/// One turn of the water on the landing day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// Index for the order.
    pub index: u8,
    /// Minute of the day.
    pub minute: u16,
    /// The water then.
    pub level: i32,
    /// High water (or low).
    pub high: bool,
    /// A party needs a boat to cross.
    pub needs_boat: bool,
}

/// The turns of the water on `landing_day`: the windows a raid sent the day before can use.
#[must_use]
pub fn windows(tide: &Tide, landing_day: i64) -> Vec<Window> {
    tide.table(landing_day)
        .extremes
        .iter()
        .enumerate()
        .map(|(i, e)| Window {
            index: u8::try_from(i).unwrap_or(0),
            minute: e.minute,
            level: e.level,
            high: e.high,
            needs_boat: e.level > WATER_UP,
        })
        .collect()
}

/// Whether `from` may send a raid against `to` today, and if not, why. The same rule the edit
/// applies, so the screen can list targets and give the reason for each that is closed.
///
/// # Errors
/// The [`Refusal`] that applies first.
pub fn check(w: &World, from: ClanIx, to: ClanIx, day: i64) -> Result<(), Refusal> {
    let (Some(a), Some(b)) = (w.clans.get(usize::from(from)), w.clans.get(usize::from(to))) else {
        return Err(Refusal::Unknown);
    };
    if from == to {
        return Err(Refusal::Unknown);
    }
    if w.pact_between(from, to).is_some() {
        return Err(Refusal::Pact);
    }
    if let Some(p) = w.pact_record(from, to)
        && p.state == super::pact::State::Broken
        && day - p.since < caps::PAIR_COOLDOWN_DAYS
    {
        return Err(Refusal::Pact);
    }
    let sent_today = if a.record.raid_day == day {
        a.record.raid_n
    } else {
        0
    };
    if sent_today >= caps::RAIDS_PER_DAY {
        return Err(Refusal::RaidsToday);
    }
    let hits = w
        .social
        .orders
        .iter()
        .filter(|o| o.to == to && o.day == day)
        .count();
    if hits >= usize::from(caps::HITS_PER_DAY) {
        return Err(Refusal::HitsToday);
    }
    let recent = w.social.raid_log.iter().any(|e| {
        day - e.day < caps::PAIR_COOLDOWN_DAYS
            && ((e.from == from && e.to == to) || (e.from == to && e.to == from))
    }) || w
        .social
        .orders
        .iter()
        .any(|o| (o.from == from && o.to == to) || (o.from == to && o.to == from));
    if recent {
        return Err(Refusal::PairCooldown);
    }
    if b.record.shield_until > day {
        return Err(Refusal::Shielded);
    }
    if day - b.posted.founded < super::legacy::new_clan_days(w.legacy) && !b.record.raided_first {
        return Err(Refusal::NewClan);
    }
    if day - b.posted.seen >= caps::DORMANT_SAFE_DAYS {
        return Err(Refusal::Dormant);
    }
    if a.score >= 20 && b.score * caps::BULLY_RATIO < a.score {
        return Err(Refusal::Beneath);
    }
    if w.social.orders.len() >= caps::ORDERS_CAP {
        return Err(Refusal::Full);
    }
    Ok(())
}

/// Send a raid. `tide` gives the window's water.
///
/// # Errors
/// See [`check`]; also [`Refusal::BadParty`] and [`Refusal::Window`].
#[allow(clippy::too_many_arguments)]
pub fn place(
    w: &mut World,
    tide: &Tide,
    from: ClanIx,
    to: ClanIx,
    day: i64,
    window: u8,
    boats: u8,
    raiders: Vec<Defender>,
    taunt: &str,
) -> Result<u32, Refusal> {
    check(w, from, to, day)?;
    if raiders.is_empty() || raiders.len() > caps::MAX_RAIDERS {
        return Err(Refusal::BadParty);
    }
    let Some(win) = windows(tide, day + 1).get(usize::from(window)).copied() else {
        return Err(Refusal::Window);
    };
    if win.needs_boat && boats == 0 {
        return Err(Refusal::Window);
    }
    let id = w.social.id();
    w.social.orders.push(Order {
        id,
        from,
        to,
        day,
        window,
        boats,
        raiders,
        taunt: taunt.to_owned(),
    });
    if let Some(a) = w.clans.get_mut(usize::from(from)) {
        let r = &mut a.record;
        if r.raid_day != day {
            r.raid_day = day;
            r.raid_n = 0;
        }
        r.raid_n = r.raid_n.saturating_add(1);
        r.raided_first = true;
    }
    Ok(id)
}

/// What the guard of `to` is on the night: the posted numbers with the hold's defences added.
#[must_use]
pub fn guard_units(w: &World, to: ClanIx, flooded: bool) -> Vec<Unit> {
    let Some(b) = w.clans.get(usize::from(to)) else {
        return Vec::new();
    };
    let rating = i16::from(b.posted.rating)
        + if flooded {
            i16::from(b.posted.flood)
        } else {
            0
        };
    let partner = w.defence_partners(to) > 0;
    b.posted
        .guard
        .iter()
        .map(|d| {
            let mut u = Unit::posted(Side::Foe, d);
            u.def = u.def.saturating_add(rating / 3 + i16::from(partner));
            let extra = rating * 2
                + if partner {
                    i16::from(b.posted.rating) * 2
                } else {
                    0
                };
            u.hp = u.hp.saturating_add(extra);
            u.max_hp = u.hp;
            u
        })
        .collect()
}

/// Fight a raid out and say what the odds looked like: used by the screen's preview.
#[must_use]
pub fn preview_odds(
    cat: &Catalog,
    w: &World,
    to: ClanIx,
    raiders: &[Defender],
    level: i32,
) -> crate::combat::Odds {
    let flooded = w
        .clans
        .get(usize::from(to))
        .is_some_and(|b| b.posted.sluice.is_some_and(|s| level >= i32::from(s)));
    let mut units: Vec<Unit> = raiders
        .iter()
        .map(|d| Unit::posted(Side::Party, d))
        .collect();
    units.extend(guard_units(w, to, flooded));
    Fight::begin(
        cat,
        units,
        "",
        flooded,
        level,
        if flooded { 6 } else { 0 },
        0,
    )
    .odds()
}

struct Fought {
    verdict: Verdict,
    rounds: u8,
    raiders: Vec<(u16, i16, i16)>,
    guard: Vec<(u16, i16, i16)>,
}

fn fight(
    cat: &Catalog,
    o: &Order,
    mut guard: Vec<Unit>,
    flooded: bool,
    level: i32,
    rng: &mut DoorRng,
) -> Fought {
    let mut units: Vec<Unit> = o
        .raiders
        .iter()
        .map(|d| Unit::posted(Side::Party, d))
        .collect();
    units.append(&mut guard);
    let mut f = Fight::begin(
        cat,
        units,
        "",
        flooded,
        level,
        if flooded { 6 } else { 0 },
        0,
    );
    let mut lines = Vec::new();
    let mut p = f.advance(rng, &mut lines);
    for _ in 0..400 {
        let Progress::Turn(idx) = p else { break };
        let a = f.auto(idx);
        p = f
            .act(a, rng, &mut lines)
            .or_else(|_| f.act(FightAction::Brace, rng, &mut lines))
            .unwrap_or(Progress::Over(Verdict::Stalemate));
    }
    let verdict = match p {
        Progress::Over(v) => v,
        Progress::Turn(_) => Verdict::Stalemate,
    };
    let of = |side: Side| -> Vec<(u16, i16, i16)> {
        f.units
            .iter()
            .filter(|u| u.side == side)
            .map(|u| (u.crew, u.hp, u.max_hp))
            .collect()
    };
    Fought {
        verdict,
        rounds: f.round,
        raiders: of(Side::Party),
        guard: of(Side::Foe),
    }
}

/// What a won raid takes from the posted stock.
fn loot(w: &World, o: &Order, survivors: usize, half: bool) -> (Vec<(Resource, u32)>, u32) {
    let Some(b) = w.clans.get(usize::from(o.to)) else {
        return (Vec::new(), 0);
    };
    let mut carry = caps::CARRY_PER_RAIDER * u32::try_from(survivors).unwrap_or(0);
    let mut budget = if half {
        caps::LOOT_MAX_VALUE / 2
    } else {
        caps::LOOT_MAX_VALUE
    };
    let mut goods: Vec<Resource> = Resource::ALL
        .iter()
        .copied()
        .filter(|r| *r != Resource::Relic)
        .collect();
    goods.sort_by_key(|r| std::cmp::Reverse((r.base_price(), r.index())));
    let mut out = Vec::new();
    for r in goods {
        let price = u32::from(r.base_price());
        let have = u32::from(b.posted.stock[r.index()]);
        let n = (have * caps::LOOT_PCT / 100)
            .min(carry)
            .min(budget / price.max(1));
        if n > 0 {
            carry -= n;
            budget -= n * price;
            out.push((r, n));
        }
    }
    // Coin is light: tallies do not count against what the raiders can carry.
    let tallies = (u32::from(b.posted.tallies) * caps::LOOT_TALLY_PCT / 100).min(budget);
    (out, tallies)
}

fn fates(raiders: &[(u16, i16, i16)], verdict: Verdict, rng: &mut DoorRng) -> Vec<(u16, Fate)> {
    raiders
        .iter()
        .map(|&(id, hp, max)| {
            let fate = if hp <= 0 {
                let death = if verdict == Verdict::Lost { 35 } else { 8 };
                if rng.chance(death, 100) {
                    Fate::Dead
                } else {
                    Fate::Hurt
                }
            } else if i32::from(hp) * 100 < i32::from(max) * 30 && rng.chance(3, 10) {
                Fate::Hurt
            } else {
                Fate::Unhurt
            };
            (id, fate)
        })
        .collect()
}

fn hurt_guard(guard: &[(u16, i16, i16)], rng: &mut DoorRng) -> Vec<u16> {
    guard
        .iter()
        .filter(|&&(_, hp, max)| {
            hp <= 0 || (i32::from(hp) * 100 < i32::from(max) * 30 && rng.chance(3, 10))
        })
        .map(|&(id, _, _)| id)
        .collect()
}

/// Settle every order sent before `day`, oldest first. The tick calls this; it needs only the
/// world, the content, the season's tide and the tick's dice.
pub fn resolve_all(w: &mut World, cat: &Catalog, tide: &Tide, day: i64, rng: &mut DoorRng) {
    let (due, later): (Vec<Order>, Vec<Order>) = std::mem::take(&mut w.social.orders)
        .into_iter()
        .partition(|o| o.day < day);
    w.social.orders = later;
    for o in due {
        resolve(w, cat, tide, &o, rng);
    }
}

#[allow(clippy::many_single_char_names, clippy::too_many_lines)]
fn resolve(w: &mut World, cat: &Catalog, tide: &Tide, o: &Order, rng: &mut DoorRng) {
    let (Some(a), Some(b)) = (
        w.clans.get(usize::from(o.from)).map(|c| c.name.clone()),
        w.clans.get(usize::from(o.to)).map(|c| c.name.clone()),
    ) else {
        return;
    };
    let land = o.day + 1;
    let win = windows(tide, land)
        .get(usize::from(o.window))
        .copied()
        .or_else(|| windows(tide, land).first().copied());
    let (minute, level) = win.map_or((0, 50), |w| (w.minute, w.level));
    let report = |role,
                  outcome,
                  rounds,
                  flooded,
                  loot: &[(Resource, u32)],
                  tallies,
                  fates: &[(u16, Fate)],
                  hurt: &[u16]| {
        Report {
            order: o.id,
            role,
            outcome,
            raider: a.clone(),
            defender: b.clone(),
            minute,
            level: i16::try_from(level).unwrap_or(50),
            flooded,
            rounds,
            loot: loot.to_vec(),
            tallies,
            fates: fates.to_vec(),
            hurt: hurt.to_vec(),
            taunt: o.taunt.clone(),
        }
    };
    // A pact signed while the raid was on the road calls it off, and nobody is hurt.
    if w.pact_between(o.from, o.to).is_some() {
        let safe: Vec<(u16, Fate)> = o.raiders.iter().map(|d| (d.id, Fate::Unhurt)).collect();
        w.social.deliver(
            o.from,
            Body::Raid(report(
                Role::Raider,
                Outcome::Recalled,
                0,
                false,
                &[],
                0,
                &safe,
                &[],
            )),
        );
        return;
    }
    let flooded = w
        .clans
        .get(usize::from(o.to))
        .is_some_and(|c| c.posted.sluice.is_some_and(|s| level >= i32::from(s)));
    let guard = guard_units(w, o.to, flooded);
    let f = fight(cat, o, guard, flooded, level, rng);
    let won = f.verdict == Verdict::Won;
    let rounds = f.rounds;
    let raider_fates = fates(&f.raiders, f.verdict, rng);
    let hurt = hurt_guard(&f.guard, rng);
    let survivors = raider_fates
        .iter()
        .filter(|(_, fate)| *fate != Fate::Dead)
        .count();
    let (stolen, tallies) = if won {
        let (seen, score_a, score_b) = {
            let tb = &w.clans[usize::from(o.to)];
            (tb.posted.seen, w.clans[usize::from(o.from)].score, tb.score)
        };
        let half = land - seen >= caps::DORMANT_HALF_DAYS || score_b * 2 < score_a;
        loot(w, o, survivors, half)
    } else {
        (Vec::new(), 0)
    };
    let value: u32 = stolen
        .iter()
        .map(|(r, n)| n * u32::from(r.base_price()))
        .sum::<u32>()
        + tallies;
    let outcome = if won { Outcome::Won } else { Outcome::Repelled };
    // The post shrinks by what was taken, so the next raider sees a poorer hold.
    if let Some(t) = w.clans.get_mut(usize::from(o.to)) {
        for (r, n) in &stolen {
            let slot = &mut t.posted.stock[r.index()];
            *slot = slot.saturating_sub(u16::try_from(*n).unwrap_or(u16::MAX));
        }
        t.posted.tallies = t
            .posted
            .tallies
            .saturating_sub(u16::try_from(tallies).unwrap_or(u16::MAX));
        if won {
            t.record.raided = t.record.raided.saturating_add(1);
            t.record.shield_until = land + caps::SHIELD_DAYS;
        } else {
            t.record.repelled = t.record.repelled.saturating_add(1);
        }
    }
    if let Some(r) = w.clans.get_mut(usize::from(o.from)) {
        if won {
            r.record.raids_won = r.record.raids_won.saturating_add(1);
            r.record.haul = r.record.haul.saturating_add(value);
        } else {
            r.record.raids_lost = r.record.raids_lost.saturating_add(1);
        }
    }
    w.social.raid_log.push(LogEntry {
        from: o.from,
        to: o.to,
        day: o.day,
    });
    let extra = w.social.raid_log.len().saturating_sub(caps::RAID_LOG);
    w.social.raid_log.drain(..extra);
    w.social.note(
        land,
        if won {
            DeedKind::RaidWon
        } else {
            DeedKind::RaidLost
        },
        o.from,
        o.to,
    );
    w.social.deliver(
        o.from,
        Body::Raid(report(
            Role::Raider,
            outcome,
            rounds,
            flooded,
            &stolen,
            tallies,
            &raider_fates,
            &hurt,
        )),
    );
    w.social.deliver(
        o.to,
        Body::Raid(report(
            Role::Defender,
            outcome,
            rounds,
            flooded,
            &stolen,
            tallies,
            &raider_fates,
            &hurt,
        )),
    );
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::assert_is_empty)]
    use super::*;
    use crate::types::Trade;

    fn fighter(id: u16, name: &str, hp: i16, atk: i16) -> Defender {
        Defender {
            id,
            name: name.into(),
            trade: Trade::Fighter,
            hp,
            atk,
            def: 1,
            spd: 4,
            mass: 6,
            row: 0,
            aquatic: false,
            armed: false,
        }
    }

    fn world(day: i64) -> World {
        let mut w = World::new(1, 7, day - 30);
        for n in ["Ash", "Brindle", "Cove"] {
            w.register(n, 0);
        }
        for c in &mut w.clans {
            c.posted.founded = day - 30;
            c.posted.seen = day;
            c.posted.stock[Resource::Salt.index()] = 100;
            c.posted.stock[Resource::Iron.index()] = 40;
            c.posted.tallies = 200;
            c.posted.guard = vec![fighter(1, "Holt", 12, 3)];
            c.score = 50;
        }
        w
    }

    fn mail_for(w: &World, to: ClanIx) -> Vec<Report> {
        w.social
            .mail
            .iter()
            .filter(|d| d.to == to)
            .filter_map(|d| match &d.body {
                Body::Raid(r) => Some(r.clone()),
                _ => None,
            })
            .collect()
    }

    fn strong() -> Vec<Defender> {
        vec![fighter(7, "Ysolt", 60, 12), fighter(8, "Brand", 60, 12)]
    }

    #[test]
    fn a_strong_party_beats_a_thin_guard_and_both_sides_are_told() {
        let cat = Catalog::embedded().unwrap();
        let tide = Tide::new(7);
        let mut w = world(20_000);
        let id = place(&mut w, &tide, 0, 1, 20_000, 0, 1, strong(), "").unwrap();
        resolve_all(&mut w, &cat, &tide, 20_000, &mut DoorRng::from_seed(1));
        assert_eq!(w.social.orders.len(), 1, "not before the night");
        resolve_all(&mut w, &cat, &tide, 20_001, &mut DoorRng::from_seed(1));
        assert!(w.social.orders.is_empty());
        let (r, d) = (mail_for(&w, 0), mail_for(&w, 1));
        assert_eq!((r.len(), d.len()), (1, 1));
        assert_eq!((r[0].role, d[0].role), (Role::Raider, Role::Defender));
        assert_eq!((r[0].order, r[0].outcome), (id, Outcome::Won));
        assert_eq!(r[0].loot, d[0].loot, "one result, two copies");
        assert!(!r[0].loot.is_empty() && r[0].tallies > 0);
        // The post is poorer by what was taken, the hold is shielded, the books agree.
        let stolen_salt = r[0]
            .loot
            .iter()
            .find(|l| l.0 == Resource::Salt)
            .map_or(0, |l| l.1);
        assert_eq!(
            u32::from(w.clans[1].posted.stock[Resource::Salt.index()]) + stolen_salt,
            100
        );
        assert_eq!(w.clans[1].record.shield_until, 20_001 + caps::SHIELD_DAYS);
        assert_eq!(
            (w.clans[0].record.raids_won, w.clans[1].record.raided),
            (1, 1)
        );
        assert_eq!(w.social.raid_log.len(), 1);
    }

    #[test]
    fn a_thin_party_is_beaten_off_and_the_defender_keeps_everything() {
        let cat = Catalog::embedded().unwrap();
        let tide = Tide::new(7);
        let mut w = world(20_000);
        w.clans[1].posted.guard = vec![fighter(1, "Holt", 60, 12), fighter(2, "Nan", 60, 12)];
        w.clans[1].posted.rating = 6;
        place(
            &mut w,
            &tide,
            0,
            1,
            20_000,
            0,
            1,
            vec![fighter(7, "Tam", 8, 2)],
            "",
        )
        .unwrap();
        resolve_all(&mut w, &cat, &tide, 20_001, &mut DoorRng::from_seed(2));
        let r = mail_for(&w, 0);
        assert_eq!(r[0].outcome, Outcome::Repelled);
        assert!(r[0].loot.is_empty() && r[0].tallies == 0);
        assert_eq!(w.clans[1].posted.tallies, 200);
        assert_eq!(
            (w.clans[0].record.raids_lost, w.clans[1].record.repelled),
            (1, 1)
        );
        assert_eq!(
            w.clans[1].record.shield_until, 0,
            "a hold that held is not shielded"
        );
    }

    #[test]
    fn the_same_night_gives_the_same_result() {
        let cat = Catalog::embedded().unwrap();
        let tide = Tide::new(7);
        let run = || {
            let mut w = world(20_000);
            place(&mut w, &tide, 0, 1, 20_000, 1, 1, strong(), "").unwrap();
            resolve_all(&mut w, &cat, &tide, 20_001, &mut DoorRng::from_seed(5));
            w
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_flooded_approach_helps_the_defender() {
        let cat = Catalog::embedded().unwrap();
        let tide = Tide::new(7);
        let w = world(20_000);
        let high = windows(&tide, 20_001).into_iter().find(|x| x.high).unwrap();
        let mut dry = w.clone();
        dry.clans[1].posted.rating = 4;
        let mut wet = dry.clone();
        wet.clans[1].posted.flood = 8;
        wet.clans[1].posted.sluice = Some(40);
        let party = vec![fighter(7, "Tam", 30, 6), fighter(8, "Ann", 30, 6)];
        let odds_dry = preview_odds(&cat, &dry, 1, &party, high.level);
        let odds_wet = preview_odds(&cat, &wet, 1, &party, high.level);
        let rank = |o: crate::combat::Odds| match o {
            crate::combat::Odds::Easy => 3,
            crate::combat::Odds::Fair => 2,
            crate::combat::Odds::Hard => 1,
            crate::combat::Odds::Grim => 0,
        };
        assert!(
            rank(odds_wet) <= rank(odds_dry),
            "{odds_dry:?} {odds_wet:?}"
        );
        // The flooded guard is stronger unit by unit.
        let a = guard_units(&dry, 1, false);
        let b = guard_units(&wet, 1, true);
        assert!(b[0].hp > a[0].hp && b[0].def > a[0].def);
    }

    #[test]
    fn high_water_needs_a_boat_and_the_low_does_not() {
        let tide = Tide::new(7);
        let mut w = world(20_000);
        let wins = windows(&tide, 20_001);
        let high = wins.iter().find(|x| x.high).unwrap();
        let low = wins.iter().find(|x| !x.high).unwrap();
        assert!(high.needs_boat && !low.needs_boat);
        assert_eq!(
            place(&mut w, &tide, 0, 1, 20_000, high.index, 0, strong(), ""),
            Err(Refusal::Window)
        );
        assert!(place(&mut w, &tide, 0, 1, 20_000, low.index, 0, strong(), "").is_ok());
        assert_eq!(
            place(&mut w, &tide, 0, 2, 20_000, 9, 1, strong(), ""),
            Err(Refusal::Window)
        );
        assert_eq!(
            place(&mut w, &tide, 0, 2, 20_000, low.index, 0, vec![], ""),
            Err(Refusal::BadParty)
        );
    }

    #[test]
    fn the_caps_stop_a_player_from_griefing_another() {
        let tide = Tide::new(7);
        let day = 20_000;
        // Per target per day, per raider per day, per pair, and the shield.
        let mut w = world(day);
        for n in ["Dun", "Eel", "Fen"] {
            w.register(n, 0);
            let c = w.clans.last_mut().unwrap();
            c.posted.founded = day - 30;
            c.posted.seen = day;
            c.score = 50;
        }
        place(&mut w, &tide, 0, 2, day, 0, 1, strong(), "").unwrap();
        place(&mut w, &tide, 1, 2, day, 0, 1, strong(), "").unwrap();
        assert_eq!(
            check(&w, 3, 2, day),
            Err(Refusal::HitsToday),
            "a hold is struck twice a day"
        );
        place(&mut w, &tide, 0, 3, day, 0, 1, strong(), "").unwrap();
        assert_eq!(
            check(&w, 0, 4, day),
            Err(Refusal::RaidsToday),
            "two raids a day"
        );
        assert_eq!(
            check(&w, 0, 2, day + 1),
            Err(Refusal::PairCooldown),
            "not the same pair again"
        );
        w.clans[4].record.shield_until = day + 1;
        assert_eq!(check(&w, 5, 4, day), Err(Refusal::Shielded));
        assert_eq!(check(&w, 5, 5, day), Err(Refusal::Unknown));
        assert_eq!(check(&w, 5, 99, day), Err(Refusal::Unknown));
    }

    #[test]
    fn new_clans_dormant_clans_small_clans_and_pact_partners_are_left_alone() {
        let day = 20_000;
        let mut w = world(day);
        w.clans[1].posted.founded = day - 3;
        assert_eq!(check(&w, 0, 1, day), Err(Refusal::NewClan));
        // A new clan that raids first loses its protection.
        w.clans[1].record.raided_first = true;
        assert_eq!(check(&w, 0, 1, day), Ok(()));
        w.clans[1].posted.seen = day - caps::DORMANT_SAFE_DAYS;
        assert_eq!(check(&w, 0, 1, day), Err(Refusal::Dormant));
        w.clans[1].posted.seen = day;
        w.clans[0].score = 90;
        w.clans[1].score = 20;
        assert_eq!(check(&w, 0, 1, day), Err(Refusal::Beneath));
        w.clans[1].score = 30;
        assert_eq!(check(&w, 0, 1, day), Ok(()));
        // A pact forbids it, and so does the memory of one just broken.
        w.clans[0].posted.buildings = 4;
        let id = w.propose(0, 1, crate::social::pact::TRUCE, day).unwrap();
        w.accept(1, id, day).unwrap();
        assert_eq!(check(&w, 0, 1, day), Err(Refusal::Pact));
        w.break_pact(0, id, day).unwrap();
        assert_eq!(check(&w, 0, 1, day), Err(Refusal::Pact));
        assert_eq!(check(&w, 0, 1, day + caps::PAIR_COOLDOWN_DAYS), Ok(()));
    }

    #[test]
    fn a_pact_signed_while_the_raid_is_on_the_road_calls_it_off() {
        let cat = Catalog::embedded().unwrap();
        let tide = Tide::new(7);
        let mut w = world(20_000);
        w.clans[0].posted.buildings = 4;
        place(&mut w, &tide, 0, 1, 20_000, 0, 1, strong(), "").unwrap();
        let id = w.propose(0, 1, crate::social::pact::TRUCE, 20_000).unwrap();
        w.accept(1, id, 20_000).unwrap();
        resolve_all(&mut w, &cat, &tide, 20_001, &mut DoorRng::from_seed(1));
        let r = mail_for(&w, 0);
        assert_eq!(r[0].outcome, Outcome::Recalled);
        assert!(r[0].fates.iter().all(|f| f.1 == Fate::Unhurt));
        assert!(mail_for(&w, 1).is_empty(), "the hold never knew");
        assert_eq!(w.clans[1].posted.tallies, 200);
    }

    #[test]
    fn loot_is_capped_by_carry_by_value_and_by_what_is_posted() {
        let mut w = world(20_000);
        w.clans[1].posted.stock = [u16::MAX; crate::types::RESOURCES];
        w.clans[1].posted.tallies = u16::MAX;
        let o = Order {
            id: 1,
            from: 0,
            to: 1,
            day: 20_000,
            window: 0,
            boats: 0,
            raiders: strong(),
            taunt: String::new(),
        };
        let (goods, tallies) = loot(&w, &o, 2, false);
        let items: u32 = goods.iter().map(|g| g.1).sum::<u32>();
        assert!(items <= 2 * caps::CARRY_PER_RAIDER, "{items}");
        let value: u32 = goods
            .iter()
            .map(|(r, n)| n * u32::from(r.base_price()))
            .sum::<u32>()
            + tallies;
        assert!(value <= caps::LOOT_MAX_VALUE, "{value}");
        assert!(!goods.iter().any(|g| g.0 == Resource::Relic));
        let (half, _) = loot(&w, &o, 2, true);
        let half_value: u32 = half
            .iter()
            .map(|(r, n)| n * u32::from(r.base_price()))
            .sum();
        assert!(half_value <= caps::LOOT_MAX_VALUE / 2);
        // An empty post yields nothing.
        w.clans[1].posted.stock = [0; crate::types::RESOURCES];
        w.clans[1].posted.tallies = 0;
        assert_eq!(loot(&w, &o, 2, false), (vec![], 0));
    }

    #[test]
    fn a_report_round_trips_through_the_wire() {
        let r = Report {
            order: 9,
            role: Role::Defender,
            outcome: Outcome::Won,
            raider: "Ash".into(),
            defender: "Brindle".into(),
            minute: 410,
            level: 71,
            flooded: true,
            rounds: 6,
            loot: vec![(Resource::Iron, 3)],
            tallies: 12,
            fates: vec![(7, Fate::Dead), (8, Fate::Hurt)],
            hurt: vec![1, 2],
            taunt: "taunt-keeps".into(),
        };
        let mut w = Writer::new();
        r.write(&mut w);
        let bytes = w.finish();
        assert_eq!(Report::read(&mut Reader::new(&bytes)), Some(r));
    }
}
