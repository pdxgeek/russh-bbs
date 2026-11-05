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
    clippy::too_many_lines,
    clippy::format_push_string,
    reason = "tests: small bounded numbers, and a failed expectation should panic"
)]
#![allow(
    clippy::many_single_char_names,
    reason = "pixel and colour maths use x, y, w, h, r, g, b by convention"
)]

//! A gallery of the UI-8 charts, canvases and image-to-cells at 132x43, in each tier and depth.
//! The regular test draws it everywhere to prove nothing panics and the ASCII tier is ASCII;
//! the ignored one writes `.cells` dumps for `docs/qa/m4-s3-screens/render.py`
//! (`UI8_GALLERY_OUT=dir cargo test ... gallery_dump -- --ignored`).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Widget};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::terminal_settings::TerminalSettings;

use super::canvas::{Braille, HalfBlock, Paint, Rgb, mix};
use super::charts::{BarChart, ChartTheme, Gauge, LineChart, Sparkline, Tone};
use super::depth::{ColorDepth, indexed_rgb};
use super::image::{Limits, decode_png};
use crate::settings::Look;
use crate::ui::Ui;

fn look(depth: ColorDepth, ascii: bool, style: &str) -> Look {
    let ui = Ui::standard().unwrap();
    let settings = TerminalSettings::default()
        .with_tier(if ascii { Tier::Ascii } else { Tier::Unicode })
        .with_theme(russh_bbs_core::terminal_settings::ThemeName::new(style).unwrap());
    ui.look(&settings, Rect::new(0, 0, 132, 43), depth)
}

/// A logo-like picture, made here so no fixture is committed: a radial gradient with a ring.
fn logo_png() -> Vec<u8> {
    let (w, h) = (96_u32, 96_u32);
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (f64::from(x) - 47.5, f64::from(y) - 47.5);
            let d = dx.hypot(dy);
            let ring = (d - 34.0).abs() < 3.0;
            let (r, g, b) = if ring {
                (255, 220, 120)
            } else if d < 30.0 {
                (
                    (40.0 + d * 4.0) as u8,
                    (120.0 + 60.0 * (1.0 - d / 30.0)) as u8,
                    (200.0 - d * 2.0) as u8,
                )
            } else {
                (14, 18, 36)
            };
            data.extend([r, g, b]);
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&data).unwrap();
    out
}

fn panel(buf: &mut Buffer, look: Look, area: Rect, title: &str) -> Rect {
    let palette = look.palette();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(look.fg(palette.border))
        .title(ratatui::text::Span::styled(
            format!(" {title} "),
            look.fg(palette.heading).add_modifier(Modifier::BOLD),
        ))
        .style(look.text());
    let inner = block.inner(area);
    block.render(area, buf);
    inner
}

fn hue(h: f32) -> Rgb {
    let f = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        (255.0 * (1.0 - k.min(4.0 - k).clamp(0.0, 1.0))) as u8
    };
    (f(5.0), f(3.0), f(1.0))
}

/// The whole gallery at 132x43.
fn draw(look: Look) -> Buffer {
    let area = Rect::new(0, 0, 132, 43);
    let mut buf = Buffer::empty(area);
    Block::default()
        .style(
            look.fg(look.palette().text)
                .patch(look.bg(look.palette().surface)),
        )
        .render(area, &mut buf);
    let theme = ChartTheme::of(look);
    let paint = Paint::new(look.depth(), look.ascii());
    let calls: Vec<u64> = (0..60)
        .map(|i| {
            (40.0 + 30.0 * (f64::from(i) / 5.0).sin() + 25.0 * (f64::from(i) / 11.0).cos()) as u64
        })
        .collect();
    let series: Vec<f64> = (0..120)
        .map(|i| {
            (f64::from(i) / 9.0).sin() * 3.0
                + (f64::from(i) / 23.0).cos() * 2.0
                + f64::from(i) / 30.0
        })
        .collect();

    let a = panel(
        &mut buf,
        look,
        Rect::new(1, 1, 64, 7),
        "Calls per hour, last 60 (sparkline)",
    );
    Sparkline::new(&calls, theme).render(Rect::new(a.x + 1, a.y, a.width - 2, 4), &mut buf);
    Sparkline::new(&calls, theme)
        .tone(Tone::Warn)
        .render(Rect::new(a.x + 1, a.y + 4, a.width - 2, 1), &mut buf);

    let b = panel(
        &mut buf,
        look,
        Rect::new(1, 8, 64, 10),
        "Posts by board (bars)",
    );
    let rows = [
        ("General", 412),
        ("Retro computing", 377),
        ("Door games", 301),
        ("Ham radio", 188),
        ("Lounge", 142),
        ("Sysop notes", 61),
        ("Off topic", 23),
    ];
    BarChart::new(&rows, theme).render(Rect::new(b.x + 1, b.y, b.width - 2, b.height), &mut buf);

    let c = panel(&mut buf, look, Rect::new(1, 18, 64, 7), "Capacity (gauges)");
    Gauge::new(0.34, theme)
        .label("Disk")
        .tone(Tone::for_ratio(0.34, 0.7, 0.9))
        .render(Rect::new(c.x + 1, c.y, c.width - 2, 1), &mut buf);
    Gauge::new(0.78, theme)
        .label("Memory")
        .tone(Tone::for_ratio(0.78, 0.7, 0.9))
        .render(Rect::new(c.x + 1, c.y + 1, c.width - 2, 1), &mut buf);
    Gauge::new(0.96, theme)
        .label("Queue")
        .tone(Tone::for_ratio(0.96, 0.7, 0.9))
        .render(Rect::new(c.x + 1, c.y + 2, c.width - 2, 1), &mut buf);
    Gauge::new(0.5, theme)
        .label("Calls today")
        .render(Rect::new(c.x + 1, c.y + 3, c.width - 2, 1), &mut buf);

    let d = panel(
        &mut buf,
        look,
        Rect::new(67, 1, 64, 12),
        "Activity (braille line)",
    );
    LineChart::new(&series, theme).render(d, &mut buf);
    let e = panel(
        &mut buf,
        look,
        Rect::new(67, 13, 64, 12),
        "Activity (braille area)",
    );
    LineChart::new(&series, theme).filled().render(e, &mut buf);

    let f = panel(
        &mut buf,
        look,
        Rect::new(1, 25, 64, 17),
        "Canvases (half-block and braille)",
    );
    let mut hb = HalfBlock::new(30, 7);
    for y in 0..14_u16 {
        for x in 0..30_u16 {
            hb.set(
                x,
                y,
                mix(hue(f32::from(x) / 30.0), (10, 10, 30), f32::from(y) / 18.0),
            );
        }
    }
    hb.render(&mut buf, Rect::new(f.x + 1, f.y, 30, 7), paint);
    let mut br = Braille::new(30, 7);
    for i in 0..400_u32 {
        let t = f64::from(i) / 400.0 * std::f64::consts::TAU;
        let (x, y) = (29.5 + 28.0 * (3.0 * t).sin(), 13.5 + 12.5 * (2.0 * t).cos());
        br.set(x as i32, y as i32, hue(t as f32 / 6.3));
    }
    br.render(&mut buf, Rect::new(f.x + 32, f.y, 30, 7), paint);
    let img = decode_png(&logo_png(), &Limits::default(), (14, 18, 36)).unwrap();
    let halves = img.to_half_blocks(28, 7);
    halves.render(&mut buf, Rect::new(f.x + 1, f.y + 8, 28, 7), paint);
    let dots = img.to_braille(28, 7);
    dots.render(&mut buf, Rect::new(f.x + 32, f.y + 8, 28, 7), paint);

    let g = panel(
        &mut buf,
        look,
        Rect::new(67, 25, 64, 17),
        "A logo as cells (PNG, 96x96, sysop upload)",
    );
    let big = img.to_half_blocks(g.width - 2, g.height);
    big.render(
        &mut buf,
        Rect::new(g.x + 1, g.y, g.width - 2, g.height),
        paint,
    );
    buf
}

#[test]
fn the_gallery_draws_in_every_tier_and_depth() {
    for style in ["default", "neon", "phosphor", "deepspace"] {
        for depth in ColorDepth::ALL {
            for ascii in [false, true] {
                let buf = draw(look(depth, ascii, style));
                let cells = &buf.content;
                assert!(cells.iter().any(|c| c.symbol() != " "));
                for cell in cells {
                    assert!(
                        !cell.modifier.contains(Modifier::REVERSED),
                        "no reserved style"
                    );
                    if ascii {
                        assert!(
                            cell.symbol()
                                .chars()
                                .all(|c| c.is_ascii() || "─│┌┐└┘".contains(c)),
                            "{style} {depth:?}: {:?}",
                            cell.symbol()
                        );
                    }
                }
            }
        }
    }
}

fn dump_color(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(i) => {
            let (r, g, b) = indexed_rgb(i);
            format!("#{r:02x}{g:02x}{b:02x}")
        }
        Color::Reset => String::new(),
        named => format!("{named:?}"),
    }
}

#[test]
#[ignore = "writes the gallery pictures on demand: set UI8_GALLERY_OUT"]
fn gallery_dump() {
    let out = std::env::var("UI8_GALLERY_OUT").expect("UI8_GALLERY_OUT");
    std::fs::create_dir_all(&out).unwrap();
    for (name, depth, ascii, style) in [
        (
            "charts-132x43-truecolor-default",
            ColorDepth::TrueColor,
            false,
            "default",
        ),
        (
            "charts-132x43-truecolor-neon",
            ColorDepth::TrueColor,
            false,
            "neon",
        ),
        (
            "charts-132x43-256-phosphor",
            ColorDepth::Ansi256,
            false,
            "phosphor",
        ),
        ("charts-132x43-16", ColorDepth::Ansi16, false, "default"),
        ("charts-132x43-ascii", ColorDepth::Mono, true, "default"),
    ] {
        let buf = draw(look(depth, ascii, style));
        let mut text = String::new();
        for y in 0..43 {
            for x in 0..132 {
                let cell = &buf[(x, y)];
                let style: Style = cell.style();
                let mut flags = String::new();
                if style.add_modifier.contains(Modifier::BOLD) {
                    flags.push('b');
                }
                text.push_str(&format!(
                    "{}\t{}\t{}\t{flags}\n",
                    if cell.symbol().is_empty() {
                        " "
                    } else {
                        cell.symbol()
                    },
                    dump_color(cell.fg),
                    dump_color(cell.bg)
                ));
            }
            text.push_str("|\n");
        }
        std::fs::write(format!("{out}/{name}.cells"), text).unwrap();
    }
}

/// Intro frames at chosen moments over a login-like static screen, for tuning the look.
#[test]
#[ignore = "writes intro pictures on demand: set UI8_GALLERY_OUT"]
fn intro_dump() {
    use crate::ui::fx::{Kind, Overlay};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let out = std::env::var("UI8_GALLERY_OUT").expect("UI8_GALLERY_OUT");
    std::fs::create_dir_all(&out).unwrap();
    let ui = Ui::standard().unwrap();
    for (w, h) in [(132_u16, 43_u16), (80, 24)] {
        for style in ["default", "neon", "deepspace"] {
            let settings = TerminalSettings::default()
                .with_tier(Tier::Unicode)
                .with_theme(russh_bbs_core::terminal_settings::ThemeName::new(style).unwrap());
            let look = ui.look(&settings, Rect::new(0, 0, w, h), ColorDepth::TrueColor);
            for (i, t) in [0.05_f32, 0.2, 0.35, 0.5, 0.65, 0.8, 0.93]
                .into_iter()
                .enumerate()
            {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                let done = terminal
                    .draw(|frame| {
                        Block::default()
                            .style(look.text())
                            .borders(Borders::ALL)
                            .title(" Signed in ")
                            .render(Rect::new(w / 2 - 38, 2, 76, h - 6), frame.buffer_mut());
                        Overlay {
                            kind: Kind::Intro,
                            t,
                            look,
                            title: "russh-bbs",
                        }
                        .apply(frame);
                    })
                    .unwrap();
                let mut text = String::new();
                for y in 0..h {
                    for x in 0..w {
                        let cell = &done.buffer[(x, y)];
                        text.push_str(&format!(
                            "{}\t{}\t{}\t\n",
                            cell.symbol(),
                            dump_color(cell.fg),
                            dump_color(cell.bg)
                        ));
                    }
                    text.push_str("|\n");
                }
                std::fs::write(format!("{out}/intro-{w}x{h}-{style}-{i}.cells"), text).unwrap();
            }
        }
    }
}
