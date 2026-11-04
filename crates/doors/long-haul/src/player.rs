//! One captain's save: purse, debt, ship, skills, standing, what they have seen.
//!
//! The save is the engine's whole per-player state. It is written as a hand-rolled binary blob
//! ([`crate::codec`]) under a format byte; [`Player::decode`] refuses anything it cannot read
//! in full, and `lib.rs` turns a refusal into the framework's corrupt-save error rather than a
//! fresh game. Every credit that enters or leaves the purse goes through [`Player::credit`] or
//! [`Player::debit`], which keep the [`Ledger`] the "no money from nothing" tests audit.

use crate::codec::{Reader, Writer};
use crate::content::{Content, Perk};
use crate::frontier::Frontier;
use crate::ship::{Installed, Lot, MAX_LOTS, Ship};
use crate::skills::{Skill, level_of, xp_for};

/// The save's format byte.
pub const SAVE_FORMAT: u8 = 1;
/// Drive-days a captain gets each real day.
pub const ALLOWANCE: i32 = 10;
/// The longest captain name kept.
pub const CAPTAIN_MAX: usize = 24;
/// Most unsold survey records a captain can carry.
pub const MAX_SURVEY: usize = 24;

/// Why credits moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// The purse a captain starts with.
    Start,
    /// Cargo bought and sold.
    Trade,
    /// Fuel.
    Fuel,
    /// Repairs.
    Repair,
    /// Modules bought and sold.
    Module,
    /// Hulls bought and traded in.
    Hull,
    /// Money borrowed.
    Loan,
    /// Payments to the lender.
    Payment,
    /// Survey data sold.
    Survey,
    /// Fines.
    Fine,
    /// A tow.
    Tow,
    /// Dock fees.
    Dock,
}

/// How many causes there are.
pub const CAUSES: usize = 12;

impl Cause {
    /// Position in a ledger array.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Start => 0,
            Self::Trade => 1,
            Self::Fuel => 2,
            Self::Repair => 3,
            Self::Module => 4,
            Self::Hull => 5,
            Self::Loan => 6,
            Self::Payment => 7,
            Self::Survey => 8,
            Self::Fine => 9,
            Self::Tow => 10,
            Self::Dock => 11,
        }
    }
}

/// Everything that ever entered or left the purse, by cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ledger {
    /// Credits received, by cause.
    pub income: [i64; CAUSES],
    /// Credits paid, by cause.
    pub spend: [i64; CAUSES],
}

impl Ledger {
    /// Net credits that moved through the purse.
    #[must_use]
    pub fn net(&self) -> i64 {
        self.income.iter().sum::<i64>() - self.spend.iter().sum::<i64>()
    }
}

/// The loan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Debt {
    /// What is owed, interest included.
    pub principal: i64,
    /// The next payment day (a real day number).
    pub next_due: i64,
    /// The day interest was last added.
    pub interest_day: i64,
    /// Payments missed in a row.
    pub missed: u8,
}

/// Prices a captain saw the last time they docked somewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intel {
    /// The station.
    pub station: u16,
    /// The real day it was seen.
    pub day: i64,
    /// The middle price of each slot, in the station's slot order.
    pub prices: Vec<u32>,
}

/// Unsold survey data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurveyLot {
    /// The system surveyed.
    pub system: u16,
    /// What the Guild pays, before any perk.
    pub value: u32,
}

/// Counts for the screen and for balance reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counters {
    /// Jumps made.
    pub jumps: u32,
    /// Tons bought and sold.
    pub tons_traded: u32,
    /// Docks inspected while carrying contraband.
    pub inspections: u32,
    /// Of those, times caught.
    pub caught: u32,
    /// Systems charted first.
    pub charted: u32,
    /// Tows.
    pub tows: u32,
    /// Contraband tons sold.
    pub contraband_sold: u32,
}

/// A captain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Player {
    /// The captain's name.
    pub captain: String,
    /// Index into the content's backgrounds.
    pub background: u8,
    /// Experience per skill.
    pub xp: [u32; 5],
    /// Standing with the League, the Miners and the Free Captains.
    pub standing: [i16; 3],
    /// Credits in hand.
    pub cash: i64,
    /// The loan.
    pub debt: Debt,
    /// The ship.
    pub ship: Ship,
    /// The system the ship is in.
    pub at: u16,
    /// The station it is docked at, if any.
    pub docked: Option<u16>,
    /// Drive-days left today (negative when a long jump ran over).
    pub days_left: i32,
    /// The real day last settled.
    pub last_day: i64,
    /// Drive-days spent in all.
    pub voyage_days: u32,
    /// What was seen at each station visited.
    pub intel: Vec<Intel>,
    /// Systems this captain has scanned in depth.
    pub scanned: Vec<u16>,
    /// Unsold survey data.
    pub survey: Vec<SurveyLot>,
    /// Times the lender took the ship.
    pub repossessions: u8,
    /// Credits moved, by cause.
    pub ledger: Ledger,
    /// Counts.
    pub stats: Counters,
    /// Credits of sales since standing last rose, per faction.
    pub turnover: [i64; 3],
}

impl Player {
    /// A new captain at the home station with the background's skills, purse and loan.
    #[must_use]
    pub fn new(
        content: &Content,
        frontier: &Frontier,
        captain: &str,
        background: usize,
        today: i64,
    ) -> Self {
        let bg = content
            .backgrounds
            .get(background)
            .or_else(|| content.backgrounds.first());
        let mut xp = [0u32; 5];
        let (mut cash, mut loan, mut standing, mut scanner) =
            (1500i64, 18_000i64, [0i16; 3], false);
        if let Some(bg) = bg {
            let s = bg.skills;
            for (slot, level) in xp
                .iter_mut()
                .zip([s.trade, s.pilot, s.engineer, s.stealth, s.survey])
            {
                *slot = xp_for(level);
            }
            cash = i64::from(bg.cash);
            loan = i64::from(bg.loan);
            standing = [bg.standing.league, bg.standing.miners, bg.standing.free];
            scanner = bg.perk() == Some(Perk::GuildContact);
        }
        let starts = frontier.start_stations(content);
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        for byte in captain.bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3);
        }
        let pick =
            usize::try_from(hash % u64::try_from(starts.len().max(1)).unwrap_or(1)).unwrap_or(0);
        let home = starts.get(pick).copied().unwrap_or(frontier.home_station);
        let system = frontier.station(home).map_or(0, |s| s.system);
        let mut player = Self {
            captain: captain.chars().take(CAPTAIN_MAX).collect(),
            background: u8::try_from(background.min(content.backgrounds.len().saturating_sub(1)))
                .unwrap_or(0),
            xp,
            standing,
            cash: 0,
            debt: Debt {
                principal: 0,
                next_due: today + 7,
                interest_day: today,
                missed: 0,
            },
            ship: Ship::starter(content, scanner),
            at: system,
            docked: Some(home),
            days_left: ALLOWANCE,
            last_day: today,
            voyage_days: 0,
            intel: Vec::new(),
            scanned: Vec::new(),
            survey: Vec::new(),
            repossessions: 0,
            ledger: Ledger::default(),
            stats: Counters::default(),
            turnover: [0; 3],
        };
        // The loan bought the ship the captain already flies; only the purse is cash in hand.
        player.credit(Cause::Start, cash);
        player.debt.principal = loan;
        player
    }

    /// Add credits to the purse.
    pub fn credit(&mut self, cause: Cause, amount: i64) {
        let amount = amount.max(0);
        self.cash += amount;
        self.ledger.income[cause.index()] += amount;
    }

    /// Take credits from the purse. The caller has already checked there is enough.
    pub fn debit(&mut self, cause: Cause, amount: i64) {
        let amount = amount.max(0).min(self.cash.max(0));
        self.cash -= amount;
        self.ledger.spend[cause.index()] += amount;
    }

    /// A skill's level.
    #[must_use]
    pub fn level(&self, skill: Skill) -> u8 {
        level_of(self.xp[skill.index()])
    }

    /// The captain's perk.
    #[must_use]
    pub fn perk(&self, content: &Content) -> Option<Perk> {
        content
            .backgrounds
            .get(usize::from(self.background))
            .and_then(crate::content::Background::perk)
    }

    /// What the whole outfit would fetch: purse, cargo at cost, ship and modules at resale,
    /// unsold survey data, less the debt.
    #[must_use]
    pub fn net_worth(&self, content: &Content) -> i64 {
        let cargo: i64 = self.ship.cargo.iter().map(|l| l.paid).sum();
        let modules: i64 = self
            .ship
            .modules
            .iter()
            .map(|m| Ship::module_resale(m, content))
            .sum();
        let survey: i64 = self.survey.iter().map(|s| i64::from(s.value)).sum();
        self.cash + cargo + modules + self.ship.hull_resale(content) + survey - self.debt.principal
    }

    /// The most recent intel on a station.
    #[must_use]
    pub fn intel_at(&self, station: u16) -> Option<&Intel> {
        self.intel.iter().find(|i| i.station == station)
    }

    /// The stored form.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(SAVE_FORMAT);
        w.text(&self.captain);
        w.u8(self.background);
        for xp in self.xp {
            w.u32(xp);
        }
        for s in self.standing {
            w.i32(i32::from(s));
        }
        w.i64(self.cash);
        w.i64(self.debt.principal);
        w.i64(self.debt.next_due);
        w.i64(self.debt.interest_day);
        w.u8(self.debt.missed);
        w.u8(self.ship.hull);
        w.u16(self.ship.hull_hp);
        w.u16(self.ship.fuel);
        w.len(self.ship.modules.len());
        for m in &self.ship.modules {
            w.u8(m.module);
            w.u16(m.wear);
        }
        w.len(self.ship.cargo.len());
        for l in &self.ship.cargo {
            w.u8(l.good);
            w.u16(l.tons);
            w.i64(l.paid);
        }
        w.u16(self.at);
        match self.docked {
            None => w.u8(0),
            Some(s) => {
                w.u8(1);
                w.u16(s);
            }
        }
        w.i32(self.days_left);
        w.i64(self.last_day);
        w.u32(self.voyage_days);
        w.len(self.intel.len());
        for i in &self.intel {
            w.u16(i.station);
            w.i64(i.day);
            w.u8(u8::try_from(i.prices.len()).unwrap_or(u8::MAX));
            for &p in &i.prices {
                w.u32(p);
            }
        }
        w.len(self.scanned.len());
        for &s in &self.scanned {
            w.u16(s);
        }
        w.len(self.survey.len());
        for s in &self.survey {
            w.u16(s.system);
            w.u32(s.value);
        }
        w.u8(self.repossessions);
        for v in self.ledger.income.into_iter().chain(self.ledger.spend) {
            w.i64(v);
        }
        let c = self.stats;
        for v in [
            c.jumps,
            c.tons_traded,
            c.inspections,
            c.caught,
            c.charted,
            c.tows,
            c.contraband_sold,
        ] {
            w.u32(v);
        }
        for t in self.turnover {
            w.i64(t);
        }
        w.finish()
    }

    /// Read a save, or `None` if any byte is wrong or missing. `content` and `frontier` bound
    /// every index, so a save that names a good, module, station or system that does not exist
    /// is refused rather than trusted.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn decode(bytes: &[u8], content: &Content, frontier: &Frontier) -> Option<Self> {
        let mut r = Reader::new(bytes);
        if r.u8()? != SAVE_FORMAT {
            return None;
        }
        let captain = r.text()?;
        let background = r.u8()?;
        if usize::from(background) >= content.backgrounds.len() {
            return None;
        }
        let mut xp = [0u32; 5];
        for slot in &mut xp {
            *slot = r.u32()?;
        }
        let mut standing = [0i16; 3];
        for slot in &mut standing {
            *slot = i16::try_from(r.i32()?.clamp(-100, 100)).ok()?;
        }
        let cash = r.i64()?;
        let debt = Debt {
            principal: r.i64()?,
            next_due: r.i64()?,
            interest_day: r.i64()?,
            missed: r.u8()?,
        };
        let hull = r.u8()?;
        if usize::from(hull) >= content.hulls.len() {
            return None;
        }
        let hull_hp = r.u16()?;
        let fuel = r.u16()?;
        let n = r.len(64, 3)?;
        let mut modules = Vec::with_capacity(n);
        for _ in 0..n {
            let module = r.u8()?;
            if usize::from(module) >= content.modules.len() {
                return None;
            }
            modules.push(Installed {
                module,
                wear: r.u16()?.min(crate::ship::WEAR_MAX),
            });
        }
        let n = r.len(MAX_LOTS, 11)?;
        let mut cargo = Vec::with_capacity(n);
        for _ in 0..n {
            let good = r.u8()?;
            if usize::from(good) >= content.goods.len() {
                return None;
            }
            cargo.push(Lot {
                good,
                tons: r.u16()?,
                paid: r.i64()?,
            });
        }
        let at = r.u16()?;
        let docked = match r.u8()? {
            0 => None,
            1 => Some(r.u16()?),
            _ => return None,
        };
        if frontier.system(at).is_none()
            || docked.is_some_and(|s| frontier.station(s).is_none_or(|st| st.system != at))
        {
            return None;
        }
        let days_left = r.i32()?;
        let last_day = r.i64()?;
        let voyage_days = r.u32()?;
        let n = r.len(frontier.stations.len(), 11)?;
        let mut intel = Vec::with_capacity(n);
        for _ in 0..n {
            let station = r.u16()?;
            let day = r.i64()?;
            let k = usize::from(r.u8()?);
            let mut prices = Vec::with_capacity(k);
            for _ in 0..k {
                prices.push(r.u32()?);
            }
            frontier.station(station)?;
            intel.push(Intel {
                station,
                day,
                prices,
            });
        }
        let n = r.len(frontier.systems.len(), 2)?;
        let mut scanned = Vec::with_capacity(n);
        for _ in 0..n {
            scanned.push(r.u16()?);
        }
        let n = r.len(MAX_SURVEY, 6)?;
        let mut survey = Vec::with_capacity(n);
        for _ in 0..n {
            survey.push(SurveyLot {
                system: r.u16()?,
                value: r.u32()?,
            });
        }
        let repossessions = r.u8()?;
        let mut ledger = Ledger::default();
        for slot in ledger.income.iter_mut().chain(ledger.spend.iter_mut()) {
            *slot = r.i64()?;
        }
        let stats = Counters {
            jumps: r.u32()?,
            tons_traded: r.u32()?,
            inspections: r.u32()?,
            caught: r.u32()?,
            charted: r.u32()?,
            tows: r.u32()?,
            contraband_sold: r.u32()?,
        };
        let mut turnover = [0i64; 3];
        for slot in &mut turnover {
            *slot = r.i64()?;
        }
        r.done().then_some(Self {
            captain,
            background,
            xp,
            standing,
            cash,
            debt,
            ship: Ship {
                hull,
                hull_hp,
                fuel,
                modules,
                cargo,
            },
            at,
            docked,
            days_left,
            last_day,
            voyage_days,
            intel,
            scanned,
            survey,
            repossessions,
            ledger,
            stats,
            turnover,
        })
    }
}
