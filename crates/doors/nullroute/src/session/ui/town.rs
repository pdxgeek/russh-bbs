//! The screens between runs: home, jobs, a briefing, fixers and standings, heat, the character.

use russh_bbs_door::DoorStyle;
use russh_bbs_door::layout::{Alignment, Constraint, Rect};
use russh_bbs_door::style::{Color, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Bar, BarChart, BarGroup, Cell, Paragraph, Row, Table, TableState};

use super::{
    AMBER, CYAN, GREEN, Ink, Pen, RED, STEEL, Size, VIOLET, bar, bold, clip, cols, fg, heat_color,
    meter, rows,
};
use crate::content::Family;
use crate::learn::Stage;
use crate::player::{HEAT_MAX, PARTS, SKILL_STEPS, skill_level};
use crate::session::{Line as TextLine, Session};
use crate::world::{ALERT_MAX, Contract};

/// The two-row wordmark.
const WORDMARK: [&str; 2] = [
    "\u{2588}\u{2584} \u{2588} \u{2588} \u{2588} \u{2588}   \u{2588}   \u{2588}\u{2580}\u{2588} \u{2588}\u{2580}\u{2588} \u{2588} \u{2588} \u{2580}\u{2588}\u{2580} \u{2588}\u{2580}\u{2580}",
    "\u{2588} \u{2580}\u{2588} \u{2588}\u{2584}\u{2588} \u{2588}\u{2584}\u{2584} \u{2588}\u{2584}\u{2584} \u{2588}\u{2580}\u{2584} \u{2588}\u{2584}\u{2588} \u{2588}\u{2584}\u{2588}  \u{2588}  \u{2588}\u{2588}\u{2584}",
];

/// A text style from the door's roles.
fn role(ink: &Ink, style: DoorStyle) -> Style {
    match style {
        DoorStyle::Dim => ink.dim,
        DoorStyle::Strong => ink.strong,
        DoorStyle::Accent => bold(CYAN),
        DoorStyle::Good => fg(GREEN),
        DoorStyle::Bad => bold(RED),
        _ => ink.plain,
    }
}

/// Text lines (as the look-around view makes them) as styled, wrapped lines.
fn styled(ink: &Ink, lines: &[TextLine], skip_first: bool) -> Vec<Line<'static>> {
    lines
        .iter()
        .skip(usize::from(skip_first))
        .map(|(t, s)| Line::styled(t.clone(), role(ink, *s)))
        .collect()
}

/// A bar that runs both ways from a middle: `value` in `-range..=range`.
pub(super) fn centered(value: i32, range: i32, half: usize) -> Vec<Span<'static>> {
    let steps = usize::try_from(value.abs().min(range)).unwrap_or(0);
    let cells = |n: usize| (n * half).div_ceil(usize::try_from(range).unwrap_or(1).max(1));
    let n = cells(steps).min(half);
    let (neg, pos) = if value < 0 { (n, 0) } else { (0, n) };
    vec![
        Span::styled("\u{2591}".repeat(half - neg), fg(STEEL)),
        Span::styled("\u{2588}".repeat(neg), fg(RED)),
        Span::styled("|", fg(CYAN)),
        Span::styled("\u{2588}".repeat(pos), fg(GREEN)),
        Span::styled("\u{2591}".repeat(half - pos), fg(STEEL)),
    ]
}

/// Level pips: `filled` of `total` as small squares.
pub(super) fn pips_pub(filled: usize, total: usize, color: Color) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for i in 0..total {
        spans.push(if i < filled {
            Span::styled("\u{25A0} ", fg(color))
        } else {
            Span::styled("\u{25A1} ", fg(STEEL))
        });
    }
    spans
}

/// How far a skill has come: level, and uses toward the next one.
fn skill_line(s: &Session, f: Family, width: usize, ink: &Ink) -> Line<'static> {
    let xp = s.p.xp.get(f.index()).copied().unwrap_or(0);
    let lvl = skill_level(xp);
    let (lo, hi) = (
        if lvl == 0 {
            0
        } else {
            SKILL_STEPS[usize::from(lvl) - 1]
        },
        SKILL_STEPS.get(usize::from(lvl)).copied(),
    );
    let tail = hi.map_or_else(|| format!("max ({xp})"), |h| format!("{xp}/{h}"));
    let (v, m) = hi.map_or((1, 1), |h| (xp - lo, h - lo));
    meter(
        &format!("{:<6} L{lvl}", f.word()),
        9,
        v,
        m,
        width,
        CYAN,
        &tail,
        ink,
    )
}

/// The home screen: status gauges, the menu, the rig and the skills.
pub(super) fn home(pen: &mut Pen<'_, '_>, s: &Session, area: Rect) {
    let p = &s.p;
    let c = &*s.content;
    let big = pen.size != Size::Small;
    let top_h = if big { 3 } else { 1 };
    let [top, gauges, mid, note, foot] = rows(
        area,
        [
            Constraint::Length(top_h),
            Constraint::Length(2),
            Constraint::Min(8),
            Constraint::Length(if big { 2 } else { 1 }),
            Constraint::Length(1),
        ],
    );
    if big {
        let mut lines: Vec<Line<'static>> = WORDMARK
            .iter()
            .map(|r| Line::styled((*r).to_owned(), bold(CYAN)))
            .collect();
        lines.push(Line::styled(
            "a freelancer's rig, a city of private networks",
            pen.ink.dim,
        ));
        pen.text(top, lines);
        pen.f.render_widget(
            Paragraph::new(vec![
                Line::styled(format!("{}  ", p.name), bold(GREEN)),
                Line::styled(format!("day {}  season {}  ", p.day, s.season), pen.ink.dim),
            ])
            .alignment(Alignment::Right),
            top,
        );
    } else {
        pen.text(
            top,
            vec![Line::from(vec![
                Span::styled(" NULLROUTE ", bold(CYAN)),
                Span::styled(format!("// {}  day {}", p.name, p.day), pen.ink.plain),
            ])],
        );
    }
    // Cycles and heat side by side, marks and reputation under them.
    let [gl, gr] = cols(
        gauges,
        [Constraint::Percentage(50), Constraint::Percentage(50)],
    );
    let w = usize::from(gl.width).saturating_sub(20).clamp(6, 30);
    pen.text(
        gl,
        vec![
            meter(
                "cycles",
                6,
                p.cycles,
                p.max_cycles(),
                w,
                AMBER,
                &format!("{}/{}", p.cycles, p.max_cycles()),
                &pen.ink,
            ),
            Line::from(vec![
                Span::styled("marks  ", pen.ink.dim),
                Span::styled(p.marks.to_string(), bold(AMBER)),
                Span::styled(format!("  safe {}", p.stash), pen.ink.dim),
            ]),
        ],
    );
    let heat_line = {
        let mut spans = vec![Span::styled("heat   ", pen.ink.dim)];
        spans.extend(bar(w, f64::from(p.heat) / f64::from(HEAT_MAX), RED, true));
        spans.push(Span::styled(format!(" {}", p.heat), pen.ink.plain));
        Line::from(spans)
    };
    pen.text(
        gr,
        vec![
            heat_line,
            Line::from(vec![
                Span::styled("rep    ", pen.ink.dim),
                Span::styled(p.rep.to_string(), bold(GREEN)),
                Span::styled(
                    format!("  runs {} jobs {} burns {}", p.runs, p.jobs, p.burns),
                    pen.ink.dim,
                ),
            ]),
        ],
    );

    let third = pen.size != Size::Small;
    let (mid, below) = if pen.size == Size::Large {
        let [a, b] = rows(mid, [Constraint::Length(15), Constraint::Min(6)]);
        (a, Some(b))
    } else {
        (mid, None)
    };
    if let Some(below) = below {
        let [jobs_r, street_r] = cols(below, [Constraint::Min(60), Constraint::Length(52)]);
        let ji = pen.panel(jobs_r, "JOB BOARD", AMBER);
        jobs_table(pen, s, ji, false);
        let si = pen.panel(street_r, "STREET WORD", VIOLET);
        street(pen, s, si);
    }
    let [menu_r, rig_r, third_r] = if third {
        cols(
            mid,
            [
                Constraint::Length(30),
                Constraint::Min(30),
                Constraint::Length(if pen.size == Size::Large { 38 } else { 32 }),
            ],
        )
    } else {
        let [a, b] = cols(mid, [Constraint::Length(27), Constraint::Min(30)]);
        [a, b, Rect::default()]
    };
    // The menu.
    let mi = pen.panel(menu_r, "MENU", CYAN);
    let open = s.offers.iter().filter(|o| p.closed(c, o).is_none()).count();
    let entries: [(&str, String); 11] = [
        ("c", format!("Contracts  {open} open")),
        ("f", "Fixers".to_owned()),
        ("d", format!("Deck {}/{}", p.deck.len(), p.slots())),
        ("s", "Market".to_owned()),
        ("b", "Black market".to_owned()),
        ("r", "Rig".to_owned()),
        ("h", "Hideout".to_owned()),
        ("p", "Profile".to_owned()),
        ("t", format!("Heat {}", p.heat)),
        ("l", "Lie low".to_owned()),
        ("q", "Leave".to_owned()),
    ];
    pen.text(
        mi,
        entries
            .iter()
            .map(|(k, what)| {
                Line::from(vec![
                    Span::styled(format!("[{k}] "), bold(AMBER)),
                    Span::styled(clip(what, 22), pen.ink.plain),
                ])
            })
            .collect(),
    );
    // The rig and the skills.
    let ri = pen.panel(rig_r, "RIG", GREEN);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for i in 0..PARTS {
        let part = c.rig.part.get(i);
        let lvl = usize::from(p.rig.get(i).copied().unwrap_or(0));
        let fried = p.fried & (1 << i) != 0;
        let mut spans = vec![Span::styled(
            format!("{:<15}", part.map_or("?", |d| d.name.as_str())),
            if fried { bold(RED) } else { pen.ink.plain },
        )];
        spans.extend(pips_pub(
            lvl,
            part.map_or(3, |d| d.levels.len()),
            if fried { RED } else { GREEN },
        ));
        if fried {
            spans.push(Span::styled("FRIED", bold(RED)));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::raw(""));
    let sw = usize::from(ri.width).saturating_sub(20).clamp(4, 16);
    for f in Family::ALL {
        lines.push(skill_line(s, f, sw, &pen.ink));
    }
    pen.text(ri, lines);
    if third {
        // The fixers' standing, and the deck.
        let ti = pen.panel(third_r, "STANDING", VIOLET);
        let half = usize::from(ti.width).saturating_sub(18).clamp(3, 8) / 2;
        let mut lines: Vec<Line<'static>> = Vec::new();
        for f in &c.corps.fixer {
            let v = i32::from(p.standing.get(usize::from(f.num)).copied().unwrap_or(0));
            let mut spans = vec![Span::styled(
                format!("{:<10}", clip(&f.name, 10)),
                pen.ink.plain,
            )];
            spans.extend(centered(v, 5, half));
            spans.push(Span::styled(
                format!(" {v:+}"),
                if v <= -3 { bold(RED) } else { pen.ink.dim },
            ));
            lines.push(Line::from(spans));
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled("DECK", pen.ink.dim));
        for n in &p.deck {
            if let Some(sc) = p.script(c, *n) {
                lines.push(Line::styled(
                    format!("{} p{}", clip(&sc.name, 18), sc.power),
                    fg(GREEN),
                ));
            }
        }
        pen.text(ti, lines);
    }
    pen.notes(note, s);
    pen.hint(
        foot,
        &[
            ("q", "leave"),
            ("l", "lie low"),
            ("letters", "open a screen"),
        ],
    );
}

/// What the street says about each corporation: how alert it is, the holes players left in it,
/// and the season's breaches.
fn street(pen: &mut Pen<'_, '_>, s: &Session, rect: Rect) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    for corp in &s.content.corps.corp {
        let nets: Vec<_> = s.world.nets.iter().filter(|n| n.corp == corp.num).collect();
        let alert = nets.iter().map(|n| n.alert).max().unwrap_or(0);
        let holes: usize = nets.iter().map(|n| n.holes.len()).sum();
        let door = nets.iter().any(|n| n.backdoor);
        let mut spans = vec![Span::styled(
            format!("{:<21}", clip(&corp.name, 19)),
            pen.ink.plain,
        )];
        spans.extend(pips_pub(usize::from(alert), usize::from(ALERT_MAX), RED));
        spans.push(Span::styled(format!(" {holes} hole(s)"), pen.ink.dim));
        if door {
            spans.push(Span::styled(" backdoor", fg(GREEN)));
        }
        lines.push(Line::from(spans));
    }
    lines.push(Line::styled(
        format!(
            "{} breaches this season, market at {}%",
            s.world.breaches, s.world.market_pct
        ),
        pen.ink.dim,
    ));
    pen.text(rect, lines);
}

/// One row of the jobs table.
fn job_row(s: &Session, n: usize, c: &Contract, ink: &Ink) -> Row<'static> {
    let tmpl = s.content.contract(c.tmpl);
    let closed = s.p.closed(&s.content, c).is_some();
    let toolless = tmpl.is_some_and(|t| {
        !s.p.deck
            .iter()
            .filter_map(|n| s.content.script(*n))
            .any(|sc| sc.effect == t.effect)
    });
    let (state, style) = if closed {
        ("closed", ink.dim)
    } else if toolless {
        ("no tool", fg(AMBER))
    } else {
        ("ready", fg(GREEN))
    };
    Row::new([
        Cell::from(n.to_string()).style(bold(AMBER)),
        Cell::from(tmpl.map_or("?", |t| t.verb.as_str()).to_owned()),
        Cell::from(clip(s.corp_name(c.corp), 26)),
        Cell::from(format!("T{}", c.tier)),
        Cell::from(format!("{:>5}", c.reward)).style(fg(AMBER)),
        Cell::from(clip(s.fixer_name(c.fixer), 13)),
        Cell::from(state).style(style),
    ])
    .style(if closed { ink.dim } else { Style::new() })
}

/// The jobs table with a cursor.
fn jobs_table(pen: &mut Pen<'_, '_>, s: &Session, rect: Rect, cursor: bool) {
    let rows_: Vec<Row<'static>> = s
        .offers
        .iter()
        .enumerate()
        .map(|(i, c)| job_row(s, i + 1, c, &pen.ink))
        .collect();
    let table = Table::new(
        rows_,
        [
            Constraint::Length(2),
            Constraint::Length(7),
            Constraint::Min(10),
            Constraint::Length(2),
            Constraint::Length(5),
            Constraint::Length(13),
            Constraint::Length(7),
        ],
    )
    .header(Row::new(["#", "job", "target", "T", "pays", "fixer", "status"]).style(pen.ink.accent))
    .column_spacing(1)
    .row_highlight_style(pen.ink.sel);
    let mut state = TableState::default()
        .with_selected(cursor.then(|| s.sel.min(s.offers.len().saturating_sub(1))));
    pen.f.render_stateful_widget(table, rect, &mut state);
}

/// Jobs: a table, and the selected job's brief beside or under it.
pub(super) fn contracts(pen: &mut Pen<'_, '_>, s: &Session, area: Rect) {
    let [body, note, foot] = rows(
        area,
        [
            Constraint::Min(8),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    );
    let selected = s.offers.get(s.sel.min(s.offers.len().saturating_sub(1)));
    let detail = |pen: &mut Pen<'_, '_>, rect: Rect| {
        let inner = pen.panel(rect, "BRIEF", VIOLET);
        if let Some(c) = selected {
            let lines = s.brief_lines(c);
            pen.wrapped(inner, styled(&pen.ink, &lines, false));
        } else {
            pen.text(
                inner,
                vec![Line::styled(
                    "The fixers have nothing for you today.",
                    pen.ink.dim,
                )],
            );
        }
    };
    if area.width >= 100 {
        let [l, r] = cols(body, [Constraint::Min(56), Constraint::Length(52)]);
        let li = pen.panel(l, "JOBS", CYAN);
        jobs_table(pen, s, li, true);
        detail(pen, r);
    } else {
        let table_h = u16::try_from(s.offers.len().max(1) + 3)
            .unwrap_or(12)
            .min(body.height.saturating_sub(4));
        let [t, d] = rows(body, [Constraint::Length(table_h), Constraint::Min(3)]);
        let ti = pen.panel(t, "JOBS", CYAN);
        jobs_table(pen, s, ti, true);
        detail(pen, d);
    }
    pen.notes(note, s);
    pen.hint(
        foot,
        &[
            ("Up/Down", "choose"),
            ("Enter", "brief"),
            ("1-9", "open"),
            ("Esc", "back"),
        ],
    );
}

/// The briefing before jacking in.
pub(super) fn brief(pen: &mut Pen<'_, '_>, s: &Session, contract: &Contract, area: Rect) {
    let c = &*s.content;
    let [body, note, foot] = rows(
        area,
        [
            Constraint::Min(8),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    );
    let [l, r] = cols(
        body,
        [Constraint::Percentage(55), Constraint::Percentage(45)],
    );
    let li = pen.panel(l, "THE JOB", VIOLET);
    let own: Vec<TextLine> = s
        .brief_lines(contract)
        .into_iter()
        .filter(|(t, _)| !t.starts_with("Deck:") && !t.starts_with("They know:"))
        .collect();
    pen.wrapped(li, styled(&pen.ink, &own, false));
    let [ri_top, ri_bot] = rows(r, [Constraint::Min(6), Constraint::Length(5)]);
    let ri = pen.panel(ri_top, "YOUR DECK AGAINST THEM", GREEN);
    let tmpl = c.contract(contract.tmpl);
    let learned = s.world.learned_by(contract.corp);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for n in &s.p.deck {
        let Some(sc) = s.p.script(c, *n) else {
            continue;
        };
        let uses = learned
            .iter()
            .find(|(sig, _)| *sig == sc.sig)
            .map_or(0, |(_, u)| *u);
        let (word, style) = match crate::learn::stage(&sc, uses) {
            Stage::Fresh => ("fresh", fg(GREEN)),
            Stage::Wary => ("wary", fg(AMBER)),
            Stage::Patched => ("PATCHED", bold(RED)),
        };
        let does = tmpl.is_some_and(|t| t.effect == sc.effect);
        lines.push(Line::from(vec![
            Span::styled(if does { "* " } else { "  " }, bold(CYAN)),
            Span::styled(format!("{:<18}", clip(&sc.name, 18)), pen.ink.plain),
            Span::styled(format!("p{} ", sc.power), pen.ink.dim),
            Span::styled(word, style),
        ]));
    }
    if lines.is_empty() {
        lines.push(Line::styled(
            "(the deck is empty: load scripts first)",
            bold(RED),
        ));
    }
    lines.push(Line::styled("* can do the objective", pen.ink.dim));
    pen.text(ri, lines);
    // Street word: the alert level and holes, as pips.
    let wi = pen.panel(ri_bot, "STREET WORD", AMBER);
    let (alert, holes, door) = s
        .world
        .net(contract.corp, contract.tier)
        .map_or((0, 0, false), |n| (n.alert, n.holes.len(), n.backdoor));
    pen.text(
        wi,
        vec![
            Line::from(
                [Span::styled("alert  ", pen.ink.dim)]
                    .into_iter()
                    .chain(pips_pub(usize::from(alert), usize::from(ALERT_MAX), RED))
                    .collect::<Vec<_>>(),
            ),
            Line::styled(
                format!(
                    "{holes} open hole(s){}",
                    if door { ", a backdoor is in" } else { "" }
                ),
                pen.ink.plain,
            ),
        ],
    );
    pen.notes(note, s);
    pen.hint(
        foot,
        &[("j", "jack in"), ("p", "buy a proxy"), ("Esc", "back")],
    );
}

/// Fixers and standings.
pub(super) fn fixers(pen: &mut Pen<'_, '_>, s: &Session, area: Rect) {
    let c = &*s.content;
    let [body, foot] = rows(area, [Constraint::Min(8), Constraint::Length(1)]);
    let [l, r] = cols(
        body,
        [
            Constraint::Min(40),
            Constraint::Length(if area.width >= 100 { 46 } else { 30 }),
        ],
    );
    let li = pen.panel(l, "FIXERS", CYAN);
    let sel = s.sel.min(c.corps.fixer.len().saturating_sub(1));
    let half = usize::from(li.width).saturating_sub(24).clamp(4, 14) / 2;
    let lines: Vec<Line<'static>> = c
        .corps
        .fixer
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let v = i32::from(s.p.standing.get(usize::from(f.num)).copied().unwrap_or(0));
            let mut spans = vec![Span::styled(
                format!(
                    "{}{:<11}",
                    if i == sel { "> " } else { "  " },
                    clip(&f.name, 11)
                ),
                if i == sel { pen.ink.sel } else { pen.ink.plain },
            )];
            spans.push(Span::raw(" "));
            spans.extend(centered(v, 5, half));
            spans.push(Span::styled(
                format!(" {v:+} {}", if v <= -3 { "CLOSED" } else { "" }),
                if v <= -3 { bold(RED) } else { pen.ink.dim },
            ));
            Line::from(spans)
        })
        .collect();
    pen.text(li, lines);
    let ri = pen.panel(r, "ABOUT", VIOLET);
    if let Some(f) = c.corps.fixer.get(sel) {
        let clients: Vec<&str> = c
            .corps
            .corp
            .iter()
            .filter(|k| k.patron == f.num)
            .map(|k| k.name.as_str())
            .collect();
        let jobs = s.offers.iter().filter(|o| o.fixer == f.num).count();
        let v = s.p.standing.get(usize::from(f.num)).copied().unwrap_or(0);
        pen.wrapped(
            ri,
            vec![
                Line::styled(f.name.clone(), bold(VIOLET)),
                Line::styled(f.blurb.clone(), pen.ink.plain),
                Line::raw(""),
                Line::styled(
                    format!(
                        "Clients: {}",
                        if clients.is_empty() {
                            "none".into()
                        } else {
                            clients.join(", ")
                        }
                    ),
                    pen.ink.dim,
                ),
                Line::styled(format!("Jobs on the board: {jobs}"), pen.ink.dim),
                Line::styled(
                    if v <= -3 {
                        "Will not deal with you.".to_owned()
                    } else {
                        format!("Standing {v:+}. A job for them lowers their clients' patron.")
                    },
                    if v <= -3 { bold(RED) } else { pen.ink.dim },
                ),
            ],
        );
    }
    pen.hint(foot, &[("Up/Down", "choose"), ("Esc", "back")]);
}

/// Heat: the gauge, the ladder of what it does, and lying low.
pub(super) fn heat(pen: &mut Pen<'_, '_>, s: &Session, area: Rect) {
    let c = &*s.content;
    let p = &s.p;
    let [body, note, foot] = rows(
        area,
        [
            Constraint::Min(8),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    );
    let inner = pen.panel(body, "HEAT", RED);
    let w = usize::from(inner.width).saturating_sub(12).max(10);
    let mut spans = vec![Span::styled("heat ", pen.ink.dim)];
    spans.extend(bar(w, f64::from(p.heat) / f64::from(HEAT_MAX), RED, true));
    spans.push(Span::styled(
        format!(" {}/{HEAT_MAX}", p.heat),
        bold(heat_color(f64::from(p.heat) / 100.0)),
    ));
    let mut lines = vec![Line::from(spans)];
    // A ruler: a tick under the bar at each threshold.
    let mut ruler = vec![' '; w + 5];
    for (at, _) in super::super::HEAT_LADDER {
        let col = 5 + (*at as usize * w / HEAT_MAX as usize).min(w - 1);
        ruler[col] = '^';
    }
    lines.push(Line::styled(
        ruler.into_iter().collect::<String>(),
        fg(AMBER),
    ));
    lines.push(Line::raw(""));
    for (at, what) in super::super::HEAT_LADDER {
        let on = p.heat >= *at;
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} {at:>3}  ", if on { "\u{25A0}" } else { "\u{25A1}" }),
                if on { bold(RED) } else { fg(STEEL) },
            ),
            Span::styled((*what).to_owned(), if on { bold(RED) } else { pen.ink.dim }),
        ]));
    }
    lines.push(Line::raw(""));
    let h = &c.rig.hideout;
    lines.push(Line::styled(
        format!(
            "Heat falls {} a night. Lying low costs {} cycles and takes off {}.",
            h.heat_decay, h.lay_low_cycles, h.lay_low_heat
        ),
        pen.ink.plain,
    ));
    lines.push(Line::styled(
        "A burned run, a loud job and a dropped line all add heat.",
        pen.ink.dim,
    ));
    pen.wrapped(inner, lines);
    pen.notes(note, s);
    pen.hint(foot, &[("l", "lie low"), ("Esc", "back")]);
}

/// The character sheet.
pub(super) fn profile(pen: &mut Pen<'_, '_>, s: &Session, area: Rect) {
    let p = &s.p;
    let [body, foot] = rows(area, [Constraint::Min(8), Constraint::Length(1)]);
    let [l, r] = cols(
        body,
        [Constraint::Percentage(45), Constraint::Percentage(55)],
    );
    let li = pen.panel(l, &p.name, CYAN);
    let burn_pct = (p.burns * 100).checked_div(p.runs).unwrap_or(0);
    let mut lines = vec![
        Line::styled(format!("day {}  season {}", p.day, s.season), pen.ink.dim),
        Line::raw(""),
    ];
    let mut row = |k: &str, v: String, style: Style| {
        lines.push(Line::from(vec![
            Span::styled(format!("{k:<14}"), pen.ink.dim),
            Span::styled(v, style),
        ]));
    };
    row("reputation", p.rep.to_string(), bold(GREEN));
    row("marks", p.marks.to_string(), bold(AMBER));
    row("in the safe", p.stash.to_string(), pen.ink.plain);
    row("earned (all)", p.earned.to_string(), pen.ink.plain);
    row("runs", p.runs.to_string(), pen.ink.plain);
    row("jobs done", p.jobs.to_string(), pen.ink.plain);
    row(
        "burned",
        format!("{} ({burn_pct}%)", p.burns),
        if p.burns > 0 { fg(RED) } else { pen.ink.plain },
    );
    row("bots", p.bots.to_string(), pen.ink.plain);
    pen.text(li, lines);
    let ri = pen.panel(r, "SKILLS", GREEN);
    if ri.width >= 36 && ri.height >= 9 {
        let bars: Vec<Bar<'static>> = Family::ALL
            .iter()
            .map(|f| {
                Bar::default()
                    .value(u64::from(p.skill(*f)))
                    .label(Line::from(f.word()))
                    .text_value(format!("L{}", p.skill(*f)))
                    .style(fg(CYAN))
            })
            .collect();
        pen.f.render_widget(
            BarChart::default()
                .data(BarGroup::default().bars(&bars))
                .bar_width(5)
                .bar_gap(1)
                .max(5)
                .value_style(bold(CYAN)),
            ri,
        );
    } else {
        let w = usize::from(ri.width).saturating_sub(22).clamp(4, 20);
        let lines: Vec<Line<'static>> = Family::ALL
            .iter()
            .map(|f| skill_line(s, *f, w, &pen.ink))
            .collect();
        pen.text(ri, lines);
    }
    pen.hint(foot, &[("Esc", "back")]);
}
