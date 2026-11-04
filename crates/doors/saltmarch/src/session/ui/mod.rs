//! The Saltmarch screens (M4-S3): every screen drawn with ratatui widgets into the door's own
//! buffer (D104). The board cleans every cell as it composites the buffer, so nothing here can
//! reach the terminal; what this module owns is how the game looks.
//!
//! **No game rules live here.** Every number on a screen is read from the engine (`Clan`,
//! `Tide`, `Map`, `Reach`, `Fight`, `trip::fail_chance`, ...); the keys are the same menu
//! entries the simulation bots press (`Session::entries`).
//!
//! **Tiers and colour.** In the ASCII tier every glyph is chosen here as plain ASCII (the
//! framework would also turn box drawing and blocks into ASCII, but a symbol it does not know
//! would stay as it is). The Nerd Font tier draws what the Unicode tier draws: the board's
//! compositing neutralizes private-use glyphs in a door's cells (D104), so a door cannot use
//! them. Colour comes from the theme's roles (`DoorStyle`) plus a few Saltmarch accents (sea
//! greens, salt whites, peat browns), which the framework degrades to the member's depth; with
//! colour off every cue that colour carried is also a glyph or a word.
//!
//! **Sizes.** Every screen is laid out for 80x24 first and gets more at 100x30 and 132x43
//! ([`Size`]). A window smaller than [`MIN_COLS`] by [`MIN_ROWS`] gets the plain text screen.

// The screens are long straight runs of drawing code over cell coordinates (`x`, `y`, `w`,
// `h`), panels and styles; splitting them for length would scatter one picture over many
// functions, and the short names are the ones ratatui itself uses.
#![allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Block, BorderType, Paragraph, Widget};
use russh_bbs_door::{Canvas, DoorEnv, DoorFrame, DoorStyle, Tier};

use super::{Entry, MenuId, Mode, Session};
use crate::clan::Clan;
use crate::engine::Env;

mod art;
mod court;
mod crewscreen;
mod describe;
mod holdscreen;
mod mapscreen;
mod market;
mod naming;
mod plan;
mod report;
mod scene;
mod skirmish;
mod social;
mod tides;

use art::{Ground, landscape};
pub(super) use describe::describe;

/// The smallest window the screens are laid out for.
pub(super) const MIN_COLS: u16 = 60;
/// The fewest rows the screens are laid out for.
pub(super) const MIN_ROWS: u16 = 16;

/// How much room a screen has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Size {
    /// Smaller than 80 columns or 22 rows: the door's area inside an 80x24 window, once the
    /// board has taken its frame. One tide row, no spare rows.
    Tiny,
    /// 80x24 and anything smaller than the next step.
    Compact,
    /// 100x30 and up.
    Roomy,
    /// 132x40 and up.
    Grand,
}

/// What the screens know about the member's terminal.
#[derive(Debug, Clone, Copy)]
pub(super) struct Look {
    /// The ASCII icon tier: every glyph drawn is ASCII.
    pub ascii: bool,
    /// Whether colour is on. With it off, cues that colour carries are glyphs and words too.
    pub color: bool,
    /// The size class.
    pub size: Size,
}

impl Look {
    fn new(env: &DoorEnv, area: Rect) -> Self {
        let size = if area.width >= 132 && area.height >= 40 {
            Size::Grand
        } else if area.width >= 100 && area.height >= 30 {
            Size::Roomy
        } else if area.width < 78 || area.height < 22 {
            Size::Tiny
        } else {
            Size::Compact
        };
        Self {
            ascii: env.tier == Tier::Ascii,
            color: env.settings.color(),
            size,
        }
    }

    /// `rich` in the Unicode and Nerd Font tiers, `plain` (ASCII) in the ASCII tier.
    pub(super) const fn g(self, rich: &'static str, plain: &'static str) -> &'static str {
        if self.ascii { plain } else { rich }
    }

    /// The glyph for `level` (0 to 8) of a bar or sparkline.
    pub(super) const fn bar_glyph(self, level: usize) -> &'static str {
        const RICH: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
        const PLAIN: [&str; 9] = [" ", ".", ".", ":", "-", "=", "+", "*", "#"];
        let i = if level > 8 { 8 } else { level };
        if self.ascii { PLAIN[i] } else { RICH[i] }
    }
}

/// An RGB colour for a style, only when colour is on.
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// The Saltmarch accents: sea greens, salt whites and peat browns, and the water's blues.
pub(super) mod hue {
    use super::{Color, rgb};
    // Each accent is a saturated colour chosen so that the nearest of the sixteen terminal
    // colours (what the board composites to today, D104) is the one named in the comment, and
    // so that a truecolor terminal shows a pleasing version of the same thing.
    /// Sea green: cyan.
    pub(crate) const SEA: Color = rgb(40, 190, 190);
    /// Deep water: blue.
    pub(crate) const SEA_DEEP: Color = rgb(30, 60, 160);
    /// Salt white: light grey.
    pub(crate) const SALT: Color = rgb(235, 238, 232);
    /// Peat brown: yellow (brown on most terminals).
    pub(crate) const PEAT: Color = rgb(200, 130, 50);
    /// Reed: yellow.
    pub(crate) const REED: Color = rgb(210, 215, 60);
    /// Moss: green.
    pub(crate) const MOSS: Color = rgb(60, 190, 70);
    /// Rust and warning: red.
    pub(crate) const RUST: Color = rgb(215, 70, 50);
    /// Gold: bright yellow.
    pub(crate) const GOLD: Color = rgb(250, 230, 40);
    /// Slate and quiet things: dark grey.
    pub(crate) const SLATE: Color = rgb(110, 125, 135);
    /// Mud: dark grey.
    pub(crate) const MUD: Color = rgb(120, 115, 100);
    /// Fog: black.
    pub(crate) const FOG: Color = rgb(24, 32, 40);
    /// Knee-deep water: cyan.
    pub(crate) const WET: Color = rgb(0, 170, 200);
}

/// The styles the screens use.
#[derive(Debug, Clone, Copy)]
pub(super) struct Pal {
    pub plain: Style,
    pub dim: Style,
    pub strong: Style,
    pub accent: Style,
    pub good: Style,
    pub bad: Style,
    pub selected: Style,
    color: bool,
}

impl Pal {
    fn new(f: &DoorFrame<'_>, color: bool) -> Self {
        Self {
            plain: f.style(DoorStyle::Plain),
            dim: f.style(DoorStyle::Dim),
            strong: f.style(DoorStyle::Strong),
            accent: f.style(DoorStyle::Accent),
            good: f.style(DoorStyle::Good),
            bad: f.style(DoorStyle::Bad),
            selected: f.style(DoorStyle::Selected),
            color,
        }
    }

    /// The theme's style for a [`DoorStyle`].
    pub(super) const fn role(&self, style: DoorStyle) -> Style {
        match style {
            DoorStyle::Plain => self.plain,
            DoorStyle::Dim => self.dim,
            DoorStyle::Strong => self.strong,
            DoorStyle::Accent => self.accent,
            DoorStyle::Good => self.good,
            DoorStyle::Bad => self.bad,
            DoorStyle::Selected => self.selected,
        }
    }

    /// `color` as a foreground when colour is on; the plain style when it is off.
    pub(super) fn fg(&self, color: Color) -> Style {
        if self.color {
            Style::new().fg(color)
        } else {
            self.plain
        }
    }

    /// `color` as a foreground, or `fallback` when colour is off.
    pub(super) fn fg_or(&self, color: Color, fallback: Style) -> Style {
        if self.color {
            Style::new().fg(color)
        } else {
            fallback
        }
    }

    /// A foreground and a background; plain when colour is off.
    pub(super) fn on(&self, fg: Color, bg: Color) -> Style {
        if self.color {
            Style::new().fg(fg).bg(bg)
        } else {
            self.plain
        }
    }

    pub(super) fn sea(&self) -> Style {
        self.fg(hue::SEA)
    }
    pub(super) fn salt(&self) -> Style {
        self.fg_or(hue::SALT, self.strong)
    }
    pub(super) fn peat(&self) -> Style {
        self.fg(hue::PEAT)
    }
    pub(super) fn reed(&self) -> Style {
        self.fg(hue::REED)
    }
    pub(super) fn moss(&self) -> Style {
        self.fg_or(hue::MOSS, self.good)
    }
    pub(super) fn rust(&self) -> Style {
        self.fg_or(hue::RUST, self.bad)
    }
    pub(super) fn gold(&self) -> Style {
        self.fg_or(hue::GOLD, self.strong)
    }
    pub(super) fn slate(&self) -> Style {
        self.fg_or(hue::SLATE, self.dim)
    }
    /// Frame lines.
    pub(super) fn border(&self) -> Style {
        self.fg_or(hue::SLATE, self.dim)
    }
}

/// Everything a screen reads, gathered once per draw.
pub(super) struct Ui<'a> {
    pub s: &'a Session,
    pub clan: &'a Clan,
    pub look: Look,
    pub pal: Pal,
    pub env: Env<'a>,
    /// The clan's clock (minutes since the epoch).
    pub minute: i64,
    /// The water now.
    pub level: i32,
    pub entries: Vec<Entry>,
}

impl Ui<'_> {
    /// A framed panel with a title.
    pub(super) fn panel(&self, title: &str) -> Block<'static> {
        let block = Block::bordered()
            .border_type(if self.look.ascii {
                BorderType::Plain
            } else {
                BorderType::Rounded
            })
            .border_style(self.pal.border());
        if title.is_empty() {
            block
        } else {
            block.title(Line::from(vec![
                Span::styled(" ", self.pal.border()),
                Span::styled(
                    title.to_owned(),
                    self.pal.salt().add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", self.pal.border()),
            ]))
        }
    }

    /// The letter that presses `entries[i]`.
    pub(super) fn key_of(i: usize) -> char {
        char::from(b'a' + u8::try_from(i).unwrap_or(0))
    }

    /// The entry with this tag, with its index.
    pub(super) fn entry(&self, tag: &str) -> Option<(usize, &Entry)> {
        self.entries.iter().enumerate().find(|(_, e)| e.tag == tag)
    }

    /// Entries whose tag starts with one of `prefixes`, with their indices.
    pub(super) fn entries_like<'e>(
        &'e self,
        prefixes: &'e [&'e str],
    ) -> impl Iterator<Item = (usize, &'e Entry)> + 'e {
        self.entries
            .iter()
            .enumerate()
            .filter(move |(_, e)| prefixes.iter().any(|p| e.tag.starts_with(p)))
    }

    /// The `[x] text` rows for `items`, the lightbar on the cursor.
    pub(super) fn order_rows<'e>(
        &self,
        items: impl Iterator<Item = (usize, &'e Entry)>,
        width: u16,
    ) -> Vec<Line<'static>> {
        items
            .map(|(i, e)| {
                let on = i == self.s.cursor;
                let key = Self::key_of(i);
                let room = usize::from(width).saturating_sub(5);
                let text = fit(&e.text, room);
                if on {
                    let pad = usize::from(width).saturating_sub(5 + text.chars().count());
                    Line::from(Span::styled(
                        format!(" {} {}{}", key, text, " ".repeat(pad)),
                        self.pal.selected,
                    ))
                } else {
                    Line::from(vec![
                        Span::styled(format!(" {key} "), self.pal.accent),
                        Span::styled(text, self.pal.plain),
                    ])
                }
            })
            .collect()
    }

    /// An "orders" list: the entries as hotkey rows, scrolled so the cursor shows.
    pub(super) fn orders(
        &self,
        f: &mut DoorFrame<'_>,
        area: Rect,
        title: &str,
        items: &[(usize, &Entry)],
    ) {
        let block = self.panel(title);
        let inner = block.inner(area);
        f.render_widget(block, area);
        if items.is_empty() {
            f.render_widget(
                Paragraph::new(Line::styled("Nothing to do here.", self.pal.dim)),
                inner,
            );
            return;
        }
        let at = items
            .iter()
            .position(|(i, _)| *i == self.s.cursor)
            .unwrap_or(0);
        let rows = usize::from(inner.height);
        let first = if at >= rows { at + 1 - rows } else { 0 };
        let lines = self.order_rows(items.iter().copied().skip(first).take(rows), inner.width);
        f.render_widget(Paragraph::new(lines), inner);
        if items.len() > rows && inner.width > 2 {
            let more = format!("{}/{}", at + 1, items.len());
            let w = u16::try_from(more.len()).unwrap_or(0);
            let r = Rect::new(inner.right().saturating_sub(w), area.bottom() - 1, w, 1);
            f.render_widget(Paragraph::new(Span::styled(more, self.pal.dim)), r);
        }
    }

    /// A horizontal bar `width` cells wide filled `have` out of `max`, with eighths of a cell.
    pub(super) fn bar(&self, have: u32, max: u32, width: u16, fill: Style) -> Vec<Span<'static>> {
        let width = usize::from(width);
        let max = max.max(1);
        let eighths =
            usize::try_from(u64::from(have.min(max)) * (width as u64) * 8 / u64::from(max))
                .unwrap_or(0);
        let (full, part) = (eighths / 8, eighths % 8);
        let mut spans = Vec::new();
        let (on, off) = (self.look.g("█", "#"), self.look.g("░", "-"));
        if full > 0 {
            spans.push(Span::styled(on.repeat(full), fill));
        }
        let mut used = full;
        if part > 0 && full < width {
            const PARTS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
            let glyph = if self.look.ascii { "=" } else { PARTS[part] };
            spans.push(Span::styled(glyph, fill));
            used += 1;
        }
        if used < width {
            spans.push(Span::styled(off.repeat(width - used), self.pal.slate()));
        }
        spans
    }

    /// `label  ████░░  have/max` on one line.
    pub(super) fn gauge_line(
        &self,
        label: &str,
        have: u32,
        max: u32,
        width: u16,
        fill: Style,
    ) -> Line<'static> {
        self.gauge_with(label, have, max, width, fill, format!(" {have}/{max}"))
    }

    /// A gauge line with its own words after the bar.
    pub(super) fn gauge_with(
        &self,
        label: &str,
        have: u32,
        max: u32,
        width: u16,
        fill: Style,
        tail: String,
    ) -> Line<'static> {
        let label_w = 8u16;
        let bar_w = width
            .saturating_sub(label_w)
            .saturating_sub(u16::try_from(tail.len()).unwrap_or(0))
            .max(2);
        let mut spans = vec![Span::styled(format!("{label:<8}"), self.pal.dim)];
        spans.extend(self.bar(have, max, bar_w, fill));
        spans.push(Span::styled(tail, self.pal.plain));
        Line::from(spans)
    }
}

/// `text` cut to `width` columns with an ellipsis.
pub(super) fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let keep = width.saturating_sub(1);
    let mut out: String = text.chars().take(keep).collect();
    out.push('~');
    out
}

/// The first word of a name: how the crew speak of each other.
pub(super) fn short_name(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

/// Wrap a paragraph to `width` columns.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    super::trip_ui::wrap(text, width.max(8))
}

/// The entries a screen shows (the lightbar moves among these), by index.
pub(super) fn shown(s: &Session, entries: &[Entry]) -> Vec<usize> {
    if s.mode == Mode::Menu(MenuId::Crew)
        && let Some(clan) = &s.clan
        && let Some(c) = clan.crew.get(s.crew_sel)
    {
        return entries
            .iter()
            .enumerate()
            .filter(|(_, e)| crewscreen::about(&e.tag, c.id))
            .map(|(i, _)| i)
            .collect();
    }
    (0..entries.len()).collect()
}

/// Draw the whole screen.
pub(super) fn draw(s: &Session, canvas: &mut Canvas<'_>, env: &DoorEnv) {
    let mut f = canvas.frame();
    let area = f.area();
    let look = Look::new(env, area);
    let pal = Pal::new(&f, look.color);
    if area.width < MIN_COLS || area.height < MIN_ROWS {
        small(s, &mut f, area, &pal);
        return;
    }
    match &s.mode {
        Mode::Unreadable => naming::unreadable(&mut f, area, look, &pal),
        Mode::Naming(buffer) => naming::draw(&mut f, area, look, &pal, buffer, &s.notes),
        _ => {
            let Some(clan) = &s.clan else { return };
            let mut env_ = s.env();
            env_.day = s.today;
            let minute = env_.minute(clan);
            let level = s.tide.level(minute);
            let ui = Ui {
                s,
                clan,
                look,
                pal,
                env: env_,
                minute,
                level,
                entries: s.entries(),
            };
            screen(&ui, &mut f, area);
        }
    }
}

/// The plain text screen, for a window too small for the real ones.
fn small(s: &Session, f: &mut DoorFrame<'_>, area: Rect, pal: &Pal) {
    let lines: Vec<Line<'_>> = s
        .lines()
        .into_iter()
        .map(|(text, style)| Line::styled(text, pal.role(style)))
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

/// The chrome (header, tide strip, notes, key hints) and the body of the current screen.
fn screen(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let tide_h: u16 = match ui.look.size {
        Size::Tiny => 1,
        Size::Compact => 2,
        Size::Roomy | Size::Grand => 3,
    };
    let notes = ui.s.notes.len().min(match ui.look.size {
        Size::Tiny => 1,
        Size::Compact => 2,
        Size::Roomy => 3,
        Size::Grand => 4,
    });
    let notes_h = u16::try_from(notes).unwrap_or(0);
    let [header, tide, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(tide_h),
        Constraint::Min(4),
        Constraint::Length(notes_h + 1),
    ])
    .areas(area);
    header_bar(ui, f, header);
    tides::strip(ui, f, tide);
    footer(ui, f, foot, notes);
    match ui.s.mode {
        Mode::Report => report::draw(ui, f, body),
        Mode::Line(_) => social::compose(ui, f, body),
        Mode::Menu(menu) => match menu {
            MenuId::Main | MenuId::Hold => holdscreen::draw(ui, f, body, menu == MenuId::Main),
            MenuId::Crew => crewscreen::draw(ui, f, body),
            MenuId::Market => market::draw(ui, f, body),
            MenuId::Tides => tides::table(ui, f, body),
            MenuId::Plan => plan::draw(ui, f, body),
            MenuId::Trip => match ui.clan.trip.as_ref().map(|t| &t.phase) {
                Some(crate::trip::Phase::Scene(sc)) => scene::draw(ui, f, body, sc),
                Some(crate::trip::Phase::Fight(fight)) => skirmish::draw(ui, f, body, fight),
                _ => mapscreen::draw(ui, f, body, true),
            },
            MenuId::Map => mapscreen::draw(ui, f, body, false),
            MenuId::Court => court::draw(ui, f, body),
            MenuId::Letters => social::letters(ui, f, body),
            MenuId::Exchange => social::exchange(ui, f, body),
            MenuId::Raid => social::raid(ui, f, body),
            MenuId::Pacts => social::pacts(ui, f, body),
            MenuId::Compose => social::compose(ui, f, body),
            MenuId::League => court::league(ui, f, body),
            MenuId::Hall => court::hall(ui, f, body),
        },
        _ => {}
    }
}

/// The clan, the day, the turns left, the tallies.
fn header_bar(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let clan = ui.clan;
    let p = &ui.pal;
    let day = ui.s.today - clan.founded + 1;
    let left = Line::from(vec![
        Span::styled(format!(" {} ", ui.look.g("⌂", "#")), p.peat()),
        Span::styled(clan.name.clone(), p.salt().add_modifier(Modifier::BOLD)),
        Span::styled(format!("  day {day} of season {}", clan.season), p.dim),
    ]);
    let left_w = u16::try_from(left.width()).unwrap_or(0);
    let turns = clan.turns_left(&ui.s.settings);
    let max = ui.s.settings.turns_per_day;
    let bar_w = 10u16;
    let mut right: Vec<Span<'static>> = vec![Span::styled("turns ", p.dim)];
    let style = if turns <= 5 { p.rust() } else { p.sea() };
    right.extend(ui.bar(u32::from(turns), u32::from(max), bar_w, style));
    right.push(Span::styled(format!(" {turns}/{max}  "), p.plain));
    right.push(Span::styled(
        format!("{} {} ", ui.look.g("◈", "$"), clan.tallies),
        p.gold(),
    ));
    right.push(Span::styled("tallies ", p.dim));
    let right = Line::from(right);
    let right_w = u16::try_from(right.width()).unwrap_or(0);
    f.render_widget(Paragraph::new(left), area);
    if left_w + right_w < area.width {
        let r = Rect::new(area.right() - right_w, area.y, right_w, 1);
        f.render_widget(Paragraph::new(right), r);
    } else {
        // A narrow window: the numbers without the bar.
        let short = Line::from(vec![
            Span::styled(format!("{turns}/{max}"), style),
            Span::styled(" turns ", p.dim),
            Span::styled(format!("{}t", clan.tallies), p.gold()),
        ]);
        let w = u16::try_from(short.width()).unwrap_or(0);
        if left_w + w < area.width {
            f.render_widget(
                Paragraph::new(short),
                Rect::new(area.right() - w, area.y, w, 1),
            );
        }
    }
}

/// The notes from the last action, the full words of the order on the lightbar (the lists cut
/// long orders short) and the keys that work here.
fn footer(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, notes: usize) {
    let p = &ui.pal;
    let mut lines: Vec<Line<'_>> = Vec::new();
    let skip = ui.s.notes.len().saturating_sub(notes);
    for (text, style) in ui.s.notes.iter().skip(skip) {
        lines.push(Line::styled(
            fit(&format!(" {text}"), usize::from(area.width)),
            p.role(*style),
        ));
    }
    let hint = match ui.s.mode {
        Mode::Report | Mode::Menu(MenuId::Tides) => "any key",
        Mode::Menu(MenuId::Trip) => "arrows walk  Enter",
        Mode::Menu(MenuId::Crew) => "up/down crew  </> order",
        Mode::Menu(MenuId::Map) => "arrows pan  Esc",
        Mode::Menu(MenuId::Letters) => "up/down letter  Enter  Esc",
        Mode::Menu(MenuId::Exchange | MenuId::Raid | MenuId::Pacts | MenuId::Compose) => {
            "</> page  up/down  Enter  Esc"
        }
        Mode::Line(_) => "type  Enter  Esc",
        _ => "up/down  Enter  Esc",
    };
    let hint_w = hint.chars().count() + 2;
    let order = match ui.s.mode {
        Mode::Menu(MenuId::Map | MenuId::Tides | MenuId::League | MenuId::Hall)
        | Mode::Report
        | Mode::Line(_) => None,
        _ => ui.entries.get(ui.s.cursor).map(|e| e.text.clone()),
    };
    let room = usize::from(area.width).saturating_sub(hint_w + 4);
    let (left, left_style) = match order {
        Some(text) => (fit(&text, room), p.salt()),
        None => (String::new(), p.dim),
    };
    let pad = usize::from(area.width).saturating_sub(left.chars().count() + hint_w + 3);
    lines.push(Line::from(vec![
        Span::styled(format!(" {} ", ui.look.g("»", ">")), p.accent),
        Span::styled(left, left_style),
        Span::raw(" ".repeat(pad)),
        Span::styled(hint, p.dim),
    ]));
    f.render_widget(Paragraph::new(lines), area);
}

/// A rectangle `w` by `h` centred in `area` (cut down to fit).
pub(super) fn centred(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

/// A widget that paints text in cells with a style each: for maps, pictures and charts that
/// need a colour per cell. One `Vec<Vec<(symbol, style)>>`, row by row from the top left.
pub(super) struct Cells {
    pub rows: Vec<Vec<(&'static str, Style)>>,
}

impl Widget for Cells {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for (dy, row) in self.rows.iter().enumerate() {
            let Ok(dy) = u16::try_from(dy) else { break };
            if dy >= area.height {
                break;
            }
            for (dx, (symbol, style)) in row.iter().enumerate() {
                let Ok(dx) = u16::try_from(dx) else { break };
                if dx >= area.width {
                    break;
                }
                if let Some(cell) = buf.cell_mut((area.x + dx, area.y + dy)) {
                    cell.set_symbol(symbol).set_style(*style);
                }
            }
        }
    }
}
