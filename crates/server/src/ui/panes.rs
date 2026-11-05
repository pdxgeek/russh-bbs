//! The pane layouts: **dashboard**, **wide**, **focus** and **split** (UI-2).
//!
//! Each is a real arrangement of panes over the same menu page, drawn into the body the frame
//! hands out. The geometry is a pure function of the body rectangle ([`plan`]), so it is tested
//! without drawing: panes never overlap and never leave the body, and the body never contains the
//! status bar, so no pane can reach the marker cell.
//!
//! **Cells.** A layout lays out generic widget cells (`Plan::cells`), in reading order. The
//! member's widgets for the layout (UI-10, [`super::widgets`]) are assigned to them in order: a
//! widget takes the first cell it fits, and a widget with nothing to show takes none. A cell no
//! widget fills is left blank, so **no pane shows made-up numbers.**
//!
//! **Chrome only.** Pane titles, the placeholder and the identity text come from the string
//! table or the board's checked text. Nothing here draws member content except the sanitized
//! signed-in line the caller already built, in the wide layout's centre pane.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use russh_bbs_core::terminal_settings::LayoutId;

use super::classic::{self, MenuScreen};
use super::keys;
use super::theme::Element;
use super::widgets;
use crate::settings::Look;

/// Where a layout puts things inside the body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Plan {
    /// The menu.
    pub(crate) nav: Rect,
    /// The wide layout's centre pane.
    pub(crate) content: Option<Rect>,
    /// The widget cells, in reading order.
    pub(crate) cells: Vec<Rect>,
    /// The split layout's divider column.
    pub(crate) divider: Option<Rect>,
}

/// The arrangement `layout` makes of `body`. Only the pane layouts have a plan; the others
/// return the body as the menu's only pane.
#[must_use]
pub(crate) fn plan(layout: LayoutId, body: Rect) -> Plan {
    match layout {
        LayoutId::Dashboard => dashboard(body),
        LayoutId::Wide => wide(body),
        LayoutId::Split => split(body),
        LayoutId::Focus | LayoutId::Classic | LayoutId::Lightbar | LayoutId::Compact => Plan {
            nav: body,
            ..Plan::default()
        },
    }
}

fn dashboard(body: Rect) -> Plan {
    let nav_width = 34.min(body.width / 2);
    let [nav, grid] =
        Layout::horizontal([Constraint::Length(nav_width), Constraint::Fill(1)]).areas(body);
    // More of the grid as the terminal grows: two columns of up to three rows at 80x24, three
    // columns of four rows at 132x43.
    let rows = match grid.height {
        34.. => 4,
        20.. => 3,
        12.. => 2,
        _ => 1,
    };
    let cols = if grid.width >= 90 { 3 } else { 2 };
    let mut cells = Vec::new();
    for row in Layout::vertical(vec![Constraint::Fill(1); rows]).split(grid).iter() {
        for col in Layout::horizontal(vec![Constraint::Fill(1); cols]).split(*row).iter() {
            cells.push(*col);
        }
    }
    Plan {
        nav,
        content: None,
        cells,
        divider: None,
    }
}

fn wide(body: Rect) -> Plan {
    let [nav, content, rail] = Layout::horizontal([
        Constraint::Length(34),
        Constraint::Fill(1),
        Constraint::Length(36.min(body.width / 3)),
    ])
    .areas(body);
    Plan {
        nav,
        content: Some(content),
        cells: stack(rail),
        divider: None,
    }
}

/// A column of cells, each at least seven rows tall (a panel with five rows inside), at most
/// five of them.
fn stack(column: Rect) -> Vec<Rect> {
    let count = usize::from((column.height / 7).clamp(1, 5));
    Layout::vertical(vec![Constraint::Fill(1); count])
        .split(column)
        .to_vec()
}

fn split(body: Rect) -> Plan {
    let nav_width = 34.min(body.width / 2);
    let [nav, divider, side] = Layout::horizontal([
        Constraint::Length(nav_width),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(body);
    Plan {
        nav,
        content: None,
        cells: stack(side),
        divider: Some(divider),
    }
}

/// Draw a pane layout's menu screen.
pub(crate) fn render(frame: &mut Frame<'_>, layout: LayoutId, screen: &MenuScreen<'_>) {
    let hints = classic::menu_hints(screen.strings, true);
    let mut hints = hints;
    if layout == LayoutId::Focus {
        hints.insert(
            2,
            super::frame::Hint::new(
                super::frame::ChromeText::fixed("Tab"),
                super::frame::ChromeText::table(screen.strings, keys::HINT_WIDGETS),
            ),
        );
    }
    let body = classic::frame_spec(screen, &hints).draw(frame);
    if screen.page.help {
        classic::draw_help(frame, body, screen, Some(keys::HELP_LIGHTBAR));
        return;
    }
    let cursor = Some(screen.page.cursor);
    let plan = plan(layout, body);
    match layout {
        LayoutId::Focus => {
            draw_focus(frame, body, screen);
            if screen.popover {
                draw_popover(frame, body, screen);
            }
        }
        LayoutId::Dashboard | LayoutId::Wide | LayoutId::Split => {
            classic::draw_menu_in(
                frame,
                plan.nav,
                screen,
                cursor,
                Element::Pane,
                plan.content.is_none(),
            );
            if let Some(content) = plan.content {
                draw_content(frame, content, screen);
            }
            if let Some(divider) = plan.divider {
                draw_divider(frame, divider, screen.look);
            }
            let cx = screen.widget_cx();
            let placed = widgets::assign(&plan.cells, screen.widgets.shown, &cx);
            widgets::draw_cells(frame, &placed, &cx);
        }
        LayoutId::Classic | LayoutId::Lightbar | LayoutId::Compact => {}
    }
}

/// The wide layout's centre pane: where the member is, the sanitized signed-in lines and the
/// row under the cursor.
fn draw_content(frame: &mut Frame<'_>, area: Rect, screen: &MenuScreen<'_>) {
    let look = screen.look;
    let strings = screen.strings;
    let inner = look
        .panel(
            frame,
            area,
            Some(strings.text(keys::TITLE)),
            Element::Pane,
            false,
        )
        .inner(Margin {
            horizontal: 2,
            vertical: 1,
        });
    let mut lines: Vec<Line<'static>> = screen
        .body
        .iter()
        .map(|text| Line::raw(text.clone()))
        .collect();
    if !lines.is_empty() {
        lines.push(Line::raw(""));
    }
    if let Some(item) = screen.page.items.get(screen.page.cursor) {
        let pointer = strings.text(keys::POINTER);
        lines.push(Line::from(vec![
            Span::styled(
                format!("{pointer} "),
                look.accent(
                    Style::new().add_modifier(Modifier::BOLD),
                    look.palette().accent,
                ),
            ),
            Span::styled(
                format!("[{}] {}", item.hotkey, strings.text(item.label)),
                look.accent(
                    Style::new().add_modifier(Modifier::BOLD),
                    look.palette().heading,
                ),
            ),
        ]));
    }
    if let Some(note) = classic::fallback_note(screen) {
        lines.push(Line::raw(""));
        lines.push(Line::styled(note, look.muted()));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The split layout's divider: a rule with a grip in the middle. (Dragging it is UI-9.)
fn draw_divider(frame: &mut Frame<'_>, area: Rect, look: Look) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let (rule, grip) = if look.ascii() {
        ("|", "#")
    } else {
        ("\u{2502}", "\u{2503}")
    };
    let style = look.accent(Style::new(), look.palette().border);
    let lines: Vec<Line<'static>> = (0..area.height)
        .map(|row| {
            let mark = if row == area.height / 2 { grip } else { rule };
            Line::styled(mark.to_owned(), style)
        })
        .collect();
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).style(look.text()), area);
}

/// The focus layout: the menu as a roomy centred column with no box, a heading and a rule.
fn draw_focus(frame: &mut Frame<'_>, body: Rect, screen: &MenuScreen<'_>) {
    let look = screen.look;
    let strings = screen.strings;
    let width = body.width.min(60);
    let column = Rect {
        x: body.x + (body.width - width) / 2,
        width,
        ..body
    };
    frame.render_widget(Clear, column);
    frame.render_widget(
        ratatui::widgets::Block::default().style(look.text()),
        column,
    );
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line<'static>> = vec![Line::styled(
        strings.text(screen.page.title).to_owned(),
        look.accent(bold, look.palette().heading),
    )];
    let rule = if look.ascii() { "-" } else { "\u{2500}" };
    lines.push(Line::styled(
        rule.repeat(usize::from(width.min(40))),
        look.accent(Style::new(), look.palette().border),
    ));
    for text in screen.body {
        lines.push(Line::styled(text.clone(), look.muted()));
    }
    if let Some(note) = classic::fallback_note(screen) {
        lines.push(Line::styled(note, look.muted()));
    }
    let used = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let items = u16::try_from(screen.page.items.len()).unwrap_or(u16::MAX);
    // Roomy when it fits: a blank row between items.
    let airy = used + 1 + items * 2 < body.height;
    lines.push(Line::raw(""));
    let pointer = strings.text(keys::POINTER);
    for (at, item) in screen.page.items.iter().enumerate() {
        let selected = at == screen.page.cursor;
        let mut row = classic::item_line(
            strings,
            item,
            look.accent(bold, look.palette().key),
            look.muted(),
        );
        let lead = if selected {
            format!("{pointer} ")
        } else {
            "  ".to_owned()
        };
        row.spans.insert(0, Span::raw(lead));
        if selected {
            row = row.style(
                look.accent(
                    Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    look.palette().selected,
                )
                .patch(look.bg(look.surfaces().select_bg)),
            );
        }
        lines.push(row);
        if airy {
            lines.push(Line::raw(""));
        }
    }
    if let Some(notice) = screen.notice {
        lines.push(classic::notice_line(
            strings,
            notice.kind,
            notice.text,
            look,
            bold,
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        column.inner(Margin {
            horizontal: 1,
            vertical: 0,
        }),
    );
}

/// The focus layout's pop-over (Tab): the widget slots in a box inside the body.
fn draw_popover(frame: &mut Frame<'_>, body: Rect, screen: &MenuScreen<'_>) {
    let look = screen.look;
    let strings = screen.strings;
    let width = body.width.saturating_sub(4).min(56);
    let height = body.height.saturating_sub(2).min(14);
    if width < 20 || height < 6 {
        return;
    }
    let area = Rect {
        x: body.x + (body.width - width) / 2,
        y: body.y + (body.height - height) / 2,
        width,
        height,
    };
    let inner = look
        .panel(
            frame,
            area,
            Some(strings.text(keys::SLOT_POPOVER)),
            Element::Popup,
            true,
        )
        .inner(Margin {
            horizontal: 2,
            vertical: 1,
        });
    let cx = screen.widget_cx();
    let lines = widgets::popover_lines(screen.widgets.shown, &cx, inner.width, inner.height);
    widgets::draw_lines(frame, inner, lines, look);
}

#[cfg(test)]
mod tests;
