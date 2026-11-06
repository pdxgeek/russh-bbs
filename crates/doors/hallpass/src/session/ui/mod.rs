//! The Hallpass screens (M4-H3): every screen drawn with ratatui widgets into the door's own
//! buffer (D104). The board cleans every cell as it composites the buffer, so nothing here can
//! reach the terminal; what this module owns is how Greywater Academy looks.
//!
//! **No game rules live here.** Every number on a screen is read from the engine (`Game`,
//! `Game::chance`, `Game::readiness`, `Game::routes`, `Game::lesson_view`, ...). The keys are the
//! menu items the text screen has always had (`Session::view`), so the headless bots, the replay
//! logs and a screen reader see the same choices; the arrows, Enter and the map cursor only
//! press those same items.
//!
//! **Look.** A strange old boarding school: ink-blue corridors, chalk-white lettering, a
//! detention-red file that grows thicker, and a timetable strip that ticks along the top. The
//! accents are chosen so the nearest of the sixteen terminal colours (what the board composites
//! to today, D104) is the one named in each comment; with colour off every cue is also a glyph or
//! a word.
//!
//! **Tiers.** In the ASCII tier every glyph is chosen here as plain ASCII ([`Look::g`]). The
//! Nerd Font tier draws what the Unicode tier draws (D104 neutralizes private-use glyphs).
//!
//! **Sizes.** Every screen is laid out for the 74x20 a door gets inside an 80x24 board first
//! ([`Size::Tiny`]), is comfortable at 80x24 ([`Size::Compact`]) and gets more at 100x30
//! ([`Size::Roomy`]) and 132x43 ([`Size::Grand`]). Below [`MIN_COLS`] by [`MIN_ROWS`] the plain
//! text screen is drawn.

// The screens are long straight runs of drawing code over cell coordinates (`x`, `y`, `w`,
// `h`), panels and styles; splitting them for length would scatter one picture over many
// functions, and the short names are the ones ratatui itself uses.
#![allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_lossless
)]

use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Block, BorderType, Paragraph, Widget};
use russh_bbs_door::{Canvas, DoorEnv, DoorFrame, DoorStyle, Tier};

use super::{Item, Mode, Session};
use crate::game::Game;
use crate::state::Status;

mod day;
mod describe;
mod file;
mod lesson;
mod map;
mod paper;
mod people;
mod plan;
mod run;

pub(super) use describe::describe;
pub(super) use map::{Dir, step_cursor};

/// The smallest window the screens are laid out for.
pub(super) const MIN_COLS: u16 = 60;
/// The fewest rows the screens are laid out for.
pub(super) const MIN_ROWS: u16 = 16;

/// Which screen is up: the engine state and the mode decide, in the order `Session::view` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Screen {
    Enrol,
    Over,
    Run,
    Lesson,
    Scene,
    Main,
    Go,
    Study,
    Shop,
    Plans,
    DraftTarget,
    DraftMethod,
    Scheme,
    Twists,
    Getaway,
    Recruit,
    Schedule,
    Stats,
    News,
    Hall,
    Yearbook,
    Talk,
    Clubs,
    Journal,
    Social,
    Classmates,
    PlotKinds,
    Rumours,
    RumourAbout,
    Election,
}

impl Screen {
    /// The screen `s` is showing now.
    pub(super) fn of(s: &Session) -> Self {
        let g = &s.game;
        if g.player.status == Status::New {
            return Self::Enrol;
        }
        if !g.playing() {
            return Self::Over;
        }
        if g.player.run.is_some() {
            return Self::Run;
        }
        if g.player.life.lesson.is_some() {
            return Self::Lesson;
        }
        if g.player.life.scene.is_some() {
            return Self::Scene;
        }
        match &s.mode {
            Mode::Main => Self::Main,
            Mode::Go => Self::Go,
            Mode::Study => Self::Study,
            Mode::Shop => Self::Shop,
            Mode::Plans => Self::Plans,
            Mode::DraftTarget => Self::DraftTarget,
            Mode::DraftMethod(_) => Self::DraftMethod,
            Mode::Scheme(_) => Self::Scheme,
            Mode::Twists(_) => Self::Twists,
            Mode::Getaway(_) => Self::Getaway,
            Mode::Recruit(_) => Self::Recruit,
            Mode::Schedule(_) => Self::Schedule,
            Mode::Stats => Self::Stats,
            Mode::News => Self::News,
            Mode::Hall => Self::Hall,
            Mode::Yearbook => Self::Yearbook,
            Mode::Talk => Self::Talk,
            Mode::Clubs => Self::Clubs,
            Mode::Journal => Self::Journal,
            Mode::Social => Self::Social,
            Mode::Classmates => Self::Classmates,
            Mode::PlotKinds(_) => Self::PlotKinds,
            Mode::Rumours => Self::Rumours,
            Mode::RumourAbout(_) => Self::RumourAbout,
            Mode::Election => Self::Election,
        }
    }

    /// The name of the screen, for the look-around and the tests.
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Enrol => "enrol",
            Self::Over => "over",
            Self::Run => "run",
            Self::Lesson => "lesson",
            Self::Scene => "scene",
            Self::Main => "day",
            Self::Go => "map",
            Self::Study => "study",
            Self::Shop => "shop",
            Self::Plans => "schemes",
            Self::DraftTarget => "target",
            Self::DraftMethod => "method",
            Self::Scheme => "planner",
            Self::Twists => "twists",
            Self::Getaway => "getaway",
            Self::Recruit => "recruit",
            Self::Schedule => "schedule",
            Self::Stats => "file",
            Self::News => "paper",
            Self::Hall => "hall",
            Self::Yearbook => "yearbook",
            Self::Talk => "talk",
            Self::Clubs => "clubs",
            Self::Journal => "journal",
            Self::Social => "social",
            Self::Classmates => "classmates",
            Self::PlotKinds => "plots",
            Self::Rumours => "rumours",
            Self::RumourAbout => "rumour",
            Self::Election => "election",
        }
    }
}

/// How much room a screen has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Size {
    /// Smaller than 78 columns or 22 rows: the 74x20 a door gets inside an 80x24 board.
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
}

/// An RGB colour for a style, only when colour is on.
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// The Hallpass accents: ink blue, chalk white, detention red, lamplight and a few quiet greys.
pub(super) mod hue {
    use super::{Color, rgb};
    // Each accent is a saturated colour chosen so that the nearest of the sixteen terminal
    // colours (what the board composites to today, D104) is the one named in the comment.
    /// Ink blue (light blue): borders, titles, the corridors.
    pub(crate) const INK: Color = rgb(96, 120, 255);
    /// Deep ink (blue): bands behind chalk lettering.
    pub(crate) const INK_DEEP: Color = rgb(20, 36, 170);
    /// Chalk white (light grey): lettering on the boards.
    pub(crate) const CHALK: Color = rgb(232, 234, 228);
    /// Detention red (red): the file, the heat, a locked door.
    pub(crate) const RED: Color = rgb(222, 56, 52);
    /// Lamplight (bright yellow): where you are, the period now.
    pub(crate) const LAMP: Color = rgb(255, 232, 80);
    /// Moss (green): good news, a friend.
    pub(crate) const MOSS: Color = rgb(60, 190, 80);
    /// Slate (dark grey): things that are over or out of reach.
    pub(crate) const SLATE: Color = rgb(112, 120, 136);
    /// Brass (yellow, brown on most terminals): clubs, cash, doors.
    pub(crate) const BRASS: Color = rgb(196, 140, 46);
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

    /// `color` as a foreground when colour is on; `fallback` when it is off.
    pub(super) fn fg_or(&self, color: Color, fallback: Style) -> Style {
        if self.color {
            Style::new().fg(color)
        } else {
            fallback
        }
    }

    /// A foreground and a background; `fallback` when colour is off.
    pub(super) fn on(&self, fg: Color, bg: Color, fallback: Style) -> Style {
        if self.color {
            Style::new().fg(fg).bg(bg)
        } else {
            fallback
        }
    }

    /// Ink blue.
    pub(super) fn ink(&self) -> Style {
        self.fg_or(hue::INK, self.accent)
    }
    /// Chalk white, bold.
    pub(super) fn chalk(&self) -> Style {
        self.fg_or(hue::CHALK, self.strong)
            .add_modifier(Modifier::BOLD)
    }
    /// Chalk white, not bold.
    pub(super) fn chalk_soft(&self) -> Style {
        self.fg_or(hue::CHALK, self.plain)
    }
    /// Detention red.
    pub(super) fn red(&self) -> Style {
        self.fg_or(hue::RED, self.bad)
    }
    /// Lamplight.
    pub(super) fn lamp(&self) -> Style {
        self.fg_or(hue::LAMP, self.strong)
            .add_modifier(Modifier::BOLD)
    }
    /// Moss.
    pub(super) fn moss(&self) -> Style {
        self.fg_or(hue::MOSS, self.good)
    }
    /// Slate.
    pub(super) fn slate(&self) -> Style {
        self.fg_or(hue::SLATE, self.dim)
    }
    /// Brass.
    pub(super) fn brass(&self) -> Style {
        self.fg_or(hue::BRASS, self.accent)
    }
    /// Frame lines.
    pub(super) fn border(&self) -> Style {
        self.fg_or(hue::INK, self.dim)
    }
    /// Lettering on a deep-ink band; the theme's selection style when colour is off.
    pub(super) fn band(&self) -> Style {
        self.on(hue::CHALK, hue::INK_DEEP, self.selected)
            .add_modifier(Modifier::BOLD)
    }
    /// The row the lightbar is on.
    pub(super) fn bar_on(&self) -> Style {
        self.on(hue::LAMP, hue::INK_DEEP, self.selected)
            .add_modifier(Modifier::BOLD)
    }
    /// A stamp in detention red: red lettering on the page, never reverse video (the reserved
    /// marker style, D39).
    pub(super) fn stamp(&self) -> Style {
        self.on(hue::CHALK, hue::RED, self.bad)
    }
}

/// Everything a screen reads, gathered once per draw.
pub(super) struct Ui<'a> {
    pub s: &'a Session,
    pub g: &'a Game,
    pub look: Look,
    pub pal: Pal,
    pub screen: Screen,
    /// The text lines `Session::view` made (the screen's own words).
    pub body: Vec<String>,
    /// The menu: what the keys press.
    pub items: Vec<Item>,
    /// The board's clock has moved to a later day than the one on screen (a visit left open
    /// across midnight): the next key starts the new morning.
    pub bell: bool,
}

impl Ui<'_> {
    /// The lightbar position, kept inside the menu.
    pub(super) fn cursor(&self) -> usize {
        self.s.cursor.min(self.items.len().saturating_sub(1))
    }

    /// The item the lightbar is on.
    pub(super) fn current(&self) -> Option<&Item> {
        self.items.get(self.cursor())
    }

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
                Span::styled(title.to_owned(), self.pal.chalk()),
                Span::styled(" ", self.pal.border()),
            ]))
        }
    }

    /// A panel drawn and its inner area returned.
    pub(super) fn framed(&self, f: &mut DoorFrame<'_>, area: Rect, title: &str) -> Rect {
        let block = self.panel(title);
        let inner = block.inner(area);
        f.render_widget(block, area);
        inner
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

    /// `label  ████░░  tail` on one line.
    pub(super) fn gauge(
        &self,
        label: &str,
        have: u32,
        max: u32,
        width: u16,
        fill: Style,
        tail: &str,
    ) -> Line<'static> {
        let label_w = 7u16;
        let tail_w = u16::try_from(tail.chars().count()).unwrap_or(0);
        let bar_w = width.saturating_sub(label_w + tail_w + 1).max(2);
        let mut spans = vec![Span::styled(format!("{label:<7}"), self.pal.slate())];
        spans.extend(self.bar(have, max, bar_w, fill));
        spans.push(Span::styled(format!(" {tail}"), self.pal.chalk_soft()));
        Line::from(spans)
    }

    /// `●●●○○`: `have` of `max` pips.
    pub(super) fn pips(&self, have: usize, max: usize, on: Style) -> Vec<Span<'static>> {
        let (full, empty) = (self.look.g("●", "*"), self.look.g("○", "o"));
        let mut out = Vec::new();
        if have > 0 {
            out.push(Span::styled(full.repeat(have.min(max)), on));
        }
        if max > have {
            out.push(Span::styled(empty.repeat(max - have), self.pal.slate()));
        }
        out
    }

    /// The `[x] text` rows for `items`, the lightbar on the cursor.
    pub(super) fn order_rows(&self, rows: &[(usize, &Item)], width: u16) -> Vec<Line<'static>> {
        rows.iter()
            .map(|(i, e)| {
                let on = *i == self.cursor();
                let room = usize::from(width).saturating_sub(5);
                let text = fit(&e.label, room);
                if on {
                    let pad = usize::from(width).saturating_sub(5 + text.chars().count());
                    Line::from(Span::styled(
                        format!(" {} {}{}", key_cap(e.key), text, " ".repeat(pad)),
                        self.pal.bar_on(),
                    ))
                } else {
                    Line::from(vec![
                        Span::styled(format!(" {} ", key_cap(e.key)), self.pal.brass()),
                        Span::styled(text, self.pal.chalk_soft()),
                    ])
                }
            })
            .collect()
    }

    /// An "orders" list in a panel: the items as hotkey rows, scrolled so the cursor shows.
    pub(super) fn orders(&self, f: &mut DoorFrame<'_>, area: Rect, title: &str) {
        let inner = self.framed(f, area, title);
        let all: Vec<(usize, &Item)> = self.items.iter().enumerate().collect();
        let at = self.cursor();
        let rows = usize::from(inner.height).max(1);
        let first = if at >= rows { at + 1 - rows } else { 0 };
        let shown: Vec<(usize, &Item)> = all.iter().copied().skip(first).take(rows).collect();
        f.render_widget(Paragraph::new(self.order_rows(&shown, inner.width)), inner);
        if all.len() > rows && inner.width > 6 && area.height > 2 {
            let more = format!("{}/{}", at + 1, all.len());
            let w = u16::try_from(more.len()).unwrap_or(0);
            let r = Rect::new(area.right().saturating_sub(w + 2), area.bottom() - 1, w, 1);
            f.render_widget(Paragraph::new(Span::styled(more, self.pal.slate())), r);
        }
    }

    /// The items as hotkey chips in rows, the cursor lit; returns the rows used.
    pub(super) fn chips(&self, f: &mut DoorFrame<'_>, area: Rect, short: bool) -> u16 {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut cur: Vec<Span<'static>> = Vec::new();
        let mut used = 0usize;
        let width = usize::from(area.width);
        let at = self.cursor();
        for (i, it) in self.items.iter().enumerate() {
            let label = chip_label(self.screen, it, short);
            let text = format!(" {} {} ", key_cap(it.key), label);
            let w = text.chars().count() + 1;
            if used + w > width && !cur.is_empty() {
                lines.push(Line::from(std::mem::take(&mut cur)));
                used = 0;
            }
            let style = if i == at {
                self.pal.bar_on()
            } else {
                self.pal.chalk_soft()
            };
            let key_style = if i == at { style } else { self.pal.brass() };
            cur.push(Span::styled(format!(" {} ", key_cap(it.key)), key_style));
            cur.push(Span::styled(format!("{label} "), style));
            cur.push(Span::raw(" "));
            used += w;
        }
        if !cur.is_empty() {
            lines.push(Line::from(cur));
        }
        let n = u16::try_from(lines.len()).unwrap_or(0).min(area.height);
        f.render_widget(Paragraph::new(lines), area);
        n
    }
}

/// How a key is shown on a hotkey: the letter or digit itself.
pub(super) fn key_cap(key: char) -> String {
    key.to_string()
}

/// A short name for a menu item as a chip.
fn chip_label(screen: Screen, it: &Item, short: bool) -> String {
    let label = match (screen, it.key) {
        (Screen::Main, 'g') => "Go",
        (Screen::Main, 'a') => "Attend",
        (Screen::Main, 's') => "Study",
        (Screen::Main, 'b') => "Buy",
        (Screen::Main, 'l') => "Loiter",
        (Screen::Main, 't') => "Talk",
        (Screen::Main, 'm') => "Meeting",
        (Screen::Main, 'r') => "Rest",
        (Screen::Main, 'p') => "Schemes",
        (Screen::Main, 'c') => "Clubs",
        (Screen::Main, 'f') => "File",
        (Screen::Main, 'j') => "Journal",
        (Screen::Main, 'n') => "Paper",
        (Screen::Main, 'o') => "Social",
        (Screen::Main, 'e') => "Search",
        (Screen::Main, 'z') => "Lights out",
        (Screen::Main, 'q') => "Leave",
        (Screen::Scheme, 't') => "Twists",
        (Screen::Scheme, 'y') => "Getaway",
        (Screen::Scheme, 'r') => "Recruit",
        (Screen::Scheme, 'w') => "Scout",
        (Screen::Scheme, 'd') => "Set time",
        (Screen::Scheme, 'x') => "GO",
        (Screen::Scheme, 'z') => "Tear up",
        (_, 'b') => "Back",
        (Screen::News, 'h') => "Hall of legend",
        (Screen::News, 'y') => "Yearbook",
        _ => "",
    };
    if label.is_empty() {
        fit(&it.label, if short { 14 } else { 18 })
    } else {
        label.to_owned()
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

/// Wrap a paragraph to `width` columns.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    super::wrap(text, width.max(8))
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
    pub rows: Vec<Vec<(String, Style)>>,
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

/// Said when the board's day has moved on from the one on screen.
const BELL: &str = "The morning bell has rung: press any key for the new day.";

/// Draw the whole screen.
pub(super) fn draw(s: &Session, canvas: &mut Canvas<'_>, env: &DoorEnv) {
    // The day on the board's clock when drawing (`Canvas::today`), not the day the visit opened.
    let bell = s.game.playing() && canvas.today().as_raw() > s.game.player.day;
    let mut f = canvas.frame();
    let area = f.area();
    let look = Look::new(env, area);
    let pal = Pal::new(&f, look.color);
    if area.width < MIN_COLS || area.height < MIN_ROWS {
        small(s, &mut f, area, &pal, bell);
        return;
    }
    let (body, items) = s.view();
    let ui = Ui {
        s,
        g: &s.game,
        look,
        pal,
        screen: Screen::of(s),
        body,
        items,
        bell,
    };
    screen(&ui, &mut f, area);
}

/// The plain text screen, for a window too small for the real ones.
fn small(s: &Session, f: &mut DoorFrame<'_>, area: Rect, pal: &Pal, bell: bool) {
    let mut lines: Vec<Line<'_>> = s
        .lines(usize::from(area.height), false)
        .into_iter()
        .map(|(text, style)| Line::styled(text, pal.role(style)))
        .collect();
    if bell {
        // The bell takes the last row.
        lines.truncate(usize::from(area.height).saturating_sub(1));
        lines.push(Line::styled(BELL, pal.lamp()));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// The chrome (masthead, timetable strip, notes and key hints) and the body of the screen.
fn screen(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    match ui.screen {
        Screen::Enrol => {
            paper::enrol(ui, f, area);
            return;
        }
        Screen::Over => {
            let [head, body, foot] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Min(4),
                Constraint::Length(2),
            ])
            .areas(area);
            masthead(ui, f, head);
            paper::over(ui, f, body);
            footer(ui, f, foot, 0);
            return;
        }
        _ => {}
    }
    let strip_h: u16 = match ui.look.size {
        Size::Tiny => 2,
        _ => 3,
    };
    let notes_cap = match ui.look.size {
        Size::Tiny => 2,
        Size::Compact => 3,
        Size::Roomy => 4,
        Size::Grand => 5,
    };
    let own_log = matches!(ui.screen, Screen::Run);
    let notes = if own_log {
        0
    } else {
        (ui.s.messages.len() + usize::from(ui.bell)).min(notes_cap)
    };
    let notes_h = u16::try_from(notes).unwrap_or(0);
    let [head, strip, body, foot] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(strip_h),
        Constraint::Min(4),
        Constraint::Length(notes_h + 1),
    ])
    .areas(area);
    masthead(ui, f, head);
    day::strip(ui, f, strip);
    footer(ui, f, foot, notes);
    match ui.screen {
        Screen::Main => day::draw(ui, f, body),
        Screen::Go => map::draw(ui, f, body),
        Screen::Study => day::study(ui, f, body),
        Screen::Shop => day::shop(ui, f, body),
        Screen::Plans
        | Screen::DraftTarget
        | Screen::DraftMethod
        | Screen::Twists
        | Screen::Getaway
        | Screen::Recruit
        | Screen::Schedule
        | Screen::Social
        | Screen::Classmates
        | Screen::PlotKinds
        | Screen::Rumours
        | Screen::RumourAbout
        | Screen::Election
        | Screen::Talk => plan::list(ui, f, body),
        Screen::Scheme => plan::planner(ui, f, body),
        Screen::Run => run::draw(ui, f, body),
        Screen::Stats => file::draw(ui, f, body),
        Screen::Lesson => lesson::draw(ui, f, body),
        Screen::Scene => people::scene(ui, f, body),
        Screen::Clubs => people::clubs(ui, f, body),
        Screen::Journal => people::journal(ui, f, body),
        Screen::News => paper::news(ui, f, body),
        Screen::Hall => paper::hall(ui, f, body),
        Screen::Yearbook => paper::yearbook(ui, f, body),
        Screen::Enrol | Screen::Over => {}
    }
}

/// The school, the term, the day.
fn masthead(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect) {
    let p = &ui.pal;
    let g = ui.g;
    let left = Line::from(vec![
        Span::styled(format!(" {} ", ui.look.g("♜", "#")), p.lamp()),
        Span::styled("GREYWATER ACADEMY", p.chalk()),
        Span::styled(format!("  {} ", ui.look.g("·", "-")), p.slate()),
        Span::styled(g.player.name.clone(), p.ink()),
    ]);
    let day_no = (g.term_day() + 1).max(1);
    let weekday = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][Game::weekday(g.player.day)];
    let sep = ui.look.g("·", "-");
    let right_text = match ui.screen {
        Screen::Enrol => String::new(),
        _ => format!(
            "term {} {sep} {weekday} day {}/{} ",
            g.player.term, day_no, g.content.rules.term_days
        ),
    };
    let lw = u16::try_from(left.width()).unwrap_or(0);
    let rw = u16::try_from(right_text.chars().count()).unwrap_or(0);
    f.render_widget(Paragraph::new(left), area);
    if lw + rw < area.width {
        f.render_widget(
            Paragraph::new(Span::styled(right_text, p.slate())),
            Rect::new(area.right() - rw, area.y, rw, 1),
        );
    }
}

/// The notes from the last action, the full words of the order on the lightbar (the lists cut
/// long orders short) and the keys that work here.
fn footer(ui: &Ui<'_>, f: &mut DoorFrame<'_>, area: Rect, notes: usize) {
    let p = &ui.pal;
    let mut lines: Vec<Line<'_>> = Vec::new();
    let mut texts: Vec<&str> = ui.s.messages.iter().map(String::as_str).collect();
    if ui.bell {
        texts.push(BELL);
    }
    let skip = texts.len().saturating_sub(notes);
    for text in texts.iter().skip(skip) {
        lines.push(Line::styled(
            fit(&format!(" {text}"), usize::from(area.width)),
            p.lamp(),
        ));
    }
    let hint = match ui.screen {
        Screen::Go => "arrows choose  Enter go  Esc",
        Screen::Scheme => "</> odds  up/down  Enter  Esc",
        Screen::Over | Screen::Enrol => "up/down  Enter",
        _ => "up/down  Enter  Esc",
    };
    let hint_w = hint.chars().count() + 2;
    let order = match ui.screen {
        Screen::Go => {
            let id = map::selected(ui);
            ui.g.content.place(&id).map(|p| p.name.clone())
        }
        Screen::Clubs => ui.g.content.clubs.get(ui.s.tab).map(|c| c.name.clone()),
        _ => ui.current().map(|e| e.label.clone()),
    };
    let room = usize::from(area.width).saturating_sub(hint_w + 4);
    let (left, left_style) = match order {
        Some(text) => (fit(&text, room), p.chalk()),
        None => (String::new(), p.slate()),
    };
    let pad = usize::from(area.width).saturating_sub(left.chars().count() + hint_w + 3);
    lines.push(Line::from(vec![
        Span::styled(format!(" {} ", ui.look.g("»", ">")), p.brass()),
        Span::styled(left, left_style),
        Span::raw(" ".repeat(pad)),
        Span::styled(hint, p.slate()),
    ]));
    f.render_widget(Paragraph::new(lines), area);
}
