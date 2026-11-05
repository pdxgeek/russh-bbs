#![allow(clippy::unwrap_used)]
#![allow(
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::unchecked_time_subtraction,
    clippy::assert_is_empty,
    clippy::manual_range_contains,
    reason = "tests: small bounded numbers, and a failed expectation should panic"
)]
#![allow(
    clippy::many_single_char_names,
    reason = "pixel and colour maths use x, y, w, h, r, g, b by convention"
)]

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Widget;
use russh_bbs_core::terminal_settings::TerminalSettings;

use super::{BarChart, ChartTheme, Gauge, LineChart, Sparkline, Tone};
use crate::settings::Look;
use crate::ui::Ui;
use crate::ui::depth::ColorDepth;

fn look(depth: ColorDepth, ascii: bool) -> Look {
    let ui = Ui::standard().unwrap();
    let settings = TerminalSettings::default();
    let settings = if ascii {
        settings.with_tier(russh_bbs_core::icons::Tier::Ascii)
    } else {
        settings.with_tier(russh_bbs_core::icons::Tier::Unicode)
    };
    ui.look(&settings, Rect::new(0, 0, 80, 24), depth)
}

fn theme(depth: ColorDepth, ascii: bool) -> ChartTheme {
    ChartTheme::of(look(depth, ascii))
}

fn rows(buf: &Buffer) -> Vec<String> {
    (buf.area.y..buf.area.y + buf.area.height)
        .map(|y| {
            (buf.area.x..buf.area.x + buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect()
        })
        .collect()
}

fn draw(widget: impl Widget, w: u16, h: u16) -> Buffer {
    let area = Rect::new(0, 0, w, h);
    let mut buf = Buffer::empty(area);
    widget.render(area, &mut buf);
    buf
}

#[test]
fn sparkline_golden_unicode_and_ascii() {
    let data = [0, 1, 2, 4, 8, 4, 2, 1];
    let unicode = draw(
        Sparkline::new(&data, theme(ColorDepth::TrueColor, false)),
        8,
        1,
    );
    assert_eq!(rows(&unicode), ["▁▁▂▄█▄▂▁"]);
    let ascii = draw(
        Sparkline::new(&data, theme(ColorDepth::TrueColor, true)),
        8,
        1,
    );
    assert_eq!(rows(&ascii), ["_..:#:.."]);
}

#[test]
fn sparkline_is_right_aligned_and_keeps_the_newest_values() {
    let data = [8, 8, 8, 1, 8];
    let wide = draw(Sparkline::new(&data, theme(ColorDepth::Mono, false)), 8, 1);
    assert_eq!(rows(&wide), ["   ███▁█"]);
    let narrow = draw(Sparkline::new(&data, theme(ColorDepth::Mono, false)), 3, 1);
    assert_eq!(rows(&narrow), ["█▁█"]);
}

#[test]
fn sparkline_two_rows_has_sixteen_levels() {
    let data = [16, 12, 8, 4];
    let buf = draw(Sparkline::new(&data, theme(ColorDepth::Mono, false)), 4, 2);
    assert_eq!(rows(&buf), ["█▄  ", "███▄"]);
}

#[test]
fn sparkline_uses_the_accent_role_and_tone_roles() {
    let t = theme(ColorDepth::TrueColor, false);
    let palette = look(ColorDepth::TrueColor, false).palette();
    let accent = palette.accent.resolve(ColorDepth::TrueColor).unwrap();
    let plain = draw(Sparkline::new(&[5], t), 1, 1);
    assert_eq!(plain[(0, 0)].fg, accent);
    let bad = draw(Sparkline::new(&[5], t).tone(Tone::Bad), 1, 1);
    assert_eq!(
        bad[(0, 0)].fg,
        palette.error.resolve(ColorDepth::TrueColor).unwrap()
    );
    assert!(
        !bad[(0, 0)].modifier.contains(Modifier::REVERSED),
        "never the reserved style"
    );
}

#[test]
fn degenerate_inputs_do_not_panic() {
    let t = theme(ColorDepth::TrueColor, false);
    for (w, h) in [(0, 0), (1, 0), (0, 1), (1, 1), (3, 2)] {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        Sparkline::new(&[], t).render(area, &mut buf);
        Sparkline::new(&[0, 0], t).render(area, &mut buf);
        Sparkline::new(&[u64::MAX, 1], t).render(area, &mut buf);
        BarChart::new(&[], t).render(area, &mut buf);
        BarChart::new(&[("a", u64::MAX)], t).render(area, &mut buf);
        LineChart::new(&[], t).render(area, &mut buf);
        LineChart::new(&[f64::NAN, f64::INFINITY, f64::NEG_INFINITY], t).render(area, &mut buf);
        LineChart::new(&[1.0], t).filled().render(area, &mut buf);
        Gauge::new(f64::NAN, t).render(area, &mut buf);
        Gauge::new(-4.0, t).render(area, &mut buf);
        Gauge::new(1e300, t).label("x").render(area, &mut buf);
    }
    // A widget given an area bigger than the buffer is clipped, not a panic.
    let mut buf = Buffer::empty(Rect::new(0, 0, 5, 2));
    Sparkline::new(&[1, 2, 3], t).render(Rect::new(0, 0, 100, 100), &mut buf);
    LineChart::new(&[1.0, 2.0], t).render(Rect::new(3, 1, 100, 100), &mut buf);
}

#[test]
fn bar_chart_golden() {
    let data = [("alpha", 8), ("be", 4), ("c", 0)];
    let unicode = draw(BarChart::new(&data, theme(ColorDepth::Mono, false)), 20, 3);
    assert_eq!(
        rows(&unicode),
        [
            "alpha ████████████ 8",
            "be    ██████······ 4",
            "c     ············ 0"
        ]
    );
    let ascii = draw(BarChart::new(&data, theme(ColorDepth::Mono, true)), 20, 3);
    assert_eq!(
        rows(&ascii),
        [
            "alpha ############ 8",
            "be    ######------ 4",
            "c     ------------ 0"
        ]
    );
}

#[test]
fn bar_chart_scales_to_the_largest_value_and_to_a_given_max() {
    let data = [("a", 5), ("b", 10)];
    let auto = draw(BarChart::new(&data, theme(ColorDepth::Mono, true)), 14, 2);
    let capped = draw(
        BarChart::new(&data, theme(ColorDepth::Mono, true)).max(20),
        14,
        2,
    );
    let count = |buf: &Buffer, row: usize| {
        rows(buf)[row]
            .chars()
            .filter(|c| *c == '#' || *c == '=')
            .count()
    };
    assert!(
        count(&auto, 1) > count(&auto, 0),
        "bigger value, longer bar"
    );
    assert!(
        count(&capped, 1) < count(&auto, 1),
        "a larger max shortens the bars"
    );
    for line in rows(&auto) {
        assert!(line.is_ascii(), "ASCII tier: {line:?}");
    }
}

#[test]
fn labels_are_sanitized_like_any_content() {
    // Escape sequences, controls and the reserved marker glyphs and delimiters never reach a cell.
    let hostile = "\u{1b}[31mred\u{7}\u{27E6}\u{2299}\u{27E7}x";
    let data = [(hostile, 3)];
    let buf = draw(
        BarChart::new(&data, theme(ColorDepth::TrueColor, false)),
        40,
        1,
    );
    let line = rows(&buf).concat();
    for forbidden in ['\u{1b}', '\u{7}', '\u{27E6}', '\u{27E7}', '\u{2299}'] {
        assert!(!line.contains(forbidden), "{forbidden:?} in {line:?}");
    }
    let gauge = draw(
        Gauge::new(0.5, theme(ColorDepth::TrueColor, false)).label(hostile),
        40,
        1,
    );
    let line = rows(&gauge).concat();
    for forbidden in ['\u{1b}', '\u{7}', '\u{27E6}', '\u{27E7}', '\u{2299}'] {
        assert!(!line.contains(forbidden), "{forbidden:?} in {line:?}");
    }
    // And no cell wears the reserved style (bold plus reverse).
    for buf in [&buf, &gauge] {
        for cell in &buf.content {
            assert!(!cell.modifier.contains(Modifier::BOLD | Modifier::REVERSED));
        }
    }
}

#[test]
fn gauge_golden_and_tone() {
    let unicode = draw(Gauge::new(0.5, theme(ColorDepth::Mono, false)), 16, 1);
    assert_eq!(rows(&unicode), ["█████▌░░░░░  50%"]);
    let ascii = draw(Gauge::new(0.25, theme(ColorDepth::Mono, true)), 16, 1);
    assert_eq!(rows(&ascii), ["##=--------  25%"]);
    let labelled = draw(
        Gauge::new(0.75, theme(ColorDepth::Mono, false)).label("cpu"),
        20,
        1,
    );
    assert_eq!(rows(&labelled), ["cpu ████████▎░░  75%"]);
    assert_eq!(Tone::for_ratio(0.5, 0.7, 0.9), Tone::Ok);
    assert_eq!(Tone::for_ratio(0.8, 0.7, 0.9), Tone::Warn);
    assert_eq!(Tone::for_ratio(0.95, 0.7, 0.9), Tone::Bad);
    let full = draw(Gauge::new(1.0, theme(ColorDepth::Mono, true)), 16, 1);
    assert!(rows(&full)[0].ends_with("100%"), "{:?}", rows(&full));
}

#[test]
fn line_chart_golden_in_braille_and_ascii() {
    let t = theme(ColorDepth::TrueColor, false);
    let data: Vec<f64> = (0..16).map(f64::from).collect();
    let line = draw(LineChart::new(&data, t), 8, 3);
    assert_eq!(rows(&line), ["     ⢀⠔⠊", "  ⢀⡠⠊⠁  ", "⡠⠔⠁     "]);
    let area = draw(LineChart::new(&data, t).filled(), 8, 3);
    assert_eq!(rows(&area), ["     ⢀⣴⣾", "  ⢀⣠⣾⣿⣿⣿", "⣠⣴⣿⣿⣿⣿⣿⣿"]);
    let ascii = draw(
        LineChart::new(&data, theme(ColorDepth::Mono, true)).filled(),
        8,
        3,
    );
    assert_eq!(rows(&ascii), ["     .+#", "  .:####", ":+######"]);
    // Colours come from the palette accent, never the reserved style.
    // (A braille dot is drawn in the RGB the accent role stands for.)
    let (r, g, b) = look(ColorDepth::TrueColor, false)
        .palette()
        .accent
        .rgb()
        .unwrap();
    let first = line.content.iter().find(|c| c.symbol() != " ").unwrap();
    assert_eq!(first.fg, Color::Rgb(r, g, b));
    assert!(!first.modifier.contains(Modifier::REVERSED));
}
