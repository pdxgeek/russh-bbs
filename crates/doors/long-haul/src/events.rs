//! What an action did (events) and why it was refused (refusals).
//!
//! Events are data, so tests match on them; [`Event::text`] turns one into the line the screen
//! shows. A refusal changes nothing: every action checks everything before it touches the
//! save, the world or the RNG.

use crate::content::{Content, Faction, SlotKind};
use crate::frontier::{Frontier, Hazard};
use crate::skills::Skill;

/// Something that happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A new real day: the drive allowance came back.
    NewDay {
        /// Drive-days now available.
        days: i32,
    },
    /// Interest was added to the loan.
    Interest {
        /// Credits added.
        added: i64,
    },
    /// A weekly payment was taken.
    DuePaid {
        /// Credits taken.
        amount: i64,
    },
    /// A weekly payment could not be met.
    DueMissed {
        /// What was short.
        short: i64,
        /// The penalty added to the loan.
        penalty: i64,
        /// Payments missed in a row.
        missed: u8,
    },
    /// The lender took the ship.
    Repossessed {
        /// The fee added to the loan.
        fee: i64,
    },
    /// The ship docked.
    Docked {
        /// The station.
        station: u16,
        /// The fee, paid or added to the loan.
        fee: i64,
    },
    /// The ship left the dock.
    Undocked,
    /// Customs boarded a ship carrying contraband and found it.
    Seized {
        /// Tons taken.
        tons: u32,
        /// The fine, paid or added to the loan.
        fine: i64,
        /// Whose station it was.
        by: Faction,
    },
    /// Customs boarded a ship carrying contraband and found nothing.
    Slipped,
    /// A jump.
    Jumped {
        /// Where from.
        from: u16,
        /// Where to.
        to: u16,
        /// Drive-days spent.
        days: u32,
        /// Fuel burned.
        fuel: u16,
    },
    /// The drive faulted mid-jump.
    DriveFault {
        /// Extra drive-days lost.
        extra_days: u32,
    },
    /// A hazard on arrival.
    Hazard {
        /// Which.
        kind: Hazard,
        /// Structural points lost.
        hull_lost: u16,
        /// Drive-days lost.
        days_lost: u32,
        /// Wear added to every module, tenths of a percent.
        wear: u16,
        /// Whether a captain's skill or scanner avoided it.
        avoided: bool,
    },
    /// A system charted for the first time, and the survey it yielded.
    Charted {
        /// The system.
        system: u16,
        /// Survey value gathered.
        value: u32,
    },
    /// A deep scan of a charted system.
    Scanned {
        /// The system.
        system: u16,
        /// Survey value gathered.
        value: u32,
    },
    /// Cargo bought.
    Bought {
        /// The good.
        good: u8,
        /// Tons.
        tons: u32,
        /// Credits paid.
        total: i64,
    },
    /// Cargo sold.
    Sold {
        /// The good.
        good: u8,
        /// Tons.
        tons: u32,
        /// Credits received.
        total: i64,
        /// Credits over what the tons cost.
        profit: i64,
    },
    /// Fuel bought.
    Refueled {
        /// Units.
        units: u16,
        /// Credits paid.
        cost: i64,
    },
    /// Gas skimmed.
    Skimmed {
        /// Fuel gained.
        units: u16,
        /// Hull lost to the skim.
        hull_lost: u16,
    },
    /// Repairs done.
    Repaired {
        /// Credits paid.
        cost: i64,
        /// Drive-days spent.
        days: u32,
    },
    /// A module bought.
    ModuleBought {
        /// The module.
        module: u8,
        /// Credits paid.
        price: i64,
    },
    /// A module sold.
    ModuleSold {
        /// The module.
        module: u8,
        /// Credits received.
        price: i64,
    },
    /// A hull bought.
    HullBought {
        /// The hull.
        hull: u8,
        /// Net credits paid after the trade-in.
        net: i64,
        /// Credits from modules that did not fit.
        modules_sold: i64,
    },
    /// Money borrowed.
    Borrowed {
        /// Credits.
        amount: i64,
    },
    /// Money repaid.
    Repaid {
        /// Credits.
        amount: i64,
    },
    /// Survey data sold.
    SurveySold {
        /// Records sold.
        lots: u32,
        /// Credits received.
        total: i64,
    },
    /// A tow to a station.
    Towed {
        /// Where.
        to: u16,
        /// The fee, paid or added to the loan.
        fee: i64,
        /// Drive-days lost.
        days: u32,
    },
    /// A skill went up a level.
    SkillUp {
        /// Which.
        skill: Skill,
        /// New level.
        level: u8,
    },
    /// Standing with a faction changed.
    Standing {
        /// Which.
        faction: Faction,
        /// By how much.
        change: i16,
    },
}

impl Event {
    /// The line for the screen.
    #[must_use]
    #[allow(
        clippy::too_many_lines,
        reason = "one flat match with a line per event"
    )]
    pub fn text(&self, content: &Content, frontier: &Frontier) -> String {
        let good = |g: &u8| {
            content
                .goods
                .get(usize::from(*g))
                .map_or("goods", |g| g.name.as_str())
        };
        let module = |m: &u8| {
            content
                .modules
                .get(usize::from(*m))
                .map_or("module", |m| m.name.as_str())
        };
        let hull = |h: &u8| {
            content
                .hulls
                .get(usize::from(*h))
                .map_or("hull", |h| h.name.as_str())
        };
        let station = |s: &u16| {
            frontier
                .station(*s)
                .map_or("a station", |s| s.name.as_str())
        };
        let system = |s: &u16| frontier.system(*s).map_or("space", |s| s.name.as_str());
        match self {
            Self::NewDay { days } => format!("A new day. {days} drive-days on the board."),
            Self::Interest { added } => format!("Interest added to the loan: {added} cr."),
            Self::DuePaid { amount } => {
                format!("The lender's weekly payment was taken: {amount} cr.")
            }
            Self::DueMissed {
                short,
                penalty,
                missed,
            } => format!(
                "Weekly payment missed ({short} cr short). Penalty {penalty} cr added. Missed in a row: {missed}."
            ),
            Self::Repossessed { fee } => format!(
                "The lender repossessed your ship. You are back in a hand-me-down courier, and {fee} cr of fees went on the loan."
            ),
            Self::Docked { station: s, fee } => {
                format!("Docked at {}. Dock fee {fee} cr.", station(s))
            }
            Self::Undocked => "Undocked.".to_owned(),
            Self::Seized { tons, fine, by } => format!(
                "{} customs found contraband: {tons} t seized, fine {fine} cr, standing with the {} falls.",
                by.name(),
                by.name()
            ),
            Self::Slipped => {
                "Customs boarded and found nothing. Your hold kept its secrets.".to_owned()
            }
            Self::Jumped {
                from,
                to,
                days,
                fuel,
            } => format!(
                "Jumped {} to {}: {days} drive-days, {fuel} fuel.",
                system(from),
                system(to)
            ),
            Self::DriveFault { extra_days } => {
                format!("The worn drive faulted mid-jump. {extra_days} extra drive-days lost.")
            }
            Self::Hazard {
                kind,
                hull_lost,
                days_lost,
                wear,
                avoided,
            } => {
                if *avoided {
                    format!("The {} was read and skirted without loss.", kind.name())
                } else {
                    format!(
                        "The {} cost you {hull_lost} hull points, {days_lost} drive-days and {}.{}% wear on every module.",
                        kind.name(),
                        wear / 10,
                        wear % 10
                    )
                }
            }
            Self::Charted { system: s, value } => format!(
                "{} is uncharted no longer: it goes on the chart under your name. Survey data worth {value} cr.",
                system(s)
            ),
            Self::Scanned { system: s, value } => {
                format!(
                    "Deep scan of {} done. Survey data worth {value} cr.",
                    system(s)
                )
            }
            Self::Bought {
                good: g,
                tons,
                total,
            } => format!("Bought {tons} t {} for {total} cr.", good(g)),
            Self::Sold {
                good: g,
                tons,
                total,
                profit,
            } => {
                format!(
                    "Sold {tons} t {} for {total} cr ({profit:+} cr on cost).",
                    good(g)
                )
            }
            Self::Refueled { units, cost } => format!("Took on {units} fuel for {cost} cr."),
            Self::Skimmed { units, hull_lost } => {
                if *hull_lost > 0 {
                    format!(
                        "Skimmed {units} fuel from the giant, and the turbulence cost {hull_lost} hull points."
                    )
                } else {
                    format!("Skimmed {units} fuel from the giant.")
                }
            }
            Self::Repaired { cost, days } => {
                format!("Repairs done: {cost} cr and {days} drive-days.")
            }
            Self::ModuleBought { module: m, price } => {
                format!("Fitted a {} for {price} cr.", module(m))
            }
            Self::ModuleSold { module: m, price } => {
                format!("Sold the {} for {price} cr.", module(m))
            }
            Self::HullBought {
                hull: h,
                net,
                modules_sold,
            } => format!(
                "Bought a {} for {net} cr net of the trade-in. Modules that did not fit sold for {modules_sold} cr.",
                hull(h)
            ),
            Self::Borrowed { amount } => format!("Borrowed {amount} cr. It goes on the loan."),
            Self::Repaid { amount } => format!("Paid {amount} cr toward the loan."),
            Self::SurveySold { lots, total } => {
                format!("The Guild bought {lots} survey records for {total} cr.")
            }
            Self::Towed { to, fee, days } => {
                format!("Towed to {}: fee {fee} cr, {days} drive-days.", station(to))
            }
            Self::SkillUp { skill, level } => format!("{} is now level {level}.", skill.name()),
            Self::Standing { faction, change } => {
                format!("Standing with the {}: {change:+}.", faction.name())
            }
        }
    }
}

/// Why an action was refused. Nothing changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Not docked.
    NotDocked,
    /// Already docked.
    AlreadyDocked,
    /// No such station, system or item.
    NoSuchThing,
    /// The target is not in this system.
    NotHere,
    /// No lane there.
    NoLane,
    /// Not enough fuel.
    NoFuel {
        /// Fuel needed.
        need: u16,
    },
    /// No drive-days left today.
    NoDays,
    /// The hull is too weak to jump.
    HullTooWeak,
    /// The faction will not let this ship dock.
    Barred(Faction),
    /// The military posts know a smuggler when they see one.
    KnownSmuggler,
    /// The haven does not sell to strangers.
    NotTrusted,
    /// The station will not sell that.
    NotForSale,
    /// Nothing in stock.
    NoStock,
    /// The warehouse is full.
    WarehouseFull,
    /// The hold is full.
    HoldFull,
    /// Not enough credits.
    NoCash {
        /// Credits needed.
        need: i64,
    },
    /// Nothing aboard to sell.
    NothingToSell,
    /// The station has no shipyard.
    NoShipyard,
    /// The station has no repair dock.
    NoRepair,
    /// Nothing to repair.
    NothingToRepair,
    /// No free slot of that kind.
    SlotsFull(SlotKind),
    /// The last drive cannot be sold.
    LastDrive,
    /// The tank is full.
    TankFull,
    /// No gas giant here.
    NoGasGiant,
    /// A scanner is needed.
    NoScanner,
    /// Already scanned by this captain.
    AlreadyScanned,
    /// Nothing to do.
    NothingToDo,
    /// The loan would pass the lender's limit.
    OverLimit {
        /// The limit.
        limit: i64,
    },
    /// The hold could not take the load.
    HoldWouldOverflow,
    /// Only the Cartographers' Guild buys survey data, and it keeps no office here.
    NoGuild,
    /// A tow is only for a ship that cannot jump anywhere.
    NotStranded,
}

impl Refusal {
    /// The line for the screen.
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::NotDocked => "You are not docked anywhere.".to_owned(),
            Self::AlreadyDocked => "Already docked.".to_owned(),
            Self::NoSuchThing => "There is no such thing here.".to_owned(),
            Self::NotHere => "That is not in this system.".to_owned(),
            Self::NoLane => "There is no lane there from here.".to_owned(),
            Self::NoFuel { need } => format!("Not enough fuel: that jump needs {need}."),
            Self::NoDays => "The drive is spent for today. Come back tomorrow.".to_owned(),
            Self::HullTooWeak => "The hull is too weak to jump. Repair it first.".to_owned(),
            Self::Barred(f) => format!(
                "The {} will not let you dock. Your name is mud with them.",
                f.name()
            ),
            Self::KnownSmuggler => {
                "The military post turns you away: they know your reputation.".to_owned()
            }
            Self::NotTrusted => "The haven does not sell to strangers yet.".to_owned(),
            Self::NotForSale => "They will not sell that here.".to_owned(),
            Self::NoStock => "None in stock.".to_owned(),
            Self::WarehouseFull => "Their warehouse is full of it.".to_owned(),
            Self::HoldFull => "Your hold is full.".to_owned(),
            Self::NoCash { need } => format!("You cannot cover that: {need} cr needed."),
            Self::NothingToSell => "You have none of that aboard.".to_owned(),
            Self::NoShipyard => "No shipyard here.".to_owned(),
            Self::NoRepair => "No repair dock here.".to_owned(),
            Self::NothingToRepair => "Nothing needs repair.".to_owned(),
            Self::SlotsFull(k) => format!("No free {} slot.", k.name()),
            Self::LastDrive => "You cannot sell your only drive.".to_owned(),
            Self::TankFull => "The tank is full.".to_owned(),
            Self::NoGasGiant => "There is no gas giant to skim here.".to_owned(),
            Self::NoScanner => "You need a working scanner for that.".to_owned(),
            Self::AlreadyScanned => "You have already scanned this system.".to_owned(),
            Self::NothingToDo => "Nothing to do.".to_owned(),
            Self::OverLimit { limit } => format!("The lender will not carry more than {limit} cr."),
            Self::HoldWouldOverflow => {
                "The new hold could not take your cargo. Sell some first.".to_owned()
            }
            Self::NoGuild => "No Cartographers' Guild office here.".to_owned(),
            Self::NotStranded => {
                "You can still jump somewhere; the tow yard does not take that call.".to_owned()
            }
        }
    }
}
