//! The application frame: a **header bar** on top, a **status bar** at the bottom, and the
//! body between them (UI-1, D109).
//!
//! ```text
//! russh-bbs  |  Forums                                     UTC 14:05    <- header bar
//!   ...body: whatever the screen draws, handed back as a Rect...
//! [marker cell]   ? help   Esc back   Ctrl-C log off                      <- status bar
//! ```
//!
//! **Using it.** A screen builds a [`FrameSpec`] (strings, look, tier, the markers that apply,
//! the area's name, hints), calls [`FrameSpec::draw`], and draws its content into the returned
//! body rectangle. The same spec serves every layout; a new layout or screen never draws its
//! own bars.
//!
//! **What the bars may say.** Only chrome: string-table words, fixed key names, and a clock. The
//! API takes [`ChromeText`], which can be built only from the string table or a `'static` str, so
//! a member's text cannot be passed to a bar by mistake. Content goes in the body.
//!
//! **Security markers (D39, D93, D109).** The status bar's left cell is the marker cell. It is
//! drawn by [`chrome::status_cell`] with the reserved style (bold + reverse) and **no colour at
//! all**: the bar's background starts after it, so no theme can tint it. It is part of the
//! rectangle this module owns and never of the body, so nothing the body, a table or a popup
//! draws can reach it (ratatui clips every widget to its rectangle; popups are clamped to the
//! body). When the bar is too small to show a marker it says so in words. Nothing else in the
//! frame uses reverse video, so the style stays unique to the markers.
//!
//! **Sizes.** The status bar needs two rows (one for it, one for the body). The header also needs
//! 12 rows and 40 columns and is dropped for the compact layout and below that. On a terminal too
//! small for either the body simply gets the space.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use russh_bbs_core::icons::{SecurityMarker, Tier};

use super::art::Slot;
use super::art::draw::ArtCtx;
use super::chrome;
use super::strings::StringTable;
use super::theme::skin::{blend, gradient_depth};
use crate::settings::Look;

/// Fewest rows for the header bar to appear.
pub(crate) const HEADER_MIN_ROWS: u16 = 12;
/// Fewest columns for the header bar to appear.
pub(crate) const HEADER_MIN_COLS: u16 = 40;
/// Fewest free header columns for the ticker to appear.
const TICKER_MIN: u16 = 12;

/// Text that is allowed in a bar: words from the string table, or fixed words in the code.
/// It cannot be made from an arbitrary `&str`, which keeps member text out of the bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChromeText<'a>(&'a str);

impl<'a> ChromeText<'a> {
    /// The string table's word for `key`.
    pub(crate) fn table(strings: &'a StringTable, key: &'a str) -> Self {
        Self(strings.text(key))
    }

    /// A fixed word in the code (a key name such as `Esc`).
    pub(crate) const fn fixed(text: &'static str) -> ChromeText<'static> {
        ChromeText(text)
    }

    pub(crate) fn as_str(self) -> &'a str {
        self.0
    }
}

/// One entry in the status bar: the key and what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hint<'a> {
    pub(crate) key: ChromeText<'a>,
    pub(crate) label: ChromeText<'a>,
}

impl<'a> Hint<'a> {
    pub(crate) const fn new(key: ChromeText<'a>, label: ChromeText<'a>) -> Self {
        Self { key, label }
    }
}

/// A request for art at the top of the body (UI-3): which slot, and how many rows the screen
/// needs for itself, so the art takes only what is left over.
#[derive(Clone, Copy)]
pub(crate) struct ArtRequest<'a> {
    pub(crate) ctx: &'a ArtCtx<'a>,
    pub(crate) slot: Slot,
    /// Rows of the body that stay for the screen's own content.
    pub(crate) keep: u16,
}

impl std::fmt::Debug for ArtRequest<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArtRequest")
            .field("slot", &self.slot)
            .field("keep", &self.keep)
            .finish_non_exhaustive()
    }
}

/// How a frame is split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Parts {
    pub(crate) header: Option<Rect>,
    pub(crate) body: Rect,
    pub(crate) status: Option<Rect>,
}

/// Everything the frame needs to draw its two bars.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameSpec<'a> {
    strings: &'a StringTable,
    look: Look,
    tier: Tier,
    markers: &'a [SecurityMarker],
    board: ChromeText<'a>,
    place: Option<ChromeText<'a>>,
    clock: Option<&'a str>,
    hints: &'a [Hint<'a>],
    header: bool,
    art: Option<ArtRequest<'a>>,
}

impl<'a> FrameSpec<'a> {
    /// A frame for the board called by the string table's title. The header is on unless the
    /// look is borderless (the compact layout).
    pub(crate) fn new(strings: &'a StringTable, look: Look, tier: Tier) -> Self {
        Self {
            strings,
            look,
            tier,
            markers: &[],
            board: ChromeText::table(strings, super::keys::TITLE),
            place: None,
            clock: None,
            hints: &[],
            header: !look.is_borderless(),
            art: None,
        }
    }

    /// Ask for art above the body (the screen's header or banner), in the rows beyond `keep`
    /// only; the returned body starts below it.
    #[must_use]
    pub(crate) const fn art(mut self, art: Option<ArtRequest<'a>>) -> Self {
        self.art = art;
        self
    }

    /// The security markers that apply to this screen (drawn in the status bar's left cell).
    #[must_use]
    pub(crate) const fn markers(mut self, markers: &'a [SecurityMarker]) -> Self {
        self.markers = markers;
        self
    }

    /// The name of the area the member is in, shown beside the board name.
    #[must_use]
    pub(crate) const fn place(mut self, place: ChromeText<'a>) -> Self {
        self.place = Some(place);
        self
    }

    /// The clock text for the header's right end, e.g. `UTC 14:05`. Numbers and fixed words
    /// only; it is redrawn with the screen, not by a timer (D49).
    #[must_use]
    pub(crate) const fn clock(mut self, clock: Option<&'a str>) -> Self {
        self.clock = clock;
        self
    }

    /// The keys for this screen, most important first. Hints that do not fit are dropped from
    /// the end, never clipped.
    #[must_use]
    pub(crate) const fn hints(mut self, hints: &'a [Hint<'a>]) -> Self {
        self.hints = hints;
        self
    }

    /// Turn the header on or off (a layout that wants none).
    #[must_use]
    pub(crate) const fn header(mut self, on: bool) -> Self {
        self.header = on;
        self
    }

    /// Split `area` into header, body and status bar.
    pub(crate) fn parts(&self, area: Rect) -> Parts {
        let status = (area.height >= 2).then(|| Rect {
            y: area.y + area.height - 1,
            height: 1,
            ..area
        });
        let after_status = if status.is_some() {
            area.height - 1
        } else {
            area.height
        };
        let header =
            (self.header && area.height >= HEADER_MIN_ROWS && area.width >= HEADER_MIN_COLS)
                .then_some(Rect { height: 1, ..area });
        let top = u16::from(header.is_some());
        let body = Rect {
            y: area.y + top,
            height: after_status - top,
            ..area
        };
        Parts {
            header,
            body,
            status,
        }
    }

    /// Draw the bars into `frame` and return the body rectangle for the screen's content.
    pub(crate) fn draw(&self, frame: &mut Frame<'_>) -> Rect {
        let parts = self.parts(frame.area());
        // The style's glass and texture first, over everything above the status bar: the status
        // bar clears its own row, so the marker cell never carries any of it.
        let above = Rect {
            height: parts
                .status
                .map_or(frame.area().height, |_| frame.area().height - 1),
            ..frame.area()
        };
        super::backdrop::paint(frame, above, self.look);
        if let Some(area) = parts.header {
            self.draw_header(frame, area);
        }
        if let Some(area) = parts.status {
            self.draw_status(frame, area);
        }
        self.draw_art(frame, &parts)
    }

    /// Draw the requested art at the top of the body and return the body below it. The art goes
    /// only in the body, never on the status bar's row where the marker cell is.
    fn draw_art(&self, frame: &mut Frame<'_>, parts: &Parts) -> Rect {
        let body = parts.body;
        let Some(request) = self.art else {
            return body;
        };
        let spare = body.height.saturating_sub(request.keep);
        let Some(filling) = request.ctx.fill(request.slot, self.tier, body.width, spare) else {
            return body;
        };
        let Ok(rows) = u16::try_from(filling.height()) else {
            return body;
        };
        // One blank row under the art when there is room for it.
        let used = if spare > rows { rows + 1 } else { rows };
        let slot = Rect {
            height: rows,
            ..body
        };
        super::art::draw::draw(frame, slot, &filling, self.look, self.tier);
        Rect {
            y: body.y + used,
            height: body.height - used,
            ..body
        }
    }

    fn bar(&self) -> Style {
        let surfaces = self.look.surfaces();
        self.look
            .bg(surfaces.bar_bg)
            .patch(self.look.fg(surfaces.bar_fg))
    }

    fn separator(&self) -> &'static str {
        if self.look.ascii() {
            " | "
        } else {
            " \u{2502} "
        }
    }

    fn draw_header(&self, frame: &mut Frame<'_>, area: Rect) {
        let bar = self.bar();
        frame.render_widget(Clear, area);
        frame.render_widget(Block::default().style(bar), area);
        let (before, after) = self.look.skin().deco.flank(self.look.ascii());
        let mut left = vec![Span::styled(" ".to_owned(), bar)];
        if !before.is_empty() {
            left.push(Span::styled(
                before.to_owned(),
                bar.patch(self.look.muted()),
            ));
        }
        left.extend(self.name_spans(bar));
        if !after.is_empty() {
            left.push(Span::styled(after.to_owned(), bar.patch(self.look.muted())));
        }
        if let Some(place) = self.place {
            left.push(Span::styled(self.separator().to_owned(), bar));
            left.push(Span::styled(place.as_str().to_owned(), bar));
        }
        let right = self
            .clock
            .map(|clock| Span::styled(format!("{clock} "), bar))
            .into_iter()
            .collect::<Vec<_>>();
        let right_width = u16::try_from(Line::from(right.clone()).width()).unwrap_or(0);
        // The clock goes first: if the area is narrow, the place name is what gives way.
        let right_width = right_width.min(area.width / 2);
        let left_width = u16::try_from(Line::from(left.clone()).width()).unwrap_or(area.width);
        let left_area = Rect {
            width: area.width - right_width,
            ..area
        };
        frame.render_widget(Paragraph::new(Line::from(left)), left_area);
        // The live ticker (UI-9) takes the room between the name and the clock, if there is
        // at least a short line's worth. It is chrome in the header only, never the status bar.
        if let Some(view) = super::ticker::current() {
            let room = area
                .width
                .saturating_sub(left_width)
                .saturating_sub(right_width)
                .saturating_sub(4);
            if room >= TICKER_MIN {
                let text = view.window(usize::from(room));
                frame.render_widget(
                    Paragraph::new(Line::styled(
                        text.as_str().to_owned(),
                        bar.patch(self.look.muted()),
                    )),
                    Rect {
                        x: area.x + left_width + 2,
                        width: room,
                        ..area
                    },
                );
            }
        }
        if right_width > 0 {
            let right_area = Rect {
                x: area.x + area.width - right_width,
                width: right_width,
                ..area
            };
            frame.render_widget(Paragraph::new(Line::from(right)), right_area);
        }
    }

    /// The board name in the accent, as a gradient from the first accent to the second where the
    /// style asks for one and the colour depth has the steps for it.
    fn name_spans(&self, bar: Style) -> Vec<Span<'static>> {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let palette = self.look.palette();
        let name = self.board.as_str();
        let count = name.chars().count();
        if self.look.skin().gradient && gradient_depth(self.look.depth()) && count > 1 {
            let last = u32::try_from(count - 1).unwrap_or(1);
            let spans: Vec<Span<'static>> = name
                .chars()
                .enumerate()
                .map(|(at, c)| {
                    let t = u8::try_from(u32::try_from(at).unwrap_or(0) * 255 / last)
                        .unwrap_or(u8::MAX);
                    let tint = blend(palette.accent, palette.accent2, t).unwrap_or(palette.accent);
                    Span::styled(c.to_string(), bar.patch(self.look.accent(bold, tint)))
                })
                .collect();
            return spans;
        }
        vec![Span::styled(
            name.to_owned(),
            bar.patch(self.look.accent(bold, palette.accent)),
        )]
    }

    fn draw_status(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Clear, area);
        // The marker cell first: the reserved style and no colour.
        let cell = chrome::status_cell(self.markers, self.tier, self.strings, area.width);
        let used = cell.as_ref().map_or(0, |(_, used)| *used);
        if let Some((line, used)) = cell {
            frame.render_widget(
                Paragraph::new(line),
                Rect {
                    width: used,
                    ..area
                },
            );
        }
        // Then the bar, starting after the cell, with a gap when there was a marker.
        let gap = u16::from(used > 0).saturating_mul(2).min(area.width - used);
        let rest = Rect {
            x: area.x + used,
            width: area.width - used,
            ..area
        };
        let bar = self.bar();
        frame.render_widget(Block::default().style(bar), rest);
        let hints_area = Rect {
            x: rest.x + gap,
            width: rest.width.saturating_sub(gap),
            ..rest
        };
        if hints_area.width == 0 {
            return;
        }
        frame.render_widget(
            Paragraph::new(self.hint_line(bar, hints_area.width)),
            hints_area,
        );
    }

    /// The hints that fit in `width` columns, key chips in the theme's key colour.
    fn hint_line(&self, bar: Style, width: u16) -> Line<'static> {
        let key_style = bar.patch(self.look.accent(
            Style::new().add_modifier(Modifier::BOLD),
            self.look.palette().key,
        ));
        let mut spans = vec![Span::styled(" ".to_owned(), bar)];
        let mut used = 1_usize;
        for hint in self.hints {
            let key = hint.key.as_str();
            let label = hint.label.as_str();
            let need = key.chars().count() + 1 + label.chars().count() + 2;
            if used + need > usize::from(width) {
                break;
            }
            spans.push(Span::styled(key.to_owned(), key_style));
            spans.push(Span::styled(format!(" {label}  "), bar));
            used += need;
        }
        Line::from(spans)
    }
}

/// `UTC HH:MM` for the header, from a Unix time in seconds.
#[must_use]
pub(crate) fn clock_text(unix_seconds: u64) -> String {
    let minutes = unix_seconds / 60 % (24 * 60);
    format!("UTC {:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests;
