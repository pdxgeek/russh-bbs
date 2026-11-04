//! The social layer (M4-S4): raids, pacts, the player market, letters, renown and the hall.
//!
//! **Why everything goes through the world.** A session can read only its own clan's save and
//! can change anything shared only by asking the board for a compare-and-swap edit of the world
//! (D98). So nothing here ever reaches into another player's save:
//!
//! * a clan *posts* what a raider could see (its home guard, its defences, a few figures of
//!   stock) in the world ([`Posted`]), and a raid is an *order* in the world that the nightly
//!   tick resolves against that post with the skirmish engine, while the defender is away;
//! * every other action (a market order, a letter, a pact) is a [`dispatch::Dispatch`]: a world
//!   edit that always leaves a [`Receipt`], with the goods or tallies it risked held in the
//!   clan's own save as a [`Pending`] until the receipt shows up (or, when no receipt ever
//!   does, until the clan takes them back: the edit was dropped);
//! * what the world owes a clan (parcels of goods and tallies, raid reports, letters, pact
//!   notices) is a [`Delivery`] with a global sequence number. The clan folds each one into its
//!   save once, in its next visit, and remembers the highest number it folded, so a dropped
//!   acknowledgement can never pay twice.
//!
//! Those three rules are why a dropped or retried edit can lose a little time but can never
//! duplicate or destroy goods. The one deliberate leak is a raid's loot: it comes out of the
//! *posted* stock, and the defender gives up what it still has of it, so a defender who spent
//! the goods after posting them is robbed of less than the raider took (bounded by the caps in
//! [`caps`]).

pub mod dispatch;
pub mod hall;
pub mod ledger;
pub mod letters;
pub mod market;
pub mod pact;
pub mod raid;
pub mod renown;

use crate::codec::{Reader, Writer};
use crate::types::{RESOURCES, Resource, Trade};

/// Every limit that keeps one player from griefing another or filling the world blob.
pub mod caps {
    /// Tide-turns a raid order costs.
    pub const RAID_TURNS: u8 = 4;
    /// Raids one clan may send in a day.
    pub const RAIDS_PER_DAY: u8 = 2;
    /// Raids one hold may be the target of in a day, from everyone.
    pub const HITS_PER_DAY: u8 = 2;
    /// Days after a raid before the same two clans may raid each other again.
    pub const PAIR_COOLDOWN_DAYS: i64 = 3;
    /// Days a raided hold is left alone.
    pub const SHIELD_DAYS: i64 = 2;
    /// Days a new clan cannot be raided, unless it raids first.
    pub const NEW_CLAN_DAYS: i64 = 7;
    /// Days unseen after which a raid on a clan takes half the loot.
    pub const DORMANT_HALF_DAYS: i64 = 4;
    /// Days unseen after which a clan cannot be raided at all.
    pub const DORMANT_SAFE_DAYS: i64 = 10;
    /// Percent of each posted good a won raid can take.
    pub const LOOT_PCT: u32 = 12;
    /// Percent of the posted tallies a won raid can take.
    pub const LOOT_TALLY_PCT: u32 = 8;
    /// The most a raid can take, in tallies' worth at base prices.
    pub const LOOT_MAX_VALUE: u32 = 90;
    /// Items one raider can carry off.
    pub const CARRY_PER_RAIDER: u32 = 6;
    /// A clan whose renown is less than a third of the raider's is beneath the raider.
    pub const BULLY_RATIO: u32 = 3;
    /// The most raiders in one order.
    pub const MAX_RAIDERS: usize = 4;
    /// The most defenders a hold posts.
    pub const MAX_DEFENDERS: usize = 5;
    /// Raid orders waiting in the world.
    pub const ORDERS_CAP: usize = 60;
    /// Raids remembered for cooldowns.
    pub const RAID_LOG: usize = 48;
    /// Letters one clan may send in a day.
    pub const LETTERS_PER_DAY: u8 = 5;
    /// Unread letters from one sender that may wait for one recipient.
    pub const UNREAD_PER_SENDER: usize = 2;
    /// Letters kept in a clan's inbox.
    pub const INBOX_CAP: usize = 10;
    /// Clans one clan can block.
    pub const BLOCK_CAP: usize = 8;
    /// Deliveries waiting for one clan.
    pub const MAIL_PER_CLAN: usize = 14;
    /// Deliveries waiting in the world.
    pub const MAIL_TOTAL: usize = 600;
    /// Letters waiting in the world.
    pub const LETTERS_TOTAL: usize = 220;
    /// Open affairs (dispatches without a receipt yet) a clan may have.
    pub const PENDING_CAP: usize = 8;
    /// Receipts waiting for one clan.
    pub const RECEIPTS_PER_CLAN: usize = 10;
    /// Pacts and the record of broken ones.
    pub const PACTS_CAP: usize = 150;
    /// Pacts waiting for an answer, per proposer.
    pub const OPEN_OFFERS: usize = 3;
    /// Market orders on each side of the book.
    pub const BOOK_CAP: usize = 100;
    /// Market orders one clan may have.
    pub const OWN_ORDERS: usize = 4;
    /// Days an order stands before it lapses and is returned.
    pub const ORDER_DAYS: i64 = 7;
    /// The most one order may ask for or offer.
    pub const ORDER_QTY: u32 = 200;
    /// Percent of an order's value kept as the listing fee.
    pub const LISTING_FEE_PCT: u32 = 2;
    /// Percent of a sale kept by the exchange.
    pub const LEVY_PCT: u32 = 3;
    /// Entries of the public deed feed.
    pub const FEED: usize = 30;
    /// Seasons of the hall's book.
    pub const BOOK_SEASONS: usize = 8;
    /// The world blob is refused growth past this many bytes (the framework's cap is 256 KiB).
    pub const WORLD_SOFT_CAP: usize = 200_000;
}

/// A clan's index in the world's registry. The registry only grows within a season.
pub type ClanIx = u8;

/// Things that are stored as bytes.
pub trait Wire: Sized {
    /// Append.
    fn write(&self, w: &mut Writer);
    /// Read back, or `None` for bad data.
    fn read(r: &mut Reader<'_>) -> Option<Self>;
}

pub(crate) fn write_resource(w: &mut Writer, r: Resource) {
    w.u8(u8::try_from(r.index()).unwrap_or(0));
}

pub(crate) fn read_resource(r: &mut Reader<'_>) -> Option<Resource> {
    Resource::ALL.get(usize::from(r.u8()?)).copied()
}

pub(crate) fn write_trade(w: &mut Writer, t: Trade) {
    w.u8(u8::try_from(t.index()).unwrap_or(0));
}

pub(crate) fn read_trade(r: &mut Reader<'_>) -> Option<Trade> {
    Trade::ALL.get(usize::from(r.u8()?)).copied()
}

pub(crate) fn write_list<T: Wire>(w: &mut Writer, items: &[T]) {
    w.count(items.len());
    for i in items.iter().take(usize::from(u8::MAX)) {
        i.write(w);
    }
}

pub(crate) fn read_list<T: Wire>(r: &mut Reader<'_>) -> Option<Vec<T>> {
    (0..r.u8()?).map(|_| T::read(r)).collect()
}

fn write_i16(w: &mut Writer, v: i16) {
    w.u16(u16::from_le_bytes(v.to_le_bytes()));
}

fn read_i16(r: &mut Reader<'_>) -> Option<i16> {
    Some(i16::from_le_bytes(r.u16()?.to_le_bytes()))
}

/// A home guard member as posted for raiders to see: the numbers the skirmish engine fights
/// with, computed by the owner when it posts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defender {
    /// The crew id in the owner's clan (so injuries land on the right person).
    pub id: u16,
    /// Their name.
    pub name: String,
    /// Their trade.
    pub trade: Trade,
    /// Hit points.
    pub hp: i16,
    /// Attack.
    pub atk: i16,
    /// Defence.
    pub def: i16,
    /// Speed.
    pub spd: i16,
    /// Mass.
    pub mass: u8,
    /// Row: 0 front, 2 back.
    pub row: u8,
    /// Fights well in water.
    pub aquatic: bool,
    /// Carries arms (reaches a row deeper).
    pub armed: bool,
}

impl Wire for Defender {
    fn write(&self, w: &mut Writer) {
        w.u16(self.id);
        w.str(&self.name);
        write_trade(w, self.trade);
        for v in [self.hp, self.atk, self.def, self.spd] {
            write_i16(w, v);
        }
        w.u8(self.mass);
        w.u8(self.row);
        w.bool(self.aquatic);
        w.bool(self.armed);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let id = r.u16()?;
        let name = r.str()?;
        let trade = read_trade(r)?;
        let (hp, atk, def, spd) = (read_i16(r)?, read_i16(r)?, read_i16(r)?, read_i16(r)?);
        let (mass, row, aquatic, armed) = (r.u8()?, r.u8()?, r.bool()?, r.bool()?);
        (row <= 2).then_some(Self {
            id,
            name,
            trade,
            hp,
            atk,
            def,
            spd,
            mass,
            row,
            aquatic,
            armed,
        })
    }
}

/// What a clan shows the marsh: its home guard, its defences and a few figures of stock. The
/// owner rewrites it when it changes. Nothing here is private to the clan; it is what a scout
/// would see from the causeway.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Posted {
    /// The last day the clan saved.
    pub seen: i64,
    /// The day the clan was founded.
    pub founded: i64,
    /// The home guard: fit crew who are in the hold (not out on a trip or a raid).
    pub guard: Vec<Defender>,
    /// Defence rating of the buildings.
    pub rating: u8,
    /// Extra rating while the sluice has flooded the approach.
    pub flood: u8,
    /// The level at which the sluice floods the approach, if set and built.
    pub sluice: Option<u8>,
    /// Goods in the stores (capped at 65535).
    pub stock: [u16; RESOURCES],
    /// Tallies in the strongbox.
    pub tallies: u16,
    /// Buildings finished.
    pub buildings: u8,
    /// Causeways laid.
    pub causeways: u8,
    /// Crew in the hold in total (the guard is the fit ones).
    pub crew: u8,
}

impl Wire for Posted {
    fn write(&self, w: &mut Writer) {
        w.i64(self.seen);
        w.i64(self.founded);
        write_list(w, &self.guard);
        w.u8(self.rating);
        w.u8(self.flood);
        w.u8(self.sluice.map_or(u8::MAX, |s| s.min(254)));
        for n in self.stock {
            w.u16(n);
        }
        w.u16(self.tallies);
        w.u8(self.buildings);
        w.u8(self.causeways);
        w.u8(self.crew);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let (seen, founded) = (r.i64()?, r.i64()?);
        let guard: Vec<Defender> = read_list(r)?;
        let (rating, flood) = (r.u8()?, r.u8()?);
        let sluice = match r.u8()? {
            u8::MAX => None,
            s => Some(s),
        };
        let mut stock = [0u16; RESOURCES];
        for n in &mut stock {
            *n = r.u16()?;
        }
        Some(Self {
            seen,
            founded,
            guard,
            rating,
            flood,
            sluice,
            stock,
            tallies: r.u16()?,
            buildings: r.u8()?,
            causeways: r.u8()?,
            crew: r.u8()?,
        })
    }
}

/// The world's own counters about a clan, changed only by world edits and the tick (never by
/// the clan's save), so several sessions and the tick agree on them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Record {
    /// Standing with the Tidecourt, -100 to 100: moved by pacts kept and broken.
    pub rep: i8,
    /// Pacts this clan broke this season.
    pub betrayals: u8,
    /// Raids on this hold are refused until this day.
    pub shield_until: i64,
    /// The day of the clan's raid count.
    pub raid_day: i64,
    /// Raids the clan sent on `raid_day`.
    pub raid_n: u8,
    /// Whether the clan has raided anyone (it forfeits new-clan protection).
    pub raided_first: bool,
    /// The day of the clan's letter count.
    pub letters_day: i64,
    /// Letters the clan sent on `letters_day`.
    pub letters_n: u8,
    /// No new pacts may be offered until this day.
    pub pact_ban_until: i64,
    /// Clans whose letters this clan does not receive.
    pub blocked: Vec<ClanIx>,
    /// Raids won as the raider.
    pub raids_won: u16,
    /// Raids lost as the raider.
    pub raids_lost: u16,
    /// Raids beaten off as the defender.
    pub repelled: u16,
    /// Raids suffered and lost as the defender.
    pub raided: u16,
    /// Tallies' worth taken in raids.
    pub haul: u32,
    /// Tallies traded on the exchange.
    pub traded: u32,
}

impl Wire for Record {
    fn write(&self, w: &mut Writer) {
        w.u8(u8::from_le_bytes(self.rep.to_le_bytes()));
        w.u8(self.betrayals);
        w.i64(self.shield_until);
        w.i64(self.raid_day);
        w.u8(self.raid_n);
        w.bool(self.raided_first);
        w.i64(self.letters_day);
        w.u8(self.letters_n);
        w.i64(self.pact_ban_until);
        w.count(self.blocked.len());
        for b in &self.blocked {
            w.u8(*b);
        }
        for n in [self.raids_won, self.raids_lost, self.repelled, self.raided] {
            w.u16(n);
        }
        w.u32(self.haul);
        w.u32(self.traded);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let rep = i8::from_le_bytes([r.u8()?]);
        let betrayals = r.u8()?;
        let (shield_until, raid_day) = (r.i64()?, r.i64()?);
        let (raid_n, raided_first) = (r.u8()?, r.bool()?);
        let (letters_day, letters_n) = (r.i64()?, r.u8()?);
        let pact_ban_until = r.i64()?;
        let blocked = (0..r.u8()?).map(|_| r.u8()).collect::<Option<Vec<_>>>()?;
        Some(Self {
            rep,
            betrayals,
            shield_until,
            raid_day,
            raid_n,
            raided_first,
            letters_day,
            letters_n,
            pact_ban_until,
            blocked,
            raids_won: r.u16()?,
            raids_lost: r.u16()?,
            repelled: r.u16()?,
            raided: r.u16()?,
            haul: r.u32()?,
            traded: r.u32()?,
        })
    }
}

/// Something the world owes a clan, folded once into its save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivery {
    /// Global, increasing: the clan remembers the highest it folded.
    pub seq: u32,
    /// The recipient.
    pub to: ClanIx,
    /// What it is.
    pub body: Body,
}

/// What a delivery carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// Goods and tallies from the market.
    Parcel(market::Parcel),
    /// The result of a raid, for either side.
    Raid(raid::Report),
    /// A letter from another clan.
    Letter(letters::LetterBody),
    /// A pact changed.
    Pact(pact::Notice),
}

impl Wire for Delivery {
    fn write(&self, w: &mut Writer) {
        w.u32(self.seq);
        w.u8(self.to);
        match &self.body {
            Body::Parcel(p) => {
                w.u8(0);
                p.write(w);
            }
            Body::Raid(p) => {
                w.u8(1);
                p.write(w);
            }
            Body::Letter(p) => {
                w.u8(2);
                p.write(w);
            }
            Body::Pact(p) => {
                w.u8(3);
                p.write(w);
            }
        }
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let (seq, to) = (r.u32()?, r.u8()?);
        let body = match r.u8()? {
            0 => Body::Parcel(market::Parcel::read(r)?),
            1 => Body::Raid(raid::Report::read(r)?),
            2 => Body::Letter(letters::LetterBody::read(r)?),
            3 => Body::Pact(pact::Notice::read(r)?),
            _ => return None,
        };
        Some(Self { seq, to, body })
    }
}

/// Why the world refused something. Each has a plain sentence (the player's own words are never
/// echoed in it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The clan is not in the registry.
    Unknown,
    /// Not allowed against a pact partner.
    Pact,
    /// Too many raids sent today.
    RaidsToday,
    /// That hold has been hit enough today.
    HitsToday,
    /// These two raided each other too recently.
    PairCooldown,
    /// That hold was raided recently and is left alone.
    Shielded,
    /// That clan is new and protected.
    NewClan,
    /// That clan has not been seen for too long.
    Dormant,
    /// The target is too small to be worth a raid.
    Beneath,
    /// The order book or a table is full.
    Full,
    /// Too many open affairs.
    TooMany,
    /// The raiding party is not allowed (empty, too big).
    BadParty,
    /// The window is not open to this party.
    Window,
    /// A bad quantity or price.
    BadOrder,
    /// Too many letters today.
    LettersToday,
    /// The recipient already holds enough of this sender's letters.
    Unread,
    /// The terms are closed to this clan.
    Terms,
    /// A pact (or a broken one) already stands between the two.
    HasPact,
    /// No such order or pact.
    Missing,
    /// Not yours to change.
    NotYours,
    /// The recipient has too much waiting.
    MailFull,
    /// The sender is in the Tidecourt's bad books.
    Outlaw,
}

impl Refusal {
    /// A plain sentence for the player.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Unknown => "That clan is not on the Tidecourt's rolls.",
            Self::Pact => "A pact stands between you.",
            Self::RaidsToday => "You have sent all the raids you may today.",
            Self::HitsToday => "That hold has been struck enough today.",
            Self::PairCooldown => "You and that clan have raided each other too recently.",
            Self::Shielded => "That hold was raided lately; the Tidecourt leaves it be.",
            Self::NewClan => "That clan is too new to be raided.",
            Self::Dormant => "Nobody has seen that clan on the marsh for days.",
            Self::Beneath => "That clan is too small to be worth a raid.",
            Self::Full => "The Tidecourt's books are full. Try again tomorrow.",
            Self::TooMany => "You have too many affairs open. Wait for answers.",
            Self::BadParty => "The raiding party is not allowed.",
            Self::Window => "That tide is not open to your party.",
            Self::BadOrder => "That order is not allowed.",
            Self::LettersToday => "You have sent all the letters you may today.",
            Self::Unread => "That clan already has letters from you it has not read.",
            Self::Terms => "Those terms are closed to you.",
            Self::HasPact => "A pact, or the memory of one, already stands between you.",
            Self::Missing => "There is no such order or pact.",
            Self::NotYours => "That is not yours to change.",
            Self::MailFull => "That clan's post is full.",
            Self::Outlaw => "The Tidecourt will not carry your word.",
        }
    }

    pub(crate) const fn code(self) -> u8 {
        self as u8
    }

    pub(crate) const fn from_code(c: u8) -> Option<Self> {
        Some(match c {
            0 => Self::Unknown,
            1 => Self::Pact,
            2 => Self::RaidsToday,
            3 => Self::HitsToday,
            4 => Self::PairCooldown,
            5 => Self::Shielded,
            6 => Self::NewClan,
            7 => Self::Dormant,
            8 => Self::Beneath,
            9 => Self::Full,
            10 => Self::TooMany,
            11 => Self::BadParty,
            12 => Self::Window,
            13 => Self::BadOrder,
            14 => Self::LettersToday,
            15 => Self::Unread,
            16 => Self::Terms,
            17 => Self::HasPact,
            18 => Self::Missing,
            19 => Self::NotYours,
            20 => Self::MailFull,
            21 => Self::Outlaw,
            _ => return None,
        })
    }
}

/// The world's answer to one dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Receipt {
    /// The clan.
    pub clan: ClanIx,
    /// The clan's dispatch number.
    pub id: u16,
    /// `None`: done. Otherwise why not.
    pub refused: Option<Refusal>,
}

impl Wire for Receipt {
    fn write(&self, w: &mut Writer) {
        w.u8(self.clan);
        w.u16(self.id);
        w.u8(self.refused.map_or(u8::MAX, Refusal::code));
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let (clan, id) = (r.u8()?, r.u16()?);
        let refused = match r.u8()? {
            u8::MAX => None,
            c => Some(Refusal::from_code(c)?),
        };
        Some(Self { clan, id, refused })
    }
}

/// A public deed, for the feed and the hall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deed {
    /// The day.
    pub day: i64,
    /// What happened.
    pub kind: DeedKind,
    /// The doer.
    pub a: ClanIx,
    /// The other party.
    pub b: ClanIx,
}

/// The kinds of public deed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeedKind {
    /// `a` raided `b` and won.
    RaidWon,
    /// `a` raided `b` and was beaten off.
    RaidLost,
    /// `a` and `b` signed a pact.
    PactSigned,
    /// `a` broke the pact with `b`.
    PactBroken,
}

impl DeedKind {
    const fn code(self) -> u8 {
        self as u8
    }

    const fn from_code(c: u8) -> Option<Self> {
        Some(match c {
            0 => Self::RaidWon,
            1 => Self::RaidLost,
            2 => Self::PactSigned,
            3 => Self::PactBroken,
            _ => return None,
        })
    }
}

impl Wire for Deed {
    fn write(&self, w: &mut Writer) {
        w.i64(self.day);
        w.u8(self.kind.code());
        w.u8(self.a);
        w.u8(self.b);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            day: r.i64()?,
            kind: DeedKind::from_code(r.u8()?)?,
            a: r.u8()?,
            b: r.u8()?,
        })
    }
}

/// The world's social state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Social {
    /// The next delivery number.
    pub next_seq: u32,
    /// The next id for raid orders, pacts and market orders.
    pub next_id: u32,
    /// Raid orders waiting for the tick.
    pub orders: Vec<raid::Order>,
    /// Recent raids, for cooldowns: `(from, to, day)`.
    pub raid_log: Vec<raid::LogEntry>,
    /// Pacts, and the memory of broken ones.
    pub pacts: Vec<pact::Pact>,
    /// The player market.
    pub exchange: market::Exchange,
    /// What the world owes clans.
    pub mail: Vec<Delivery>,
    /// Answers to dispatches, waiting to be seen.
    pub receipts: Vec<Receipt>,
    /// The public feed of deeds.
    pub feed: Vec<Deed>,
    /// Past seasons.
    pub book: Vec<hall::SeasonRecord>,
}

impl Social {
    /// The next delivery number.
    pub fn seq(&mut self) -> u32 {
        self.next_seq = self.next_seq.saturating_add(1);
        self.next_seq
    }

    /// The next id.
    pub fn id(&mut self) -> u32 {
        self.next_id = self.next_id.saturating_add(1);
        self.next_id
    }

    /// Deliveries waiting for `clan`.
    #[must_use]
    pub fn mail_for(&self, clan: ClanIx) -> usize {
        self.mail.iter().filter(|d| d.to == clan).count()
    }

    /// Whether `clan` can take one more delivery.
    #[must_use]
    pub fn has_room(&self, clan: ClanIx) -> bool {
        self.mail_for(clan) < caps::MAIL_PER_CLAN && self.mail.len() < caps::MAIL_TOTAL
    }

    /// Leave a delivery for `to`. Callers check [`Social::has_room`] first where the sender
    /// can be refused; the tick's reports go in regardless (a raid's two reports are bounded
    /// by the orders cap).
    pub fn deliver(&mut self, to: ClanIx, body: Body) {
        let seq = self.seq();
        self.mail.push(Delivery { seq, to, body });
    }

    /// Note a public deed.
    pub fn note(&mut self, day: i64, kind: DeedKind, a: ClanIx, b: ClanIx) {
        self.feed.push(Deed { day, kind, a, b });
        let extra = self.feed.len().saturating_sub(caps::FEED);
        self.feed.drain(..extra);
    }

    /// Write.
    pub fn write(&self, w: &mut Writer) {
        w.u32(self.next_seq);
        w.u32(self.next_id);
        write_list(w, &self.orders);
        write_list(w, &self.raid_log);
        write_list(w, &self.pacts);
        self.exchange.write(w);
        w.u16(u16::try_from(self.mail.len()).unwrap_or(u16::MAX));
        for d in self.mail.iter().take(usize::from(u16::MAX)) {
            d.write(w);
        }
        write_list(w, &self.receipts);
        write_list(w, &self.feed);
        write_list(w, &self.book);
    }

    /// Read.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        let (next_seq, next_id) = (r.u32()?, r.u32()?);
        let orders = read_list(r)?;
        let raid_log = read_list(r)?;
        let pacts = read_list(r)?;
        let exchange = market::Exchange::read(r)?;
        let mail = (0..r.u16()?)
            .map(|_| Delivery::read(r))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            next_seq,
            next_id,
            orders,
            raid_log,
            pacts,
            exchange,
            mail,
            receipts: read_list(r)?,
            feed: read_list(r)?,
            book: read_list(r)?,
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn a_posted_defence_and_a_record_round_trip() {
        let posted = Posted {
            seen: 20_001,
            founded: 19_990,
            guard: vec![Defender {
                id: 3,
                name: "Ysolt".into(),
                trade: Trade::Fighter,
                hp: 22,
                atk: 5,
                def: 2,
                spd: 4,
                mass: 6,
                row: 0,
                aquatic: true,
                armed: false,
            }],
            rating: 7,
            flood: 8,
            sluice: Some(60),
            stock: [3; RESOURCES],
            tallies: 99,
            buildings: 6,
            causeways: 1,
            crew: 5,
        };
        let mut w = Writer::new();
        posted.write(&mut w);
        let bytes = w.finish();
        assert_eq!(Posted::read(&mut Reader::new(&bytes)), Some(posted));
        let record = Record {
            rep: -25,
            betrayals: 1,
            blocked: vec![4, 9],
            haul: 70,
            ..Record::default()
        };
        let mut w = Writer::new();
        record.write(&mut w);
        let bytes = w.finish();
        assert_eq!(Record::read(&mut Reader::new(&bytes)), Some(record));
    }

    #[test]
    fn every_refusal_has_a_sentence_and_a_code_that_round_trips() {
        for code in 0..=21u8 {
            let r = Refusal::from_code(code).unwrap();
            assert_eq!(r.code(), code);
            assert!(r.text().ends_with('.') && r.text().len() > 12);
        }
        assert_eq!(Refusal::from_code(22), None);
    }

    #[test]
    fn deliveries_are_numbered_in_order_and_the_feed_is_capped() {
        let mut s = Social::default();
        s.deliver(
            2,
            Body::Parcel(market::Parcel {
                goods: None,
                tallies: 5,
                memo: market::Memo::Expired,
                about: None,
            }),
        );
        s.deliver(
            2,
            Body::Parcel(market::Parcel {
                goods: None,
                tallies: 6,
                memo: market::Memo::Expired,
                about: None,
            }),
        );
        assert_eq!(s.mail.iter().map(|d| d.seq).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(s.mail_for(2), 2);
        for day in 0..100 {
            s.note(day, DeedKind::RaidWon, 0, 1);
        }
        assert_eq!(s.feed.len(), caps::FEED);
        assert_eq!(s.feed.last().map(|d| d.day), Some(99));
    }
}
