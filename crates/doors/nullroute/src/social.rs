//! The social layer: rivals, hacks on hideouts, honeypots, bounties, deliveries and the street
//! word (M4-N4).
//!
//! **Nothing here reads another player's save.** A door sees its own player's save and the shared
//! world, and the world can only be changed by a compare-and-swap edit that the board may retry
//! or drop. So the layer works the way Saltmarch's does (D120):
//!
//! * a player **posts** what a scout could see of their hideout ([`Posted`]) whenever it changes;
//! * a hack on a hideout is an **order** in the world that the nightly tick settles against the
//!   posted record ([`Social::settle`]); it is never resolved in the attacker's session;
//! * what the world owes a player is a numbered **delivery**, folded once into that player's save
//!   at their next visit (`Player::fold`), with the highest number remembered;
//! * every edit that took something from a player (cycles, marks) leaves a **receipt** (an order, a
//!   bounty or a refusal that gives it back); no receipt means the edit never ran and the player
//!   takes the stake back, so a dropped edit loses time but cannot duplicate or destroy marks.
//!
//! **The caps are the abuse rules**, enforced in one place ([`Social::check_hack`],
//! [`Social::check_bounty`]) so no screen can route around them. Text that one player shows
//! another is a template ([`Lines`]) with a handle the framework already vetted, or one line that
//! went through `Ctx::player_text` (a bounty's note).

use russh_bbs_door::DoorRng;

use crate::content::Content;
use crate::crew::{Crew, Op};

/// Hideouts posted on the board.
pub const POSTED_MAX: usize = 120;
/// Hack orders waiting for the night.
pub const ORDERS_MAX: usize = 40;
/// Deliveries kept in the world.
pub const DELIVERIES_MAX: usize = 160;
/// Deliveries kept for one player.
pub const PER_RECIPIENT_MAX: usize = 14;
/// Days a delivery waits for its player before it is dropped.
pub const DELIVERY_DAYS: i64 = 14;
/// Open bounties.
pub const BOUNTIES_MAX: usize = 24;
/// Deeds on the street feed.
pub const DEEDS_MAX: usize = 16;
/// Nights of market prices kept.
pub const HIST_MAX: usize = 14;
/// Seasons kept in the hall.
pub const HALL_MAX: usize = 8;
/// Days a new character is safe from rivals.
pub const PROTECT_DAYS: i64 = 7;
/// Hacks one player can file in a day.
pub const HACKS_PER_DAY: usize = 2;
/// Hacks one hideout can have waiting.
pub const HITS_PER_DAY: usize = 2;
/// Days before the same pair may repeat (counted from the filing day).
pub const PAIR_DAYS: i64 = 3;
/// Days of peace after a breach.
pub const PEACE_DAYS: i64 = 2;
/// Cycles a hack costs the attacker.
pub const HACK_CYCLES: u32 = 6;
/// The share of the target's posted cash a breach takes, in percent.
pub const STEAL_PCT: u32 = 10;
/// The most a breach takes.
pub const STEAL_MAX: u32 = 60;
/// The fine for tripping a honeypot.
pub const TRAP_FINE: u32 = 25;
/// The chance, per honeypot level, that a hack trips it.
pub const TRAP_PCT: u32 = 25;
/// The most honeypot levels.
pub const HONEYPOT_MAX: u8 = 3;
/// The smallest and largest stake for a bounty.
pub const BOUNTY_STAKE: (u32, u32) = (100, 400);
/// The fixer's cut of a stake, in percent.
pub const BOUNTY_FEE_PCT: u32 = 20;
/// Days a bounty stands.
pub const BOUNTY_DAYS: i64 = 7;
/// Bounties one player may have open.
pub const BOUNTIES_PER_POSTER: usize = 2;
/// Bounties that may stand on one player.
pub const BOUNTIES_PER_TARGET: usize = 2;
/// The heat at which a fixer puts a price on someone unasked.
pub const AUTO_BOUNTY_HEAT: u32 = 70;
/// Fixer bounties open at once.
pub const AUTO_BOUNTIES_MAX: usize = 6;
/// Days before someone who opted out can opt back in.
pub const OPT_IN_DAYS: i64 = 2;
/// The longest note on a bounty, in characters.
pub const NOTE_MAX: usize = 60;
/// Characters of a handle kept.
pub const HANDLE_MAX: usize = 32;
/// Pair records kept.
pub const PAIRS_MAX: usize = 60;

/// Whether two handles are the same player (handles are compared without case).
#[must_use]
pub fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// What a scout could see of a hideout. Posted by its owner whenever it changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Posted {
    /// The handle.
    pub handle: String,
    /// The tier the player works at (1 to 3, from reputation).
    pub tier: u8,
    /// Heat.
    pub heat: u8,
    /// Honeypot levels (0 to [`HONEYPOT_MAX`]).
    pub honeypot: u8,
    /// Shield level (0 to 3).
    pub shield: u8,
    /// How good the player is at getting in (0 to 10), from skills and interface.
    pub edge: u8,
    /// Marks on hand (not in the safe), in tens.
    pub cash: u16,
    /// Reputation.
    pub rep: u32,
    /// The day the character was made: the protection week counts from it.
    pub born: i64,
    /// The last day the hideout was posted.
    pub seen: i64,
    /// The world's: no hack is accepted before this day. Kept when the player reposts.
    pub peace_until: i64,
    /// Whether the player is on the rivals board (the opt-out is this being false): they can
    /// hack, be hacked and claim bounties only while it is true.
    pub pvp: bool,
    /// Whether the player takes crew invitations.
    pub crews_open: bool,
}

/// A hack on a hideout, waiting for the night.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    /// Unique.
    pub id: u32,
    /// The attacker's handle.
    pub from: String,
    /// The target's handle.
    pub to: String,
    /// The day it was filed.
    pub day: i64,
}

/// What a delivery does to its player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Nothing but the message (also the receipt for a filed order).
    None,
    /// Marks, plus or minus (a debit takes what the player has, no more).
    Marks,
    /// Heat, plus or minus.
    Heat,
    /// Reputation.
    Rep,
    /// Cycles back today.
    Cycles,
    /// A rig part is fried.
    Fry,
}

impl Effect {
    /// The byte in the world.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Marks => 1,
            Self::Heat => 2,
            Self::Rep => 3,
            Self::Cycles => 4,
            Self::Fry => 5,
        }
    }

    /// From the byte.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::None,
            1 => Self::Marks,
            2 => Self::Heat,
            3 => Self::Rep,
            4 => Self::Cycles,
            5 => Self::Fry,
            _ => return None,
        })
    }
}

/// What the world owes a player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    /// Unique, rising: a player remembers the highest they applied.
    pub id: u32,
    /// The player's handle.
    pub to: String,
    /// The day it was made.
    pub day: i64,
    /// What it does.
    pub effect: Effect,
    /// How much (signed).
    pub n: i32,
    /// The other party's handle (empty for the world or a fixer).
    pub who: String,
    /// The template line that tells the player, from `social.toml`.
    pub line: u8,
    /// The nonce of the player's pending edit this answers (0 for none).
    pub nonce: u32,
}

/// A price on a head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bounty {
    /// Unique.
    pub id: u32,
    /// Who it is on.
    pub target: String,
    /// Who paid (empty for a fixer's own).
    pub poster: String,
    /// The fixer who holds it.
    pub fixer: u8,
    /// What a hack that lands is paid.
    pub reward: u32,
    /// What the poster staked (0 for a fixer's own).
    pub stake: u32,
    /// The target's tier when it was posted.
    pub tier: u8,
    /// The day it was posted.
    pub posted: i64,
    /// The last day it stands.
    pub expires: i64,
    /// One line from the poster (sanitized by the framework's `player_text`), or empty.
    pub note: String,
}

/// A thing that happened, for the street feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deed {
    /// The day.
    pub day: i64,
    /// The template line.
    pub line: u8,
    /// The player it is about.
    pub who: String,
    /// A number for the line.
    pub n: u32,
}

/// What the whole board did this season: the ending is chosen from these.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tallies {
    /// Objectives finished on tier-3 networks.
    pub deep_breaches: u32,
    /// Wardens put down.
    pub wardens: u32,
    /// Crew runs finished with the objective.
    pub crew_jobs: u32,
    /// Hacks on hideouts that got in.
    pub hacks_in: u32,
    /// Hacks a honeypot caught.
    pub hacks_caught: u32,
    /// Bounties paid.
    pub bounties_paid: u32,
    /// Runs that burned.
    pub burns: u32,
    /// Runs that finished the objective.
    pub jobs: u32,
    /// 1 once a crew has cracked the season's finale.
    pub finale: u32,
}

/// The crew that cracked the season's finale, if any.
pub type Finale = Option<String>;

/// A season, as the hall keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonRecord {
    /// The season number.
    pub season: u32,
    /// The ending (a `season_ending` number).
    pub ending: u8,
    /// The five with the most reputation when it closed.
    pub top: Vec<(String, u32)>,
    /// What the board did.
    pub tallies: Tallies,
    /// The crew that cracked the finale, or empty.
    pub finale: String,
}

/// A pair of hackers, for the cooldown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    /// Who hacked.
    pub from: String,
    /// Who was hacked.
    pub to: String,
    /// The day it was filed.
    pub day: i64,
}

/// Why a social edit was refused. The word goes to the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// A player cannot be their own rival.
    SelfTarget,
    /// The player is not on the board (never posted, or opted out).
    NotPosted,
    /// The target is not on the board.
    TargetGone,
    /// The target is a new character.
    Protected,
    /// The target is too far below the attacker.
    TooLow,
    /// The target is in the attacker's crew.
    Crewmate,
    /// The attacker filed enough today.
    TooMany,
    /// The target has enough waiting.
    Swamped,
    /// The same pair hacked recently.
    Cooldown,
    /// The target was just breached.
    Peace,
    /// The orders are full.
    Full,
    /// The stake is outside the limits.
    Stake,
    /// The poster has enough bounties open.
    PosterCap,
    /// The target has enough bounties on it.
    TargetCap,
    /// Not allowed against someone below the poster's tier.
    Below,
}

impl Why {
    /// The word for the player.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::SelfTarget => "that is you",
            Self::NotPosted => "you are not on the rivals board (PvP is off or you have not posted)",
            Self::TargetGone => "they are not on the board any more",
            Self::Protected => "they are new: protected for their first week",
            Self::TooLow => "they are a tier below you",
            Self::Crewmate => "they are in your crew",
            Self::TooMany => "you have filed enough hacks today",
            Self::Swamped => "that hideout already has hacks waiting",
            Self::Cooldown => "you hit them recently; give it a few days",
            Self::Peace => "they were just breached and are left in peace",
            Self::Full => "the board is full tonight",
            Self::Stake => "a stake is 100 to 400 marks",
            Self::PosterCap => "you have enough bounties open",
            Self::TargetCap => "enough bounties already stand on them",
            Self::Below => "bounties are for rivals at your tier or above",
        }
    }
}

/// The shared social state: part of the world.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Social {
    /// The next id for orders, deliveries, bounties and crews.
    pub next_id: u32,
    /// Hideouts on the board.
    pub posted: Vec<Posted>,
    /// Hacks waiting for the night.
    pub orders: Vec<Order>,
    /// What the world owes players.
    pub deliveries: Vec<Delivery>,
    /// Prices on heads.
    pub bounties: Vec<Bounty>,
    /// The street feed, newest last.
    pub deeds: Vec<Deed>,
    /// This season's tallies.
    pub tallies: Tallies,
    /// The crew that cracked the finale this season.
    pub finale: Finale,
    /// Past seasons, oldest first.
    pub hall: Vec<SeasonRecord>,
    /// Recent hacks, for the cooldown.
    pub pairs: Vec<Pair>,
    /// Crews.
    pub crews: Vec<Crew>,
    /// Crew runs in flight or just over.
    pub ops: Vec<Op>,
}

impl Social {
    /// A fresh id.
    pub fn id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.next_id
    }

    /// A hideout by handle.
    #[must_use]
    pub fn posted_of(&self, handle: &str) -> Option<&Posted> {
        self.posted.iter().find(|p| same(&p.handle, handle))
    }

    /// Post (or repost) a hideout. A repost keeps the world's own `peace_until`; a full board
    /// drops the hideout seen longest ago.
    pub fn post(&mut self, mut p: Posted) {
        p.handle.truncate(HANDLE_MAX);
        if let Some(at) = self.posted.iter().position(|q| same(&q.handle, &p.handle)) {
            p.peace_until = self.posted[at].peace_until;
            self.posted[at] = p;
            return;
        }
        if self.posted.len() >= POSTED_MAX
            && let Some(at) = self
                .posted
                .iter()
                .enumerate()
                .min_by_key(|(_, q)| q.seen)
                .map(|(i, _)| i)
        {
            self.posted.remove(at);
        }
        self.posted.push(p);
    }

    /// Take a hideout off the board (the player opted out).
    pub fn unpost(&mut self, handle: &str) {
        self.posted.retain(|p| !same(&p.handle, handle));
    }

    /// Whether two players are in one crew.
    #[must_use]
    pub fn crewmates(&self, a: &str, b: &str) -> bool {
        self.crews
            .iter()
            .any(|c| c.has(a) && c.has(b))
    }

    /// The caps on filing a hack. This is the only place they are decided.
    ///
    /// # Errors
    /// The first rule the hack breaks.
    pub fn check_hack(&self, from: &str, to: &str, today: i64) -> Result<(), Why> {
        if same(from, to) {
            return Err(Why::SelfTarget);
        }
        let a = self.posted_of(from).filter(|a| a.pvp).ok_or(Why::NotPosted)?;
        let t = self.posted_of(to).filter(|t| t.pvp).ok_or(Why::TargetGone)?;
        if today - t.born < PROTECT_DAYS {
            return Err(Why::Protected);
        }
        if t.tier + 1 < a.tier {
            return Err(Why::TooLow);
        }
        if self.crewmates(from, to) {
            return Err(Why::Crewmate);
        }
        if self.orders.iter().filter(|o| same(&o.from, from)).count() >= HACKS_PER_DAY {
            return Err(Why::TooMany);
        }
        if self.orders.iter().filter(|o| same(&o.to, to)).count() >= HITS_PER_DAY {
            return Err(Why::Swamped);
        }
        if self.pairs.iter().any(|p| {
            same(&p.from, from) && same(&p.to, to) && today - p.day < PAIR_DAYS
        }) {
            return Err(Why::Cooldown);
        }
        if today < t.peace_until {
            return Err(Why::Peace);
        }
        if self.orders.len() >= ORDERS_MAX {
            return Err(Why::Full);
        }
        Ok(())
    }

    /// File a hack. Always leaves a receipt for the attacker: an order (and a delivery that only
    /// confirms) or a refusal that gives the cycles back. `nonce` ties it to the attacker's
    /// pending record.
    pub fn file_hack(&mut self, from: &str, to: &str, today: i64, nonce: u32) -> Result<(), Why> {
        match self.check_hack(from, to, today) {
            Ok(()) => {
                let id = self.id();
                self.orders.push(Order {
                    id,
                    from: from.to_owned(),
                    to: to.to_owned(),
                    day: today,
                });
                self.pairs.push(Pair {
                    from: from.to_owned(),
                    to: to.to_owned(),
                    day: today,
                });
                if self.pairs.len() > PAIRS_MAX {
                    self.pairs.remove(0);
                }
                self.deliver(from, today, Effect::None, 0, to, Lines::FILED, nonce);
                Ok(())
            }
            Err(why) => {
                let cycles = i32::try_from(HACK_CYCLES).unwrap_or(6);
                self.deliver(from, today, Effect::Cycles, cycles, to, Lines::REFUSED, nonce);
                Err(why)
            }
        }
    }

    /// The caps on a bounty.
    ///
    /// # Errors
    /// The first rule it breaks.
    pub fn check_bounty(&self, from: &str, target: &str, stake: u32) -> Result<(), Why> {
        if same(from, target) {
            return Err(Why::SelfTarget);
        }
        if !(BOUNTY_STAKE.0..=BOUNTY_STAKE.1).contains(&stake) {
            return Err(Why::Stake);
        }
        let a = self.posted_of(from).filter(|a| a.pvp).ok_or(Why::NotPosted)?;
        let t = self.posted_of(target).filter(|t| t.pvp).ok_or(Why::TargetGone)?;
        if t.tier < a.tier {
            return Err(Why::Below);
        }
        if self.crewmates(from, target) {
            return Err(Why::Crewmate);
        }
        if self.bounties.iter().filter(|b| same(&b.poster, from)).count() >= BOUNTIES_PER_POSTER {
            return Err(Why::PosterCap);
        }
        if self
            .bounties
            .iter()
            .filter(|b| same(&b.target, target))
            .count()
            >= BOUNTIES_PER_TARGET
            || self
                .bounties
                .iter()
                .any(|b| same(&b.target, target) && same(&b.poster, from))
        {
            return Err(Why::TargetCap);
        }
        if self.bounties.len() >= BOUNTIES_MAX {
            return Err(Why::Full);
        }
        Ok(())
    }

    /// Post a bounty from a stake. Always leaves a receipt: the bounty or a refund of the stake.
    pub fn post_bounty(
        &mut self,
        content: &Content,
        from: &str,
        target: &str,
        stake: u32,
        note: &str,
        today: i64,
        nonce: u32,
    ) -> Result<(), Why> {
        match self.check_bounty(from, target, stake) {
            Ok(()) => {
                let id = self.id();
                let t = self.posted_of(target).map_or(1, |t| t.tier);
                let fixer = fixer_for(content, id);
                self.bounties.push(Bounty {
                    id,
                    target: target.to_owned(),
                    poster: from.to_owned(),
                    fixer,
                    reward: stake * (100 - BOUNTY_FEE_PCT) / 100,
                    stake,
                    tier: t,
                    posted: today,
                    expires: today + BOUNTY_DAYS,
                    note: note.chars().take(NOTE_MAX).collect(),
                });
                self.deliver(from, today, Effect::None, 0, target, Lines::BOUNTY_FILED, nonce);
                self.deliver(
                    target,
                    today,
                    Effect::None,
                    i32::try_from(stake * (100 - BOUNTY_FEE_PCT) / 100).unwrap_or(0),
                    "",
                    Lines::BOUNTY_ON_YOU,
                    0,
                );
                Ok(())
            }
            Err(why) => {
                self.deliver(
                    from,
                    today,
                    Effect::Marks,
                    i32::try_from(stake).unwrap_or(0),
                    target,
                    Lines::REFUSED_BOUNTY,
                    nonce,
                );
                Err(why)
            }
        }
    }

    /// Add a delivery. A player keeps at most [`PER_RECIPIENT_MAX`] (the oldest go first), and the
    /// world at most [`DELIVERIES_MAX`].
    #[allow(clippy::too_many_arguments)]
    pub fn deliver(
        &mut self,
        to: &str,
        day: i64,
        effect: Effect,
        n: i32,
        who: &str,
        line: u8,
        nonce: u32,
    ) {
        let id = self.id();
        self.deliveries.push(Delivery {
            id,
            to: to.to_owned(),
            day,
            effect,
            n,
            who: who.to_owned(),
            line,
            nonce,
        });
        let mine = self.deliveries.iter().filter(|d| same(&d.to, to)).count();
        if mine > PER_RECIPIENT_MAX
            && let Some(at) = self.deliveries.iter().position(|d| same(&d.to, to))
        {
            self.deliveries.remove(at);
        }
        while self.deliveries.len() > DELIVERIES_MAX {
            self.deliveries.remove(0);
        }
    }

    /// A line for the street feed.
    pub fn deed(&mut self, day: i64, line: u8, who: &str, n: u32) {
        self.deeds.push(Deed {
            day,
            line,
            who: who.to_owned(),
            n,
        });
        while self.deeds.len() > DEEDS_MAX {
            self.deeds.remove(0);
        }
    }

    /// The bounty on `target` that `hacker` may claim, if any: not their own, and not on someone
    /// below their tier.
    fn claimable(&self, hacker: &Posted, target: &str) -> Option<usize> {
        self.bounties.iter().position(|b| {
            same(&b.target, target) && !same(&b.poster, &hacker.handle) && b.tier >= hacker.tier
        })
    }

    /// The night: settle the orders against the posted hideouts, lapse old bounties, let the
    /// fixers put a price on the hottest, and drop what nobody came for.
    pub fn settle(&mut self, content: &Content, rng: &mut DoorRng, day: i64) {
        let mut orders = std::mem::take(&mut self.orders);
        orders.sort_by_key(|o| o.id);
        for o in orders {
            self.settle_order(content, rng, day, &o);
        }
        // Bounties that ran out give the stake back less the fixer's fee. A bounty on someone
        // who left the board (opted out) just runs out, too.
        let (live, dead): (Vec<Bounty>, Vec<Bounty>) =
            std::mem::take(&mut self.bounties)
                .into_iter()
                .partition(|b| b.expires >= day);
        self.bounties = live;
        for b in dead {
            if !b.poster.is_empty() {
                let back = i32::try_from(b.stake * (100 - BOUNTY_FEE_PCT) / 100).unwrap_or(0);
                self.deliver(&b.poster, day, Effect::Marks, back, &b.target, Lines::BOUNTY_LAPSED, 0);
            }
        }
        // A fixer notices the hottest name.
        let mut hot: Vec<(u8, String)> = self
            .posted
            .iter()
            .filter(|p| u32::from(p.heat) >= AUTO_BOUNTY_HEAT)
            .map(|p| (p.heat, p.handle.clone()))
            .collect();
        hot.sort_by(|a, b| b.cmp(a));
        for (heat, handle) in hot {
            let autos = self.bounties.iter().filter(|b| b.poster.is_empty()).count();
            if autos >= AUTO_BOUNTIES_MAX || self.bounties.len() >= BOUNTIES_MAX {
                break;
            }
            if self
                .bounties
                .iter()
                .any(|b| same(&b.target, &handle) && b.poster.is_empty())
            {
                continue;
            }
            let id = self.id();
            let tier = self.posted_of(&handle).map_or(1, |p| p.tier);
            let reward = 40 + u32::from(heat) * 2 + 25 * u32::from(tier);
            self.bounties.push(Bounty {
                id,
                target: handle.clone(),
                poster: String::new(),
                fixer: fixer_for(content, id),
                reward,
                stake: 0,
                tier,
                posted: day,
                expires: day + 5,
                note: String::new(),
            });
            self.deliver(
                &handle,
                day,
                Effect::None,
                i32::try_from(reward).unwrap_or(0),
                "",
                Lines::BOUNTY_ON_YOU,
                0,
            );
            self.deed(day, Lines::DEED_PRICE, &handle, reward);
        }
        self.deliveries.retain(|d| day - d.day < DELIVERY_DAYS);
        self.posted.retain(|p| day - p.seen < 14);
        self.pairs.retain(|p| day - p.day < PAIR_DAYS + 1);
    }

    fn settle_order(&mut self, content: &Content, rng: &mut DoorRng, day: i64, o: &Order) {
        let (Some(a), Some(t)) = (
            self.posted_of(&o.from).cloned(),
            self.posted_of(&o.to).cloned(),
        ) else {
            let cycles = i32::try_from(HACK_CYCLES).unwrap_or(6);
            self.deliver(&o.from, day, Effect::Cycles, cycles, &o.to, Lines::GONE, 0);
            return;
        };
        let _ = content;
        if rng.chance(TRAP_PCT * u32::from(t.honeypot), 100) {
            let fine = i32::try_from(TRAP_FINE).unwrap_or(25);
            self.deliver(&o.from, day, Effect::Marks, -fine, &o.to, Lines::TRAP_ATT, 0);
            self.deliver(&o.from, day, Effect::Heat, 6, &o.to, Lines::TRAP_HEAT, 0);
            self.deliver(&o.to, day, Effect::Rep, 2, &o.from, Lines::TRAP_DEF, 0);
            self.tallies.hacks_caught += 1;
            return;
        }
        let mut chance = 50 + 6 * i32::from(a.edge) - 18 * i32::from(t.shield);
        if u32::from(t.heat) >= 50 {
            chance += 10;
        }
        let chance = u32::try_from(chance.clamp(15, 85)).unwrap_or(15);
        if !rng.chance(chance, 100) {
            self.deliver(&o.from, day, Effect::Heat, 4, &o.to, Lines::FAIL_ATT, 0);
            self.deliver(&o.to, day, Effect::None, 0, &o.from, Lines::FAIL_DEF, 0);
            return;
        }
        let steal = (u32::from(t.cash) * 10 * STEAL_PCT / 100).min(STEAL_MAX);
        let steal_i = i32::try_from(steal).unwrap_or(0);
        self.deliver(&o.from, day, Effect::Marks, steal_i, &o.to, Lines::HACK_ATT, 0);
        self.deliver(&o.to, day, Effect::Marks, -steal_i, &o.from, Lines::HACK_DEF, 0);
        self.tallies.hacks_in += 1;
        if let Some(p) = self.posted.iter_mut().find(|p| same(&p.handle, &o.to)) {
            p.peace_until = day + PEACE_DAYS;
        }
        if let Some(at) = self.claimable(&a, &o.to) {
            let b = self.bounties.remove(at);
            let reward = i32::try_from(b.reward).unwrap_or(0);
            self.deliver(&o.from, day, Effect::Marks, reward, &o.to, Lines::BOUNTY_ATT, 0);
            self.deliver(&o.to, day, Effect::Fry, 1, &o.from, Lines::BOUNTY_DEF, 0);
            self.deliver(&o.to, day, Effect::Heat, 6, &o.from, Lines::BOUNTY_HEAT, 0);
            if !b.poster.is_empty() {
                self.deliver(&b.poster, day, Effect::None, 0, &o.to, Lines::BOUNTY_DONE, 0);
            }
            self.tallies.bounties_paid += 1;
            self.deed(day, Lines::DEED_BOUNTY, &o.from, b.reward);
        }
    }
}

/// A fixer to hold a bounty: spread by the bounty's id so they are not all one fixer's.
fn fixer_for(content: &Content, id: u32) -> u8 {
    let n = u32::try_from(content.corps.fixer.len().max(1)).unwrap_or(1);
    content
        .corps
        .fixer
        .get((id % n) as usize)
        .map_or(0, |f| f.num)
}

/// The template numbers of `social.toml`: which line a delivery or a deed uses.
pub struct Lines;

impl Lines {
    /// "Your hack is filed."
    pub const FILED: u8 = 1;
    /// "The hack was refused."
    pub const REFUSED: u8 = 2;
    /// The hack got in (attacker).
    pub const HACK_ATT: u8 = 3;
    /// The hack got in (defender).
    pub const HACK_DEF: u8 = 4;
    /// The hack bounced (attacker).
    pub const FAIL_ATT: u8 = 5;
    /// The hack bounced (defender).
    pub const FAIL_DEF: u8 = 6;
    /// A honeypot caught the hack (attacker fined).
    pub const TRAP_ATT: u8 = 7;
    /// A honeypot caught the hack (heat).
    pub const TRAP_HEAT: u8 = 8;
    /// A honeypot caught the hack (defender).
    pub const TRAP_DEF: u8 = 9;
    /// The target left the board.
    pub const GONE: u8 = 10;
    /// A bounty claimed (hacker).
    pub const BOUNTY_ATT: u8 = 11;
    /// A bounty claimed (target).
    pub const BOUNTY_DEF: u8 = 12;
    /// A bounty claimed (target's heat).
    pub const BOUNTY_HEAT: u8 = 13;
    /// A bounty claimed (poster).
    pub const BOUNTY_DONE: u8 = 14;
    /// A bounty filed.
    pub const BOUNTY_FILED: u8 = 15;
    /// A bounty refused.
    pub const REFUSED_BOUNTY: u8 = 16;
    /// A bounty ran out.
    pub const BOUNTY_LAPSED: u8 = 17;
    /// A price on your head.
    pub const BOUNTY_ON_YOU: u8 = 18;
    /// Street feed: a fixer put a price on a name.
    pub const DEED_PRICE: u8 = 30;
    /// Street feed: a bounty was paid.
    pub const DEED_BOUNTY: u8 = 31;
    /// Street feed: a warden went down.
    pub const DEED_WARDEN: u8 = 32;
    /// Street feed: a burn at tier 3.
    pub const DEED_BURN: u8 = 33;
    /// Street feed: a crew finished a job.
    pub const DEED_CREW: u8 = 34;
    /// Street feed: the finale was cracked.
    pub const DEED_FINALE: u8 = 35;
    /// Street feed: a deep breach.
    pub const DEED_DEEP: u8 = 36;
    /// A crew's invitation.
    pub const CREW_INVITED: u8 = 40;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn posted(handle: &str, tier: u8, born: i64) -> Posted {
        Posted {
            handle: handle.into(),
            tier,
            heat: 0,
            honeypot: 0,
            shield: 0,
            edge: 4,
            cash: 20,
            rep: 0,
            born,
            seen: 100,
            peace_until: 0,
            pvp: true,
            crews_open: true,
        }
    }

    #[test]
    fn handles_compare_without_case() {
        assert!(same("Ann", "aNN"));
        assert!(!same("ann", "anne"));
    }

    #[test]
    fn a_repost_keeps_the_worlds_peace_and_a_full_board_drops_the_stalest() {
        let mut s = Social::default();
        s.post(posted("ann", 1, 0));
        s.posted[0].peace_until = 50;
        let mut again = posted("ANN", 1, 0);
        again.cash = 99;
        s.post(again);
        assert_eq!(s.posted.len(), 1);
        assert_eq!((s.posted[0].peace_until, s.posted[0].cash), (50, 99));
        for i in 0..POSTED_MAX + 5 {
            let mut p = posted(&format!("p{i}"), 1, 0);
            p.seen = i64::try_from(i).unwrap() + 1_000;
            s.post(p);
        }
        assert_eq!(s.posted.len(), POSTED_MAX);
    }

    #[test]
    fn the_caps_on_a_hack() {
        let mut s = Social::default();
        s.post(posted("ann", 1, 0));
        s.post(posted("bob", 1, 0));
        s.post(posted("cat", 1, 0));
        s.post(posted("dee", 1, 0));
        s.post(posted("newbie", 1, 95));
        let today = 100;
        assert_eq!(s.check_hack("ann", "ann", today), Err(Why::SelfTarget));
        assert_eq!(s.check_hack("ann", "zed", today), Err(Why::TargetGone));
        assert_eq!(s.check_hack("zed", "bob", today), Err(Why::NotPosted));
        assert_eq!(s.check_hack("ann", "newbie", today), Err(Why::Protected));
        assert_eq!(s.check_hack("ann", "bob", today), Ok(()));
        s.file_hack("ann", "bob", today, 1).unwrap();
        // The same pair is on cooldown, and a second target is the last one for the day.
        assert_eq!(s.check_hack("ann", "bob", today), Err(Why::Cooldown));
        s.file_hack("ann", "cat", today, 2).unwrap();
        assert_eq!(s.check_hack("ann", "dee", today), Err(Why::TooMany));
        // A target takes two waiting hacks at most.
        s.file_hack("cat", "bob", today, 3).unwrap();
        assert_eq!(s.check_hack("dee", "bob", today), Err(Why::Swamped));
        // Every refusal gives the cycles back; every filing leaves a receipt.
        let refused = s.file_hack("dee", "bob", today, 9);
        assert_eq!(refused, Err(Why::Swamped));
        let back = s.deliveries.iter().find(|d| d.nonce == 9).unwrap();
        assert_eq!((back.effect, back.n), (Effect::Cycles, 6));
        assert!(s.deliveries.iter().any(|d| d.nonce == 1 && d.effect == Effect::None));
    }

    #[test]
    fn deliveries_are_capped_per_player_and_in_all() {
        let mut s = Social::default();
        for _ in 0..PER_RECIPIENT_MAX + 6 {
            s.deliver("ann", 1, Effect::Heat, 1, "", Lines::FAIL_DEF, 0);
        }
        assert_eq!(
            s.deliveries.iter().filter(|d| same(&d.to, "ann")).count(),
            PER_RECIPIENT_MAX
        );
        for i in 0..400 {
            s.deliver(&format!("p{i}"), 1, Effect::None, 0, "", 1, 0);
        }
        assert!(s.deliveries.len() <= DELIVERIES_MAX);
        // Ids only rise, so a player's "highest applied" is a sound memory.
        assert!(s.deliveries.windows(2).all(|w| w[0].id < w[1].id));
    }
}
