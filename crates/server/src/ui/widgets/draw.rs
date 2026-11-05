//! The widgets' charts: the shared `ui/charts` widgets (UI-8) drawn into a scratch buffer and
//! read back as styled text, so a widget stays a list of lines.
//!
//! The charts take their colours from the palette roles through `ChartTheme::of(look)`, sanitize
//! their own labels, never set bold or reverse, and draw seven-bit characters in the ASCII tier.
//! Nothing here draws a security marker or reserved style.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget as _;

use crate::settings::Look;
use crate::ui::charts::{ChartTheme, LineChart, Meter, Sparkline, Tone};

/// Draw `chart` into a `width` x `height` scratch buffer and return its rows as styled lines.
pub(crate) fn painted(
    width: usize,
    height: usize,
    chart: impl FnOnce(Rect, &mut Buffer),
) -> Vec<Line<'static>> {
    let (Ok(w), Ok(h)) = (u16::try_from(width), u16::try_from(height)) else {
        return Vec::new();
    };
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    chart(area, &mut buf);
    (0..h)
        .map(|y| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut run = String::new();
            let mut run_style = Style::new();
            for x in 0..w {
                let cell = &buf[(x, y)];
                let style = cell.style();
                if !run.is_empty() && style != run_style {
                    spans.push(Span::styled(std::mem::take(&mut run), run_style));
                }
                run_style = style;
                run.push_str(cell.symbol());
            }
            if !run.is_empty() {
                spans.push(Span::styled(run, run_style));
            }
            Line::from(spans)
        })
        .collect()
}

/// A bar `width` cells wide filled to `part` of `whole`, as spans for one line.
pub(crate) fn meter(
    theme: ChartTheme,
    part: u64,
    whole: u64,
    width: usize,
    tone: Tone,
) -> Vec<Span<'static>> {
    painted(width, 1, |area, buf| {
        Meter::new(part, whole, theme).tone(tone).render(area, buf);
    })
    .into_iter()
    .flat_map(|l| l.spans)
    .collect()
}

/// A one-row sparkline of the newest `width` values.
pub(crate) fn sparkline(
    theme: ChartTheme,
    series: &[u64],
    width: usize,
    tone: Tone,
) -> Line<'static> {
    painted(width, 1, |area, buf| {
        Sparkline::new(series, theme).tone(tone).render(area, buf);
    })
    .into_iter()
    .next()
    .unwrap_or_default()
}

/// Tallest area chart: more rows add nothing, and the braille canvas counts dots in `u16`.
const MAX_CHART_ROWS: usize = 32;

/// A braille area chart of `series`, scaled from zero to its largest value, at most
/// [`MAX_CHART_ROWS`] rows tall.
#[allow(
    clippy::cast_precision_loss,
    reason = "chart heights only need the ratio; a 52-bit mantissa is far finer than a terminal"
)]
pub(crate) fn area(
    theme: ChartTheme,
    series: &[u64],
    width: usize,
    rows: usize,
) -> Vec<Line<'static>> {
    let data: Vec<f64> = series.iter().map(|v| *v as f64).collect();
    let top = series.iter().copied().max().unwrap_or(0).max(1) as f64;
    painted(width, rows.min(MAX_CHART_ROWS), |area, buf| {
        LineChart::new(&data, theme)
            .filled()
            .range(0.0, top)
            .render(area, buf);
    })
}

/// The chart theme for `look`, forced to the ASCII forms when `ascii` is set.
pub(crate) fn theme(look: Look, ascii: bool) -> ChartTheme {
    ChartTheme::of(look).with_ascii(ascii)
}
