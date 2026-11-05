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

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::{ImageError, Limits, decode_png};
use crate::ui::canvas::Paint;
use crate::ui::depth::ColorDepth;

const BG: (u8, u8, u8) = (10, 20, 30);

/// Encode a picture at test time (no fixture files, nothing downloaded).
fn encode(w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(color);
    encoder.set_depth(depth);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(data)
        .unwrap();
    out
}

fn rgb_image(w: u32, h: u32, pixel: impl Fn(u32, u32) -> (u8, u8, u8)) -> Vec<u8> {
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let (r, g, b) = pixel(x, y);
            data.extend([r, g, b]);
        }
    }
    encode(w, h, png::ColorType::Rgb, png::BitDepth::Eight, &data)
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(kind: [u8; 4], data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend((data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend(data);
    out.extend(&body);
    out.extend(crc32(&body).to_be_bytes());
    out
}

/// A PNG made by hand: a header with any claimed size (valid CRC), then `idat` as stored zlib.
fn handmade(w: u32, h: u32, interlace: u8, raw: Option<&[u8]>) -> Vec<u8> {
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 0, 0, 0, interlace]); // 8-bit grey
    out.extend(chunk(*b"IHDR", &ihdr));
    if let Some(raw) = raw {
        // zlib header, one stored block, adler32.
        let mut z = vec![0x78, 0x01, 0x01];
        z.extend((raw.len() as u16).to_le_bytes());
        z.extend((!(raw.len() as u16)).to_le_bytes());
        z.extend(raw);
        let (mut a, mut b) = (1_u32, 0_u32);
        for byte in raw {
            a = (a + u32::from(*byte)) % 65_521;
            b = (b + a) % 65_521;
        }
        z.extend(((b << 16) | a).to_be_bytes());
        out.extend(chunk(*b"IDAT", &z));
        out.extend(chunk(*b"IEND", &[]));
    }
    out
}

#[test]
fn decodes_rgb_grey_alpha_palette_and_sixteen_bit() {
    let limits = Limits::default();
    let rgb = rgb_image(3, 2, |x, y| (x as u8 * 100, y as u8 * 100, 7));
    let img = decode_png(&rgb, &limits, BG).unwrap();
    assert_eq!((img.width(), img.height()), (3, 2));
    assert_eq!(img.pixels[0], (0, 0, 7));
    assert_eq!(img.pixels[5], (200, 100, 7));

    let grey = encode(
        2,
        1,
        png::ColorType::Grayscale,
        png::BitDepth::Eight,
        &[0, 255],
    );
    let img = decode_png(&grey, &limits, BG).unwrap();
    assert_eq!(img.pixels, [(0, 0, 0), (255, 255, 255)]);

    // RGBA: opaque stays, transparent shows the background, half mixes.
    let rgba = encode(
        3,
        1,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &[200, 0, 0, 255, 200, 0, 0, 0, 200, 0, 0, 128],
    );
    let img = decode_png(&rgba, &limits, BG).unwrap();
    assert_eq!(img.pixels[0], (200, 0, 0));
    assert_eq!(img.pixels[1], BG);
    let half = img.pixels[2];
    assert!(half.0 > BG.0 && half.0 < 200, "{half:?}");

    // Palette.
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, 2, 1);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_palette(vec![1, 2, 3, 250, 251, 252]);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&[1, 0])
        .unwrap();
    let img = decode_png(&out, &limits, BG).unwrap();
    assert_eq!(img.pixels, [(250, 251, 252), (1, 2, 3)]);

    // 16-bit grey is stripped to 8.
    let deep = encode(
        1,
        1,
        png::ColorType::Grayscale,
        png::BitDepth::Sixteen,
        &[0xFF, 0xFF],
    );
    assert_eq!(
        decode_png(&deep, &limits, BG).unwrap().pixels,
        [(255, 255, 255)]
    );
}

#[test]
fn an_adam7_interlaced_picture_decodes() {
    // One pixel: all of Adam7's first pass, with a filter byte in front.
    let png = handmade(1, 1, 1, Some(&[0, 77]));
    let img = decode_png(&png, &Limits::default(), BG).unwrap();
    assert_eq!(img.pixels, [(77, 77, 77)]);
}

#[test]
fn half_blocks_keep_the_shape_and_fit_the_box() {
    // 8 wide, 4 tall: in a 4x4-cell box (4 wide, 8 pixel rows) it scales to 4x2 pixels = 4x1 cells.
    let img = decode_png(&rgb_image(8, 4, |_, _| (9, 9, 9)), &Limits::default(), BG).unwrap();
    let canvas = img.to_half_blocks(4, 4);
    assert_eq!(canvas.cells(), (4, 1));
    // A square picture in a wide box is limited by the height.
    let img = decode_png(&rgb_image(4, 4, |_, _| (9, 9, 9)), &Limits::default(), BG).unwrap();
    let canvas = img.to_half_blocks(40, 3);
    assert_eq!(canvas.cells(), (6, 3));
    // A 1x1 picture still gives one cell.
    let one = decode_png(&rgb_image(1, 1, |_, _| (1, 2, 3)), &Limits::default(), BG).unwrap();
    assert_eq!(one.to_half_blocks(0, 0).cells(), (1, 1));
}

#[test]
fn half_block_golden_in_each_depth_and_the_ascii_tier() {
    // Top half red, bottom half blue.
    let img = decode_png(
        &rgb_image(2, 2, |_, y| if y == 0 { (255, 0, 0) } else { (0, 0, 255) }),
        &Limits::default(),
        BG,
    )
    .unwrap();
    let canvas = img.to_half_blocks(2, 1);
    assert_eq!(canvas.cells(), (2, 1));
    let area = Rect::new(0, 0, 2, 1);
    let draw = |paint: Paint| {
        let mut buf = Buffer::empty(area);
        canvas.render(&mut buf, area, paint);
        buf
    };
    let tc = draw(Paint::new(ColorDepth::TrueColor, false));
    assert_eq!(
        (tc[(0, 0)].symbol(), tc[(0, 0)].fg, tc[(0, 0)].bg),
        ("▀", Color::Rgb(255, 0, 0), Color::Rgb(0, 0, 255))
    );
    let c256 = draw(Paint::new(ColorDepth::Ansi256, false));
    assert_eq!(
        (c256[(0, 0)].fg, c256[(0, 0)].bg),
        (Color::Indexed(196), Color::Indexed(21))
    );
    let c16 = draw(Paint::new(ColorDepth::Ansi16, false));
    assert_eq!(
        (c16[(0, 0)].fg, c16[(0, 0)].bg),
        (Color::LightRed, Color::Blue)
    );
    let mono = draw(Paint::new(ColorDepth::Mono, false));
    assert_eq!(mono[(0, 0)].fg, Color::Reset);
    let ascii = draw(Paint::new(ColorDepth::TrueColor, true));
    assert!(ascii[(0, 0)].symbol().is_ascii());
}

#[test]
fn braille_keeps_an_edge() {
    // Left half white, right half black.
    let img = decode_png(
        &rgb_image(8, 8, |x, _| if x < 4 { (255, 255, 255) } else { (0, 0, 0) }),
        &Limits::default(),
        BG,
    )
    .unwrap();
    let canvas = img.to_braille(4, 2);
    let area = Rect::new(0, 0, 4, 2);
    let mut buf = Buffer::empty(area);
    canvas.render(&mut buf, area, Paint::new(ColorDepth::TrueColor, false));
    assert_eq!(buf[(0, 0)].symbol(), "⣿", "bright side lit");
    assert_eq!(buf[(1, 0)].symbol(), "⣿");
    assert_eq!(buf[(3, 0)].symbol(), " ", "a flat dark cell stays empty");
}

#[test]
fn only_cells_come_out() {
    // Whatever the picture, every symbol drawn is a block, shade, braille or ASCII glyph:
    // no escape sequence, no control character, no marker glyph or delimiter.
    let img = decode_png(
        &rgb_image(33, 17, |x, y| {
            ((x * 7) as u8, (y * 13) as u8, (x * y) as u8)
        }),
        &Limits::default(),
        BG,
    )
    .unwrap();
    for ascii in [false, true] {
        for depth in ColorDepth::ALL {
            let area = Rect::new(0, 0, 40, 12);
            for braille in [false, true] {
                let mut buf = Buffer::empty(area);
                let paint = Paint::new(depth, ascii);
                if braille {
                    img.to_braille(40, 12).render(&mut buf, area, paint);
                } else {
                    img.to_half_blocks(40, 12).render(&mut buf, area, paint);
                }
                for cell in &buf.content {
                    for ch in cell.symbol().chars() {
                        let ok = ch == ' '
                            || ch.is_ascii_graphic()
                            || matches!(ch, '▀' | '▄' | '█' | '░' | '▒' | '▓')
                            || ('\u{2801}'..='\u{28FF}').contains(&ch);
                        assert!(ok, "{ch:?} ({depth:?}, ascii {ascii}, braille {braille})");
                        if ascii {
                            assert!(ch.is_ascii(), "ASCII tier drew {ch:?}");
                        }
                    }
                }
            }
        }
    }
}

// Hostile files: every one is an error, none panics, none allocates the claimed picture.

#[test]
fn a_file_over_the_byte_cap_is_refused() {
    let png = rgb_image(2, 2, |_, _| (1, 1, 1));
    let tight = Limits {
        max_bytes: png.len() - 1,
        ..Limits::default()
    };
    assert!(matches!(
        decode_png(&png, &tight, BG),
        Err(ImageError::TooManyBytes { .. })
    ));
    let exact = Limits {
        max_bytes: png.len(),
        ..Limits::default()
    };
    assert!(decode_png(&png, &exact, BG).is_ok());
}

#[test]
fn things_that_are_not_pngs_are_refused() {
    let limits = Limits::default();
    for junk in [
        &b""[..],
        b"\x89PNG",
        b"GIF89a....................",
        b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01\x01\x00\x00\x01",
        b"<svg onload=alert(1)></svg>",
        b"\x1b]0;title\x07\x1b[2J",
    ] {
        assert_eq!(
            decode_png(junk, &limits, BG),
            Err(ImageError::NotPng),
            "{junk:?}"
        );
    }
}

#[test]
fn a_header_claiming_a_huge_picture_is_refused_before_any_pixels_exist() {
    // Valid chunks, claims 100000x100000 (10 gigapixels), carries no pixel data at all.
    let started = Instant::now();
    let png = handmade(100_000, 100_000, 0, None);
    assert!(matches!(
        decode_png(&png, &Limits::default(), BG),
        Err(ImageError::TooLarge { .. })
    ));
    // The largest header a PNG can claim.
    let png = handmade(0x7FFF_FFFF, 0x7FFF_FFFF, 0, None);
    assert!(matches!(
        decode_png(&png, &Limits::default(), BG),
        Err(ImageError::TooLarge { .. })
    ));
    // A zero side.
    let png = handmade(0, 5, 0, None);
    assert!(matches!(
        decode_png(&png, &Limits::default(), BG),
        Err(ImageError::TooLarge { .. } | ImageError::Invalid(_))
    ));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn too_many_pixels_with_each_side_in_range_is_refused() {
    let png = handmade(1024, 1024, 0, None);
    let limits = Limits {
        max_pixels: 1_000_000,
        ..Limits::default()
    };
    assert!(matches!(
        decode_png(&png, &limits, BG),
        Err(ImageError::TooManyPixels { .. })
    ));
}

#[test]
fn a_decompression_bomb_is_refused() {
    // 4000x4000 grey zeros: 16 MB of pixels in a file of about 16 KB. Written a row at a time so
    // building the fixture does not itself take 16 MB.
    use std::io::Write;
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 4000, 4000);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        let mut stream = writer.stream_writer().unwrap();
        let row = vec![0_u8; 4000];
        for _ in 0..4000 {
            stream.write_all(&row).unwrap();
        }
        stream.finish().unwrap();
    }
    assert!(
        out.len() < Limits::default().max_bytes,
        "{} bytes",
        out.len()
    );
    assert!(matches!(
        decode_png(&out, &Limits::default(), BG),
        Err(ImageError::TooLarge { .. })
    ));
    // Inside the dimension cap but with a tighter pixel cap, the same result.
    let small = Limits {
        max_pixels: 100,
        ..Limits::default()
    };
    let png = rgb_image(64, 64, |_, _| (0, 0, 0));
    assert!(matches!(
        decode_png(&png, &small, BG),
        Err(ImageError::TooManyPixels { .. })
    ));
}

#[test]
fn a_truncated_or_corrupt_picture_is_an_error_not_a_panic() {
    let limits = Limits::default();
    let png = rgb_image(40, 40, |x, y| ((x * 6) as u8, (y * 6) as u8, 99));
    // Cut anywhere in the file: never a partial picture. Either an error, or (when only the
    // trailing end marker is missing) the complete picture.
    for cut in 8..png.len() {
        match decode_png(&png[..cut], &limits, BG) {
            Err(_) => {}
            Ok(img) => assert_eq!(
                img.pixels.len(),
                40 * 40,
                "cut at {cut} gave a partial picture"
            ),
        }
    }
    for cut in [9, 20, 33, png.len() / 2] {
        assert!(
            decode_png(&png[..cut], &limits, BG).is_err(),
            "cut at {cut}"
        );
    }
    // Flip a byte in the middle of the pixel data and in the header.
    for at in [png.len() / 2, 20] {
        let mut bad = png.clone();
        bad[at] ^= 0xFF;
        assert!(decode_png(&bad, &limits, BG).is_err(), "flip at {at}");
    }
    // A header whose pixel data is missing.
    let empty = handmade(10, 10, 0, Some(&[]));
    assert!(matches!(
        decode_png(&empty, &limits, BG),
        Err(ImageError::Invalid(_))
    ));
}

#[test]
fn a_decode_over_the_time_cap_is_refused() {
    let png = rgb_image(64, 64, |x, y| (x as u8, y as u8, 0));
    let none = Limits {
        max_time: Duration::ZERO,
        ..Limits::default()
    };
    assert_eq!(decode_png(&png, &none, BG), Err(ImageError::Timeout));
}
