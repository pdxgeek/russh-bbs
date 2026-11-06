//! The coin toss screen. A window of at least [`PANELS_MIN_COLS`] by [`PANELS_MIN_ROWS`] gets
//! the full ratatui screen: a table of this visit's calls, a gauge of the day's tosses, a
//! sparkline of the streak and a braille chart of the purse, drawn with the theme's roles and a
//! few colours of its own. A smaller window gets the plain text lines, which are also the
//! look-around view. The framework cleans every cell and degrades every colour, so this file
//! has nothing to do for the ASCII tier or for a member without colour except not to rely on
//! colour alone (the table says "won" and "lost" in words).

use russh_bbs_door::layout::{Constraint, Layout, Rect};
use russh_bbs_door::style::{Color, Style};
use russh_bbs_door::symbols::Marker;
use russh_bbs_door::text::Line;
use russh_bbs_door::widgets::{
    Axis, Block, Cell, Chart, Dataset, Gauge, GraphType, Paragraph, Row, Sparkline, Table,
};
use russh_bbs_door::{Canvas, Ctx, DoorEnv, DoorFrame, DoorKey, DoorSession, DoorStyle, Flow};

use crate::content::Phrases;
use crate::engine::{Call, DAILY_TOSSES, NAME_MAX, Outcome, State};
use crate::save;
use crate::world::World;

/// The narrowest and shortest window that gets the panels.
pub const PANELS_MIN_COLS: u16 = 56;
/// See [`PANELS_MIN_COLS`].
pub const PANELS_MIN_ROWS: u16 = 14;

/// The most tosses of this visit the table and the charts remember.
const HISTORY_MAX: usize = 24;

/// One toss of this visit. Not saved: it is rebuilt from the keys, so a replay shows the same.
#[derive(Clone, Copy)]
struct Toss {
    call: Call,
    won: bool,
    /// The purse after the toss.
    coins: u32,
    /// The streak after the toss.
    streak: u16,
}

enum Mode {
    Main,
    /// Typing a nickname.
    Name(String),
}

pub(crate) struct Session {
    state: State,
    world: World,
    phrases: Phrases,
    mode: Mode,
    /// What the last toss said, or the last notice.
    message: Option<(String, DoorStyle)>,
    /// The tosses of this visit, oldest first, and the purse the visit began with.
    history: Vec<Toss>,
    start_coins: u32,
}

impl Session {
    pub(crate) fn new(state: State, world: World, phrases: Phrases) -> Self {
        Self {
            start_coins: state.coins,
            state,
            world,
            phrases,
            mode: Mode::Main,
            message: None,
            history: Vec::new(),
        }
    }

    fn toss(&mut self, call: Call, ctx: &mut Ctx<'_>) {
        self.state.new_day(ctx.today().as_raw());
        let outcome = self.state.toss(call, ctx.rng());
        // The flavor line is drawn from the same RNG, so a replay shows the same words.
        let roll = ctx.rng().below(1_000);
        self.message = Some(match outcome {
            Outcome::Won(_) => (
                format!(
                    "You called {}. +1 coin. {}",
                    name_of(call),
                    self.phrases.pick(true, roll)
                ),
                DoorStyle::Good,
            ),
            Outcome::Lost(_) => (
                format!(
                    "You called {}. -1 coin. {}",
                    name_of(call),
                    self.phrases.pick(false, roll)
                ),
                DoorStyle::Bad,
            ),
            Outcome::NoTossesLeft => (
                "No tosses left today. Come back tomorrow.".to_owned(),
                DoorStyle::Dim,
            ),
            Outcome::NoCoins => ("Your purse is empty.".to_owned(), DoorStyle::Bad),
        });
        if let Outcome::Won(_) | Outcome::Lost(_) = outcome {
            self.history.push(Toss {
                call,
                won: matches!(outcome, Outcome::Won(_)),
                coins: self.state.coins,
                streak: self.state.streak,
            });
            if self.history.len() > HISTORY_MAX {
                self.history.remove(0);
            }
            self.world.count_toss();
            ctx.edit_world(|world| {
                let mut world = world.and_then(World::decode).unwrap_or_default();
                world.count_toss();
                Some(world.encode())
            });
            ctx.request_save(save::encode(&self.state));
        }
    }

    fn name_key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) {
        let Mode::Name(buffer) = &mut self.mode else {
            return;
        };
        match key {
            DoorKey::Char(c) if buffer.chars().count() < NAME_MAX => buffer.push(c),
            DoorKey::Backspace => {
                buffer.pop();
            }
            DoorKey::Escape => self.mode = Mode::Main,
            DoorKey::Enter => {
                // The board's username rules, reserved names included. The raw text is never
                // stored: only the checked name.
                let checked = ctx.player_name(buffer);
                self.mode = Mode::Main;
                if let Ok(name) = checked {
                    name.as_str().clone_into(&mut self.state.name);
                    ctx.request_save(save::encode(&self.state));
                    self.message = Some((
                        format!("Now known as {}.", self.state.name),
                        DoorStyle::Good,
                    ));
                } else {
                    self.message =
                        Some(("That name is not allowed here.".to_owned(), DoorStyle::Bad));
                }
            }
            _ => {}
        }
    }

    fn title(&self) -> String {
        format!("{} - day {}", self.state.name, self.state.day)
    }

    fn stats(&self) -> String {
        let s = &self.state;
        format!(
            "Coins: {}   Tosses left today: {}   Streak: {} (best {})",
            s.coins, s.left, s.streak, s.best
        )
    }

    /// The line or two that say what keys do now.
    fn prompt(&self) -> Vec<(String, DoorStyle)> {
        match &self.mode {
            Mode::Main => vec![(
                "[h] call heads   [t] call tails   [n] change name   [q] leave".to_owned(),
                DoorStyle::Strong,
            )],
            Mode::Name(buffer) => vec![
                (format!("New name: {buffer}_"), DoorStyle::Strong),
                (
                    "Enter to keep it, Esc to cancel.".to_owned(),
                    DoorStyle::Dim,
                ),
            ],
        }
    }

    fn board(&self) -> [String; 2] {
        [
            format!(
                "The board has tossed {} coins today and {} yesterday.",
                self.world.today, self.world.yesterday
            ),
            format!(
                "Days the board has been keeping count: {}.",
                self.world.days
            ),
        ]
    }

    /// The whole screen as text: the small-window drawing and the look-around view.
    fn lines(&self) -> Vec<(String, DoorStyle)> {
        let mut out = vec![
            (self.title(), DoorStyle::Accent),
            (self.stats(), DoorStyle::Plain),
            (String::new(), DoorStyle::Plain),
        ];
        out.extend(self.prompt());
        out.push((String::new(), DoorStyle::Plain));
        if let Some((text, style)) = &self.message {
            out.push((text.clone(), *style));
            out.push((String::new(), DoorStyle::Plain));
        }
        out.extend(self.board().into_iter().map(|b| (b, DoorStyle::Dim)));
        out
    }

    fn render_text(&self, canvas: &mut Canvas<'_>) {
        for (y, (text, style)) in self.lines().iter().enumerate() {
            let Ok(y) = u16::try_from(y) else { break };
            canvas.text(1, y, text, *style);
        }
    }

    /// The full screen. Every widget is plain ratatui; none of it needs to know the tier, the
    /// colour depth or the transport.
    fn render_panels(&self, canvas: &mut Canvas<'_>) {
        let mut frame = canvas.frame();
        let area = frame.area();
        let accent = frame.style(DoorStyle::Accent);
        let dim = frame.style(DoorStyle::Dim);
        let strong = frame.style(DoorStyle::Strong);

        let outer = Block::bordered()
            .border_style(dim)
            .title(Line::styled(format!(" {} ", self.title()), accent));
        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        let foot_rows =
            2 + u16::try_from(self.prompt().len()).unwrap_or(1) + u16::from(self.message.is_some());
        let [stats, gauge, middle, foot] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(5),
            Constraint::Length(foot_rows),
        ])
        .areas(inner);
        frame.render_widget(Paragraph::new(self.stats()).style(strong), stats);

        // The day's tosses. A full block is drawn with the foreground, so the bar shows
        // without colour too.
        let used = f64::from(DAILY_TOSSES.saturating_sub(self.state.left));
        frame.render_widget(
            Gauge::default()
                .use_unicode(true)
                .ratio((used / f64::from(DAILY_TOSSES)).clamp(0.0, 1.0))
                .label(format!("{used:.0}/{DAILY_TOSSES} tosses used today"))
                .gauge_style(Style::new().fg(AMBER)),
            gauge,
        );

        let [left, right] =
            Layout::horizontal([Constraint::Min(30), Constraint::Length(30)]).areas(middle);
        self.render_table(&mut frame, left);
        let [streak, purse] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(3)]).areas(right);
        self.render_streak(&mut frame, streak);
        self.render_purse(&mut frame, purse);

        let mut text: Vec<Line<'_>> = self
            .prompt()
            .into_iter()
            .map(|(line, style)| Line::styled(line, frame.style(style)))
            .collect();
        if let Some((message, style)) = &self.message {
            text.push(Line::styled(message.clone(), frame.style(*style)));
        }
        text.extend(self.board().into_iter().map(|b| Line::styled(b, dim)));
        frame.render_widget(Paragraph::new(text), foot);
    }

    fn render_table(&self, frame: &mut DoorFrame<'_>, area: Rect) {
        let dim = frame.style(DoorStyle::Dim);
        let block = Block::bordered()
            .border_style(dim)
            .title(Line::styled(" This visit ", frame.style(DoorStyle::Accent)));
        let room = usize::from(block.inner(area).height.saturating_sub(1));
        let skip = self.history.len().saturating_sub(room);
        let rows: Vec<Row<'_>> = self
            .history
            .iter()
            .enumerate()
            .skip(skip)
            .map(|(n, toss)| {
                let (word, style) = if toss.won {
                    ("won", DoorStyle::Good)
                } else {
                    ("lost", DoorStyle::Bad)
                };
                Row::new([
                    Cell::from(format!("{}", n + 1)),
                    Cell::from(name_of(toss.call)),
                    Cell::from(word).style(frame.style(style)),
                    Cell::from(format!("{}", toss.coins)),
                ])
            })
            .collect();
        let table = if rows.is_empty() {
            None
        } else {
            Some(
                Table::new(
                    rows,
                    [
                        Constraint::Length(3),
                        Constraint::Length(6),
                        Constraint::Length(6),
                        Constraint::Min(5),
                    ],
                )
                .header(
                    Row::new(["#", "Call", "Result", "Coins"])
                        .style(frame.style(DoorStyle::Accent)),
                )
                .column_spacing(1),
            )
        };
        let inside = block.inner(area);
        frame.render_widget(block, area);
        match table {
            Some(table) => frame.render_widget(table, inside),
            None => frame.render_widget(
                Paragraph::new("Call heads or tails to start.").style(dim),
                inside,
            ),
        }
    }

    fn render_streak(&self, frame: &mut DoorFrame<'_>, area: Rect) {
        let block = Block::bordered()
            .border_style(frame.style(DoorStyle::Dim))
            .title(Line::styled(" Streak ", frame.style(DoorStyle::Accent)));
        let inside = block.inner(area);
        frame.render_widget(block, area);
        let data: Vec<u64> = self.history.iter().map(|t| u64::from(t.streak)).collect();
        frame.render_widget(
            Sparkline::default()
                .data(&data)
                .style(Style::new().fg(TEAL)),
            inside,
        );
    }

    fn render_purse(&self, frame: &mut DoorFrame<'_>, area: Rect) {
        let dim = frame.style(DoorStyle::Dim);
        let block = Block::bordered()
            .border_style(dim)
            .title(Line::styled(" Purse ", frame.style(DoorStyle::Accent)));
        let inside = block.inner(area);
        frame.render_widget(block, area);
        if self.history.is_empty() {
            return;
        }
        // The purse the visit began with, then after each toss.
        let mut points: Vec<(f64, f64)> = vec![(0.0, f64::from(self.start_coins))];
        for (n, toss) in self.history.iter().enumerate() {
            let x = f64::from(u16::try_from(n + 1).unwrap_or(u16::MAX));
            points.push((x, f64::from(toss.coins)));
        }
        let top = points.iter().map(|p| p.1).fold(1.0, f64::max);
        let bottom = points.iter().map(|p| p.1).fold(top, f64::min);
        let last = points.last().map_or(1.0, |p| p.0.max(1.0));
        let chart = Chart::new(vec![
            Dataset::default()
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style(Style::new().fg(AMBER))
                .data(&points),
        ])
        .x_axis(Axis::default().bounds([0.0, last]))
        .y_axis(Axis::default().bounds([bottom - 1.0, top + 1.0]));
        frame.render_widget(chart, inside);
    }
}

/// A warm colour of the door's own (truecolor; the board brings it down to the member's depth).
const AMBER: Color = Color::Rgb(0xe8, 0xa3, 0x3d);
/// A cool one for the second chart.
const TEAL: Color = Color::Rgb(0x3d, 0xc4, 0xb0);

fn name_of(call: Call) -> &'static str {
    match call {
        Call::Heads => "heads",
        Call::Tails => "tails",
    }
}

impl DoorSession for Session {
    fn captures_text(&self) -> bool {
        matches!(self.mode, Mode::Name(_))
    }

    fn key(&mut self, key: DoorKey, ctx: &mut Ctx<'_>) -> Flow {
        if matches!(self.mode, Mode::Name(_)) {
            self.name_key(key, ctx);
            return Flow::Stay;
        }
        match key {
            DoorKey::Char('h' | 'H') => self.toss(Call::Heads, ctx),
            DoorKey::Char('t' | 'T') => self.toss(Call::Tails, ctx),
            DoorKey::Char('n' | 'N') => self.mode = Mode::Name(String::new()),
            DoorKey::Char('q' | 'Q') | DoorKey::Escape => return Flow::Exit,
            _ => {}
        }
        Flow::Stay
    }

    fn render(&self, canvas: &mut Canvas<'_>, _env: &DoorEnv) {
        canvas.clear();
        if canvas.width() >= PANELS_MIN_COLS && canvas.height() >= PANELS_MIN_ROWS {
            self.render_panels(canvas);
        } else {
            self.render_text(canvas);
        }
    }

    fn look_around(&self, _env: &DoorEnv) -> Vec<String> {
        // The charts say nothing a reader cannot get from the tosses, in words.
        let mut out: Vec<String> = self.lines().into_iter().map(|(text, _)| text).collect();
        if !self.history.is_empty() {
            out.push(String::new());
            out.push("This visit:".to_owned());
            for (n, toss) in self.history.iter().enumerate() {
                out.push(format!(
                    "  {}. called {}, {}, {} coins, streak {}",
                    n + 1,
                    name_of(toss.call),
                    if toss.won { "won" } else { "lost" },
                    toss.coins,
                    toss.streak
                ));
            }
        }
        out
    }
}
