//! The social screens: the exchange, the gate (raids), pacts, the post and the letter being
//! written. Like every screen they read the engine (`social/` and the world snapshot) and
//! draw it; no rule lives here.
//!
//! **Honesty on these screens.** What a raider sees of a hold is what the hold *posted*: the
//! guard, the defences and a scout's guess at the stores, in fives. Nothing on these screens
//! is private to its clan, and nothing is called private: letters are readable by the board
//! and by the clan they are written to.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::{Size, Ui, court, fit, market, wrap};
use crate::session::Mode;
use crate::social::letters::Kind as LetterKind;
use crate::social::pact::{self, ALL_TERMS, TRUCE};
use crate::social::{caps, raid};
use crate::types::Resource;
use crate::world::World;

/// The height of an orders panel for `n` entries, at most `max` rows.
fn orders_h(n: usize, max: u16) -> u16 {
    (u16::try_from(n).unwrap_or(max) + 2).clamp(3, max)
}

fn owner_name(ui: &Ui<'_>, ix: u8) -> String {
    ui.s.world
        .clans
        .get(usize::from(ix))
        .map_or_else(|| "?".to_owned(), |c| c.name.clone())
}

/// The exchange: the book for one good, its price against the Tidecourt's, and the orders.
pub(super) fn exchange(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let what = ui.s.good();
    let x = &ui.s.world.social.exchange;
    let me = ui.s.me();
    let roomy = ui.look.size >= Size::Roomy;
    let [left, right] = Layout::horizontal([
        Constraint::Min(34),
        Constraint::Length(if roomy { 42 } else { 34 }),
    ])
    .areas(area);
    let [book, trail] = Layout::vertical([
        Constraint::Min(7),
        Constraint::Length(if roomy { 5 } else { 4 }),
    ])
    .areas(left);
    let prices = &ui.s.world.prices;
    let mut asks: Vec<_> = x.asks.iter().filter(|o| o.what == what).collect();
    asks.sort_by_key(|o| (o.price, o.id));
    let mut bids: Vec<_> = x.bids.iter().filter(|o| o.what == what).collect();
    bids.sort_by_key(|o| (std::cmp::Reverse(o.price), o.id));
    let each = usize::from(book.height.saturating_sub(5) / 2).max(1);
    let name_w = usize::from(book.width).saturating_sub(18).clamp(6, 20);
    let row = |o: &crate::social::market::Order, ask: bool| {
        let mine = Some(o.owner) == me;
        Line::from(vec![
            Span::styled(
                format!(
                    "{}{:<w$}",
                    if mine { ui.look.g("▸", ">") } else { " " },
                    fit(&owner_name(ui, o.owner), name_w),
                    w = name_w
                ),
                if mine { p.gold() } else { p.plain },
            ),
            Span::styled(
                format!(" {:>3} @ {:>4}", o.qty, o.price),
                if ask { p.gold() } else { p.sea() },
            ),
        ])
    };
    let mut lines = vec![Line::from(vec![
        Span::styled("Tidecourt pays ", p.dim),
        Span::styled(World::sell_value(prices, what, 1).to_string(), p.moss()),
        Span::styled(", sells at ", p.dim),
        Span::styled(World::buy_cost(prices, what, 1).to_string(), p.gold()),
    ])];
    lines.push(Line::styled("Asks: goods for sale", p.accent));
    if asks.is_empty() {
        lines.push(Line::styled("  none standing", p.dim));
    }
    for o in asks.iter().take(each) {
        lines.push(row(o, true));
    }
    lines.push(Line::styled("Bids: tallies offered", p.accent));
    if bids.is_empty() {
        lines.push(Line::styled("  none standing", p.dim));
    }
    for o in bids.iter().take(each) {
        lines.push(row(o, false));
    }
    let waiting = ui.clan.social.pending.len();
    if waiting > 0 {
        lines.push(Line::styled(
            format!(
                "{waiting} dispatch{} awaiting the Tidecourt",
                if waiting == 1 { "" } else { "es" }
            ),
            p.rust(),
        ));
    }
    f.render_widget(
        Paragraph::new(lines).block(ui.panel(&format!("The exchange: {}", what.name()))),
        book,
    );
    // The two price trails side by side in words and glyphs.
    let ex = x.trail(what);
    let tc = ui.s.world.price_trail(what);
    let w = usize::from(trail.width).saturating_sub(24).clamp(6, 14);
    let line = |label: &str, trail: &[u16], now: String| {
        Line::from(vec![
            Span::styled(format!("{label:<10}"), p.dim),
            Span::styled(market::spark(ui, trail, w), p.sea()),
            Span::styled(format!(" {now}"), p.plain),
        ])
    };
    let mut trail_lines = vec![line("Tidecourt", &tc, market::price(prices[what.index()]))];
    trail_lines.push(if ex.is_empty() {
        Line::styled("Exchange   no trades yet", p.dim)
    } else {
        line("Exchange", &ex, market::price(*ex.last().unwrap_or(&0)))
    });
    if roomy {
        trail_lines.push(Line::styled(
            format!(
                "Fees: {}% to post, {}% of a sale (half between trade partners).",
                caps::LISTING_FEE_PCT,
                caps::LEVY_PCT
            ),
            p.dim,
        ));
    }
    f.render_widget(
        Paragraph::new(trail_lines).block(ui.panel("Prices, two weeks")),
        trail,
    );
    let items: Vec<_> = ui.entries.iter().enumerate().collect();
    ui.orders(f, right, "Orders", &items);
}

/// The gate: the target, the party, the tide, and the orders.
pub(super) fn raid(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let others = ui.s.others();
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Min(30)]).areas(area);
    if others.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::styled("No other clan is on the Tidecourt's rolls yet.", p.plain),
                Line::styled(
                    "Raids need someone to raid: the rolls fill as other clans found their holds.",
                    p.dim,
                ),
            ])
            .wrap(Wrap { trim: true })
            .block(ui.panel("The gate")),
            area,
        );
        return;
    }
    let (ix, target) = others[ui.s.soc.raid.target % others.len()];
    let items: Vec<_> = ui.entries.iter().enumerate().collect();
    let [info, orders] = Layout::vertical([
        Constraint::Min(8),
        Constraint::Length(orders_h(items.len(), 9)),
    ])
    .areas(left);
    let mut lines = vec![Line::from(vec![
        Span::styled(format!("renown {}", target.score), p.gold()),
        Span::styled(
            format!(
                "   rating {}{}",
                target.posted.rating,
                target
                    .posted
                    .sluice
                    .map_or(String::new(), |s| format!("   sluice at {s}"))
            ),
            p.plain,
        ),
    ])];
    lines.push(Line::styled(
        format!(
            "Guard at the gate ({} of {} crew):",
            target.posted.guard.len(),
            target.posted.crew
        ),
        p.accent,
    ));
    if target.posted.guard.is_empty() {
        lines.push(Line::styled("  nobody: the gate stands open", p.rust()));
    }
    for d in target
        .posted
        .guard
        .iter()
        .take(usize::from(info.height.saturating_sub(7)).max(1))
    {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<11}", fit(&d.name, 11)), p.plain),
            Span::styled(format!("{:<9}", fit(d.trade.name(), 8)), p.peat()),
            Span::styled(format!("hp{} a{} d{}", d.hp, d.atk, d.def), p.dim),
        ]));
    }
    let stores: Vec<String> = Resource::ALL
        .iter()
        .filter(|r| target.posted.stock[r.index()] > 0)
        .take(4)
        .map(|r| format!("{} {}", r.name(), target.posted.stock[r.index()]))
        .collect();
    lines.push(Line::styled(
        format!(
            "Stores, a scout's guess: {}{}",
            if stores.is_empty() {
                "bare".to_owned()
            } else {
                stores.join(", ")
            },
            if target.posted.tallies > 0 {
                format!(", {} tallies", target.posted.tallies)
            } else {
                String::new()
            }
        ),
        p.dim,
    ));
    match ui.s.raid_closed(ix) {
        Some(why) => lines.push(Line::styled(why.text().to_owned(), p.rust())),
        None => lines.push(Line::styled("The way is open.", p.moss())),
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel(&format!("Target: {}", fit(&target.name, 22)))),
        info,
    );
    ui.orders(f, orders, "Orders", &items);
    // The party and the tide.
    let windows = ui.s.windows();
    let sel = usize::from(ui.s.soc.raid.window) % windows.len().max(1);
    let party = ui.s.raiders();
    let level = windows.get(sel).map_or(50, |w| w.level);
    let [party_area, tide_area] = Layout::vertical([
        Constraint::Length(
            u16::try_from(party.len().max(1) + 4)
                .unwrap_or(8)
                .min(right.height / 2 + 1),
        ),
        Constraint::Min(5),
    ])
    .areas(right);
    let mut lines = Vec::new();
    if party.is_empty() {
        lines.push(Line::styled("Nobody in the hold is fit to go.", p.rust()));
    }
    for d in &party {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<12}", fit(&d.name, 11)), p.plain),
            Span::styled(format!("{:<11}", d.trade.name()), p.peat()),
            Span::styled(format!("hp {} atk {}", d.hp, d.atk), p.dim),
        ]));
    }
    if !party.is_empty() {
        let odds = raid::preview_odds(&ui.s.cat, &ui.s.world, ix, &party, level);
        lines.push(Line::from(vec![
            Span::styled("Odds at this tide: ", p.dim),
            Span::styled(odds.word().to_owned(), p.salt()),
        ]));
        lines.push(Line::styled(
            format!(
                "Costs {} turns. The raiders are away from the hold until the report.",
                caps::RAID_TURNS
            ),
            p.dim,
        ));
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("Your raiders")),
        party_area,
    );
    let mut lines = vec![Line::styled(
        "It lands tomorrow, at a turn of the water:",
        p.dim,
    )];
    for (i, w) in windows.iter().enumerate() {
        let on = i == sel;
        lines.push(Line::from(vec![
            Span::styled(if on { ui.look.g("▸ ", "> ") } else { "  " }, p.gold()),
            Span::styled(
                format!(
                    "{:02}:{:02} {} water {}",
                    w.minute / 60,
                    w.minute % 60,
                    if w.high { "high" } else { "low " },
                    w.level
                ),
                if on { p.salt() } else { p.plain },
            ),
            Span::styled(
                if w.needs_boat {
                    "  needs a boat"
                } else {
                    "  on foot"
                },
                if w.needs_boat { p.rust() } else { p.moss() },
            ),
        ]));
    }
    if let Some(s) = target.posted.sluice {
        lines.push(Line::styled(
            format!("Their sluice floods the approach at water {s}."),
            p.dim,
        ));
    }
    if ui.s.soc.raid.taunt > 0
        && let Some(t) = ui.s.taunts().get(ui.s.soc.raid.taunt - 1)
    {
        lines.push(Line::styled(
            format!("Taunt: {}", fit(&t.words(&ui.clan.name, &target.name), 60)),
            p.dim,
        ));
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("The tide")),
        tide_area,
    );
}

/// Pacts: those in force, the standing, and the terms being drawn up.
pub(super) fn pacts(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let world = &ui.s.world;
    let me = ui.s.me();
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(46), Constraint::Min(32)]).areas(area);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if let Some(m) = me {
        let rec = world.clans.get(usize::from(m)).map(|c| &c.record);
        if let Some(r) = rec {
            lines.push(Line::from(vec![
                Span::styled("Standing with the Tidecourt ", p.dim),
                Span::styled(
                    format!("{:+}", r.rep),
                    if r.rep < 0 { p.rust() } else { p.moss() },
                ),
            ]));
            if r.betrayals > 0 {
                lines.push(Line::styled(
                    format!(
                        "{} pact{} broken on record: trade and defence are closed to you this season.",
                        r.betrayals,
                        if r.betrayals == 1 { "" } else { "s" }
                    ),
                    p.rust(),
                ));
            }
            if r.pact_ban_until > ui.s.today {
                lines.push(Line::styled(
                    format!(
                        "Cooling off: no offers until day {}.",
                        r.pact_ban_until - ui.clan.founded + 1
                    ),
                    p.rust(),
                ));
            }
        }
        lines.push(Line::styled("Pacts in force", p.accent));
        let mut any = false;
        for pt in world.pacts_of(m) {
            any = true;
            let other = owner_name(ui, pt.other(m));
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<14}", fit(&other, 14)), p.plain),
                Span::styled(format!("{} days", (ui.s.today - pt.since).max(0)), p.dim),
            ]));
            lines.push(Line::styled(
                format!("    {}", crate::event::terms_phrase(pt.terms)),
                p.sea(),
            ));
        }
        if !any {
            lines.push(Line::styled("  none", p.dim));
        }
        let offered: Vec<_> = world
            .social
            .pacts
            .iter()
            .filter(|pt| pt.state == pact::State::Proposed && pt.a == m)
            .collect();
        if !offered.is_empty() {
            lines.push(Line::styled("Offers you made", p.accent));
            for pt in offered {
                lines.push(Line::styled(
                    format!(
                        "  to {} ({})",
                        owner_name(ui, pt.b),
                        crate::event::terms_phrase(pt.terms)
                    ),
                    p.plain,
                ));
            }
        }
    } else {
        lines.push(Line::styled(
            "The Tidecourt has not yet entered your clan on its rolls.",
            p.dim,
        ));
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("Pacts")),
        left,
    );
    let others = ui.s.others();
    let items: Vec<_> = ui.entries.iter().enumerate().collect();
    let [terms_area, orders] = Layout::vertical([
        Constraint::Min(8),
        Constraint::Length(orders_h(items.len(), 10)),
    ])
    .areas(right);
    let mut lines = Vec::new();
    if let (Some(m), Some((ix, t))) = (me, others.get(ui.s.soc.pact.target % others.len().max(1))) {
        lines.push(Line::from(vec![
            Span::styled("With ", p.dim),
            Span::styled(fit(&t.name, 20), p.salt()),
            Span::styled(format!("  renown {}", t.score), p.dim),
        ]));
        for term in ALL_TERMS {
            let on = term == TRUCE || ui.s.soc.pact.terms & term != 0;
            let closed = world.closed_reason(m, *ix, term, ui.s.today);
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" [{}] ", if on { ui.look.g("✓", "x") } else { " " }),
                    if on { p.moss() } else { p.dim },
                ),
                Span::styled(format!("{:<17}", pact::term_name(term)), p.plain),
                match closed {
                    None => Span::styled(pact::term_blurb(term), p.dim),
                    Some(why) => Span::styled(format!("closed: {why}"), p.rust()),
                },
            ]));
        }
    } else {
        lines.push(Line::styled("No other clan is on the rolls yet.", p.dim));
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("Terms")),
        terms_area,
    );
    ui.orders(f, orders, "Orders", &items);
}

/// The post: the inbox, the letter being read, and the notices.
pub(super) fn letters(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let inbox = &ui.clan.social.inbox;
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Min(30)]).areas(area);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if inbox.is_empty() {
        lines.push(Line::styled(
            "No letters. Letters from other clan leaders arrive here with the morning.",
            p.dim,
        ));
    }
    let empty = inbox.is_empty();
    let rows = usize::from(left.height).saturating_sub(2);
    let sel = ui.s.soc.letter.min(inbox.len().saturating_sub(1));
    let first = (sel + 1).saturating_sub(rows);
    for (i, l) in inbox.iter().enumerate().skip(first).take(rows) {
        let on = i == sel;
        lines.push(Line::from(vec![
            Span::styled(if on { ui.look.g("▸ ", "> ") } else { "  " }, p.gold()),
            Span::styled(
                if l.read {
                    "  "
                } else {
                    ui.look.g("● ", "* ")
                },
                p.sea(),
            ),
            Span::styled(
                fit(&l.from, usize::from(left.width).saturating_sub(12)),
                if on { p.salt() } else { p.plain },
            ),
            Span::styled(if l.offer.is_some() { " pact" } else { "" }, p.gold()),
        ]));
    }
    let list = Paragraph::new(lines)
        .block(ui.panel(&format!("The post ({} unread)", ui.clan.social.unread())));
    f.render_widget(
        if empty {
            list.wrap(Wrap { trim: true })
        } else {
            list
        },
        left,
    );
    let items: Vec<_> = ui.entries.iter().enumerate().collect();
    let [read, orders] = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(orders_h(items.len(), 8)),
    ])
    .areas(right);
    let mut text: Vec<Line<'static>> = Vec::new();
    if let Some(l) = inbox.get(sel) {
        let words =
            ui.s.cat
                .letters
                .entries
                .iter()
                .find(|d| d.id == l.template)
                .map_or_else(String::new, |d| d.words(&l.from, &ui.clan.name));
        for line in wrap(&words, usize::from(read.width).saturating_sub(4)) {
            text.push(Line::styled(line, p.plain));
        }
        if let Some(own) = &l.line {
            text.push(Line::raw(""));
            text.push(Line::from(vec![
                Span::styled(format!("{} says: ", l.from), p.dim),
                Span::styled(own.clone(), p.plain),
            ]));
        }
        if let Some((_, terms)) = l.offer {
            text.push(Line::raw(""));
            text.push(Line::styled(
                format!("The offer: {}", crate::event::terms_phrase(terms)),
                p.gold(),
            ));
        }
        text.push(Line::raw(""));
        text.push(Line::styled(
            "Letters are read by the board and by the clan they are for. Nothing sent here is private.",
            p.dim,
        ));
    } else {
        let tomorrow = ui.s.tide.table(ui.s.today + 1);
        text.push(Line::styled(
            format!(
                "The harbour-reeve says tomorrow's tides are {}{}.",
                tomorrow.phase,
                if tomorrow.storm {
                    ", with a storm surge"
                } else {
                    ""
                }
            ),
            p.plain,
        ));
        text.push(Line::styled(
            format!(
                "{} hand(s) wait at your gate for work.",
                ui.clan.candidates.len()
            ),
            p.plain,
        ));
        for (r, pct) in court::movers(ui) {
            text.push(Line::styled(
                format!("{} is {pct:+}% on the day.", r.name()),
                p.plain,
            ));
        }
    }
    f.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: true })
            .block(ui.panel("Letter")),
        read,
    );
    ui.orders(f, orders, "Orders", &items);
}

/// The letter being written.
pub(super) fn compose(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let others = ui.s.others();
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(58), Constraint::Min(28)]).areas(area);
    let templates = ui.s.templates();
    let c = &ui.s.soc.compose;
    let mut lines = Vec::new();
    if let Some((_, to)) = others.get(c.to % others.len().max(1)) {
        lines.push(Line::from(vec![
            Span::styled("To      ", p.dim),
            Span::styled(to.name.clone(), p.salt()),
        ]));
        if let Some(t) = templates.get(c.template % templates.len().max(1)) {
            lines.push(Line::from(vec![
                Span::styled("Kind    ", p.dim),
                Span::styled(t.kind.name().to_owned(), p.sea()),
            ]));
            lines.push(Line::raw(""));
            for l in wrap(
                &t.words(&ui.clan.name, &to.name),
                usize::from(left.width).saturating_sub(4),
            ) {
                lines.push(Line::styled(l, p.plain));
            }
        }
    } else {
        lines.push(Line::styled("No other clan is on the rolls yet.", p.dim));
    }
    lines.push(Line::raw(""));
    match &ui.s.mode {
        Mode::Line(buffer) => {
            lines.push(Line::from(vec![
                Span::styled("Your line: ", p.dim),
                Span::styled(format!("{buffer}{}", ui.look.g("▌", "_")), p.strong),
            ]));
            lines.push(Line::styled(
                format!(
                    "Enter sets it, Esc leaves it. {} characters at most; the board cleans it.",
                    crate::social::letters::LINE_MAX
                ),
                p.dim,
            ));
        }
        _ => lines.push(match &c.line {
            Some(l) => Line::from(vec![
                Span::styled("Your line: ", p.dim),
                Span::styled(l.clone(), p.plain),
            ]),
            None => Line::styled("No line of your own.", p.dim),
        }),
    }
    let _ = LetterKind::Greeting;
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(ui.panel("A letter")),
        left,
    );
    let items: Vec<_> = ui.entries.iter().enumerate().collect();
    ui.orders(f, right, "Orders", &items);
}
