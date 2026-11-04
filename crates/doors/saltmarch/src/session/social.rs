//! The social orders of the menu: the exchange, the gate (raids), pacts and the post. Like the
//! rest of the menu, every order is an [`Entry`] with a tag the simulation bots read; what each
//! does is in `social/` (the rules) and here (asking the world for it, through
//! [`dispatch`](crate::social::dispatch)).
//!
//! **Nothing here settles at once.** A session cannot see another player's save, and the world
//! it opened with is a snapshot. So an order that crosses the exchange, a letter, a pact or a
//! raid is sent to the world as a dispatch; the clan keeps what it risked as a pending, and
//! the answer arrives (and is folded into the save) the next time the door is opened. The
//! screens say so.

use russh_bbs_door::{Ctx, DoorStyle, TextRefused};

use super::{Act, Entry, MenuId, Mode, Session, entry};
use crate::clan::Clan;
use crate::social::dispatch::{self, Act as Send, Dispatch};
use crate::social::ledger::{self, Kind as PendKind, Pending};
use crate::social::letters::{Kind as LetterKind, LetterDef};
use crate::social::market::{self, Side};
use crate::social::pact::{self, CAUSEWAY, DEFENCE, TRADE, TRUCE};
use crate::social::raid::{self, Window};
use crate::social::{ClanIx, Defender, Refusal, caps};
use crate::types::Resource;
use crate::world::ClanEntry;

/// The raid being planned.
#[derive(Debug, Clone)]
pub(super) struct RaidPlan {
    /// Index into [`Session::others`].
    pub target: usize,
    /// How many raiders (the strongest hands in the hold).
    pub size: u8,
    /// Index into the landing day's windows.
    pub window: u8,
    /// Index into the open taunts (0: none).
    pub taunt: usize,
}

impl Default for RaidPlan {
    fn default() -> Self {
        Self {
            target: 0,
            size: 2,
            window: 0,
            taunt: 0,
        }
    }
}

/// The pact being drawn up.
#[derive(Debug, Clone, Default)]
pub(super) struct PactPlan {
    /// Index into [`Session::others`].
    pub target: usize,
    /// Terms beyond the truce that are switched on.
    pub terms: u8,
    /// A break waiting for the second press.
    pub sure: Option<u32>,
}

/// The letter being written.
#[derive(Debug, Clone, Default)]
pub(super) struct Compose {
    /// Index into [`Session::others`].
    pub to: usize,
    /// Index into [`Session::templates`].
    pub template: usize,
    /// The sender's own line, already through the board's text rules.
    pub line: Option<String>,
}

/// The social screens' state.
#[derive(Debug, Clone, Default)]
pub(super) struct SocialUi {
    /// The good the exchange screen is showing (index into `Resource::ALL`).
    pub exch: usize,
    pub raid: RaidPlan,
    pub pact: PactPlan,
    pub compose: Compose,
    /// The letter on the inbox's highlight.
    pub letter: usize,
}

/// What a social order does.
#[derive(Debug, Clone)]
pub(super) enum Soc {
    Good(i8),
    Post { side: Side, qty: u32, price: u16 },
    Cancel(u32),
    RaidTarget(i8),
    RaidSize(i8),
    RaidWindow(i8),
    RaidTaunt(i8),
    RaidSend,
    PactTarget(i8),
    PactTerm(u8),
    PactOffer,
    PactBreak(u32),
    PactAnswer { pact: u32, accept: bool },
    ComposeTo(i8),
    ComposeTemplate(i8),
    ComposeLine,
    ComposeSend,
    Reply(String),
    Block(String),
    Discard,
}

fn step(i: usize, by: i8, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let to =
        (i32::try_from(i).unwrap_or(0) + i32::from(by)).rem_euclid(i32::try_from(len).unwrap_or(1));
    usize::try_from(to).unwrap_or(0)
}

fn soc(tag: impl Into<String>, text: impl Into<String>, s: Soc) -> Entry {
    entry(tag, text, Act::Soc(s))
}

impl Session {
    /// This clan's place in the snapshot of the world.
    pub(super) fn me(&self) -> Option<ClanIx> {
        self.world.ix(&self.clan.as_ref()?.name)
    }

    /// The other clans on the rolls, the most renowned first.
    pub(super) fn others(&self) -> Vec<(ClanIx, &ClanEntry)> {
        let me = self.me();
        let mut out: Vec<(ClanIx, &ClanEntry)> = self
            .world
            .clans
            .iter()
            .enumerate()
            .filter_map(|(i, c)| Some((ClanIx::try_from(i).ok()?, c)))
            .filter(|(i, _)| Some(*i) != me)
            .collect();
        out.sort_by(|a, b| {
            b.1.score
                .cmp(&a.1.score)
                .then_with(|| a.1.name.cmp(&b.1.name))
        });
        out
    }

    /// The letter templates this clan may send as a letter (offers go with a pact offer).
    pub(super) fn templates(&self) -> Vec<&LetterDef> {
        let me = self.me();
        self.cat
            .letters
            .entries
            .iter()
            .filter(|l| l.kind != LetterKind::Offer)
            .filter(|l| me.is_some_and(|m| self.world.opens(m, l.requires)))
            .collect()
    }

    /// Taunts this clan may leave with a raid.
    pub(super) fn taunts(&self) -> Vec<&LetterDef> {
        let me = self.me();
        self.cat
            .letters
            .entries
            .iter()
            .filter(|l| l.kind == LetterKind::Taunt)
            .filter(|l| me.is_some_and(|m| self.world.opens(m, l.requires)))
            .collect()
    }

    /// The landing day's windows of the water.
    pub(super) fn windows(&self) -> Vec<Window> {
        raid::windows(&self.tide, self.today + 1)
    }

    /// The raiding party for the plan: the strongest hands in the hold.
    pub(super) fn raiders(&self) -> Vec<Defender> {
        let Some(clan) = &self.clan else {
            return Vec::new();
        };
        let mut all = ledger::available(&self.cat, clan);
        all.truncate(usize::from(self.soc.raid.size).min(caps::MAX_RAIDERS));
        all
    }

    /// The Tidecourt's price of a good, in whole tallies.
    pub(super) fn tide_price(&self, r: Resource) -> u16 {
        (self.world.prices[r.index()] / 100).max(1)
    }

    /// The exchange screen's good.
    pub(super) fn good(&self) -> Resource {
        Resource::ALL[self.soc.exch % Resource::ALL.len()]
    }

    pub(super) fn social_entries(&self, menu: MenuId, clan: &Clan) -> Vec<Entry> {
        match menu {
            MenuId::Exchange => self.exchange_entries(clan),
            MenuId::Raid => self.raid_entries(clan),
            MenuId::Pacts => self.pact_entries(clan),
            MenuId::Letters => self.letter_entries(clan),
            MenuId::Compose => self.compose_entries(),
            _ => Vec::new(),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn exchange_entries(&self, clan: &Clan) -> Vec<Entry> {
        let mut out = vec![
            soc(
                "good:next",
                format!("Next good (now {})", self.good().name()),
                Soc::Good(1),
            ),
            soc("good:prev", "Previous good", Soc::Good(-1)),
        ];
        let what = self.good();
        let me = self.me();
        let x = &self.world.social.exchange;
        let have = clan.stock.get(what);
        let base = self.tide_price(what);
        let fair = u16::try_from(u32::from(base) * 105 / 100)
            .unwrap_or(base)
            .max(1);
        let high = u16::try_from(u32::from(base) * 112 / 100)
            .unwrap_or(base)
            .max(fair + 1);
        let low = u16::try_from(u32::from(base) * 92 / 100)
            .unwrap_or(base)
            .max(1);
        if let Some(a) = x.best_ask(what, me) {
            let n = a.qty.min(5);
            out.push(soc(
                format!("xbuy:{}:{n}", what.id()),
                format!("Buy {n} {} at the best ask, {} each", what.name(), a.price),
                Soc::Post {
                    side: Side::Bid,
                    qty: n,
                    price: a.price,
                },
            ));
        }
        if let Some(b) = x.best_bid(what, me)
            && have > 0
        {
            let n = b.qty.min(have).min(5);
            out.push(soc(
                format!("xsell:{}:{n}", what.id()),
                format!("Sell {n} {} to the best bid, {} each", what.name(), b.price),
                Soc::Post {
                    side: Side::Ask,
                    qty: n,
                    price: b.price,
                },
            ));
        }
        if have > 0 {
            let n = have.min(5);
            out.push(soc(
                format!("ask:{}:{n}@{fair}", what.id()),
                format!("Post an ask: {n} {} at {fair} each", what.name()),
                Soc::Post {
                    side: Side::Ask,
                    qty: n,
                    price: fair,
                },
            ));
            out.push(soc(
                format!("ask:{}:{n}@{high}", what.id()),
                format!("Post an ask: {n} {} at {high} each (patient)", what.name()),
                Soc::Post {
                    side: Side::Ask,
                    qty: n,
                    price: high,
                },
            ));
            if have > 5 {
                let all = have.min(caps::ORDER_QTY);
                out.push(soc(
                    format!("ask:{}:{all}@{fair}", what.id()),
                    format!("Post an ask: all {all} {} at {fair} each", what.name()),
                    Soc::Post {
                        side: Side::Ask,
                        qty: all,
                        price: fair,
                    },
                ));
            }
        }
        out.push(soc(
            format!("bid:{}:5@{base}", what.id()),
            format!("Post a bid: 5 {} at {base} each", what.name()),
            Soc::Post {
                side: Side::Bid,
                qty: 5,
                price: base,
            },
        ));
        out.push(soc(
            format!("bid:{}:5@{low}", what.id()),
            format!("Post a bid: 5 {} at {low} each (patient)", what.name()),
            Soc::Post {
                side: Side::Bid,
                qty: 5,
                price: low,
            },
        ));
        if let Some(m) = me {
            for o in x.asks.iter().chain(&x.bids).filter(|o| o.owner == m) {
                out.push(soc(
                    format!("cancel:{}", o.id),
                    format!(
                        "Take down your {} of {} {} at {}",
                        if x.asks.iter().any(|a| a.id == o.id) {
                            "ask"
                        } else {
                            "bid"
                        },
                        o.qty,
                        o.what.name(),
                        o.price
                    ),
                    Soc::Cancel(o.id),
                ));
            }
        }
        out
    }

    /// Why a raid on `to` is closed today, if it is.
    pub(super) fn raid_closed(&self, to: ClanIx) -> Option<Refusal> {
        let me = self.me()?;
        // Raids already sent from this session (or since the snapshot) are not in the snapshot
        // of the world yet: the clan's own pending list knows them.
        if let Some(clan) = &self.clan {
            let name = self
                .world
                .clans
                .get(usize::from(to))
                .map(|c| c.name.as_str());
            let sent: Vec<&Pending> = clan
                .social
                .pending
                .iter()
                .filter(|p| p.kind == PendKind::Raid)
                .collect();
            if sent.iter().any(|p| Some(p.about.as_str()) == name) {
                return Some(Refusal::PairCooldown);
            }
            if sent.iter().filter(|p| p.day == self.today).count()
                >= usize::from(caps::RAIDS_PER_DAY)
            {
                return Some(Refusal::RaidsToday);
            }
        }
        raid::check(&self.world, me, to, self.today).err()
    }

    fn raid_entries(&self, clan: &Clan) -> Vec<Entry> {
        let others = self.others();
        let mut out = Vec::new();
        if others.is_empty() {
            return out;
        }
        let plan = &self.soc.raid;
        let t = plan.target % others.len();
        out.push(soc(
            "target:next",
            format!("Next target (now {})", others[t].1.name),
            Soc::RaidTarget(1),
        ));
        out.push(soc("target:prev", "Previous target", Soc::RaidTarget(-1)));
        out.push(soc(
            "party:more",
            format!("More raiders (now {})", plan.size),
            Soc::RaidSize(1),
        ));
        out.push(soc("party:fewer", "Fewer raiders", Soc::RaidSize(-1)));
        let windows = self.windows();
        if !windows.is_empty() {
            out.push(soc(
                "window:next",
                "Next turn of the water",
                Soc::RaidWindow(1),
            ));
        }
        if !self.taunts().is_empty() {
            out.push(soc(
                "taunt:next",
                "Next taunt to leave behind",
                Soc::RaidTaunt(1),
            ));
        }
        let ready = self.clan.as_ref().is_some_and(|c| c.trip.is_none())
            && self.raid_closed(others[t].0).is_none();
        if ready {
            out.push(soc(
                format!("raid:send:{}", others[t].0),
                format!(
                    "Send the raid on {} ({} turns)",
                    others[t].1.name,
                    caps::RAID_TURNS
                ),
                Soc::RaidSend,
            ));
        }
        let _ = clan;
        out
    }

    fn pact_entries(&self, clan: &Clan) -> Vec<Entry> {
        let mut out = Vec::new();
        let me = self.me();
        let others = self.others();
        if !others.is_empty() {
            let t = self.soc.pact.target % others.len();
            out.push(soc(
                "ptarget:next",
                format!("Next clan (now {})", others[t].1.name),
                Soc::PactTarget(1),
            ));
            out.push(soc("ptarget:prev", "Previous clan", Soc::PactTarget(-1)));
            for term in [TRADE, DEFENCE, CAUSEWAY] {
                let on = self.soc.pact.terms & term != 0;
                // A closed term is shown on the screen with its reason, and has no order.
                let closed = me.is_some_and(|m| {
                    self.world
                        .closed_reason(m, others[t].0, term, self.today)
                        .is_some()
                });
                if closed && !on {
                    continue;
                }
                out.push(soc(
                    format!("term:{}", pact::term_name(term).replace(' ', "-")),
                    format!(
                        "{} {}: {}",
                        if on { "Drop" } else { "Add" },
                        pact::term_name(term),
                        pact::term_blurb(term)
                    ),
                    Soc::PactTerm(term),
                ));
            }
            let terms = TRUCE | self.soc.pact.terms;
            if me.is_some_and(|m| {
                self.world
                    .may_offer(m, others[t].0, terms, self.today)
                    .is_ok()
            }) {
                out.push(soc(
                    format!("offer:{}", others[t].0),
                    format!("Offer {} a pact", others[t].1.name),
                    Soc::PactOffer,
                ));
            }
        }
        if let Some(m) = me {
            for p in self.world.pacts_of(m) {
                let other = p.other(m);
                let name = self
                    .world
                    .clans
                    .get(usize::from(other))
                    .map_or("?", |c| c.name.as_str());
                let sure = self.soc.pact.sure == Some(p.id);
                out.push(soc(
                    if sure {
                        format!("break:{}:sure", p.id)
                    } else {
                        format!("break:{}", p.id)
                    },
                    if sure {
                        format!(
                            "Press again to break faith with {name}: -{} standing, trade and defence closed to you all season",
                            pact::BREAK_COST
                        )
                    } else {
                        format!("Break the pact with {name}")
                    },
                    Soc::PactBreak(p.id),
                ));
            }
            for p in self
                .world
                .social
                .pacts
                .iter()
                .filter(|p| p.state == pact::State::Proposed && p.has(m))
            {
                let other = self
                    .world
                    .clans
                    .get(usize::from(p.other(m)))
                    .map_or("?", |c| c.name.as_str());
                if p.a == m {
                    out.push(soc(
                        format!("withdraw:{}", p.id),
                        format!("Withdraw your offer to {other}"),
                        Soc::PactAnswer {
                            pact: p.id,
                            accept: false,
                        },
                    ));
                }
            }
        }
        let _ = clan;
        out
    }

    fn letter_entries(&self, clan: &Clan) -> Vec<Entry> {
        let mut out = vec![soc("compose", "Write a letter", Soc::Reply(String::new()))];
        let Some(l) = clan.social.inbox.get(self.soc.letter) else {
            return out;
        };
        if let Some((pact, terms)) = l.offer {
            let live = self
                .world
                .social
                .pacts
                .iter()
                .any(|p| p.id == pact && p.state == pact::State::Proposed);
            if live {
                out.push(soc(
                    format!("accept:{pact}"),
                    format!("Accept the pact ({})", crate::event::terms_phrase(terms)),
                    Soc::PactAnswer { pact, accept: true },
                ));
                out.push(soc(
                    format!("decline:{pact}"),
                    format!("Decline the pact from {}", l.from),
                    Soc::PactAnswer {
                        pact,
                        accept: false,
                    },
                ));
            }
        }
        out.push(soc(
            "reply",
            format!("Reply to {}", l.from),
            Soc::Reply(l.from.clone()),
        ));
        out.push(soc(
            "block",
            format!("Block letters from {}", l.from),
            Soc::Block(l.from.clone()),
        ));
        out.push(soc("discard", "Throw this letter away", Soc::Discard));
        out
    }

    fn compose_entries(&self) -> Vec<Entry> {
        let others = self.others();
        let mut out = Vec::new();
        if others.is_empty() {
            return out;
        }
        let c = &self.soc.compose;
        let to = c.to % others.len();
        out.push(soc(
            "to:next",
            format!("Next recipient (now {})", others[to].1.name),
            Soc::ComposeTo(1),
        ));
        out.push(soc("to:prev", "Previous recipient", Soc::ComposeTo(-1)));
        let templates = self.templates();
        if !templates.is_empty() {
            out.push(soc("template:next", "Next words", Soc::ComposeTemplate(1)));
        }
        out.push(soc(
            "line",
            if c.line.is_some() {
                "Change your own line"
            } else {
                "Add a line of your own"
            },
            Soc::ComposeLine,
        ));
        if !templates.is_empty() {
            out.push(soc(
                format!("send:{}", others[to].0),
                format!("Send the letter to {}", others[to].1.name),
                Soc::ComposeSend,
            ));
        }
        out
    }

    /// Plain lines for the screens the social orders live on (also the look-around's body).
    #[allow(clippy::too_many_lines)]
    pub(super) fn social_lines(&self, menu: MenuId, clan: &Clan) -> Vec<(String, DoorStyle)> {
        let mut out: Vec<(String, DoorStyle)> = Vec::new();
        let plain = |s: String| (s, DoorStyle::Plain);
        match menu {
            MenuId::Exchange => {
                let what = self.good();
                let x = &self.world.social.exchange;
                out.push(plain(format!(
                    "The exchange: {}. The Tidecourt pays {}, sells at {}.",
                    what.name(),
                    World::sell_value(&self.world.prices, what, 1),
                    World::buy_cost(&self.world.prices, what, 1)
                )));
                let asks: Vec<String> = x
                    .asks
                    .iter()
                    .filter(|o| o.what == what)
                    .map(|o| format!("{}@{}", o.qty, o.price))
                    .collect();
                let bids: Vec<String> = x
                    .bids
                    .iter()
                    .filter(|o| o.what == what)
                    .map(|o| format!("{}@{}", o.qty, o.price))
                    .collect();
                out.push(plain(format!(
                    "Asks: {}. Bids: {}.",
                    if asks.is_empty() {
                        "none".into()
                    } else {
                        asks.join(" ")
                    },
                    if bids.is_empty() {
                        "none".into()
                    } else {
                        bids.join(" ")
                    }
                )));
                if !clan.social.pending.is_empty() {
                    out.push(plain(format!(
                        "{} dispatch{} awaiting the Tidecourt's answer.",
                        clan.social.pending.len(),
                        if clan.social.pending.len() == 1 {
                            ""
                        } else {
                            "es"
                        }
                    )));
                }
            }
            MenuId::Raid => out.extend(self.raid_lines(clan)),
            MenuId::Pacts => {
                if let Some(m) = self.me() {
                    for p in self.world.pacts_of(m) {
                        let other = self
                            .world
                            .clans
                            .get(usize::from(p.other(m)))
                            .map_or("?", |c| c.name.as_str());
                        out.push(plain(format!(
                            "Pact with {other}: {} ({} days)",
                            crate::event::terms_phrase(p.terms),
                            (self.today - p.since).max(0)
                        )));
                    }
                    let rec = self.world.clans.get(usize::from(m)).map(|c| &c.record);
                    if let Some(r) = rec {
                        out.push(plain(format!(
                            "Standing with the Tidecourt: {}. Betrayals on record: {}.",
                            r.rep, r.betrayals
                        )));
                        if r.pact_ban_until > self.today {
                            out.push((
                                format!(
                                    "Cooling off after a broken pact: no offers until day {}.",
                                    r.pact_ban_until - clan.founded + 1
                                ),
                                DoorStyle::Bad,
                            ));
                        }
                    }
                    let others = self.others();
                    if let Some((ix, t)) = others.get(self.soc.pact.target % others.len().max(1)) {
                        out.push(plain(format!("Terms with {}:", t.name)));
                        for term in [TRADE, DEFENCE, CAUSEWAY] {
                            out.push(plain(
                                match self.world.closed_reason(m, *ix, term, self.today) {
                                    None => format!("  {} is open.", pact::term_name(term)),
                                    Some(why) => format!(
                                        "  {} is closed: {}.",
                                        pact::term_name(term),
                                        why.trim_end_matches('.')
                                    ),
                                },
                            ));
                        }
                    }
                }
            }
            MenuId::Letters => {
                if clan.social.inbox.is_empty() {
                    out.push(plain("No letters.".to_owned()));
                }
                if let Some(l) = clan.social.inbox.get(self.soc.letter) {
                    let words = self
                        .cat
                        .letters
                        .entries
                        .iter()
                        .find(|d| d.id == l.template)
                        .map_or_else(String::new, |d| d.words(&l.from, &clan.name));
                    out.push(plain(format!("Reading: {words}")));
                    if let Some(own) = &l.line {
                        out.push(plain(format!("{} says: {own}", l.from)));
                    }
                }
                for (i, l) in clan.social.inbox.iter().enumerate() {
                    out.push(plain(format!(
                        "{}{} from {}{}",
                        if i == self.soc.letter { "> " } else { "  " },
                        if l.read { "" } else { "(new) " },
                        l.from,
                        if l.offer.is_some() {
                            " - a pact offer"
                        } else {
                            ""
                        }
                    )));
                }
            }
            MenuId::Compose => {
                let others = self.others();
                if let Some((_, to)) = others.get(self.soc.compose.to % others.len().max(1)) {
                    out.push(plain(format!("To: {}", to.name)));
                }
                if let Some(t) = self
                    .templates()
                    .get(self.soc.compose.template % self.templates().len().max(1))
                {
                    out.push(plain(t.words(&clan.name, "the reader")));
                }
                if let Some(l) = &self.soc.compose.line {
                    out.push(plain(format!("Your line: {l}")));
                }
            }
            _ => {}
        }
        out
    }

    fn raid_lines(&self, clan: &Clan) -> Vec<(String, DoorStyle)> {
        let others = self.others();
        let mut out = Vec::new();
        let Some((ix, target)) = others.get(self.soc.raid.target % others.len().max(1)) else {
            out.push((
                "No other clan is on the Tidecourt's rolls yet.".to_owned(),
                DoorStyle::Dim,
            ));
            return out;
        };
        out.push((
            format!(
                "Target: {} (renown {}), guard {} at the gate, rating {}{}",
                target.name,
                target.score,
                target.posted.guard.len(),
                target.posted.rating,
                target
                    .posted
                    .sluice
                    .map_or(String::new(), |s| format!(", sluice at {s}"))
            ),
            DoorStyle::Plain,
        ));
        match self.raid_closed(*ix) {
            Some(why) => out.push((why.text().to_owned(), DoorStyle::Bad)),
            None => out.push(("The way is open.".to_owned(), DoorStyle::Good)),
        }
        let party = self.raiders();
        out.push((
            format!(
                "Raiders: {}",
                if party.is_empty() {
                    "nobody is fit".to_owned()
                } else {
                    party
                        .iter()
                        .map(|d| d.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ),
            DoorStyle::Plain,
        ));
        let windows = self.windows();
        if let Some(w) = windows.get(usize::from(self.soc.raid.window) % windows.len().max(1)) {
            out.push((
                format!(
                    "Landing at {:02}:{:02}, {} water {}{}",
                    w.minute / 60,
                    w.minute % 60,
                    if w.high { "high" } else { "low" },
                    w.level,
                    if w.needs_boat { " (needs a boat)" } else { "" }
                ),
                DoorStyle::Plain,
            ));
        }
        let _ = clan;
        out
    }

    // ------------------------------------------------------------------ acting

    /// Tell the world, and keep what is at risk until its receipt comes.
    fn send(&mut self, ctx: &mut Ctx<'_>, kind: PendKind, about: String, hold: Pending, act: Send) {
        let Some(clan) = self.clan.as_mut() else {
            return;
        };
        let id = clan.social.begin(Pending {
            kind,
            about,
            day: self.today,
            ..hold
        });
        let d = Dispatch {
            from: clan.name.clone(),
            id,
            day: self.today,
            act,
        };
        let cat = std::sync::Arc::clone(&self.cat);
        let season = self.world.season;
        ctx.edit_world(move |old| dispatch::run(old, season, &d, &cat));
    }

    fn note(&mut self, text: impl Into<String>, style: DoorStyle) {
        self.notes = vec![(text.into(), style)];
    }

    fn blank_on(day: i64) -> Pending {
        Pending {
            id: 0,
            day,
            kind: PendKind::Letter,
            about: String::new(),
            goods: None,
            tallies: 0,
            turns: 0,
            crew: Vec::new(),
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn soc_act(&mut self, s: Soc, ctx: &mut Ctx<'_>) {
        let paging = matches!(
            s,
            Soc::Good(_)
                | Soc::RaidTarget(_)
                | Soc::RaidSize(_)
                | Soc::RaidWindow(_)
                | Soc::RaidTaunt(_)
                | Soc::PactTarget(_)
                | Soc::PactTerm(_)
                | Soc::ComposeTo(_)
                | Soc::ComposeTemplate(_)
                | Soc::ComposeLine
        );
        let others = self.others();
        let names: Vec<(ClanIx, String)> =
            others.iter().map(|(i, c)| (*i, c.name.clone())).collect();
        drop(others);
        match s {
            Soc::Good(by) => self.soc.exch = step(self.soc.exch, by, Resource::ALL.len()),
            Soc::RaidTarget(by) => {
                self.soc.raid.target = step(self.soc.raid.target, by, names.len());
            }
            Soc::RaidSize(by) => {
                let size = i16::from(self.soc.raid.size) + i16::from(by);
                self.soc.raid.size = u8::try_from(size.clamp(1, 4)).unwrap_or(2);
            }
            Soc::RaidWindow(by) => {
                self.soc.raid.window = u8::try_from(step(
                    usize::from(self.soc.raid.window),
                    by,
                    self.windows().len(),
                ))
                .unwrap_or(0);
            }
            Soc::RaidTaunt(by) => {
                self.soc.raid.taunt = step(self.soc.raid.taunt, by, self.taunts().len() + 1);
            }
            Soc::PactTarget(by) => {
                self.soc.pact.target = step(self.soc.pact.target, by, names.len());
                self.soc.pact.terms = 0;
                self.soc.pact.sure = None;
            }
            Soc::PactTerm(t) => self.soc.pact.terms ^= t,
            Soc::ComposeTo(by) => self.soc.compose.to = step(self.soc.compose.to, by, names.len()),
            Soc::ComposeTemplate(by) => {
                self.soc.compose.template =
                    step(self.soc.compose.template, by, self.templates().len());
            }
            Soc::ComposeLine => {
                self.mode = Mode::Line(self.soc.compose.line.clone().unwrap_or_default());
            }
            Soc::Post { side, qty, price } => self.post_order(ctx, side, qty, price),
            Soc::Cancel(order) => {
                self.send(
                    ctx,
                    PendKind::Cancel,
                    format!("order {order}"),
                    Self::blank_on(self.today),
                    Send::Cancel { order },
                );
                self.note("Asked the exchange to take your order down. The goods come back with the answer.", DoorStyle::Plain);
            }
            Soc::RaidSend => self.send_raid(ctx, &names),
            Soc::PactOffer => {
                let Some((_, to)) = names
                    .get(self.soc.pact.target % names.len().max(1))
                    .cloned()
                else {
                    return;
                };
                let terms = TRUCE | self.soc.pact.terms;
                let template = if terms & TRADE != 0 {
                    "offer-trade"
                } else if terms & DEFENCE != 0 {
                    "offer-guard"
                } else {
                    "offer-truce"
                };
                self.send(
                    ctx,
                    PendKind::Offer,
                    to.clone(),
                    Self::blank_on(self.today),
                    Send::Offer {
                        to: to.clone(),
                        terms,
                        template: template.to_owned(),
                        line: None,
                    },
                );
                self.note(
                    format!("Your offer goes to {to}. Their answer comes with the post."),
                    DoorStyle::Good,
                );
            }
            Soc::PactBreak(id) => {
                if self.soc.pact.sure == Some(id) {
                    self.soc.pact.sure = None;
                    self.send(
                        ctx,
                        PendKind::Break,
                        format!("pact {id}"),
                        Self::blank_on(self.today),
                        Send::Break { pact: id },
                    );
                    self.note(
                        "You break faith. The Tidecourt will hear of it.",
                        DoorStyle::Bad,
                    );
                } else {
                    self.soc.pact.sure = Some(id);
                }
            }
            Soc::PactAnswer { pact, accept } => {
                let kind = if accept {
                    PendKind::Accept
                } else {
                    PendKind::Decline
                };
                let act = if accept {
                    Send::Accept { pact }
                } else {
                    Send::Decline { pact }
                };
                self.send(
                    ctx,
                    kind,
                    format!("pact {pact}"),
                    Self::blank_on(self.today),
                    act,
                );
                if let Some(c) = self.clan.as_mut() {
                    c.social
                        .inbox
                        .retain(|l| l.offer.map(|o| o.0) != Some(pact));
                }
                self.note(
                    if accept {
                        "You accept. The pact stands once the Tidecourt has it on the rolls."
                    } else {
                        "Answered."
                    },
                    DoorStyle::Good,
                );
            }
            Soc::Reply(to) => {
                if let Some(i) = names.iter().position(|(_, n)| *n == to) {
                    self.soc.compose.to = i;
                }
                self.soc.compose.line = None;
                self.mode = Mode::Menu(MenuId::Compose);
                self.cursor = 0;
            }
            Soc::Block(who) => {
                self.send(
                    ctx,
                    PendKind::Block,
                    who.clone(),
                    Self::blank_on(self.today),
                    Send::Block { who: who.clone() },
                );
                if let Some(c) = self.clan.as_mut() {
                    c.social.inbox.retain(|l| l.from != who);
                }
                self.soc.letter = 0;
                self.note(
                    format!("Letters from {who} will not reach you."),
                    DoorStyle::Plain,
                );
            }
            Soc::Discard => {
                if let Some(c) = self.clan.as_mut()
                    && self.soc.letter < c.social.inbox.len()
                {
                    c.social.inbox.remove(self.soc.letter);
                }
                self.soc.letter = self.soc.letter.saturating_sub(1);
            }
            Soc::ComposeSend => self.send_letter(ctx, &names),
        }
        // Paging through targets, goods or words changes nothing worth saving.
        if !paging {
            self.save(ctx);
        }
    }

    fn post_order(&mut self, ctx: &mut Ctx<'_>, side: Side, qty: u32, price: u16) {
        let what = self.good();
        let Some(clan) = self.clan.as_mut() else {
            return;
        };
        if !clan.social.has_room() {
            self.note(
                "You have too many dispatches waiting for answers.",
                DoorStyle::Bad,
            );
            return;
        }
        let value = qty * u32::from(price);
        let fee = market::listing_fee(value);
        let mut hold = Self::blank_on(self.today);
        match side {
            Side::Ask => {
                if clan.stock.get(what) < qty || clan.tallies < fee {
                    self.note(
                        format!(
                            "You need {qty} {} and {fee} tallies for the fee.",
                            what.name()
                        ),
                        DoorStyle::Bad,
                    );
                    return;
                }
                clan.stock.take_up_to(what, qty);
                clan.tallies -= fee;
                hold.goods = Some((what, qty));
                hold.tallies = fee;
            }
            Side::Bid => {
                if clan.tallies < value + fee {
                    self.note(format!("You need {} tallies.", value + fee), DoorStyle::Bad);
                    return;
                }
                clan.tallies -= value + fee;
                hold.tallies = value + fee;
            }
        }
        let about = format!("{qty} {} at {price}", what.name());
        let (kind, act) = match side {
            Side::Ask => (PendKind::Ask, Send::Ask { what, qty, price }),
            Side::Bid => (PendKind::Bid, Send::Bid { what, qty, price }),
        };
        self.send(ctx, kind, about.clone(), hold, act);
        self.note(
            format!("Sent to the exchange: {about}. It settles with the post, the next time you open the door."),
            DoorStyle::Good,
        );
    }

    fn send_raid(&mut self, ctx: &mut Ctx<'_>, names: &[(ClanIx, String)]) {
        let Some((to_ix, to)) = names
            .get(self.soc.raid.target % names.len().max(1))
            .cloned()
        else {
            return;
        };
        let Some(me) = self.me() else { return };
        let windows = self.windows();
        let Some(win) = windows
            .get(usize::from(self.soc.raid.window) % windows.len().max(1))
            .copied()
        else {
            return;
        };
        let raiders = self.raiders();
        let taunt = if self.soc.raid.taunt == 0 {
            String::new()
        } else {
            self.taunts()
                .get(self.soc.raid.taunt - 1)
                .map_or(String::new(), |t| t.id.clone())
        };
        let Some(clan) = self.clan.as_mut() else {
            return;
        };
        if let Err(why) = raid::check(&self.world, me, to_ix, self.today) {
            self.note(why.text(), DoorStyle::Bad);
            return;
        }
        if raiders.is_empty() {
            self.note("Nobody in the hold is fit to go.", DoorStyle::Bad);
            return;
        }
        if clan.turns_left(&self.settings) < caps::RAID_TURNS {
            self.note(
                format!("A raid takes {} tide-turns.", caps::RAID_TURNS),
                DoorStyle::Bad,
            );
            return;
        }
        if !clan.social.has_room() {
            self.note(
                "You have too many dispatches waiting for answers.",
                DoorStyle::Bad,
            );
            return;
        }
        let boats = u8::from(win.needs_boat);
        if boats > 0 && clan.stock.get(Resource::Boats) < u32::from(boats) {
            self.note("That tide needs a boat, and you have none.", DoorStyle::Bad);
            return;
        }
        let ids: Vec<u16> = raiders.iter().map(|d| d.id).collect();
        let mut hold = Self::blank_on(self.today);
        if boats > 0 {
            clan.stock.take_up_to(Resource::Boats, u32::from(boats));
            hold.goods = Some((Resource::Boats, u32::from(boats)));
        }
        clan.turns_used = clan.turns_used.saturating_add(caps::RAID_TURNS);
        clan.social.away.extend(ids.iter().copied());
        clan.social.away_since = self.today;
        clan.social.raids_sent = clan.social.raids_sent.saturating_add(1);
        hold.turns = caps::RAID_TURNS;
        hold.crew = ids;
        self.send(
            ctx,
            PendKind::Raid,
            to.clone(),
            hold,
            Send::Raid {
                to: to.clone(),
                window: win.index,
                boats,
                raiders,
                taunt,
            },
        );
        self.note(
            format!("The raiders slip out toward {to}. The Tidecourt settles it overnight; the report comes with the morning."),
            DoorStyle::Good,
        );
    }

    fn send_letter(&mut self, ctx: &mut Ctx<'_>, names: &[(ClanIx, String)]) {
        let Some((_, to)) = names.get(self.soc.compose.to % names.len().max(1)).cloned() else {
            return;
        };
        let templates = self.templates();
        let Some(t) = templates.get(self.soc.compose.template % templates.len().max(1)) else {
            return;
        };
        let template = t.id.clone();
        let line = self.soc.compose.line.clone();
        let Some(clan) = self.clan.as_ref() else {
            return;
        };
        if !clan.social.has_room() {
            self.note(
                "You have too many dispatches waiting for answers.",
                DoorStyle::Bad,
            );
            return;
        }
        self.send(
            ctx,
            PendKind::Letter,
            to.clone(),
            Self::blank_on(self.today),
            Send::Letter {
                to: to.clone(),
                template,
                line,
            },
        );
        self.soc.compose.line = None;
        self.mode = Mode::Menu(MenuId::Letters);
        self.note(format!("Your letter goes to {to}."), DoorStyle::Good);
    }

    /// A line typed for a letter: through the board's text rules, which sanitize it, cap it
    /// and rate-limit it.
    pub(super) fn take_line(&mut self, buffer: &str, ctx: &mut Ctx<'_>) {
        match ctx.player_text(buffer) {
            Ok(clean) => self.soc.compose.line = Some(clean.as_str().to_owned()),
            Err(TextRefused::Empty) => self.soc.compose.line = None,
            Err(TextRefused::RateLimited) => {
                self.note("Slow down: too many messages.", DoorStyle::Bad);
            }
        }
        self.mode = Mode::Menu(MenuId::Compose);
    }
}

use crate::world::World;
