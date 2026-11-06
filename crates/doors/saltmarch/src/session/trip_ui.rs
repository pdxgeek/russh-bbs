//! The plain-text screens for expeditions: the planner, the march, scenes and skirmishes. Like
//! the rest of `session`, these are a debug view that also serves as the look-around; the real
//! screens are the UI slice. Every line is plain text, and the menu tags (`party:3`, `march`,
//! `choose:1`, `strike:4`) are what the simulation bots read.

use std::sync::Arc;

use russh_bbs_door::DoorStyle;

use super::{Act, Entry, Session, entry};
use crate::clan::Clan;
use crate::combat::{Fight, FightAction, Side};
use crate::engine::Action;
use crate::lore::{Daypart, Water};
use crate::tide::Access;
use crate::trip::{
    self, Dir, Kit, MAX_BOATS, MAX_PARTY, Phase, Reach, Trip, TripAction, safe_level,
};
use crate::types::{Resource, Trade};

/// The plan being made at the gate.
#[derive(Debug, Clone, Default)]
pub(super) struct Plan {
    pub(super) party: Vec<u16>,
    pub(super) kit: Kit,
    pub(super) target: Option<(u8, u8)>,
}

/// What a scene option shows.
pub(super) struct OptionInfo {
    pub(super) label: String,
    pub(super) risk: Option<u32>,
    pub(super) fight: &'static str,
    pub(super) skill: Option<Trade>,
}

/// What the planner can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Edit {
    Crew(u16),
    Preset(Preset),
    Target(Option<(u8, u8)>),
}

/// A ready-made kit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Preset {
    Light,
    Standard,
    Heavy,
}

impl Preset {
    fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Standard => "standard",
            Self::Heavy => "heavy",
        }
    }
}

fn clamp(n: usize, have: u32) -> u8 {
    u8::try_from(u32::try_from(n).unwrap_or(0).min(have)).unwrap_or(u8::MAX)
}

/// The kit a preset makes for `crew` people from the clan's stores.
pub(super) fn kit_for(clan: &Clan, crew: usize, preset: Preset) -> Kit {
    let have = |r| clan.stock.get(r);
    match preset {
        Preset::Light => Kit {
            food: clamp(crew, have(Resource::Food)),
            peat: clamp(1, have(Resource::Peat)),
            ..Kit::default()
        },
        Preset::Standard => Kit {
            food: clamp(crew * 2, have(Resource::Food)),
            rope: clamp(2, have(Resource::Rope)),
            peat: clamp(2, have(Resource::Peat)),
            tools: clamp(1, have(Resource::Tools)),
            ..Kit::default()
        },
        Preset::Heavy => Kit {
            food: clamp(crew * 3, have(Resource::Food)),
            rope: clamp(3, have(Resource::Rope)),
            peat: clamp(3, have(Resource::Peat)),
            tools: clamp(2, have(Resource::Tools)),
            arms: clamp(crew, have(Resource::Arms)),
            boats: clamp(1, have(Resource::Boats)).min(MAX_BOATS),
        },
    }
}

fn kit_text(kit: Kit) -> String {
    let parts: Vec<String> = kit
        .pairs()
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(r, n)| format!("{n} {}", r.name()))
        .collect();
    if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join(", ")
    }
}

/// Wrap a paragraph to `width` columns.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn access_word(a: Access) -> &'static str {
    match a {
        Access::Dry => "dry",
        Access::Wet => "knee-deep",
        Access::Drowned => "under water",
    }
}

impl Session {
    /// The charted places, nearest the hold first.
    pub(super) fn known_places(&self, clan: &Clan) -> Vec<((u8, u8), String)> {
        let env = self.env();
        let (hx, hy) = trip::home(&env, clan);
        let mut out: Vec<((u8, u8), String, u32)> = self
            .map
            .pois
            .iter()
            .filter(|p| clan.is_charted(usize::from(p.x), usize::from(p.y)))
            .map(|p| {
                let name = self
                    .cat
                    .pois
                    .get(usize::from(p.template))
                    .map_or("a place", |t| t.name.as_str());
                let d = u32::from(p.x.abs_diff(hx)) + u32::from(p.y.abs_diff(hy));
                ((p.x, p.y), name.to_owned(), d)
            })
            .collect();
        out.sort_by_key(|(at, _, d)| (*d, at.1, at.0));
        out.into_iter().map(|(at, name, _)| (at, name)).collect()
    }

    /// The search of the marsh from where the party stands, redone only when something it
    /// depends on has changed.
    pub(super) fn reach_of(&self, clan: &Clan, t: &Trip) -> Arc<Reach> {
        let env = self.env();
        let key = Reach::key(&env, clan, t);
        if let Ok(mut cache) = self.reach.lock() {
            if let Some((k, r)) = cache.as_ref()
                && *k == key
            {
                return Arc::clone(r);
            }
            let fresh = Arc::new(Reach::new(&env, clan, t));
            *cache = Some((key, Arc::clone(&fresh)));
            return fresh;
        }
        Arc::new(Reach::new(&env, clan, t))
    }

    pub(super) fn plan_entries(&self, clan: &Clan) -> Vec<Entry> {
        let cat = &*self.cat;
        let mut out = Vec::new();
        if !self.plan.party.is_empty() {
            out.push(entry(
                "depart",
                format!(
                    "Set out with {} (kit: {})",
                    self.plan.party.len(),
                    kit_text(self.plan.kit)
                ),
                Act::Do(Action::Depart {
                    party: self.plan.party.clone(),
                    kit: self.plan.kit,
                    target: self.plan.target,
                }),
            ));
        }
        for c in clan.crew.iter().filter(|c| c.fit(cat)) {
            let on = self.plan.party.contains(&c.id);
            out.push(entry(
                format!("party:{}", c.id),
                format!(
                    "[{}] {} the {} ({} hurt)",
                    if on { "x" } else { " " },
                    c.name,
                    c.trade.name(),
                    c.injuries.len()
                ),
                Act::Edit(Edit::Crew(c.id)),
            ));
        }
        for p in [Preset::Light, Preset::Standard, Preset::Heavy] {
            out.push(entry(
                format!("kit:{}", p.name()),
                format!(
                    "Kit: {} ({})",
                    p.name(),
                    kit_text(kit_for(clan, self.plan.party.len().max(1), p))
                ),
                Act::Edit(Edit::Preset(p)),
            ));
        }
        for ((x, y), name) in self.known_places(clan).into_iter().take(6) {
            out.push(entry(
                format!("target:{x},{y}"),
                format!("Aim for {name} at {x},{y}"),
                Act::Edit(Edit::Target(Some((x, y)))),
            ));
        }
        out
    }

    /// The trip the plan would make, standing at the hold with nothing in the cargo: for the
    /// carrying capacity and the cost preview.
    pub(super) fn plan_trip(&self, clan: &Clan) -> Trip {
        Trip {
            party: self.plan.party.clone(),
            kit: self.plan.kit,
            loot: crate::types::Stock::default(),
            tallies: 0,
            at: trip::home(&self.env(), clan),
            target: self.plan.target,
            turns_out: 0,
            night_turns: 0,
            phase: Phase::Free,
            ruin: None,
            visited: Vec::new(),
            worked: Vec::new(),
            hardship: 0,
            scenes: 0,
            fights: 0,
            rng: [1; 4],
        }
    }

    pub(super) fn plan_lines(&self, clan: &Clan) -> Vec<(String, DoorStyle)> {
        let names: Vec<String> = self
            .plan
            .party
            .iter()
            .filter_map(|id| clan.crew_by(*id))
            .map(|c| c.name.clone())
            .collect();
        let cap = self.plan_trip(clan).capacity(clan, &self.cat);
        let target = self.plan.target.map_or_else(
            || "none".to_owned(),
            |(x, y)| {
                self.map
                    .poi_at(x, y)
                    .and_then(|p| self.cat.pois.get(usize::from(p.template)))
                    .map_or(format!("{x},{y}"), |t| format!("{} ({x},{y})", t.name))
            },
        );
        vec![
            (
                format!(
                    "Plan an expedition: pick 1 to {MAX_PARTY} crew, a kit and a place. Party: {}",
                    if names.is_empty() {
                        "nobody yet".to_owned()
                    } else {
                        names.join(", ")
                    }
                ),
                DoorStyle::Strong,
            ),
            (
                format!(
                    "Kit: {}. Can carry {cap}. Target: {target}.",
                    kit_text(self.plan.kit)
                ),
                DoorStyle::Plain,
            ),
        ]
    }

    pub(super) fn edit(&mut self, e: Edit) {
        let Some(clan) = &self.clan else { return };
        match e {
            Edit::Crew(id) => {
                if let Some(at) = self.plan.party.iter().position(|p| *p == id) {
                    self.plan.party.remove(at);
                } else if self.plan.party.len() < MAX_PARTY {
                    self.plan.party.push(id);
                }
                // The kit follows the party's size if it was a preset.
                let n = self.plan.party.len().max(1);
                self.plan.kit = kit_for(clan, n, Preset::Standard);
            }
            Edit::Preset(p) => {
                self.plan.kit = kit_for(clan, self.plan.party.len().max(1), p);
            }
            Edit::Target(t) => self.plan.target = t,
        }
    }

    // ------------------------------------------------------------------ on the marsh

    pub(super) fn trip_entries(&self, clan: &Clan) -> Vec<Entry> {
        let Some(t) = &clan.trip else {
            return Vec::new();
        };
        match &t.phase {
            Phase::Scene(s) => self.scene_entries(s),
            Phase::Fight(f) => Self::fight_entries(f),
            Phase::Free => self.march_entries(clan, t),
        }
    }

    /// What the screen says about option `i` of a scene: its words, the chance it goes badly
    /// (only when there is one) and a warning when it can start a fight.
    pub(super) fn option_info(
        &self,
        clan: &Clan,
        t: &Trip,
        s: &trip::Scene,
        i: u8,
    ) -> Option<OptionInfo> {
        let def = self.cat.event(&s.event)?;
        let o = def.options.get(usize::from(i))?;
        let risk =
            trip::fail_chance(&self.cat, clan, t, &s.event, usize::from(i)).filter(|p| *p > 0);
        // A choice that can end in a fight says so: the player should know.
        let fights = |out: &crate::lore::Outcome| {
            out.fx
                .iter()
                .any(|e| matches!(e, crate::lore::Effect::Fight(_)))
        };
        let fight = if fights(&o.win) {
            " (starts a fight)"
        } else if o.lose.as_ref().is_some_and(fights) {
            " (a bad turn starts a fight)"
        } else {
            ""
        };
        Some(OptionInfo {
            label: o.label.clone(),
            risk,
            fight,
            skill: o.skill,
        })
    }

    fn scene_entries(&self, s: &trip::Scene) -> Vec<Entry> {
        let pass = || {
            vec![entry(
                "choose:0",
                "Let the moment pass",
                Act::Do(Action::Go(TripAction::Choose(0))),
            )]
        };
        if self.cat.event(&s.event).is_none() {
            return pass();
        }
        let Some(clan) = &self.clan else {
            return Vec::new();
        };
        let Some(t) = &clan.trip else {
            return Vec::new();
        };
        s.offered
            .iter()
            .enumerate()
            .filter_map(|(n, &i)| {
                let info = self.option_info(clan, t, s, i)?;
                let risk = info
                    .risk
                    .map_or(String::new(), |p| format!(" [{p}% to go badly]"));
                Some(entry(
                    format!("choose:{n}"),
                    format!("{}{risk}{}", info.label, info.fight),
                    Act::Do(Action::Go(TripAction::Choose(u8::try_from(n).unwrap_or(0)))),
                ))
            })
            .collect()
    }

    fn fight_entries(f: &Fight) -> Vec<Entry> {
        let mut out = Vec::new();
        let Some(&slot) = f.order.get(usize::from(f.cursor)) else {
            return out;
        };
        let me = usize::from(slot);
        let go = |a: FightAction| Act::Do(Action::Go(TripAction::Fight(a)));
        let reach = f.in_reach(me);
        let melee = f.melee_targets(me);
        let foes: Vec<usize> = f.alive(Side::Foe).collect();
        let idx = |i: usize| u8::try_from(i).unwrap_or(0);
        let who = &f.units[me];
        for &t in &melee {
            if reach {
                out.push(entry(
                    format!("strike:{t}"),
                    format!("{} strikes {}", who.name, f.units[t].name),
                    go(FightAction::Strike { target: idx(t) }),
                ));
            }
        }
        if who.trade.is_some() && who.cool == 0 {
            let needs_target =
                !matches!(who.trade, Some(Trade::Raker | Trade::Healer | Trade::Smith));
            if needs_target {
                for &t in &foes {
                    out.push(entry(
                        format!("special:{t}"),
                        format!("Special on {}", f.units[t].name),
                        go(FightAction::Special {
                            target: Some(idx(t)),
                        }),
                    ));
                }
            } else {
                out.push(entry(
                    "special",
                    "Special (the whole line)",
                    go(FightAction::Special { target: None }),
                ));
            }
        }
        out.push(entry("brace", "Brace", go(FightAction::Brace)));
        out.push(entry(
            "flee",
            "Flee (costs part of the cargo)",
            go(FightAction::Flee),
        ));
        out.push(entry(
            "quick",
            "Quick-resolve the rest of the fight",
            Act::Do(Action::Go(TripAction::Quick)),
        ));
        for &t in &melee {
            if reach {
                out.push(entry(
                    format!("shove:{t}"),
                    format!("Shove {}", f.units[t].name),
                    go(FightAction::Shove { target: idx(t) }),
                ));
            }
        }
        for &t in &foes {
            out.push(entry(
                format!("throw:{t}"),
                format!("Throw at {}", f.units[t].name),
                go(FightAction::Throw { target: idx(t) }),
            ));
        }
        for (i, u) in f.units.iter().enumerate() {
            if u.side == Side::Party && !u.gone && u.hp < u.max_hp {
                out.push(entry(
                    format!("mend:{i}"),
                    format!("Mend {}", u.name),
                    go(FightAction::Mend { ally: idx(i) }),
                ));
            }
        }
        out
    }

    #[allow(clippy::too_many_lines)]
    fn march_entries(&self, clan: &Clan, t: &Trip) -> Vec<Entry> {
        let env = self.env();
        let mut out = Vec::new();
        let home = trip::home(&env, clan);
        let reach = self.reach_of(clan, t);
        if let Some(to) = t.target
            && let Some(cost) = reach.cost(to)
        {
            out.push(entry(
                "march",
                format!("March to {},{} ({cost} turns)", to.0, to.1),
                Act::Do(Action::Go(TripAction::March { to: None })),
            ));
        }
        if t.at == home {
            out.push(entry(
                "home",
                "Stand down and unload (no turns)",
                Act::Do(Action::Go(TripAction::March { to: Some(home) })),
            ));
        } else if let Some(cost) = reach.cost(home) {
            out.push(entry(
                "home",
                format!("March home ({cost} turns)"),
                Act::Do(Action::Go(TripAction::March { to: Some(home) })),
            ));
        }
        let tile = env.map.tile(usize::from(t.at.0), usize::from(t.at.1));
        if let Some(tile) = tile
            && t.at != home
            && self.cat.gather(tile.kind).is_some()
            && t.worked.iter().all(|(x, y, n)| (*x, *y) != t.at || *n < 2)
        {
            out.push(entry(
                "gather",
                format!("Work the {} here (2 turns)", tile.kind.name()),
                Act::Do(Action::Go(TripAction::Gather)),
            ));
        }
        if let Some(tile) = tile
            && t.at != home
            && crate::map::can_lay_on(tile.kind)
            && env.map.poi_at(t.at.0, t.at.1).is_none()
            && t.kit.rope >= crate::trip::LAY_ROPE
        {
            out.push(entry(
                "lay",
                format!(
                    "Lay a causeway tile here ({} rope, {} turns, toll {})",
                    crate::trip::LAY_ROPE,
                    crate::trip::LAY_TURNS,
                    clan.story.toll
                ),
                Act::Do(Action::Go(TripAction::Lay)),
            ));
        }
        if let Some(r) = t.ruin
            && r.room < r.rooms
        {
            out.push(entry(
                "delve",
                format!(
                    "Go deeper into the ruin (room {} of {}, 2 turns)",
                    r.room + 1,
                    r.rooms
                ),
                Act::Do(Action::Go(TripAction::Delve)),
            ));
        }
        for dir in Dir::ALL {
            if let Some(to) = dir.step(t.at)
                && env.map.tile(usize::from(to.0), usize::from(to.1)).is_some()
            {
                out.push(entry(
                    format!("step:{}", dir.letter()),
                    format!("Step {}", dir.name()),
                    Act::Do(Action::Go(TripAction::Step(dir))),
                ));
            }
        }
        for dir in Dir::ALL {
            // The farthest tile up to five away in this direction that the party can reach.
            let mut line = Vec::new();
            let mut at = t.at;
            for _ in 0..5 {
                match dir.step(at) {
                    Some(n) => {
                        at = n;
                        line.push(n);
                    }
                    None => break,
                }
            }
            if let Some(to) = line.into_iter().rev().find(|n| reach.cost(*n).is_some()) {
                out.push(entry(
                    format!("explore:{}", dir.letter()),
                    format!("Strike out {} to {},{}", dir.name(), to.0, to.1),
                    Act::Do(Action::Go(TripAction::March { to: Some(to) })),
                ));
            }
        }
        out.push(entry(
            "camp:3",
            "Make camp for three turns and let the water move",
            Act::Do(Action::Go(TripAction::Camp { turns: 3 })),
        ));
        if t.overloaded(clan, &self.cat) {
            let heaviest = Resource::ALL
                .into_iter()
                .filter(|r| t.loot.get(*r) > 0)
                .max_by_key(|r| (t.loot.get(*r) * trip::weight(*r), r.index()));
            if let Some(r) = heaviest {
                out.push(entry(
                    format!("drop:{}", r.id()),
                    format!("Drop {} {} to lighten the load", t.loot.get(r), r.name()),
                    Act::Do(Action::Go(TripAction::Jettison {
                        what: r,
                        qty: t.loot.get(r),
                    })),
                ));
            }
        }
        for ((x, y), name) in self.known_places(clan).into_iter().take(5) {
            if t.target != Some((x, y)) && (x, y) != t.at {
                out.push(entry(
                    format!("go:{x},{y}"),
                    format!("Set out for {name} at {x},{y}"),
                    Act::Do(Action::Go(TripAction::March { to: Some((x, y)) })),
                ));
            }
        }
        out
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn trip_lines(&self, clan: &Clan) -> Vec<(String, DoorStyle)> {
        let Some(t) = &clan.trip else {
            return Vec::new();
        };
        let env = self.env();
        let cat = &*self.cat;
        let level = env.level(clan);
        let minute = env.minute(clan);
        let mut out = Vec::new();
        let names: Vec<String> = t
            .party
            .iter()
            .filter_map(|id| clan.crew_by(*id))
            .map(|c| format!("{} ({})", c.name, c.trade.name()))
            .collect();
        out.push((format!("Party: {}", names.join(", ")), DoorStyle::Strong));
        let kind = env
            .map
            .tile(usize::from(t.at.0), usize::from(t.at.1))
            .map_or("nowhere", |x| x.kind.name());
        let (hx, hy) = trip::home(&env, clan);
        let home_eta = if t.at == (hx, hy) {
            "at the hold".to_owned()
        } else {
            self.reach_of(clan, t).cost((hx, hy)).map_or_else(
                || "no safe way home".to_owned(),
                |c| format!("{c} turns home"),
            )
        };
        out.push((
            format!(
                "At {},{} ({kind}). {} ({:02}:{:02}), water {level} ({} tide). {home_eta}.",
                t.at.0,
                t.at.1,
                Daypart::of(minute),
                minute.rem_euclid(1440) / 60,
                minute.rem_euclid(60),
                Water::of(level),
            ),
            DoorStyle::Plain,
        ));
        let ground = match safe_level(&env, t) {
            Some(s) => match trip::turns_until_drowned(&env, clan, t, 12) {
                Some(n) => format!(
                    "This ground goes under at water {}, in about {n} turns.",
                    s + 1
                ),
                None => format!(
                    "This ground goes under at water {}, but not in the next 12 turns.",
                    s + 1
                ),
            },
            None => "A boat keeps the party afloat.".to_owned(),
        };
        out.push((
            format!(
                "Load {}/{}{}. Supplies: {}. Cargo: {}{}. {ground}",
                t.load(),
                t.capacity(clan, cat),
                if t.overloaded(clan, cat) {
                    " OVERLOADED"
                } else {
                    ""
                },
                kit_text(t.kit),
                loot_text(t),
                if t.hungry() { ". HUNGRY" } else { "" },
            ),
            DoorStyle::Plain,
        ));
        match &t.phase {
            Phase::Scene(s) => {
                if let Some(def) = cat.event(&s.event) {
                    out.push((def.title.clone(), DoorStyle::Accent));
                    out.extend(
                        wrap(&def.text, 74)
                            .into_iter()
                            .map(|l| (l, DoorStyle::Plain)),
                    );
                }
            }
            Phase::Fight(f) => out.extend(fight_lines(f)),
            Phase::Free => {
                let mut near = Vec::new();
                for dir in Dir::ALL {
                    let Some((x, y)) = dir.step(t.at) else {
                        continue;
                    };
                    if let Some(tile) = env.map.tile(usize::from(x), usize::from(y)) {
                        let a = Access::of(i32::from(tile.elevation), level);
                        near.push(format!(
                            "{}: {} ({})",
                            dir.name(),
                            tile.kind.name(),
                            access_word(a)
                        ));
                    }
                }
                out.push((format!("Around you - {}", near.join("; ")), DoorStyle::Dim));
            }
        }
        out
    }
}

fn loot_text(t: &Trip) -> String {
    let mut parts: Vec<String> = Resource::ALL
        .iter()
        .filter(|r| t.loot.get(**r) > 0)
        .map(|r| format!("{} {}", t.loot.get(*r), r.name()))
        .collect();
    if t.tallies > 0 {
        parts.push(format!("{} tallies", t.tallies));
    }
    if parts.is_empty() {
        "empty".to_owned()
    } else {
        parts.join(", ")
    }
}

fn fight_lines(f: &Fight) -> Vec<(String, DoorStyle)> {
    let mut out = vec![(
        format!(
            "Skirmish, round {}. Odds: {}. {}{}",
            f.round,
            f.odds().word(),
            if f.wet {
                "Knee-deep in the water. "
            } else {
                "Solid ground. "
            },
            if f.limit > 0 {
                format!("The water takes the field at round {}.", f.limit)
            } else {
                String::new()
            }
        ),
        DoorStyle::Accent,
    )];
    for (side, label) in [(Side::Party, "You"), (Side::Foe, "Foes")] {
        for row in 0..3u8 {
            let here: Vec<String> = f
                .units
                .iter()
                .enumerate()
                .filter(|(_, u)| u.side == side && !u.gone && u.row == row)
                .map(|(i, u)| {
                    format!(
                        "#{i} {} {}/{}{}{}",
                        u.name,
                        u.hp.max(0),
                        u.max_hp,
                        if u.netted > 0 { " netted" } else { "" },
                        if u.hexed > 0 { " hexed" } else { "" },
                    )
                })
                .collect();
            if !here.is_empty() {
                let rank = ["front", "middle", "back"][usize::from(row)];
                out.push((
                    format!("  {label} {rank}: {}", here.join("; ")),
                    if side == Side::Party {
                        DoorStyle::Good
                    } else {
                        DoorStyle::Bad
                    },
                ));
            }
        }
    }
    out
}
