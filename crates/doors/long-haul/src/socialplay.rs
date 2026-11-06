//! The captain's side of the social layer: opting in to piracy, hunting, convoys, outpost
//! pledges, and collecting what the night settled (M4-L4).
//!
//! A door cannot read another captain's save, so none of this touches another player directly.
//! Each action asks the world for a change ([`SocialDelta`]) and each captain collects their own
//! side the next time they open the door, with cursors kept in the save so nothing is collected
//! twice. See [`crate::social`] for the world half.

use crate::arcs::{ARC_COUNT, ArcState, NO_ENDING};
use crate::codec::{Reader, Writer};
use crate::content::Faction;
use crate::engine::Env;
use crate::events::{Event, Refusal};
use crate::player::{Cause, Player};
use crate::social::{
    self, CONVOY_MAX, FLAG_SETTLE_DAYS, INTERCEPTS_PER_DAY, LOOT_PCT, Member, MIN_PLEDGE, Note,
    NOTES_PER_DAY, OUTPOST_MIN_DAY, POOL_PCT, Pirate, RaidOutcome, Social, SocialDelta,
};
use crate::world::Delta;

/// The most prizes a hunter waits on.
pub const MAX_AWAITED: usize = 8;
/// The most finished seasons a captain remembers.
pub const MAX_LEGACY: usize = 6;
/// The most one pledge can be.
pub const MAX_PLEDGE: i64 = 30_000;

/// A finished season, remembered by the next captain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Legacy {
    /// The season.
    pub season: u32,
    /// The ending reached (an index into the endings table; 255 for none).
    pub ending: u8,
    /// Net worth at the end.
    pub worth: i64,
}

/// A captain's social standing and bookkeeping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSocial {
    /// The board handle the world knows this captain by.
    pub handle: String,
    /// Opted in to piracy.
    pub flagged: bool,
    /// The day they opted in.
    pub flagged_since: i64,
    /// Infamy, 0 to 100: what a pirate has done.
    pub infamy: i16,
    /// Renown, 0 to 100: what a defender has done.
    pub renown: i16,
    /// The day intercepts were last counted.
    pub hunts_day: i64,
    /// Intercepts started that day.
    pub hunts_today: u8,
    /// Raids with a higher id than this have not been looked at.
    pub raid_cursor: u32,
    /// Raids won but waiting for the victim to pay.
    pub awaited: Vec<u32>,
    /// The convoy flown with (0 for none).
    pub convoy: u32,
    /// Dividends already collected from it.
    pub collected: i64,
    /// Convoy events already felt.
    pub event_cursor: u32,
    /// The day notes were last counted.
    pub notes_day: i64,
    /// Notes posted that day.
    pub notes_today: u8,
    /// Credits pledged to outposts in all.
    pub pledged: i64,
    /// Raids won, repelled, suffered and fought off.
    pub tally: [u16; 4],
    /// The convoy's guard rating as of the last look (0 to 100); not a rule, a cache.
    pub guard: u8,
    /// Where the captain is in each arc.
    pub arcs: [ArcState; ARC_COUNT],
    /// Earlier seasons.
    pub legacy: Vec<Legacy>,
    /// A module carried over from an earlier season (index), or 255.
    pub heirloom: u8,
}

impl Default for PlayerSocial {
    fn default() -> Self {
        Self {
            handle: String::new(),
            flagged: false,
            flagged_since: 0,
            infamy: 0,
            renown: 0,
            hunts_day: 0,
            hunts_today: 0,
            raid_cursor: 0,
            awaited: Vec::new(),
            convoy: 0,
            collected: 0,
            event_cursor: 0,
            notes_day: 0,
            notes_today: 0,
            pledged: 0,
            tally: [0; 4],
            guard: 0,
            arcs: [ArcState::default(); ARC_COUNT],
            legacy: Vec::new(),
            heirloom: 255,
        }
    }
}

impl PlayerSocial {
    /// Write to a save.
    pub fn write(&self, w: &mut Writer) {
        w.text(&self.handle);
        w.u8(u8::from(self.flagged));
        w.i64(self.flagged_since);
        w.i32(i32::from(self.infamy));
        w.i32(i32::from(self.renown));
        w.i64(self.hunts_day);
        w.u8(self.hunts_today);
        w.u32(self.raid_cursor);
        w.u8(u8::try_from(self.awaited.len()).unwrap_or(0));
        for id in &self.awaited {
            w.u32(*id);
        }
        w.u32(self.convoy);
        w.i64(self.collected);
        w.u32(self.event_cursor);
        w.i64(self.notes_day);
        w.u8(self.notes_today);
        w.i64(self.pledged);
        for t in self.tally {
            w.u16(t);
        }
        w.u8(self.guard);
        for a in &self.arcs {
            a.write(w);
        }
        w.u8(u8::try_from(self.legacy.len()).unwrap_or(0));
        for l in &self.legacy {
            w.u32(l.season);
            w.u8(l.ending);
            w.i64(l.worth);
        }
        w.u8(self.heirloom);
    }

    /// Read from a save; `modules` bounds the heirloom.
    pub fn read(r: &mut Reader<'_>, modules: usize) -> Option<Self> {
        let handle = r.text()?;
        let flagged = match r.u8()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let flagged_since = r.i64()?;
        let infamy = i16::try_from(r.i32()?.clamp(0, 100)).ok()?;
        let renown = i16::try_from(r.i32()?.clamp(0, 100)).ok()?;
        let hunts_day = r.i64()?;
        let hunts_today = r.u8()?;
        let raid_cursor = r.u32()?;
        let n = usize::from(r.u8()?);
        if n > MAX_AWAITED {
            return None;
        }
        let mut awaited = Vec::with_capacity(n);
        for _ in 0..n {
            awaited.push(r.u32()?);
        }
        let convoy = r.u32()?;
        let collected = r.i64()?;
        let event_cursor = r.u32()?;
        let notes_day = r.i64()?;
        let notes_today = r.u8()?;
        let pledged = r.i64()?;
        let mut tally = [0u16; 4];
        for t in &mut tally {
            *t = r.u16()?;
        }
        let guard = r.u8()?.min(100);
        let mut arcs = [ArcState::default(); ARC_COUNT];
        for a in &mut arcs {
            *a = ArcState::read(r)?;
        }
        let n = usize::from(r.u8()?);
        if n > MAX_LEGACY {
            return None;
        }
        let mut legacy = Vec::with_capacity(n);
        for _ in 0..n {
            legacy.push(Legacy {
                season: r.u32()?,
                ending: r.u8()?,
                worth: r.i64()?,
            });
        }
        let heirloom = r.u8()?;
        if heirloom != 255 && usize::from(heirloom) >= modules {
            return None;
        }
        Some(Self {
            handle,
            flagged,
            flagged_since,
            infamy,
            renown,
            hunts_day,
            hunts_today,
            raid_cursor,
            awaited,
            convoy,
            collected,
            event_cursor,
            notes_day,
            notes_today,
            pledged,
            tally,
            guard,
            arcs,
            legacy,
            heirloom,
        })
    }
}

fn say(text: String, tone: i8) -> Event {
    Event::Social { text, tone }
}

fn refuse(text: &str) -> Refusal {
    Refusal::Social(text.to_owned())
}

/// A line a captain typed, already sanitized by `Ctx::player_text`. The only way to make one is
/// from the session, so the engine never stores text that skipped the sanitizer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Say(String);

impl Say {
    /// Wrap a sanitized line.
    pub(crate) fn from_sanitized(text: &str) -> Self {
        Self(social::fit(text, 140))
    }

    /// For tests: wrap any text (it is still cut and stripped of control characters).
    #[must_use]
    pub fn for_tests(text: &str) -> Self {
        Self::from_sanitized(text)
    }

    /// The text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Player {
    /// What the world lists for this ship.
    #[must_use]
    pub fn roster_entry(&self, content: &crate::content::Content, today: i64) -> Pirate {
        let stats = self.stats(content);
        let worth = match self.net_worth(content) {
            ..=0 => 0,
            1..=4_999 => 1,
            5_000..=19_999 => 2,
            20_000..=59_999 => 3,
            _ => 4,
        };
        Pirate {
            handle: self.social.handle.clone(),
            name: self.captain.clone(),
            since: self.social.flagged_since,
            seen: today,
            power: u16::from(stats.power),
            hull: self.ship.hull_hp,
            speed: u8::try_from((stats.speed_x10 / 40).min(9)).unwrap_or(9),
            infamy: self.social.infamy,
            renown: self.social.renown,
            worth,
        }
    }

    fn member_entry(&self, content: &crate::content::Content, today: i64) -> Member {
        Member {
            handle: self.social.handle.clone(),
            name: self.captain.clone(),
            joined: today,
            seen: today,
            power: u16::from(self.stats(content).power),
            earned: 0,
        }
    }

    fn need_handle(&self) -> Result<(), Refusal> {
        if self.social.handle.is_empty() {
            Err(refuse("The board does not know who you are yet."))
        } else {
            Ok(())
        }
    }

    /// Opt in to (or out of) piracy.
    pub(crate) fn set_flag(&mut self, env: &mut Env<'_>, on: bool) -> Result<Vec<Event>, Refusal> {
        self.need_handle()?;
        if self.social.flagged == on {
            return Err(Refusal::NothingToDo);
        }
        if self.docked.is_none() {
            return Err(Refusal::NotDocked);
        }
        if on {
            self.social.flagged = true;
            self.social.flagged_since = env.today;
            let entry = self.roster_entry(env.content, env.today);
            env.push(Delta::Social(SocialDelta::Roster(entry)));
            Ok(vec![say(
                "You fly the Free Captains' colours. Other flagged captains can find you, and you them, \
                 and the server can read it all. Nobody who has not flagged can be touched."
                    .to_owned(),
                0,
            )])
        } else {
            if env.today - self.social.flagged_since < FLAG_SETTLE_DAYS {
                return Err(refuse("You cannot strike the colours the day you raised them."));
            }
            self.social.flagged = false;
            env.push(Delta::Social(SocialDelta::Unroster(
                self.social.handle.clone(),
            )));
            Ok(vec![say("You strike the colours. No one can hunt you now.".to_owned(), 0)])
        }
    }

    /// Why this captain cannot hunt right now, if so.
    pub(crate) fn hunt_block(&self, env: &Env<'_>) -> Option<Refusal> {
        if !self.social.flagged {
            return Some(refuse("Only captains who have raised the colours can hunt."));
        }
        if env.today - self.social.flagged_since < FLAG_SETTLE_DAYS {
            return Some(refuse("Your colours are too new: wait a day before you hunt."));
        }
        if self.stats(env.content).power == 0 {
            return Some(refuse("You cannot hunt without a gun."));
        }
        let today = if self.social.hunts_day == env.today {
            self.social.hunts_today
        } else {
            0
        };
        if today >= INTERCEPTS_PER_DAY {
            return Some(refuse("You have hunted enough for one day."));
        }
        None
    }

    /// Start an intercept on a flagged captain. The night settles it.
    pub(crate) fn intercept(
        &mut self,
        env: &mut Env<'_>,
        target: &str,
    ) -> Result<Vec<Event>, Refusal> {
        self.need_handle()?;
        if let Some(block) = self.hunt_block(env) {
            return Err(block);
        }
        if self.scene.is_some() {
            return Err(Refusal::Busy);
        }
        let other = env
            .world
            .social
            .pirate(target)
            .filter(|p| env.today - p.seen <= social::ROSTER_DAYS && p.handle != self.social.handle)
            .cloned()
            .ok_or_else(|| refuse("That captain is not flying the colours."))?;
        if env
            .world
            .social
            .convoy_of(&self.social.handle)
            .is_some_and(|c| c.members.iter().any(|m| m.handle == other.handle))
        {
            return Err(refuse("You do not hunt your own convoy."));
        }
        if env.world.social.intents.iter().any(|i| {
            i.attacker == self.social.handle && i.target == other.handle
        }) || env.world.social.raids.iter().any(|r| {
            r.attacker == self.social.handle
                && r.victim == other.handle
                && r.outcome != RaidOutcome::Void
                && env.today - r.day < social::PAIR_COOLDOWN_DAYS
        }) {
            return Err(refuse("You hunted that captain too recently."));
        }
        self.require_day()?;
        self.spend_days(1);
        if self.social.hunts_day != env.today {
            self.social.hunts_day = env.today;
            self.social.hunts_today = 0;
        }
        self.social.hunts_today += 1;
        let ship = self.roster_entry(env.content, env.today);
        let id = social::hash_id(
            &self.social.handle,
            env.today,
            u32::from(self.social.hunts_today) + self.social.raid_cursor,
        );
        env.push(Delta::Social(SocialDelta::Intent(social::Intent {
            id,
            day: env.today,
            attacker: self.social.handle.clone(),
            aname: self.captain.clone(),
            target: other.handle.clone(),
            ship,
        })));
        Ok(vec![say(
            format!(
                "You set a course to cut off {}. The night will tell how it goes; look for the \
                 report when you next dock.",
                other.name
            ),
            0,
        )])
    }

    /// Form a convoy with a name drawn from the tables.
    pub(crate) fn form_convoy(&mut self, env: &mut Env<'_>) -> Result<Vec<Event>, Refusal> {
        self.need_handle()?;
        if self.social.convoy != 0 {
            return Err(refuse("You already fly with a convoy."));
        }
        if self.docked.is_none() {
            return Err(Refusal::NotDocked);
        }
        if env.world.social.convoy_of(&self.social.handle).is_some() {
            return Err(refuse("You already fly with a convoy."));
        }
        if env.world.social.convoys.len() >= social::MAX_CONVOYS {
            return Err(refuse("The lanes have all the convoys they can carry."));
        }
        let id = social::hash_id(&self.social.handle, env.today, 7);
        let t = &env.content.social;
        let adj = &t.convoy_adjective[usize::try_from(id).unwrap_or(0) % t.convoy_adjective.len()];
        let noun =
            &t.convoy_noun[usize::try_from(id >> 8).unwrap_or(0) % t.convoy_noun.len()];
        let name = format!("{adj} {noun}");
        let member = self.member_entry(env.content, env.today);
        self.social.convoy = id;
        self.social.collected = 0;
        self.social.event_cursor = 0;
        env.push(Delta::Social(SocialDelta::ConvoyForm {
            id,
            name: name.clone(),
            member,
            day: env.today,
        }));
        Ok(vec![say(
            format!("You raise the {name} convoy. Up to {CONVOY_MAX} captains can join it."),
            1,
        )])
    }

    /// Join a convoy that has room.
    pub(crate) fn join_convoy(&mut self, env: &mut Env<'_>, id: u32) -> Result<Vec<Event>, Refusal> {
        self.need_handle()?;
        if self.social.convoy != 0 {
            return Err(refuse("Leave your convoy first."));
        }
        let c = env
            .world
            .social
            .convoy(id)
            .ok_or_else(|| refuse("That convoy has broken up."))?;
        if c.members.len() >= CONVOY_MAX {
            return Err(refuse("That convoy is full."));
        }
        let name = c.name.clone();
        let member = self.member_entry(env.content, env.today);
        self.social.convoy = id;
        self.social.collected = 0;
        self.social.event_cursor = c.events.last().map_or(0, |e| e.id);
        env.push(Delta::Social(SocialDelta::ConvoyJoin { id, member }));
        Ok(vec![say(format!("You fall in with the {name} convoy."), 1)])
    }

    /// Leave the convoy.
    pub(crate) fn leave_convoy(&mut self, env: &mut Env<'_>) -> Result<Vec<Event>, Refusal> {
        self.need_handle()?;
        if self.social.convoy == 0 {
            return Err(Refusal::NothingToDo);
        }
        let id = self.social.convoy;
        // Collect what is owed before going.
        let mut events = self.claim_convoy(env);
        env.push(Delta::Social(SocialDelta::ConvoyLeave {
            id,
            handle: self.social.handle.clone(),
        }));
        self.social.convoy = 0;
        self.social.collected = 0;
        self.social.event_cursor = 0;
        self.social.guard = 0;
        events.push(say("You leave the convoy.".to_owned(), 0));
        Ok(events)
    }

    /// Post a quick note from the table to the convoy.
    pub(crate) fn post_template(&mut self, env: &mut Env<'_>, n: usize) -> Result<Vec<Event>, Refusal> {
        let text = env
            .content
            .social
            .notes
            .get(n)
            .cloned()
            .ok_or(Refusal::NoSuchThing)?;
        self.post_note(env, &text)
    }

    /// Post a line the captain typed (sanitized by the session).
    pub(crate) fn post_say(&mut self, env: &mut Env<'_>, text: &Say) -> Result<Vec<Event>, Refusal> {
        if text.as_str().trim().is_empty() {
            return Err(Refusal::NothingToDo);
        }
        self.post_note(env, text.as_str())
    }

    fn post_note(&mut self, env: &mut Env<'_>, text: &str) -> Result<Vec<Event>, Refusal> {
        self.need_handle()?;
        if self.social.convoy == 0 {
            return Err(refuse("You are not in a convoy."));
        }
        if self.social.notes_day != env.today {
            self.social.notes_day = env.today;
            self.social.notes_today = 0;
        }
        if self.social.notes_today >= NOTES_PER_DAY {
            return Err(refuse("Your convoy has heard enough from you today."));
        }
        self.social.notes_today += 1;
        env.push(Delta::Social(SocialDelta::ConvoyNote {
            id: self.social.convoy,
            note: Note {
                day: env.today,
                who: self.captain.clone(),
                text: text.to_owned(),
            },
        }));
        Ok(vec![say("Your note goes out on the convoy channel.".to_owned(), 0)])
    }

    /// Why an outpost cannot be funded at `system` right now.
    pub(crate) fn pledge_block(&self, env: &Env<'_>, system: u16) -> Option<Refusal> {
        if self.at != system {
            return Some(Refusal::NotHere);
        }
        let sys = env.frontier.system(system)?;
        let day = env.world.social.season_day(env.today);
        if day < OUTPOST_MIN_DAY && env.world.social.outpost_at(system).is_none() {
            return Some(refuse(&format!(
                "Outposts are for late in a season: day {OUTPOST_MIN_DAY}, and this is day {day}."
            )));
        }
        let has_outpost = env.world.social.outpost_at(system).is_some();
        let built = sys
            .stations
            .iter()
            .any(|s| usize::from(*s) < env.frontier.stations.len() && !self.is_outpost(env, *s));
        if built {
            return Some(refuse("A station already stands here."));
        }
        if !has_outpost && !env.world.is_charted(env.frontier, system) {
            return Some(refuse("Nobody has charted this system."));
        }
        None
    }

    fn is_outpost(&self, env: &Env<'_>, station: u16) -> bool {
        let base = env
            .frontier
            .stations
            .len()
            .saturating_sub(env.world.social.outposts.len());
        usize::from(station) >= base
    }

    /// Pledge credits toward an outpost in this system.
    pub(crate) fn pledge(
        &mut self,
        env: &mut Env<'_>,
        system: u16,
        amount: i64,
    ) -> Result<Vec<Event>, Refusal> {
        self.need_handle()?;
        if let Some(block) = self.pledge_block(env, system) {
            return Err(block);
        }
        if self.scene.is_some() {
            return Err(Refusal::Busy);
        }
        if !(MIN_PLEDGE..=MAX_PLEDGE).contains(&amount) {
            return Err(refuse(&format!(
                "A pledge is between {MIN_PLEDGE} and {MAX_PLEDGE} credits."
            )));
        }
        crate::engine::can_pay_pub(self.cash, amount)?;
        if let Some(o) = env.world.social.outpost_at(system)
            && o.level >= 3
        {
            return Err(refuse("This outpost is as big as it gets."));
        }
        let site_full = env.world.social.site_at(system).is_none()
            && env.world.social.outpost_at(system).is_none()
            && env.world.social.sites.len() >= social::MAX_SITES;
        if site_full {
            return Err(refuse("The Guild is already tracking as many sites as it can."));
        }
        self.debit(Cause::Outpost, amount);
        self.social.pledged += amount;
        env.push(Delta::Social(SocialDelta::Pledge {
            system,
            amount,
            handle: self.social.handle.clone(),
            name: self.captain.clone(),
        }));
        self.shift_standing_quiet(Faction::Guild, 1);
        Ok(vec![say(
            format!(
                "You put {amount} credits toward a station here. It opens when {} credits are pledged.",
                Social::cost_of(1)
            ),
            1,
        )])
    }

    fn shift_standing_quiet(&mut self, faction: Faction, by: i16) {
        let slot = &mut self.standing[faction.index()];
        *slot = slot.saturating_add(by).clamp(-100, 100);
    }

    /// Pay the pool its share of a profit.
    pub(crate) fn convoy_dividend(&mut self, env: &mut Env<'_>, events: &mut Vec<Event>) {
        if self.social.convoy == 0 {
            return;
        }
        let profit: i64 = events
            .iter()
            .map(|e| match e {
                Event::Sold { profit, .. } if *profit > 0 => *profit,
                _ => 0,
            })
            .sum();
        let pay = (profit * POOL_PCT / 100).min(self.cash.max(0));
        if pay > 0 {
            self.debit(Cause::Convoy, pay);
            env.push(Delta::Social(SocialDelta::ConvoyPay {
                id: self.social.convoy,
                amount: pay,
            }));
            events.push(say(format!("{pay} credits go into the convoy pool."), 0));
        }
    }

    /// Show the world this captain is still flying (once a day).
    pub(crate) fn touch_social(&mut self, env: &mut Env<'_>) {
        if self.social.handle.is_empty() {
            return;
        }
        if self.social.flagged {
            let entry = self.roster_entry(env.content, env.today);
            env.push(Delta::Social(SocialDelta::Roster(entry)));
        }
        if self.social.convoy != 0 {
            let power = u16::from(self.stats(env.content).power);
            env.push(Delta::Social(SocialDelta::ConvoyTouch {
                id: self.social.convoy,
                handle: self.social.handle.clone(),
                day: env.today,
                power,
            }));
        }
    }

    fn claim_convoy(&mut self, env: &mut Env<'_>) -> Vec<Event> {
        let mut out = Vec::new();
        if self.social.convoy == 0 {
            return out;
        }
        let Some(c) = env.world.social.convoy(self.social.convoy).cloned() else {
            self.social.convoy = 0;
            self.social.guard = 0;
            out.push(say("Your convoy has broken up.".to_owned(), 0));
            return out;
        };
        let Some(me) = c.members.iter().find(|m| m.handle == self.social.handle) else {
            self.social.convoy = 0;
            self.social.guard = 0;
            out.push(say("The convoy has struck you from its roll.".to_owned(), 0));
            return out;
        };
        if me.earned > self.social.collected {
            let owed = me.earned - self.social.collected;
            self.social.collected = me.earned;
            self.credit(Cause::Convoy, owed);
            out.push(say(format!("Your convoy dividend: {owed} credits."), 1));
        }
        self.social.guard = c.guard_for(&self.social.handle, env.today);
        let seen = self.social.event_cursor;
        for ev in c.events.iter().filter(|e| e.id > seen) {
            self.social.event_cursor = self.social.event_cursor.max(ev.id);
            if ev.day < me.joined {
                continue;
            }
            let Some(def) = env.content.social.convoy_event.get(usize::from(ev.kind)) else {
                continue;
            };
            let mut line = format!("{}: {}", def.title, def.text);
            if def.hull > 0 {
                let max = env
                    .content
                    .hulls
                    .get(usize::from(self.ship.hull))
                    .map_or(1, |h| u32::from(h.hull_points));
                let loss = max * u32::from(def.hull) * u32::from(100 - ev.guard) / 10_000;
                let loss = u16::try_from(loss).unwrap_or(0).min(self.ship.hull_hp.saturating_sub(1));
                self.ship.hull_hp -= loss;
                if loss > 0 {
                    line.push_str(&format!(" Hull -{loss}."));
                }
            }
            if def.fuel != 0 {
                let cap = self.stats(env.content).fuel_cap;
                let fuel = i32::from(self.ship.fuel) + def.fuel;
                self.ship.fuel = u16::try_from(fuel.clamp(0, i32::from(cap))).unwrap_or(0);
            }
            out.push(say(line, if def.hull > 0 { -1 } else { 1 }));
        }
        out
    }

    fn claim_raids(&mut self, env: &mut Env<'_>) -> Vec<Event> {
        let mut out = Vec::new();
        if self.social.handle.is_empty() {
            return out;
        }
        let me = self.social.handle.clone();
        let raids: Vec<_> = env
            .world
            .social
            .raids
            .iter()
            .filter(|r| r.id > self.social.raid_cursor)
            .cloned()
            .collect();
        for r in &raids {
            self.social.raid_cursor = self.social.raid_cursor.max(r.id);
            if r.victim == me {
                match r.outcome {
                    RaidOutcome::Raided if !r.settled => {
                        let loss = (self.cash.max(0) * LOOT_PCT / 100).min(i64::from(r.cap));
                        self.debit(Cause::Piracy, loss);
                        let max = env
                            .content
                            .hulls
                            .get(usize::from(self.ship.hull))
                            .map_or(1, |h| h.hull_points);
                        let dent = (max / 12).min(self.ship.hull_hp.saturating_sub(1));
                        self.ship.hull_hp -= dent;
                        self.social.tally[2] = self.social.tally[2].saturating_add(1);
                        env.push(Delta::Social(SocialDelta::Settle {
                            raid: r.id,
                            paid: u32::try_from(loss).unwrap_or(0),
                        }));
                        out.push(say(
                            format!(
                                "{} caught you in the lanes and took {loss} credits. Hull -{dent}.",
                                r.aname
                            ),
                            -1,
                        ));
                    }
                    RaidOutcome::Repelled => {
                        self.social.renown = (self.social.renown + 3).min(100);
                        self.social.tally[3] = self.social.tally[3].saturating_add(1);
                        self.shift_standing_quiet(Faction::League, 1);
                        out.push(say(
                            format!("{} came for you and you drove them off. Your name grows.", r.aname),
                            1,
                        ));
                    }
                    RaidOutcome::Escaped => out.push(say(
                        format!("{} chased you down the lane and you got away.", r.aname),
                        0,
                    )),
                    _ => {}
                }
            }
            if r.attacker == me {
                match r.outcome {
                    RaidOutcome::Raided => {
                        if r.settled {
                            self.take_prize(r.paid, &r.vname, &mut out);
                        } else if self.social.awaited.len() < MAX_AWAITED {
                            self.social.awaited.push(r.id);
                            out.push(say(
                                format!(
                                    "You caught {}. The prize is counted when they come back to their dock.",
                                    r.vname
                                ),
                                1,
                            ));
                        }
                    }
                    RaidOutcome::Repelled => {
                        let max = env
                            .content
                            .hulls
                            .get(usize::from(self.ship.hull))
                            .map_or(1, |h| h.hull_points);
                        let dent = (max / 8).min(self.ship.hull_hp.saturating_sub(1));
                        self.ship.hull_hp -= dent;
                        self.social.infamy = (self.social.infamy - 2).max(0);
                        out.push(say(
                            format!("{} fought you off. Hull -{dent}, and your name suffers.", r.vname),
                            -1,
                        ));
                    }
                    RaidOutcome::Escaped => {
                        out.push(say(format!("{} slipped away from you.", r.vname), 0));
                    }
                    RaidOutcome::Void => {
                        out.push(say(
                            "You found no one: the target was gone, or the lanes were watched."
                                .to_owned(),
                            0,
                        ));
                    }
                }
            }
        }
        // Prizes waiting on a victim who has since come back.
        let awaited = std::mem::take(&mut self.social.awaited);
        for id in awaited {
            match env.world.social.raids.iter().find(|r| r.id == id) {
                Some(r) if r.settled => {
                    let (paid, who) = (r.paid, r.vname.clone());
                    self.take_prize(paid, &who, &mut out);
                }
                Some(_) => self.social.awaited.push(id),
                None => out.push(say(
                    "A prize you were owed lapsed: its owner never came back to their dock.".to_owned(),
                    0,
                )),
            }
        }
        out
    }

    fn take_prize(&mut self, paid: u32, victim: &str, out: &mut Vec<Event>) {
        self.credit(Cause::Piracy, i64::from(paid));
        self.social.infamy = (self.social.infamy + 3).min(100);
        self.social.tally[0] = self.social.tally[0].saturating_add(1);
        self.shift_standing_quiet(Faction::Free, 3);
        self.shift_standing_quiet(Faction::League, -3);
        out.push(say(format!("{victim} paid {paid} credits. Your name is spoken in havens."), 1));
    }

    /// Collect everything the night left for this captain: raids, dividends, convoy events.
    /// Safe to call at every open; the cursors make it count once.
    pub fn claim_social(&mut self, env: &mut Env<'_>) -> Vec<Event> {
        let mut events = self.claim_raids(env);
        events.extend(self.claim_convoy(env));
        events
    }

    /// Whether the dock refuses a high-infamy pirate (the League shuts its docks at 60).
    #[must_use]
    pub const fn infamous(&self) -> bool {
        self.social.infamy >= 60
    }

    /// An ending the arcs have not given yet is 255.
    #[must_use]
    pub fn arc_ending(&self) -> Option<(usize, u8)> {
        self.social
            .arcs
            .iter()
            .enumerate()
            .find(|(_, a)| a.ending != NO_ENDING)
            .map(|(i, a)| (i, a.ending))
    }
}

impl Player {
    /// An arc reaches one of its endings: remembered in the save, and, for the ending that opens
    /// or seals the gates, told to the whole frontier through the world.
    pub(crate) fn reach_ending(
        &mut self,
        env: &mut Env<'_>,
        arc: &str,
        ending: &str,
        events: &mut Vec<Event>,
    ) {
        let content = env.content;
        let (Some(ai), Some(ei)) = (content.arc_ix(arc), content.arc_ending_ix(arc, ending)) else {
            return;
        };
        let Some(state) = self.social.arcs.get_mut(ai) else {
            return;
        };
        if state.finished() {
            return;
        }
        let code = u8::try_from(ai * 8).unwrap_or(0) + ei;
        state.ending = code;
        state.day = env.today;
        let def = &content.arcs.arc[ai].ending[usize::from(ei)];
        events.push(say(format!("THE END OF A STORY - {}: {}", def.title, def.text), 1));
        if def.gates {
            env.push(Delta::Social(SocialDelta::Gates(social::Gates {
                day: env.today,
                by: self.captain.clone(),
                ending: code,
            })));
        }
    }
}
