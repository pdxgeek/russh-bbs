//! Drawing sysop pages and the Menus and pages screen (UI-6).
//!
//! A page is drawn in the frame's **body** only: the bars, and with them the status row and the
//! marker cell, belong to [`FrameSpec`] and are drawn first and never touched again. Page art
//! goes through the same drawing as board art (cells with a colour and nothing else, clipped to
//! the frame without its last row). Page text is sanitized when it is loaded and is drawn here
//! with plain palette styles (heading and quote colours, bold for a heading): never reverse
//! video, so nothing on a page can look like the reserved marker style.

use ratatui::Frame;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::menu::{MenuToolsScreen, MenuToolsView, OutlineKind};

use super::{Block, MenuSetup, Page};
use crate::settings::Look;
use crate::ui::art::draw::{Filling, draw as draw_art};
use crate::ui::frame::{ChromeText, FrameSpec, Hint};
use crate::ui::keys;
use crate::ui::pager::wrap;
use crate::ui::strings::StringTable;
use crate::ui::theme::Element;

/// Columns a page's text is set in, as on a classic bulletin board.
const COLUMNS: u16 = 72;

/// What both screens are drawn with.
pub(crate) struct DrawCtx<'a> {
    pub(crate) strings: &'a StringTable,
    pub(crate) look: Look,
    pub(crate) tier: Tier,
    pub(crate) markers: &'a [SecurityMarker],
    pub(crate) clock: Option<&'a str>,
}

const fn arrows(tier: Tier) -> &'static str {
    match tier {
        Tier::Ascii => "Up/Dn",
        _ => "\u{2191}\u{2193}",
    }
}

/// The page's text as rows of `width` columns, styled from the palette.
pub(crate) fn text_lines(page: &Page, width: usize, look: Look) -> Vec<Line<'static>> {
    let palette = look.palette();
    let ascii = look.ascii();
    let width = width.max(8);
    let heading = look.fg(palette.heading).add_modifier(Modifier::BOLD);
    let quote = look.fg(palette.info);
    let accent = look.fg(palette.key);
    let mut out: Vec<Line<'static>> = Vec::new();
    for block in page.blocks() {
        match block {
            Block::Blank => out.push(Line::raw("")),
            Block::Rule => {
                let glyph = if ascii { "-" } else { "\u{2500}" };
                out.push(Line::styled(glyph.repeat(width), look.muted()));
            }
            Block::Heading(text) => {
                for row in wrap(std::slice::from_ref(text), width) {
                    out.push(Line::styled(row, heading));
                }
                let glyph = if ascii { "=" } else { "\u{2550}" };
                let under = text.chars().count().min(width);
                out.push(Line::styled(glyph.repeat(under), accent));
            }
            Block::Text(text) => {
                for row in wrap(std::slice::from_ref(text), width) {
                    out.push(Line::styled(row, look.text()));
                }
            }
            Block::Bullet(text) => {
                let (dot, indent) = if ascii {
                    ("* ", "  ")
                } else {
                    ("\u{2022} ", "  ")
                };
                for (at, row) in wrap(std::slice::from_ref(text), width.saturating_sub(2))
                    .into_iter()
                    .enumerate()
                {
                    out.push(Line::from(vec![
                        Span::styled(if at == 0 { dot } else { indent }, accent),
                        Span::styled(row, look.text()),
                    ]));
                }
            }
            Block::Quote(text) => {
                let bar = if ascii { "| " } else { "\u{2502} " };
                for row in wrap(std::slice::from_ref(text), width.saturating_sub(2)) {
                    out.push(Line::from(vec![
                        Span::styled(bar, accent),
                        Span::styled(row, quote),
                    ]));
                }
            }
        }
    }
    out
}

/// Draw `page` as a member sees it, scrolled `scroll` rows. Returns how far it can scroll, so
/// the flow can stop at the end.
pub(crate) fn render_page(
    frame: &mut Frame<'_>,
    page: &Page,
    scroll: usize,
    ctx: &DrawCtx<'_>,
) -> usize {
    let strings = ctx.strings;
    let key = ChromeText::fixed;
    let word = |k: &'static str| ChromeText::table(strings, k);
    let hints = [
        Hint::new(key(arrows(ctx.tier)), word(keys::MENUS_HINT_SCROLL)),
        Hint::new(key("b"), word(keys::MENUS_HINT_BACK)),
    ];
    let body = FrameSpec::new(strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(ChromeText::table(strings, page.title()))
        .clock(ctx.clock)
        .hints(&hints)
        .draw(frame);
    draw_body(frame, body, page, scroll, ctx)
}

/// The page inside `body`: art on top when it fits, then the text in a panel.
fn draw_body(
    frame: &mut Frame<'_>,
    body: Rect,
    page: &Page,
    scroll: usize,
    ctx: &DrawCtx<'_>,
) -> usize {
    if body.height < 5 || body.width < 20 {
        return 0;
    }
    let look = ctx.look;
    let mut top = body.y;
    let mut room = body.height;
    if let Some(piece) = page.art()
        && (ctx.tier != Tier::Ascii || piece.ascii_safe())
        && piece.fits(body.width, body.height / 2)
    {
        let rows = u16::try_from(piece.grid().height()).unwrap_or(0);
        let slot = Rect {
            height: rows,
            ..body
        };
        draw_art(frame, slot, &Filling::Piece(piece), look, ctx.tier);
        top += rows + 1;
        room = room.saturating_sub(rows + 1);
    }
    if room < 4 {
        return 0;
    }
    let width = body.width.min(COLUMNS + 4);
    let outer = Rect {
        x: body.x + (body.width - width) / 2,
        y: top,
        width,
        height: room,
    };
    let inner = look.panel(frame, outer, Some(page.title()), Element::Frame, false);
    if inner.width < 10 || inner.height < 2 {
        return 0;
    }
    let text_area = Rect {
        x: inner.x + 1,
        width: inner.width.saturating_sub(3),
        ..inner
    };
    let lines = text_lines(
        page,
        usize::from(text_area.width).min(usize::from(COLUMNS)),
        look,
    );
    let view = usize::from(text_area.height);
    let max = lines.len().saturating_sub(view);
    let at = scroll.min(max);
    let shown: Vec<Line<'static>> = lines.into_iter().skip(at).take(view).collect();
    let total = max + view;
    frame.render_widget(Paragraph::new(shown), text_area);
    if max > 0 {
        let mut bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .style(look.accent(Style::new(), look.palette().border));
        bar = if look.ascii() {
            bar.track_symbol(Some("|")).thumb_symbol("#")
        } else {
            bar.track_symbol(Some("\u{2502}")).thumb_symbol("\u{2588}")
        };
        let mut state = ScrollbarState::new(max).position(at);
        frame.render_stateful_widget(
            bar,
            inner.inner(Margin {
                vertical: 0,
                horizontal: 0,
            }),
            &mut state,
        );
        let position = ctx.strings.filled(
            keys::MENUS_POSITION,
            &[
                ("from", &(at + 1).to_string()),
                ("to", &(at + view).min(total).to_string()),
                ("total", &total.to_string()),
            ],
        );
        let row = Rect {
            y: outer.y + outer.height - 1,
            height: 1,
            x: outer.x + 2,
            width: outer.width.saturating_sub(4),
        };
        frame.render_widget(Paragraph::new(Line::styled(position, look.muted())), row);
    }
    max
}

/// Draw page `id` of `setup` as a member sees it. A page the board no longer has (a reload
/// removed it while a session had it open) says so.
pub(crate) fn render_bulletin(
    frame: &mut Frame<'_>,
    setup: &MenuSetup,
    id: &str,
    scroll: usize,
    ctx: &DrawCtx<'_>,
) -> usize {
    if let Some(page) = setup.page(id) {
        return render_page(frame, page, scroll, ctx);
    }
    let strings = ctx.strings;
    let hints = [Hint::new(
        ChromeText::fixed("b"),
        ChromeText::table(strings, keys::MENUS_HINT_BACK),
    )];
    let body = FrameSpec::new(strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(ChromeText::table(strings, keys::MENUS_TITLE))
        .clock(ctx.clock)
        .hints(&hints)
        .draw(frame);
    frame.render_widget(Clear, body);
    frame.render_widget(
        Paragraph::new(Line::styled(
            strings.text(keys::MENUS_PAGE_GONE).to_owned(),
            ctx.look.fg(ctx.look.palette().warning),
        )),
        body,
    );
    0
}

/// Draw the sysop's Menus and pages screen. Returns how far the open page, or else the role's
/// menu, can scroll.
#[allow(clippy::too_many_lines)] // one screen top to bottom: status, then the menu or the pages
pub(crate) fn render_tools(
    frame: &mut Frame<'_>,
    screen: &MenuToolsScreen,
    setup: &MenuSetup,
    ctx: &DrawCtx<'_>,
) -> usize {
    let strings = ctx.strings;
    if let Some(id) = screen.viewing() {
        return match setup.page(id) {
            Some(page) => render_page(frame, page, screen.scroll(), ctx),
            None => 0,
        };
    }
    let key = ChromeText::fixed;
    let word = |k: &'static str| ChromeText::table(strings, k);
    let mut hints = vec![
        Hint::new(key("r"), word(keys::MENUS_HINT_ROLE)),
        Hint::new(key("t"), word(keys::MENUS_HINT_TOGGLE)),
        Hint::new(key(arrows(ctx.tier)), word(keys::MENUS_HINT_PAGE)),
    ];
    if screen.view() == MenuToolsView::Pages {
        hints.push(Hint::new(key("Enter"), word(keys::MENUS_HINT_VIEW)));
    }
    hints.push(Hint::new(key("l"), word(keys::MENUS_HINT_RELOAD)));
    hints.push(Hint::new(key("b"), word(keys::MENUS_HINT_BACK)));
    let body = FrameSpec::new(strings, ctx.look, ctx.tier)
        .markers(ctx.markers)
        .place(word(keys::MENUS_TITLE))
        .clock(ctx.clock)
        .hints(&hints)
        .draw(frame);
    let look = ctx.look;
    let palette = look.palette();
    let heading = look.fg(palette.heading).add_modifier(Modifier::BOLD);
    let clean = |text: &str| {
        russh_bbs_core::sanitize::SingleLine::new(160)
            .sanitize(text)
            .into_string()
    };
    // The status lines stay on top; the list under them scrolls.
    let mut top: Vec<Line<'static>> = Vec::new();
    let mut list: Vec<Line<'static>> = Vec::new();
    let mut cursor_line = 0;
    let mut scroll = 0;
    match screen.listing() {
        None => top.push(Line::styled(
            strings.text(keys::MENUS_LOADING).to_owned(),
            look.muted(),
        )),
        Some(listing) if listing.unavailable => top.push(Line::styled(
            strings.text(keys::MENUS_UNAVAILABLE).to_owned(),
            look.fg(palette.warning),
        )),
        Some(listing) => {
            let state = if listing.custom {
                keys::MENUS_USING_FILE
            } else {
                keys::MENUS_USING_DEFAULT
            };
            top.push(Line::styled(
                strings.text(state).to_owned(),
                look.fg(palette.key),
            ));
            if let Some(reason) = &listing.problem {
                top.push(Line::styled(
                    strings.filled(keys::MENUS_REFUSED, &[("reason", &clean(reason))]),
                    look.fg(palette.warning),
                ));
            }
            match screen.view() {
                MenuToolsView::Menu => {
                    scroll = screen.outline_scroll();
                    if let Some(role) = listing.roles.get(screen.role()) {
                        top.push(Line::styled(
                            strings.filled(keys::MENUS_MENU_FOR, &[("role", &clean(&role.name))]),
                            heading,
                        ));
                        if role.lines.is_empty() {
                            list.push(Line::styled(
                                strings.text(keys::MENUS_EMPTY).to_owned(),
                                look.muted(),
                            ));
                        }
                        for line in &role.lines {
                            let indent = "  ".repeat(line.depth + 1);
                            let tag = match line.kind {
                                OutlineKind::Menu => " >",
                                OutlineKind::Page => " *",
                                OutlineKind::Action | OutlineKind::Placeholder => "",
                            };
                            list.push(Line::from(vec![
                                Span::raw(indent),
                                Span::styled(format!("[{}] ", line.hotkey), look.fg(palette.key)),
                                Span::styled(
                                    format!("{}{tag}", clean(strings.text(line.label))),
                                    look.text(),
                                ),
                            ]));
                        }
                    }
                }
                MenuToolsView::Pages => {
                    top.push(Line::styled(
                        strings.text(keys::MENUS_PAGES).to_owned(),
                        heading,
                    ));
                    if listing.pages.is_empty() {
                        list.push(Line::styled(
                            strings.text(keys::MENUS_NO_PAGES).to_owned(),
                            look.muted(),
                        ));
                    }
                    for (at, page) in listing.pages.iter().enumerate() {
                        let current = at == screen.row();
                        if current {
                            cursor_line = list.len();
                        }
                        let style = if current {
                            look.fg(palette.selected)
                                .patch(look.bg(palette.select_bg))
                                .add_modifier(Modifier::BOLD)
                        } else {
                            look.text()
                        };
                        list.push(Line::styled(
                            format!(
                                "{}{:<24} {}",
                                if current { "> " } else { "  " },
                                page.id,
                                clean(&page.title)
                            ),
                            style,
                        ));
                    }
                }
            }
        }
    }
    frame.render_widget(Clear, body);
    let head = u16::try_from(top.len())
        .unwrap_or(0)
        .min(body.height.saturating_sub(2));
    let head_area = Rect {
        height: head,
        ..body
    };
    frame.render_widget(Paragraph::new(top), head_area);
    // One row at the bottom for the last outcome; the list gets the rows between.
    let list_rows = usize::from(body.height.saturating_sub(head + 1));
    let max = list.len().saturating_sub(list_rows);
    let first = if screen.view() == MenuToolsView::Pages {
        (cursor_line + 1).saturating_sub(list_rows).min(max)
    } else {
        scroll.min(max)
    };
    let shown: Vec<Line<'static>> = list.into_iter().skip(first).take(list_rows).collect();
    let list_area = Rect {
        y: body.y + head,
        height: u16::try_from(list_rows).unwrap_or(0),
        ..body
    };
    frame.render_widget(Paragraph::new(shown), list_area);
    if body.height >= 2 {
        let row = Rect {
            y: body.y + body.height - 1,
            height: 1,
            ..body
        };
        frame.render_widget(
            Paragraph::new(Line::styled(
                screen.message().unwrap_or("").to_owned(),
                look.fg(palette.info),
            )),
            row,
        );
    }
    if screen.view() == MenuToolsView::Menu {
        max
    } else {
        0
    }
}
