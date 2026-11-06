//! Pacts and diplomacy: proposals, terms, breaking a pact, and the reputation that remembers.
//!
//! A pact is between two clans and lives in the world. Its terms:
//!
//! | Term | Effect | Open to a clan that... |
//! |---|---|---|
//! | **Truce** | neither may raid the other | has not broken two pacts, is not an outlaw and is not in the cooling-off after breaking one |
//! | **Trade** | the exchange's levy on their trades is halved | has finished three buildings, and neither has ever broken a pact |
//! | **Defence** | each side's posted guard fights with +2 hit points a point of rating and +1 defence | has a defence building, and neither has ever broken a pact |
//! | **Causeway** | the partners cross each other's causeways without toll | has laid a causeway |
//!
//! **Reputation remembers.** Breaking a pact is allowed but public: it is a deed on the feed,
//! it costs 25 standing with the Tidecourt, it starts a cooling-off during which the breaker
//! may offer nothing, the two clans may never sign again this season, and a clan with a betrayal
//! on record is closed out of the Trade and Defence pacts of *everyone* for the season. A
//! second betrayal closes the Truce too. Keeping a pact for a week earns standing.

use super::{ClanIx, DeedKind, Refusal, Wire, caps};
use crate::codec::{Reader, Writer};
use crate::world::World;

/// A truce: no raids between the two.
pub const TRUCE: u8 = 1;
/// Trade: half the exchange's levy.
pub const TRADE: u8 = 2;
/// Defence: a better guard.
pub const DEFENCE: u8 = 4;
/// Causeway: no tolls between the two.
pub const CAUSEWAY: u8 = 8;
/// Every term.
pub const ALL_TERMS: [u8; 4] = [TRUCE, TRADE, DEFENCE, CAUSEWAY];

/// Standing lost by breaking a pact.
pub const BREAK_COST: i8 = 25;
/// Standing at or below which a clan is an outlaw.
pub const OUTLAW: i8 = -60;
/// Days a clan that broke a pact may offer nothing.
pub const COOLING_DAYS: i64 = 3;
/// Days after a raid between two clans before they may sign.
pub const GRUDGE_DAYS: i64 = 7;
/// Active pacts one clan may hold.
pub const ACTIVE_CAP: usize = 3;
/// Days an unanswered offer stands before it lapses.
pub const OFFER_DAYS: i64 = 7;

/// The name of a term.
#[must_use]
pub const fn term_name(term: u8) -> &'static str {
    match term {
        TRUCE => "truce",
        TRADE => "trade",
        DEFENCE => "mutual defence",
        CAUSEWAY => "shared causeways",
        _ => "?",
    }
}

/// What a term does, in a line.
#[must_use]
pub const fn term_blurb(term: u8) -> &'static str {
    match term {
        TRUCE => "neither clan raids the other",
        TRADE => "half the exchange levy between you",
        DEFENCE => "each guard fights better when the other is raided",
        CAUSEWAY => "no tolls on each other's causeways",
        _ => "",
    }
}

/// The state of a pact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Offered; not yet answered.
    Proposed,
    /// In force.
    Active,
    /// Broken: kept as the memory that the two may not sign again.
    Broken,
}

/// A pact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pact {
    /// Its id.
    pub id: u32,
    /// The proposer.
    pub a: ClanIx,
    /// The other clan.
    pub b: ClanIx,
    /// The terms (bits).
    pub terms: u8,
    /// Where it stands.
    pub state: State,
    /// The day it was offered, signed or (when broken) broken.
    pub since: i64,
    /// Who broke it, once broken.
    pub by: ClanIx,
}

impl Pact {
    /// Whether the pact is between these two, in either order.
    #[must_use]
    pub const fn joins(&self, x: ClanIx, y: ClanIx) -> bool {
        (self.a == x && self.b == y) || (self.a == y && self.b == x)
    }

    /// Whether `x` is a party.
    #[must_use]
    pub const fn has(&self, x: ClanIx) -> bool {
        self.a == x || self.b == x
    }

    /// The other party.
    #[must_use]
    pub const fn other(&self, x: ClanIx) -> ClanIx {
        if self.a == x { self.b } else { self.a }
    }
}

impl Wire for Pact {
    fn write(&self, w: &mut Writer) {
        w.u32(self.id);
        w.u8(self.a);
        w.u8(self.b);
        w.u8(self.terms);
        w.u8(self.state as u8);
        w.i64(self.since);
        w.u8(self.by);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let (id, a, b, terms) = (r.u32()?, r.u8()?, r.u8()?, r.u8()?);
        let state = match r.u8()? {
            0 => State::Proposed,
            1 => State::Active,
            2 => State::Broken,
            _ => return None,
        };
        Some(Self {
            id,
            a,
            b,
            terms,
            state,
            since: r.i64()?,
            by: r.u8()?,
        })
    }
}

/// What a notice says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// A clan offered a pact.
    Offered,
    /// The offer was accepted.
    Accepted,
    /// The offer was declined.
    Declined,
    /// The pact was broken by the other clan.
    Broken,
}

/// A pact news item delivered to a clan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// What happened.
    pub kind: NoticeKind,
    /// The pact.
    pub pact: u32,
    /// The other clan's name.
    pub with: String,
    /// The terms.
    pub terms: u8,
    /// The words that went with an offer (a letter template id), or empty.
    pub template: String,
    /// The sender's own line with an offer.
    pub line: Option<String>,
}

impl Wire for Notice {
    fn write(&self, w: &mut Writer) {
        w.u8(self.kind as u8);
        w.u32(self.pact);
        w.str(&self.with);
        w.u8(self.terms);
        w.str(&self.template);
        match &self.line {
            Some(l) => {
                w.bool(true);
                w.str(l);
            }
            None => w.bool(false),
        }
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let kind = match r.u8()? {
            0 => NoticeKind::Offered,
            1 => NoticeKind::Accepted,
            2 => NoticeKind::Declined,
            3 => NoticeKind::Broken,
            _ => return None,
        };
        Some(Self {
            kind,
            pact: r.u32()?,
            with: r.str()?,
            terms: r.u8()?,
            template: r.str()?,
            line: if r.bool()? { Some(r.str()?) } else { None },
        })
    }
}

impl World {
    /// The pact in force between two clans, if any.
    #[must_use]
    pub fn pact_between(&self, x: ClanIx, y: ClanIx) -> Option<&Pact> {
        self.social
            .pacts
            .iter()
            .find(|p| p.state == State::Active && p.joins(x, y))
    }

    /// Any record of a pact between the two (offered, in force or broken).
    #[must_use]
    pub fn pact_record(&self, x: ClanIx, y: ClanIx) -> Option<&Pact> {
        self.social.pacts.iter().find(|p| p.joins(x, y))
    }

    /// Whether the two have a trade pact in force.
    #[must_use]
    pub fn has_trade_pact(&self, x: ClanIx, y: ClanIx) -> bool {
        self.pact_between(x, y)
            .is_some_and(|p| p.terms & TRADE != 0)
    }

    /// Pacts in force that include `clan`.
    pub fn pacts_of(&self, clan: ClanIx) -> impl Iterator<Item = &Pact> {
        self.social
            .pacts
            .iter()
            .filter(move |p| p.state == State::Active && p.has(clan))
    }

    /// How many of `clan`'s pacts in force have the defence term.
    #[must_use]
    pub fn defence_partners(&self, clan: ClanIx) -> usize {
        self.pacts_of(clan)
            .filter(|p| p.terms & DEFENCE != 0)
            .count()
    }

    /// Whether `proposer` may put `terms` to `target` today, and if not, why. The same rule the
    /// edit applies, so the screen can show the terms that are open and the reason for each
    /// that is not.
    ///
    /// # Errors
    /// [`Refusal::Unknown`], [`Refusal::HasPact`], [`Refusal::Terms`], [`Refusal::Outlaw`],
    /// [`Refusal::Full`] or [`Refusal::TooMany`].
    pub fn may_offer(
        &self,
        proposer: ClanIx,
        target: ClanIx,
        terms: u8,
        day: i64,
    ) -> Result<(), Refusal> {
        let (Some(me), Some(them)) = (
            self.clans.get(usize::from(proposer)),
            self.clans.get(usize::from(target)),
        ) else {
            return Err(Refusal::Unknown);
        };
        if proposer == target {
            return Err(Refusal::Unknown);
        }
        if terms & TRUCE == 0 || terms & !(TRUCE | TRADE | DEFENCE | CAUSEWAY) != 0 {
            return Err(Refusal::Terms);
        }
        if self.pact_record(proposer, target).is_some() {
            return Err(Refusal::HasPact);
        }
        if self.social.pacts.len() >= caps::PACTS_CAP {
            return Err(Refusal::Full);
        }
        let open = self
            .social
            .pacts
            .iter()
            .filter(|p| p.state == State::Proposed && p.a == proposer)
            .count();
        if open >= caps::OPEN_OFFERS {
            return Err(Refusal::TooMany);
        }
        if self.pacts_of(proposer).count() >= ACTIVE_CAP {
            return Err(Refusal::Full);
        }
        let rec = &me.record;
        if rec.rep <= OUTLAW {
            return Err(Refusal::Outlaw);
        }
        if rec.pact_ban_until > day || rec.betrayals >= 2 {
            return Err(Refusal::Terms);
        }
        // The two raided each other lately: the grudge is fresh.
        if self.social.raid_log.iter().any(|e| {
            day - e.day < GRUDGE_DAYS
                && ((e.from == proposer && e.to == target)
                    || (e.from == target && e.to == proposer))
        }) {
            return Err(Refusal::Terms);
        }
        let clean = rec.betrayals == 0 && them.record.betrayals == 0;
        if terms & TRADE != 0 && !(clean && me.posted.buildings >= 3) {
            return Err(Refusal::Terms);
        }
        if terms & DEFENCE != 0 && !(clean && me.posted.rating > 0) {
            return Err(Refusal::Terms);
        }
        if terms & CAUSEWAY != 0 && me.posted.causeways == 0 {
            return Err(Refusal::Terms);
        }
        Ok(())
    }

    /// Why `term` is closed to `proposer` for `target` today, in plain words, or `None` if it
    /// is open.
    #[must_use]
    pub fn closed_reason(
        &self,
        proposer: ClanIx,
        target: ClanIx,
        term: u8,
        day: i64,
    ) -> Option<&'static str> {
        let why = self.may_offer(proposer, target, TRUCE | term, day).err()?;
        if why != Refusal::Terms {
            return Some(why.text());
        }
        let broken = [proposer, target].iter().any(|c| {
            self.clans
                .get(usize::from(*c))
                .is_some_and(|e| e.record.betrayals > 0)
        });
        // A pact on record says why trade and defence are closed before anything else does.
        if broken && matches!(term, TRADE | DEFENCE) {
            return Some(if term == TRADE {
                "a pact was broken: merchants will not deal"
            } else {
                "a pact was broken: nobody stands guard for an oathbreaker"
            });
        }
        // Otherwise the truce is closed by the proposer's history or by a fresh grudge.
        if let Some(me) = self.clans.get(usize::from(proposer))
            && (me.record.pact_ban_until > day || me.record.betrayals >= 2)
        {
            return Some(if me.record.pact_ban_until > day {
                "cooling off after a broken pact"
            } else {
                "two pacts broken: nobody trusts your word"
            });
        }
        if self.social.raid_log.iter().any(|e| {
            day - e.day < GRUDGE_DAYS
                && ((e.from == proposer && e.to == target)
                    || (e.from == target && e.to == proposer))
        }) {
            return Some("the two of you raided each other within the week");
        }
        Some(match term {
            TRADE => "needs three buildings finished",
            DEFENCE => "needs a defence building",
            CAUSEWAY => "needs a causeway laid",
            _ => "your history closes it for now",
        })
    }

    /// Offer a pact. The target is told by a notice.
    ///
    /// # Errors
    /// See [`World::may_offer`], and [`Refusal::MailFull`].
    pub fn propose(
        &mut self,
        proposer: ClanIx,
        target: ClanIx,
        terms: u8,
        day: i64,
    ) -> Result<u32, Refusal> {
        self.propose_with(proposer, target, terms, day, String::new(), None)
    }

    /// Offer a pact with a letter's words (a template id and the sender's own sanitized line).
    ///
    /// # Errors
    /// See [`World::propose`].
    pub fn propose_with(
        &mut self,
        proposer: ClanIx,
        target: ClanIx,
        terms: u8,
        day: i64,
        template: String,
        line: Option<String>,
    ) -> Result<u32, Refusal> {
        self.may_offer(proposer, target, terms, day)?;
        if !self.social.has_room(target) {
            return Err(Refusal::MailFull);
        }
        let id = self.social.id();
        self.social.pacts.push(Pact {
            id,
            a: proposer,
            b: target,
            terms,
            state: State::Proposed,
            since: day,
            by: 0,
        });
        let with = self.clans[usize::from(proposer)].name.clone();
        self.social.deliver(
            target,
            super::Body::Pact(Notice {
                kind: NoticeKind::Offered,
                pact: id,
                with,
                terms,
                template,
                line,
            }),
        );
        Ok(id)
    }

    /// Accept an offer made to `clan`.
    ///
    /// # Errors
    /// [`Refusal::Missing`], [`Refusal::NotYours`], [`Refusal::Terms`] (the acceptor's own
    /// history closes the terms), [`Refusal::Outlaw`], [`Refusal::Full`] or
    /// [`Refusal::MailFull`].
    pub fn accept(&mut self, clan: ClanIx, pact: u32, day: i64) -> Result<(), Refusal> {
        let Some(p) = self.social.pacts.iter().find(|p| p.id == pact).cloned() else {
            return Err(Refusal::Missing);
        };
        if p.b != clan || p.state != State::Proposed {
            return Err(Refusal::NotYours);
        }
        let (Some(me), Some(them)) = (
            self.clans.get(usize::from(clan)),
            self.clans.get(usize::from(p.a)),
        ) else {
            return Err(Refusal::Unknown);
        };
        if me.record.rep <= OUTLAW {
            return Err(Refusal::Outlaw);
        }
        let unbroken = me.record.betrayals == 0 && them.record.betrayals == 0;
        if me.record.betrayals >= 2 || (p.terms & (TRADE | DEFENCE) != 0 && !unbroken) {
            return Err(Refusal::Terms);
        }
        if self.pacts_of(clan).count() >= ACTIVE_CAP || self.pacts_of(p.a).count() >= ACTIVE_CAP {
            return Err(Refusal::Full);
        }
        if !self.social.has_room(p.a) {
            return Err(Refusal::MailFull);
        }
        let with = me.name.clone();
        if let Some(q) = self.social.pacts.iter_mut().find(|q| q.id == pact) {
            q.state = State::Active;
            q.since = day;
        }
        self.social.deliver(
            p.a,
            super::Body::Pact(Notice {
                kind: NoticeKind::Accepted,
                pact,
                with,
                terms: p.terms,
                template: String::new(),
                line: None,
            }),
        );
        self.social.note(day, DeedKind::PactSigned, p.a, clan);
        Ok(())
    }

    /// Decline an offer, or withdraw one the clan made.
    ///
    /// # Errors
    /// [`Refusal::Missing`] or [`Refusal::NotYours`].
    pub fn decline(&mut self, clan: ClanIx, pact: u32) -> Result<(), Refusal> {
        let Some(p) = self.social.pacts.iter().find(|p| p.id == pact).cloned() else {
            return Err(Refusal::Missing);
        };
        if p.state != State::Proposed || !p.has(clan) {
            return Err(Refusal::NotYours);
        }
        self.social.pacts.retain(|q| q.id != pact);
        if p.b == clan && self.social.has_room(p.a) {
            let with = self.clans[usize::from(clan)].name.clone();
            self.social.deliver(
                p.a,
                super::Body::Pact(Notice {
                    kind: NoticeKind::Declined,
                    pact,
                    with,
                    terms: p.terms,
                    template: String::new(),
                    line: None,
                }),
            );
        }
        Ok(())
    }

    /// Break a pact in force. Allowed, public and remembered (see the module notes).
    ///
    /// # Errors
    /// [`Refusal::Missing`] or [`Refusal::NotYours`].
    pub fn break_pact(&mut self, clan: ClanIx, pact: u32, day: i64) -> Result<(), Refusal> {
        let Some(p) = self.social.pacts.iter().find(|p| p.id == pact).cloned() else {
            return Err(Refusal::Missing);
        };
        if p.state != State::Active || !p.has(clan) {
            return Err(Refusal::NotYours);
        }
        if let Some(q) = self.social.pacts.iter_mut().find(|q| q.id == pact) {
            q.state = State::Broken;
            q.since = day;
            q.by = clan;
        }
        if let Some(c) = self.clans.get_mut(usize::from(clan)) {
            let r = &mut c.record;
            r.betrayals = r.betrayals.saturating_add(1);
            r.rep = r.rep.saturating_sub(BREAK_COST).max(-100);
            r.pact_ban_until = day + COOLING_DAYS;
        }
        let other = p.other(clan);
        let with = self.clans[usize::from(clan)].name.clone();
        // The injured party is told even when their post is full: a betrayal must be seen.
        self.social.deliver(
            other,
            super::Body::Pact(Notice {
                kind: NoticeKind::Broken,
                pact,
                with,
                terms: p.terms,
                template: String::new(),
                line: None,
            }),
        );
        self.social.note(day, DeedKind::PactBroken, clan, other);
        Ok(())
    }

    /// The nightly pass: an offer nobody answered in a week lapses (so it cannot hold a pair,
    /// or its proposer's offers, shut for the season), and a week of keeping a pact earns both
    /// parties standing.
    pub fn pacts_tick(&mut self, day: i64) {
        self.social
            .pacts
            .retain(|p| !(p.state == State::Proposed && day - p.since >= OFFER_DAYS));
        let kept: Vec<(ClanIx, ClanIx)> = self
            .social
            .pacts
            .iter()
            .filter(|p| p.state == State::Active && day > p.since && (day - p.since) % 7 == 0)
            .map(|p| (p.a, p.b))
            .collect();
        for (a, b) in kept {
            for c in [a, b] {
                if let Some(e) = self.clans.get_mut(usize::from(c)) {
                    e.record.rep = e.record.rep.saturating_add(2).min(100);
                }
                // A kept pact pays: the Tidecourt's peace dividend.
                if self.social.mail_for(c) < super::caps::MAIL_PER_CLAN {
                    self.social.deliver(
                        c,
                        super::Body::Parcel(super::market::Parcel {
                            goods: None,
                            tallies: super::caps::PEACE_DIVIDEND,
                            memo: super::market::Memo::Dividend,
                            about: None,
                        }),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::assert_is_empty)]
    use super::*;

    fn world() -> World {
        let mut w = World::new(1, 7, 100);
        for n in ["Ash", "Brindle", "Cove", "Dun"] {
            w.register(n, 0);
        }
        for c in &mut w.clans {
            c.posted.buildings = 4;
            c.posted.rating = 3;
        }
        w
    }

    #[test]
    fn a_pact_is_offered_accepted_and_both_sides_are_told() {
        let mut w = world();
        let id = w.propose(0, 1, TRUCE | TRADE, 100).unwrap();
        assert_eq!(w.pact_between(0, 1), None, "not in force until accepted");
        assert_eq!(
            w.accept(0, id, 101),
            Err(Refusal::NotYours),
            "only the target"
        );
        w.accept(1, id, 101).unwrap();
        assert!(w.pact_between(1, 0).is_some() && w.has_trade_pact(0, 1));
        let kinds: Vec<(ClanIx, NoticeKind)> = w
            .social
            .mail
            .iter()
            .filter_map(|d| match &d.body {
                super::super::Body::Pact(n) => Some((d.to, n.kind)),
                _ => None,
            })
            .collect();
        assert_eq!(kinds, [(1, NoticeKind::Offered), (0, NoticeKind::Accepted)]);
        assert_eq!(w.propose(0, 1, TRUCE, 101), Err(Refusal::HasPact));
    }

    #[test]
    fn a_declined_offer_leaves_no_trace_and_can_be_made_again() {
        let mut w = world();
        let id = w.propose(0, 1, TRUCE, 100).unwrap();
        w.decline(1, id).unwrap();
        assert!(w.social.pacts.is_empty());
        w.propose(0, 1, TRUCE, 100).unwrap();
    }

    #[test]
    fn breaking_a_pact_is_public_costly_and_remembered() {
        let mut w = world();
        let id = w.propose(0, 1, TRUCE, 100).unwrap();
        w.accept(1, id, 100).unwrap();
        w.break_pact(0, id, 103).unwrap();
        let me = &w.clans[0].record;
        assert_eq!((me.betrayals, me.rep), (1, -BREAK_COST));
        assert_eq!(me.pact_ban_until, 103 + COOLING_DAYS);
        assert!(w.social.feed.iter().any(|d| d.kind == DeedKind::PactBroken));
        // The two may never sign again this season.
        assert_eq!(w.propose(1, 0, TRUCE, 200), Err(Refusal::HasPact));
        // The breaker cannot offer anyone anything until the cooling-off passes.
        assert_eq!(w.propose(0, 2, TRUCE, 104), Err(Refusal::Terms));
        w.propose(0, 2, TRUCE, 103 + COOLING_DAYS).unwrap();
        // The injured party was told.
        assert!(w.social.mail.iter().any(|d| d.to == 1
            && matches!(&d.body, super::super::Body::Pact(n) if n.kind == NoticeKind::Broken)));
    }

    #[test]
    fn a_betrayal_closes_trade_and_defence_pacts_with_everyone_but_not_the_truce() {
        let mut w = world();
        let id = w.propose(0, 1, TRUCE, 100).unwrap();
        w.accept(1, id, 100).unwrap();
        w.break_pact(0, id, 100).unwrap();
        let later = 100 + COOLING_DAYS;
        // The betrayer is closed out of trade and defence...
        assert_eq!(w.may_offer(0, 2, TRUCE | TRADE, later), Err(Refusal::Terms));
        assert_eq!(
            w.may_offer(0, 2, TRUCE | DEFENCE, later),
            Err(Refusal::Terms)
        );
        assert_eq!(w.may_offer(0, 2, TRUCE, later), Ok(()));
        // ...and so is anyone who would sign a trade pact with them.
        assert_eq!(w.may_offer(2, 0, TRUCE | TRADE, later), Err(Refusal::Terms));
        assert_eq!(w.may_offer(2, 3, TRUCE | TRADE, later), Ok(()));
        // The accepting side is held to the same rule.
        let id = w.propose(2, 3, TRUCE | TRADE, later).unwrap();
        assert!(w.accept(3, id, later).is_ok());
    }

    #[test]
    fn what_a_clan_has_built_opens_and_closes_terms() {
        let mut w = world();
        w.clans[0].posted.buildings = 2;
        assert_eq!(w.may_offer(0, 1, TRUCE | TRADE, 100), Err(Refusal::Terms));
        w.clans[0].posted.buildings = 3;
        assert_eq!(w.may_offer(0, 1, TRUCE | TRADE, 100), Ok(()));
        w.clans[0].posted.rating = 0;
        assert_eq!(w.may_offer(0, 1, TRUCE | DEFENCE, 100), Err(Refusal::Terms));
        assert_eq!(
            w.may_offer(0, 1, TRUCE | CAUSEWAY, 100),
            Err(Refusal::Terms)
        );
        w.clans[0].posted.causeways = 1;
        assert_eq!(w.may_offer(0, 1, TRUCE | CAUSEWAY, 100), Ok(()));
        assert_eq!(
            w.may_offer(0, 1, TRADE, 100),
            Err(Refusal::Terms),
            "a truce is always part"
        );
    }

    #[test]
    fn a_closed_term_says_why_in_plain_words() {
        let mut w = world();
        assert_eq!(w.closed_reason(0, 1, TRADE, 100), None);
        w.clans[0].posted.buildings = 1;
        assert_eq!(
            w.closed_reason(0, 1, TRADE, 100),
            Some("needs three buildings finished")
        );
        w.clans[0].posted.rating = 0;
        assert_eq!(
            w.closed_reason(0, 1, DEFENCE, 100),
            Some("needs a defence building")
        );
        assert_eq!(
            w.closed_reason(0, 1, CAUSEWAY, 100),
            Some("needs a causeway laid")
        );
        w.clans[1].record.betrayals = 1;
        w.clans[0].posted.buildings = 5;
        assert_eq!(
            w.closed_reason(0, 1, TRADE, 100),
            Some("a pact was broken: merchants will not deal")
        );
        w.clans[0].record.rep = OUTLAW;
        assert_eq!(
            w.closed_reason(0, 1, TRUCE, 100),
            Some(Refusal::Outlaw.text())
        );
    }

    #[test]
    fn a_recent_raid_between_two_clans_keeps_them_from_signing() {
        let mut w = world();
        w.social.raid_log.push(super::super::raid::LogEntry {
            from: 1,
            to: 0,
            day: 98,
        });
        assert_eq!(w.may_offer(0, 1, TRUCE, 100), Err(Refusal::Terms));
        assert_eq!(w.may_offer(0, 1, TRUCE, 98 + GRUDGE_DAYS), Ok(()));
    }

    #[test]
    fn offers_and_pacts_are_capped() {
        let mut w = world();
        for n in ["Eel", "Fen", "Gull"] {
            w.register(n, 0);
        }
        for c in &mut w.clans {
            c.posted.buildings = 4;
        }
        for t in 1..=u8::try_from(caps::OPEN_OFFERS).unwrap() {
            w.propose(0, t, TRUCE, 100).unwrap();
        }
        assert_eq!(w.propose(0, 5, TRUCE, 100), Err(Refusal::TooMany));
    }

    #[test]
    fn an_offer_nobody_answers_lapses_after_a_week_and_frees_the_pair_and_the_proposer() {
        let mut w = world();
        let id = w.propose(0, 1, TRUCE, 100).unwrap();
        let kept = w.propose(0, 2, TRUCE, 100).unwrap();
        w.accept(2, kept, 100).unwrap();
        w.pacts_tick(100 + OFFER_DAYS - 1);
        assert!(w.social.pacts.iter().any(|p| p.id == id), "not yet");
        assert_eq!(w.propose(0, 1, TRUCE, 105), Err(Refusal::HasPact));
        w.pacts_tick(100 + OFFER_DAYS);
        assert!(!w.social.pacts.iter().any(|p| p.id == id), "lapsed");
        assert!(
            w.social.pacts.iter().any(|p| p.id == kept),
            "a pact in force is not an offer"
        );
        assert!(
            w.propose(0, 1, TRUCE, 108).is_ok(),
            "the pair is free again"
        );
    }

    #[test]
    fn a_week_of_keeping_a_pact_earns_standing() {
        let mut w = world();
        let id = w.propose(0, 1, TRUCE, 100).unwrap();
        w.accept(1, id, 100).unwrap();
        w.pacts_tick(106);
        assert_eq!(w.clans[0].record.rep, 0);
        w.pacts_tick(107);
        assert_eq!((w.clans[0].record.rep, w.clans[1].record.rep), (2, 2));
        // An outlaw can offer nothing.
        w.clans[2].record.rep = OUTLAW;
        assert_eq!(w.may_offer(2, 3, TRUCE, 100), Err(Refusal::Outlaw));
    }
}
