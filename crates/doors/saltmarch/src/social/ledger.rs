//! The clan's side of the social layer: what it has sent and not yet seen answered
//! ([`Pending`]), what it has read ([`Letter`]), and [`fold`], which turns what the world owes
//! it into changes to its save, once.
//!
//! The rules that make a dropped or repeated edit harmless:
//!
//! * Anything the clan sends the world that risks goods or tallies is a [`Pending`] in the
//!   save *before* the edit runs. The world always answers with a [`Receipt`]. On the next
//!   visit a pending with a receipt is settled (the world holds the escrow, or refused and the
//!   clan takes its own goods back); a pending **without** a receipt means the edit never ran,
//!   and the clan takes its goods back. Either way they exist once.
//! * Everything the world owes the clan is a numbered delivery. The clan remembers the highest
//!   number it folded and folds a number only once, so a lost acknowledgement cannot pay twice.

use russh_bbs_door::DoorRng;

use super::raid::{Fate, Outcome, Report, Role};
use super::{Body, Defender, Delivery, Posted, Receipt, Refusal, Wire, caps};
use super::{market, pact};
use crate::clan::{Clan, HallEntry};
use crate::codec::{Reader, Writer};
use crate::combat::Unit;
use crate::content::{Catalog, Cause, EpitaphCause};
use crate::engine::injure;
use crate::event::Event;
use crate::types::{RESOURCES, Resource};

/// What a pending dispatch is, for the line that says how it came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A standing ask on the exchange.
    Ask,
    /// A standing bid on the exchange.
    Bid,
    /// A cancelled order.
    Cancel,
    /// A letter.
    Letter,
    /// A pact offer.
    Offer,
    /// An accepted pact.
    Accept,
    /// A declined pact.
    Decline,
    /// A broken pact.
    Break,
    /// A raid.
    Raid,
    /// A blocked sender.
    Block,
}

impl Kind {
    const fn code(self) -> u8 {
        self as u8
    }

    const fn from_code(c: u8) -> Option<Self> {
        Some(match c {
            0 => Self::Ask,
            1 => Self::Bid,
            2 => Self::Cancel,
            3 => Self::Letter,
            4 => Self::Offer,
            5 => Self::Accept,
            6 => Self::Decline,
            7 => Self::Break,
            8 => Self::Raid,
            9 => Self::Block,
            _ => return None,
        })
    }

    /// A word for the report.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Bid => "bid",
            Self::Cancel => "cancellation",
            Self::Letter => "letter",
            Self::Offer => "pact offer",
            Self::Accept => "pact",
            Self::Decline => "answer",
            Self::Break => "break",
            Self::Raid => "raid",
            Self::Block => "block",
        }
    }
}

/// A dispatch the clan sent whose receipt it has not yet seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The clan's dispatch number.
    pub id: u16,
    /// The day it was sent.
    pub day: i64,
    /// What it is.
    pub kind: Kind,
    /// A short label for the report ("10 salt at 5", the other clan's name).
    pub about: String,
    /// Goods the clan put at risk.
    pub goods: Option<(Resource, u32)>,
    /// Tallies the clan put at risk.
    pub tallies: u32,
    /// Tide-turns it cost (given back, the same day, if it never ran).
    pub turns: u8,
    /// Crew sent with it (a raid's party).
    pub crew: Vec<u16>,
}

impl Wire for Pending {
    fn write(&self, w: &mut Writer) {
        w.u16(self.id);
        w.i64(self.day);
        w.u8(self.kind.code());
        w.str(&self.about);
        match self.goods {
            Some((r, n)) => {
                w.bool(true);
                super::write_resource(w, r);
                w.u32(n);
            }
            None => w.bool(false),
        }
        w.u32(self.tallies);
        w.u8(self.turns);
        w.count(self.crew.len());
        for c in &self.crew {
            w.u16(*c);
        }
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let (id, day) = (r.u16()?, r.i64()?);
        let kind = Kind::from_code(r.u8()?)?;
        let about = r.str()?;
        let goods = if r.bool()? {
            Some((super::read_resource(r)?, r.u32()?))
        } else {
            None
        };
        let (tallies, turns) = (r.u32()?, r.u8()?);
        let crew = (0..r.u8()?).map(|_| r.u16()).collect::<Option<Vec<_>>>()?;
        Some(Self {
            id,
            day,
            kind,
            about,
            goods,
            tallies,
            turns,
            crew,
        })
    }
}

/// A letter in the clan's inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Letter {
    /// The delivery number it came with (or 0 for one the clan wrote itself).
    pub seq: u32,
    /// The sender.
    pub from: String,
    /// The template id.
    pub template: String,
    /// The sender's own line.
    pub line: Option<String>,
    /// The day.
    pub day: i64,
    /// A pact offer it carries: `(pact id, terms)`.
    pub offer: Option<(u32, u8)>,
    /// Whether the clan has read it.
    pub read: bool,
}

impl Wire for Letter {
    fn write(&self, w: &mut Writer) {
        w.u32(self.seq);
        w.str(&self.from);
        w.str(&self.template);
        match &self.line {
            Some(l) => {
                w.bool(true);
                w.str(l);
            }
            None => w.bool(false),
        }
        w.i64(self.day);
        match self.offer {
            Some((id, terms)) => {
                w.bool(true);
                w.u32(id);
                w.u8(terms);
            }
            None => w.bool(false),
        }
        w.bool(self.read);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            seq: r.u32()?,
            from: r.str()?,
            template: r.str()?,
            line: if r.bool()? { Some(r.str()?) } else { None },
            day: r.i64()?,
            offer: if r.bool()? {
                Some((r.u32()?, r.u8()?))
            } else {
                None
            },
            read: r.bool()?,
        })
    }
}

/// Everything social that a clan keeps in its own save.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClanSocial {
    /// The last dispatch number used.
    pub seq: u16,
    /// Dispatches waiting for their receipt.
    pub pending: Vec<Pending>,
    /// The highest delivery number folded.
    pub credited_to: u32,
    /// Crew away on a raid (until its report is folded).
    pub away: Vec<u16>,
    /// Letters.
    pub inbox: Vec<Letter>,
    /// Raids sent.
    pub raids_sent: u16,
    /// Raids won as the raider.
    pub raids_won: u16,
    /// Raids lost as the raider.
    pub raids_lost: u16,
    /// Raids suffered and lost as the defender.
    pub raided: u16,
    /// Raids beaten off as the defender.
    pub repelled: u16,
    /// Tallies' worth taken in raids.
    pub plundered: u32,
    /// Tallies' worth lost to raids.
    pub robbed: u32,
    /// Clans that raided or betrayed this one, remembered (names).
    pub grudges: Vec<String>,
}

impl ClanSocial {
    /// Write.
    pub fn write(&self, w: &mut Writer) {
        w.u16(self.seq);
        super::write_list(w, &self.pending);
        w.u32(self.credited_to);
        w.count(self.away.len());
        for c in &self.away {
            w.u16(*c);
        }
        super::write_list(w, &self.inbox);
        for n in [
            self.raids_sent,
            self.raids_won,
            self.raids_lost,
            self.raided,
            self.repelled,
        ] {
            w.u16(n);
        }
        w.u32(self.plundered);
        w.u32(self.robbed);
        w.count(self.grudges.len());
        for g in &self.grudges {
            w.str(g);
        }
    }

    /// Read.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        let seq = r.u16()?;
        let pending = super::read_list(r)?;
        let credited_to = r.u32()?;
        let away = (0..r.u8()?).map(|_| r.u16()).collect::<Option<Vec<_>>>()?;
        let inbox = super::read_list(r)?;
        let counts = [r.u16()?, r.u16()?, r.u16()?, r.u16()?, r.u16()?];
        let (plundered, robbed) = (r.u32()?, r.u32()?);
        let grudges = (0..r.u8()?).map(|_| r.str()).collect::<Option<Vec<_>>>()?;
        Some(Self {
            seq,
            pending,
            credited_to,
            away,
            inbox,
            raids_sent: counts[0],
            raids_won: counts[1],
            raids_lost: counts[2],
            raided: counts[3],
            repelled: counts[4],
            plundered,
            robbed,
            grudges,
        })
    }

    /// Whether `crew` is away on a raid.
    #[must_use]
    pub fn is_away(&self, crew: u16) -> bool {
        self.away.contains(&crew)
    }

    /// Whether the clan can take on another open affair.
    #[must_use]
    pub fn has_room(&self) -> bool {
        self.pending.len() < caps::PENDING_CAP
    }

    /// Open a dispatch: the next number, and the pending that holds what is at risk.
    pub fn begin(&mut self, mut p: Pending) -> u16 {
        self.seq = self.seq.wrapping_add(1).max(1);
        p.id = self.seq;
        let id = p.id;
        self.pending.push(p);
        id
    }

    /// Unread letters.
    #[must_use]
    pub fn unread(&self) -> usize {
        self.inbox.iter().filter(|l| !l.read).count()
    }

    /// Remember a grudge against a clan.
    pub fn remember(&mut self, name: &str) {
        if !self.grudges.iter().any(|g| g == name) {
            self.grudges.push(name.to_owned());
            if self.grudges.len() > 8 {
                self.grudges.remove(0);
            }
        }
    }

    /// Put a letter in the inbox. The oldest read letter makes room; if all are unread the
    /// oldest goes.
    pub fn receive(&mut self, letter: Letter) {
        if letter.seq != 0 && self.inbox.iter().any(|l| l.seq == letter.seq) {
            return;
        }
        self.inbox.push(letter);
        while self.inbox.len() > caps::INBOX_CAP {
            let at = self.inbox.iter().position(|l| l.read).unwrap_or(0);
            self.inbox.remove(at);
        }
    }
}

/// What the world holds for one clan, copied out of the snapshot the session opened with.
#[derive(Debug, Clone, Default)]
pub struct Mail {
    /// Deliveries for the clan, oldest first.
    pub deliveries: Vec<Delivery>,
    /// Receipts for the clan.
    pub receipts: Vec<Receipt>,
}

impl Mail {
    /// Whether there is nothing to fold.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.deliveries.is_empty() && self.receipts.is_empty()
    }
}

/// What to tell the world once the fold is saved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Acks {
    /// Remove this clan's deliveries up to and including this number.
    pub upto: u32,
    /// Remove this clan's receipts with these dispatch numbers.
    pub receipts: Vec<u16>,
}

impl Acks {
    /// Whether there is nothing to acknowledge.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.upto == 0 && self.receipts.is_empty()
    }
}

fn take_back(clan: &mut Clan, p: &Pending, today: i64) {
    clan.tallies = clan.tallies.saturating_add(p.tallies);
    if let Some((r, n)) = p.goods {
        clan.stock.add(r, n);
    }
    if p.day == today {
        clan.turns_used = clan.turns_used.saturating_sub(p.turns);
    }
    clan.social.away.retain(|c| !p.crew.contains(c));
}

/// Fold what the world holds for the clan into its save. Returns the events (for the morning
/// report) and what to acknowledge.
#[allow(clippy::too_many_lines)]
pub fn fold(
    cat: &Catalog,
    clan: &mut Clan,
    mail: &Mail,
    today: i64,
    events: &mut Vec<Event>,
) -> Acks {
    let mut acks = Acks::default();
    // 1. Receipts settle pendings; a pending with none means the edit never ran.
    let pending = std::mem::take(&mut clan.social.pending);
    for p in pending {
        match mail.receipts.iter().find(|r| r.id == p.id) {
            Some(r) => {
                acks.receipts.push(p.id);
                if let Some(why) = r.refused {
                    take_back(clan, &p, today);
                    events.push(Event::Refused {
                        what: format!("{} ({})", p.kind.name(), p.about),
                        why,
                    });
                }
            }
            None => {
                take_back(clan, &p, today);
                events.push(Event::Unsent {
                    what: format!("{} ({})", p.kind.name(), p.about),
                });
            }
        }
    }
    // Receipts for dispatches the clan no longer has pending are stale: just clear them.
    for r in &mail.receipts {
        if !acks.receipts.contains(&r.id) {
            acks.receipts.push(r.id);
        }
    }
    // 2. Deliveries, once each, in order.
    let mut top = clan.social.credited_to;
    for d in &mail.deliveries {
        if d.seq <= clan.social.credited_to {
            continue;
        }
        top = top.max(d.seq);
        let mut rng = DoorRng::from_label(
            &format!("saltmarch-fold-{}-{}", clan.name, clan.seed),
            i64::from(d.seq),
        );
        match &d.body {
            Body::Parcel(p) => parcel(clan, p, events),
            Body::Raid(r) => raid(cat, clan, r, d.seq, &mut rng, events),
            Body::Letter(l) => {
                events.push(Event::LetterArrived {
                    from: l.from.clone(),
                });
                clan.social.receive(Letter {
                    seq: d.seq,
                    from: l.from.clone(),
                    template: l.template.clone(),
                    line: l.line.clone(),
                    day: l.day,
                    offer: None,
                    read: false,
                });
            }
            Body::Pact(n) => notice(clan, n, d.seq, today, events),
        }
    }
    if top > clan.social.credited_to {
        clan.social.credited_to = top;
        acks.upto = top;
    } else if !mail.deliveries.is_empty() {
        // Everything was folded before; the acknowledgement was what got lost.
        acks.upto = mail.deliveries.iter().map(|d| d.seq).max().unwrap_or(0);
    }
    acks
}

fn parcel(clan: &mut Clan, p: &market::Parcel, events: &mut Vec<Event>) {
    clan.tallies = clan.tallies.saturating_add(p.tallies);
    if let Some((r, n)) = p.goods {
        clan.stock.add(r, n);
    }
    if p.memo == market::Memo::Sold {
        clan.stats.earned = clan.stats.earned.saturating_add(p.tallies);
    }
    events.push(Event::Parcel {
        memo: p.memo,
        about: p.about,
        goods: p.goods,
        tallies: p.tallies,
    });
}

fn notice(clan: &mut Clan, n: &pact::Notice, seq: u32, today: i64, events: &mut Vec<Event>) {
    match n.kind {
        pact::NoticeKind::Offered => {
            events.push(Event::PactOffered {
                from: n.with.clone(),
                terms: n.terms,
            });
            let template = if n.template.is_empty() {
                "offer-truce".to_owned()
            } else {
                n.template.clone()
            };
            clan.social.receive(Letter {
                seq,
                from: n.with.clone(),
                template,
                line: n.line.clone(),
                day: today,
                offer: Some((n.pact, n.terms)),
                read: false,
            });
        }
        pact::NoticeKind::Accepted => events.push(Event::PactSigned {
            with: n.with.clone(),
            terms: n.terms,
        }),
        pact::NoticeKind::Declined => events.push(Event::PactDeclined {
            with: n.with.clone(),
        }),
        pact::NoticeKind::Broken => {
            clan.social.remember(&n.with);
            clan.morale = clan.morale.saturating_sub(4);
            events.push(Event::PactBroken { by: n.with.clone() });
        }
    }
}

fn goods_value(loot: &[(Resource, u32)], tallies: u32) -> u32 {
    loot.iter()
        .map(|(r, n)| n * u32::from(r.base_price()))
        .sum::<u32>()
        + tallies
}

fn raid(
    cat: &Catalog,
    clan: &mut Clan,
    r: &Report,
    seq: u32,
    rng: &mut DoorRng,
    events: &mut Vec<Event>,
) {
    let value = goods_value(&r.loot, r.tallies);
    match r.role {
        Role::Raider => {
            let ids: Vec<u16> = r.fates.iter().map(|f| f.0).collect();
            clan.social.away.retain(|c| !ids.contains(c));
            match r.outcome {
                Outcome::Recalled => events.push(Event::RaidRecalled {
                    against: r.defender.clone(),
                }),
                Outcome::Won => {
                    for (res, n) in &r.loot {
                        clan.stock.add(*res, *n);
                    }
                    clan.tallies = clan.tallies.saturating_add(r.tallies);
                    clan.social.raids_won = clan.social.raids_won.saturating_add(1);
                    clan.social.plundered = clan.social.plundered.saturating_add(value);
                    clan.morale = clan.morale.saturating_add(3).min(100);
                    events.push(Event::RaidWon {
                        against: r.defender.clone(),
                        loot: r.loot.clone(),
                        tallies: r.tallies,
                    });
                }
                Outcome::Repelled => {
                    clan.social.raids_lost = clan.social.raids_lost.saturating_add(1);
                    clan.morale = clan.morale.saturating_sub(3);
                    events.push(Event::RaidFailed {
                        against: r.defender.clone(),
                    });
                }
            }
            for &(id, fate) in &r.fates {
                let Some(name) = clan.crew_by(id).map(|c| c.name.clone()) else {
                    continue;
                };
                match fate {
                    Fate::Unhurt => {}
                    Fate::Hurt => {
                        injure(cat, clan, id, Cause::Fight, rng, events);
                    }
                    Fate::Dead => {
                        clan.remove_crew(cat, id, Some(EpitaphCause::Lost), rng, events);
                        events.push(Event::Died { name });
                    }
                }
            }
            if r.outcome != Outcome::Recalled {
                push_hall(
                    clan,
                    format!(
                        "{} {}.",
                        if r.outcome == Outcome::Won {
                            "Raided"
                        } else {
                            "Beaten off at the gate of"
                        },
                        r.defender
                    ),
                );
            }
        }
        Role::Defender => {
            let mut lost = 0u32;
            for (res, n) in &r.loot {
                let took = clan.stock.take_up_to(*res, *n);
                lost += took * u32::from(res.base_price());
            }
            let took = r.tallies.min(clan.tallies);
            clan.tallies -= took;
            lost += took;
            clan.social.remember(&r.raider);
            if r.outcome == Outcome::Won {
                clan.social.raided = clan.social.raided.saturating_add(1);
                clan.social.robbed = clan.social.robbed.saturating_add(lost);
                clan.morale = clan.morale.saturating_sub(5);
                events.push(Event::RaidSuffered {
                    by: r.raider.clone(),
                    loot: r.loot.clone(),
                    tallies: r.tallies,
                });
                push_hall(clan, format!("Raided by {}; the gate fell.", r.raider));
            } else {
                clan.social.repelled = clan.social.repelled.saturating_add(1);
                clan.morale = clan.morale.saturating_add(3).min(100);
                events.push(Event::RaidRepelled {
                    by: r.raider.clone(),
                });
                push_hall(clan, format!("Beat {} off the gate.", r.raider));
            }
            for &id in &r.hurt {
                if clan.crew_by(id).is_some() {
                    injure(cat, clan, id, Cause::Fight, rng, events);
                }
            }
            if !r.taunt.is_empty() {
                clan.social.receive(Letter {
                    seq,
                    from: r.raider.clone(),
                    template: r.taunt.clone(),
                    line: None,
                    day: 0,
                    offer: None,
                    read: false,
                });
            }
        }
    }
}

fn push_hall(clan: &mut Clan, text: String) {
    clan.hall.push(HallEntry {
        season: clan.season,
        text,
    });
    if clan.hall.len() > crate::clan::HALL_CAP {
        clan.hall.remove(0);
    }
}

/// A unit's numbers as the board shows them to raiders.
fn defender_of(cat: &Catalog, c: &crate::crew::Crew, armed: bool) -> Defender {
    let u = Unit::from_crew(cat, c, armed);
    Defender {
        id: c.id,
        name: c.name.clone(),
        trade: c.trade,
        hp: u.hp,
        atk: u.atk,
        def: u.def,
        spd: u.spd,
        mass: u.mass,
        row: u.row,
        aquatic: u.aquatic,
        armed,
    }
}

/// The crew who could stand at the gate or go raiding: fit, in the hold, not away.
#[must_use]
pub fn available(cat: &Catalog, clan: &Clan) -> Vec<Defender> {
    let out_on_trip = |id: u16| clan.trip.as_ref().is_some_and(|t| t.party.contains(&id));
    let arms = usize::try_from(clan.stock.get(Resource::Arms)).unwrap_or(0);
    let mut fit: Vec<&crate::crew::Crew> = clan
        .crew
        .iter()
        .filter(|c| c.fit(cat) && !out_on_trip(c.id) && !clan.social.is_away(c.id))
        .collect();
    fit.sort_by_key(|c| {
        let u = Unit::from_crew(cat, c, false);
        (
            std::cmp::Reverse(i32::from(u.hp) * i32::from(u.atk.max(1))),
            c.id,
        )
    });
    fit.iter()
        .enumerate()
        .map(|(i, c)| defender_of(cat, c, i < arms))
        .collect()
}

impl Posted {
    /// What the clan shows the marsh today.
    #[must_use]
    pub fn of(cat: &Catalog, clan: &Clan, today: i64) -> Self {
        let mut guard = available(cat, clan);
        guard.truncate(caps::MAX_DEFENDERS);
        let rating = clan.hold.defence(cat, 0);
        let flooded = clan.hold.defence(cat, 100);
        let sluice = clan
            .hold
            .sluice_level
            .filter(|_| clan.hold.built(cat).any(|(_, b)| b.is_sluice()));
        let mut stock = [0u16; RESOURCES];
        for r in Resource::ALL {
            // A scout's guess, in fives: the post is not rewritten for every shift worked.
            let n = clan.stock.get(r) / 5 * 5;
            stock[r.index()] = u16::try_from(n).unwrap_or(u16::MAX);
        }
        Self {
            seen: today,
            founded: clan.founded,
            guard,
            rating: u8::try_from(rating).unwrap_or(u8::MAX),
            flood: u8::try_from(flooded.saturating_sub(rating)).unwrap_or(u8::MAX),
            sluice,
            stock,
            tallies: u16::try_from(clan.tallies / 10 * 10).unwrap_or(u16::MAX),
            buildings: u8::try_from(clan.hold.built(cat).count()).unwrap_or(u8::MAX),
            causeways: 0,
            crew: u8::try_from(clan.crew.len()).unwrap_or(u8::MAX),
        }
    }
}

/// The reason line for a refusal in the report.
#[must_use]
pub fn refusal_line(what: &str, why: Refusal) -> String {
    format!("The Tidecourt turned your {what} away: {}", why.text())
}

#[cfg(test)]
mod tests;
