//! The widget system (UI-10, D-TBD): small live panels the pane layouts place in their cells.
//!
//! **Built in, not plug-ins.** [`Widget`] is implemented once per [`WidgetId`] in
//! [`kinds`]; [`widget`] is the registry. A widget never reads the store: the shell fetches
//! [`WidgetData`] through `russh_bbs_core::widgets::fetch` (which asks the same services the
//! screens ask, so a widget cannot show a board, mail or poll the viewer cannot open) and the
//! server adds the live pieces (who is online, chat rooms, doors, status) after checking
//! [`WidgetId::reachable_by`]. The widget only draws what it is handed.
//!
//! **Rules every widget follows.**
//! * It draws into the `Rect` it is given, with the theme's palette roles, and nothing else: a
//!   cell comes from [`panes::plan`](super::panes::plan), which never includes the status bar, so
//!   a widget cannot reach the marker cell.
//! * Colour comes from the look, so mono draws weight and underline only; no widget uses reverse
//!   video (that is the marker's reserved style) or a Nerd Font glyph.
//! * Chrome words come from the string table; every piece of member text goes through
//!   [`Cx::member`], the shared sanitizer, before it is drawn.
//! * In the ASCII tier every widget draws seven-bit characters only ([`Widget::text`] is that
//!   form, also used by the focus layout's pop-over).
//! * A widget with nothing to show ([`Widget::has_content`] is false) collapses: it takes no cell.
//! * Each widget has a refresh budget ([`Widget::budget`]); the session refreshes data no
//!   faster than the smallest budget among the widgets it shows, never faster than
//!   [`MIN_REFRESH`], far under the D49 redraw cap.

pub(crate) mod gfx;
mod kinds;
pub(crate) mod screen;
pub mod settings;

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::widgets::{WidgetData, WidgetId};

pub use settings::{WidgetSettings, WidgetsError};

use super::board::Board;
use super::strings::StringTable;
use super::theme::Element;
use crate::settings::Look;

/// The fastest the session refreshes widget data on its own, whatever the widgets ask: one
/// timer-driven redraw in five seconds is 0.2 a second against D49's cap of 20.
pub const MIN_REFRESH: Duration = Duration::from_secs(5);

/// Everything a widget draws from.
#[derive(Clone, Copy)]
pub(crate) struct Cx<'a> {
    pub(crate) look: Look,
    pub(crate) strings: &'a StringTable,
    pub(crate) data: &'a WidgetData,
    pub(crate) board: &'a Board,
    pub(crate) settings: &'a WidgetSettings,
    /// Unix seconds now (the clock, "5m ago" and the calendar).
    pub(crate) now: i64,
    /// Draw seven-bit characters only.
    pub(crate) ascii: bool,
}

impl<'a> Cx<'a> {
    /// A context for the member's icon tier: the ASCII tier draws the ASCII forms.
    pub(crate) fn new(
        look: Look,
        tier: Tier,
        strings: &'a StringTable,
        data: &'a WidgetData,
        board: &'a Board,
        settings: &'a WidgetSettings,
        now: i64,
    ) -> Self {
        Self {
            look,
            strings,
            data,
            board,
            settings,
            now,
            ascii: tier == Tier::Ascii,
        }
    }

    /// The same context drawing the ASCII form, whatever the tier (the text form).
    pub(crate) fn in_ascii(mut self) -> Self {
        self.ascii = true;
        self
    }

    /// Member text, ready to draw: control characters, escapes, invisible and private-use
    /// characters and the reserved marker glyphs are gone or neutralised, at most `max`
    /// characters, one line. In ASCII, anything left that is not ASCII becomes `?`.
    pub(crate) fn member(&self, text: &str, max: usize) -> String {
        let clean = SingleLine::new(max).sanitize(text).into_string();
        if self.ascii {
            clean
                .chars()
                .map(|c| if c.is_ascii() { c } else { '?' })
                .collect()
        } else {
            clean
        }
    }

    /// A string-table sentence with `{n}` filled.
    pub(crate) fn said(&self, key: &str, n: impl std::fmt::Display) -> String {
        self.strings.filled(key, &[("n", &n.to_string())])
    }
}

/// What a menu screen is given to fill its widget cells: the widgets to show, in order, the data
/// to show them with, and the time.
#[derive(Clone, Copy)]
pub(crate) struct WidgetView<'a> {
    pub(crate) shown: &'a [WidgetId],
    pub(crate) data: &'a WidgetData,
    pub(crate) settings: &'a WidgetSettings,
    pub(crate) now: i64,
}

#[cfg(test)]
impl WidgetView<'static> {
    /// No widgets and no data: the layouts draw their cells empty.
    pub(crate) fn none() -> Self {
        static DATA: std::sync::OnceLock<WidgetData> = std::sync::OnceLock::new();
        static SETTINGS: std::sync::OnceLock<WidgetSettings> = std::sync::OnceLock::new();
        Self {
            shown: &[],
            data: DATA.get_or_init(WidgetData::default),
            settings: SETTINGS.get_or_init(WidgetSettings::default),
            now: 0,
        }
    }
}

/// The styles widgets use, all from the theme's palette roles (none has reverse video).
#[derive(Clone, Copy)]
pub(crate) struct Pen(pub(crate) Look);

impl Pen {
    pub(crate) fn text(self) -> Style {
        self.0.fg(self.0.palette().text)
    }
    pub(crate) fn muted(self) -> Style {
        self.0.muted()
    }
    pub(crate) fn heading(self) -> Style {
        self.0.accent(
            Style::new().add_modifier(Modifier::BOLD),
            self.0.palette().heading,
        )
    }
    pub(crate) fn accent(self) -> Style {
        self.0.accent(
            Style::new().add_modifier(Modifier::BOLD),
            self.0.palette().accent,
        )
    }
    pub(crate) fn accent2(self) -> Style {
        self.0.accent(Style::new(), self.0.palette().accent2)
    }
    pub(crate) fn key(self) -> Style {
        self.0.accent(
            Style::new().add_modifier(Modifier::BOLD),
            self.0.palette().key,
        )
    }
    pub(crate) fn good(self) -> Style {
        self.0.accent(
            Style::new().add_modifier(Modifier::BOLD),
            self.0.palette().success,
        )
    }
    pub(crate) fn warn(self) -> Style {
        self.0.accent(Style::new(), self.0.palette().warning)
    }
    pub(crate) fn bad(self) -> Style {
        self.0.accent(Style::new(), self.0.palette().error)
    }
    pub(crate) fn stamp(self) -> Style {
        self.0.accent(Style::new(), self.0.palette().timestamp)
    }
    pub(crate) fn quote(self) -> Style {
        self.0.accent(
            Style::new().add_modifier(Modifier::ITALIC),
            self.0.palette().quote,
        )
    }
    /// Today in a calendar: bold and underlined, which survives mono.
    pub(crate) fn today(self) -> Style {
        self.0.accent(
            Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            self.0.palette().accent,
        )
    }
}

/// One built-in widget.
pub(crate) trait Widget: Sync {
    /// The widget's id, which never changes.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the registry is keyed by id; tests check the two agree"
        )
    )]
    fn id(&self) -> WidgetId;
    /// The string-table key of the panel title.
    fn title_key(&self) -> &'static str;
    /// The smallest inside (columns, rows) it draws sensibly in. A smaller cell skips it.
    fn min_size(&self) -> (u16, u16);
    /// The inside it looks best in.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "a size hint for the Widgets screen and tests")
    )]
    fn preferred(&self) -> (u16, u16);
    /// How often its data may be refreshed. Never below [`MIN_REFRESH`]; static widgets say an
    /// hour.
    fn budget(&self) -> Duration;
    /// Whether it has anything to show; a widget without collapses.
    fn has_content(&self, cx: &Cx<'_>) -> bool;
    /// The widget as styled lines for an inside of `width` x `height`. In the ASCII tier
    /// (`cx.ascii`) the lines hold seven-bit characters only. At most `height` lines are used.
    fn lines(&self, cx: &Cx<'_>, width: u16, height: u16) -> Vec<Line<'static>>;

    /// Draw into `area` (the inside of the panel).
    fn draw(&self, frame: &mut Frame<'_>, area: Rect, cx: &Cx<'_>) {
        let lines = self.lines(cx, area.width, area.height);
        frame.render_widget(Paragraph::new(lines).style(cx.look.text()), area);
    }

    /// The ASCII text form: the same lines, seven-bit characters, no styling.
    fn text(&self, cx: &Cx<'_>, width: u16) -> Vec<String> {
        let ascii = cx.in_ascii();
        self.lines(&ascii, width, u16::MAX)
            .into_iter()
            .map(|line| line.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }
}

/// The widget for `id`.
#[must_use]
pub(crate) fn widget(id: WidgetId) -> &'static dyn Widget {
    kinds::widget(id)
}

/// The string-table keys of every widget title and label, checked with the framework's keys.
pub(crate) use kinds::TEXT_KEYS;

/// The refresh interval for a session showing `shown`: the smallest budget among them, never
/// below [`MIN_REFRESH`]. `None` when no widget is shown, so no timer runs.
#[must_use]
pub(crate) fn refresh_interval(shown: &[WidgetId]) -> Option<Duration> {
    shown
        .iter()
        .map(|id| widget(*id).budget().max(MIN_REFRESH))
        .min()
}

/// The inside of a widget cell: the panel's inner rectangle with a column of padding each side.
#[must_use]
pub(crate) fn inside(look: Look, cell: Rect) -> Rect {
    look.block(None, Element::Pane, false)
        .inner(cell)
        .inner(Margin {
            horizontal: 1,
            vertical: 0,
        })
}

/// Which widget goes in which cell: widgets in order, each into the first remaining cell it
/// fits, skipping widgets with nothing to show. A widget too big for every remaining cell is
/// left out (it does not squeeze).
#[must_use]
pub(crate) fn assign(cells: &[Rect], shown: &[WidgetId], cx: &Cx<'_>) -> Vec<(WidgetId, Rect)> {
    let mut waiting: Vec<WidgetId> = shown
        .iter()
        .copied()
        .filter(|id| widget(*id).has_content(cx))
        .collect();
    let mut placed = Vec::new();
    for cell in cells {
        let room = inside(cx.look, *cell);
        let fit = waiting.iter().position(|id| {
            let (w, h) = widget(*id).min_size();
            room.width >= w && room.height >= h
        });
        if let Some(at) = fit {
            placed.push((waiting.remove(at), *cell));
        }
    }
    placed
}

/// Draw `placed` widgets, each in a titled panel in its cell.
pub(crate) fn draw_cells(frame: &mut Frame<'_>, placed: &[(WidgetId, Rect)], cx: &Cx<'_>) {
    for (id, cell) in placed {
        if cell.width < 4 || cell.height < 3 {
            continue;
        }
        let widget = widget(*id);
        let title = cx.strings.text(widget.title_key());
        let panel = cx
            .look
            .panel(frame, *cell, Some(title), Element::Pane, false);
        let inner = panel.inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
        if inner.width == 0 || inner.height == 0 {
            continue;
        }
        widget.draw(frame, inner, cx);
    }
}

/// The focus layout's pop-over body: each shown widget's title and its text form, as many as
/// fit in `height` rows of `width` columns.
#[must_use]
pub(crate) fn popover_lines(
    shown: &[WidgetId],
    cx: &Cx<'_>,
    width: u16,
    height: u16,
) -> Vec<Line<'static>> {
    let pen = Pen(cx.look);
    let mut out: Vec<Line<'static>> = Vec::new();
    for id in shown {
        let widget = widget(*id);
        if !widget.has_content(cx) {
            continue;
        }
        // The ASCII tier shows the plain text form of each widget.
        let body: Vec<Line<'static>> = if cx.ascii {
            widget
                .text(cx, width)
                .into_iter()
                .take(3)
                .map(|text| Line::styled(text, pen.text()))
                .collect()
        } else {
            widget.lines(cx, width, 3)
        };
        let need = 1 + body.len();
        if out.len() + need > usize::from(height) {
            break;
        }
        out.push(Line::styled(
            cx.strings.text(widget.title_key()).to_owned(),
            pen.heading(),
        ));
        out.extend(body);
    }
    out
}

/// Draw the pop-over lines wrapped nowhere (they are already cut to width).
pub(crate) fn draw_lines(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>, look: Look) {
    frame.render_widget(
        Paragraph::new(lines)
            .style(look.text())
            .wrap(Wrap { trim: false }),
        area,
    );
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
