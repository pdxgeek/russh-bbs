//! What an action did (events) and why it was refused (refusals).
//!
//! Events are data, so tests match on them; [`Event::text`] turns one into the line the screen
//! shows. A refusal changes nothing: every action checks everything before it touches the
//! save, the world or the RNG.

use crate::combat::Range;
use crate::content::{Content, Faction, SlotKind};
use crate::contracts::Kind;
use crate::crew::Role;
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
    /// A fight began.
    FightBegins {
        /// The enemy type.
        enemy: u8,
    },
    /// The captain got away.
    Ran {
        /// Fuel burned.
        fuel: u16,
    },
    /// The getaway failed.
    RunFailed,
    /// The range band changed.
    RangeShift {
        /// The new band.
        to: Range,
        /// Whether the captain's manoeuvre won it.
        by_you: bool,
    },
    /// The captain's guns hit.
    YouHit {
        /// Absorbed by the enemy's shield.
        shield: u16,
        /// Through to the hull.
        hull: u16,
    },
    /// The captain's guns missed.
    YouMissed,
    /// A shot went to the escorted ship.
    WardHit {
        /// Its hull percent now.
        ward: u8,
    },
    /// The shields soaked a volley.
    Shielded,
    /// The captain's hull was hit.
    YouWereHit {
        /// Points lost.
        hull: u16,
    },
    /// The enemy missed.
    EnemyMissed,
    /// The grapple fell short.
    GrappleFailed,
    /// The engineer patched the hull.
    Patched {
        /// Points restored.
        hull: u16,
    },
    /// Boarders were thrown back.
    BoardersRepelled,
    /// Boarders took cargo and cash.
    Boarded {
        /// Tons taken.
        tons: u32,
        /// Credits taken.
        cash: i64,
    },
    /// The enemy broke off and got away.
    EnemyEscaped,
    /// Both sides broke off.
    BrokeOff,
    /// The enemy was wrecked.
    Wrecked {
        /// The enemy type.
        enemy: u8,
        /// Credits salvaged.
        loot: i64,
    },
    /// A bounty target was wrecked.
    BountyHit {
        /// The enemy type.
        enemy: u8,
    },
    /// The captain was beaten.
    Beaten {
        /// Tons taken.
        tons: u32,
        /// Credits taken.
        cash: i64,
        /// A fine added.
        fine: i64,
    },
    /// A hand signed on.
    Hired {
        /// Their name.
        name: String,
        /// Their role.
        role: Role,
        /// The signing fee.
        fee: i64,
    },
    /// A hand was let go.
    Dismissed {
        /// Their name.
        name: String,
    },
    /// Wages paid.
    WagesPaid {
        /// Credits.
        amount: i64,
    },
    /// Wages could not be paid.
    WagesMissed {
        /// Credits owed.
        owed: i64,
    },
    /// Rations bought.
    Provisioned {
        /// Credits.
        cost: i64,
        /// Days covered.
        days: i64,
    },
    /// A hand's goal was met: they are sworn to the ship.
    GoalMet {
        /// Their name.
        name: String,
        /// The goal's wording.
        goal: u8,
    },
    /// A hand left at a dock.
    Deserted {
        /// Their name.
        name: String,
        /// The goal they left with unmet.
        goal: u8,
    },
    /// Several hands left together, with a cut.
    Mutiny {
        /// How many.
        left: u8,
        /// Credits taken.
        stolen: i64,
    },
    /// A hand was hurt.
    CrewHurt {
        /// Their name.
        name: String,
        /// Days until fit.
        days: u8,
        /// Whether it left a scar.
        scarred: bool,
    },
    /// A hand was lost.
    CrewLost {
        /// Their name.
        name: String,
        /// Their role.
        role: Role,
    },
    /// A hand's skill went up.
    CrewSkill {
        /// Their name.
        name: String,
        /// Their role.
        role: Role,
        /// New skill.
        skill: u8,
    },
    /// A boarding began.
    BoardStart {
        /// The layout.
        layout: u8,
    },
    /// The party entered a room.
    RoomEntered {
        /// The layout.
        layout: u8,
        /// The room.
        room: u8,
    },
    /// A sealed door was cut open.
    Breached {
        /// The room behind it.
        room: u8,
    },
    /// The party struck the hostiles.
    Struck {
        /// Killed this round.
        killed: u8,
        /// Left in the room.
        left: u8,
    },
    /// A hand went down.
    Downed {
        /// Their name.
        name: String,
    },
    /// The captain went down.
    CaptainDown,
    /// The hostiles hit the party.
    PartyHit {
        /// Hit points lost in all.
        hp: u16,
    },
    /// The party took a room's loot.
    Looted {
        /// Credits.
        credits: u32,
        /// Hull points of parts.
        parts: u16,
        /// Tons of cargo.
        tons: u16,
    },
    /// A data cache was read.
    CacheRead {
        /// Survey value.
        value: u32,
    },
    /// A wound was patched.
    Patched2 {
        /// Hit points restored.
        hp: u16,
    },
    /// The boarding ended.
    BoardEnd {
        /// Credits brought home.
        credits: u32,
        /// Hull points of parts.
        parts: u16,
        /// Tons brought aboard.
        tons: u16,
        /// Survey value kept.
        survey: u32,
        /// Whether the party got out (otherwise it was wiped out and the haul lost).
        clean: bool,
    },
    /// A transit event opened.
    Transit {
        /// The event.
        event: u16,
    },
    /// The result of a choice.
    Outcome {
        /// The event.
        event: u16,
        /// The option picked.
        option: u8,
        /// The outcome drawn.
        outcome: u8,
    },
    /// Cargo found.
    CargoFound {
        /// The good.
        good: u8,
        /// Tons.
        tons: u16,
    },
    /// Cargo lost.
    CargoLost {
        /// The good.
        good: u8,
        /// Tons.
        tons: u16,
    },
    /// Cargo dumped or taken.
    CargoDumped {
        /// Tons.
        tons: u32,
    },
    /// A contract was taken.
    Accepted {
        /// The kind.
        kind: Kind,
        /// The destination station.
        to: u16,
    },
    /// A contract was finished.
    ContractDone {
        /// The kind.
        kind: Kind,
        /// Who posted it.
        faction: Faction,
        /// Credits paid.
        reward: i64,
    },
    /// A contract failed or was given up.
    ContractFailed {
        /// The kind.
        kind: Kind,
        /// Who posted it.
        faction: Faction,
        /// Whether it ran out of time.
        late: bool,
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
            Self::FightBegins { enemy } => {
                let e = &content.enemies.enemy[usize::from(*enemy)];
                format!(
                    "{} ({}) closes on you. {}",
                    e.name,
                    e.faction.name(),
                    e.blurb
                )
            }
            Self::Ran { fuel } => format!("You broke away and burned {fuel} fuel getting clear."),
            Self::RunFailed => "The getaway failed; they are still on you.".to_owned(),
            Self::RangeShift { to, by_you } => format!(
                "The range is now {}{}.",
                to.name(),
                if *by_you { " (your move)" } else { " (theirs)" }
            ),
            Self::YouHit { shield, hull } => {
                format!("Your guns hit: {shield} to their shields, {hull} to the hull.")
            }
            Self::YouMissed => "Your guns miss.".to_owned(),
            Self::WardHit { ward } => format!("The ship you escort is hit; it is at {ward} %."),
            Self::Shielded => "The shields soak the volley.".to_owned(),
            Self::YouWereHit { hull } => format!("You are hit: {hull} hull points."),
            Self::EnemyMissed => "Their volley goes wide.".to_owned(),
            Self::GrappleFailed => "Their grapples fall short.".to_owned(),
            Self::Patched { hull } => format!("Your engineer patches {hull} hull points."),
            Self::BoardersRepelled => "Boarders thrown back by your crew.".to_owned(),
            Self::Boarded { tons, cash } => format!(
                "Boarders strip the hold: {tons} t gone, {cash} cr taken from the strongbox."
            ),
            Self::EnemyEscaped => "The enemy breaks off and burns for the dark.".to_owned(),
            Self::BrokeOff => "Both ships break off, spent.".to_owned(),
            Self::Wrecked { enemy, loot } => format!(
                "{} is wrecked. Salvage: {loot} cr.",
                content.enemies.enemy[usize::from(*enemy)].name
            ),
            Self::BountyHit { enemy } => format!(
                "That was the bounty: {} taken. Collect at the posting station.",
                content.enemies.enemy[usize::from(*enemy)].name
            ),
            Self::Beaten { tons, cash, fine } => {
                format!("You are beaten. {tons} t taken, {cash} cr taken, fine {fine} cr.")
            }
            Self::Hired { name, role, fee } => {
                format!("{name} signs on as {} (signing fee {fee} cr).", role.name())
            }
            Self::Dismissed { name } => format!("{name} is paid off and leaves the ship."),
            Self::WagesPaid { amount } => format!("The crew's wages were paid: {amount} cr."),
            Self::WagesMissed { owed } => {
                format!("Wages missed: {owed} cr owed. The crew notices.")
            }
            Self::Provisioned { cost, days } => {
                format!("The galley is stocked for {days} days: {cost} cr.")
            }
            Self::GoalMet { name, goal } => format!(
                "{name}: {}",
                content
                    .crew
                    .goal
                    .get(usize::from(*goal))
                    .map_or("It is done.", |g| g.done.as_str())
            ),
            Self::Deserted { name, goal } => format!(
                "{name} left the ship. {}",
                content
                    .crew
                    .goal
                    .get(usize::from(*goal))
                    .map_or("", |g| g.leave.as_str())
            ),
            Self::Mutiny { left, stolen } => format!(
                "Mutiny: {left} hands walk off together and take {stolen} cr from the strongbox."
            ),
            Self::CrewHurt {
                name,
                days,
                scarred,
            } => format!(
                "{name} is hurt and off duty for {days} days{}.",
                if *scarred {
                    "; it will leave a scar"
                } else {
                    ""
                }
            ),
            Self::CrewLost { name, role } => {
                format!("{name}, your {}, is lost.", role.name().to_lowercase())
            }
            Self::CrewSkill { name, role, skill } => {
                format!(
                    "{name} is now a {} of skill {skill}.",
                    role.name().to_lowercase()
                )
            }
            Self::BoardStart { layout } => format!(
                "You go aboard: {}.",
                content.derelicts.derelict[usize::from(*layout)].name
            ),
            Self::RoomEntered { layout, room } => {
                let r = &content.derelicts.derelict[usize::from(*layout)].rooms[usize::from(*room)];
                format!("{}. {}", r.name, r.text)
            }
            Self::Breached { room } => format!("The door to room {room} is cut open."),
            Self::Struck { killed, left } => {
                format!("The party strikes: {killed} down, {left} left in the room.")
            }
            Self::Downed { name } => format!("{name} goes down."),
            Self::CaptainDown => "The captain goes down.".to_owned(),
            Self::PartyHit { hp } => format!("The party takes {hp} points of hurt."),
            Self::Looted {
                credits,
                parts,
                tons,
            } => {
                format!("Taken: {credits} cr, {parts} hull points of parts, {tons} t of cargo.")
            }
            Self::CacheRead { value } => {
                format!("A data cache is read: survey data worth {value} cr.")
            }
            Self::Patched2 { hp } => format!("A wound is patched: {hp} hit points."),
            Self::BoardEnd {
                credits,
                parts,
                tons,
                survey,
                clean,
            } => {
                if *clean {
                    format!(
                        "Back aboard with {credits} cr, {parts} hull points of parts, {tons} t of cargo and {survey} cr of survey data."
                    )
                } else {
                    "The party is wiped out; what was taken is lost.".to_owned()
                }
            }
            Self::Transit { event } => {
                let e = &content.transit.event[usize::from(*event)];
                format!("{}. {}", e.title, e.text)
            }
            Self::Outcome {
                event,
                option,
                outcome,
            } => content.transit.event[usize::from(*event)].options[usize::from(*option)].outcomes
                [usize::from(*outcome)]
            .text
            .clone(),
            Self::CargoFound { good: g, tons } => format!("{tons} t of {} comes aboard.", good(g)),
            Self::CargoLost { good: g, tons } => format!("{tons} t of {} is gone.", good(g)),
            Self::CargoDumped { tons } => format!("{tons} t of cargo is lost."),
            Self::Accepted { kind, to } => {
                format!(
                    "{} contract taken: bring it to {}.",
                    kind.name(),
                    station(to)
                )
            }
            Self::ContractDone {
                kind,
                faction,
                reward,
            } => format!(
                "{} done for the {}: {reward} cr.",
                kind.name(),
                faction.name()
            ),
            Self::ContractFailed {
                kind,
                faction,
                late,
            } => format!(
                "{} for the {} {}.",
                kind.name(),
                faction.name(),
                if *late {
                    "ran out of time"
                } else {
                    "was given up"
                }
            ),
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
    /// A scene is open; finish it first.
    Busy,
    /// No event is waiting for a choice.
    NoEvent,
    /// No fight is open.
    NoFight,
    /// No boarding is open.
    NoBoarding,
    /// More reactor power than the ship has.
    OverPower {
        /// What the reactor can give.
        budget: u8,
    },
    /// A manoeuvre needs engine power.
    NoEnginePower,
    /// An option's requirement is not met.
    Needs(String),
    /// Recruits here will not sign with this captain.
    Unwelcome(Faction),
    /// Eight hands is the most.
    CrewFull,
    /// No free berth.
    NoBerth,
    /// A job needs more standing.
    NeedStanding {
        /// Whose.
        faction: Faction,
        /// What it takes.
        need: i16,
    },
    /// Four jobs at a time.
    TooManyJobs,
    /// Hostiles are in the room.
    FoesHere,
    /// A sealed door: the party's engineering is not up to it.
    Sealed {
        /// The lock's level.
        lock: u8,
    },
    /// Nobody aboard can read the cache.
    NoReader,
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
            Self::Busy => "You are in the middle of something. Finish it first.".to_owned(),
            Self::NoEvent => "Nothing is waiting for a choice.".to_owned(),
            Self::NoFight => "There is no fight to fight.".to_owned(),
            Self::NoBoarding => "There is no boarding under way.".to_owned(),
            Self::OverPower { budget } => {
                format!("The reactor gives {budget} points; the order asks for more.")
            }
            Self::NoEnginePower => {
                "A manoeuvre needs at least one point of engine power.".to_owned()
            }
            Self::Needs(what) => format!("Closed to you: {what}."),
            Self::Unwelcome(f) => format!(
                "Recruits here will not sign with you: the {} have a bad word for your name.",
                f.name()
            ),
            Self::CrewFull => "Eight hands is the most a ship can carry.".to_owned(),
            Self::NoBerth => "No free berth aboard.".to_owned(),
            Self::NeedStanding { faction, need } => format!(
                "The {} will not post that for you: it needs standing {need:+}.",
                faction.name()
            ),
            Self::TooManyJobs => "You are carrying all the jobs you can.".to_owned(),
            Self::FoesHere => "Hostiles are in the room.".to_owned(),
            Self::Sealed { lock } => format!(
                "The door is sealed (lock {lock}); the party's engineering is not up to it."
            ),
            Self::NoReader => {
                "Nobody aboard can read that cache: it needs a scientist or a practised surveyor."
                    .to_owned()
            }
        }
    }
}
