//! Dispatches: what a session asks the world to do for it, as one compare-and-swap edit that
//! always leaves a receipt (see the module notes of [`super`]).
//!
//! [`run`] is the edit: it takes the stored world bytes and returns the new ones. It may be
//! called more than once for one dispatch (the board retries on a conflict), so it computes
//! only from its argument, and a dispatch it has already applied (its receipt is there) is not
//! applied again.

use super::market::Side;
use super::{ClanIx, Defender, Posted, Receipt, Refusal, caps, letters::Kind};
use crate::content::Catalog;
use crate::story::{Flag, LAY_MAX, TOLL_MAX};
use crate::tide::Tide;
use crate::types::Resource;
use crate::world::{SPANS_CAP, Span, World};

/// What to ask of the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Post an ask.
    Ask {
        /// What.
        what: Resource,
        /// How many.
        qty: u32,
        /// Tallies each.
        price: u16,
    },
    /// Post a bid.
    Bid {
        /// What.
        what: Resource,
        /// How many.
        qty: u32,
        /// Tallies each.
        price: u16,
    },
    /// Take down an order.
    Cancel {
        /// The order.
        order: u32,
    },
    /// Send a letter.
    Letter {
        /// The recipient clan.
        to: String,
        /// The template id.
        template: String,
        /// The sender's own line (already through the board's text rules).
        line: Option<String>,
    },
    /// Offer a pact.
    Offer {
        /// The other clan.
        to: String,
        /// The terms.
        terms: u8,
        /// The words that go with it (an `offer` template id), or empty.
        template: String,
        /// The sender's own line.
        line: Option<String>,
    },
    /// Accept an offer.
    Accept {
        /// The pact.
        pact: u32,
    },
    /// Decline (or withdraw) an offer.
    Decline {
        /// The pact.
        pact: u32,
    },
    /// Break a pact in force.
    Break {
        /// The pact.
        pact: u32,
    },
    /// Send a raid.
    Raid {
        /// The hold.
        to: String,
        /// The turn of the water on the landing day.
        window: u8,
        /// Boats taken.
        boats: u8,
        /// The party.
        raiders: Vec<Defender>,
        /// A taunt template id to leave behind, or empty.
        taunt: String,
    },
    /// Stop letters from a clan.
    Block {
        /// The clan.
        who: String,
    },
    /// Pay a toll to the clan that built a causeway (the tallies left the payer already; the
    /// world turns them into a parcel for the builder).
    Toll {
        /// The builder's clan.
        owner: String,
        /// Tallies.
        tallies: u32,
    },
    /// Lay a causeway tile.
    Lay {
        /// Column.
        x: u8,
        /// Row.
        y: u8,
        /// The toll the builder sets.
        toll: u8,
    },
    /// A quest changed the marsh for everyone.
    Flag {
        /// What changed.
        flag: Flag,
    },
}

/// A request, with the number the clan will find its receipt under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatch {
    /// The sending clan.
    pub from: String,
    /// The clan's dispatch number.
    pub id: u16,
    /// Today.
    pub day: i64,
    /// What to do.
    pub act: Act,
}

fn target(w: &World, name: &str) -> Result<ClanIx, Refusal> {
    w.ix(name).ok_or(Refusal::Unknown)
}

#[allow(clippy::too_many_lines)]
fn apply(w: &mut World, cat: &Catalog, me: ClanIx, d: &Dispatch) -> Result<(), Refusal> {
    let day = d.day;
    match &d.act {
        Act::Ask { what, qty, price } => {
            super::market::post(w, me, Side::Ask, *what, *qty, *price, day).map(|_| ())
        }
        Act::Bid { what, qty, price } => {
            super::market::post(w, me, Side::Bid, *what, *qty, *price, day).map(|_| ())
        }
        Act::Cancel { order } => super::market::cancel(w, me, *order),
        Act::Letter { to, template, line } => {
            let to = target(w, to)?;
            w.send_letter(&cat.letters, me, to, template, line.as_deref(), day)
        }
        Act::Offer {
            to,
            terms,
            template,
            line,
        } => {
            let to = target(w, to)?;
            if !template.is_empty() {
                let ok = cat
                    .letters
                    .entries
                    .iter()
                    .find(|l| l.id == *template)
                    .is_some_and(|l| l.kind == Kind::Offer && w.opens(me, l.requires));
                if !ok {
                    return Err(Refusal::Terms);
                }
            }
            let line = line
                .clone()
                .filter(|l| !l.trim().is_empty() && l.chars().count() <= super::letters::LINE_MAX);
            w.propose_with(me, to, *terms, day, template.clone(), line)
                .map(|_| ())
        }
        Act::Accept { pact } => w.accept(me, *pact, day),
        Act::Decline { pact } => w.decline(me, *pact),
        Act::Break { pact } => w.break_pact(me, *pact, day),
        Act::Raid {
            to,
            window,
            boats,
            raiders,
            taunt,
        } => {
            let to = target(w, to)?;
            if !taunt.is_empty() {
                let ok = cat
                    .letters
                    .entries
                    .iter()
                    .find(|l| l.id == *taunt)
                    .is_some_and(|l| l.kind == Kind::Taunt && w.opens(me, l.requires));
                if !ok {
                    return Err(Refusal::Terms);
                }
            }
            let tide = Tide::new(w.seed);
            super::raid::place(
                w,
                &tide,
                me,
                to,
                day,
                *window,
                *boats,
                raiders.clone(),
                taunt,
            )
            .map(|_| ())
        }
        Act::Block { who } => {
            let who = target(w, who)?;
            w.block(me, who)
        }
        Act::Toll { owner, tallies } => {
            let owner = target(w, owner)?;
            if owner == me || *tallies == 0 {
                return Err(Refusal::Unknown);
            }
            if w.social.mail_for(owner) >= caps::MAIL_PER_CLAN {
                return Err(Refusal::MailFull);
            }
            w.social.deliver(
                owner,
                super::Body::Parcel(super::market::Parcel {
                    goods: None,
                    tallies: *tallies,
                    memo: super::market::Memo::Toll,
                    about: None,
                }),
            );
            Ok(())
        }
        Act::Lay { x, y, toll } => lay(w, cat, me, (*x, *y), *toll, day),
        Act::Flag { flag } => {
            apply_flag(w, *flag);
            Ok(())
        }
    }
}

/// Lay a causeway tile for `me`, if the ground and the caps allow.
fn lay(
    w: &mut World,
    cat: &Catalog,
    me: ClanIx,
    (x, y): (u8, u8),
    toll: u8,
    day: i64,
) -> Result<(), Refusal> {
    if w.causeways.len() >= SPANS_CAP {
        return Err(Refusal::Full);
    }
    if w.causeways.iter().filter(|c| c.owner == me).count() >= usize::from(LAY_MAX) {
        return Err(Refusal::TooMany);
    }
    if w.causeways.iter().any(|c| c.x == x && c.y == y) {
        return Err(Refusal::Blocked);
    }
    let mut map = crate::map::Map::generate(w.seed, &cat.pois);
    map.apply_flags(w.flags, &cat.pois);
    let ok = map
        .tile(usize::from(x), usize::from(y))
        .is_some_and(|t| crate::map::can_lay_on(t.kind) && map.poi_at(x, y).is_none());
    if !ok {
        return Err(Refusal::Blocked);
    }
    w.causeways.push(Span {
        x,
        y,
        owner: me,
        toll: toll.min(TOLL_MAX),
        laid: day,
    });
    Ok(())
}

/// A quest's change to the marsh: a surge takes the oldest causeway once; the rest set a
/// lasting bit that every session's map reads.
fn apply_flag(w: &mut World, flag: Flag) {
    if flag == Flag::Washout {
        if let Some(i) = w
            .causeways
            .iter()
            .enumerate()
            .min_by_key(|(i, c)| (c.laid, *i))
            .map(|(i, _)| i)
        {
            w.causeways.remove(i);
        }
    } else {
        w.flags |= flag.bit();
    }
}

/// The world edit for a dispatch: the new world bytes, or `None` for no change at all (the
/// world is not this season's, the clan is not registered, or its receipts are full: in each
/// case the clan will find no receipt and take back what it risked).
#[must_use]
pub fn run(stored: Option<&[u8]>, season: u32, d: &Dispatch, cat: &Catalog) -> Option<Vec<u8>> {
    let orig = crate::world::World::decode(stored?).filter(|w| w.season == season)?;
    let me = orig.ix(&d.from)?;
    if orig
        .social
        .receipts
        .iter()
        .any(|r| r.clan == me && r.id == d.id)
    {
        return None;
    }
    if orig.social.receipts.iter().filter(|r| r.clan == me).count() >= caps::RECEIPTS_PER_CLAN {
        return None;
    }
    let mut tried = orig.clone();
    let result = apply(&mut tried, cat, me, d);
    let finish = |mut w: World, refused: Option<Refusal>| {
        w.social.receipts.push(Receipt {
            clan: me,
            id: d.id,
            refused,
        });
        w.encode()
    };
    match result {
        Ok(()) => {
            let bytes = finish(tried, None);
            if bytes.len() <= caps::WORLD_SOFT_CAP {
                return Some(bytes);
            }
            // Too big to keep: turn it away instead.
            let bytes = finish(orig, Some(Refusal::Full));
            (bytes.len() <= caps::WORLD_SOFT_CAP).then_some(bytes)
        }
        Err(why) => {
            let bytes = finish(orig, Some(why));
            (bytes.len() <= caps::WORLD_SOFT_CAP).then_some(bytes)
        }
    }
}

/// The world edit that updates a clan's posted defence and renown. `None` when nothing
/// changed.
#[must_use]
pub fn post_update(
    stored: Option<&[u8]>,
    season: u32,
    name: &str,
    posted: &Posted,
    score: Option<u32>,
    badge: crate::world::Badge,
) -> Option<Vec<u8>> {
    let mut w = World::decode(stored?).filter(|w| w.season == season)?;
    let me = usize::from(w.ix(name)?);
    let entry = &mut w.clans[me];
    let mut changed = false;
    if entry.posted != *posted {
        entry.posted = posted.clone();
        changed = true;
    }
    if let Some(s) = score
        && entry.score != s
    {
        entry.score = s;
        changed = true;
    }
    if entry.badge != badge {
        entry.badge = badge;
        changed = true;
    }
    if !changed {
        return None;
    }
    let bytes = w.encode();
    (bytes.len() <= caps::WORLD_SOFT_CAP).then_some(bytes)
}

/// The world edit that clears what a clan has folded: its deliveries up to `upto` and the
/// receipts for the dispatches it has settled. `None` when there is nothing to clear.
#[must_use]
pub fn ack(
    stored: Option<&[u8]>,
    season: u32,
    name: &str,
    upto: u32,
    receipts: &[u16],
) -> Option<Vec<u8>> {
    let mut w = World::decode(stored?).filter(|w| w.season == season)?;
    let me = w.ix(name)?;
    let (m, r) = (w.social.mail.len(), w.social.receipts.len());
    w.social.mail.retain(|d| !(d.to == me && d.seq <= upto));
    w.social
        .receipts
        .retain(|x| !(x.clan == me && receipts.contains(&x.id)));
    (w.social.mail.len() != m || w.social.receipts.len() != r).then(|| w.encode())
}

/// What the world holds for a clan, for the session's fold.
#[must_use]
pub fn mail_for(w: &World, name: &str) -> super::ledger::Mail {
    let Some(me) = w.ix(name) else {
        return super::ledger::Mail::default();
    };
    super::ledger::Mail {
        deliveries: w
            .social
            .mail
            .iter()
            .filter(|d| d.to == me)
            .cloned()
            .collect(),
        receipts: w
            .social
            .receipts
            .iter()
            .filter(|r| r.clan == me)
            .copied()
            .collect(),
    }
}

#[cfg(test)]
mod tests;
