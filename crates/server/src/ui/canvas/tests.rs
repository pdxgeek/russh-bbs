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
use ratatui::style::Color;

use super::{Braille, HalfBlock, Paint, brightness, mix, quantize, rgb_of, scale};
use crate::ui::depth::ColorDepth;

const RED: (u8, u8, u8) = (255, 0, 0);
const BLUE: (u8, u8, u8) = (0, 0, 255);
const WHITE: (u8, u8, u8) = (255, 255, 255);

fn symbols(buf: &Buffer, area: Rect) -> Vec<String> {
    (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect()
        })
        .collect()
}

#[test]
fn half_block_golden_at_every_depth() {
    // Two cells: (red over blue) and (white over white).
    let mut canvas = HalfBlock::new(2, 1);
    canvas.set(0, 0, RED);
    canvas.set(0, 1, BLUE);
    canvas.set(1, 0, WHITE);
    canvas.set(1, 1, WHITE);
    let area = Rect::new(0, 0, 2, 1);
    for (depth, red, blue, white) in [
        (
            ColorDepth::TrueColor,
            Color::Rgb(255, 0, 0),
            Color::Rgb(0, 0, 255),
            Color::Rgb(255, 255, 255),
        ),
        (
            ColorDepth::Ansi256,
            Color::Indexed(196),
            Color::Indexed(21),
            Color::Indexed(231),
        ),
        (
            ColorDepth::Ansi16,
            Color::LightRed,
            Color::Blue,
            Color::White,
        ),
    ] {
        let mut buf = Buffer::empty(area);
        canvas.render(&mut buf, area, Paint::new(depth, false));
        assert_eq!(symbols(&buf, area), ["▀█"], "{depth:?}");
        assert_eq!(buf[(0, 0)].fg, red, "{depth:?} top");
        assert_eq!(buf[(0, 0)].bg, blue, "{depth:?} bottom");
        assert_eq!(buf[(1, 0)].fg, white, "{depth:?} solid");
    }
}

#[test]
fn half_block_in_mono_and_ascii_uses_a_brightness_ramp_and_no_colour() {
    let mut canvas = HalfBlock::new(4, 1);
    for (x, level) in [0_u8, 85, 170, 255].into_iter().enumerate() {
        canvas.set(x as u16, 0, (level, level, level));
        canvas.set(x as u16, 1, (level, level, level));
    }
    let area = Rect::new(0, 0, 4, 1);
    let mut mono = Buffer::empty(area);
    canvas.render(&mut mono, area, Paint::new(ColorDepth::Mono, false));
    assert_eq!(symbols(&mono, area), [" ░▓█"]);
    let mut ascii = Buffer::empty(area);
    canvas.render(&mut ascii, area, Paint::new(ColorDepth::TrueColor, true));
    assert_eq!(symbols(&ascii, area), [" -*@"]);
    for buf in [&mono, &ascii] {
        for x in 0..4 {
            assert_eq!(buf[(x, 0)].fg, Color::Reset);
            assert_eq!(buf[(x, 0)].bg, Color::Reset);
        }
    }
}

#[test]
fn half_block_one_sided_pixels_use_the_matching_half() {
    let mut canvas = HalfBlock::new(2, 1);
    canvas.set(0, 0, RED);
    canvas.set(1, 1, BLUE);
    let area = Rect::new(0, 0, 2, 1);
    let mut buf = Buffer::empty(area);
    canvas.render(&mut buf, area, Paint::new(ColorDepth::TrueColor, false));
    assert_eq!(symbols(&buf, area), ["▀▄"]);
}

#[test]
fn a_cell_with_no_pixel_is_left_alone() {
    let mut canvas = HalfBlock::new(3, 1);
    canvas.set(1, 0, RED);
    let area = Rect::new(0, 0, 3, 1);
    let mut buf = Buffer::empty(area);
    buf[(0, 0)].set_symbol("a");
    buf[(2, 0)].set_symbol("b");
    canvas.render(&mut buf, area, Paint::new(ColorDepth::TrueColor, false));
    assert_eq!(symbols(&buf, area), ["a▀b"]);
}

#[test]
fn drawing_is_clipped_to_the_area_and_the_buffer() {
    let mut canvas = HalfBlock::new(10, 10);
    canvas.fill(0, 0, 10, 20, RED);
    let buffer = Rect::new(0, 0, 4, 3);
    let mut buf = Buffer::empty(buffer);
    // An area that sticks out of the buffer must not panic or write outside it.
    canvas.render(
        &mut buf,
        Rect::new(2, 1, 50, 50),
        Paint::new(ColorDepth::TrueColor, false),
    );
    assert_eq!(buf[(1, 1)].symbol(), " ");
    assert_eq!(buf[(2, 1)].symbol(), "█");
    assert_eq!(buf[(3, 2)].symbol(), "█");
    // Pixels outside the canvas are ignored.
    canvas.set(60_000, 60_000, RED);
    canvas.set_i(-4, -4, RED);
    assert_eq!(canvas.get(60_000, 60_000), None);
}

#[test]
fn braille_dots_map_to_the_unicode_block() {
    let mut canvas = Braille::new(1, 1);
    // One dot each in the eight positions, in turn.
    let expected = [
        ((0, 0), 0x2801),
        ((0, 1), 0x2802),
        ((0, 2), 0x2804),
        ((1, 0), 0x2808),
        ((1, 1), 0x2810),
        ((1, 2), 0x2820),
        ((0, 3), 0x2840),
        ((1, 3), 0x2880),
    ];
    for ((x, y), code) in expected {
        let mut one = Braille::new(1, 1);
        one.set(x, y, RED);
        let area = Rect::new(0, 0, 1, 1);
        let mut buf = Buffer::empty(area);
        one.render(&mut buf, area, Paint::new(ColorDepth::TrueColor, false));
        assert_eq!(
            buf[(0, 0)].symbol().chars().next().unwrap() as u32,
            code,
            "dot {x},{y}"
        );
    }
    canvas.set(0, 0, RED);
    canvas.set(1, 3, RED);
    assert!(canvas.is_set(0, 0) && canvas.is_set(1, 3) && !canvas.is_set(1, 0));
}

#[test]
fn braille_golden_line_in_each_tier() {
    // A diagonal across a 4x1 cell canvas (8x4 dots).
    let mut canvas = Braille::new(4, 1);
    canvas.line((0, 3), (7, 0), RED);
    let area = Rect::new(0, 0, 4, 1);

    let mut unicode = Buffer::empty(area);
    canvas.render(&mut unicode, area, Paint::new(ColorDepth::TrueColor, false));
    assert_eq!(symbols(&unicode, area), ["⣀⠤⠒⠉"]);
    assert_eq!(unicode[(0, 0)].fg, Color::Rgb(255, 0, 0));

    let mut c256 = Buffer::empty(area);
    canvas.render(&mut c256, area, Paint::new(ColorDepth::Ansi256, false));
    assert_eq!(c256[(0, 0)].fg, Color::Indexed(196));

    let mut mono = Buffer::empty(area);
    canvas.render(&mut mono, area, Paint::new(ColorDepth::Mono, false));
    assert_eq!(symbols(&mono, area), ["⣀⠤⠒⠉"]);
    assert_eq!(mono[(0, 0)].fg, Color::Reset);

    let mut ascii = Buffer::empty(area);
    canvas.render(&mut ascii, area, Paint::new(ColorDepth::TrueColor, true));
    assert_eq!(symbols(&ascii, area), ["....".to_owned()]);
    for x in 0..4 {
        assert!(ascii[(x, 0)].symbol().is_ascii(), "ASCII tier is ASCII");
    }
}

#[test]
fn braille_ascii_density_rises_with_dots_set() {
    let area = Rect::new(0, 0, 4, 1);
    let mut canvas = Braille::new(4, 1);
    // Cell 0: 1 dot; cell 1: 3 dots; cell 2: 5 dots; cell 3: all 8.
    let dots = [(0, vec![(0, 0)]), (1, vec![(0, 0), (1, 0), (0, 1)])];
    for (cell, list) in dots {
        for (x, y) in list {
            canvas.set(cell * 2 + x, y, RED);
        }
    }
    for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1), (0, 2)] {
        canvas.set(4 + x, y, RED);
    }
    for x in 0..2 {
        for y in 0..4 {
            canvas.set(6 + x, y, RED);
        }
    }
    let mut ascii = Buffer::empty(area);
    canvas.render(&mut ascii, area, Paint::new(ColorDepth::Mono, true));
    assert_eq!(symbols(&ascii, area), [".:+#"]);
}

#[test]
fn braille_fill_down_makes_an_area() {
    let mut canvas = Braille::new(1, 2);
    canvas.fill_down(0, 2, RED);
    assert!(!canvas.is_set(0, 1));
    assert!(canvas.is_set(0, 2) && canvas.is_set(0, 7));
    // A hostile line far outside the canvas terminates and draws nothing.
    canvas.line((-1_000_000, -1_000_000), (1_000_000, 1_000_000), RED);
}

#[test]
fn colour_maths() {
    assert_eq!(mix(RED, BLUE, 0.0), RED);
    assert_eq!(mix(RED, BLUE, 1.0), BLUE);
    assert_eq!(mix(RED, BLUE, 2.0), BLUE, "t is clamped");
    assert_eq!(scale(WHITE, 0.0), (0, 0, 0));
    assert!(brightness(WHITE) > 0.99 && brightness((0, 0, 0)) < 0.01);
    assert_eq!(quantize(RED, ColorDepth::Mono), None);
    assert_eq!(rgb_of(Color::Rgb(1, 2, 3)), Some((1, 2, 3)));
    assert_eq!(rgb_of(Color::Reset), None);
    assert_eq!(rgb_of(Color::Indexed(196)), Some((255, 0, 0)));
    // Greys use the grey ramp, not the cube.
    assert!(matches!(
        quantize((128, 128, 128), ColorDepth::Ansi256),
        Some(Color::Indexed(i)) if i >= 232
    ));
}
