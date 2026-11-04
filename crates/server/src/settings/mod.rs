//! Drawing the signed-in screens (login, glyph test, settings, home, logoff). The rules live in
//! `russh_bbs_core::terminal_settings`; this module only turns a [`View`] into ratatui widgets
//! according to the person's [`TerminalSettings`] (M1-E).
//!
//! * **Color off** draws with bold and dim only. Every state is also in words, so nothing
//!   depends on color.
//! * **ASCII tier** draws the border with `+ - |` instead of line-drawing characters, so the
//!   plain layout needs no Unicode at all.
//! * No security marker is drawn on these screens. When one is needed it must go through
//!   `Rendered::to_terminal()` / `chrome_style()` in a chrome-only position (D39), never through
//!   this renderer's content styles.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::terminal_settings::{LayoutId, TerminalSettings};
use russh_bbs_core::waiting_room::View;

use crate::ui::classic::notice_line;
use crate::ui::depth::{ColorDepth, Surfaces, Tint};
use crate::ui::strings::StringTable;
use crate::ui::theme::skin::ASCII_BORDER;
use crate::ui::theme::{BorderSet, Element, Palette, Skin, Theme, TitleAlign};

mod prefs;

/// Widest the box is drawn, so long lines stay readable on a wide terminal.
pub(crate) const MAX_WIDTH: u16 = 76;

/// How a view is drawn, derived from the person's settings, the style and the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Look {
    color: bool,
    ascii_borders: bool,
    /// The compact layout draws no box (M3-A).
    borderless: bool,
    /// The layout in force (a fallback for a small terminal, not the member's choice).
    layout: LayoutId,
    palette: Palette,
    skin: Skin,
    /// What the client can show (UI-1): the frame's surfaces degrade to it.
    depth: ColorDepth,
    /// The member's ceiling on the depth (UI-2); `None` is automatic.
    cap: Option<ColorDepth>,
    /// Whether motion is allowed: the member wants it, the board allows it and the style has it.
    motion: bool,
    surfaces: Surfaces,
}

impl Look {
    /// The look for `settings`: unanswered glyph test and a rejected one both give the plain
    /// layout (ASCII, no color), because [`TerminalSettings`] says so.
    pub(crate) fn of(settings: &TerminalSettings) -> Self {
        Self::themed(settings, settings.layout(), Palette::default())
    }

    /// The look for `settings` drawn in `layout` (the layout actually in force, which may be a
    /// fallback for a small terminal) with a theme's `palette` and the default skin.
    pub(crate) fn themed(settings: &TerminalSettings, layout: LayoutId, palette: Palette) -> Self {
        let cap = prefs::depth_cap(settings);
        Self {
            color: settings.use_color(),
            ascii_borders: settings.icon_tier() == Tier::Ascii || prefs::ascii_borders(settings),
            borderless: layout == LayoutId::Compact,
            layout,
            palette,
            skin: Skin::default(),
            // Until the client's terminal is known the sixteen colours are the safe depth;
            // colour off is always mono.
            depth: ColorDepth::effective(ColorDepth::Ansi16, settings.use_color(), cap),
            cap,
            motion: prefs::motion(settings),
            surfaces: palette.surfaces(),
        }
    }

    /// The look for `settings` in `layout` with all of `theme`: palette and skin.
    pub(crate) fn for_theme(settings: &TerminalSettings, layout: LayoutId, theme: &Theme) -> Self {
        let mut look = Self::themed(settings, layout, theme.palette());
        look.skin = theme.skin();
        look.motion = look.motion && theme.skin().motion;
        look
    }

    /// This look with the board's identity applied: its accent colour replaces the style's, and
    /// a board that turns motion off turns it off for everyone.
    #[must_use]
    pub(crate) fn with_board(mut self, accent: Option<Tint>, motion_allowed: bool) -> Self {
        if let Some(accent) = accent {
            self.palette = self.palette.with_accent(accent);
        }
        self.motion = self.motion && motion_allowed;
        self
    }

    /// This look drawn at the depth the client advertised (`detected`, from `TERM`,
    /// `COLORTERM` or the telnet terminal type), lowered by the member's settings. Colour off
    /// stays mono whatever the client can do.
    #[must_use]
    pub(crate) fn at_depth(mut self, detected: ColorDepth) -> Self {
        self.depth = ColorDepth::effective(detected, self.color, self.cap);
        self.color = self.depth.has_color();
        self
    }

    /// The depth in force.
    pub(crate) const fn depth(self) -> ColorDepth {
        self.depth
    }

    /// Whether borders are drawn with plain ASCII (the ASCII tier, or the member's choice).
    pub(crate) const fn ascii(self) -> bool {
        self.ascii_borders
    }

    /// Whether the layout draws no box (compact).
    pub(crate) const fn is_borderless(self) -> bool {
        self.borderless
    }

    /// The layout in force.
    pub(crate) const fn layout(self) -> LayoutId {
        self.layout
    }

    /// The style's drawing choices.
    pub(crate) const fn skin(self) -> Skin {
        self.skin
    }

    /// Whether motion is allowed on this screen. Nothing animates yet (UI-4 and UI-8 will); the
    /// flag is here so they read one place.
    #[allow(dead_code, reason = "read by UI-4 and UI-8 when motion lands")]
    pub(crate) const fn motion(self) -> bool {
        self.motion
    }

    /// The frame's surface colours for this theme.
    pub(crate) const fn surfaces(self) -> Surfaces {
        self.surfaces
    }

    /// A foreground style for `tint` at this depth: nothing on a mono terminal.
    pub(crate) fn fg(self, tint: Tint) -> Style {
        match tint.resolve(self.depth) {
            Some(Color::Reset) | None => Style::new(),
            Some(color) => Style::new().fg(color),
        }
    }

    /// A background style for `tint` at this depth: nothing on a mono terminal, and nothing
    /// where the tint has no sixteen-colour equivalent.
    pub(crate) fn bg(self, tint: Tint) -> Style {
        match tint.resolve(self.depth) {
            Some(Color::Reset) | None => Style::new(),
            Some(color) => Style::new().bg(color),
        }
    }

    /// The colour `tint` resolves to at this depth, if any.
    pub(crate) fn color_of(self, tint: Tint) -> Option<Color> {
        match tint.resolve(self.depth) {
            Some(Color::Reset) | None => None,
            Some(color) => Some(color),
        }
    }

    /// The theme's colours.
    pub(crate) const fn palette(self) -> Palette {
        self.palette
    }

    /// `style` with `tint` as its foreground, when colour is on.
    pub(crate) fn accent(self, style: Style, tint: Tint) -> Style {
        style.patch(self.fg(tint))
    }

    /// Quiet text: the style's muted colour, or dim where the style has none (and on mono).
    pub(crate) fn muted(self) -> Style {
        match self.color_of(self.palette.muted) {
            Some(color) => Style::new().fg(color),
            None => Style::new().add_modifier(Modifier::DIM),
        }
    }

    /// Ordinary text on a panel: the style's text and panel colours.
    pub(crate) fn text(self) -> Style {
        self.fg(self.palette.text)
            .patch(self.bg(self.palette.panel))
    }

    /// The border block for `element`: the skin's border set (ASCII in the ASCII tier or when
    /// the member chose it), the border colour, the panel background and the title written in
    /// the skin's treatment. `centered` is the screen's own title alignment when the skin has
    /// none. The block does not clear what is under it; use [`Look::panel`] for that.
    pub(crate) fn block(
        self,
        title: Option<&str>,
        element: Element,
        centered: bool,
    ) -> Block<'static> {
        let set = if self.ascii_borders {
            BorderSet::Ascii
        } else {
            self.skin.border(element)
        };
        let mut block = Block::default().style(self.text());
        if set != BorderSet::None && !self.borderless {
            block = block
                .borders(Borders::ALL)
                .border_style(self.accent(Style::new(), self.palette.border));
            block = match (set.border_type(), set) {
                (Some(kind), _) => block.border_type(kind),
                (None, BorderSet::Ascii) => block.border_set(ASCII_BORDER),
                (None, _) => block,
            };
        }
        if let (Some(title), false) = (title, self.borderless) {
            let heading = self.accent(
                Style::new().add_modifier(Modifier::BOLD),
                self.palette.title,
            );
            let text = self.skin.title.render(title, self.ascii_borders);
            let align = match self.skin.align {
                Some(TitleAlign::Left) => Alignment::Left,
                Some(TitleAlign::Center) => Alignment::Center,
                None if centered => Alignment::Center,
                None => Alignment::Left,
            };
            block = block
                .title(Line::from(Span::styled(text, heading)))
                .title_alignment(align);
        }
        block
    }

    /// Clear `area`, draw the `element` box in it and return the inside: a panel that hides
    /// any backdrop under it.
    pub(crate) fn panel(
        self,
        frame: &mut Frame<'_>,
        area: Rect,
        title: Option<&str>,
        element: Element,
        centered: bool,
    ) -> Rect {
        let block = self.block(title, element, centered);
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        inner
    }
}

/// Draw `view` centered in the frame. The view's fixed fields are string-table keys (the member
/// screens) or text (anything else); `strings` resolves the keys and leaves other text alone.
#[cfg(test)]
pub(crate) fn render(frame: &mut Frame<'_>, view: &View, look: Look, strings: &StringTable) {
    boxed(
        frame,
        frame.area(),
        view.title,
        lines(view, look, strings),
        look,
    );
}

/// Like [`render`], centred in `area` (the body a frame leaves for the screen).
pub(crate) fn render_in(
    frame: &mut Frame<'_>,
    area: Rect,
    view: &View,
    look: Look,
    strings: &StringTable,
) {
    boxed_flush(frame, area, view.title, lines(view, look, strings), look);
}

/// Draw `lines` in the bordered box, centered in `area`. Shared with the menu framework's
/// layout (`crate::ui`), which draws its content here and keeps its own reserved rows outside
/// `area`.
pub(crate) fn boxed(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    look: Look,
) {
    boxed_with(frame, area, title, lines, look, true);
}

/// Like [`boxed`] for a body the frame already insets (between its header and status bars): the
/// box fills the body's height, with no extra margin.
pub(crate) fn boxed_flush(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    look: Look,
) {
    boxed_with(frame, area, title, lines, look, false);
}

fn boxed_with(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    look: Look,
    margin: bool,
) {
    let [_, middle, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(MAX_WIDTH.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let block = look.block(Some(title), Element::Frame, true);
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(block);
    let rect = if margin { fit(middle) } else { middle };
    frame.render_widget(Clear, rect);
    frame.render_widget(paragraph, rect);
}

/// The columns and rows inside the box [`boxed`] draws in `area`: what a caller has to fit its
/// text into if it wants to page or wrap it itself.
pub(crate) fn inner_size(area: Rect) -> (usize, usize) {
    let [_, middle, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(MAX_WIDTH.min(area.width)),
        Constraint::Fill(1),
    ])
    .areas(area);
    let boxed = fit(middle);
    (
        usize::from(boxed.width.saturating_sub(2)),
        usize::from(boxed.height.saturating_sub(2)),
    )
}

/// Keep the box off the very edge of a roomy terminal; use all of a small one, and all of an
/// 80x24 screen (the settings screen needs those two rows, M3-A).
fn fit(area: Rect) -> Rect {
    if area.height > 24 {
        area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        })
    } else {
        area
    }
}

fn lines(view: &View, look: Look, strings: &StringTable) -> Vec<Line<'static>> {
    let bold = Style::new().add_modifier(Modifier::BOLD);
    let dim = Style::new().add_modifier(Modifier::DIM);
    let heading = look.accent(bold, look.palette.heading);
    let key = look.accent(bold, look.palette.key);
    let mut out = Vec::new();
    if !view.heading.is_empty() {
        out.push(Line::styled(strings.text(view.heading).to_owned(), heading));
        out.push(Line::raw(""));
    }
    for text in view.banner.iter().chain(&view.body) {
        out.push(Line::raw(text.clone()));
    }
    if let Some(note) = view.note {
        out.push(Line::raw(""));
        out.push(Line::styled(strings.text(note).to_owned(), dim));
    }
    if !view.menu.is_empty() {
        out.push(Line::raw(""));
    }
    for item in &view.menu {
        out.push(Line::from(vec![
            Span::styled(format!("[{}]", item.key), key),
            Span::raw(format!(" {}", strings.text(item.label))),
        ]));
    }
    for field in &view.fields {
        let cursor = if field.active { "_" } else { "" };
        out.push(Line::styled(
            format!("{}:", strings.text(field.label)),
            bold,
        ));
        out.push(Line::raw(format!("> {}{cursor}", field.value)));
    }
    if let Some(notice) = view.notice {
        // The word, not only a color: the message must read correctly on a mono terminal.
        out.push(Line::raw(""));
        out.push(notice_line(strings, notice.kind, notice.text, look, bold));
    }
    if !view.footer.is_empty() {
        out.push(Line::raw(""));
        out.push(Line::styled(strings.text(view.footer).to_owned(), dim));
    }
    out
}

#[cfg(test)]
mod tests;
