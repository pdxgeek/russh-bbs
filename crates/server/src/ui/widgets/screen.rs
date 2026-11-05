//! The member's **Widgets screen** (UI-10): switch widgets on and off, reorder them and pick
//! which layout is being edited, with a live preview of the result.
//!
//! The screen is a small state machine over keys ([`WidgetsScreen::press`]); it holds no data of
//! its own. What a member may do comes from `russh_bbs_core::widgets` ([`Rules`] and
//! [`Choice`]): a widget the sysop hid is not listed, one locked on cannot be switched off (it
//! can still be moved), one locked off cannot be switched on, and one the member cannot reach
//! is not offered. The preview draws the real widgets, with the real data, in a miniature of the
//! real layout geometry ([`panes::plan`](crate::ui::panes::plan)), so what it shows is what the
//! menu will show at the member's terminal size.
//!
//! Saving goes through `russh_bbs_core::widgets::save` (the shell does it), which needs
//! `account.edit_settings`.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use russh_bbs_core::access::Snapshot;
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::terminal_settings::LayoutId;
use russh_bbs_core::widgets::{Choice, Lock, Rules, WidgetData, WidgetId};

use super::{Cx, Pen, WidgetSettings, assign, widget};
use crate::settings::Look;
use crate::ssh::terminal::Key;
use crate::ui::board::Board;
use crate::ui::classic;
use crate::ui::frame::{ChromeText, FrameSpec, Hint};
use crate::ui::keys;
use crate::ui::panes;
use crate::ui::strings::StringTable;
use crate::ui::theme::Element;

/// The chrome sentences of this screen, as string-table keys.
pub(crate) const TEXT_KEYS: &[&str] = &[
    "ui.widgets.heading",
    "ui.widgets.showing",
    "ui.widgets.source_default",
    "ui.widgets.source_own",
    "ui.widgets.on",
    "ui.widgets.off",
    "ui.widgets.locked_on",
    "ui.widgets.locked_off",
    "ui.widgets.preview",
    "ui.widgets.menu",
    "ui.widgets.none_layout",
    "ui.widgets.hint_toggle",
    "ui.widgets.hint_move",
    "ui.widgets.hint_layout",
    "ui.widgets.hint_reorder",
    "ui.widgets.hint_default",
];

/// The layouts that have widget panes, in the order the screen cycles through them.
pub(crate) const EDITABLE: [LayoutId; 4] = [
    LayoutId::Dashboard,
    LayoutId::Wide,
    LayoutId::Focus,
    LayoutId::Split,
];

/// One row of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) id: WidgetId,
    pub(crate) state: State,
}

/// Where a widget stands for this member in this layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    /// Shown; the member may switch it off.
    On,
    /// Not shown; the member may switch it on.
    Off,
    /// Shown, and the sysop locked it on.
    LockedOn,
    /// The sysop locked it off.
    LockedOff,
}

/// What the screen needs to decide a key.
#[derive(Clone, Copy)]
pub(crate) struct Ctx<'a> {
    pub(crate) rules: &'a Rules,
    pub(crate) choice: &'a Choice,
    pub(crate) access: &'a Snapshot,
}

/// What a key did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Redraw.
    Stay,
    /// Leave the screen.
    Back,
    /// The member's lists changed: keep `choice` and store `list` (`None`: follow the default)
    /// for `layout`.
    Edit {
        choice: Choice,
        layout: LayoutId,
        list: Option<Vec<WidgetId>>,
    },
}

/// The screen's own state: which layout, which row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WidgetsScreen {
    layout: LayoutId,
    cursor: usize,
}

/// The rows of `layout` for this member: what is shown, in order, then the rest the member may
/// see (off, or locked off). Hidden and unreachable widgets are not listed.
#[must_use]
pub(crate) fn rows(layout: LayoutId, ctx: &Ctx<'_>) -> Vec<Row> {
    let shown = ctx.rules.resolve(layout, ctx.choice, ctx.access);
    let mut rows: Vec<Row> = shown
        .iter()
        .map(|id| Row {
            id: *id,
            state: if ctx.rules.lock(*id) == Lock::On {
                State::LockedOn
            } else {
                State::On
            },
        })
        .collect();
    for (id, lock) in ctx.rules.offered(ctx.access) {
        if !shown.contains(&id) {
            rows.push(Row {
                id,
                state: if lock == Lock::Off {
                    State::LockedOff
                } else {
                    State::Off
                },
            });
        }
    }
    rows
}

impl WidgetsScreen {
    /// Open on `layout` if it has widget panes, else on the dashboard.
    #[must_use]
    pub(crate) fn open(layout: LayoutId) -> Self {
        Self {
            layout: if EDITABLE.contains(&layout) {
                layout
            } else {
                LayoutId::Dashboard
            },
            cursor: 0,
        }
    }

    /// The layout being edited.
    #[must_use]
    #[cfg_attr(not(test), expect(dead_code, reason = "read by tests"))]
    pub(crate) const fn layout(&self) -> LayoutId {
        self.layout
    }

    /// The row under the cursor.
    #[must_use]
    #[cfg_attr(not(test), expect(dead_code, reason = "read by tests"))]
    pub(crate) const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Apply one key.
    pub(crate) fn press(&mut self, key: Key, ctx: &Ctx<'_>) -> Outcome {
        let rows = rows(self.layout, ctx);
        self.cursor = self.cursor.min(rows.len().saturating_sub(1));
        match key {
            Key::Escape | Key::Char('b' | 'B' | 'q' | 'Q') => Outcome::Back,
            Key::Up | Key::Char('k') => {
                self.cursor = self.cursor.saturating_sub(1);
                Outcome::Stay
            }
            Key::Down | Key::Char('j') => {
                self.cursor = (self.cursor + 1).min(rows.len().saturating_sub(1));
                Outcome::Stay
            }
            Key::Left | Key::Char('h') => self.next_layout(-1),
            Key::Right | Key::Char('l' | 'L') | Key::Ctrl('i') => self.next_layout(1),
            Key::Enter | Key::Char(' ' | 'x' | 'X') => self.toggle(&rows, ctx),
            Key::Char('u' | 'U') => self.shift(&rows, ctx, -1),
            Key::Char('d' | 'D') => self.shift(&rows, ctx, 1),
            Key::Char('r' | 'R') => {
                if ctx.choice.get(self.layout).is_none() {
                    return Outcome::Stay;
                }
                Outcome::Edit {
                    choice: ctx.choice.clone().reset(self.layout),
                    layout: self.layout,
                    list: None,
                }
            }
            _ => Outcome::Stay,
        }
    }

    fn next_layout(&mut self, by: i8) -> Outcome {
        let at = EDITABLE.iter().position(|l| *l == self.layout).unwrap_or(0);
        let next = if by < 0 {
            (at + EDITABLE.len() - 1) % EDITABLE.len()
        } else {
            (at + 1) % EDITABLE.len()
        };
        self.layout = EDITABLE[next];
        self.cursor = 0;
        Outcome::Stay
    }

    fn toggle(&mut self, rows: &[Row], ctx: &Ctx<'_>) -> Outcome {
        let Some(row) = rows.get(self.cursor) else {
            return Outcome::Stay;
        };
        if !matches!(row.state, State::On | State::Off) {
            return Outcome::Stay;
        }
        // The list a toggle starts from is what the member sees now, so a default list the
        // member never saved is saved as shown with the change applied.
        let shown = ctx.rules.resolve(self.layout, ctx.choice, ctx.access);
        let start = Choice::from_lists([(self.layout, shown)]);
        let Ok(choice) = start.toggle(self.layout, row.id, ctx.rules, true) else {
            return Outcome::Stay;
        };
        let list = choice.get(self.layout).map(<[WidgetId]>::to_vec);
        Outcome::Edit {
            choice: merge(ctx.choice, self.layout, &choice),
            layout: self.layout,
            list,
        }
    }

    fn shift(&mut self, rows: &[Row], ctx: &Ctx<'_>, by: i8) -> Outcome {
        let Some(row) = rows.get(self.cursor) else {
            return Outcome::Stay;
        };
        if !matches!(row.state, State::On | State::LockedOn) {
            return Outcome::Stay;
        }
        let shown = ctx.rules.resolve(self.layout, ctx.choice, ctx.access);
        let Ok(choice) = ctx.choice.shift(self.layout, row.id, by, &shown) else {
            return Outcome::Stay;
        };
        let moved = if by < 0 {
            self.cursor.saturating_sub(1)
        } else {
            (self.cursor + 1).min(shown.len().saturating_sub(1))
        };
        self.cursor = moved;
        let list = choice.get(self.layout).map(<[WidgetId]>::to_vec);
        Outcome::Edit {
            choice,
            layout: self.layout,
            list,
        }
    }
}

/// `base` with `layout`'s list taken from `edited`.
fn merge(base: &Choice, layout: LayoutId, edited: &Choice) -> Choice {
    let mut lists: Vec<(LayoutId, Vec<WidgetId>)> = base
        .lists()
        .filter(|(l, _)| *l != layout)
        .map(|(l, list)| (l, list.to_vec()))
        .collect();
    if let Some(list) = edited.get(layout) {
        lists.push((layout, list.to_vec()));
    }
    Choice::from_lists(lists)
}

/// What drawing the screen needs.
pub(crate) struct DrawIn<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    pub(crate) markers: &'a [SecurityMarker],
    pub(crate) clock: Option<&'a str>,
    pub(crate) ctx: Ctx<'a>,
    pub(crate) data: &'a WidgetData,
    pub(crate) board: &'a Board,
    pub(crate) settings: &'a WidgetSettings,
    pub(crate) now: i64,
}

/// Draw the screen: the frame's bars, the list on the left and the preview on the right.
pub(crate) fn render(frame: &mut Frame<'_>, screen: &WidgetsScreen, input: &DrawIn<'_>) {
    let strings = input.strings;
    let hints = [
        Hint::new(
            ChromeText::fixed("Up/Down"),
            ChromeText::table(strings, "ui.widgets.hint_move"),
        ),
        Hint::new(
            ChromeText::fixed("Space"),
            ChromeText::table(strings, "ui.widgets.hint_toggle"),
        ),
        Hint::new(
            ChromeText::fixed("u/d"),
            ChromeText::table(strings, "ui.widgets.hint_reorder"),
        ),
        Hint::new(
            ChromeText::fixed("Tab"),
            ChromeText::table(strings, "ui.widgets.hint_layout"),
        ),
        Hint::new(
            ChromeText::fixed("r"),
            ChromeText::table(strings, "ui.widgets.hint_default"),
        ),
        Hint::new(
            ChromeText::fixed("Esc"),
            ChromeText::table(strings, keys::HINT_BACK),
        ),
    ];
    let body = FrameSpec::new(strings, input.look, input.tier)
        .markers(input.markers)
        .place(ChromeText::table(strings, "ui.widgets.heading"))
        .clock(input.clock)
        .hints(&hints)
        .draw(frame);
    if body.width < 30 || body.height < 6 {
        return;
    }
    let list_width = if body.width >= 90 {
        40
    } else {
        32.min(body.width / 2)
    };
    let [left, right] =
        Layout::horizontal([Constraint::Length(list_width), Constraint::Fill(1)]).areas(body);
    draw_list(frame, left, screen, input);
    draw_preview(frame, right, screen, input);
}

fn draw_list(frame: &mut Frame<'_>, area: Rect, screen: &WidgetsScreen, input: &DrawIn<'_>) {
    let look = input.look;
    let strings = input.strings;
    let pen = Pen(look);
    let layout_name = strings.text(keys::LAYOUT_NAMES[classic::layout_index(screen.layout)]);
    let source = if input.ctx.choice.get(screen.layout).is_some() {
        "ui.widgets.source_own"
    } else {
        "ui.widgets.source_default"
    };
    let inner = look
        .panel(
            frame,
            area,
            Some(strings.text("ui.widgets.heading")),
            Element::Pane,
            false,
        )
        .inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
    let rows = rows(screen.layout, &input.ctx);
    let mut lines: Vec<Line<'static>> = vec![
        Line::from(vec![Span::styled(
            strings.filled("ui.widgets.showing", &[("layout", layout_name)]),
            pen.heading(),
        )]),
        Line::styled(format!("({})", strings.text(source)), pen.muted()),
    ];
    let room = usize::from(inner.height).saturating_sub(lines.len());
    // Keep the cursor row in view.
    let top = screen.cursor.saturating_sub(room.saturating_sub(1));
    let width = usize::from(inner.width);
    for (at, row) in rows.iter().enumerate().skip(top).take(room) {
        let title = strings.text(widget(row.id).title_key());
        let (mark, note, base) = match row.state {
            State::On => (
                if input.tier == Tier::Ascii {
                    "[x]"
                } else {
                    "[\u{2713}]"
                },
                "",
                pen.text(),
            ),
            State::Off => ("[ ]", "", pen.muted()),
            State::LockedOn => (
                if input.tier == Tier::Ascii {
                    "[x]"
                } else {
                    "[\u{2713}]"
                },
                strings.text("ui.widgets.locked_on"),
                pen.text(),
            ),
            State::LockedOff => ("[-]", strings.text("ui.widgets.locked_off"), pen.muted()),
        };
        let place = rows[..at]
            .iter()
            .filter(|r| matches!(r.state, State::On | State::LockedOn))
            .count()
            + 1;
        let order = if matches!(row.state, State::On | State::LockedOn) {
            format!("{place:>2}")
        } else {
            "  ".to_owned()
        };
        let selected = at == screen.cursor;
        let pointer = if selected { "> " } else { "  " };
        let style = if selected {
            look.accent(
                Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                look.palette().selected,
            )
            .patch(look.bg(look.surfaces().select_bg))
        } else {
            base
        };
        let mut text = format!("{pointer}{order} {mark} {title}");
        if !note.is_empty() {
            let gap = width.saturating_sub(text.chars().count() + note.chars().count());
            text.push_str(&" ".repeat(gap.max(1)));
            text.push_str(note);
        }
        let text: String = text.chars().take(width).collect();
        lines.push(Line::styled(text, style));
    }
    frame.render_widget(Paragraph::new(lines).style(look.text()), inner);
}

/// A rectangle of the real layout scaled into `into`, kept at least one cell and inside it.
fn scaled(rect: Rect, from: Rect, into: Rect) -> Rect {
    let sx = |v: u16| u32::from(v) * u32::from(into.width) / u32::from(from.width.max(1));
    let sy = |v: u16| u32::from(v) * u32::from(into.height) / u32::from(from.height.max(1));
    let x = into.x
        + u16::try_from(sx(rect.x - from.x))
            .unwrap_or(0)
            .min(into.width.saturating_sub(1));
    let y = into.y
        + u16::try_from(sy(rect.y - from.y))
            .unwrap_or(0)
            .min(into.height.saturating_sub(1));
    let w = u16::try_from(sx(rect.width)).unwrap_or(1).max(1);
    let h = u16::try_from(sy(rect.height)).unwrap_or(1).max(1);
    Rect {
        x,
        y,
        width: w.min(into.x + into.width - x),
        height: h.min(into.y + into.height - y),
    }
}

fn draw_preview(frame: &mut Frame<'_>, area: Rect, screen: &WidgetsScreen, input: &DrawIn<'_>) {
    let look = input.look;
    let strings = input.strings;
    let pen = Pen(look);
    let inner = look
        .panel(
            frame,
            area,
            Some(strings.text("ui.widgets.preview")),
            Element::Pane,
            false,
        )
        .inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
    if inner.width < 8 || inner.height < 3 {
        return;
    }
    // The body the menu would have at this terminal size: the whole screen less the bars.
    let full = frame.area();
    let real = Rect::new(0, 0, full.width, full.height.saturating_sub(3).max(1));
    let plan = panes::plan(screen.layout, real);
    if plan.cells.is_empty() {
        frame.render_widget(
            Paragraph::new(strings.filled(
                "ui.widgets.none_layout",
                &[(
                    "layout",
                    strings.text(keys::LAYOUT_NAMES[classic::layout_index(screen.layout)]),
                )],
            ))
            .style(pen.muted()),
            inner,
        );
        return;
    }
    // The menu pane, as a quiet box, so the preview reads as the whole screen.
    let nav = scaled(plan.nav, real, inner);
    if nav.width >= 4 && nav.height >= 3 {
        look.panel(
            frame,
            nav,
            Some(strings.text("ui.widgets.menu")),
            Element::Pane,
            false,
        );
    }
    let shown = input
        .ctx
        .rules
        .resolve(screen.layout, input.ctx.choice, input.ctx.access);
    let cx = Cx::new(
        look,
        input.tier,
        strings,
        input.data,
        input.board,
        input.settings,
        input.now,
    );
    // Assign by the real cells (so which widget lands where matches the menu), draw in the
    // scaled ones.
    let placed = assign(&plan.cells, &shown, &cx);
    for (id, cell) in placed {
        let mini = scaled(cell, real, inner);
        if mini.width < 4 || mini.height < 3 {
            continue;
        }
        let w = widget(id);
        let body = look
            .panel(
                frame,
                mini,
                Some(strings.text(w.title_key())),
                Element::Pane,
                false,
            )
            .inner(Margin {
                horizontal: 1,
                vertical: 0,
            });
        if body.width > 0 && body.height > 0 {
            w.draw(frame, body, &cx);
        }
    }
}
