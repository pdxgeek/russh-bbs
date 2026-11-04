//! The pane layouts: **dashboard**, **wide**, **focus** and **split** (UI-2).
//!
//! Each is a real arrangement of panes over the same menu page, drawn into the body the frame
//! hands out. The geometry is a pure function of the body rectangle ([`plan`]), so it is tested
//! without drawing: panes never overlap and never leave the body, and the body never contains the
//! status bar, so no pane can reach the marker cell.
//!
//! **Slots.** A layout places named slots ([`Slot`]) that UI-10's widgets will fill. Today a slot
//! shows what the server really has: the board's identity and the clock. Every other slot says in
//! plain words that there is nothing to show yet; **no pane shows made-up numbers.**
//!
//! **Chrome only.** Pane titles, the placeholder and the identity text come from the string
//! table or the board's checked text. Nothing here draws member content except the sanitized
//! signed-in line the caller already built, in the wide layout's centre pane.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::terminal_settings::LayoutId;

use super::classic::{self, MenuScreen};
use super::keys;
use super::theme::Element;
use crate::settings::Look;

/// A named place a widget will fill (UI-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Slot {
    /// The board's name, tagline, location and age.
    Identity,
    /// The time.
    Clock,
    /// Who is online.
    WhoOnline,
    /// The last callers.
    LastCallers,
    /// New posts since the last call.
    NewPosts,
    /// Unread mail.
    UnreadMail,
    /// A live chat room.
    Chat,
    /// Board activity.
    Activity,
}

impl Slot {
    const fn title_key(self) -> &'static str {
        keys::SLOT_TITLES[self as usize]
    }
}

/// Where a layout puts things inside the body.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Plan {
    /// The menu.
    pub(crate) nav: Rect,
    /// The wide layout's centre pane.
    pub(crate) content: Option<Rect>,
    /// The widget slots, in reading order.
    pub(crate) slots: Vec<(Slot, Rect)>,
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
    let rows: &[Constraint] = if grid.height >= 20 {
        &[
            Constraint::Length(6),
            Constraint::Fill(1),
            Constraint::Fill(1),
        ]
    } else if grid.height >= 12 {
        &[Constraint::Length(5), Constraint::Fill(1)]
    } else {
        &[Constraint::Fill(1)]
    };
    let order = [
        [Slot::Identity, Slot::Clock],
        [Slot::WhoOnline, Slot::LastCallers],
        [Slot::NewPosts, Slot::UnreadMail],
    ];
    let mut slots = Vec::new();
    for (at, row) in Layout::vertical(rows.to_vec())
        .split(grid)
        .iter()
        .enumerate()
    {
        let cols = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).split(*row);
        for (slot, col) in order[at].into_iter().zip(cols.iter()) {
            slots.push((slot, *col));
        }
    }
    Plan {
        nav,
        content: None,
        slots,
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
    let wanted = [
        Slot::Identity,
        Slot::Clock,
        Slot::WhoOnline,
        Slot::NewPosts,
        Slot::Activity,
    ];
    // Identity and clock have fixed heights; the rest share what is left, at least six rows each.
    let fixed = 6 + 3;
    let flexible = usize::from(rail.height.saturating_sub(fixed) / 6).min(3);
    let count = if rail.height >= fixed + 6 {
        2 + flexible
    } else {
        1
    };
    let mut constraints = Vec::new();
    for slot in wanted.iter().take(count) {
        constraints.push(match slot {
            Slot::Identity => Constraint::Length(6),
            Slot::Clock => Constraint::Length(3),
            _ => Constraint::Fill(1),
        });
    }
    let cells = Layout::vertical(constraints).split(rail);
    let slots = wanted.iter().copied().zip(cells.iter().copied()).collect();
    Plan {
        nav,
        content: Some(content),
        slots,
        divider: None,
    }
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
        slots: vec![(Slot::Chat, side)],
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
            for (slot, area) in &plan.slots {
                draw_slot(frame, *area, *slot, screen);
            }
        }
        LayoutId::Classic | LayoutId::Lightbar | LayoutId::Compact => {}
    }
}

/// One widget slot as a titled panel.
fn draw_slot(frame: &mut Frame<'_>, area: Rect, slot: Slot, screen: &MenuScreen<'_>) {
    let look = screen.look;
    let strings = screen.strings;
    if area.width < 4 || area.height < 3 {
        return;
    }
    let title = strings.text(slot.title_key());
    let inner = look
        .panel(frame, area, Some(title), Element::Pane, false)
        .inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
    let lines: Vec<Line<'static>> = match slot {
        Slot::Identity => identity_lines(screen, usize::from(inner.width)),
        Slot::Clock => vec![Line::styled(
            screen.clock.unwrap_or("--:--").to_owned(),
            look.accent(
                Style::new().add_modifier(Modifier::BOLD),
                look.palette().accent,
            ),
        )],
        Slot::WhoOnline
        | Slot::LastCallers
        | Slot::NewPosts
        | Slot::UnreadMail
        | Slot::Chat
        | Slot::Activity => vec![Line::styled(
            strings.text(keys::SLOT_EMPTY).to_owned(),
            look.muted(),
        )],
    };
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// The board's name, tagline, location and age: the sysop's own checked words.
fn identity_lines(screen: &MenuScreen<'_>, width: usize) -> Vec<Line<'static>> {
    let look = screen.look;
    let board = screen.board;
    let mut lines = vec![Line::styled(
        screen.strings.text(keys::TITLE).to_owned(),
        look.accent(
            Style::new().add_modifier(Modifier::BOLD),
            look.palette().accent,
        ),
    )];
    if !board.tagline().is_empty() {
        lines.push(Line::raw(board.tagline().as_str().to_owned()));
    }
    // Location and age share a line when they fit, so the panel stays short.
    let dot = if screen.tier == Tier::Ascii {
        "-"
    } else {
        "\u{b7}"
    };
    let place = match (board.location().is_empty(), board.since().is_empty()) {
        (false, false) => {
            let both = format!(
                "{} {dot} since {}",
                board.location().as_str(),
                board.since().as_str()
            );
            Some(if both.chars().count() <= width {
                both
            } else {
                board.location().as_str().to_owned()
            })
        }
        (false, true) => Some(board.location().as_str().to_owned()),
        (true, false) => Some(format!("since {}", board.since().as_str())),
        (true, true) => None,
    };
    if let Some(place) = place {
        lines.push(Line::styled(place, look.muted()));
    }
    lines
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
    let mut lines: Vec<Line<'static>> = Vec::new();
    let heading = look.accent(
        Style::new().add_modifier(Modifier::BOLD),
        look.palette().heading,
    );
    for slot in [Slot::Identity, Slot::Clock, Slot::WhoOnline, Slot::NewPosts] {
        lines.push(Line::styled(
            strings.text(slot.title_key()).to_owned(),
            heading,
        ));
        lines.extend(match slot {
            Slot::Identity => identity_lines(screen, usize::from(inner.width))
                .into_iter()
                .skip(1)
                .collect(),
            Slot::Clock => vec![Line::raw(screen.clock.unwrap_or("--:--").to_owned())],
            _ => vec![Line::styled(
                strings.text(keys::SLOT_EMPTY).to_owned(),
                look.muted(),
            )],
        });
    }
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        inner,
    );
}

#[cfg(test)]
mod tests;
