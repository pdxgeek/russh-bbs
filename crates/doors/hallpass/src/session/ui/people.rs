//! People: conversations (a speaker's portrait, what they say, and the answers, the shut ones
//! saying what they want), clubs (cards with ranks and the week's meetings) and the journal
//! (friendships on a bond meter, and where each storyline stands).

use russh_bbs_door::DoorFrame;
use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Color, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Paragraph, Widget, Wrap};

use super::plan::bond_line;
use super::{Size, Ui, fit, hue, wrap};
use crate::game::{Action, Game};
use crate::session::Cmd;

// ----------------------------------------------------------------------------------- portrait

/// A small procedural portrait: skin, hair and clothes picked from the character's id, drawn
/// as half blocks. The same id always gives the same face.
struct Portrait {
    skin: Color,
    hair: Color,
    cloth: Color,
    hair_style: u8,
}

fn hash(id: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in id.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

const SKIN: [(u8, u8, u8); 5] = [
    (250, 200, 160),
    (230, 170, 120),
    (190, 130, 90),
    (140, 90, 60),
    (250, 220, 190),
];
const HAIR: [(u8, u8, u8); 6] = [
    (40, 30, 30),
    (120, 70, 30),
    (210, 170, 70),
    (170, 60, 40),
    (200, 200, 200),
    (50, 60, 90),
];

impl Portrait {
    fn of(id: &str, cloth: Color) -> Self {
        let h = hash(id);
        let s = SKIN[(h % 5) as usize];
        let hr = HAIR[((h >> 3) % 6) as usize];
        Self {
            skin: Color::Rgb(s.0, s.1, s.2),
            hair: Color::Rgb(hr.0, hr.1, hr.2),
            cloth,
            hair_style: ((h >> 8) % 3) as u8,
        }
    }

    /// The colour of the pixel at (x, y) on an 11 by 12 grid, if any.
    fn pixel(&self, x: i32, y: i32) -> Option<Color> {
        let head = (3..=7).contains(&x) && (2..=8).contains(&y);
        let hair = match self.hair_style {
            0 => (2..=8).contains(&x) && (1..=3).contains(&y) && !(y == 3 && (4..=6).contains(&x)),
            1 => {
                ((2..=8).contains(&x) && (1..=3).contains(&y))
                    || ((x == 2 || x == 8) && (3..=9).contains(&y))
            }
            _ => (3..=7).contains(&x) && y == 2,
        };
        let eyes = y == 5 && (x == 4 || x == 6);
        let mouth = y == 7 && (4..=6).contains(&x);
        let body = (1..=9).contains(&x) && y >= 9;
        if eyes {
            Some(Color::Rgb(20, 20, 30))
        } else if mouth {
            Some(Color::Rgb(160, 60, 60))
        } else if hair {
            Some(self.hair)
        } else if head {
            Some(self.skin)
        } else if body {
            Some(self.cloth)
        } else {
            None
        }
    }
}

impl Widget for Portrait {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for row in 0..6i32 {
            for col in 0..11i32 {
                let (Ok(dx), Ok(dy)) = (u16::try_from(col), u16::try_from(row)) else {
                    continue;
                };
                if dx >= area.width || dy >= area.height {
                    continue;
                }
                let top = self.pixel(col, row * 2);
                let bottom = self.pixel(col, row * 2 + 1);
                let Some(cell) = buf.cell_mut((area.x + dx, area.y + dy)) else {
                    continue;
                };
                match (top, bottom) {
                    (None, None) => {}
                    (Some(t), None) => {
                        cell.set_symbol("▀").set_style(Style::new().fg(t));
                    }
                    (None, Some(b)) => {
                        cell.set_symbol("▄").set_style(Style::new().fg(b));
                    }
                    (Some(t), Some(b)) => {
                        cell.set_symbol("▀").set_style(Style::new().fg(t).bg(b));
                    }
                }
            }
        }
    }
}

/// The ASCII bust for terminals without colour or in the ASCII tier.
const BUST: [&str; 6] = [
    "   .-\"\"-.   ",
    "  / ____ \\  ",
    "  | o  o |  ",
    "  |  --  |  ",
    "   \\____/   ",
    "  /|    |\\  ",
];

fn draw_portrait(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, id: &str, authority: bool) {
    if ui.look.ascii || !ui.look.color {
        let lines: Vec<Line<'static>> = BUST
            .iter()
            .map(|l| Line::styled((*l).to_owned(), ui.pal.chalk_soft()))
            .collect();
        f.render_widget(Paragraph::new(lines), area);
    } else {
        let cloth = if authority {
            Color::Rgb(60, 60, 80)
        } else {
            hue::INK
        };
        f.render_widget(Portrait::of(id, cloth), area);
    }
}

// ----------------------------------------------------------------------------------- scenes

/// A conversation: who, what they say, what you can answer.
pub(super) fn scene(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let Some(view) = g.scene_view() else {
        return;
    };
    let npc = g
        .player
        .life
        .scene
        .as_ref()
        .and_then(|sc| g.content.talks.iter().find(|t| t.id == sc.talk))
        .and_then(|t| g.content.npc(&t.npc));
    let choice_rows = u16::try_from(view.choices.len()).unwrap_or(3) + 3;
    let [top, answers] = Layout::vertical([
        Constraint::Min(5),
        Constraint::Length(choice_rows.min(area.height.saturating_sub(5))),
    ])
    .areas(area);
    let narrow = top.width < 60;
    let [who_area, say_area] = if narrow {
        Layout::horizontal([Constraint::Length(14), Constraint::Min(20)]).areas(top)
    } else {
        Layout::horizontal([Constraint::Length(18), Constraint::Min(30)]).areas(top)
    };
    // ---- the speaker.
    let inner = ui.framed(f, who_area, "");
    if let Some(n) = npc {
        let authority = n.is_authority();
        if inner.height >= 6 {
            draw_portrait(
                ui,
                f,
                Rect::new(
                    inner.x + (inner.width.saturating_sub(11)) / 2,
                    inner.y,
                    11.min(inner.width),
                    6,
                ),
                &n.id,
                authority,
            );
        }
        let mut lines: Vec<Line<'static>> = Vec::new();
        let used = if inner.height >= 6 { 6 } else { 0 };
        lines.push(Line::styled(
            fit(
                n.name.split_whitespace().last().unwrap_or(&n.name),
                usize::from(inner.width),
            ),
            pal.chalk(),
        ));
        lines.push(Line::styled(
            fit(&n.title, usize::from(inner.width)),
            pal.ink(),
        ));
        if inner.height > used + 4 {
            lines.push(Line::styled(
                format!("{} {}", g.regard(&n.id).word().to_uppercase(), ""),
                pal.moss(),
            ));
        }
        let r = Rect::new(
            inner.x,
            inner.y + used,
            inner.width,
            inner.height.saturating_sub(used),
        );
        f.render_widget(Paragraph::new(lines), r);
    }
    // ---- what they say.
    let inner = ui.framed(f, say_area, &view.title);
    let w = usize::from(inner.width);
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::styled(view.who.clone(), pal.ink()));
    for l in wrap(&view.text, w) {
        lines.push(Line::styled(l, pal.chalk_soft()));
    }
    if let Some(n) = npc {
        lines.push(Line::default());
        lines.push(bond_line(ui, g.bond(&n.id), inner.width));
        // What you know of them, when there is room.
        if inner.height >= 9 {
            lines.push(Line::default());
            for l in wrap(&n.blurb, w) {
                lines.push(Line::styled(l, pal.slate()));
            }
            if let Some(clique) = n
                .clique
                .as_deref()
                .and_then(|c| g.content.cliques.iter().find(|x| x.id == c))
            {
                lines.push(Line::from(vec![
                    Span::styled(format!("{} ", clique.name), pal.ink()),
                    Span::styled(
                        format!("(your standing {})", g.player.standing_with(&clique.id)),
                        pal.slate(),
                    ),
                ]));
            }
            let wary = g.player.wary_of(&n.id);
            if wary > 0 {
                let mut spans = vec![Span::styled("wary of you ", pal.slate())];
                spans.extend(ui.pips(usize::try_from(wary).unwrap_or(0), 3, pal.red()));
                lines.push(Line::from(spans));
            }
        }
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
    // ---- the answers.
    let frame = ui.framed(f, answers, "You say");
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut item_at = 0usize;
    let cursor = ui.cursor();
    for (i, choice) in view.choices.iter().enumerate() {
        let key = char::from(b'1' + u8::try_from(i).unwrap_or(0));
        let room = usize::from(frame.width).saturating_sub(6);
        if choice.open {
            let on = item_at == cursor;
            item_at += 1;
            let text = fit(&choice.label, room);
            if on {
                let pad = usize::from(frame.width).saturating_sub(5 + text.chars().count());
                rows.push(Line::styled(
                    format!(
                        " {key} {} {text}{}",
                        ui.look.g("▸", ">"),
                        " ".repeat(pad.saturating_sub(2))
                    ),
                    pal.bar_on(),
                ));
            } else {
                rows.push(Line::from(vec![
                    Span::styled(format!(" {key}   "), pal.brass()),
                    Span::styled(text, pal.chalk_soft()),
                ]));
            }
        } else {
            let wants = format!("needs {}", choice.wants);
            let label_room = room.saturating_sub(wants.chars().count() + 3);
            rows.push(Line::from(vec![
                Span::styled(format!(" {}   ", ui.look.g("╳", "x")), pal.slate()),
                Span::styled(fit(&choice.label, label_room), pal.slate()),
                Span::styled(format!("  {wants}"), pal.red()),
            ]));
        }
    }
    let leave_on = item_at == cursor;
    rows.push(if leave_on {
        Line::styled(
            format!(" x {} excuse yourself", ui.look.g("▸", ">")),
            pal.bar_on(),
        )
    } else {
        Line::from(vec![
            Span::styled(" x   ", pal.brass()),
            Span::styled("excuse yourself", pal.slate()),
        ])
    });
    f.render_widget(Paragraph::new(rows), frame);
}

// ----------------------------------------------------------------------------------- clubs

/// The week as `M T W T F S S`, the meeting days lit and today marked.
fn week(ui: &Ui<'_>, days: &[u8]) -> Vec<Span<'static>> {
    let pal = &ui.pal;
    let today = Game::weekday(ui.g.player.day);
    let mut out = Vec::new();
    for (i, name) in ["M", "T", "W", "T", "F", "S", "S"].iter().enumerate() {
        let meets = days.iter().any(|d| usize::from(*d) == i);
        let style = match (meets, i == today) {
            (true, true) => pal.lamp(),
            (true, false) => pal.moss(),
            (false, true) => pal.chalk_soft(),
            (false, false) => pal.slate(),
        };
        let glyph = if meets && ui.look.ascii {
            name.to_lowercase()
        } else {
            (*name).to_owned()
        };
        out.push(Span::styled(glyph, style));
        out.push(Span::raw(" "));
    }
    out
}

/// The clubs screen.
pub(super) fn clubs(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let [list, detail] = if area.width >= 100 {
        Layout::horizontal([Constraint::Percentage(58), Constraint::Percentage(42)]).areas(area)
    } else {
        Layout::vertical([Constraint::Min(6), Constraint::Length(5)]).areas(area)
    };
    // The item for each club, if it has one.
    let item_for = |club: &str| -> Option<(usize, char)> {
        ui.items
            .iter()
            .enumerate()
            .find_map(|(i, it)| match &it.cmd {
                Cmd::Do(Action::Join(c) | Action::Quit(c)) if c == club => Some((i, it.key)),
                _ => None,
            })
    };
    let sel = ui.s.tab % g.content.clubs.len().max(1);
    let inner = ui.framed(
        f,
        list,
        &format!("Clubs (up to {})", g.content.rules.max_clubs),
    );
    let mut lines: Vec<Line<'static>> = Vec::new();
    for (n, club) in g.content.clubs.iter().enumerate() {
        let rank = g.player.life.rank_in(&club.id);
        let it = item_for(&club.id);
        let on = n == sel;
        let marker = if on { ui.look.g("▸", ">") } else { " " };
        let key = it.map_or(' ', |(_, k)| k);
        let name = club.name.trim_start_matches("The ");
        let mut spans = vec![
            Span::styled(
                format!("{marker}{key} "),
                if on { pal.lamp() } else { pal.brass() },
            ),
            Span::styled(
                format!("{:<16}", fit(name, 16)),
                if on { pal.chalk() } else { pal.chalk_soft() },
            ),
        ];
        if rank > 0 {
            let rank_name = club
                .ranks
                .get(usize::try_from(rank - 1).unwrap_or(0))
                .map_or("", |r| r.0.as_str());
            spans.extend(ui.pips(
                usize::try_from(rank).unwrap_or(0),
                club.ranks.len(),
                pal.lamp(),
            ));
            spans.push(Span::styled(format!(" {rank_name:<7}"), pal.moss()));
        } else if g.join_block(club).is_some() {
            spans.push(Span::styled("closed", pal.red()));
        } else {
            spans.push(Span::styled("open", pal.moss()));
        }
        // The week, right-aligned if there is room.
        let week_spans = week(ui, &club.days);
        let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
        let ww = 14usize;
        if used + ww < usize::from(inner.width) {
            spans.push(Span::raw(" ".repeat(usize::from(inner.width) - used - ww)));
            spans.extend(week_spans);
        }
        lines.push(Line::from(spans));
    }
    // Back, and whatever else is on the menu.
    for it in &ui.items {
        if matches!(it.cmd, Cmd::Do(Action::Join(_) | Action::Quit(_))) {
            continue;
        }
        lines.push(Line::from(vec![
            Span::styled(format!("  {} ", it.key), pal.brass()),
            Span::styled(it.label.clone(), pal.slate()),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
    // ---- the club under the cursor.
    let inner = ui.framed(f, detail, "The club");
    let Some(club) = g.content.clubs.get(sel) else {
        return;
    };
    let w = usize::from(inner.width);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for l in wrap(&club.blurb, w)
        .into_iter()
        .take(if ui.look.size == Size::Tiny { 1 } else { 3 })
    {
        lines.push(Line::styled(l, pal.chalk_soft()));
    }
    let place = g
        .content
        .place(&club.place)
        .map_or("?", |p| p.name.as_str());
    let advisor = g.content.npc(&club.advisor).map_or("", |n| n.name.as_str());
    lines.push(Line::from(vec![
        Span::styled("meets ", pal.slate()),
        Span::styled(format!("{place}, advisor {advisor}"), pal.ink()),
    ]));
    let rank = g.player.life.rank_in(&club.id);
    if rank > 0 {
        let merit = g
            .player
            .life
            .clubs
            .iter()
            .find(|m| m.club == club.id)
            .map_or(0, |m| i32::from(m.merit));
        let next = club
            .ranks
            .get(usize::try_from(rank).unwrap_or(usize::MAX))
            .map(|r| r.1);
        lines.push(match next {
            Some(need) => ui.gauge(
                "merit",
                u32::try_from(merit.max(0)).unwrap_or(0),
                u32::try_from(need.max(1)).unwrap_or(1),
                inner.width,
                pal.lamp(),
                &format!("{merit}/{need}"),
            ),
            None => Line::styled(format!("merit {merit}: top rank"), pal.lamp()),
        });
    } else if let Some(why) = g.join_block(club) {
        for l in wrap(&why, w).into_iter().take(2) {
            lines.push(Line::styled(l, pal.red()));
        }
    } else {
        lines.push(Line::styled("open to you: press Enter to join", pal.moss()));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

// ----------------------------------------------------------------------------------- journal

/// The journal: friendships and the storylines.
pub(super) fn journal(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let g = ui.g;
    let pal = &ui.pal;
    let [back, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(4)]).areas(area);
    ui.chips(f, back, false);
    let [stories, friends] = if body.width >= 90 {
        Layout::horizontal([Constraint::Percentage(46), Constraint::Percentage(54)]).areas(body)
    } else {
        Layout::horizontal([Constraint::Percentage(48), Constraint::Percentage(52)]).areas(body)
    };
    // ---- storylines.
    let inner = ui.framed(f, stories, "Stories");
    let mut lines: Vec<Line<'static>> = Vec::new();
    if g.player.life.arcs.is_empty() {
        lines.push(Line::styled(
            "Nothing has begun yet. Talk to people; listen.",
            pal.slate(),
        ));
    }
    for p in &g.player.life.arcs {
        let Some(spec) = g.content.arc(&p.id) else {
            continue;
        };
        let at = spec.step.iter().position(|s| s.id == p.step).unwrap_or(0);
        let mut head = vec![Span::styled(
            format!("{} ", fit(&spec.name, 22)),
            pal.chalk(),
        )];
        head.extend(ui.pips(
            if p.ended.is_some() {
                spec.step.len()
            } else {
                at
            },
            spec.step.len(),
            if p.ended.is_some() {
                pal.moss()
            } else {
                pal.lamp()
            },
        ));
        lines.push(Line::from(head));
        let text = match &p.ended {
            Some(end) => spec
                .ending
                .iter()
                .find(|e| e.id == *end)
                .map_or(String::new(), |e| format!("over: {}", e.name)),
            None => spec
                .step
                .iter()
                .find(|s| s.id == p.step)
                .map_or(String::new(), |s| s.text.clone()),
        };
        for l in wrap(&text, usize::from(inner.width).saturating_sub(2))
            .into_iter()
            .take(3)
        {
            lines.push(Line::styled(format!(" {l}"), pal.chalk_soft()));
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
    // ---- friendships.
    let inner = ui.framed(f, friends, "Friends and rivals");
    let mut bonds: Vec<(String, i32)> = g
        .player
        .life
        .bonds
        .iter()
        .map(|(id, v)| (id.clone(), i32::from(*v)))
        .collect();
    bonds.sort_by_key(|(_, v)| -*v);
    let mut lines: Vec<Line<'static>> = Vec::new();
    if bonds.is_empty() {
        lines.push(Line::styled("You know nobody yet.", pal.slate()));
    }
    let rows = usize::from(inner.height);
    for (id, v) in bonds.iter().take(rows) {
        let Some(n) = g.content.npc(id) else {
            continue;
        };
        let mut l = bond_line(ui, *v, inner.width);
        if let Some(first) = l.spans.first_mut() {
            let last = n.name.split_whitespace().last().unwrap_or(&n.name);
            *first = Span::styled(format!("{:<7}", fit(last, 7)), pal.chalk_soft());
        }
        lines.push(l);
    }
    f.render_widget(Paragraph::new(lines), inner);
}
