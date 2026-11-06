//! The shared side of the social layer: what the world remembers about convoys, piracy,
//! outposts and the season's records (M4-L4).
//!
//! Nothing here reads a clock or a random source of its own. A door cannot read another
//! captain's save, so every interaction between players is **asynchronous and goes through the
//! world**: one captain leaves a note, an intent or a pledge as a change to the stored world
//! ([`SocialDelta`]), the nightly tick settles it as a pure function of the world and the day
//! ([`Social::tick`]), and each captain collects what is theirs the next time they open the door,
//! with a cursor in their own save so nothing is collected twice. Every list has a cap and every
//! number a clamp, so a hostile or busy board cannot grow the world without bound.
//!
//! Money only moves one way at a time and is accounted on both sides: a pirate's prize is exactly
//! what the victim paid, a convoy's dividend is exactly what members paid in, and an outpost
//! pledge leaves the economy for good.

use russh_bbs_door::DoorRng;
use russh_bbs_door::content::{Table, plain_text};
use serde::Deserialize;

use crate::codec::{Reader, Writer};

/// The most captains the piracy roster lists.
pub const MAX_ROSTER: usize = 48;
/// The most intercepts waiting for the night.
pub const MAX_INTENTS: usize = 32;
/// The most settled raids kept.
pub const MAX_RAIDS: usize = 64;
/// The most convoys at once.
pub const MAX_CONVOYS: usize = 12;
/// The most captains in a convoy.
pub const CONVOY_MAX: usize = 4;
/// The most notes a convoy keeps.
pub const MAX_NOTES: usize = 8;
/// The most convoy events a convoy remembers.
pub const MAX_CONVOY_EVENTS: usize = 6;
/// The most sites that can collect pledges at once.
pub const MAX_SITES: usize = 6;
/// The most outposts in a season.
pub const MAX_OUTPOSTS: usize = 12;
/// The most backers an outpost names.
pub const MAX_BACKERS: usize = 8;
/// Seasons of hall records kept.
pub const MAX_PAST_SEASONS: usize = 3;
/// Records kept per category.
pub const HALL_PER_CATEGORY: usize = 3;
/// Days a roster entry stays after its captain was last seen.
pub const ROSTER_DAYS: i64 = 5;
/// Days a settled raid is kept for collection.
pub const RAID_KEEP_DAYS: i64 = 14;
/// Days of silence before a convoy member is dropped.
pub const MEMBER_IDLE_DAYS: i64 = 10;
/// Days a member counts as active for guard and dividends.
pub const ACTIVE_DAYS: i64 = 3;
/// Intercepts a captain may start in one day.
pub const INTERCEPTS_PER_DAY: u8 = 2;
/// Times one captain may be the target of a raid in one day.
pub const TARGET_PER_DAY: usize = 2;
/// Days before the same pair may be matched again.
pub const PAIR_COOLDOWN_DAYS: i64 = 3;
/// Days a captain must have been flagged before hunting.
pub const FLAG_SETTLE_DAYS: i64 = 1;
/// The most credits one raid can take.
pub const LOOT_CAP: i64 = 1_500;
/// The share of the victim's purse a raid can take, percent.
pub const LOOT_PCT: i64 = 8;
/// The share of a member's profit paid into the convoy pool, percent.
pub const POOL_PCT: i64 = 5;
/// Notes a captain may post per day.
pub const NOTES_PER_DAY: u8 = 5;
/// Days into a season before outposts may be funded ("late in a season").
pub const OUTPOST_MIN_DAY: i64 = 21;
/// Credits pledged in all that found (level 1) and then raise an outpost to level 2 and 3.
pub const OUTPOST_COST: [i64; 3] = [12_000, 24_000, 48_000];
/// The smallest pledge.
pub const MIN_PLEDGE: i64 = 500;
/// Tons traded at an outpost that raise it a level on their own, per level reached.
pub const OUTPOST_TRAFFIC: [i64; 2] = [1_500, 6_000];
/// The longest text kept in the world, in bytes (the codec's text limit leaves room).
const TEXT_BYTES: usize = 240;

/// Cut `text` to at most `max` bytes without splitting a character.
#[must_use]
pub fn fit(text: &str, max: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_control() || out.len() + c.len_utf8() > max {
            if c.is_control() {
                continue;
            }
            break;
        }
        out.push(c);
    }
    out
}

fn name(text: &str) -> String {
    fit(text, 48)
}

/// How a raid ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaidOutcome {
    /// The hunter took a prize.
    Raided,
    /// The target drove the hunter off.
    Repelled,
    /// The target got away; nothing changed hands.
    Escaped,
    /// Nobody was found, or a cap held the hunter back.
    Void,
}

impl RaidOutcome {
    const fn code(self) -> u8 {
        match self {
            Self::Raided => 0,
            Self::Repelled => 1,
            Self::Escaped => 2,
            Self::Void => 3,
        }
    }

    const fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Raided,
            1 => Self::Repelled,
            2 => Self::Escaped,
            3 => Self::Void,
            _ => return None,
        })
    }
}

/// A captain who has opted in to piracy, as the world lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pirate {
    /// The board handle (the key).
    pub handle: String,
    /// The captain's name.
    pub name: String,
    /// The day they opted in.
    pub since: i64,
    /// The last day they were seen.
    pub seen: i64,
    /// Firepower rating of the ship (snapshot).
    pub power: u16,
    /// Hull points (snapshot).
    pub hull: u16,
    /// Drive speed rating (snapshot).
    pub speed: u8,
    /// Infamy, 0 to 100.
    pub infamy: i16,
    /// Renown, 0 to 100.
    pub renown: i16,
    /// A coarse band of what the ship is worth, 0 to 4.
    pub worth: u8,
}

impl Pirate {
    /// How hard the ship is to beat: the one number a raid is decided by.
    #[must_use]
    pub fn strength(&self) -> i64 {
        i64::from(self.power) * 3 + i64::from(self.hull) / 10 + i64::from(self.speed) * 2
    }
}

/// An intercept the night will settle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    /// A stable id (a hash of who and when).
    pub id: u32,
    /// The day it was started.
    pub day: i64,
    /// The hunter's handle.
    pub attacker: String,
    /// The hunter's captain name.
    pub aname: String,
    /// The target's handle.
    pub target: String,
    /// The hunter's ship, as it was.
    pub ship: Pirate,
}

/// A settled raid, waiting for both sides to collect it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Raid {
    /// Increasing id: each captain's cursor runs over these.
    pub id: u32,
    /// The day it was settled.
    pub day: i64,
    /// The hunter's handle.
    pub attacker: String,
    /// The hunter's captain name.
    pub aname: String,
    /// The target's handle.
    pub victim: String,
    /// The target's captain name.
    pub vname: String,
    /// How it ended.
    pub outcome: RaidOutcome,
    /// The most a prize can be.
    pub cap: u32,
    /// Whether the victim has collected (so the prize is known).
    pub settled: bool,
    /// What the victim actually paid.
    pub paid: u32,
}

/// One captain in a convoy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The board handle.
    pub handle: String,
    /// The captain's name.
    pub name: String,
    /// The day they joined.
    pub joined: i64,
    /// The last day they were seen.
    pub seen: i64,
    /// Their ship's firepower.
    pub power: u16,
    /// Dividends earned in all (only ever grows; each captain remembers what they collected).
    pub earned: i64,
}

/// A line in a convoy's log: a template or a sanitized line a captain typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The day.
    pub day: i64,
    /// Who wrote it.
    pub who: String,
    /// The text, already sanitized.
    pub text: String,
}

/// Something that happened to a convoy in the night.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvoyEvent {
    /// Increasing id.
    pub id: u32,
    /// The day.
    pub day: i64,
    /// Index into the content's convoy events.
    pub kind: u8,
    /// How much guard the convoy had (0 to 100).
    pub guard: u8,
}

/// Captains flying together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Convoy {
    /// A stable id.
    pub id: u32,
    /// Its name.
    pub name: String,
    /// The day it formed.
    pub founded: i64,
    /// The captains.
    pub members: Vec<Member>,
    /// Dividends not yet shared out.
    pub pool: i64,
    /// Every credit ever paid in.
    pub paid_in: i64,
    /// The log.
    pub notes: Vec<Note>,
    /// What happened in the nights.
    pub events: Vec<ConvoyEvent>,
}

impl Convoy {
    /// Members seen lately.
    #[must_use]
    pub fn active(&self, today: i64) -> Vec<&Member> {
        self.members
            .iter()
            .filter(|m| today - m.seen <= ACTIVE_DAYS)
            .collect()
    }

    /// The convoy's guard rating for a captain, counting the *other* active members' guns:
    /// 0 to 100.
    #[must_use]
    pub fn guard_for(&self, handle: &str, today: i64) -> u8 {
        let sum: u32 = self
            .members
            .iter()
            .filter(|m| m.handle != handle && today - m.seen <= ACTIVE_DAYS)
            .map(|m| u32::from(m.power))
            .sum();
        u8::try_from((sum * 10).min(100)).unwrap_or(100)
    }
}

/// A place pledges are collecting toward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The system.
    pub system: u16,
    /// Credits pledged so far.
    pub fund: i64,
    /// Who pledged most so far (name), kept to name the outpost.
    pub lead: String,
    /// How much the lead has put in.
    pub lead_amount: i64,
    /// Backers' handles.
    pub backers: Vec<String>,
}

/// A player-funded station.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outpost {
    /// The system.
    pub system: u16,
    /// Its economy (an index into the content's economies).
    pub economy: u8,
    /// Its name.
    pub name: String,
    /// The day it was founded.
    pub founded: i64,
    /// Level 1 to 3: size, repairs and a yard.
    pub level: u8,
    /// Credits pledged in all.
    pub fund: i64,
    /// Tons traded here since it opened.
    pub traded: i64,
    /// Those who put in the most, by handle.
    pub backers: Vec<String>,
    /// The captain's name who founded it.
    pub founder: String,
}

/// One hall-of-fame entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// 0 richest, 1 best surveyor, 2 most feared.
    pub category: u8,
    /// The captain's name.
    pub name: String,
    /// The score.
    pub value: i64,
    /// The day it was set.
    pub day: i64,
}

/// A finished season's records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastSeason {
    /// The season number.
    pub season: u32,
    /// The records.
    pub records: Vec<Record>,
    /// How the gates went, if they opened.
    pub gates: Option<String>,
}

/// The gates, once someone has opened (or sealed) them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gates {
    /// The day.
    pub day: i64,
    /// The captain's name.
    pub by: String,
    /// The ending (an index into the arc endings table).
    pub ending: u8,
}

/// Everything the world remembers about players.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Social {
    /// The day this season's world was made.
    pub born: i64,
    /// The last raid id handed out.
    pub raid_serial: u32,
    /// Outposts that exist.
    pub outposts: Vec<Outpost>,
    /// Places collecting pledges.
    pub sites: Vec<Site>,
    /// Convoys.
    pub convoys: Vec<Convoy>,
    /// Captains who opted in to piracy.
    pub roster: Vec<Pirate>,
    /// Intercepts waiting for the night.
    pub intents: Vec<Intent>,
    /// Raids to collect.
    pub raids: Vec<Raid>,
    /// This season's records.
    pub live: Vec<Record>,
    /// Earlier seasons.
    pub past: Vec<PastSeason>,
    /// The gates, if opened.
    pub gates: Option<Gates>,
}

/// A change a captain makes to the social state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocialDelta {
    /// List (or refresh) a captain on the piracy roster.
    Roster(Pirate),
    /// Take a captain off it.
    Unroster(String),
    /// Start an intercept.
    Intent(Intent),
    /// The victim collected a raid and paid this much.
    Settle {
        /// The raid.
        raid: u32,
        /// What was paid.
        paid: u32,
    },
    /// Form a convoy.
    ConvoyForm {
        /// Its id.
        id: u32,
        /// Its name.
        name: String,
        /// The founder.
        member: Member,
        /// Today.
        day: i64,
    },
    /// Join one.
    ConvoyJoin {
        /// The convoy.
        id: u32,
        /// The captain.
        member: Member,
    },
    /// Leave one (a convoy with nobody left disbands).
    ConvoyLeave {
        /// The convoy.
        id: u32,
        /// The captain's handle.
        handle: String,
    },
    /// Show a member is still flying.
    ConvoyTouch {
        /// The convoy.
        id: u32,
        /// The captain's handle.
        handle: String,
        /// Today.
        day: i64,
        /// Their firepower now.
        power: u16,
    },
    /// Post a note.
    ConvoyNote {
        /// The convoy.
        id: u32,
        /// The note.
        note: Note,
    },
    /// Pay into the pool.
    ConvoyPay {
        /// The convoy.
        id: u32,
        /// Credits.
        amount: i64,
    },
    /// Put credits toward an outpost at a system.
    Pledge {
        /// The system.
        system: u16,
        /// Credits.
        amount: i64,
        /// The backer's handle.
        handle: String,
        /// The backer's captain name.
        name: String,
    },
    /// Offer a score to the hall.
    Record(Record),
    /// The gates were opened.
    Gates(Gates),
}

fn put_pirate(w: &mut Writer, p: &Pirate) {
    w.text(&p.handle);
    w.text(&p.name);
    w.i64(p.since);
    w.i64(p.seen);
    w.u16(p.power);
    w.u16(p.hull);
    w.u8(p.speed);
    w.i32(i32::from(p.infamy));
    w.i32(i32::from(p.renown));
    w.u8(p.worth);
}

fn get_pirate(r: &mut Reader<'_>) -> Option<Pirate> {
    Some(Pirate {
        handle: r.text()?,
        name: r.text()?,
        since: r.i64()?,
        seen: r.i64()?,
        power: r.u16()?,
        hull: r.u16()?,
        speed: r.u8()?,
        infamy: i16::try_from(r.i32()?.clamp(-100, 100)).ok()?,
        renown: i16::try_from(r.i32()?.clamp(-100, 100)).ok()?,
        worth: r.u8()?.min(4),
    })
}

fn put_member(w: &mut Writer, m: &Member) {
    w.text(&m.handle);
    w.text(&m.name);
    w.i64(m.joined);
    w.i64(m.seen);
    w.u16(m.power);
    w.i64(m.earned);
}

fn get_member(r: &mut Reader<'_>) -> Option<Member> {
    Some(Member {
        handle: r.text()?,
        name: r.text()?,
        joined: r.i64()?,
        seen: r.i64()?,
        power: r.u16()?,
        earned: r.i64()?.max(0),
    })
}

fn put_record(w: &mut Writer, rec: &Record) {
    w.u8(rec.category);
    w.text(&rec.name);
    w.i64(rec.value);
    w.i64(rec.day);
}

fn get_record(r: &mut Reader<'_>) -> Option<Record> {
    let category = r.u8()?;
    (category <= 2).then_some(())?;
    Some(Record {
        category,
        name: r.text()?,
        value: r.i64()?,
        day: r.i64()?,
    })
}

impl Social {
    /// A fresh section for a world made on `born`.
    #[must_use]
    pub fn new(born: i64) -> Self {
        Self {
            born,
            ..Self::default()
        }
    }

    /// Days into the season on `today`.
    #[must_use]
    pub const fn season_day(&self, today: i64) -> i64 {
        today - self.born
    }

    /// The convoy a captain is in, if any.
    #[must_use]
    pub fn convoy_of(&self, handle: &str) -> Option<&Convoy> {
        self.convoys
            .iter()
            .find(|c| c.members.iter().any(|m| m.handle == handle))
    }

    /// A convoy by id.
    #[must_use]
    pub fn convoy(&self, id: u32) -> Option<&Convoy> {
        self.convoys.iter().find(|c| c.id == id)
    }

    /// The roster entry for a handle.
    #[must_use]
    pub fn pirate(&self, handle: &str) -> Option<&Pirate> {
        self.roster.iter().find(|p| p.handle == handle)
    }

    /// An outpost at a system.
    #[must_use]
    pub fn outpost_at(&self, system: u16) -> Option<&Outpost> {
        self.outposts.iter().find(|o| o.system == system)
    }

    /// The pledges collecting at a system, if a site is open there.
    #[must_use]
    pub fn site_at(&self, system: u16) -> Option<&Site> {
        self.sites.iter().find(|s| s.system == system)
    }

    /// The credits an outpost at `level` (1 to 3) needs pledged in all.
    #[must_use]
    pub fn cost_of(level: u8) -> i64 {
        OUTPOST_COST
            .get(usize::from(level.clamp(1, 3)) - 1)
            .copied()
            .unwrap_or(i64::MAX)
    }

    /// Apply one captain's change. Every branch re-checks its caps, so a replayed or hostile
    /// delta cannot grow a list past its limit or reach another captain's entry.
    pub fn apply(&mut self, delta: &SocialDelta, today: i64) {
        match delta {
            SocialDelta::Roster(p) => {
                let mut p = p.clone();
                p.handle = name(&p.handle);
                p.name = name(&p.name);
                if let Some(slot) = self.roster.iter_mut().find(|x| x.handle == p.handle) {
                    // The day they opted in is kept across refreshes.
                    p.since = slot.since.min(p.since);
                    *slot = p;
                } else if self.roster.len() < MAX_ROSTER {
                    self.roster.push(p);
                }
            }
            SocialDelta::Unroster(handle) => self.roster.retain(|p| &p.handle != handle),
            SocialDelta::Intent(intent) => {
                let started_today = self
                    .intents
                    .iter()
                    .filter(|i| i.attacker == intent.attacker && i.day == intent.day)
                    .count();
                if self.intents.len() < MAX_INTENTS
                    && started_today < usize::from(INTERCEPTS_PER_DAY)
                    && !self.intents.iter().any(|i| i.id == intent.id)
                    && intent.attacker != intent.target
                {
                    self.intents.push(intent.clone());
                }
            }
            SocialDelta::Settle { raid, paid } => {
                if let Some(r) = self.raids.iter_mut().find(|r| r.id == *raid)
                    && !r.settled
                    && r.outcome == RaidOutcome::Raided
                {
                    r.settled = true;
                    r.paid = (*paid).min(r.cap);
                }
            }
            SocialDelta::ConvoyForm {
                id,
                name: cname,
                member,
                day,
            } => {
                if self.convoys.len() < MAX_CONVOYS
                    && self.convoy(*id).is_none()
                    && self.convoy_of(&member.handle).is_none()
                {
                    self.convoys.push(Convoy {
                        id: *id,
                        name: name(cname),
                        founded: *day,
                        members: vec![member.clone()],
                        pool: 0,
                        paid_in: 0,
                        notes: Vec::new(),
                        events: Vec::new(),
                    });
                }
            }
            SocialDelta::ConvoyJoin { id, member } => {
                if self.convoy_of(&member.handle).is_none()
                    && let Some(c) = self.convoys.iter_mut().find(|c| c.id == *id)
                    && c.members.len() < CONVOY_MAX
                {
                    c.members.push(member.clone());
                }
            }
            SocialDelta::ConvoyLeave { id, handle } => {
                if let Some(c) = self.convoys.iter_mut().find(|c| c.id == *id) {
                    c.members.retain(|m| &m.handle != handle);
                }
                self.convoys.retain(|c| !c.members.is_empty());
            }
            SocialDelta::ConvoyTouch {
                id,
                handle,
                day,
                power,
            } => {
                if let Some(m) = self
                    .convoys
                    .iter_mut()
                    .find(|c| c.id == *id)
                    .and_then(|c| c.members.iter_mut().find(|m| &m.handle == handle))
                {
                    m.seen = m.seen.max(*day);
                    m.power = *power;
                }
            }
            SocialDelta::ConvoyNote { id, note } => {
                let posted_today = self
                    .convoys
                    .iter()
                    .find(|c| c.id == *id)
                    .map_or(0, |c| {
                        c.notes
                            .iter()
                            .filter(|n| n.who == note.who && n.day == note.day)
                            .count()
                    });
                if posted_today < usize::from(NOTES_PER_DAY)
                    && let Some(c) = self.convoys.iter_mut().find(|c| c.id == *id)
                    && c.members.iter().any(|m| m.name == note.who)
                {
                    c.notes.push(Note {
                        day: note.day,
                        who: name(&note.who),
                        text: fit(&note.text, TEXT_BYTES),
                    });
                    while c.notes.len() > MAX_NOTES {
                        c.notes.remove(0);
                    }
                }
            }
            SocialDelta::ConvoyPay { id, amount } => {
                if let Some(c) = self.convoys.iter_mut().find(|c| c.id == *id) {
                    let amount = (*amount).clamp(0, 1_000_000);
                    c.pool += amount;
                    c.paid_in += amount;
                }
            }
            SocialDelta::Pledge {
                system,
                amount,
                handle,
                name: who,
            } => self.pledge(*system, *amount, handle, who),
            SocialDelta::Record(rec) => self.record(rec),
            SocialDelta::Gates(g) => {
                if self.gates.is_none() {
                    self.gates = Some(Gates {
                        day: g.day,
                        by: name(&g.by),
                        ending: g.ending,
                    });
                }
            }
        }
        let _ = today;
    }

    fn pledge(&mut self, system: u16, amount: i64, handle: &str, who: &str) {
        let amount = amount.clamp(0, 1_000_000);
        if amount < MIN_PLEDGE {
            return;
        }
        if let Some(o) = self.outposts.iter_mut().find(|o| o.system == system) {
            o.fund += amount;
            if !o.backers.iter().any(|b| b == handle) && o.backers.len() < MAX_BACKERS {
                o.backers.push(name(handle));
            }
            return;
        }
        let site = if let Some(i) = self.sites.iter().position(|s| s.system == system) {
            &mut self.sites[i]
        } else if self.sites.len() < MAX_SITES {
            self.sites.push(Site {
                system,
                fund: 0,
                lead: String::new(),
                lead_amount: 0,
                backers: Vec::new(),
            });
            let last = self.sites.len() - 1;
            &mut self.sites[last]
        } else {
            return;
        };
        site.fund += amount;
        if amount > site.lead_amount {
            site.lead_amount = amount;
            site.lead = name(who);
        }
        if !site.backers.iter().any(|b| b == handle) && site.backers.len() < MAX_BACKERS {
            site.backers.push(name(handle));
        }
    }

    fn record(&mut self, rec: &Record) {
        let rec = Record {
            name: name(&rec.name),
            ..rec.clone()
        };
        if rec.category > 2 || rec.value <= 0 {
            return;
        }
        // One entry per captain per category; keep the best.
        if let Some(own) = self
            .live
            .iter_mut()
            .find(|r| r.category == rec.category && r.name == rec.name)
        {
            if rec.value > own.value {
                *own = rec;
            }
        } else {
            self.live.push(rec);
        }
        for category in 0..=2u8 {
            let mut rows: Vec<Record> = self
                .live
                .iter()
                .filter(|r| r.category == category)
                .cloned()
                .collect();
            rows.sort_by(|a, b| b.value.cmp(&a.value).then(a.name.cmp(&b.name)));
            rows.truncate(HALL_PER_CATEGORY);
            self.live.retain(|r| r.category != category);
            self.live.extend(rows);
        }
    }

    /// The hall for the screen: each category's best, this season.
    #[must_use]
    pub fn hall(&self, category: u8) -> Vec<&Record> {
        let mut rows: Vec<&Record> = self.live.iter().filter(|r| r.category == category).collect();
        rows.sort_by(|a, b| b.value.cmp(&a.value).then(a.name.cmp(&b.name)));
        rows
    }

    /// Close this season's records into the hall of the next world.
    #[must_use]
    pub fn closed_into(&self, season: u32) -> Vec<PastSeason> {
        let mut past = self.past.clone();
        past.push(PastSeason {
            season,
            records: self.live.clone(),
            gates: self.gates.as_ref().map(|g| g.by.clone()),
        });
        while past.len() > MAX_PAST_SEASONS {
            past.remove(0);
        }
        past
    }

    /// Settle the night. Called by the world's tick with the day's RNG; the result depends on
    /// the world, the day and the RNG only.
    pub fn tick(&mut self, today: i64, convoy_kinds: usize, rng: &mut DoorRng) {
        self.roster.retain(|p| today - p.seen <= ROSTER_DAYS);
        self.resolve_intents(today);
        self.raids.retain(|r| today - r.day <= RAID_KEEP_DAYS);
        self.convoy_night(today, convoy_kinds, rng);
    }

    fn resolve_intents(&mut self, today: i64) {
        let intents = std::mem::take(&mut self.intents);
        for intent in intents {
            let target = self.pirate(&intent.target).cloned();
            let void = |me: &mut Self, victim: &str, vname: &str| {
                me.push_raid(&intent, victim, vname, RaidOutcome::Void, today);
            };
            let Some(target) = target else {
                void(self, &intent.target, "");
                continue;
            };
            let hits_today = self
                .raids
                .iter()
                .filter(|r| {
                    r.victim == target.handle && r.day == today && r.outcome != RaidOutcome::Void
                })
                .count();
            let paired = self.raids.iter().any(|r| {
                r.attacker == intent.attacker
                    && r.victim == target.handle
                    && r.outcome != RaidOutcome::Void
                    && today - r.day < PAIR_COOLDOWN_DAYS
            });
            if hits_today >= TARGET_PER_DAY || paired {
                void(self, &target.handle, &target.name);
                continue;
            }
            let atk = intent.ship.strength();
            let def = target.strength();
            let p_win = (50 + (atk - def) * 3).clamp(15, 85);
            let mut rng = DoorRng::from_label("long-haul/raid", i64::from(intent.id));
            let roll = i64::from(rng.below(100));
            let outcome = if roll < p_win {
                RaidOutcome::Raided
            } else if roll < p_win + (100 - p_win) / 3 {
                RaidOutcome::Escaped
            } else {
                RaidOutcome::Repelled
            };
            self.push_raid(&intent, &target.handle, &target.name, outcome, today);
        }
    }

    fn push_raid(
        &mut self,
        intent: &Intent,
        victim: &str,
        vname: &str,
        outcome: RaidOutcome,
        today: i64,
    ) {
        self.raid_serial = self.raid_serial.saturating_add(1);
        self.raids.push(Raid {
            id: self.raid_serial,
            day: today,
            attacker: intent.attacker.clone(),
            aname: intent.aname.clone(),
            victim: name(victim),
            vname: name(vname),
            outcome,
            cap: u32::try_from(LOOT_CAP).unwrap_or(0),
            settled: false,
            paid: 0,
        });
        while self.raids.len() > MAX_RAIDS {
            self.raids.remove(0);
        }
    }

    fn convoy_night(&mut self, today: i64, kinds: usize, rng: &mut DoorRng) {
        for convoy in &mut self.convoys {
            convoy.members.retain(|m| today - m.seen <= MEMBER_IDLE_DAYS);
        }
        self.convoys.retain(|c| !c.members.is_empty());
        for convoy in &mut self.convoys {
            // The pool is shared out among the captains who were flying lately.
            let active: Vec<usize> = convoy
                .members
                .iter()
                .enumerate()
                .filter(|(_, m)| today - m.seen <= ACTIVE_DAYS)
                .map(|(i, _)| i)
                .collect();
            if !active.is_empty() && convoy.pool > 0 {
                let each = convoy.pool / i64::try_from(active.len()).unwrap_or(1);
                if each > 0 {
                    for &i in &active {
                        convoy.members[i].earned += each;
                    }
                    convoy.pool -= each * i64::try_from(active.len()).unwrap_or(1);
                }
            }
            // A convoy with two or more captains flying draws a night event now and then.
            if active.len() >= 2 && kinds > 0 && rng.chance(1, 3) {
                let kind = u8::try_from(rng.below(u32::try_from(kinds).unwrap_or(1))).unwrap_or(0);
                let guard = convoy
                    .members
                    .iter()
                    .filter(|m| today - m.seen <= ACTIVE_DAYS)
                    .map(|m| u32::from(m.power))
                    .sum::<u32>();
                let id = convoy.events.last().map_or(1, |e| e.id + 1);
                convoy.events.push(ConvoyEvent {
                    id,
                    day: today,
                    kind,
                    guard: u8::try_from((guard * 8).min(100)).unwrap_or(100),
                });
                while convoy.events.len() > MAX_CONVOY_EVENTS {
                    convoy.events.remove(0);
                }
            }
        }
    }

    /// Turn the sites that have enough pledged into outposts. `economy_for` picks a system's
    /// economy; `name_for` names it from the lead backer. Returns the new outposts' systems so
    /// the world can open their markets.
    pub fn found_outposts(
        &mut self,
        today: i64,
        mut economy_for: impl FnMut(u16) -> u8,
        mut name_for: impl FnMut(&str, u16) -> String,
    ) -> Vec<u16> {
        let mut founded = Vec::new();
        let ready: Vec<usize> = self
            .sites
            .iter()
            .enumerate()
            .filter(|(_, s)| s.fund >= OUTPOST_COST[0])
            .map(|(i, _)| i)
            .collect();
        for i in ready.into_iter().rev() {
            if self.outposts.len() >= MAX_OUTPOSTS {
                break;
            }
            let site = self.sites.remove(i);
            if self.outpost_at(site.system).is_some() {
                continue;
            }
            self.outposts.push(Outpost {
                system: site.system,
                economy: economy_for(site.system),
                name: name(&name_for(&site.lead, site.system)),
                founded: today,
                level: 1,
                fund: site.fund,
                traded: 0,
                backers: site.backers,
                founder: site.lead,
            });
            founded.push(site.system);
        }
        founded
    }

    /// Raise outposts whose pledges or traffic reached the next level. Returns the systems that
    /// changed.
    pub fn grow_outposts(&mut self) -> Vec<u16> {
        let mut changed = Vec::new();
        for o in &mut self.outposts {
            let before = o.level;
            while o.level < 3 {
                let l = usize::from(o.level);
                let funded = o.fund >= Self::cost_of(o.level + 1);
                let busy = o.traded >= OUTPOST_TRAFFIC[l - 1] && o.fund >= Self::cost_of(o.level);
                if funded || busy {
                    o.level += 1;
                } else {
                    break;
                }
            }
            if o.level != before {
                changed.push(o.system);
            }
        }
        changed
    }

    /// Write the section.
    pub fn write(&self, w: &mut Writer) {
        w.i64(self.born);
        w.u32(self.raid_serial);
        w.len(self.outposts.len());
        for o in &self.outposts {
            w.u16(o.system);
            w.u8(o.economy);
            w.text(&o.name);
            w.i64(o.founded);
            w.u8(o.level);
            w.i64(o.fund);
            w.i64(o.traded);
            w.text(&o.founder);
            w.u8(u8::try_from(o.backers.len()).unwrap_or(0));
            for b in &o.backers {
                w.text(b);
            }
        }
        w.len(self.sites.len());
        for s in &self.sites {
            w.u16(s.system);
            w.i64(s.fund);
            w.text(&s.lead);
            w.i64(s.lead_amount);
            w.u8(u8::try_from(s.backers.len()).unwrap_or(0));
            for b in &s.backers {
                w.text(b);
            }
        }
        w.len(self.convoys.len());
        for c in &self.convoys {
            w.u32(c.id);
            w.text(&c.name);
            w.i64(c.founded);
            w.i64(c.pool);
            w.i64(c.paid_in);
            w.u8(u8::try_from(c.members.len()).unwrap_or(0));
            for m in &c.members {
                put_member(w, m);
            }
            w.u8(u8::try_from(c.notes.len()).unwrap_or(0));
            for n in &c.notes {
                w.i64(n.day);
                w.text(&n.who);
                w.text(&fit(&n.text, TEXT_BYTES));
            }
            w.u8(u8::try_from(c.events.len()).unwrap_or(0));
            for e in &c.events {
                w.u32(e.id);
                w.i64(e.day);
                w.u8(e.kind);
                w.u8(e.guard);
            }
        }
        w.len(self.roster.len());
        for p in &self.roster {
            put_pirate(w, p);
        }
        w.len(self.intents.len());
        for i in &self.intents {
            w.u32(i.id);
            w.i64(i.day);
            w.text(&i.attacker);
            w.text(&i.aname);
            w.text(&i.target);
            put_pirate(w, &i.ship);
        }
        w.len(self.raids.len());
        for r in &self.raids {
            w.u32(r.id);
            w.i64(r.day);
            w.text(&r.attacker);
            w.text(&r.aname);
            w.text(&r.victim);
            w.text(&r.vname);
            w.u8(r.outcome.code());
            w.u32(r.cap);
            w.u8(u8::from(r.settled));
            w.u32(r.paid);
        }
        w.len(self.live.len());
        for rec in &self.live {
            put_record(w, rec);
        }
        w.len(self.past.len());
        for p in &self.past {
            w.u32(p.season);
            w.len(p.records.len());
            for rec in &p.records {
                put_record(w, rec);
            }
            match &p.gates {
                None => w.u8(0),
                Some(g) => {
                    w.u8(1);
                    w.text(g);
                }
            }
        }
        match &self.gates {
            None => w.u8(0),
            Some(g) => {
                w.u8(1);
                w.i64(g.day);
                w.text(&g.by);
                w.u8(g.ending);
            }
        }
    }

    /// Read the section, refusing anything out of range.
    #[allow(clippy::too_many_lines)]
    pub fn read(r: &mut Reader<'_>, systems: usize, economies: usize) -> Option<Self> {
        let born = r.i64()?;
        let raid_serial = r.u32()?;
        let n = r.len(MAX_OUTPOSTS, 20)?;
        let mut outposts = Vec::with_capacity(n);
        for _ in 0..n {
            let system = r.u16()?;
            let economy = r.u8()?;
            if usize::from(system) >= systems || usize::from(economy) >= economies {
                return None;
            }
            let oname = r.text()?;
            let founded = r.i64()?;
            let level = r.u8()?;
            if !(1..=3).contains(&level) {
                return None;
            }
            let fund = r.i64()?;
            let traded = r.i64()?;
            let founder = r.text()?;
            let nb = usize::from(r.u8()?);
            if nb > MAX_BACKERS {
                return None;
            }
            let mut backers = Vec::with_capacity(nb);
            for _ in 0..nb {
                backers.push(r.text()?);
            }
            outposts.push(Outpost {
                system,
                economy,
                name: oname,
                founded,
                level,
                fund,
                traded,
                backers,
                founder,
            });
        }
        let n = r.len(MAX_SITES, 20)?;
        let mut sites = Vec::with_capacity(n);
        for _ in 0..n {
            let system = r.u16()?;
            if usize::from(system) >= systems {
                return None;
            }
            let fund = r.i64()?;
            let lead = r.text()?;
            let lead_amount = r.i64()?;
            let nb = usize::from(r.u8()?);
            if nb > MAX_BACKERS {
                return None;
            }
            let mut backers = Vec::with_capacity(nb);
            for _ in 0..nb {
                backers.push(r.text()?);
            }
            sites.push(Site {
                system,
                fund,
                lead,
                lead_amount,
                backers,
            });
        }
        let n = r.len(MAX_CONVOYS, 30)?;
        let mut convoys = Vec::with_capacity(n);
        for _ in 0..n {
            let id = r.u32()?;
            let cname = r.text()?;
            let founded = r.i64()?;
            let pool = r.i64()?;
            let paid_in = r.i64()?;
            let nm = usize::from(r.u8()?);
            if nm > CONVOY_MAX {
                return None;
            }
            let mut members = Vec::with_capacity(nm);
            for _ in 0..nm {
                members.push(get_member(r)?);
            }
            let nn = usize::from(r.u8()?);
            if nn > MAX_NOTES {
                return None;
            }
            let mut notes = Vec::with_capacity(nn);
            for _ in 0..nn {
                notes.push(Note {
                    day: r.i64()?,
                    who: r.text()?,
                    text: r.text()?,
                });
            }
            let ne = usize::from(r.u8()?);
            if ne > MAX_CONVOY_EVENTS {
                return None;
            }
            let mut events = Vec::with_capacity(ne);
            for _ in 0..ne {
                events.push(ConvoyEvent {
                    id: r.u32()?,
                    day: r.i64()?,
                    kind: r.u8()?,
                    guard: r.u8()?.min(100),
                });
            }
            convoys.push(Convoy {
                id,
                name: cname,
                founded,
                members,
                pool: pool.max(0),
                paid_in: paid_in.max(0),
                notes,
                events,
            });
        }
        let n = r.len(MAX_ROSTER, 20)?;
        let mut roster = Vec::with_capacity(n);
        for _ in 0..n {
            roster.push(get_pirate(r)?);
        }
        let n = r.len(MAX_INTENTS, 30)?;
        let mut intents = Vec::with_capacity(n);
        for _ in 0..n {
            intents.push(Intent {
                id: r.u32()?,
                day: r.i64()?,
                attacker: r.text()?,
                aname: r.text()?,
                target: r.text()?,
                ship: get_pirate(r)?,
            });
        }
        let n = r.len(MAX_RAIDS, 20)?;
        let mut raids = Vec::with_capacity(n);
        for _ in 0..n {
            raids.push(Raid {
                id: r.u32()?,
                day: r.i64()?,
                attacker: r.text()?,
                aname: r.text()?,
                victim: r.text()?,
                vname: r.text()?,
                outcome: RaidOutcome::from_code(r.u8()?)?,
                cap: r.u32()?,
                settled: match r.u8()? {
                    0 => false,
                    1 => true,
                    _ => return None,
                },
                paid: r.u32()?,
            });
        }
        let n = r.len(HALL_PER_CATEGORY * 3, 18)?;
        let mut live = Vec::with_capacity(n);
        for _ in 0..n {
            live.push(get_record(r)?);
        }
        let n = r.len(MAX_PAST_SEASONS, 6)?;
        let mut past = Vec::with_capacity(n);
        for _ in 0..n {
            let season = r.u32()?;
            let nr = r.len(HALL_PER_CATEGORY * 3, 18)?;
            let mut records = Vec::with_capacity(nr);
            for _ in 0..nr {
                records.push(get_record(r)?);
            }
            let gates = match r.u8()? {
                0 => None,
                1 => Some(r.text()?),
                _ => return None,
            };
            past.push(PastSeason {
                season,
                records,
                gates,
            });
        }
        let gates = match r.u8()? {
            0 => None,
            1 => Some(Gates {
                day: r.i64()?,
                by: r.text()?,
                ending: r.u8()?,
            }),
            _ => return None,
        };
        Some(Self {
            born,
            raid_serial,
            outposts,
            sites,
            convoys,
            roster,
            intents,
            raids,
            live,
            past,
            gates,
        })
    }

    /// The bytes of an empty section made on `born` (what a migrated world gets).
    #[must_use]
    pub fn empty_bytes(born: i64) -> Vec<u8> {
        let mut w = Writer::new();
        Self::new(born).write(&mut w);
        w.finish()
    }
}

/// A stable 32-bit id from a handle, a day and a salt (convoys, intents).
#[must_use]
pub fn hash_id(handle: &str, day: i64, salt: u32) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in handle.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
    }
    for b in day.to_le_bytes().into_iter().chain(salt.to_le_bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3);
    }
    u32::try_from(h >> 32).unwrap_or(0).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ship(power: u16) -> Pirate {
        Pirate {
            handle: "wren".into(),
            name: "Wren".into(),
            since: 1,
            seen: 5,
            power,
            hull: 100,
            speed: 3,
            infamy: 0,
            renown: 0,
            worth: 1,
        }
    }

    #[test]
    fn fit_never_splits_a_character_or_keeps_control_codes() {
        let s = "ab\u{1b}[31mc✦✦✦";
        let f = fit(s, 8);
        assert!(f.len() <= 8 && !f.contains('\u{1b}'));
        assert!(String::from_utf8(f.into_bytes()).is_ok());
    }

    #[test]
    fn an_empty_section_round_trips() {
        let bytes = Social::empty_bytes(7);
        let mut r = Reader::new(&bytes);
        let got = Social::read(&mut r, 120, 6).unwrap();
        assert!(r.done());
        assert_eq!(got, Social::new(7));
    }

    #[test]
    fn a_full_section_round_trips() {
        let mut s = Social::new(3);
        s.apply(&SocialDelta::Roster(ship(2)), 5);
        s.apply(
            &SocialDelta::ConvoyForm {
                id: 9,
                name: "Slow Kettle".into(),
                member: Member {
                    handle: "wren".into(),
                    name: "Wren".into(),
                    joined: 4,
                    seen: 5,
                    power: 2,
                    earned: 0,
                },
                day: 4,
            },
            5,
        );
        s.apply(
            &SocialDelta::ConvoyNote {
                id: 9,
                note: Note {
                    day: 5,
                    who: "Wren".into(),
                    text: "hold at Kiln".into(),
                },
            },
            5,
        );
        s.apply(
            &SocialDelta::Pledge {
                system: 4,
                amount: 700,
                handle: "wren".into(),
                name: "Wren".into(),
            },
            5,
        );
        s.apply(
            &SocialDelta::Record(Record {
                category: 0,
                name: "Wren".into(),
                value: 900,
                day: 5,
            }),
            5,
        );
        let mut w = Writer::new();
        s.write(&mut w);
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(Social::read(&mut r, 120, 6).unwrap(), s);
        assert!(r.done());
        // Any truncation is refused.
        for cut in 0..bytes.len() {
            assert!(Social::read(&mut Reader::new(&bytes[..cut]), 120, 6).is_none() || cut == 0);
        }
    }

    #[test]
    fn the_hall_keeps_one_entry_per_captain_and_the_best_three() {
        let mut s = Social::new(0);
        for (n, v) in [("A", 10), ("B", 30), ("C", 20), ("D", 40), ("A", 50), ("B", 5)] {
            s.apply(
                &SocialDelta::Record(Record {
                    category: 0,
                    name: n.into(),
                    value: v,
                    day: 1,
                }),
                1,
            );
        }
        let names: Vec<_> = s.hall(0).iter().map(|r| (r.name.clone(), r.value)).collect();
        assert_eq!(
            names,
            vec![("A".into(), 50), ("D".into(), 40), ("B".into(), 30)]
        );
    }
}

/// One thing that can happen to a convoy in the night.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConvoyEventDef {
    /// Stable id.
    pub id: String,
    /// Heading.
    pub title: String,
    /// What the captains read.
    pub text: String,
    /// Credits into (or out of) the pool.
    pub pool: i64,
    /// Hull percent each member loses with no guard.
    pub hull: u8,
    /// Fuel units each member gains.
    pub fuel: i32,
}

/// The social tables.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SocialTable {
    /// Outpost name endings.
    pub outpost_suffix: Vec<String>,
    /// Convoy name first words.
    pub convoy_adjective: Vec<String>,
    /// Convoy name second words.
    pub convoy_noun: Vec<String>,
    /// Quick notes a convoy can post without typing.
    pub notes: Vec<String>,
    /// Night events.
    pub convoy_event: Vec<ConvoyEventDef>,
}

impl Table for SocialTable {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        for (name, list, max) in [
            ("outpost_suffix", &self.outpost_suffix, 14),
            ("convoy_adjective", &self.convoy_adjective, 12),
            ("convoy_noun", &self.convoy_noun, 12),
        ] {
            if list.len() < 6 {
                p.push(format!("`{name}` needs at least 6 entries"));
            }
            if list.iter().any(|s| s.trim().is_empty() || s.chars().count() > max || !plain_text(s))
            {
                p.push(format!("`{name}` entries must be plain text of {max} characters or fewer"));
            }
        }
        if self.notes.len() < 6
            || self.notes.iter().any(|s| s.trim().is_empty() || s.chars().count() > 100 || !plain_text(s))
        {
            p.push("`notes` needs 6 or more plain lines of 100 characters or fewer".into());
        }
        if self.convoy_event.len() < 4 || self.convoy_event.len() > 32 {
            p.push("between 4 and 32 convoy events".into());
        }
        let mut seen = std::collections::HashSet::new();
        for e in &self.convoy_event {
            if !seen.insert(e.id.as_str()) {
                p.push(format!("convoy event `{}` repeated", e.id));
            }
            if e.title.is_empty()
                || e.title.chars().count() > 40
                || e.text.is_empty()
                || e.text.chars().count() > 200
                || !plain_text(&e.title)
                || !plain_text(&e.text)
                || e.pool.abs() > 1_000
                || e.hull > 40
                || e.fuel.abs() > 20
            {
                p.push(format!("convoy event `{}` is out of range", e.id));
            }
        }
        p
    }
}
