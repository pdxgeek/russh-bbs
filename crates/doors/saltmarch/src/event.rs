//! What happened, as data. The engine returns events; screens (and tests) read them.

// Every variant is documented; its fields are named for what they hold.
#![allow(missing_docs)]

use std::fmt;

use crate::combat::Verdict;
use crate::trip::Ending;
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
    /// A party set out.
    Departed { crew: u8 },
    /// The party reached a place.
    Arrived { place: String },
    /// A scene began.
    Scene { title: String, text: String },
    /// A line of story: what a choice came to.
    Told(String),
    /// Goods went into the pack.
    Found { what: Resource, qty: u32 },
    /// Tallies went into the pack.
    FoundTallies { qty: u32 },
    /// Goods were left behind: the pack was full.
    LeftBehind { what: Resource, qty: u32 },
    /// Goods were lost or dropped.
    Dropped { what: Resource, qty: u32 },
    /// Someone died.
    Died { name: String },
    /// A line of a skirmish.
    Fight(String),
    /// A skirmish ended.
    Skirmish { verdict: Verdict },
    /// The party made camp.
    Camped { turns: u8, level: i32 },
    /// A trip ended.
    TripEnded {
        ending: Ending,
        value: u32,
        turns: u16,
    },
}

impl fmt::Display for Event {
    #[allow(clippy::too_many_lines)]
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
            Self::Departed { crew } => write!(
                f,
                "{crew} set{} out from the hold.",
                if *crew == 1 { "s" } else { "" }
            ),
            Self::Arrived { place } => write!(f, "You reach {place}."),
            Self::Scene { title, text } => write!(f, "{title}. {text}"),
            Self::Told(text) | Self::Fight(text) => f.write_str(text),
            Self::Found { what, qty } => write!(f, "+{qty} {}.", what.name()),
            Self::FoundTallies { qty } => write!(f, "+{qty} tallies."),
            Self::LeftBehind { what, qty } => {
                write!(f, "No room: {qty} {} left behind.", what.name())
            }
            Self::Dropped { what, qty } => write!(f, "-{qty} {}.", what.name()),
            Self::Died { name } => write!(f, "{name} is dead."),
            Self::Skirmish { verdict } => f.write_str(match verdict {
                Verdict::Won => "The skirmish is won.",
                Verdict::Lost => "The party is beaten.",
                Verdict::Fled => "The party breaks away.",
                Verdict::Washed => "The rising water ends the fight.",
                Verdict::Stalemate => "Neither side can finish it, and both draw off.",
            }),
            Self::Camped { turns, level } => {
                write!(f, "You camp {turns} turns. The water stands at {level}.")
            }
            Self::TripEnded {
                ending,
                value,
                turns,
            } => match ending {
                Ending::Home => write!(
                    f,
                    "Home after {turns} turns with {value} tallies' worth of cargo."
                ),
                Ending::Stranded | Ending::Overnight => write!(
                    f,
                    "Cut off after {turns} turns. The party limps home with what it can carry ({value} tallies' worth)."
                ),
                Ending::Swept => write!(
                    f,
                    "The flood sweeps the party off the ground. They wash up at the hold with {value} tallies' worth."
                ),
                Ending::Routed => {
                    f.write_str("Beaten and robbed, the survivors crawl home with nothing.")
                }
                Ending::Lost => f.write_str("Nobody comes back."),
            },
        }
    }
}
