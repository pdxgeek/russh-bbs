//! The morning report: a dashboard of what the nights brought, with today's tide as a chart
//! and the season's standings. Read in ten seconds; any key goes on.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Wrap};

use super::{Size, Ui, court, fit, tides};
use crate::event::Event;
use crate::types::Resource;

/// The night's tallies, for the tiles at the top.
struct Counts {
    nights: usize,
    made: u32,
    healed: usize,
    setbacks: usize,
    news: usize,
}

fn count(events: &[Event]) -> Counts {
    let mut c = Counts {
        nights: 0,
        made: 0,
        healed: 0,
        setbacks: 0,
        news: 0,
    };
    for e in events {
        match e {
            Event::Night { .. } => c.nights += 1,
            Event::NightProduced { qty, .. } => c.made += qty,
            Event::Healed { .. } => c.healed += 1,
            Event::Injured { .. }
            | Event::Starved { .. }
            | Event::Deserted { .. }
            | Event::Hungry { .. }
            | Event::Cold { .. }
            | Event::Died { .. } => c.setbacks += 1,
            Event::Bonded { .. } | Event::Aged { .. } | Event::Recruited { .. } => c.news += 1,
            _ => {}
        }
    }
    c
}

/// One line of the report: a mark, a style and the text.
fn line_for(ui: &Ui<'_>, e: &Event) -> Option<(&'static str, Style, String)> {
    let p = &ui.pal;
    let g = |rich, plain| ui.look.g(rich, plain);
    Some(match e {
        Event::Night { .. } | Event::NightProduced { .. } => return None,
        Event::Healed { .. } | Event::Tended { .. } => (g("+", "+"), p.moss(), e.to_string()),
        Event::Bonded { .. } => (g("♥", "&"), p.gold(), e.to_string()),
        Event::Injured { .. } | Event::Scarred { .. } => (g("×", "x"), p.rust(), e.to_string()),
        Event::Hungry { .. } | Event::Cold { .. } => (g("▼", "v"), p.rust(), e.to_string()),
        Event::Starved { .. }
        | Event::Deserted { .. }
        | Event::Died { .. }
        | Event::Grief { .. } => (g("†", "t"), p.rust(), e.to_string()),
        Event::TripEnded { .. } => (g("▶", "!"), p.gold(), e.to_string()),
        Event::RaidWon { .. } | Event::PactSigned { .. } => {
            (g("▣", "!"), p.gold(), e.to_string())
        }
        Event::RaidFailed { .. } | Event::RaidSuffered { .. } | Event::PactBroken { .. } => {
            (g("▣", "!"), p.rust(), e.to_string())
        }
        Event::RaidRepelled { .. } => (g("▣", "!"), p.moss(), e.to_string()),
        Event::RaidRecalled { .. } | Event::PactDeclined { .. } => {
            (g("◦", "o"), p.slate(), e.to_string())
        }
        Event::LetterArrived { .. } | Event::PactOffered { .. } => {
            (g("▤", "@"), p.sea(), e.to_string())
        }
        Event::Parcel { .. } => (g("◈", "$"), p.gold(), e.to_string()),
        Event::Refused { .. } | Event::Unsent { .. } => (g("▲", "^"), p.rust(), e.to_string()),
        Event::Retired { .. } | Event::Aged { .. } => (g("◦", "o"), p.slate(), e.to_string()),
        _ => (g("•", "*"), p.plain, e.to_string()),
    })
}

/// What the hold made overnight, as one line: "+6 salt  +4 peat".
fn production(events: &[Event]) -> Vec<(Resource, u32)> {
    let mut out: Vec<(Resource, u32)> = Vec::new();
    for e in events {
        if let Event::NightProduced { what, qty, .. } = e {
            match out.iter_mut().find(|(r, _)| r == what) {
                Some((_, n)) => *n += qty,
                None => out.push((*what, *qty)),
            }
        }
    }
    out
}

fn tile(ui: &Ui<'_>, label: &str, value: String, style: Style) -> Paragraph<'static> {
    Paragraph::new(vec![
        Line::from(Span::styled(
            value,
            style.add_modifier(russh_bbs_door::style::Modifier::BOLD),
        ))
        .centered(),
        Line::from(Span::styled(label.to_owned(), ui.pal.dim)).centered(),
    ])
    .block(ui.panel(""))
}

pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let events = &ui.s.ledger;
    let c = count(events);
    let p = &ui.pal;
    let roomy = ui.look.size >= Size::Roomy;
    let (tiles, body) = if roomy {
        let [t, b] = Layout::vertical([Constraint::Length(4), Constraint::Min(8)]).areas(area);
        (Some(t), b)
    } else {
        (None, area)
    };
    if let Some(t) = tiles {
        let cols = Layout::horizontal([Constraint::Ratio(1, 5); 5]).split(t);
        let items = [
            ("nights passed", c.nights.to_string(), p.sea()),
            ("made overnight", c.made.to_string(), p.moss()),
            ("injuries healed", c.healed.to_string(), p.moss()),
            (
                "setbacks",
                c.setbacks.to_string(),
                if c.setbacks > 0 { p.rust() } else { p.dim },
            ),
            ("crew news", c.news.to_string(), p.gold()),
        ];
        for (col, (label, value, style)) in cols.iter().zip(items) {
            f.render_widget(tile(ui, label, value, style), *col);
        }
    }
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(56), Constraint::Min(30)]).areas(body);

    // The left: what happened, and what today holds.
    let today_h = if roomy { 8 } else { 6 };
    let [left, today] =
        Layout::vertical([Constraint::Min(6), Constraint::Length(today_h)]).areas(left);
    let block = ui.panel("While you were away");
    let inner = block.inner(left);
    f.render_widget(block, left);
    let width = usize::from(inner.width).saturating_sub(3);
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            if c.nights == 0 {
                "News from the Tidecourt".to_owned()
            } else {
                format!(
                    "{} night{} passed",
                    c.nights,
                    if c.nights == 1 { "" } else { "s" }
                )
            },
            p.sea(),
        ),
        Span::styled(
            format!(
                "  {} {}",
                ui.look.g("◗", "("),
                if c.nights > 1 {
                    "the tide ran on without you"
                } else {
                    "the tide kept its hours"
                }
            ),
            p.dim,
        ),
    ]));
    let made = production(events);
    if made.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", ui.look.g("◆", "%")), p.peat()),
            Span::styled("Nothing was made: nobody is posted to work.", p.dim),
        ]));
    } else {
        let mut spans = vec![Span::styled(format!("{} ", ui.look.g("◆", "%")), p.peat())];
        spans.push(Span::styled("Made:", p.plain));
        for (r, n) in made {
            spans.push(Span::styled(format!("  +{n} {}", r.name()), p.moss()));
        }
        lines.push(Line::from(spans));
    }
    for e in events {
        if let Some((mark, style, text)) = line_for(ui, e) {
            lines.push(Line::from(vec![
                Span::styled(format!("{mark} "), style),
                Span::styled(fit(&text, width * 2), p.plain),
            ]));
        }
    }
    // The things that did not happen are news too, and true.
    let raided = events.iter().any(|e| {
        matches!(
            e,
            Event::RaidWon { .. }
                | Event::RaidFailed { .. }
                | Event::RaidRecalled { .. }
                | Event::RaidSuffered { .. }
                | Event::RaidRepelled { .. }
        )
    });
    if !raided {
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", ui.look.g("▣", "#")), p.slate()),
            Span::styled("Raids: none. Nobody came for the hold.", p.dim),
        ]));
    }
    let unread = ui.clan.social.unread();
    lines.push(Line::from(vec![
        Span::styled(format!("{} ", ui.look.g("▤", "@")), p.slate()),
        Span::styled(
            if unread == 0 {
                "Letters: none waiting at the post.".to_owned()
            } else {
                format!("Letters: {unread} waiting at the post.")
            },
            if unread == 0 { p.dim } else { p.sea() },
        ),
    ]));
    // With room, the crew as they stand this morning.
    if roomy {
        lines.push(Line::raw(""));
        lines.push(Line::styled("The crew this morning", p.accent));
        for c in &ui.clan.crew {
            let hurt = c.injuries.len();
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "{} ",
                        if hurt > 0 {
                            ui.look.g("+", "+")
                        } else {
                            ui.look.g("●", "o")
                        }
                    ),
                    if hurt > 0 { p.rust() } else { p.moss() },
                ),
                Span::styled(format!("{:<18}", fit(&c.name, 18)), p.plain),
                Span::styled(format!("{:<11}", c.trade.name()), p.peat()),
                Span::styled(format!("age {:<3}", c.age), p.dim),
                Span::styled(
                    if hurt > 0 {
                        format!("{hurt} hurt")
                    } else {
                        "well".to_owned()
                    },
                    if hurt > 0 { p.rust() } else { p.moss() },
                ),
                Span::styled(
                    if ui.clan.partners(c.id).is_empty() {
                        ""
                    } else {
                        ui.look.g("  ♥", "  &")
                    },
                    p.gold(),
                ),
            ]));
        }
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);

    // What today holds: the tide's calendar and the hold's larder.
    let block = ui.panel("Today");
    let inner = block.inner(today);
    f.render_widget(block, today);
    let table = ui.s.tide.table(ui.s.today);
    let turns = ui.s.settings.turns_per_day;
    let mut day: Vec<Line<'static>> = vec![Line::from(vec![
        Span::styled(format!("{turns} tide-turns. "), p.plain),
        Span::styled(
            format!(
                "{} tides{}.",
                table.phase,
                if table.storm {
                    " and a storm surge"
                } else {
                    ""
                }
            ),
            if table.storm { p.rust() } else { p.sea() },
        ),
    ])];
    let windows = tides::flood_windows(ui);
    day.push(if windows.is_empty() {
        Line::styled("The water stays under the flood line all day.", p.moss())
    } else {
        Line::from(vec![
            Span::styled("Over the flood line: ", p.dim),
            Span::styled(
                windows
                    .iter()
                    .map(|(a, b)| format!("{}-{}", tides::clock(*a), tides::clock(*b % 1440)))
                    .collect::<Vec<_>>()
                    .join(", "),
                p.rust(),
            ),
        ])
    });
    let lows: Vec<String> = table
        .extremes
        .iter()
        .filter(|e| !e.high)
        .map(|e| format!("{} ({})", tides::clock(i64::from(e.minute)), e.level))
        .collect();
    day.push(Line::from(vec![
        Span::styled("Lows, the best time on the flats: ", p.dim),
        Span::styled(lows.join(", "), p.salt()),
    ]));
    day.push(Line::styled(
        format!(
            "{} crew, {} at the gate, morale {}.",
            ui.clan.crew.len(),
            ui.clan.candidates.len(),
            ui.clan.morale
        ),
        p.dim,
    ));
    f.render_widget(Paragraph::new(day).wrap(Wrap { trim: true }), inner);

    // The right: today's water, and the standings.
    let table_h = if roomy { 9u16 } else { 7 };
    let [chart, standings] =
        Layout::vertical([Constraint::Min(7), Constraint::Length(table_h)]).areas(right);
    tides::day_chart(
        ui,
        f,
        chart,
        &format!("Today: {} tides", ui.s.tide.table(ui.s.today).phase),
    );
    court::standings_panel(ui, f, standings, usize::from(table_h).saturating_sub(3));
}
