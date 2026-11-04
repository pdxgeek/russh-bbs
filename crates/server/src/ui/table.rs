//! A table with a header row, a highlighted row, zebra stripes and a scrollbar (UI-1, D100).
//!
//! ```text
//! +- Forums -----------------------------------------------------+
//! |  #  Board            Threads  About                           |
//! |> 1  General               12  Anything goes                   #
//! |  2  Tech                   4  Software and hardware           #
//! +----------------------------------------------------- 1-2 of 9 +
//! ```
//!
//! **Using it.** Build a [`TableSpec`] (columns, rows, which row is selected) and call
//! [`render`] with the rectangle to fill. The table keeps the selected row in view, draws the
//! visible slice, and returns a [`TableView`] (`top`, `shown`) that the screen stores so its
//! paging keys know how many rows are on screen. The caller passes the previous `top` back in,
//! so the window moves only when the selection leaves it.
//!
//! **Content and chrome.** A [`Cell`] is built either from chrome (`Cell::chrome`) or from
//! content (`Cell::content`). Content is sanitized again inside the constructor, with the shared
//! sanitizer, so a caller that forgot to clean a handle still cannot put an escape sequence or a
//! marker codepoint on screen. Column titles are chrome. Nothing here applies the reserved
//! marker style: the selected row uses a pointer, bold and underline (and the theme's selection
//! colour when colour is on), never reverse video (D39), so the table cannot imitate a marker.
//!
//! **Tiers.** In the ASCII tier the border, the scrollbar and the ellipsis are plain ASCII. With
//! colour off the selected row is the pointer plus bold and underline, and the header is bold and
//! underlined, so it reads on a mono terminal.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table,
};
use russh_bbs_core::sanitize::SingleLine;

use super::frame::ChromeText;
use crate::settings::Look;

/// Longest cell text kept, in characters, whatever the column.
const CELL_MAX: usize = 240;

/// Printable-ASCII box for the ASCII tier.
pub(crate) const ASCII_BORDER: border::Set = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

/// How wide a column is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Width {
    /// Exactly this many columns.
    Fixed(u16),
    /// At least this many, taking no more than its content (never shrinks below the minimum).
    Min(u16),
    /// A share of what is left, by weight.
    Fill(u16),
}

/// One column: its title, width and, for a column that gives way on a narrow terminal, the
/// narrowest table it is shown in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Column<'a> {
    pub(crate) title: ChromeText<'a>,
    pub(crate) width: Width,
    pub(crate) align_right: bool,
    pub(crate) shown_from: u16,
}

impl<'a> Column<'a> {
    pub(crate) const fn new(title: ChromeText<'a>, width: Width) -> Self {
        Self {
            title,
            width,
            align_right: false,
            shown_from: 0,
        }
    }

    /// Right-align the column's text (numbers).
    #[must_use]
    pub(crate) const fn right(mut self) -> Self {
        self.align_right = true;
        self
    }

    /// Drop this column when the table's inner width is below `width`.
    #[must_use]
    pub(crate) const fn from_width(mut self, width: u16) -> Self {
        self.shown_from = width;
        self
    }
}

/// How a cell's text is styled. Never the reserved marker style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellStyle {
    Plain,
    Dim,
    Key,
    Bold,
}

/// One cell: chrome words or sanitized content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cell {
    text: String,
    style: CellStyle,
}

impl Cell {
    /// Content from a member, a feed or the store: sanitized here.
    pub(crate) fn content(text: &str) -> Self {
        Self {
            text: SingleLine::new(CELL_MAX).sanitize(text).into_string(),
            style: CellStyle::Plain,
        }
    }

    /// Fixed chrome words or a number the code formatted.
    pub(crate) fn chrome(text: ChromeText<'_>) -> Self {
        Self {
            text: text_of(text),
            style: CellStyle::Plain,
        }
    }

    /// A number or other text the code itself formatted (a count, a date): no member text.
    pub(crate) fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: CellStyle::Plain,
        }
    }

    #[must_use]
    pub(crate) const fn styled(mut self, style: CellStyle) -> Self {
        self.style = style;
        self
    }
}

fn text_of(text: ChromeText<'_>) -> String {
    text.as_str().to_owned()
}

/// One row: its cells and, for a two-line row, a dimmer detail line under them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableRow {
    pub(crate) cells: Vec<Cell>,
    pub(crate) detail: Option<Cell>,
}

impl TableRow {
    pub(crate) fn new(cells: Vec<Cell>) -> Self {
        Self {
            cells,
            detail: None,
        }
    }

    /// A second line under the row's cells (a search snippet).
    #[must_use]
    pub(crate) fn with_detail(mut self, detail: Cell) -> Self {
        self.detail = Some(detail);
        self
    }
}

/// What to draw.
pub(crate) struct TableSpec<'a> {
    /// The block's title: chrome, or content that is sanitized when it is drawn.
    pub(crate) title: Title<'a>,
    pub(crate) columns: Vec<Column<'a>>,
    pub(crate) rows: Vec<TableRow>,
    /// The selected row, an index into `rows`; `None` for a list picked by hotkeys.
    pub(crate) selected: Option<usize>,
    /// Draw the border (a table that is a whole box) or not (a table inside another box).
    pub(crate) bordered: bool,
    /// The first row shown last time, so the window moves only when it has to.
    pub(crate) top: usize,
    /// Shown instead of rows when there are none (chrome).
    pub(crate) empty: &'a str,
    /// Show the `1`-`9` jump number in a first column.
    pub(crate) numbered: bool,
    /// A line under the table inside the border, such as a page number (chrome).
    pub(crate) footer: Option<String>,
}

/// A table or popup title.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Title<'a> {
    None,
    Chrome(ChromeText<'a>),
    /// Content (a board name): sanitized on the way in.
    Content(&'a str),
}

impl Title<'_> {
    pub(crate) fn text(self) -> Option<String> {
        match self {
            Self::None => None,
            Self::Chrome(text) => Some(text_of(text)),
            Self::Content(text) => Some(SingleLine::new(60).sanitize(text).into_string()),
        }
    }
}

/// Where the table's window ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct TableView {
    /// The first row drawn.
    pub(crate) top: usize,
    /// How many rows are drawn.
    pub(crate) shown: usize,
}

/// The border block for a framed region: the theme's border colour, plain ASCII in the ASCII
/// tier, a centred title. Shared by tables, popups and the converted menu screens.
pub(crate) fn framed(look: Look, title: Option<String>) -> Block<'static> {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_style(look.accent(Style::new(), look.palette().border));
    if look.ascii() {
        block = block.border_set(ASCII_BORDER);
    } else {
        block = block.border_type(ratatui::widgets::BorderType::Rounded);
    }
    if let Some(title) = title {
        let heading = look.accent(
            Style::new().add_modifier(Modifier::BOLD),
            look.palette().heading,
        );
        block = block
            .title(Line::from(Span::styled(format!(" {title} "), heading)))
            .title_alignment(Alignment::Left);
    }
    block
}

/// Cut `text` to `width` columns, ending with an ellipsis when it was cut.
pub(crate) fn clip(text: &str, width: usize, ascii: bool) -> String {
    if Span::raw(text).width() <= width {
        return text.to_owned();
    }
    let ellipsis = if ascii { "..." } else { "\u{2026}" };
    let room = width.saturating_sub(Span::raw(ellipsis).width());
    let mut out = String::new();
    for c in text.chars() {
        out.push(c);
        if Span::raw(out.as_str()).width() > room {
            out.pop();
            break;
        }
    }
    if width == 0 {
        return String::new();
    }
    out.push_str(ellipsis);
    out
}

/// Draw the table in `area` and return the window it drew.
pub(crate) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    look: Look,
    spec: &TableSpec<'_>,
) -> TableView {
    let block = if spec.bordered {
        framed(look, spec.title.text())
    } else {
        Block::default()
    };
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return TableView::default();
    }
    let total = spec.rows.len();
    let two_line = spec.rows.iter().any(|row| row.detail.is_some());
    let row_height = if two_line { 2 } else { 1 };
    // The header takes a row when there is room for it and one data row besides.
    let header_rows = u16::from(inner.height >= 2 + row_height);
    let footer_rows = u16::from(spec.footer.is_some() && inner.height > header_rows + row_height);
    let data_height = inner.height - header_rows - footer_rows;
    let capacity = usize::from(data_height / row_height).max(1);
    let visible = capacity.min(total.max(1));
    let selected = spec.selected.map(|at| at.min(total.saturating_sub(1)));
    let mut top = spec.top.min(total.saturating_sub(visible));
    if let Some(selected) = selected {
        if selected < top {
            top = selected;
        } else if selected >= top + visible {
            top = selected + 1 - visible;
        }
    }
    let shown = visible.min(total.saturating_sub(top));

    // The scrollbar sits on the right border, so the table keeps its columns.
    let overflow = total > shown;
    let body_width = inner.width;
    // Columns that give way on a narrow table are dropped with their cells.
    let shown_at: Vec<usize> = spec
        .columns
        .iter()
        .enumerate()
        .filter(|(_, column)| column.shown_from <= body_width)
        .map(|(at, _)| at)
        .collect();
    let columns: Vec<&Column<'_>> = shown_at.iter().map(|at| &spec.columns[*at]).collect();
    let number_width = u16::from(spec.numbered) * 3;
    let widths = column_widths(&columns, body_width.saturating_sub(2 + number_width));

    let ascii = look.ascii();
    let surfaces = look.surfaces();
    let head_style = Style::new()
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        .patch(look.bg(surfaces.head_bg))
        .patch(look.fg(surfaces.bar_fg));
    let select_style = Style::new()
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        .patch(look.bg(surfaces.select_bg))
        .patch(look.fg(surfaces.select_fg));
    let stripe_style = look.bg(surfaces.stripe_bg);
    let key_style = look.accent(
        Style::new().add_modifier(Modifier::BOLD),
        look.palette().key,
    );
    let dim = Style::new().add_modifier(Modifier::DIM);

    let mut constraints: Vec<Constraint> = Vec::new();
    constraints.extend(widths.iter().map(|w| Constraint::Length(*w)));

    let cells_of = |at: usize, row: &TableRow| -> Row<'static> {
        let mut cells: Vec<ratatui::widgets::Cell<'static>> = Vec::new();
        let picked = shown_at.iter().filter_map(|at| row.cells.get(*at));
        for (cell, (column, width)) in picked.zip(columns.iter().zip(&widths)) {
            let text = clip(&cell.text, usize::from(*width), ascii);
            let style = match cell.style {
                CellStyle::Plain => Style::new(),
                CellStyle::Dim => dim,
                CellStyle::Key => key_style,
                CellStyle::Bold => Style::new().add_modifier(Modifier::BOLD),
            };
            let line = Line::from(Span::styled(text, style));
            cells.push(ratatui::widgets::Cell::from(if column.align_right {
                line.alignment(Alignment::Right)
            } else {
                line
            }));
        }
        Row::new(cells)
    };

    let data_area = Rect {
        y: inner.y + header_rows,
        height: data_height,
        ..inner
    };
    if header_rows == 1 {
        draw_header(
            frame,
            inner,
            &columns,
            &widths,
            spec.numbered,
            head_style,
            ascii,
        );
    }
    if total == 0 {
        let message =
            Paragraph::new(Line::styled(spec.empty.to_owned(), dim)).alignment(Alignment::Center);
        frame.render_widget(message, data_area);
        return TableView::default();
    }
    // Rows are drawn one by one into their own rectangles: the pointer, the stripe and an
    // optional detail line then never depend on how a table widget measures its cells.
    let pointer = if ascii { "> " } else { "\u{25b8} " };
    for (offset, at) in (top..top + shown).enumerate() {
        let Ok(offset) = u16::try_from(offset) else {
            break;
        };
        let y = data_area.y + offset * row_height;
        if y >= data_area.y + data_area.height {
            break;
        }
        let row_area = Rect {
            y,
            height: row_height.min(data_area.y + data_area.height - y),
            ..data_area
        };
        let row = &spec.rows[at];
        let chosen = selected == Some(at);
        let first = Rect {
            height: 1,
            ..row_area
        };
        let base = if chosen {
            select_style
        } else if at % 2 == 1 {
            stripe_style
        } else {
            Style::new()
        };
        frame.render_widget(Block::default().style(base), row_area);
        // The pointer column: a word-free cue that survives a mono terminal.
        let lead = Rect {
            width: (2 + number_width).min(first.width),
            ..first
        };
        let lead_text = if chosen { pointer } else { "  " };
        let number = at - top + 1;
        let number_text = if spec.numbered && number <= 9 {
            format!("{number}.")
        } else {
            String::new()
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(lead_text.to_owned(), base.patch(key_style)),
                Span::styled(number_text, base.patch(key_style)),
            ])),
            lead,
        );
        let table_area = Rect {
            x: first.x + lead.width,
            width: first.width.saturating_sub(lead.width),
            ..first
        };
        frame.render_widget(
            Table::new(vec![cells_of(at, row)], constraints.clone()).column_spacing(2),
            table_area,
        );
        if let (Some(detail), true) = (&row.detail, row_area.height >= 2) {
            let second = Rect {
                y: row_area.y + 1,
                height: 1,
                ..row_area
            };
            let indent = usize::from(2 + number_width);
            let text = clip(
                &detail.text,
                usize::from(second.width).saturating_sub(indent),
                ascii,
            );
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(" ".repeat(indent), base),
                    Span::styled(text, base.patch(dim)),
                ])),
                second,
            );
        }
    }
    if footer_rows == 1
        && let Some(footer) = &spec.footer
    {
        let footer_area = Rect {
            y: inner.y + inner.height - 1,
            height: 1,
            ..inner
        };
        frame.render_widget(
            Paragraph::new(Line::styled(
                clip(footer, usize::from(inner.width), ascii),
                dim,
            )),
            footer_area,
        );
    }
    if overflow {
        let mut bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .style(look.accent(Style::new(), look.palette().border));
        if ascii {
            bar = bar.track_symbol(Some("|")).thumb_symbol("#");
        } else {
            bar = bar.track_symbol(Some("\u{2502}")).thumb_symbol("\u{2588}");
        }
        let mut state = ScrollbarState::new(total.saturating_sub(shown)).position(top);
        let track = if spec.bordered {
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            })
        } else {
            area
        };
        frame.render_stateful_widget(bar, track, &mut state);
    }
    TableView { top, shown }
}

fn draw_header(
    frame: &mut Frame<'_>,
    inner: Rect,
    columns: &[&Column<'_>],
    widths: &[u16],
    numbered: bool,
    head_style: Style,
    ascii: bool,
) {
    let area = Rect { height: 1, ..inner };
    frame.render_widget(Block::default().style(head_style), area);
    let mut cells: Vec<ratatui::widgets::Cell<'static>> = Vec::new();
    let mut constraints: Vec<Constraint> = Vec::new();
    let lead = 2 + u16::from(numbered) * 3;
    if numbered {
        frame.render_widget(
            Paragraph::new(Span::raw("#")),
            Rect {
                x: area.x + 2.min(area.width),
                width: 1.min(area.width.saturating_sub(2)),
                ..area
            },
        );
    }
    for (column, width) in columns.iter().zip(widths) {
        let text = clip(column.title.as_str(), usize::from(*width), ascii);
        let line = Line::from(Span::raw(text));
        cells.push(ratatui::widgets::Cell::from(if column.align_right {
            line.alignment(Alignment::Right)
        } else {
            line
        }));
        constraints.push(Constraint::Length(*width));
    }
    let table_area = Rect {
        x: area.x + lead.min(area.width),
        width: area.width.saturating_sub(lead),
        ..area
    };
    frame.render_widget(
        Table::new(vec![Row::new(cells)], constraints)
            .column_spacing(2)
            .style(head_style),
        table_area,
    );
}

/// The widths of the shown columns inside `available` columns (spacing of one between them).
pub(crate) fn column_widths(columns: &[&Column<'_>], available: u16) -> Vec<u16> {
    if columns.is_empty() {
        return Vec::new();
    }
    let constraints: Vec<Constraint> = columns
        .iter()
        .map(|column| match column.width {
            Width::Fixed(n) => Constraint::Length(n),
            Width::Min(n) => Constraint::Min(n),
            Width::Fill(weight) => Constraint::Fill(weight.max(1)),
        })
        .collect();
    let spacing = u16::try_from(columns.len().saturating_sub(1) * 2).unwrap_or(0);
    let area = Rect::new(0, 0, available.saturating_add(spacing), 1);
    Layout::horizontal(constraints)
        .spacing(2)
        .split(area)
        .iter()
        .map(|rect| rect.width)
        .collect()
}
