//! The day: the ticking timetable strip every screen wears, and the day view itself (where you
//! are, who is here, how hot things are, and the orders you can give). Also the study and tuck
//! shop lists, which are short.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::Paragraph;

use super::{Size, Ui, fit, hue, wrap};
use crate::game::Game;
use crate::state::map_get;

/// The short name of a period's activity for the strip.
pub(super) fn slot_label(g: &Game, day: i64, slot: usize) -> String {
    let act = g.activity(day, slot);
    if let Some(sub) = act.strip_prefix("class:") {
        return match sub {
            "maths" => "Maths".to_owned(),
            "sciences" => "Sci".to_owned(),
            "languages" => "Lang".to_owned(),
            "history" => "Hist".to_owned(),
            "art" => "Art".to_owned(),
            "games" => "Games".to_owned(),
            other => g.content.subject(other).map_or_else(
                || other.chars().take(4).collect(),
                |s| s.name.chars().take(5).collect(),
            ),
        };
    }
    match act {
        "lunch" => "Lunch",
        "free" => "Free",
        "after" => "After",
        "evening" => "Eve",
        "night" => "Night",
        "chapel" => "Chapel",
        other => return other.chars().take(5).collect(),
    }
    .to_owned()
}

/// A colour a fraction of the way from `a` to `b` (0 to 255 for the fraction).
fn lerp(a: (u8, u8, u8), b: (u8, u8, u8), t: u32) -> Color {
    let mix = |x: u8, y: u8| {
        u8::try_from((u32::from(x) * (255 - t) + u32::from(y) * t) / 255).unwrap_or(255)
    };
    Color::Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// The sky for a point `x` of `w` across the day: dawn gold, noon white, dusk red, night ink.
fn sky(x: u32, w: u32) -> Color {
    let t = (x * 255) / w.max(1);
    match t {
        0..=84 => lerp((255, 214, 90), (236, 238, 232), t * 3),
        85..=169 => lerp((236, 238, 232), (222, 90, 60), (t - 85) * 3),
        _ => lerp((222, 90, 60), (40, 56, 190), (t - 170) * 3),
    }
}

/// The timetable strip: the day's periods across the top, the one now lit, the ones gone
/// dimmed, and under them the day's sky with a marker that ticks along as the periods pass.
pub(super) fn strip(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let p = &ui.pal;
    let n = g.periods().max(1);
    let day = g.player.day;
    let now = g.current_slot();
    let w = usize::from(area.width);
    let mut labels: Vec<Span<'static>> = Vec::new();
    let mut marks: Vec<Span<'static>> = Vec::new();
    let mut sky_row: Vec<Span<'static>> = Vec::new();
    for slot in 0..n {
        // Spread the leftover columns over the first cells.
        let start = slot * w / n;
        let end = (slot + 1) * w / n;
        let cw = end - start;
        let label = slot_label(g, day, slot);
        let past = now.is_none_or(|c| slot < c);
        let here = now == Some(slot);
        let text = if here {
            format!("{}{label}{}", ui.look.g("▐", "["), ui.look.g("▌", "]"))
        } else {
            label
        };
        let pad = cw.saturating_sub(text.chars().count());
        let (l, r) = (pad / 2, pad - pad / 2);
        let style = if here {
            p.bar_on()
        } else if past {
            p.slate()
        } else {
            p.chalk_soft()
        };
        let cell = format!("{}{}{}", " ".repeat(l), fit(&text, cw), " ".repeat(r));
        // The lit cell is only its text; the padding keeps the ink.
        if here {
            labels.push(Span::styled(" ".repeat(l), p.slate()));
            labels.push(Span::styled(fit(&text, cw), style));
            labels.push(Span::styled(" ".repeat(r), p.slate()));
        } else {
            labels.push(Span::styled(cell, style));
        }
        // The second row: a rule under each period, lit on the current one.
        let (glyph, mstyle) = if here {
            (ui.look.g("▀", "="), p.lamp())
        } else if past {
            (ui.look.g("━", "-"), p.fg_or(hue::INK_DEEP, p.dim))
        } else {
            (ui.look.g("┄", "."), p.slate())
        };
        marks.push(Span::styled(glyph.repeat(cw), mstyle));
        // The third row: the sky.
        for x in start..end {
            let col = u32::try_from(x).unwrap_or(0);
            let cols = u32::try_from(w).unwrap_or(1);
            let (sym, style) = if p_past(now, slot) {
                (ui.look.g("▒", "-"), p.fg_or_color(sky(col, cols), p.dim))
            } else if here {
                (ui.look.g("█", "#"), p.fg_or_color(sky(col, cols), p.lamp()))
            } else {
                (
                    ui.look.g("░", "."),
                    p.fg_or_color(sky(col, cols), p.slate()),
                )
            };
            sky_row.push(Span::styled(sym, style));
        }
    }
    let mut lines = vec![Line::from(labels), Line::from(marks)];
    if area.height >= 3 {
        lines.push(Line::from(sky_row));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// Whether `slot` is already over.
fn p_past(now: Option<usize>, slot: usize) -> bool {
    now.is_none_or(|c| slot < c)
}

impl super::Pal {
    /// A foreground colour when colour is on; `fallback` when it is off.
    pub(super) fn fg_or_color(&self, color: Color, fallback: Style) -> Style {
        self.fg_or(color, fallback)
    }
}

/// How hot things are, in the file's red.
fn heat_style(ui: &Ui<'_>, heat: u8) -> Style {
    if heat >= 20 {
        ui.pal.red().add_modifier(Modifier::BOLD)
    } else if heat >= 8 {
        ui.pal.brass()
    } else {
        ui.pal.moss()
    }
}

/// The vitals as gauges: heat, the file, nerve and the purse.
pub(super) fn vitals(ui: &Ui<'_>, width: u16) -> Vec<Line<'static>> {
    let g = ui.g;
    let p = &g.player;
    let pal = &ui.pal;
    let mut out = Vec::new();
    out.push(ui.gauge(
        "heat",
        u32::from(p.heat),
        30,
        width,
        heat_style(ui, p.heat),
        &format!("{}", p.heat),
    ));
    let full = u32::try_from(g.content.rules.file_full.max(1)).unwrap_or(1);
    let pts = u32::try_from(p.file_points().max(0)).unwrap_or(0);
    out.push(ui.gauge(
        "file",
        pts,
        full,
        width,
        if pts * 2 >= full {
            pal.red()
        } else {
            pal.brass()
        },
        &format!("{pts}/{full}"),
    ));
    let mut nerve = vec![Span::styled(format!("{:<7}", "nerve"), pal.slate())];
    nerve.extend(ui.pips(
        usize::from(p.nerve),
        usize::from(p.nerve_max),
        pal.fg_or(hue::INK, pal.accent),
    ));
    nerve.push(Span::styled(
        format!(" {}/{}", p.nerve, p.nerve_max),
        pal.chalk_soft(),
    ));
    out.push(Line::from(nerve));
    out.push(Line::from(vec![
        Span::styled(format!("{:<7}", "legend"), pal.slate()),
        Span::styled(format!("{}", p.legend), pal.lamp()),
        Span::styled(format!("   {} cash ", ui.look.g("◈", "$")), pal.slate()),
        Span::styled(format!("{}", p.cash), pal.brass()),
    ]));
    out
}

/// The warnings that belong on the day: detention owed, a summons, a final warning, a letter.
fn warnings(ui: &Ui<'_>) -> Vec<Line<'static>> {
    let p = &ui.g.player;
    let pal = &ui.pal;
    let mut out = Vec::new();
    if p.summons {
        out.push(Line::from(vec![
            Span::styled(format!(" {} SUMMONS ", ui.look.g("!", "!")), pal.stamp()),
            Span::styled(" the headmistress, tomorrow", pal.red()),
        ]));
    }
    if p.detention > 0 {
        out.push(Line::from(vec![
            Span::styled(" DETENTION ", pal.stamp()),
            Span::styled(format!(" {} owed", p.detention), pal.red()),
        ]));
    }
    if p.final_warning {
        out.push(Line::styled(" Final warning on your file.", pal.red()));
    }
    if p.letter {
        out.push(Line::styled(" A letter has gone home.", pal.brass()));
    }
    out
}

/// A character as the day names them: name, a mark for friend or rival.
pub(super) fn who(ui: &Ui<'_>, id: &str, short: bool) -> Option<Span<'static>> {
    let npc = ui.g.content.npc(id)?;
    let name = if short {
        npc.name
            .split_whitespace()
            .last()
            .unwrap_or(&npc.name)
            .to_owned()
    } else {
        npc.name.clone()
    };
    let (mark, style) = match ui.g.regard(id) {
        crate::talk::Regard::Friend => (ui.look.g(" +", " +"), ui.pal.moss()),
        crate::talk::Regard::Rival => (ui.look.g(" !", " !"), ui.pal.red()),
        crate::talk::Regard::Betrayed => (ui.look.g(" x", " x"), ui.pal.red()),
        crate::talk::Regard::Neutral => ("", ui.pal.chalk_soft()),
    };
    let authority = npc.is_authority();
    let style = if authority && mark.is_empty() {
        ui.pal.brass()
    } else {
        style
    };
    Some(Span::styled(format!("{name}{mark}"), style))
}

/// The people in `place` now, as spans separated by commas.
pub(super) fn people_line(ui: &Ui<'_>, place: &str, short: bool) -> Vec<Span<'static>> {
    let g = ui.g;
    let Some(slot) = g.current_slot() else {
        return Vec::new();
    };
    let mut spans: Vec<Span<'static>> = Vec::new();
    for npc in g.npcs_at(place, g.player.day, slot) {
        if let Some(span) = who(ui, &npc.id, short) {
            if !spans.is_empty() {
                spans.push(Span::styled(", ", ui.pal.slate()));
            }
            spans.push(span);
        }
    }
    spans
}

/// The place panel: where you are, what it looks like, who is here, what the period is.
fn here_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let place = g.content.place(&g.player.place);
    let name = place.map_or("?", |p| p.name.as_str());
    let inner = ui.framed(f, area, name);
    let w = usize::from(inner.width);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let wing = place.map_or("", |p| p.wing.as_str());
    lines.push(Line::from(vec![
        Span::styled(format!("{} wing", capitalise(wing)), pal.ink()),
        Span::styled(
            if g.patrol_level(&g.player.place) > 0 {
                format!("  {} patrolled", ui.look.g("†", "+"))
            } else {
                String::new()
            },
            pal.red(),
        ),
    ]));
    let look_rows = match ui.look.size {
        Size::Tiny => 3,
        _ => 4,
    };
    for l in wrap(&g.describe_place(&g.player.place), w)
        .into_iter()
        .take(look_rows)
    {
        lines.push(Line::styled(l, pal.chalk_soft()));
    }
    if let Some(slot) = g.current_slot() {
        let here = people_line(ui, &g.player.place, ui.look.size == Size::Tiny);
        if here.is_empty() {
            lines.push(Line::styled("Nobody else is here.", pal.slate()));
        } else {
            let mut spans = vec![Span::styled("Here: ", pal.slate())];
            spans.extend(here);
            lines.push(Line::from(spans));
        }
        let act = g.activity(g.player.day, slot);
        let this = match act.strip_prefix("class:") {
            Some(s) => format!(
                "{} class",
                g.content.subject(s).map_or(s, |x| x.name.as_str())
            ),
            None => act.to_owned(),
        };
        lines.push(Line::from(vec![
            Span::styled("Now: ", pal.slate()),
            Span::styled(this, pal.lamp()),
        ]));
        if slot + 1 < g.periods() {
            let next = g.activity(g.player.day, slot + 1);
            let room = next
                .strip_prefix("class:")
                .and_then(|s| g.content.subject(s))
                .and_then(|s| g.content.place(&s.room))
                .map(|p| format!(" in the {}", p.name))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled("Next: ", pal.slate()),
                Span::styled(
                    format!("{}{room}", slot_label(g, g.player.day, slot + 1)),
                    pal.chalk_soft(),
                ),
            ]));
        }
        // Who is next door: the corridors you could walk to with someone in them.
        let mut left = usize::from(inner.height).saturating_sub(lines.len() + 1);
        for (place, hops) in g.routes() {
            if left == 0 {
                break;
            }
            if hops > 1 {
                continue;
            }
            let who = people_line(ui, &place, true);
            if who.is_empty() {
                continue;
            }
            let name = g.content.place(&place).map_or("?", |p| p.name.as_str());
            let mut spans = vec![Span::styled(format!("{}: ", fit(name, 14)), pal.ink())];
            spans.extend(who);
            lines.push(Line::from(spans));
            left -= 1;
        }
    } else {
        lines.push(Line::styled(
            "Lights are out. Come back tomorrow.",
            pal.slate(),
        ));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// A word with its first letter capitalised.
pub(super) fn capitalise(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The panel of vitals and warnings.
fn you_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let inner = ui.framed(f, area, "You");
    let mut lines = vitals(ui, inner.width);
    lines.extend(warnings(ui));
    // The schemes in hand, with their state.
    let g = ui.g;
    if !g.player.schemes.is_empty() && usize::from(inner.height) > lines.len() + 1 {
        lines.push(Line::styled("schemes", ui.pal.slate()));
        for s in &g.player.schemes {
            let state = if g.player.run.as_ref().is_some_and(|r| r.scheme == s.id) {
                ("RUNNING", ui.pal.lamp())
            } else if g.readiness(s).ready() {
                ("READY", ui.pal.moss())
            } else {
                ("setup", ui.pal.brass())
            };
            let name = g.content.method(&s.method).map_or("?", |m| m.name.as_str());
            let room = usize::from(inner.width).saturating_sub(state.0.len() + 6);
            lines.push(Line::from(vec![
                Span::styled(format!(" #{} ", s.id), ui.pal.slate()),
                Span::styled(format!("{:<room$}", fit(name, room)), ui.pal.chalk_soft()),
                Span::styled(format!(" {}", state.0), state.1),
            ]));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The rest of today, period by period, with where the timetable puts you.
fn today_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let inner = ui.framed(f, area, "Today");
    let now = g.current_slot();
    let mut lines = Vec::new();
    for slot in 0..g.periods() {
        let act = g.activity(g.player.day, slot);
        let room = act
            .strip_prefix("class:")
            .and_then(|s| g.content.subject(s))
            .and_then(|s| g.content.place(&s.room))
            .map_or(String::new(), |p| format!("  {}", p.name));
        let name = g.content.periods.get(slot).map_or("?", |p| p.name.as_str());
        let past = now.is_none_or(|c| slot < c);
        let here = now == Some(slot);
        let style = if here {
            pal.bar_on()
        } else if past {
            pal.slate()
        } else {
            pal.chalk_soft()
        };
        let mark = if here {
            ui.look.g("▶", ">")
        } else if past {
            ui.look.g("✓", "x")
        } else {
            " "
        };
        let text = format!(
            " {mark} {name:<14}{}{room}",
            slot_label(g, g.player.day, slot)
        );
        lines.push(Line::styled(fit(&text, usize::from(inner.width)), style));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// Who is in the places next door, as far as you can see from where you stand.
fn nearby_panel(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let inner = ui.framed(f, area, "Nearby");
    let mut lines = Vec::new();
    for (place, hops) in g.routes() {
        if hops > 1 {
            continue;
        }
        let who = people_line(ui, &place, true);
        if who.is_empty() {
            continue;
        }
        let name = g.content.place(&place).map_or("?", |p| p.name.as_str());
        let mut spans = vec![Span::styled(format!("{name}: "), pal.ink())];
        spans.extend(who);
        lines.push(Line::from(spans));
        if lines.len() >= usize::from(inner.height) {
            break;
        }
    }
    if lines.is_empty() {
        lines.push(Line::styled("Nobody in sight.", pal.slate()));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The day view.
pub(super) fn draw(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let chip_rows = {
        let per_row = usize::from(area.width / 12).max(1);
        u16::try_from(ui.items.len().div_ceil(per_row))
            .unwrap_or(3)
            .min(4)
    };
    let [top, menu] =
        Layout::vertical([Constraint::Min(5), Constraint::Length(chip_rows)]).areas(area);
    match ui.look.size {
        Size::Tiny | Size::Compact => {
            let [a, b] =
                Layout::horizontal([Constraint::Percentage(52), Constraint::Percentage(48)])
                    .areas(top);
            here_panel(ui, f, a);
            you_panel(ui, f, b);
        }
        Size::Roomy => {
            let [a, b, c] = Layout::horizontal([
                Constraint::Percentage(36),
                Constraint::Percentage(30),
                Constraint::Percentage(34),
            ])
            .areas(top);
            here_panel(ui, f, a);
            let [b1, b2] = Layout::vertical([Constraint::Length(8), Constraint::Min(3)]).areas(b);
            you_panel(ui, f, b1);
            nearby_panel(ui, f, b2);
            today_panel(ui, f, c);
        }
        Size::Grand => {
            let [a, b, c] = Layout::horizontal([
                Constraint::Percentage(36),
                Constraint::Percentage(30),
                Constraint::Percentage(34),
            ])
            .areas(top);
            let [a1, a2] = Layout::vertical([Constraint::Length(12), Constraint::Min(3)]).areas(a);
            here_panel(ui, f, a1);
            nearby_panel(ui, f, a2);
            you_panel(ui, f, b);
            today_panel(ui, f, c);
        }
    }
    ui.chips(f, menu, true);
}

/// The study list: every subject with its grade bar.
pub(super) fn study(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)]).areas(area);
    ui.orders(f, left, "Study which subject?");
    let inner = ui.framed(f, right, "Grades");
    let mut lines = Vec::new();
    for s in &g.content.subjects {
        let grade = map_get(&g.player.grades, &s.id).copied().unwrap_or(0);
        let style = if i32::from(grade) >= g.content.rules.honour_roll {
            pal.moss()
        } else if i32::from(grade) < g.content.rules.poor_grade {
            pal.red()
        } else {
            pal.fg_or(hue::INK, pal.accent)
        };
        lines.push(ui.gauge(
            &fit(&s.name, 7),
            u32::from(grade),
            100,
            inner.width,
            style,
            &format!("{grade}"),
        ));
    }
    lines.push(Line::default());
    lines.push(Line::styled(
        format!(
            "Honour roll at {}. Under {} goes on the file on Friday.",
            g.content.rules.honour_roll, g.content.rules.poor_grade
        ),
        pal.slate(),
    ));
    f.render_widget(
        Paragraph::new(lines).wrap(russh_bbs_door::widgets::Wrap { trim: false }),
        inner,
    );
}

/// The tuck shop and its like: items with prices, and what you carry.
pub(super) fn shop(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).areas(area);
    ui.orders(f, left, "For sale");
    let inner = ui.framed(f, right, "Your pockets");
    let mut lines = vec![Line::from(vec![
        Span::styled("cash ", pal.slate()),
        Span::styled(format!("{}", g.player.cash), pal.brass()),
    ])];
    if g.player.inventory.is_empty() {
        lines.push(Line::styled("You carry nothing useful.", pal.slate()));
    }
    for (id, n) in &g.player.inventory {
        let name = g.content.item(id).map_or(id.as_str(), |x| x.name.as_str());
        lines.push(Line::from(vec![
            Span::styled(format!("{n:>2}x "), pal.brass()),
            Span::styled(name.to_owned(), pal.chalk_soft()),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
}
