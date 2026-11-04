//! What happened, as data. The engine returns events; screens (and tests) read them.

// Every variant is documented; its fields are named for what they hold.
#![allow(missing_docs)]

use std::fmt;

use crate::types::{Resource, Trade};

/// Something that happened in the hold or at the market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Someone joined the clan.
    Recruited {
        name: String,
        trade: Trade,
        price: u32,
    },
    /// Someone retired, with an epitaph in the hall.
    Retired { name: String },
    /// Someone was posted to a building (or taken off).
    Posted {
        name: String,
        building: Option<String>,
    },
    /// Work started on a building.
    Started { building: String, plot: u8 },
    /// Labour went into a building.
    Labour { building: String, left: u8 },
    /// A building was finished.
    Completed { building: String, plot: u8 },
    /// A recipe became known.
    Learned { recipe: String },
    /// A shift produced something.
    Produced { what: Resource, qty: u32 },
    /// A building produced something overnight.
    NightProduced {
        building: String,
        what: Resource,
        qty: u32,
    },
    /// A greedy hand kept some of the take.
    Pocketed {
        name: String,
        what: Resource,
        qty: u32,
    },
    /// A lucky hand found extra.
    Lucky { name: String },
    /// Someone was hurt.
    Injured { name: String, injury: String },
    /// An injury healed.
    Healed { name: String, injury: String },
    /// An injury left a mark.
    Scarred { name: String, scar: String },
    /// Two crew became bonded.
    Bonded { a: String, b: String },
    /// A bonded partner was lost.
    Grief { name: String },
    /// A recipe was worked.
    Crafted {
        recipe: String,
        what: Resource,
        qty: u32,
    },
    /// Someone trained.
    Trained {
        name: String,
        skill: Trade,
        level: u8,
    },
    /// A healer tended a patient.
    Tended { healer: String, patient: String },
    /// The nearby marsh was charted.
    Surveyed { tiles: u32 },
    /// Sold at the Tidecourt.
    Sold {
        what: Resource,
        qty: u32,
        tallies: u32,
    },
    /// Bought at the Tidecourt.
    Bought {
        what: Resource,
        qty: u32,
        tallies: u32,
    },
    /// Turns passed waiting for the water.
    Waited { turns: u8, level: i32 },
    /// The sluice was set (or cleared).
    SluiceSet { level: Option<u8> },
    /// The larder fell short.
    Hungry { short: u32 },
    /// The peat fell short.
    Cold { short: u32 },
    /// Someone starved.
    Starved { name: String },
    /// Someone left the clan.
    Deserted { name: String },
    /// Someone had a birthday.
    Aged { name: String, age: u8 },
    /// Days passed while the player was away.
    Night { day: i64 },
    /// The clan was founded.
    Founded { name: String },
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recruited { name, trade, price } => {
                write!(
                    f,
                    "{name}, a {}, joins the clan for {price} tallies.",
                    trade.name()
                )
            }
            Self::Retired { name } => write!(f, "{name} retires to the hearth."),
            Self::Posted {
                name,
                building: Some(b),
            } => write!(f, "{name} takes a post at the {b}."),
            Self::Posted {
                name,
                building: None,
            } => write!(f, "{name} leaves their post."),
            Self::Started { building, plot } => {
                write!(f, "Work begins on the {building} (plot {plot}).")
            }
            Self::Labour { building, left } => {
                write!(f, "The {building}: {left} turns of labour left.")
            }
            Self::Completed { building, plot } => {
                write!(f, "The {building} on plot {plot} is finished.")
            }
            Self::Learned { recipe } => write!(f, "The clan learns to {recipe}."),
            Self::Produced { what, qty } => write!(f, "The shift brings in {qty} {}.", what.name()),
            Self::NightProduced {
                building,
                what,
                qty,
            } => {
                write!(f, "Overnight the {building} made {qty} {}.", what.name())
            }
            Self::Pocketed { name, what, qty } => {
                write!(f, "{name} pockets {qty} {} on the way in.", what.name())
            }
            Self::Lucky { name } => write!(f, "{name} finds a rich seam."),
            Self::Injured { name, injury } => write!(f, "{name} suffers {injury}."),
            Self::Healed { name, injury } => write!(f, "{name} is over {injury}."),
            Self::Scarred { name, scar } => write!(f, "{name} is marked for it: {scar}."),
            Self::Bonded { a, b } => write!(f, "{a} and {b} are bonded now."),
            Self::Grief { name } => write!(f, "{name} grieves."),
            Self::Crafted { recipe, what, qty } => write!(f, "{recipe}: {qty} {}.", what.name()),
            Self::Trained { name, skill, level } => {
                write!(f, "{name} trains {} (level {level}).", skill.name())
            }
            Self::Tended { healer, patient } => write!(f, "{healer} tends {patient}."),
            Self::Surveyed { tiles } => write!(f, "{tiles} new tiles charted."),
            Self::Sold { what, qty, tallies } => {
                write!(f, "Sold {qty} {} for {tallies} tallies.", what.name())
            }
            Self::Bought { what, qty, tallies } => {
                write!(f, "Bought {qty} {} for {tallies} tallies.", what.name())
            }
            Self::Waited { turns, level } => {
                write!(f, "You wait {turns} turns. The water stands at {level}.")
            }
            Self::SluiceSet { level: Some(l) } => {
                write!(f, "The sluice will flood the approach at water {l}.")
            }
            Self::SluiceSet { level: None } => write!(f, "The sluice stays open."),
            Self::Hungry { short } => {
                write!(f, "The larder is short by {short}. The crew go hungry.")
            }
            Self::Cold { short } => write!(f, "No peat for {short} fires. The hold is cold."),
            Self::Starved { name } => write!(f, "{name} starves."),
            Self::Deserted { name } => write!(f, "{name} walks out in the night."),
            Self::Aged { name, age } => write!(f, "{name} is {age} now."),
            Self::Night { day } => write!(f, "Night of day {day} passes."),
            Self::Founded { name } => write!(f, "The {name} clan stakes its hold."),
        }
    }
}
