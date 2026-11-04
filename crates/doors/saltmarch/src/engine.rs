//! The rules: a clan plus an action gives a changed clan plus events. Nothing here reads a
//! clock or a random source of its own: the day comes in through [`Env`], randomness through
//! the [`DoorRng`] the caller passes, so a recorded log reproduces a clan exactly.

use std::fmt;

use russh_bbs_door::DoorRng;

use crate::clan::{Clan, Settings};
use crate::content::{Catalog, Cause, EpitaphCause, Learn};
use crate::crew::RETIRE_AGE;
use crate::event::Event;
use crate::hold::Construction;
use crate::map::Map;
use crate::tide::{HOLD_FLOOD_LEVEL, MID, Tide};
use crate::trip::{self, Kit, TripAction};
use crate::types::{Bundle, RESOURCES, Resource, Skill, Track, Trade};
use crate::world::{Prices, World};

/// Turns a shift at a workplace takes.
pub const SHIFT_TURNS: u8 = 2;

/// A shift yields this many nights' worth of the building's output.
pub const SHIFT_NIGHTS: u32 = 3;

/// The most nights replayed for a clan that was away (older ones are skipped).
pub const MAX_CATCH_UP: i64 = 14;

/// Days in a crew year.
pub const AGE_DAYS: i64 = 14;

/// The most crew sent on one shift.
pub const MAX_PARTY: usize = 3;

/// The water is "up" (work sites wet, yields and accident odds change) from this level.
pub const WATER_UP: i32 = MID + 10;

/// What the engine needs to know about the world, all of it passed in.
#[derive(Debug, Clone, Copy)]
pub struct Env<'a> {
    /// Content.
    pub cat: &'a Catalog,
    /// The season's map.
    pub map: &'a Map,
    /// The season's tide.
    pub tide: &'a Tide,
    /// The Tidecourt's prices.
    pub prices: &'a Prices,
    /// Game settings.
    pub settings: Settings,
    /// Today (days since the Unix epoch), by the board's clock.
    pub day: i64,
}

impl Env<'_> {
    /// The minute of the epoch the clan's day has reached: each tide-turn spent moves it on.
    #[must_use]
    pub fn minute(&self, clan: &Clan) -> i64 {
        Tide::turn_minute(self.day, clan.turns_used, self.settings.turns_per_day)
    }

    /// The water level now.
    #[must_use]
    pub fn level(&self, clan: &Clan) -> i32 {
        self.tide.level(self.minute(clan))
    }
}

/// Something the player does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Take on a candidate from the gate.
    Recruit {
        /// Index into the clan's candidates.
        candidate: usize,
    },
    /// Retire a crew member to the hall.
    Retire {
        /// Crew id.
        crew: u16,
    },
    /// Post a crew member to a building overnight (or `None` to stand them down).
    Post {
        /// Crew id.
        crew: u16,
        /// The plot.
        plot: Option<u8>,
    },
    /// Start a building.
    Build {
        /// The plot.
        plot: u8,
        /// The building id.
        building: String,
    },
    /// Put labour into a track's construction.
    Labour {
        /// The track.
        track: Track,
    },
    /// Work a building by hand.
    Shift {
        /// The plot.
        plot: u8,
        /// Crew ids (one to three).
        crew: Vec<u16>,
    },
    /// Work a recipe at a workshop.
    Craft {
        /// The recipe id.
        recipe: String,
    },
    /// Train a skill.
    Train {
        /// Crew id.
        crew: u16,
        /// The skill.
        skill: Skill,
    },
    /// Have the best healer tend a patient.
    Tend {
        /// Crew id of the patient.
        crew: u16,
    },
    /// Chart the marsh around the hold.
    Survey,
    /// Sell to the Tidecourt.
    Sell {
        /// What.
        what: Resource,
        /// How many.
        qty: u32,
    },
    /// Buy from the Tidecourt.
    Buy {
        /// What.
        what: Resource,
        /// How many.
        qty: u32,
    },
    /// Let the tide-turns pass (one to ten) so the water moves on.
    Wait {
        /// How many turns.
        turns: u8,
    },
    /// Set the level at which the sluice floods the approach.
    Sluice {
        /// The level, or `None` to leave it open.
        level: Option<u8>,
    },
    /// Send a party out on an expedition.
    Depart {
        /// Crew ids (one to five).
        party: Vec<u16>,
        /// The supplies taken from the stores.
        kit: Kit,
        /// Where they mean to go.
        target: Option<(u8, u8)>,
    },
    /// Do something on the expedition.
    Go(TripAction),
}

/// Why an action was refused. A refused action changes nothing and costs no turns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    /// Not enough turns left today.
    NoTurns {
        /// Turns it takes.
        need: u8,
        /// Turns left.
        have: u8,
    },
    /// No such crew member.
    NoCrew(u16),
    /// No such candidate.
    NoCandidate(usize),
    /// No such plot.
    NoPlot(u8),
    /// The plot is not open yet.
    PlotLocked(u8),
    /// The plot is taken.
    PlotBusy(u8),
    /// No such building.
    UnknownBuilding(String),
    /// A requirement is missing.
    Missing(String),
    /// Cannot pay.
    CantAfford(String),
    /// The track already has a construction.
    TrackBusy,
    /// The track has nothing under construction.
    NothingBuilding,
    /// The hold already has as many as it may.
    Maxed(String),
    /// The clan has no room for more crew.
    Full,
    /// The crew member is too hurt.
    Unfit(String),
    /// The crew member will not go.
    Refuses(String),
    /// The water is too high to work there.
    TideTooHigh {
        /// The highest water it can be worked at.
        max: u8,
        /// The water now.
        now: i32,
    },
    /// Nothing to work there.
    NotAWorkplace(u8),
    /// The post has all the hands it takes.
    PostFull,
    /// The party is empty, too large or has a repeat.
    BadParty,
    /// The clan does not know the recipe.
    NotKnown(String),
    /// No such recipe.
    UnknownRecipe(String),
    /// The workshop is not built.
    NoWorkshop(String),
    /// Nobody fit can do it.
    NoCrafter,
    /// Nothing to tend.
    NoPatient,
    /// No healer.
    NoHealer,
    /// An amount of zero.
    BadAmount,
    /// The Tidecourt does not sell relics.
    NotForSale,
    /// There is no sluice.
    NoSluice,
    /// The party is out: the hold waits.
    Away,
    /// There is no expedition.
    NoTrip,
    /// A party is already out.
    OnTrip,
    /// The kit is not allowed.
    BadKit(String),
    /// The kit is too heavy for the party.
    Overweight,
    /// The way is shut.
    Blocked(String),
    /// No route fits in the turns left.
    TooFar,
    /// Nothing here to work.
    NothingToGather,
    /// Not in a ruin.
    NoRuin,
    /// Not now.
    NotNow(String),
    /// A plain refusal with its reason.
    Cannot(String),
    /// No such choice.
    BadChoice(u8),
}

impl fmt::Display for ActionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTurns { need, have } => write!(f, "That takes {need} turns; you have {have}."),
            Self::NoCrew(_) => f.write_str("No such crew member."),
            Self::NoCandidate(_) => f.write_str("Nobody is waiting at the gate by that number."),
            Self::NoPlot(_) => f.write_str("There is no such plot."),
            Self::PlotLocked(_) => {
                f.write_str("That plot is not cleared yet. Shelter clears more ground.")
            }
            Self::PlotBusy(_) => f.write_str("Something stands (or is being built) there already."),
            Self::UnknownBuilding(_) => f.write_str("No such building."),
            Self::Missing(what) => write!(f, "You need the {what} first."),
            Self::CantAfford(what) => write!(f, "You cannot pay: {what}."),
            Self::TrackBusy => f.write_str("That track is already building something."),
            Self::NothingBuilding => f.write_str("Nothing is being built on that track."),
            Self::Maxed(what) => write!(f, "The hold has as many {what} as it can."),
            Self::Full => f.write_str("There is no room for more crew."),
            Self::Unfit(name) => write!(f, "{name} is too hurt."),
            Self::Refuses(name) => write!(f, "{name} will not go out with the water this high."),
            Self::TideTooHigh { max, now } => {
                write!(
                    f,
                    "The water is at {now}; this can only be worked at {max} or below."
                )
            }
            Self::NotAWorkplace(_) => f.write_str("There is nothing to work there."),
            Self::PostFull => f.write_str("That post has all the hands it takes."),
            Self::BadParty => f.write_str("Send one to three different crew."),
            Self::NotKnown(_) => f.write_str("The clan does not know how."),
            Self::UnknownRecipe(_) => f.write_str("No such recipe."),
            Self::NoWorkshop(w) => write!(f, "You need a {w}."),
            Self::NoCrafter => f.write_str("Nobody fit can do the work."),
            Self::NoPatient => f.write_str("They are not hurt."),
            Self::NoHealer => f.write_str("Nobody here knows healing."),
            Self::BadAmount => f.write_str("How many?"),
            Self::NotForSale => f.write_str("The Tidecourt does not sell relics."),
            Self::NoSluice => f.write_str("There is no sluice to set."),
            Self::Away => f.write_str("The party is out on the marsh. Bring it home first."),
            Self::NoTrip => f.write_str("No party is out."),
            Self::OnTrip => f.write_str("A party is already out."),
            Self::BadKit(why) => write!(f, "That kit will not do: {why}."),
            Self::Overweight => f.write_str("That is more than the party can carry."),
            Self::Blocked(why) | Self::NotNow(why) | Self::Cannot(why) => f.write_str(why),
            Self::TooFar => f.write_str("There is no safe way there in the turns you have left."),
            Self::NothingToGather => f.write_str("There is nothing to work on this ground."),
            Self::NoRuin => f.write_str("You are not in a ruin."),
            Self::BadChoice(_) => f.write_str("There is no such choice."),
        }
    }
}

/// `n * pct / 100`, rounded down, plus one more with the chance of the remainder, so averages
/// are exact over many rolls.
pub(crate) fn scale(n: u32, pct: i32, rng: &mut DoorRng) -> u32 {
    let total = u64::from(n) * u64::try_from(pct.max(0)).unwrap_or(0);
    let whole = u32::try_from(total / 100).unwrap_or(u32::MAX);
    let rest = u32::try_from(total % 100).unwrap_or(0);
    whole.saturating_add(u32::from(rng.chance(rest, 100)))
}

fn ensure_turns(env: &Env<'_>, clan: &Clan, need: u8) -> Result<(), ActionError> {
    let have = clan.turns_left(&env.settings);
    if have < need {
        Err(ActionError::NoTurns { need, have })
    } else {
        Ok(())
    }
}

fn spend(clan: &mut Clan, turns: u8) {
    clan.turns_used = clan.turns_used.saturating_add(turns);
}

/// Hurt `crew`: roll an injury of `cause` they do not have already. Returns whether they were hurt.
pub(crate) fn injure(
    cat: &Catalog,
    clan: &mut Clan,
    crew: u16,
    cause: Cause,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> bool {
    let Some(member) = clan.crew_by(crew) else {
        return false;
    };
    let options: Vec<_> = cat
        .injuries
        .iter()
        .filter(|i| i.cause == cause && !member.injuries.iter().any(|x| x.id == i.id))
        .collect();
    if options.is_empty() {
        return false;
    }
    let pick =
        options[usize::try_from(rng.below(u32::try_from(options.len()).unwrap_or(1))).unwrap_or(0)];
    let name = member.name.clone();
    if let Some(m) = clan.crew_mut(crew) {
        m.injuries.push(crate::crew::Injured {
            id: pick.id.clone(),
            days: pick.days,
        });
    }
    clan.stats.injuries += 1;
    events.push(Event::Injured {
        name,
        injury: pick.name.clone(),
    });
    true
}

/// An injury is over: remove it, and roll for a scar.
pub(crate) fn finish_injury(
    cat: &Catalog,
    clan: &mut Clan,
    crew: u16,
    injury: &str,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) {
    let Some(def) = cat.injury(injury) else {
        return;
    };
    let Some(m) = clan.crew_mut(crew) else {
        return;
    };
    m.injuries.retain(|i| i.id != injury);
    let name = m.name.clone();
    events.push(Event::Healed {
        name: name.clone(),
        injury: def.name.clone(),
    });
    if let Some(scar) = &def.scar
        && m.traits.len() < 3
        && !m.traits.contains(scar)
        && rng.chance(def.scar_pct, 100)
    {
        m.traits.push(scar.clone());
        let label = cat
            .trait_def(scar)
            .map_or_else(|| scar.clone(), |t| t.name.clone());
        events.push(Event::Scarred { name, scar: label });
    }
}

/// Do one thing. A refused action returns an error and changes nothing.
///
/// # Errors
/// [`ActionError`] says why.
pub fn apply(
    env: &Env<'_>,
    clan: &mut Clan,
    action: Action,
    rng: &mut DoorRng,
) -> Result<Vec<Event>, ActionError> {
    let mut events = Vec::new();
    if clan.trip.is_some() && !matches!(action, Action::Go(_)) {
        return Err(ActionError::Away);
    }
    match action {
        Action::Recruit { candidate } => recruit(env, clan, candidate, &mut events)?,
        Action::Retire { crew } => {
            if clan.crew_by(crew).is_none() {
                return Err(ActionError::NoCrew(crew));
            }
            let name = clan
                .crew_by(crew)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            clan.remove_crew(env.cat, crew, Some(EpitaphCause::Retired), rng, &mut events);
            events.push(Event::Retired { name });
        }
        Action::Post { crew, plot } => post(env, clan, crew, plot, &mut events)?,
        Action::Build { plot, building } => build(env, clan, plot, &building, &mut events)?,
        Action::Labour { track } => labour(env, clan, track, &mut events)?,
        Action::Shift { plot, crew } => shift(env, clan, plot, &crew, rng, &mut events)?,
        Action::Craft { recipe } => craft(env, clan, &recipe, rng, &mut events)?,
        Action::Train { crew, skill } => train(env, clan, crew, skill, &mut events)?,
        Action::Tend { crew } => tend(env, clan, crew, rng, &mut events)?,
        Action::Survey => survey(env, clan, &mut events)?,
        Action::Sell { what, qty } => sell(env, clan, what, qty, &mut events)?,
        Action::Buy { what, qty } => buy(env, clan, what, qty, &mut events)?,
        Action::Wait { turns } => {
            if turns == 0 || turns > 10 {
                return Err(ActionError::BadAmount);
            }
            ensure_turns(env, clan, turns)?;
            spend(clan, turns);
            events.push(Event::Waited {
                turns,
                level: env.level(clan),
            });
        }
        Action::Depart { party, kit, target } => {
            trip::depart(env, clan, &party, kit, target, &mut events)?;
        }
        Action::Go(act) => return trip::apply(env, clan, act, rng),
        Action::Sluice { level } => {
            if !clan.hold.built(env.cat).any(|(_, b)| b.is_sluice()) {
                return Err(ActionError::NoSluice);
            }
            clan.hold.sluice_level = level;
            events.push(Event::SluiceSet { level });
        }
    }
    Ok(events)
}

fn recruit(
    env: &Env<'_>,
    clan: &mut Clan,
    idx: usize,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    ensure_turns(env, clan, 1)?;
    let cand = clan
        .candidates
        .get(idx)
        .ok_or(ActionError::NoCandidate(idx))?;
    if clan.crew.len() >= clan.hold.crew_cap(env.cat) {
        return Err(ActionError::Full);
    }
    let price = cand.price(env.cat);
    if clan.tallies < price {
        return Err(ActionError::CantAfford(format!("{price} tallies")));
    }
    let mut member = clan.candidates.remove(idx);
    member.id = clan.take_id();
    clan.tallies -= price;
    events.push(Event::Recruited {
        name: member.name.clone(),
        trade: member.trade,
        price,
    });
    clan.crew.push(member);
    spend(clan, 1);
    Ok(())
}

fn post(
    env: &Env<'_>,
    clan: &mut Clan,
    crew: u16,
    plot: Option<u8>,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    let name = clan
        .crew_by(crew)
        .ok_or(ActionError::NoCrew(crew))?
        .name
        .clone();
    let mut label = None;
    if let Some(p) = plot {
        let id = clan
            .hold
            .plots
            .get(usize::from(p))
            .ok_or(ActionError::NoPlot(p))?
            .clone()
            .ok_or(ActionError::NotAWorkplace(p))?;
        let def = env.cat.building(&id).ok_or(ActionError::NotAWorkplace(p))?;
        if def.slots == 0 {
            return Err(ActionError::NotAWorkplace(p));
        }
        let held = clan
            .crew
            .iter()
            .filter(|c| c.post == Some(p) && c.id != crew)
            .count();
        if held >= usize::from(def.slots) {
            return Err(ActionError::PostFull);
        }
        label = Some(def.name.clone());
    }
    if let Some(c) = clan.crew_mut(crew) {
        c.post = plot;
    }
    events.push(Event::Posted {
        name,
        building: label,
    });
    Ok(())
}

fn build(
    env: &Env<'_>,
    clan: &mut Clan,
    plot: u8,
    id: &str,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    let def = env
        .cat
        .building(id)
        .ok_or_else(|| ActionError::UnknownBuilding(id.to_owned()))?;
    let at = usize::from(plot);
    if at >= crate::hold::PLOTS {
        return Err(ActionError::NoPlot(plot));
    }
    if at >= clan.hold.unlocked(env.cat) {
        return Err(ActionError::PlotLocked(plot));
    }
    if clan.hold.plots[at].is_some() || clan.hold.building.iter().flatten().any(|c| c.plot == plot)
    {
        return Err(ActionError::PlotBusy(plot));
    }
    if clan.hold.building[def.track.index()].is_some() {
        return Err(ActionError::TrackBusy);
    }
    let have = clan.hold.count(id)
        + clan
            .hold
            .building
            .iter()
            .flatten()
            .filter(|c| c.building == id)
            .count();
    if have >= usize::from(def.max) {
        return Err(ActionError::Maxed(def.name.clone()));
    }
    for need in &def.requires {
        if !clan.hold.has(need) {
            let label = env
                .cat
                .building(need)
                .map_or(need.as_str(), |b| b.name.as_str());
            return Err(ActionError::Missing(label.to_owned()));
        }
    }
    if !clan.pay(&def.cost) {
        return Err(ActionError::CantAfford(def.cost.describe()));
    }
    clan.hold.building[def.track.index()] = Some(Construction {
        building: id.to_owned(),
        plot,
        remaining: def.build_turns,
    });
    events.push(Event::Started {
        building: def.name.clone(),
        plot,
    });
    Ok(())
}

fn labour(
    env: &Env<'_>,
    clan: &mut Clan,
    track: Track,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    ensure_turns(env, clan, 1)?;
    let slot = track.index();
    let building = clan.hold.building[slot]
        .as_ref()
        .ok_or(ActionError::NothingBuilding)?
        .building
        .clone();
    let builders: Vec<u16> = clan
        .crew
        .iter()
        .filter(|c| c.fit(env.cat) && track.builders().contains(&c.trade))
        .map(|c| c.id)
        .collect();
    let progress = 1 + u8::try_from(builders.len().min(2)).unwrap_or(2);
    let name = env
        .cat
        .building(&building)
        .map_or(building.clone(), |b| b.name.clone());
    let done = {
        let Some(c) = clan.hold.building[slot].as_mut() else {
            return Err(ActionError::NothingBuilding);
        };
        c.remaining = c.remaining.saturating_sub(progress);
        c.remaining == 0
    };
    spend(clan, 1);
    if let Some(first) = builders.first().copied()
        && let Some(m) = clan.crew_mut(first)
    {
        let skill = m.trade;
        m.gain(env.cat, skill, 1);
    }
    if done {
        let Some(c) = clan.hold.building[slot].take() else {
            return Ok(());
        };
        clan.hold.plots[usize::from(c.plot)] = Some(c.building.clone());
        events.push(Event::Completed {
            building: name,
            plot: c.plot,
        });
        for r in &env.cat.recipes {
            if r.workshop == c.building
                && r.learn == Learn::Workshop
                && !clan.recipes.contains(&r.id)
            {
                clan.recipes.push(r.id.clone());
                events.push(Event::Learned {
                    recipe: r.name.clone(),
                });
            }
        }
    } else {
        let left = clan.hold.building[slot].as_ref().map_or(0, |c| c.remaining);
        events.push(Event::Labour {
            building: name,
            left,
        });
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn shift(
    env: &Env<'_>,
    clan: &mut Clan,
    plot: u8,
    party: &[u16],
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    ensure_turns(env, clan, SHIFT_TURNS)?;
    let id = clan
        .hold
        .plots
        .get(usize::from(plot))
        .ok_or(ActionError::NoPlot(plot))?
        .clone()
        .ok_or(ActionError::NotAWorkplace(plot))?;
    let def = env
        .cat
        .building(&id)
        .ok_or(ActionError::NotAWorkplace(plot))?;
    if def.produce.is_empty() {
        return Err(ActionError::NotAWorkplace(plot));
    }
    let distinct = party
        .iter()
        .enumerate()
        .all(|(i, c)| !party[..i].contains(c));
    if party.is_empty() || party.len() > MAX_PARTY || !distinct {
        return Err(ActionError::BadParty);
    }
    let level = env.level(clan);
    for &cid in party {
        let c = clan.crew_by(cid).ok_or(ActionError::NoCrew(cid))?;
        if !c.fit(env.cat) {
            return Err(ActionError::Unfit(c.name.clone()));
        }
        if level >= HOLD_FLOOD_LEVEL && c.trait_any(env.cat, |t| t.refuses_high) {
            return Err(ActionError::Refuses(c.name.clone()));
        }
    }
    if let Some(max) = def.max_level
        && level > i32::from(max)
    {
        return Err(ActionError::TideTooHigh { max, now: level });
    }
    let eaten = Bundle {
        items: def
            .consume
            .items
            .iter()
            .map(|(r, n)| (*r, n * SHIFT_NIGHTS))
            .collect(),
        tallies: 0,
    };
    if !clan.can_pay(&eaten) {
        return Err(ActionError::CantAfford(eaten.describe()));
    }
    clan.pay(&eaten);
    let trade = def.trade.unwrap_or(Trade::Wader);
    let water_up = level >= WATER_UP;
    let mut totals = [0u32; RESOURCES];
    let mut pocketed: Vec<(String, Resource, u32)> = Vec::new();
    for &cid in party {
        let Some(c) = clan.crew_by(cid) else { continue };
        let modifier = c.trait_sum(env.cat, |t| {
            t.yield_pct + if water_up { t.wet_pct } else { 0 }
        });
        let power = c.power(env.cat, trade) * (100 + modifier).max(0) / 100;
        let luck = u32::try_from(c.trait_sum(env.cat, |t| i32::try_from(t.luck_pct).unwrap_or(0)))
            .unwrap_or(0);
        let pocket = c.trait_sum(env.cat, |t| i32::try_from(t.pocket_pct).unwrap_or(0));
        let lucky = rng.chance(luck, 100);
        if lucky {
            events.push(Event::Lucky {
                name: c.name.clone(),
            });
        }
        for &(res, n) in &def.produce.items {
            let mut got = scale(n * SHIFT_NIGHTS, power, rng);
            if lucky {
                got *= 2;
            }
            let kept = scale(got, pocket, rng).min(got);
            totals[res.index()] += got - kept;
            if kept > 0 {
                pocketed.push((c.name.clone(), res, kept));
            }
        }
    }
    let pairs = i32::try_from(clan.bonded_pairs_in(party)).unwrap_or(0);
    for res in Resource::ALL {
        let base = totals[res.index()];
        if base == 0 {
            continue;
        }
        let qty = scale(base, 100 + 10 * pairs, rng);
        clan.stock.add(res, qty);
        events.push(Event::Produced { what: res, qty });
    }
    for (name, what, qty) in pocketed {
        events.push(Event::Pocketed { name, what, qty });
    }
    // Accidents: more likely with the water up, less with a steady hand.
    let cause = if level >= HOLD_FLOOD_LEVEL {
        Cause::Water
    } else {
        Cause::Hurt
    };
    let base = if level >= HOLD_FLOOD_LEVEL {
        20
    } else if water_up {
        12
    } else {
        6
    };
    let mut accident = false;
    for &cid in party {
        let Some(c) = clan.crew_by(cid) else { continue };
        let pct =
            u32::try_from(base * (100 + c.trait_sum(env.cat, |t| t.accident_pct)).max(0) / 100)
                .unwrap_or(0);
        if rng.chance(pct, 100) && injure(env.cat, clan, cid, cause, rng, events) {
            accident = true;
        }
    }
    // Hardship shared builds bonds.
    let points = 1 + u8::from(water_up) + 2 * u8::from(accident);
    for (i, &a) in party.iter().enumerate() {
        for &b in &party[i + 1..] {
            if let Some((x, y)) = clan.share_hardship(a, b, points) {
                clan.stats.bonds += 1;
                let name = |id| clan.crew_by(id).map(|c| c.name.clone()).unwrap_or_default();
                events.push(Event::Bonded {
                    a: name(x),
                    b: name(y),
                });
            }
        }
    }
    for &cid in party {
        if let Some(c) = clan.crew_mut(cid) {
            c.shifts = c.shifts.saturating_add(1);
            c.gain(env.cat, trade, 2);
        }
    }
    clan.stats.shifts += 1;
    spend(clan, SHIFT_TURNS);
    Ok(())
}

fn craft(
    env: &Env<'_>,
    clan: &mut Clan,
    recipe: &str,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    let def = env
        .cat
        .recipe(recipe)
        .ok_or_else(|| ActionError::UnknownRecipe(recipe.to_owned()))?;
    if !clan.recipes.iter().any(|r| r == recipe) {
        return Err(ActionError::NotKnown(def.name.clone()));
    }
    if !clan.hold.has(&def.workshop) {
        let label = env
            .cat
            .building(&def.workshop)
            .map_or(def.workshop.as_str(), |b| b.name.as_str());
        return Err(ActionError::NoWorkshop(label.to_owned()));
    }
    ensure_turns(env, clan, def.turns)?;
    let crafter = clan
        .crew
        .iter()
        .filter(|c| c.fit(env.cat))
        .max_by_key(|c| (c.power(env.cat, def.skill), std::cmp::Reverse(c.id)))
        .map(|c| (c.id, c.level(def.skill)))
        .ok_or(ActionError::NoCrafter)?;
    if !clan.pay(&def.inputs) {
        return Err(ActionError::CantAfford(def.inputs.describe()));
    }
    // Skill decides how often the work comes out better than the recipe.
    let bonus = u32::from(rng.chance(u32::from(crafter.1) * 5, 100));
    let qty = u32::from(def.qty) + bonus;
    clan.stock.add(def.output, qty);
    clan.stats.crafted += 1;
    if let Some(m) = clan.crew_mut(crafter.0) {
        m.gain(env.cat, def.skill, 2);
    }
    spend(clan, def.turns);
    events.push(Event::Crafted {
        recipe: def.name.clone(),
        what: def.output,
        qty,
    });
    Ok(())
}

fn train(
    env: &Env<'_>,
    clan: &mut Clan,
    crew: u16,
    skill: Skill,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    ensure_turns(env, clan, 2)?;
    let c = clan.crew_by(crew).ok_or(ActionError::NoCrew(crew))?;
    if !c.fit(env.cat) {
        return Err(ActionError::Unfit(c.name.clone()));
    }
    if clan.tallies < 3 {
        return Err(ActionError::CantAfford("3 tallies".to_owned()));
    }
    let amount = u16::try_from(4 + c.attr(crate::types::Attr::Wit) / 3).unwrap_or(4);
    clan.tallies -= 3;
    spend(clan, 2);
    if let Some(m) = clan.crew_mut(crew) {
        m.gain(env.cat, skill, amount);
        events.push(Event::Trained {
            name: m.name.clone(),
            skill,
            level: m.level(skill),
        });
    }
    Ok(())
}

fn tend(
    env: &Env<'_>,
    clan: &mut Clan,
    patient: u16,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    ensure_turns(env, clan, 1)?;
    let p = clan.crew_by(patient).ok_or(ActionError::NoCrew(patient))?;
    let worst = p
        .injuries
        .iter()
        .max_by_key(|i| i.days)
        .map(|i| i.id.clone())
        .ok_or(ActionError::NoPatient)?;
    let patient_name = p.name.clone();
    let healer = clan
        .crew
        .iter()
        .filter(|c| c.id != patient && c.fit(env.cat) && c.level(Trade::Healer) >= 1)
        .max_by_key(|c| (c.power(env.cat, Trade::Healer), std::cmp::Reverse(c.id)))
        .map(|c| (c.id, c.name.clone(), c.level(Trade::Healer)))
        .ok_or(ActionError::NoHealer)?;
    let cut = 1 + u8::from(healer.2 >= 4);
    let mut finished = false;
    if let Some(m) = clan.crew_mut(patient)
        && let Some(i) = m.injuries.iter_mut().find(|i| i.id == worst)
    {
        i.days = i.days.saturating_sub(cut);
        finished = i.days == 0;
    }
    if let Some(h) = clan.crew_mut(healer.0) {
        h.gain(env.cat, Trade::Healer, 2);
    }
    spend(clan, 1);
    events.push(Event::Tended {
        healer: healer.1,
        patient: patient_name,
    });
    if finished {
        finish_injury(env.cat, clan, patient, &worst, rng, events);
    }
    Ok(())
}

fn survey(env: &Env<'_>, clan: &mut Clan, events: &mut Vec<Event>) -> Result<(), ActionError> {
    ensure_turns(env, clan, 2)?;
    let tower = env
        .cat
        .buildings
        .iter()
        .map(|b| (b, clan.hold.count(&b.id)))
        .map(|(b, n)| usize::from(b.survey) * n)
        .sum::<usize>();
    let reader = usize::from(
        clan.crew
            .iter()
            .any(|c| c.fit(env.cat) && c.level(Trade::Wader) >= 2),
    );
    let radius = 4 + tower + reader;
    let (x, y) = env
        .map
        .sites
        .get(usize::from(clan.site))
        .copied()
        .unwrap_or(env.map.tidecourt);
    let tiles = clan.chart_around(usize::from(x), usize::from(y), radius);
    spend(clan, 2);
    events.push(Event::Surveyed { tiles });
    Ok(())
}

fn sell(
    env: &Env<'_>,
    clan: &mut Clan,
    what: Resource,
    qty: u32,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    if qty == 0 {
        return Err(ActionError::BadAmount);
    }
    ensure_turns(env, clan, 1)?;
    if clan.stock.get(what) < qty {
        return Err(ActionError::CantAfford(format!("{qty} {}", what.name())));
    }
    let tallies = World::sell_value(env.prices, what, qty);
    clan.stock.take_up_to(what, qty);
    clan.tallies = clan.tallies.saturating_add(tallies);
    clan.stats.earned = clan.stats.earned.saturating_add(tallies);
    spend(clan, 1);
    events.push(Event::Sold { what, qty, tallies });
    Ok(())
}

fn buy(
    env: &Env<'_>,
    clan: &mut Clan,
    what: Resource,
    qty: u32,
    events: &mut Vec<Event>,
) -> Result<(), ActionError> {
    if qty == 0 {
        return Err(ActionError::BadAmount);
    }
    if what == Resource::Relic {
        return Err(ActionError::NotForSale);
    }
    ensure_turns(env, clan, 1)?;
    let tallies = World::buy_cost(env.prices, what, qty);
    if clan.tallies < tallies {
        return Err(ActionError::CantAfford(format!("{tallies} tallies")));
    }
    clan.tallies -= tallies;
    clan.stock.add(what, qty);
    spend(clan, 1);
    events.push(Event::Bought { what, qty, tallies });
    Ok(())
}

/// Run the nights the clan slept through, up to [`MAX_CATCH_UP`] of them, and open `today`.
/// Idempotent: a clan already at `today` is left alone.
pub fn catch_up(env: &Env<'_>, clan: &mut Clan, today: i64) -> Vec<Event> {
    let mut events = Vec::new();
    if clan.last_day >= today {
        return events;
    }
    if clan.trip.is_some() {
        // A party left out past midnight spends the night on the marsh.
        let mut rng = DoorRng::from_label(
            &format!("saltmarch-benighted-{}-{}", clan.name, clan.seed),
            clan.last_day,
        );
        trip::overnight(env, clan, &mut rng, &mut events);
    }
    let first = (clan.last_day + 1).max(today - MAX_CATCH_UP + 1);
    for day in first..=today {
        let mut rng =
            DoorRng::from_label(&format!("saltmarch-night-{}-{}", clan.name, clan.seed), day);
        night(env, clan, day, &mut rng, &mut events);
    }
    clan.last_day = today;
    clan.turns_used = 0;
    events
}

/// Scale `produce` by how well it is worked.
fn night_output(env: &Env<'_>, clan: &Clan, plot: u8, trade: Trade) -> i32 {
    let staff: i32 = clan
        .crew
        .iter()
        .filter(|c| c.post == Some(plot) && c.fit(env.cat))
        .map(|c| {
            let m = c.trait_sum(env.cat, |t| t.yield_pct);
            c.power(env.cat, trade) * (100 + m).max(0) / 100
        })
        .sum();
    (30 + staff).min(250)
}

#[allow(clippy::too_many_lines)]
fn night(env: &Env<'_>, clan: &mut Clan, day: i64, rng: &mut DoorRng, events: &mut Vec<Event>) {
    let cat = env.cat;
    events.push(Event::Night { day });
    // Production.
    let works: Vec<(u8, String)> = clan
        .hold
        .built(cat)
        .filter(|(_, b)| !b.produce.is_empty())
        .map(|(i, b)| (u8::try_from(i).unwrap_or(0), b.id.clone()))
        .collect();
    for (plot, id) in works {
        let Some(def) = cat.building(&id) else {
            continue;
        };
        if !clan.can_pay(&Bundle {
            items: def.consume.items.clone(),
            tallies: 0,
        }) {
            continue;
        }
        let pct = night_output(env, clan, plot, def.trade.unwrap_or(Trade::Wader));
        for (r, n) in &def.consume.items {
            clan.stock.take_up_to(*r, *n);
        }
        for &(r, n) in &def.produce.items {
            let qty = scale(n, pct, rng);
            if qty > 0 {
                clan.stock.add(r, qty);
                events.push(Event::NightProduced {
                    building: def.name.clone(),
                    what: r,
                    qty,
                });
            }
        }
        let posted: Vec<u16> = clan
            .crew
            .iter()
            .filter(|c| c.post == Some(plot))
            .map(|c| c.id)
            .collect();
        for cid in posted {
            if let Some(c) = clan.crew_mut(cid) {
                c.gain(cat, def.trade.unwrap_or(Trade::Wader), 1);
            }
        }
    }
    // Food: preserved food first, then fish at two to the ration.
    let mouths = u32::try_from(clan.crew.len()).unwrap_or(0);
    let from_food = clan.stock.take_up_to(Resource::Food, mouths);
    let hungry = mouths - from_food;
    let fish = clan.stock.take_up_to(Resource::Fish, hungry * 2);
    let short = hungry - fish / 2;
    clan.stock.add(Resource::Fish, fish % 2);
    let drink: u32 = clan
        .crew
        .iter()
        .map(|c| {
            u32::try_from(c.trait_sum(cat, |t| i32::try_from(t.upkeep_tallies).unwrap_or(0)))
                .unwrap_or(0)
        })
        .sum();
    clan.tallies = clan.tallies.saturating_sub(drink);
    if short > 0 {
        clan.starving = clan.starving.saturating_add(1);
        clan.morale = clan.morale.saturating_sub(12);
        events.push(Event::Hungry { short });
        let weakest: Option<u16> = clan
            .crew
            .iter()
            .min_by_key(|c| (c.attr(crate::types::Attr::Grit), c.id))
            .map(|c| c.id);
        if let Some(w) = weakest
            && rng.chance(1, 4)
        {
            injure(cat, clan, w, Cause::Sickness, rng, events);
        }
        if clan.starving >= 3 {
            let victim = clan
                .crew
                .iter()
                .min_by_key(|c| {
                    (
                        c.attr(crate::types::Attr::Grit) + c.attr(crate::types::Attr::Wit),
                        c.id,
                    )
                })
                .map(|c| (c.id, c.name.clone()));
            if let Some((id, name)) = victim {
                clan.remove_crew(cat, id, Some(EpitaphCause::Starved), rng, events);
                events.push(Event::Starved { name });
            }
        }
    } else {
        clan.starving = 0;
        clan.morale = clan.morale.saturating_add(2).min(100);
    }
    // Peat: one fire per building that burns it, and one per four crew.
    let need = clan.hold.upkeep_peat(cat) + mouths.div_ceil(4);
    let burned = clan.stock.take_up_to(Resource::Peat, need);
    if burned < need {
        clan.morale = clan.morale.saturating_sub(6);
        events.push(Event::Cold {
            short: need - burned,
        });
        let cold: Option<u16> = clan
            .crew
            .iter()
            .min_by_key(|c| (c.attr(crate::types::Attr::Grit), c.id))
            .map(|c| c.id);
        if let Some(c) = cold
            && rng.chance(3, 10)
        {
            injure(cat, clan, c, Cause::Cold, rng, events);
        }
    }
    // Healing: a night's rest, plus the infirmary, plus a healer in the hold.
    let healer_present = clan
        .crew
        .iter()
        .any(|c| c.fit(cat) && c.level(Trade::Healer) >= 2);
    let steps = 1 + i32::from(clan.hold.heal_steps(cat)) + i32::from(healer_present);
    let ids: Vec<u16> = clan.crew.iter().map(|c| c.id).collect();
    for cid in ids {
        let Some(c) = clan.crew_by(cid) else { continue };
        let chance = u32::try_from(c.trait_sum(cat, |t| t.heal_pct).clamp(0, 100)).unwrap_or(0);
        let extra = i32::from(rng.chance(chance, 100));
        let injuries: Vec<String> = c.injuries.iter().map(|i| i.id.clone()).collect();
        for inj in injuries {
            let mut done = false;
            if let Some(m) = clan.crew_mut(cid)
                && let Some(i) = m.injuries.iter_mut().find(|i| i.id == inj)
            {
                let cut = u8::try_from(steps + extra).unwrap_or(1);
                i.days = i.days.saturating_sub(cut);
                done = i.days == 0;
            }
            if done {
                finish_injury(cat, clan, cid, &inj, rng, events);
            }
        }
    }
    // Morale drifts to the hold's and the crew's temper.
    let temper: i32 = clan
        .crew
        .iter()
        .map(|c| c.trait_sum(cat, |t| t.morale))
        .sum::<i32>()
        + clan.hold.morale(cat);
    let drift = (60 - i32::from(clan.morale)).signum();
    clan.morale =
        u8::try_from((i32::from(clan.morale) + temper + drift).clamp(0, 100)).unwrap_or(0);
    if clan.morale < 25 && rng.chance(1, 5) {
        let leaver = clan
            .crew
            .iter()
            .min_by_key(|c| (clan.partners(c.id).len(), c.id))
            .map(|c| (c.id, c.name.clone()));
        if let Some((id, name)) = leaver {
            clan.remove_crew(cat, id, None, rng, events);
            events.push(Event::Deserted { name });
        }
    }
    // Age: a crew year is a fortnight. The old retire, with an epitaph.
    if (day - clan.founded) % AGE_DAYS == 0 && day != clan.founded {
        let ids: Vec<u16> = clan.crew.iter().map(|c| c.id).collect();
        for cid in ids {
            let Some(c) = clan.crew_mut(cid) else {
                continue;
            };
            c.age = c.age.saturating_add(1);
            let (name, age) = (c.name.clone(), c.age);
            events.push(Event::Aged {
                name: name.clone(),
                age,
            });
            let tired =
                age >= RETIRE_AGE || (age >= 52 && rng.chance(u32::from(age - 51) * 10, 100));
            if tired {
                clan.remove_crew(cat, cid, Some(EpitaphCause::Retired), rng, events);
                events.push(Event::Retired { name });
            }
        }
    }
    clan.refresh_candidates(cat, rng);
    clan.turns_used = 0;
    clan.last_day = day;
}

#[cfg(test)]
mod tests;
