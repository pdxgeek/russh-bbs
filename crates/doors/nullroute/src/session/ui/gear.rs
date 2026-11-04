//! The screens about what you own: the deck, the market, the black market, the rig, the hideout
//! and the module editor.

use russh_bbs_door::layout::{Constraint, Rect};
use russh_bbs_door::style::Style;
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Cell, Row, Sparkline, Table, TableState};

use super::{
    AMBER, CYAN, GREEN, Ink, Pen, RED, STEEL, Size, VIOLET, bar, bold, clip, cols, fg, meter, rows,
};
use crate::code::{self, CODE_CYCLES, MEMORY_BUDGET, NOISE_BUDGET};
use crate::content::{Family, FragKind, ScriptDef};
use crate::learn::{self, Stage};
use crate::player::PARTS;
use crate::session::{CodeStep, Draft, PAGE, Session};

/// The stage of a script in each corporation, from what the world has learned.
fn stages(s: &Session, sc: &ScriptDef) -> Vec<Stage> {
    s.content
        .corps
        .corp
        .iter()
        .map(|corp| {
            let uses = s
                .world
                .learned_by(corp.num)
                .iter()
                .find(|(sig, _)| *sig == sc.sig)
                .map_or(0, |(_, u)| *u);
            learn::stage(sc, uses)
        })
        .collect()
}

/// The degradation matrix of a script as one cell of letters: fresh `.`, wary `w`, patched `X`.
fn matrix(stages: &[Stage]) -> Line<'static> {
    Line::from(
        stages
            .iter()
            .map(|st| match st {
                Stage::Fresh => Span::styled(". ", fg(GREEN)),
                Stage::Wary => Span::styled("w ", fg(AMBER)),
                Stage::Patched => Span::styled("X ", bold(RED)),
            })
            .collect::<Vec<_>>(),
    )
}

/// The loudest a deck of `slots` scripts could be, the scale of the noise gauge.
fn noise_scale(s: &Session) -> u32 {
    let loudest = s
        .content
        .scripts
        .script
        .iter()
        .map(|x| x.noise)
        .max()
        .unwrap_or(1);
    loudest * u32::try_from(s.p.slots()).unwrap_or(1)
}

/// Power, memory and noise as three short bars.
fn stat_bars(sc: &ScriptDef, w: usize, ink: &Ink) -> Vec<Line<'static>> {
    vec![
        meter(
            "power",
            7,
            sc.power,
            9,
            w,
            GREEN,
            &sc.power.to_string(),
            ink,
        ),
        meter(
            "memory",
            7,
            sc.memory,
            8,
            w,
            CYAN,
            &sc.memory.to_string(),
            ink,
        ),
        meter(
            "noise",
            7,
            sc.noise,
            10,
            w,
            AMBER,
            &sc.noise.to_string(),
            ink,
        ),
    ]
}

/// The deck: what is loaded against the rig's memory and slots, every script you own, and what
/// each corporation has learned of it.
pub(super) fn deck(pen: &mut Pen<'_, '_>, s: &Session, page: usize, area: Rect) {
    let p = &s.p;
    let c = &*s.content;
    let owned: Vec<(usize, u8)> = p
        .owned
        .iter()
        .enumerate()
        .skip(page * PAGE)
        .take(PAGE)
        .map(|(i, n)| (i, *n))
        .collect();
    let sel = s.sel.min(owned.len().saturating_sub(1));
    let [gauges, body, note, foot] = rows(
        area,
        [
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    );
    let w = usize::from(gauges.width / 2)
        .saturating_sub(24)
        .clamp(6, 24);
    let loaded_noise: u32 = p
        .deck
        .iter()
        .filter_map(|n| p.script(c, *n))
        .map(|sc| sc.noise)
        .sum();
    let [gl, gr] = cols(
        gauges,
        [Constraint::Percentage(50), Constraint::Percentage(50)],
    );
    pen.text(
        gl,
        vec![
            meter(
                "memory",
                7,
                p.deck_memory(c),
                p.memory(),
                w,
                if p.deck_memory(c) >= p.memory() {
                    AMBER
                } else {
                    GREEN
                },
                &format!("{}/{}", p.deck_memory(c), p.memory()),
                &pen.ink,
            ),
            meter(
                "slots",
                7,
                u32::try_from(p.deck.len()).unwrap_or(0),
                u32::try_from(p.slots()).unwrap_or(1),
                w,
                CYAN,
                &format!("{}/{}", p.deck.len(), p.slots()),
                &pen.ink,
            ),
        ],
    );
    pen.text(
        gr,
        vec![
            meter(
                "noise",
                7,
                loaded_noise,
                noise_scale(s),
                w,
                AMBER,
                &loaded_noise.to_string(),
                &pen.ink,
            ),
            Line::from(vec![
                Span::styled("corporations ", pen.ink.dim),
                Span::styled(
                    c.corps
                        .corp
                        .iter()
                        .map(|k| k.name.chars().next().unwrap_or('?').to_string())
                        .collect::<Vec<_>>()
                        .join(" "),
                    pen.ink.accent,
                ),
                Span::styled("   . fresh  w wary  X patched", pen.ink.dim),
            ]),
        ],
    );
    let (left, right) = if pen.size == Size::Small {
        (body, None)
    } else {
        let [l, r] = cols(body, [Constraint::Min(58), Constraint::Length(34)]);
        (l, Some(r))
    };
    let li = pen.panel(left, &format!("SCRIPTS  page {}", page + 1), GREEN);
    let wide = li.width >= 96;
    let initials: String = c.corps.corp.iter().fold(String::new(), |mut acc, k| {
        acc.push(k.name.chars().next().unwrap_or('?'));
        acc.push(' ');
        acc
    });
    let rows_: Vec<Row<'static>> = owned
        .iter()
        .enumerate()
        .filter_map(|(k, (_, num))| {
            let sc = p.script(c, *num)?;
            let loaded = p.deck.contains(num);
            let st = stages(s, &sc);
            let key = char::from(b'a' + u8::try_from(k).unwrap_or(0));
            Some(
                Row::new([
                    Cell::from(format!("[{key}]")).style(pen.ink.accent),
                    Cell::from(if loaded { "*" } else { " " }).style(bold(GREEN)),
                    Cell::from(clip(&sc.name, 16)),
                    Cell::from(sc.family.word()),
                    Cell::from(format!("{:?}", sc.effect).to_lowercase()),
                    Cell::from(format!("p{}", sc.power)),
                    Cell::from(format!("m{}", sc.memory)),
                    Cell::from(format!("n{}", sc.noise)),
                    Cell::from(matrix(&st)),
                    Cell::from(if wide {
                        clip(&sc.blurb, 60)
                    } else {
                        String::new()
                    }),
                ])
                .style(if loaded { pen.ink.strong } else { Style::new() }),
            )
        })
        .collect();
    let table = Table::new(
        rows_,
        [
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(10),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(u16::try_from(c.corps.corp.len() * 2).unwrap_or(10)),
            Constraint::Length(if wide { 40 } else { 0 }),
        ],
    )
    .header(
        Row::new([
            Cell::from(""),
            Cell::from(""),
            Cell::from("script"),
            Cell::from("family"),
            Cell::from("effect"),
            Cell::from("pow"),
            Cell::from("mem"),
            Cell::from("noi"),
            Cell::from(initials.clone()),
            Cell::from(if wide { "what it does" } else { "" }),
        ])
        .style(pen.ink.dim),
    )
    .column_spacing(1)
    .row_highlight_style(pen.ink.sel);
    let mut state = TableState::default().with_selected(Some(sel));
    pen.f.render_stateful_widget(table, li, &mut state);
    if let Some(r) = right {
        let ri = pen.panel(r, "DETAIL", VIOLET);
        if let Some((_, num)) = owned.get(sel)
            && let Some(sc) = p.script(c, *num)
        {
            let mut lines = vec![
                Line::styled(sc.name.clone(), bold(VIOLET)),
                Line::styled(
                    format!("{} / {:?}", sc.family.word(), sc.effect),
                    pen.ink.dim,
                ),
                Line::raw(""),
            ];
            lines.extend(stat_bars(
                &sc,
                usize::from(ri.width).saturating_sub(12).clamp(4, 14),
                &pen.ink,
            ));
            lines.push(Line::raw(""));
            lines.push(Line::styled(sc.blurb.clone(), pen.ink.plain));
            if sc.min_skill > 0 {
                lines.push(Line::styled(
                    format!("needs skill {}", sc.min_skill),
                    pen.ink.dim,
                ));
            }
            if let Some(t) = sc.target {
                lines.push(Line::styled(
                    format!("tuned to {}", crate::session::plural(t)),
                    fg(GREEN),
                ));
            }
            pen.wrapped(ri, lines);
        }
    }
    if s.msgs.is_empty() {
        let blurb = owned
            .get(sel)
            .and_then(|(_, n)| p.script(c, *n))
            .map_or_else(String::new, |sc| sc.blurb);
        pen.text(
            note,
            vec![Line::styled(
                clip(&blurb, usize::from(note.width)),
                pen.ink.dim,
            )],
        );
    } else {
        pen.notes(note, s);
    }
    pen.hint(
        foot,
        &[
            ("a-l", "load/unload"),
            ("Enter", "toggle"),
            ("A-L", "scrap coded"),
            ("n/p", "page"),
            ("Esc", "back"),
        ],
    );
}

/// The script market: a table, the market's tide, and a sparkline of the prices on offer.
pub(super) fn market(pen: &mut Pen<'_, '_>, s: &Session, page: usize, area: Rect) {
    let p = &s.p;
    let all = s.for_sale();
    let shown: Vec<&ScriptDef> = all.iter().copied().skip(page * PAGE).take(PAGE).collect();
    let sel = s.sel.min(shown.len().saturating_sub(1));
    let price = |sc: &ScriptDef| sc.price * s.world.market_pct / 100;
    let [body, note, foot] = rows(
        area,
        [
            Constraint::Min(8),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    );
    let (left, right) = if pen.size == Size::Small {
        (body, None)
    } else {
        let [l, r] = cols(body, [Constraint::Min(54), Constraint::Length(40)]);
        (l, Some(r))
    };
    let li = pen.panel(
        left,
        &format!("MARKET  marks {}  page {}", p.marks, page + 1),
        AMBER,
    );
    let rows_: Vec<Row<'static>> = shown
        .iter()
        .enumerate()
        .map(|(k, sc)| {
            let afford = p.marks >= price(sc) && p.rep >= sc.min_rep;
            let key = char::from(b'a' + u8::try_from(k).unwrap_or(0));
            Row::new([
                Cell::from(format!("[{key}]")).style(pen.ink.accent),
                Cell::from(clip(&sc.name, 16)),
                Cell::from(sc.family.word()),
                Cell::from(format!("{:?}", sc.effect).to_lowercase()),
                Cell::from(format!("p{}", sc.power)),
                Cell::from(format!("m{}", sc.memory)),
                Cell::from(format!("{:>5}", price(sc))).style(if afford {
                    fg(AMBER)
                } else {
                    pen.ink.dim
                }),
                Cell::from(if p.rep < sc.min_rep {
                    format!("rep {}", sc.min_rep)
                } else {
                    String::new()
                })
                .style(fg(RED)),
            ])
            .style(if afford { Style::new() } else { pen.ink.dim })
        })
        .collect();
    let table = Table::new(
        rows_,
        [
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(7),
            Constraint::Length(8),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Length(7),
        ],
    )
    .header(
        Row::new([
            "", "script", "family", "effect", "pow", "mem", "price", "gate",
        ])
        .style(pen.ink.dim),
    )
    .column_spacing(1)
    .row_highlight_style(pen.ink.sel);
    let mut state = TableState::default().with_selected(Some(sel));
    pen.f.render_stateful_widget(table, li, &mut state);
    if let Some(r) = right {
        let [tide, detail] = rows(r, [Constraint::Length(7), Constraint::Min(6)]);
        let ti = pen.panel(tide, "THE TIDE", AMBER);
        let prices: Vec<u64> = all.iter().map(|sc| u64::from(price(sc))).collect();
        let pct = s.world.market_pct;
        let w = usize::from(ti.width).saturating_sub(12).max(6);
        let [t0, t1, t2] = rows(
            ti,
            [
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(2),
            ],
        );
        pen.text(
            t0,
            vec![meter(
                "market",
                7,
                pct.saturating_sub(80),
                50,
                w,
                if pct > 100 { RED } else { GREEN },
                &format!("{pct}%"),
                &pen.ink,
            )],
        );
        pen.text(
            t1,
            vec![Line::styled("moves nightly, 80% to 130%", pen.ink.dim)],
        );
        pen.f
            .render_widget(Sparkline::default().data(&prices).style(fg(AMBER)), t2);
        let di = pen.panel(detail, "DETAIL", VIOLET);
        if let Some(sc) = shown.get(sel) {
            let mut lines = vec![Line::styled(sc.name.clone(), bold(VIOLET))];
            lines.extend(stat_bars(
                sc,
                usize::from(di.width).saturating_sub(12).clamp(4, 14),
                &pen.ink,
            ));
            lines.push(Line::raw(""));
            lines.push(Line::styled(sc.blurb.clone(), pen.ink.plain));
            lines.push(Line::styled(
                format!("costs {} of your {}", price(sc), p.marks),
                if p.marks >= price(sc) {
                    fg(GREEN)
                } else {
                    bold(RED)
                },
            ));
            pen.wrapped(di, lines);
        }
    }
    pen.notes(note, s);
    pen.hint(
        foot,
        &[
            ("a-l", "buy"),
            ("Enter", "buy selected"),
            ("n/p", "page"),
            ("Esc", "back"),
        ],
    );
}

/// The under-the-counter trade: the fence, proxies, and the safe.
pub(super) fn black_market(pen: &mut Pen<'_, '_>, s: &Session, area: Rect) {
    let p = &s.p;
    let c = &*s.content;
    let h = &c.rig.hideout;
    let [body, note, foot] = rows(
        area,
        [
            Constraint::Min(8),
            Constraint::Length(1),
            Constraint::Length(2),
        ],
    );
    let [l, r] = cols(
        body,
        [Constraint::Percentage(55), Constraint::Percentage(45)],
    );
    let li = pen.panel(l, "THE FENCE", AMBER);
    let w = usize::from(li.width).saturating_sub(40).clamp(4, 20);
    let lines: Vec<Line<'static>> = c
        .rig
        .fence
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let mine = usize::from(p.fence) == i;
            let mut spans = vec![Span::styled(
                format!("{} {:<18}", if mine { ">" } else { " " }, clip(&f.name, 18)),
                if mine { bold(AMBER) } else { pen.ink.plain },
            )];
            spans.extend(bar(w, f64::from(f.rate) / 100.0, AMBER, false));
            spans.push(Span::styled(format!(" {:>3}%", f.rate), pen.ink.plain));
            if usize::from(p.fence) + 1 == i {
                spans.push(Span::styled(format!("  next {}", f.cost), fg(GREEN)));
            }
            Line::from(spans)
        })
        .collect();
    pen.text(li, lines);
    let [pr, sf] = rows(r, [Constraint::Length(6), Constraint::Min(4)]);
    let pi = pen.panel(pr, "PROXIES", CYAN);
    let mut spans = vec![Span::styled("chain  ", pen.ink.dim)];
    for i in 0..h.proxy_max {
        spans.push(if i < p.proxies {
            Span::styled("\u{25A0} ", fg(CYAN))
        } else {
            Span::styled("\u{25A1} ", fg(STEEL))
        });
    }
    pen.text(
        pi,
        vec![
            Line::from(spans),
            Line::styled(
                format!(
                    "{} marks each, soaks {} trace",
                    p.proxy_price(c),
                    h.proxy_buffer
                ),
                pen.ink.plain,
            ),
            Line::styled(
                if p.proxy_price(c) > h.proxy_cost {
                    "heat makes them dearer"
                } else {
                    ""
                },
                fg(RED),
            ),
        ],
    );
    let si = pen.panel(sf, "THE SAFE", GREEN);
    let total = p.marks + p.stash;
    let w = usize::from(si.width).saturating_sub(16).max(6);
    pen.text(
        si,
        vec![
            meter(
                "safe",
                5,
                p.stash,
                total.max(1),
                w,
                GREEN,
                &p.stash.to_string(),
                &pen.ink,
            ),
            Line::styled(
                format!("{} marks on hand; raids never touch the safe", p.marks),
                pen.ink.dim,
            ),
        ],
    );
    pen.notes(note, s);
    pen.hint(
        foot,
        &[
            ("f", "better fence"),
            ("p", "proxy"),
            ("s", "stash"),
            ("t", "take"),
            ("Esc", "back"),
        ],
    );
}

/// The rig: five parts with their levels and what each does.
pub(super) fn rig(pen: &mut Pen<'_, '_>, s: &Session, area: Rect) {
    let p = &s.p;
    let c = &*s.content;
    let [body, note, foot] = rows(
        area,
        [
            Constraint::Min(8),
            Constraint::Length(1),
            Constraint::Length(1),
        ],
    );
    let li = pen.panel(body, &format!("RIG  marks {}", p.marks), GREEN);
    let effects = [
        "+4 cycles a level",
        "+4 memory and a slot a level",
        "fewer fried parts when burned",
        "+5% script success a level",
        "6% less trace a level",
    ];
    let mut lines: Vec<Line<'static>> = Vec::new();
    for i in 0..PARTS {
        let part = c.rig.part.get(i);
        let lvl = usize::from(p.rig.get(i).copied().unwrap_or(0));
        let fried = p.fried & (1 << i) != 0;
        let now = if lvl == 0 {
            "stock".to_owned()
        } else {
            part.and_then(|d| d.levels.get(lvl - 1))
                .map_or("?".into(), |l| l.name.clone())
        };
        let mut spans = vec![
            Span::styled(format!("[{}] ", i + 1), pen.ink.accent),
            Span::styled(
                format!("{:<15}", part.map_or("?", |d| d.name.as_str())),
                if fried { bold(RED) } else { pen.ink.strong },
            ),
        ];
        spans.extend(super::town::pips_pub(
            lvl,
            part.map_or(3, |d| d.levels.len()),
            if fried { RED } else { GREEN },
        ));
        spans.push(Span::styled(
            format!(" {:<18}", clip(&now, 18)),
            pen.ink.plain,
        ));
        spans.push(match p.upgrade_cost(c, i) {
            Some(cost) => Span::styled(
                format!("next {cost}"),
                if p.marks >= cost {
                    fg(AMBER)
                } else {
                    pen.ink.dim
                },
            ),
            None => Span::styled("maxed", fg(GREEN)),
        });
        if fried {
            spans.push(Span::styled(
                format!("  FRIED, repair {}", p.repair_cost(c, i)),
                bold(RED),
            ));
        }
        lines.push(Line::from(spans));
        lines.push(Line::styled(
            format!("      {}", effects.get(i).copied().unwrap_or("")),
            pen.ink.dim,
        ));
    }
    pen.text(li, lines);
    pen.notes(note, s);
    pen.hint(
        foot,
        &[
            ("1-5", "upgrade"),
            ("x", "repair a fried part"),
            ("Esc", "back"),
        ],
    );
}

/// The hideout: bots, the workbench, the fence and the case of fragments.
pub(super) fn hideout(pen: &mut Pen<'_, '_>, s: &Session, area: Rect) {
    let p = &s.p;
    let c = &*s.content;
    let h = &c.rig.hideout;
    let [body, note, foot] = rows(
        area,
        [
            Constraint::Min(8),
            Constraint::Length(1),
            Constraint::Length(2),
        ],
    );
    let li = pen.panel(
        body,
        &format!("HIDEOUT  marks {} (safe {})", p.marks, p.stash),
        CYAN,
    );
    let mut bots = vec![Span::styled("bots       ", pen.ink.dim)];
    bots.extend(super::town::pips_pub(
        usize::from(p.bots),
        usize::from(h.bot_max),
        GREEN,
    ));
    bots.push(Span::styled(
        format!("{} marks each a night", h.bot_income),
        pen.ink.plain,
    ));
    let lines = vec![
        Line::from(bots),
        Line::from(vec![
            Span::styled("workbench  ", pen.ink.dim),
            if p.workbench {
                Span::styled(
                    format!("built; {} fragment(s) in the case", p.fragments.len()),
                    fg(GREEN),
                )
            } else {
                Span::styled(
                    format!("not built ({} marks)", h.workbench_cost),
                    pen.ink.plain,
                )
            },
        ]),
        Line::from(vec![
            Span::styled("fence      ", pen.ink.dim),
            Span::styled(
                format!(
                    "{} at {} percent",
                    c.rig
                        .fence
                        .get(usize::from(p.fence))
                        .map_or("?", |f| f.name.as_str()),
                    c.fence_rate(p.fence)
                ),
                pen.ink.plain,
            ),
        ]),
        Line::raw(""),
        Line::styled(
            "Heat 50 and up brings raids; the safe is never touched.",
            pen.ink.dim,
        ),
    ];
    let [left, right] = cols(li, [Constraint::Min(34), Constraint::Length(30)]);
    pen.text(left, lines);
    // The case: each fragment you carry, by name.
    let case = pen.panel(right, "FRAGMENT CASE", VIOLET);
    let mut counts: Vec<(u8, usize)> = Vec::new();
    for n in &p.fragments {
        match counts.iter_mut().find(|(k, _)| k == n) {
            Some((_, c)) => *c += 1,
            None => counts.push((*n, 1)),
        }
    }
    let mut lines: Vec<Line<'static>> = counts
        .iter()
        .filter_map(|(n, k)| c.fragment(*n).map(|f| (f, *k)))
        .map(|(f, k)| {
            Line::from(vec![
                Span::styled(format!("{:<15}", clip(&f.name, 15)), pen.ink.plain),
                Span::styled(format!("x{k}"), pen.ink.dim),
            ])
        })
        .collect();
    if lines.is_empty() {
        lines.push(Line::styled(
            "empty: vaults and downed machines drop fragments",
            pen.ink.dim,
        ));
    }
    pen.wrapped(case, lines);
    pen.notes(note, s);
    pen.hint(
        foot,
        &[
            ("b", "bot"),
            ("w", "workbench"),
            ("c", "code"),
            ("f", "fence"),
            ("s", "safe +"),
            ("t", "take"),
            ("Esc", "back"),
        ],
    );
}

/// The module editor: three slots, the fragments you can put in the next one, the budgets, and a
/// preview of the script.
pub(super) fn code(pen: &mut Pen<'_, '_>, s: &Session, d: Draft, area: Rect) {
    let c = &*s.content;
    let [pipe, body, note, foot] = rows(
        area,
        [
            Constraint::Length(3),
            Constraint::Min(6),
            Constraint::Length(2),
            Constraint::Length(1),
        ],
    );
    // The pipeline: effect + target + modifier => script.
    let frag_name = |n: u8| {
        if n == 0 {
            "-".to_owned()
        } else {
            c.fragment(n).map_or("?".to_owned(), |f| f.name.clone())
        }
    };
    let slots = [
        ("EFFECT", d.build.effect, d.step == CodeStep::Effect),
        ("TARGET", d.build.target, d.step == CodeStep::Target),
        ("MODIFIER", d.build.modifier, d.step == CodeStep::Modifier),
    ];
    let [a, plus1, b, plus2, m, arrow, out] = cols(
        pipe,
        [
            Constraint::Percentage(20),
            Constraint::Length(3),
            Constraint::Percentage(20),
            Constraint::Length(3),
            Constraint::Percentage(20),
            Constraint::Length(4),
            Constraint::Min(10),
        ],
    );
    for (rect, (title, num, active)) in [a, b, m].into_iter().zip(slots) {
        let inner = pen.panel(rect, title, if active { CYAN } else { STEEL });
        pen.text(
            inner,
            vec![Line::styled(
                clip(&frag_name(num), usize::from(inner.width)),
                if active {
                    bold(CYAN)
                } else if num != 0 {
                    bold(GREEN)
                } else {
                    pen.ink.dim
                },
            )],
        );
    }
    for rect in [plus1, plus2] {
        pen.text(
            Rect::new(rect.x, rect.y + 1, rect.width, 1),
            vec![Line::styled(" + ", pen.ink.dim)],
        );
    }
    pen.text(
        Rect::new(arrow.x, arrow.y + 1, arrow.width, 1),
        vec![Line::styled("=>", bold(AMBER))],
    );
    let oi = pen.panel(out, "SCRIPT", GREEN);
    let name = if d.build.effect == 0 {
        "-".to_owned()
    } else {
        code::name(c, d.build)
    };
    pen.text(
        oi,
        vec![Line::styled(
            clip(&name, usize::from(oi.width)),
            bold(GREEN),
        )],
    );

    let [l, r] = cols(
        body,
        [Constraint::Percentage(55), Constraint::Percentage(45)],
    );
    let kind = match d.step {
        CodeStep::Effect | CodeStep::Confirm => FragKind::Effect,
        CodeStep::Target => FragKind::Target,
        CodeStep::Modifier => FragKind::Modifier,
    };
    let title = match d.step {
        CodeStep::Effect => "PICK AN EFFECT",
        CodeStep::Target => "PICK A TARGET TYPE (0 for none)",
        CodeStep::Modifier => "PICK A MODIFIER (0 for none)",
        CodeStep::Confirm => "FRAGMENTS IN THE CASE",
    };
    let li = pen.panel(l, title, CYAN);
    let frags = s.fragment_rows(kind);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if frags.is_empty() {
        lines.push(Line::styled(
            "none in the case: find fragments on runs.",
            pen.ink.dim,
        ));
    }
    for (i, (num, n)) in frags.iter().enumerate().take(9) {
        if let Some(f) = c.fragment(*num) {
            lines.push(Line::from(vec![
                Span::styled(format!("{}) ", i + 1), pen.ink.accent),
                Span::styled(format!("{:<14}", clip(&f.name, 14)), pen.ink.plain),
                Span::styled(format!("x{n} "), pen.ink.dim),
                Span::styled(
                    format!("p{:+} m{:+} n{:+}", f.power, f.memory, f.noise),
                    fg(AMBER),
                ),
            ]));
            if pen.size != Size::Small {
                lines.push(Line::styled(
                    format!(
                        "   {}",
                        clip(&f.blurb, usize::from(li.width).saturating_sub(4))
                    ),
                    pen.ink.dim,
                ));
            }
        }
    }
    pen.text(li, lines);
    let ri = pen.panel(r, "PREVIEW", GREEN);
    let w = usize::from(ri.width).saturating_sub(22).clamp(4, 16);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if d.build.effect == 0 {
        lines.push(Line::styled(
            "pick an effect fragment to begin",
            pen.ink.dim,
        ));
    } else {
        match code::stats(c, d.build) {
            Ok(st) => {
                let over_m = st.memory > MEMORY_BUDGET;
                let over_n = st.noise > NOISE_BUDGET;
                lines.push(meter(
                    "power",
                    7,
                    st.power,
                    9,
                    w,
                    GREEN,
                    &st.power.to_string(),
                    &pen.ink,
                ));
                lines.push(meter(
                    "memory",
                    7,
                    st.memory,
                    MEMORY_BUDGET,
                    w,
                    if over_m { RED } else { CYAN },
                    &format!(
                        "{}/{MEMORY_BUDGET}{}",
                        st.memory,
                        if over_m { " OVER" } else { "" }
                    ),
                    &pen.ink,
                ));
                lines.push(meter(
                    "noise",
                    7,
                    st.noise,
                    NOISE_BUDGET,
                    w,
                    if over_n { RED } else { AMBER },
                    &format!(
                        "{}/{NOISE_BUDGET}{}",
                        st.noise,
                        if over_n { " OVER" } else { "" }
                    ),
                    &pen.ink,
                ));
                lines.push(Line::styled(
                    format!(
                        "cycles {}  charges {}",
                        st.cycles,
                        if st.charges == 0 {
                            "unlimited".to_owned()
                        } else {
                            st.charges.to_string()
                        }
                    ),
                    pen.ink.dim,
                ));
            }
            Err(why) => lines.push(Line::styled(why, bold(RED))),
        }
        if d.step == CodeStep::Confirm {
            match code::assemble(c, d.build, code::CODED_BASE) {
                Ok(sc) => {
                    lines.push(Line::raw(""));
                    lines.push(Line::styled(
                        format!(
                            "Ready: {}{}",
                            sc.name,
                            sc.target.map_or_else(String::new, |t| format!(
                                ", tuned to {}",
                                crate::session::plural(t)
                            ))
                        ),
                        bold(GREEN),
                    ));
                    lines.push(Line::styled(
                        "A corporation learns a recipe, so vary it.",
                        pen.ink.dim,
                    ));
                }
                Err(why) => lines.push(Line::styled(why, bold(RED))),
            }
        }
    }
    pen.wrapped(ri, lines);
    pen.notes(note, s);
    let hint: &[(&str, &str)] = match d.step {
        CodeStep::Confirm => &[("Enter", "code it"), ("Esc", "back")],
        CodeStep::Effect => &[("1-9", "pick"), ("Esc", "back")],
        _ => &[("1-9", "pick"), ("0", "none"), ("Esc", "back")],
    };
    pen.hint(foot, hint);
    let _ = (CODE_CYCLES, Family::ALL);
}
