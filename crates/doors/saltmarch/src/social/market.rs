//! The player market, the Tidecourt's exchange: standing asks and bids across all clans.
//!
//! Orders live in the world and hold their own escrow (an ask holds the goods, a bid holds
//! `qty * price` tallies), so the only way goods and tallies move between clans is a
//! [`Parcel`] delivery. An order that crosses the book trades at once against the resting
//! orders (price first, then age), at the resting order's price; the rest stands for
//! [`caps::ORDER_DAYS`] days. The exchange keeps 3% of every sale and a 2% listing fee is paid
//! when an order is posted, so trade is never free and goods are never made from nothing.

use super::{
    Body, ClanIx, Refusal, Wire, caps, read_list, read_resource, write_list, write_resource,
};
use crate::codec::{Reader, Writer};
use crate::types::{RESOURCES, Resource};
use crate::world::World;

/// Why a parcel was sent, for the line in the morning report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Memo {
    /// Proceeds of a sale (`about`: what and how many were sold).
    Sold,
    /// Goods bought (`about`: what and how many), with any change.
    Bought,
    /// An order lapsed and its escrow came back.
    Expired,
    /// A cancelled order's escrow.
    Cancelled,
    /// An order the world refused: its escrow came back.
    Refused,
}

impl Memo {
    const fn code(self) -> u8 {
        self as u8
    }

    const fn from_code(c: u8) -> Option<Self> {
        Some(match c {
            0 => Self::Sold,
            1 => Self::Bought,
            2 => Self::Expired,
            3 => Self::Cancelled,
            4 => Self::Refused,
            _ => return None,
        })
    }
}

/// Goods and tallies the world owes a clan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parcel {
    /// Goods in the parcel.
    pub goods: Option<(Resource, u32)>,
    /// Tallies in the parcel.
    pub tallies: u32,
    /// Why.
    pub memo: Memo,
    /// What the trade was, for the line in the report: what and how many changed hands.
    pub about: Option<(Resource, u32)>,
}

impl Wire for Parcel {
    fn write(&self, w: &mut Writer) {
        match self.goods {
            Some((r, n)) => {
                w.bool(true);
                write_resource(w, r);
                w.u32(n);
            }
            None => w.bool(false),
        }
        w.u32(self.tallies);
        w.u8(self.memo.code());
        match self.about {
            Some((r, n)) => {
                w.bool(true);
                write_resource(w, r);
                w.u32(n);
            }
            None => w.bool(false),
        }
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let goods = if r.bool()? {
            Some((read_resource(r)?, r.u32()?))
        } else {
            None
        };
        let (tallies, memo) = (r.u32()?, Memo::from_code(r.u8()?)?);
        let about = if r.bool()? {
            Some((read_resource(r)?, r.u32()?))
        } else {
            None
        };
        Some(Self {
            goods,
            tallies,
            memo,
            about,
        })
    }
}

/// A standing order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    /// Its id.
    pub id: u32,
    /// Whose.
    pub owner: ClanIx,
    /// What.
    pub what: Resource,
    /// How many are left.
    pub qty: u32,
    /// Tallies each.
    pub price: u16,
    /// The day it was posted.
    pub day: i64,
}

impl Wire for Order {
    fn write(&self, w: &mut Writer) {
        w.u32(self.id);
        w.u8(self.owner);
        write_resource(w, self.what);
        w.u32(self.qty);
        w.u16(self.price);
        w.i64(self.day);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        Some(Self {
            id: r.u32()?,
            owner: r.u8()?,
            what: read_resource(r)?,
            qty: r.u32()?,
            price: r.u16()?,
            day: r.i64()?,
        })
    }
}

/// Nights of exchange prices kept for the sparklines.
pub const HISTORY_DAYS: usize = 14;

/// Which side of the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Offering goods for tallies.
    Ask,
    /// Offering tallies for goods.
    Bid,
}

/// The book and its price history.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exchange {
    /// Goods for sale, in no particular order.
    pub asks: Vec<Order>,
    /// Tallies offered, in no particular order.
    pub bids: Vec<Order>,
    /// Quantity and value traded since the last night, per good.
    pub today: [(u32, u32); RESOURCES],
    /// Each night's volume-weighted price in hundredths of a tally (0: nothing traded).
    pub history: Vec<[u16; RESOURCES]>,
}

impl Exchange {
    /// Write.
    pub fn write(&self, w: &mut Writer) {
        write_list(w, &self.asks);
        write_list(w, &self.bids);
        for (q, v) in self.today {
            w.u32(q);
            w.u32(v);
        }
        w.count(self.history.len());
        for row in &self.history {
            for p in row {
                w.u16(*p);
            }
        }
    }

    /// Read.
    pub fn read(r: &mut Reader<'_>) -> Option<Self> {
        let asks = read_list(r)?;
        let bids = read_list(r)?;
        let mut today = [(0, 0); RESOURCES];
        for slot in &mut today {
            *slot = (r.u32()?, r.u32()?);
        }
        let history = (0..r.u8()?)
            .map(|_| {
                let mut row = [0u16; RESOURCES];
                for p in &mut row {
                    *p = r.u16()?;
                }
                Some(row)
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            asks,
            bids,
            today,
            history,
        })
    }

    /// How many standing orders `clan` has.
    #[must_use]
    pub fn orders_of(&self, clan: ClanIx) -> usize {
        self.asks
            .iter()
            .chain(&self.bids)
            .filter(|o| o.owner == clan)
            .count()
    }

    /// The cheapest ask for `what` not owned by `except`: price, then age.
    #[must_use]
    pub fn best_ask(&self, what: Resource, except: Option<ClanIx>) -> Option<&Order> {
        self.asks
            .iter()
            .filter(|o| o.what == what && Some(o.owner) != except)
            .min_by_key(|o| (o.price, o.id))
    }

    /// The highest bid for `what` not owned by `except`: price, then age.
    #[must_use]
    pub fn best_bid(&self, what: Resource, except: Option<ClanIx>) -> Option<&Order> {
        self.bids
            .iter()
            .filter(|o| o.what == what && Some(o.owner) != except)
            .max_by_key(|o| (o.price, std::cmp::Reverse(o.id)))
    }

    /// An order of this id, on either side.
    #[must_use]
    pub fn order(&self, id: u32) -> Option<(Side, &Order)> {
        self.asks
            .iter()
            .find(|o| o.id == id)
            .map(|o| (Side::Ask, o))
            .or_else(|| {
                self.bids
                    .iter()
                    .find(|o| o.id == id)
                    .map(|o| (Side::Bid, o))
            })
    }

    /// The nightly prices of `what`, oldest first, a night with no trade carrying the last
    /// price forward and the nights before the first trade left out.
    #[must_use]
    pub fn trail(&self, what: Resource) -> Vec<u16> {
        let mut last = 0u16;
        let mut out = Vec::new();
        for row in &self.history {
            let p = row[what.index()];
            if p > 0 {
                last = p;
            }
            if last > 0 {
                out.push(last);
            }
        }
        out
    }

    /// Close the night's trading into the history.
    pub fn end_day(&mut self) {
        let mut row = [0u16; RESOURCES];
        for (i, (qty, value)) in self.today.iter().enumerate() {
            if *qty > 0 {
                row[i] =
                    u16::try_from(u64::from(*value) * 100 / u64::from(*qty)).unwrap_or(u16::MAX);
            }
        }
        self.today = [(0, 0); RESOURCES];
        self.history.push(row);
        let extra = self.history.len().saturating_sub(HISTORY_DAYS);
        self.history.drain(..extra);
    }
}

/// The fee for posting an order worth `value` tallies; halved between pact partners' trade
/// pacts is not possible here (the book is anonymous), so it is one rate for everyone.
#[must_use]
pub fn listing_fee(value: u32) -> u32 {
    if value == 0 {
        0
    } else {
        (value * caps::LISTING_FEE_PCT).div_ceil(100).max(1)
    }
}

/// What the exchange keeps of a sale of `value` tallies. A trade pact between the two clans
/// halves it.
#[must_use]
pub fn levy(value: u32, partners: bool) -> u32 {
    let pct = if partners {
        caps::LEVY_PCT / 2
    } else {
        caps::LEVY_PCT
    };
    value * pct / 100
}

fn valid(qty: u32, price: u16) -> bool {
    (1..=caps::ORDER_QTY).contains(&qty) && (1..=9_999).contains(&price)
}

/// What a post did, for the tests and the caller's receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Filled {
    /// Quantity traded at once.
    pub qty: u32,
    /// The part still standing.
    pub resting: u32,
}

/// Post an order for `owner`. It trades at once against the book where it crosses; what is
/// left stands. The owner's parcel for the fills is delivered here.
///
/// # Errors
/// [`Refusal::BadOrder`] for a bad quantity or price, [`Refusal::TooMany`] if the clan has
/// [`caps::OWN_ORDERS`] standing, [`Refusal::Full`] if the book is full, and
/// [`Refusal::MailFull`] if the clan has no room for the parcel.
#[allow(clippy::too_many_lines)]
pub fn post(
    w: &mut World,
    owner: ClanIx,
    side: Side,
    what: Resource,
    qty: u32,
    price: u16,
    day: i64,
) -> Result<Filled, Refusal> {
    if !valid(qty, price) || what == Resource::Relic {
        return Err(Refusal::BadOrder);
    }
    if w.social.exchange.orders_of(owner) >= caps::OWN_ORDERS {
        return Err(Refusal::TooMany);
    }
    if !w.social.has_room(owner) {
        return Err(Refusal::MailFull);
    }
    let book_len = match side {
        Side::Ask => w.social.exchange.asks.len(),
        Side::Bid => w.social.exchange.bids.len(),
    };
    if book_len >= caps::BOOK_CAP {
        return Err(Refusal::Full);
    }
    let mut left = qty;
    let mut gained_tallies = 0u32;
    let mut gained_goods = 0u32;
    while left > 0 {
        // The best resting order on the other side that this one crosses and whose owner has
        // room for the parcel.
        let pick = {
            let x = &w.social.exchange;
            let ok = |o: &&Order| {
                o.what == what
                    && o.owner != owner
                    && w.social.has_room(o.owner)
                    && match side {
                        Side::Ask => o.price >= price,
                        Side::Bid => o.price <= price,
                    }
            };
            match side {
                Side::Ask => x
                    .bids
                    .iter()
                    .filter(ok)
                    .max_by_key(|o| (o.price, std::cmp::Reverse(o.id)))
                    .map(|o| o.id),
                Side::Bid => x
                    .asks
                    .iter()
                    .filter(ok)
                    .min_by_key(|o| (o.price, o.id))
                    .map(|o| o.id),
            }
        };
        let Some(id) = pick else { break };
        let book = match side {
            Side::Ask => &mut w.social.exchange.bids,
            Side::Bid => &mut w.social.exchange.asks,
        };
        let Some(at) = book.iter().position(|o| o.id == id) else {
            break;
        };
        let n = left.min(book[at].qty);
        let (resting_owner, trade_price) = (book[at].owner, u32::from(book[at].price));
        book[at].qty -= n;
        if book[at].qty == 0 {
            book.remove(at);
        }
        left -= n;
        let value = n * trade_price;
        let pact = w.has_trade_pact(owner, resting_owner);
        let cut = levy(value, pact);
        let slot = &mut w.social.exchange.today[what.index()];
        *slot = (slot.0.saturating_add(n), slot.1.saturating_add(value));
        match side {
            Side::Ask => {
                // The taker sold to a standing bid: the bidder's goods, the taker's tallies.
                gained_tallies += value - cut;
                w.social.deliver(
                    resting_owner,
                    Body::Parcel(Parcel {
                        goods: Some((what, n)),
                        tallies: 0,
                        memo: Memo::Bought,
                        about: Some((what, n)),
                    }),
                );
            }
            Side::Bid => {
                // The taker bought from a standing ask: the taker's goods and change, the
                // seller's tallies.
                gained_goods += n;
                gained_tallies += n * (u32::from(price) - trade_price);
                w.social.deliver(
                    resting_owner,
                    Body::Parcel(Parcel {
                        goods: None,
                        tallies: value - cut,
                        memo: Memo::Sold,
                        about: Some((what, n)),
                    }),
                );
            }
        }
        w.note_traded(owner, value);
        w.note_traded(resting_owner, value);
    }
    if gained_tallies > 0 || gained_goods > 0 {
        let memo = match side {
            Side::Ask => Memo::Sold,
            Side::Bid => Memo::Bought,
        };
        w.social.deliver(
            owner,
            Body::Parcel(Parcel {
                goods: (gained_goods > 0).then_some((what, gained_goods)),
                tallies: gained_tallies,
                memo,
                about: Some((what, qty - left)),
            }),
        );
    }
    if left > 0 {
        let id = w.social.id();
        let order = Order {
            id,
            owner,
            what,
            qty: left,
            price,
            day,
        };
        match side {
            Side::Ask => w.social.exchange.asks.push(order),
            Side::Bid => w.social.exchange.bids.push(order),
        }
    }
    Ok(Filled {
        qty: qty - left,
        resting: left,
    })
}

/// Take back a standing order; its escrow comes back as a parcel.
///
/// # Errors
/// [`Refusal::Missing`], [`Refusal::NotYours`] or [`Refusal::MailFull`].
pub fn cancel(w: &mut World, owner: ClanIx, id: u32) -> Result<(), Refusal> {
    let Some((side, o)) = w.social.exchange.order(id) else {
        return Err(Refusal::Missing);
    };
    if o.owner != owner {
        return Err(Refusal::NotYours);
    }
    if !w.social.has_room(owner) {
        return Err(Refusal::MailFull);
    }
    let parcel = escrow_back(side, o, Memo::Cancelled);
    match side {
        Side::Ask => w.social.exchange.asks.retain(|x| x.id != id),
        Side::Bid => w.social.exchange.bids.retain(|x| x.id != id),
    }
    w.social.deliver(owner, Body::Parcel(parcel));
    Ok(())
}

fn escrow_back(side: Side, o: &Order, memo: Memo) -> Parcel {
    match side {
        Side::Ask => Parcel {
            goods: Some((o.what, o.qty)),
            tallies: 0,
            memo,
            about: Some((o.what, o.qty)),
        },
        Side::Bid => Parcel {
            goods: None,
            tallies: o.qty * u32::from(o.price),
            memo,
            about: Some((o.what, o.qty)),
        },
    }
}

/// The nightly pass: orders older than [`caps::ORDER_DAYS`] lapse and their escrow goes back,
/// then the night's prices are closed into the history.
pub fn tick(w: &mut World, day: i64) {
    let mut back: Vec<(ClanIx, Parcel)> = Vec::new();
    for (side, book) in [
        (Side::Ask, std::mem::take(&mut w.social.exchange.asks)),
        (Side::Bid, std::mem::take(&mut w.social.exchange.bids)),
    ] {
        let (keep, lapsed): (Vec<Order>, Vec<Order>) = book
            .into_iter()
            .partition(|o| day - o.day < caps::ORDER_DAYS);
        for o in &lapsed {
            back.push((o.owner, escrow_back(side, o, Memo::Expired)));
        }
        match side {
            Side::Ask => w.social.exchange.asks = keep,
            Side::Bid => w.social.exchange.bids = keep,
        }
    }
    for (owner, parcel) in back {
        w.social.deliver(owner, Body::Parcel(parcel));
    }
    w.social.exchange.end_day();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::assert_is_empty)]
    use super::*;

    fn world() -> World {
        let mut w = World::new(1, 7, 100);
        for n in ["Ash", "Brindle", "Cove"] {
            w.register(n, 0);
        }
        w
    }

    fn parcels(w: &World, to: ClanIx) -> Vec<Parcel> {
        w.social
            .mail
            .iter()
            .filter(|d| d.to == to)
            .filter_map(|d| match &d.body {
                Body::Parcel(p) => Some(p.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_ask_waits_and_a_crossing_bid_trades_at_the_asking_price_with_change() {
        let mut w = world();
        let filled = post(&mut w, 0, Side::Ask, Resource::Salt, 10, 5, 100).unwrap();
        assert_eq!(
            filled,
            Filled {
                qty: 0,
                resting: 10
            }
        );
        // A bid of 7 each for 4 pays the ask's 5: 20 tallies, change 8, 4 salt.
        let filled = post(&mut w, 1, Side::Bid, Resource::Salt, 4, 7, 100).unwrap();
        assert_eq!(filled, Filled { qty: 4, resting: 0 });
        assert_eq!(w.social.exchange.asks[0].qty, 6);
        let buyer = parcels(&w, 1);
        assert_eq!(
            buyer,
            [Parcel {
                goods: Some((Resource::Salt, 4)),
                tallies: 8,
                memo: Memo::Bought,
                about: Some((Resource::Salt, 4)),
            }]
        );
        // The seller gets 20 less the 3% levy (0 on 20), and the volume is recorded.
        let seller = parcels(&w, 0);
        assert_eq!(seller[0].tallies, 20);
        assert_eq!(w.social.exchange.today[Resource::Salt.index()], (4, 20));
    }

    #[test]
    fn a_sale_into_a_standing_bid_pays_the_bidders_price_and_the_levy_is_kept() {
        let mut w = world();
        post(&mut w, 1, Side::Bid, Resource::Iron, 10, 20, 100).unwrap();
        post(&mut w, 0, Side::Ask, Resource::Iron, 10, 15, 100).unwrap();
        assert!(w.social.exchange.bids.is_empty() && w.social.exchange.asks.is_empty());
        // 200 tallies less 3% = 194 to the seller; the iron goes to the bidder.
        assert_eq!(parcels(&w, 0)[0].tallies, 194);
        assert_eq!(parcels(&w, 1)[0].goods, Some((Resource::Iron, 10)));
    }

    #[test]
    fn nothing_is_made_or_lost_but_the_levy() {
        let mut w = world();
        // Escrow in: 10 iron (ask) and 10*20 tallies (bid). Out: parcels plus the levy.
        post(&mut w, 1, Side::Bid, Resource::Iron, 10, 20, 100).unwrap();
        post(&mut w, 0, Side::Ask, Resource::Iron, 10, 15, 100).unwrap();
        let goods: u32 = w
            .social
            .mail
            .iter()
            .filter_map(|d| match &d.body {
                Body::Parcel(p) => p.goods.map(|g| g.1),
                _ => None,
            })
            .sum();
        let tallies: u32 = w
            .social
            .mail
            .iter()
            .filter_map(|d| match &d.body {
                Body::Parcel(p) => Some(p.tallies),
                _ => None,
            })
            .sum();
        assert_eq!(goods, 10);
        assert_eq!(tallies + levy(200, false), 200);
    }

    #[test]
    fn the_best_price_trades_first_then_the_oldest() {
        let mut w = world();
        post(&mut w, 0, Side::Ask, Resource::Salt, 5, 6, 100).unwrap();
        post(&mut w, 1, Side::Ask, Resource::Salt, 5, 5, 100).unwrap();
        post(&mut w, 1, Side::Ask, Resource::Salt, 5, 5, 100).unwrap();
        post(&mut w, 2, Side::Bid, Resource::Salt, 7, 9, 100).unwrap();
        // The two 5s trade (5 then 2), the 6 is untouched.
        assert_eq!(w.social.exchange.asks.len(), 2);
        let left: Vec<(u16, u32)> = w
            .social
            .exchange
            .asks
            .iter()
            .map(|o| (o.price, o.qty))
            .collect();
        assert!(left.contains(&(6, 5)) && left.contains(&(5, 3)), "{left:?}");
    }

    #[test]
    fn nobody_trades_with_themselves_and_bad_orders_are_refused() {
        let mut w = world();
        post(&mut w, 0, Side::Ask, Resource::Salt, 5, 5, 100).unwrap();
        let f = post(&mut w, 0, Side::Bid, Resource::Salt, 5, 9, 100).unwrap();
        assert_eq!(f.qty, 0, "no self-trade");
        assert_eq!(
            post(&mut w, 1, Side::Ask, Resource::Salt, 0, 5, 100),
            Err(Refusal::BadOrder)
        );
        assert_eq!(
            post(&mut w, 1, Side::Ask, Resource::Salt, 5, 0, 100),
            Err(Refusal::BadOrder)
        );
        assert_eq!(
            post(&mut w, 1, Side::Ask, Resource::Relic, 1, 5, 100),
            Err(Refusal::BadOrder)
        );
        assert_eq!(
            post(&mut w, 1, Side::Ask, Resource::Salt, 500, 5, 100),
            Err(Refusal::BadOrder)
        );
    }

    #[test]
    fn a_clan_can_only_have_a_few_orders_standing() {
        let mut w = world();
        for _ in 0..caps::OWN_ORDERS {
            post(&mut w, 0, Side::Ask, Resource::Salt, 1, 9, 100).unwrap();
        }
        assert_eq!(
            post(&mut w, 0, Side::Ask, Resource::Salt, 1, 9, 100),
            Err(Refusal::TooMany)
        );
    }

    #[test]
    fn cancelling_returns_the_escrow_to_its_owner_only() {
        let mut w = world();
        post(&mut w, 0, Side::Ask, Resource::Salt, 8, 9, 100).unwrap();
        post(&mut w, 1, Side::Bid, Resource::Iron, 3, 11, 100).unwrap();
        let ask = w.social.exchange.asks[0].id;
        let bid = w.social.exchange.bids[0].id;
        assert_eq!(cancel(&mut w, 1, ask), Err(Refusal::NotYours));
        assert_eq!(cancel(&mut w, 0, 999), Err(Refusal::Missing));
        cancel(&mut w, 0, ask).unwrap();
        cancel(&mut w, 1, bid).unwrap();
        assert_eq!(parcels(&w, 0)[0].goods, Some((Resource::Salt, 8)));
        assert_eq!(parcels(&w, 1)[0].tallies, 33);
        assert!(w.social.exchange.asks.is_empty() && w.social.exchange.bids.is_empty());
    }

    #[test]
    fn old_orders_lapse_at_the_tick_and_the_nights_prices_are_kept() {
        let mut w = world();
        post(&mut w, 0, Side::Ask, Resource::Salt, 8, 9, 100).unwrap();
        post(&mut w, 1, Side::Bid, Resource::Salt, 2, 12, 100).unwrap();
        tick(&mut w, 101);
        assert_eq!(w.social.exchange.history.len(), 1);
        assert_eq!(w.social.exchange.history[0][Resource::Salt.index()], 900);
        tick(&mut w, 100 + caps::ORDER_DAYS);
        assert!(w.social.exchange.asks.is_empty());
        let back = parcels(&w, 0);
        assert!(
            back.iter()
                .any(|p| p.memo == Memo::Expired && p.goods == Some((Resource::Salt, 6)))
        );
        assert_eq!(w.social.exchange.trail(Resource::Salt), [900, 900]);
    }

    #[test]
    fn fees_never_round_to_free() {
        assert_eq!(listing_fee(0), 0);
        assert_eq!(listing_fee(10), 1);
        assert_eq!(listing_fee(100), 2);
        assert_eq!(levy(100, false), 3);
        assert_eq!(levy(100, true), 1);
    }
}
