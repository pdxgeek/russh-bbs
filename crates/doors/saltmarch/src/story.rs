//! The story layer (M4-S5): the three factions and their five tiers of standing, the hand-written
//! quest chains, the daily contract boards, the heirloom a clan carries between seasons and the
//! clan's outbox to the world (tolls, causeways, changes to the map).
//!
//! Quests are **data over marks**. A chain is a list of steps; a step is done when the clan holds
//! its mark, and the scenes that set the marks are ordinary expedition scenes gated by the
//! marks before them. So a branch that closes a door is a scene option that sets one mark and
//! removes another, and the chain's log is nothing but a view of the clan's marks.
//!
//! Contracts are the faction boards' daily work: four offers a day, drawn from the season's seed
//! and the day, from the templates the clan's standing with that faction has opened. One is
//! taken at a time.

use russh_bbs_door::DoorRng;
use russh_bbs_door::content::{Table, plain_text};
use serde::Deserialize;

use crate::codec::{Reader, Writer};
use crate::map::PoiKind;
use crate::types::Resource;

/// Quests table text.
pub const QUESTS_TOML: &str = concat!(
    include_str!("../content/quests.toml"),
    "\n",
    include_str!("../content/quests-b.toml"),
    "\n",
    include_str!("../content/quests-c.toml"),
);
/// Contracts table text.
pub const CONTRACTS_TOML: &str = include_str!("../content/contracts.toml");

/// Bytes of branch telemetry kept (about 1500 branches).
pub const SEEN_CAP: usize = 256;

/// The most outbox orders a clan keeps waiting.
pub const OUTBOX_CAP: usize = 8;
/// The most contracts offered on a board in a day.
pub const OFFERS: usize = 4;
/// Standing is kept between these.
pub const REP_MIN: i16 = -50;
/// Standing is kept between these.
pub const REP_MAX: i16 = 100;
/// The most causeway tiles one clan may lay in a season.
pub const LAY_MAX: u8 = 6;
/// The default toll a clan sets on what it lays, in tallies.
pub const TOLL_DEFAULT: u8 = 2;
/// The highest toll.
pub const TOLL_MAX: u8 = 5;

/// The three marsh factions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Faction {
    /// The Rakers' guild: salt, tools and the old trades.
    Rakers,
    /// The Tidecourt: the neutral town, its market and its law.
    Court,
    /// The Reed-folk: those who never left the marsh.
    Reed,
}

impl Faction {
    /// All three.
    pub const ALL: [Self; 3] = [Self::Rakers, Self::Court, Self::Reed];

    /// The id used in content.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Rakers => "rakers",
            Self::Court => "court",
            Self::Reed => "reed",
        }
    }

    /// The name on a board.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rakers => "Rakers' Guild",
            Self::Court => "The Tidecourt",
            Self::Reed => "The Reed-folk",
        }
    }

    /// Index into [`Story::rep`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// By content id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }
}

/// The names of the five tiers, lowest first.
pub const TIERS: [&str; 5] = ["Outcast", "Stranger", "Known", "Trusted", "Honoured"];

/// The tier (0 to 4) of a standing. Outcast closes the faction's boards and scenes.
#[must_use]
pub const fn tier_of(standing: i16) -> u8 {
    if standing < -20 {
        0
    } else if standing < 15 {
        1
    } else if standing < 40 {
        2
    } else if standing < 70 {
        3
    } else {
        4
    }
}

/// What a clan carries into the next season: one thing, and it never stacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heirloom {
    /// The league's winner: a purse of tallies.
    SaltCrown,
    /// A clan that stood in the Deep: a lantern that spares peat.
    DeepLantern,
    /// The clan with the longest pact: crew start the season in better heart.
    OathRing,
    /// The best raider: arms for the first expeditions.
    RaidersHorn,
    /// The Tidecourt's honoured: a seat that opens the board a tier early.
    CourtSeal,
    /// The Reed-folk's honoured: a plan the reed-witches kept.
    ReedKnot,
}

impl Heirloom {
    /// Every heirloom, in the order the season's end offers them (the first earned is kept).
    pub const ALL: [Self; 6] = [
        Self::SaltCrown,
        Self::DeepLantern,
        Self::CourtSeal,
        Self::ReedKnot,
        Self::OathRing,
        Self::RaidersHorn,
    ];

    /// Save code (never zero).
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8 + 1
    }

    /// From a save code.
    #[must_use]
    pub fn from_code(c: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.code() == c)
    }

    /// The name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SaltCrown => "the Salt Crown",
            Self::DeepLantern => "the Deep Lantern",
            Self::OathRing => "the Oath-ring",
            Self::RaidersHorn => "the Raider's Horn",
            Self::CourtSeal => "the Court Seal",
            Self::ReedKnot => "the Reed Knot",
        }
    }

    /// What it does, in a line.
    #[must_use]
    pub const fn effect(self) -> &'static str {
        match self {
            Self::SaltCrown => "starts the season with 25 more tallies",
            Self::DeepLantern => "starts with 6 more peat and the Deep's gate charted",
            Self::OathRing => "the crew start in better heart (+12 morale)",
            Self::RaidersHorn => "starts with 3 arms",
            Self::CourtSeal => "starts known to the Tidecourt (standing 15)",
            Self::ReedKnot => "starts known to the Reed-folk (standing 15) and knowing a plan",
        }
    }
}

/// A contract the clan has taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Active {
    /// The template.
    pub id: String,
    /// The last day it can be done (inclusive).
    pub deadline: i64,
    /// Fights won or goods counted so far.
    pub progress: u16,
}

/// Something the clan has done that the world must hear about. It is kept in the save until
/// the session has sent it, so a dropped connection loses nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    /// A toll paid to a causeway's builder.
    Toll {
        /// The builder's clan name.
        owner: String,
        /// Tallies.
        tallies: u16,
    },
    /// A causeway tile laid at a place, with the toll the builder set.
    Lay {
        /// Tile.
        x: u8,
        /// Tile.
        y: u8,
        /// Toll in tallies.
        toll: u8,
    },
    /// A change to the marsh itself for everyone (a quest's outcome).
    Flag(Flag),
}

/// A change to the map that a quest can make for the whole board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Flag {
    /// The channels near the Tidecourt are dredged: boats are no longer needed there.
    Dredge,
    /// A storm-surge takes the oldest causeway on the board.
    Washout,
    /// The eel lairs are cleared for everyone.
    ClearEels,
    /// The bog-hag's lairs are cleared for everyone.
    ClearHags,
}

impl Flag {
    /// Every flag.
    pub const ALL: [Self; 4] = [
        Self::Dredge,
        Self::Washout,
        Self::ClearEels,
        Self::ClearHags,
    ];

    /// The bit in the world's flags.
    #[must_use]
    pub const fn bit(self) -> u8 {
        1 << (self as u8)
    }

    /// The id used in content.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Dredge => "dredge",
            Self::Washout => "washout",
            Self::ClearEels => "clear-eels",
            Self::ClearHags => "clear-hags",
        }
    }

    /// By content id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }

    /// A line for the news.
    #[must_use]
    pub const fn news(self) -> &'static str {
        match self {
            Self::Dredge => "The channels by the Tidecourt have been dredged: the flats there can be walked.",
            Self::Washout => "A surge has taken the oldest causeway on the marsh.",
            Self::ClearEels => "The eel lairs are cleared. The channels are quieter for everyone.",
            Self::ClearHags => "The bog-hag's lairs are cleared. The peat is safer for everyone.",
        }
    }
}

/// What a clan has done that the story reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Story {
    /// Standing with each faction, -50 to 100.
    pub rep: [i16; 3],
    /// The contract in hand.
    pub contract: Option<Active>,
    /// Contracts finished this season.
    pub done: u16,
    /// Contracts that ran out.
    pub failed: u16,
    /// The heirloom carried from the season before (0: none).
    pub heirloom: u8,
    /// The toll this clan sets on what it lays.
    pub toll: u8,
    /// Causeway tiles laid this season.
    pub laid: u8,
    /// Tolls paid this season.
    pub tolls_paid: u32,
    /// Tolls earned on the clan's causeways this season.
    pub toll_income: u32,
    /// Orders for the world.
    pub outbox: Vec<Out>,
    /// Which branches of the scenes this clan has taken (two bits for each option of each
    /// scene in table order: won, lost). Telemetry for the balance runs and the hall; no rule
    /// reads it.
    pub seen: Vec<u8>,
}

impl Default for Story {
    fn default() -> Self {
        Self {
            rep: [0; 3],
            contract: None,
            done: 0,
            failed: 0,
            heirloom: 0,
            toll: TOLL_DEFAULT,
            laid: 0,
            tolls_paid: 0,
            toll_income: 0,
            outbox: Vec::new(),
            seen: Vec::new(),
        }
    }
}

impl Story {
    /// The tier of standing with `f`.
    #[must_use]
    pub fn tier(&self, f: Faction) -> u8 {
        tier_of(self.rep[f.index()])
    }

    /// Move standing; returns the tiers before and after.
    pub fn add_rep(&mut self, f: Faction, by: i16) -> (u8, u8) {
        let before = self.tier(f);
        let r = &mut self.rep[f.index()];
        *r = (*r + by).clamp(REP_MIN, REP_MAX);
        (before, self.tier(f))
    }

    /// The heirloom, if the clan has one.
    #[must_use]
    pub fn heirloom(&self) -> Option<Heirloom> {
        Heirloom::from_code(self.heirloom)
    }

    /// Note that branch `bit` of the scenes was taken.
    pub fn note_branch(&mut self, bit: usize) {
        let byte = bit / 8;
        if byte >= SEEN_CAP {
            return;
        }
        if self.seen.len() <= byte {
            self.seen.resize(byte + 1, 0);
        }
        self.seen[byte] |= 1 << (bit % 8);
    }

    /// Whether branch `bit` was taken.
    #[must_use]
    pub fn branch_seen(&self, bit: usize) -> bool {
        self.seen
            .get(bit / 8)
            .is_some_and(|b| b & (1 << (bit % 8)) != 0)
    }

    /// Queue an order for the world (the oldest goes if the outbox is full of tolls).
    pub fn queue(&mut self, out: Out) {
        if self.outbox.len() >= OUTBOX_CAP {
            self.outbox.remove(0);
        }
        self.outbox.push(out);
    }

    /// Write.
    pub fn write(&self, w: &mut Writer) {
        for r in self.rep {
            w.u16(u16::from_le_bytes(r.to_le_bytes()));
        }
        match &self.contract {
            Some(a) => {
                w.bool(true);
                w.str(&a.id);
                w.i64(a.deadline);
                w.u16(a.progress);
            }
            None => w.bool(false),
        }
        w.u16(self.done);
        w.u16(self.failed);
        w.u8(self.heirloom);
        w.u8(self.toll);
        w.u8(self.laid);
        w.u32(self.tolls_paid);
        w.u32(self.toll_income);
        w.u16(u16::try_from(self.seen.len()).unwrap_or(0));
        w.raw(&self.seen);
        w.count(self.outbox.len());
        for o in &self.outbox {
            match o {
                Out::Toll { owner, tallies } => {
                    w.u8(0);
                    w.str(owner);
                    w.u16(*tallies);
                }
                Out::Lay { x, y, toll } => {
                    w.u8(1);
                    w.u8(*x);
                    w.u8(*y);
                    w.u8(*toll);
                }
                Out::Flag(f) => {
                    w.u8(2);
                    w.u8(*f as u8);
                }
            }
        }
    }

    /// Read back, or `None` for bad data.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        let mut rep = [0i16; 3];
        for slot in &mut rep {
            *slot = i16::from_le_bytes(r.u16()?.to_le_bytes()).clamp(REP_MIN, REP_MAX);
        }
        let contract = if r.bool()? {
            Some(Active {
                id: r.str()?,
                deadline: r.i64()?,
                progress: r.u16()?,
            })
        } else {
            None
        };
        let (done, failed, heirloom, toll, laid, tolls_paid) =
            (r.u16()?, r.u16()?, r.u8()?, r.u8()?, r.u8()?, r.u32()?);
        let toll_income = r.u32()?;
        let seen_len = usize::from(r.u16()?);
        if seen_len > SEEN_CAP {
            return None;
        }
        let seen = r.take(seen_len)?.to_vec();
        let n = r.u8()?;
        let mut outbox = Vec::new();
        for _ in 0..n {
            outbox.push(match r.u8()? {
                0 => Out::Toll {
                    owner: r.str()?,
                    tallies: r.u16()?,
                },
                1 => Out::Lay {
                    x: r.u8()?,
                    y: r.u8()?,
                    toll: r.u8()?,
                },
                2 => Out::Flag(*Flag::ALL.get(usize::from(r.u8()?))?),
                _ => return None,
            });
        }
        (heirloom == 0 || Heirloom::from_code(heirloom).is_some()).then_some(Self {
            rep,
            contract,
            done,
            failed,
            heirloom,
            toll: toll.min(TOLL_MAX),
            laid,
            tolls_paid,
            toll_income,
            outbox,
            seen,
        })
    }
}

// ------------------------------------------------------------------ contracts

/// What a contract asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContractKind {
    /// Hand over goods from the stores at the Tidecourt.
    Deliver,
    /// Win fights against a kind of foe.
    Clear,
    /// Bring goods home in the cargo of an expedition.
    Salvage,
    /// Take a party to a kind of place and be seen there.
    Escort,
}

impl ContractKind {
    /// A word for the board.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Deliver => "deliver",
            Self::Clear => "clear",
            Self::Salvage => "salvage",
            Self::Escort => "escort",
        }
    }
}

/// One contract template.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractDef {
    /// Stable id.
    pub id: String,
    /// What it asks.
    pub kind: ContractKind,
    /// Whose board.
    pub faction: Faction,
    /// The tier of standing that opens it (1 to 4).
    pub tier: u8,
    /// The heading on the board.
    pub title: String,
    /// What the poster says.
    pub text: String,
    /// The good (deliver, salvage).
    #[serde(default)]
    pub good: Option<String>,
    /// How many (goods, or fights won for a clear).
    pub qty: u16,
    /// The foe (clear): fights that include this foe count.
    #[serde(default)]
    pub foe: Option<String>,
    /// The place (escort).
    #[serde(default)]
    pub poi: Option<PoiKind>,
    /// Days allowed, counting the day it is taken.
    pub days: u8,
    /// Pay in tallies, low.
    pub pay_lo: u16,
    /// Pay in tallies, high.
    pub pay_hi: u16,
    /// Standing gained on success.
    pub rep: i8,
    /// Standing lost on failure (a positive number).
    pub fail: u8,
}

impl ContractDef {
    /// The resource the contract wants, if any.
    #[must_use]
    pub fn resource(&self) -> Option<Resource> {
        self.good.as_deref().and_then(Resource::from_id)
    }
}

/// The contracts table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contracts {
    /// Every template.
    #[serde(rename = "contract", default)]
    pub entries: Vec<ContractDef>,
}

// ------------------------------------------------------------------ quests

/// One step of a chain (done when the clan holds the mark).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepDef {
    /// The mark that says it is done.
    pub mark: String,
    /// The journal line, once done or while it is the next thing to do.
    pub text: String,
}

/// One way a chain ends: the last mark is one of these, and the rest of the ends close.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndDef {
    /// The mark.
    pub mark: String,
    /// The journal line.
    pub text: String,
}

/// A hand-written chain.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestDef {
    /// Stable id.
    pub id: String,
    /// Whose story.
    pub faction: Faction,
    /// The heading in the log.
    pub title: String,
    /// What is heard before it starts.
    pub intro: String,
    /// The steps, in order.
    #[serde(rename = "step")]
    pub steps: Vec<StepDef>,
    /// The ways it can end (two or three).
    #[serde(rename = "end")]
    pub ends: Vec<EndDef>,
}

/// Where a clan stands in a chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// Not begun (its first mark is not held).
    Unheard,
    /// Under way: this many steps are done.
    Under(usize),
    /// Finished, by this way out (index into the ends).
    Ended(usize),
}

impl QuestDef {
    /// Where the clan stands, by its marks.
    #[must_use]
    pub fn progress(&self, has: impl Fn(&str) -> bool) -> Progress {
        if let Some(i) = self.ends.iter().position(|e| has(&e.mark)) {
            return Progress::Ended(i);
        }
        let done = self.steps.iter().take_while(|s| has(&s.mark)).count();
        if done == 0 {
            Progress::Unheard
        } else {
            Progress::Under(done)
        }
    }
}

/// The quests table.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Quests {
    /// Every chain.
    #[serde(rename = "quest", default)]
    pub entries: Vec<QuestDef>,
}

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 30 && id.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
}

fn text_ok(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && plain_text(text)
}

impl Table for Contracts {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        for (i, c) in self.entries.iter().enumerate() {
            let id = &c.id;
            if !id_ok(id) || self.entries[..i].iter().any(|x| x.id == *id) {
                p.push(format!("[contract] id `{id}` must be a-z and - and unique"));
            }
            if !text_ok(&c.title, 40) || !text_ok(&c.text, 220) {
                p.push(format!(
                    "[contract] `{id}`: title (40) and text (220) must be plain text"
                ));
            }
            if !(1..=4).contains(&c.tier) || !(1..=6).contains(&c.days) {
                p.push(format!("[contract] `{id}`: tier is 1 to 4 and days 1 to 6"));
            }
            if c.pay_lo == 0 || c.pay_lo > c.pay_hi || c.pay_hi > 400 {
                p.push(format!("[contract] `{id}`: pay is 1 to 400, low then high"));
            }
            if !(1..=12).contains(&c.rep) || !(1..=12).contains(&c.fail) {
                p.push(format!(
                    "[contract] `{id}`: rep is 1 to 12 and fail 1 to 12"
                ));
            }
            if c.qty == 0 || c.qty > 200 {
                p.push(format!("[contract] `{id}`: qty is 1 to 200"));
            }
            match c.kind {
                ContractKind::Deliver | ContractKind::Salvage => {
                    if c.resource().is_none() {
                        p.push(format!("[contract] `{id}`: a `good` that is a resource"));
                    }
                }
                ContractKind::Clear => {
                    if c.foe.is_none() {
                        p.push(format!("[contract] `{id}`: a clear names its `foe`"));
                    }
                }
                ContractKind::Escort => {
                    if c.poi.is_none() {
                        p.push(format!("[contract] `{id}`: an escort names its `poi`"));
                    }
                }
            }
        }
        if self.entries.is_empty() {
            return p;
        }
        for kind in [
            ContractKind::Deliver,
            ContractKind::Clear,
            ContractKind::Salvage,
            ContractKind::Escort,
        ] {
            if !self.entries.iter().any(|c| c.kind == kind) {
                p.push(format!("[contract] no `{}` contract", kind.word()));
            }
        }
        for f in Faction::ALL {
            for tier in 1..=3u8 {
                if !self
                    .entries
                    .iter()
                    .any(|c| c.faction == f && c.tier == tier)
                {
                    p.push(format!(
                        "[contract] {} has no contract at tier {tier}",
                        f.name()
                    ));
                }
            }
        }
        p
    }
}

impl Table for Quests {
    fn validate(&self) -> Vec<String> {
        let mut p = Vec::new();
        let mut marks: Vec<&str> = Vec::new();
        for (i, q) in self.entries.iter().enumerate() {
            let id = &q.id;
            if !id_ok(id) || self.entries[..i].iter().any(|x| x.id == *id) {
                p.push(format!("[quest] id `{id}` must be a-z and - and unique"));
            }
            if !text_ok(&q.title, 40) || !text_ok(&q.intro, 300) {
                p.push(format!(
                    "[quest] `{id}`: title (40) and intro (300) must be plain text"
                ));
            }
            if !(3..=7).contains(&q.steps.len()) {
                p.push(format!("[quest] `{id}`: three to seven steps"));
            }
            if !(2..=3).contains(&q.ends.len()) {
                p.push(format!("[quest] `{id}`: two or three ends"));
            }
            for (label, mark, text) in q
                .steps
                .iter()
                .map(|s| ("step", &s.mark, &s.text))
                .chain(q.ends.iter().map(|e| ("end", &e.mark, &e.text)))
            {
                if !crate::lore::mark_ok(mark) || !mark.starts_with("q-") {
                    p.push(format!(
                        "[quest] `{id}` {label}: mark `{mark}` must be a-z and - and start with `q-`"
                    ));
                }
                if marks.contains(&mark.as_str()) {
                    p.push(format!("[quest] mark `{mark}` is used twice"));
                }
                marks.push(mark);
                if !text_ok(text, 200) {
                    p.push(format!("[quest] `{id}` {label}: text (200) must be plain text"));
                }
            }
        }
        p
    }
}

// ------------------------------------------------------------------ the board

/// The contracts a clan is offered on a day: up to [`OFFERS`], drawn from the templates its
/// standing opens (a tier at or below its tier with the faction, and never an outcast's), the
/// same for everyone with the same standing on the same day of the same season.
#[must_use]
pub fn board<'a>(
    contracts: &'a [ContractDef],
    story: &Story,
    seed: u64,
    day: i64,
) -> Vec<&'a ContractDef> {
    let mut pool: Vec<&ContractDef> = contracts
        .iter()
        .filter(|c| {
            let t = story.tier(c.faction);
            t >= 1 && c.tier <= t
        })
        .collect();
    let mut rng = DoorRng::from_label(&format!("board-{seed}"), day);
    let mut out = Vec::new();
    while out.len() < OFFERS && !pool.is_empty() {
        let i = usize::try_from(rng.below(u32::try_from(pool.len()).unwrap_or(1))).unwrap_or(0);
        out.push(pool.remove(i));
    }
    out
}

/// Why a contract cannot be taken or done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContractError {
    /// One is already in hand.
    Busy,
    /// Not on today's board.
    NotOffered,
    /// There is nothing in hand.
    None,
    /// The stores fall short.
    Short(String),
    /// Not that kind.
    WrongKind,
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => f.write_str("You already hold a contract. Finish it or give it back."),
            Self::NotOffered => f.write_str("That contract is not on today's board."),
            Self::None => f.write_str("You hold no contract."),
            Self::Short(what) => write!(f, "The stores are short of {what}."),
            Self::WrongKind => f.write_str("That is not done by handing goods over."),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn tiers_run_from_outcast_to_honoured() {
        assert_eq!(tier_of(-50), 0);
        assert_eq!(tier_of(-21), 0);
        assert_eq!(tier_of(-20), 1);
        assert_eq!(tier_of(0), 1);
        assert_eq!(tier_of(14), 1);
        assert_eq!(tier_of(15), 2);
        assert_eq!(tier_of(40), 3);
        assert_eq!(tier_of(70), 4);
        assert_eq!(tier_of(100), 4);
        assert_eq!(TIERS.len(), 5);
    }

    #[test]
    fn standing_is_clamped_and_reports_tier_changes() {
        let mut s = Story::default();
        assert_eq!(s.add_rep(Faction::Reed, 20), (1, 2));
        assert_eq!(s.add_rep(Faction::Reed, 500), (2, 4));
        assert_eq!(s.rep[Faction::Reed.index()], REP_MAX);
        assert_eq!(s.add_rep(Faction::Rakers, -500), (1, 0));
        assert_eq!(s.rep[Faction::Rakers.index()], REP_MIN);
        assert_eq!(s.tier(Faction::Court), 1);
    }

    #[test]
    fn a_story_round_trips_with_its_outbox_and_contract() {
        let mut s = Story::default();
        s.rep = [-5, 33, 71];
        s.contract = Some(Active {
            id: "salt-for-the-court".into(),
            deadline: 20_000,
            progress: 2,
        });
        s.done = 4;
        s.heirloom = Heirloom::DeepLantern.code();
        s.queue(Out::Toll {
            owner: "Ash".into(),
            tallies: 3,
        });
        s.queue(Out::Lay { x: 3, y: 9, toll: 2 });
        s.queue(Out::Flag(Flag::Dredge));
        let mut w = Writer::new();
        s.write(&mut w);
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        assert_eq!(Story::read(&mut r), Some(s));
        assert!(r.done());
        assert_eq!(Story::read(&mut Reader::new(&bytes[..bytes.len() - 1])), None);
    }

    #[test]
    fn the_outbox_keeps_the_newest_orders() {
        let mut s = Story::default();
        for i in 0..20u16 {
            s.queue(Out::Toll {
                owner: "Ash".into(),
                tallies: i,
            });
        }
        assert_eq!(s.outbox.len(), OUTBOX_CAP);
        assert!(matches!(s.outbox.last(), Some(Out::Toll { tallies: 19, .. })));
    }

    #[test]
    fn heirlooms_and_flags_have_unique_codes() {
        let codes: Vec<u8> = Heirloom::ALL.iter().map(|h| h.code()).collect();
        assert!(codes.iter().all(|c| *c > 0));
        let mut sorted = codes.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len());
        let bits: u8 = Flag::ALL.iter().map(|f| f.bit()).fold(0, |a, b| a | b);
        assert_eq!(bits.count_ones() as usize, Flag::ALL.len());
        for f in Flag::ALL {
            assert_eq!(Flag::from_id(f.id()), Some(f));
        }
    }

    #[test]
    fn a_chain_reads_its_progress_from_marks() {
        let q = QuestDef {
            id: "x".into(),
            faction: Faction::Court,
            title: "t".into(),
            intro: "i".into(),
            steps: ["q-a", "q-b", "q-c"]
                .iter()
                .map(|m| StepDef {
                    mark: (*m).into(),
                    text: "s".into(),
                })
                .collect(),
            ends: ["q-end-1", "q-end-2"]
                .iter()
                .map(|m| EndDef {
                    mark: (*m).into(),
                    text: "e".into(),
                })
                .collect(),
        };
        let has = |held: &'static [&'static str]| move |m: &str| held.contains(&m);
        assert_eq!(q.progress(has(&[])), Progress::Unheard);
        assert_eq!(q.progress(has(&["q-a", "q-b"])), Progress::Under(2));
        assert_eq!(q.progress(has(&["q-b"])), Progress::Unheard);
        assert_eq!(
            q.progress(has(&["q-a", "q-b", "q-c", "q-end-2"])),
            Progress::Ended(1)
        );
    }
}
