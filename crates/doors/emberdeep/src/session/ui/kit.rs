//! The drawing kit every Emberdeep screen shares: the palette (the theme's roles plus a few
//! Emberdeep accents), the meter, the panels, the key chips and a few helpers. Nothing here knows a
//! rule; a screen hands it numbers and words.
//!
//! **Colour.** Text uses the theme's roles ([`Roles`], from `DoorFrame::style`). Art (borders,
//! meters, the map, the town skyline) uses the accents below, which are plain RGB: the framework
//! brings them down to the member's colour depth and drops them for a member with colour off, so
//! nothing here may rely on colour alone (every meter prints its numbers, every threat has a word).
//! **Icons.** Block elements, box drawing and the few arrows and bullets that the ASCII tier maps
//! (D104) are fine; `Look::ascii` lets a screen choose better letters where the stand-in would be
//! ugly.

use russh_bbs_door::DoorFrame;
use russh_bbs_door::DoorStyle;
use russh_bbs_door::buffer::Buffer;
use russh_bbs_door::layout::Rect;
use russh_bbs_door::style::{Color, Modifier, Style};
use russh_bbs_door::text::{Line, Span};
use russh_bbs_door::widgets::{Block, BorderType, Borders, Widget};

/// Torchlight at its brightest.
pub(super) const EMBER_HOT: Color = Color::Rgb(255, 214, 120);
/// A bright amber, the top of the meters' heat (it stays orange or yellow at 16 colours).
pub(super) const AMBER: Color = Color::Rgb(255, 196, 64);
/// The main ember orange.
pub(super) const EMBER: Color = Color::Rgb(255, 138, 40);
/// A dying coal.
pub(super) const EMBER_DIM: Color = Color::Rgb(184, 56, 24);
/// The barely-warm dark of a cooled forge.
pub(super) const COAL: Color = Color::Rgb(58, 30, 20);
/// Cold stone, light to dark.
pub(super) const STONE_LIGHT: Color = Color::Rgb(170, 178, 190);
pub(super) const STONE: Color = Color::Rgb(112, 120, 134);
pub(super) const STONE_DARK: Color = Color::Rgb(62, 68, 80);
pub(super) const STONE_DEEP: Color = Color::Rgb(36, 40, 50);
/// The deep blue of the cold levels and of anything remembered.
pub(super) const DEEP: Color = Color::Rgb(36, 64, 200);
pub(super) const DEEP_BRIGHT: Color = Color::Rgb(104, 152, 232);
/// Blood, moss and bone, for health, safety and the dead.
pub(super) const BLOOD: Color = Color::Rgb(206, 52, 48);
pub(super) const MOSS: Color = Color::Rgb(64, 200, 80);
pub(super) const BONE: Color = Color::Rgb(226, 216, 190);

/// The theme's seven roles, as styles.
#[derive(Clone, Copy)]
pub(in crate::session) struct Roles {
    pub plain: Style,
    pub dim: Style,
    pub strong: Style,
    pub accent: Style,
    pub good: Style,
    pub bad: Style,
}

impl Roles {
    /// No styling at all (for counting rows without a frame).
    pub(super) fn none() -> Self {
        let n = Style::new();
        Self {
            plain: n,
            dim: n,
            strong: n,
            accent: n,
            good: n,
            bad: n,
        }
    }

    pub(super) fn of(frame: &DoorFrame<'_>) -> Self {
        Self {
            plain: frame.style(DoorStyle::Plain),
            dim: frame.style(DoorStyle::Dim),
            strong: frame.style(DoorStyle::Strong),
            accent: frame.style(DoorStyle::Accent),
            good: frame.style(DoorStyle::Good),
            bad: frame.style(DoorStyle::Bad),
        }
    }

    /// The role a log tone is drawn in.
    pub(super) fn tone(&self, tone: crate::game::Tone) -> Style {
        match tone {
            crate::game::Tone::Plain => self.plain,
            crate::game::Tone::Good => self.good,
            crate::game::Tone::Bad => self.bad,
            crate::game::Tone::Dim => self.dim,
        }
    }
}

/// What a screen needs to know about the window and the member.
#[derive(Clone, Copy)]
pub(in crate::session) struct Look {
    pub ascii: bool,
    /// The member has colour on, so art may rely on it for shading.
    pub color: bool,
    /// A small number that changes with what the hero does, so the torchlight and the fog do not
    /// sit still but a replay and a snapshot still draw the same.
    pub phase: u32,
    pub w: u16,
    pub h: u16,
}

impl Look {
    /// Room for three columns.
    pub(super) const fn wide(&self) -> bool {
        self.w >= 100
    }

    /// Room for the tall layouts.
    pub(super) const fn tall(&self) -> bool {
        self.h >= 28
    }
}

/// Mix two colours; `t` from 0 (all `a`) to 1 (all `b`). Non-RGB colours are returned as is.
pub(super) fn lerp(a: Color, b: Color, t: f32) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            let t = t.clamp(0.0, 1.0);
            let mix = |x: u8, y: u8| -> u8 {
                let v = f32::from(x) + (f32::from(y) - f32::from(x)) * t;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let out = v.round().clamp(0.0, 255.0) as u8;
                out
            };
            Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
        }
        _ => {
            if t < 0.5 {
                a
            } else {
                b
            }
        }
    }
}

/// The heat of a fire from 0 (coal) to 1 (white-hot). The stops are saturated, so that at 16
/// colours the ramp reads black, red, yellow, white and never falls into grey.
pub(super) fn heat(t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let stops = [
        (0.0, Color::Rgb(58, 22, 16)),
        (0.35, Color::Rgb(184, 48, 20)),
        (0.65, Color::Rgb(236, 112, 18)),
        (0.85, Color::Rgb(255, 196, 64)),
        (1.0, Color::Rgb(255, 232, 150)),
    ];
    for pair in stops.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if t <= t1 {
            return lerp(c0, c1, (t - t0) / (t1 - t0));
        }
    }
    stops[stops.len() - 1].1
}

/// Health from blood through amber to green. Saturated stops, so that at 16 colours it reads red,
/// yellow, green and never grey.
pub(super) fn vitality(t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let stops = [
        (0.0, BLOOD),
        (0.45, AMBER),
        (0.7, AMBER),
        (0.88, MOSS),
        (1.0, MOSS),
    ];
    for pair in stops.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if t <= t1 {
            return lerp(c0, c1, (t - t0) / (t1 - t0));
        }
    }
    MOSS
}

/// A cheap deterministic hash for texture (fog, stars, sparks).
pub(super) fn hash(a: u32, b: u32, c: u32) -> u32 {
    let mut h =
        a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA6B) ^ c.wrapping_mul(0xC2B2_AE35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^ (h >> 15)
}

/// Fraction `num / den` in 0..=1 (0 when `den` is 0).
#[allow(clippy::cast_precision_loss)]
pub(super) fn frac(num: i64, den: i64) -> f32 {
    if den <= 0 {
        return 0.0;
    }
    (num as f32 / den as f32).clamp(0.0, 1.0)
}

/// Cut `text` to `width` columns, ending in an ellipsis when it was cut.
pub(super) fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_owned()
    } else {
        let cut: String = text.chars().take(width.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// Word-wrap into at most `max_lines` lines of `width`, the last ending in an ellipsis when
/// the text was longer.
pub(super) fn wrap_to(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let mut lines = crate::session::wrap(text, width.max(4));
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            *last = clip(&format!("{last} …"), width);
            if !last.ends_with('…') {
                last.push('…');
            }
        }
    }
    lines
}

/// A bordered panel: rounded, stone, with a title in `title_style`. `lit` makes the border ember.
pub(super) fn panel<'a>(title: &str, lit: bool) -> Block<'a> {
    let border = if lit { EMBER_DIM } else { STONE_DARK };
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border));
    if !title.is_empty() {
        block = block.title(Line::from(vec![
            Span::styled(" ", Style::new()),
            Span::styled(
                title.to_owned(),
                Style::new()
                    .fg(if lit { EMBER } else { STONE_LIGHT })
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::new()),
        ]));
    }
    block
}

/// The inner area of a panel.
pub(super) fn inner(rect: Rect) -> Rect {
    Rect::new(
        rect.x.saturating_add(1),
        rect.y.saturating_add(1),
        rect.width.saturating_sub(2),
        rect.height.saturating_sub(2),
    )
}

/// A bar that fills left to right in eighths, with an optional gradient and its numbers beside
/// it. `label` is drawn first when there is room; `text` last.
pub(super) struct Meter {
    pub label: String,
    pub frac: f32,
    pub text: String,
    pub ramp: Ramp,
    pub label_style: Style,
    pub text_style: Style,
    /// Width reserved for the label (0: none).
    pub label_w: u16,
}

/// How a meter's colour runs along its bar.
#[derive(Clone, Copy)]
pub(super) enum Ramp {
    /// Between two colours (use two that blend well).
    Lerp(Color, Color),
    /// Along the fire ramp, from one heat to another.
    Heat(f32, f32),
    /// Along health, from blood to moss, over the whole bar.
    Vital,
}

impl Ramp {
    fn at(self, t: f32) -> Color {
        match self {
            Self::Lerp(a, b) => lerp(a, b, t),
            Self::Heat(a, b) => heat(a + (b - a) * t),
            Self::Vital => vitality(t),
        }
    }
}

impl Meter {
    pub(super) fn new(
        label: &str,
        frac: f32,
        text: impl Into<String>,
        from: Color,
        to: Color,
    ) -> Self {
        Self::with(label, frac, text, Ramp::Lerp(from, to))
    }

    pub(super) fn heated(
        label: &str,
        frac: f32,
        text: impl Into<String>,
        from: f32,
        to: f32,
    ) -> Self {
        Self::with(label, frac, text, Ramp::Heat(from, to))
    }

    pub(super) fn vital(label: &str, frac: f32, text: impl Into<String>) -> Self {
        Self::with(label, frac, text, Ramp::Vital)
    }

    fn with(label: &str, frac: f32, text: impl Into<String>, ramp: Ramp) -> Self {
        Self {
            label: label.to_owned(),
            frac,
            text: text.into(),
            ramp,
            label_style: Style::new().fg(STONE_LIGHT),
            text_style: Style::new().fg(BONE),
            label_w: 0,
        }
    }

    pub(super) fn labelled(mut self, width: u16, style: Style, text_style: Style) -> Self {
        self.label_w = width;
        self.label_style = style;
        self.text_style = text_style;
        self
    }
}

const EIGHTHS: [&str; 9] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];

impl Widget for Meter {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let text_w = u16::try_from(self.text.chars().count()).unwrap_or(u16::MAX);
        let label_w = if area.width >= 18 { self.label_w } else { 0 };
        let mut x = area.x;
        if label_w > 0 {
            buf.set_stringn(
                x,
                area.y,
                &self.label,
                usize::from(label_w),
                self.label_style,
            );
            x += label_w;
        }
        let spare = area.right().saturating_sub(x);
        let show_text = spare >= text_w + 6;
        let bar_w = if show_text { spare - text_w - 1 } else { spare };
        if bar_w == 0 {
            return;
        }
        let total = f32::from(bar_w) * 8.0 * self.frac.clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let eighths = total.round() as u32;
        let full = u16::try_from(eighths / 8).unwrap_or(u16::MAX).min(bar_w);
        let part = (eighths % 8) as usize;
        for i in 0..bar_w {
            let t = if bar_w <= 1 {
                1.0
            } else {
                f32::from(i) / f32::from(bar_w - 1)
            };
            let color = self.ramp.at(t);
            let cell_x = x + i;
            if i < full {
                buf.set_string(cell_x, area.y, "█", Style::new().fg(color));
            } else if i == full && part > 0 {
                buf.set_string(cell_x, area.y, EIGHTHS[part], Style::new().fg(color));
            } else {
                buf.set_string(cell_x, area.y, "░", Style::new().fg(STONE_DEEP));
            }
        }
        if show_text {
            buf.set_stringn(
                x + bar_w + 1,
                area.y,
                &self.text,
                usize::from(text_w),
                self.text_style,
            );
        }
    }
}

/// `n` filled pips out of `of`: `●●○○○`.
pub(super) fn pips(n: u32, of: u32, on: Color) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    for i in 0..of {
        if i < n {
            out.push(Span::styled("●", Style::new().fg(on)));
        } else {
            out.push(Span::styled("○", Style::new().fg(STONE_DARK)));
        }
    }
    out
}

/// A gradient rule: a line of `glyph` from `from` to `to`.
pub(super) struct Rule {
    pub glyph: &'static str,
    pub from: Color,
    pub to: Color,
}

impl Widget for Rule {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for i in 0..area.width {
            let t = if area.width <= 1 {
                0.0
            } else {
                f32::from(i) / f32::from(area.width - 1)
            };
            buf.set_string(
                area.x + i,
                area.y,
                self.glyph,
                Style::new().fg(lerp(self.from, self.to, t)),
            );
        }
    }
}

/// One command on a key strip.
#[derive(Clone)]
pub(super) struct Chip {
    pub key: String,
    pub label: String,
    pub on: bool,
    /// The one under the cursor.
    pub hot: bool,
}

impl Chip {
    pub(super) fn new(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            on: true,
            hot: false,
        }
    }

    pub(super) fn off(mut self) -> Self {
        self.on = false;
        self
    }
}

/// Chips laid out as `[k] label` runs, wrapping onto as many rows as the area has. Returns the
/// rows used.
pub(super) struct Keys<'a> {
    pub chips: &'a [Chip],
    pub roles: Roles,
}

impl Keys<'_> {
    /// How many rows the chips need at `width`.
    pub(super) fn rows(chips: &[Chip], width: u16) -> u16 {
        let mut rows = 1u16;
        let mut x = 0usize;
        for c in chips {
            let w = c.key.chars().count() + c.label.chars().count() + 4;
            if x > 0 && x + w > usize::from(width) {
                rows += 1;
                x = 0;
            }
            x += w;
        }
        rows
    }
}

impl Widget for Keys<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut x = area.x;
        let mut y = area.y;
        for c in self.chips {
            let w = u16::try_from(c.key.chars().count() + c.label.chars().count() + 4)
                .unwrap_or(u16::MAX);
            if x > area.x && x + w > area.right() {
                x = area.x;
                y += 1;
            }
            if y >= area.bottom() {
                return;
            }
            let (key_style, label_style) = if c.hot {
                (
                    Style::new().fg(EMBER_HOT).add_modifier(Modifier::BOLD),
                    Style::new().fg(EMBER_HOT).add_modifier(Modifier::BOLD),
                )
            } else if c.on {
                (self.roles.accent, self.roles.plain)
            } else {
                (self.roles.dim, self.roles.dim)
            };
            let k = format!("[{}]", c.key);
            let (ex, _) = buf.set_stringn(
                x,
                y,
                &k,
                usize::from(area.right().saturating_sub(x)),
                key_style,
            );
            let (ex2, _) = buf.set_stringn(
                ex + 1,
                y,
                &c.label,
                usize::from(area.right().saturating_sub(ex + 1)),
                label_style,
            );
            x = ex2 + 2;
        }
    }
}

/// A line of spans from `(text, style)` pairs.
pub(super) fn spans(parts: &[(&str, Style)]) -> Line<'static> {
    Line::from(
        parts
            .iter()
            .map(|(t, s)| Span::styled((*t).to_owned(), *s))
            .collect::<Vec<_>>(),
    )
}
