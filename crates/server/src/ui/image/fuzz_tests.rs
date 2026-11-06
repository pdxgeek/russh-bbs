//! H-4: the PNG decoder under hostile input. No panic, the byte, side, pixel and time caps
//! hold, and what decodes is within the caps.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::time::{Duration, Instant};

use proptest::prelude::*;

use super::{Limits, decode_png};
use crate::fuzz::reason_is_loggable;

const BG: (u8, u8, u8) = (10, 20, 30);

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

fn encode(w: u32, h: u32, color: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(color);
    encoder.set_depth(depth);
    if color == png::ColorType::Indexed {
        encoder.set_palette(vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
    }
    encoder
        .write_header()
        .unwrap()
        .write_image_data(data)
        .unwrap();
    out
}

/// A handful of valid pictures of different shapes to mutate.
fn samples() -> Vec<Vec<u8>> {
    let rgb = |n: usize| (0..n).map(|i| (i * 7 % 256) as u8).collect::<Vec<_>>();
    vec![
        encode(
            8,
            8,
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            &rgb(8 * 8 * 3),
        ),
        encode(
            8,
            8,
            png::ColorType::Rgba,
            png::BitDepth::Eight,
            &rgb(8 * 8 * 4),
        ),
        encode(
            8,
            8,
            png::ColorType::Grayscale,
            png::BitDepth::One,
            &[0xAA; 8],
        ),
        encode(
            8,
            8,
            png::ColorType::Grayscale,
            png::BitDepth::Sixteen,
            &rgb(8 * 8 * 2),
        ),
        encode(
            8,
            8,
            png::ColorType::Indexed,
            png::BitDepth::Two,
            &[0b0001_1011; 16],
        ),
        encode(
            1,
            1,
            png::ColorType::GrayscaleAlpha,
            png::BitDepth::Eight,
            &[9, 200],
        ),
    ]
}

/// Set a chunk-aware field with a valid CRC, so the decoder reaches the field.
fn fix_ihdr_crc(bytes: &mut [u8]) {
    if bytes.len() >= 33 {
        let crc = crc32(&bytes[12..29]);
        bytes[29..33].copy_from_slice(&crc.to_be_bytes());
    }
}

fn apply(bytes: &mut Vec<u8>, (kind, a, b): (u8, u32, u32)) {
    let len = bytes.len().max(1);
    let at = a as usize % len;
    match kind % 9 {
        0 => bytes.truncate(at),
        1 => {
            if let Some(slot) = bytes.get_mut(at) {
                *slot ^= 1 << (b % 8);
            }
        }
        2 => {
            let end = (at + b as usize % 64).min(bytes.len());
            bytes.drain(at.min(end)..end);
        }
        3 => {
            let junk: Vec<u8> = (0..b % 64).map(|i| (a >> (i % 24)) as u8).collect();
            let pos = at.min(bytes.len());
            bytes.splice(pos..pos, junk);
        }
        // Change the claimed size, with the header's CRC made right again.
        4 | 5 => {
            if bytes.len() >= 33 {
                let value = [
                    0,
                    1,
                    2,
                    255,
                    1024,
                    1025,
                    65_535,
                    1 << 24,
                    u32::MAX,
                    i32::MAX as u32,
                ][b as usize % 10];
                let field = if kind % 9 == 4 { 16 } else { 20 };
                bytes[field..field + 4].copy_from_slice(&value.to_be_bytes());
                fix_ihdr_crc(bytes);
            }
        }
        // Change bit depth, colour type, interlace.
        6 => {
            if bytes.len() >= 33 {
                bytes[24] = (b % 20) as u8;
                bytes[25] = (a % 9) as u8;
                bytes[28] = (b % 4) as u8;
                fix_ihdr_crc(bytes);
            }
        }
        // A huge chunk length claim somewhere.
        7 => {
            if bytes.len() >= at + 4 {
                bytes[at..at + 4].copy_from_slice(&[0x7f, 0xff, 0xff, (b % 256) as u8]);
            }
        }
        // Repeat the data many times (a bigger body than the header says).
        _ => {
            let from = at.min(bytes.len());
            let span: Vec<u8> = bytes[from..].iter().copied().take(64).collect();
            for _ in 0..(b % 400) {
                bytes.extend(&span);
            }
        }
    }
}

fn check(bytes: &[u8], limits: &Limits) {
    let started = Instant::now();
    let result = decode_png(bytes, limits, BG);
    // The decoder's own cap is 250 ms; allow slack for a loaded debug build.
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "decode took {:?}",
        started.elapsed()
    );
    match result {
        Ok(raster) => {
            assert!(bytes.len() <= limits.max_bytes);
            assert!(raster.width() > 0 && raster.width() <= limits.max_side);
            assert!(raster.height() > 0 && raster.height() <= limits.max_side);
            assert!(u64::from(raster.width()) * u64::from(raster.height()) <= limits.max_pixels);
        }
        Err(error) => {
            if let Err(why) = reason_is_loggable(&error.to_string()) {
                panic!("{why}");
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn mutated_pictures_never_panic_and_keep_the_caps(
        which in 0_usize..6,
        ops in prop::collection::vec((any::<u8>(), any::<u32>(), any::<u32>()), 1..5),
    ) {
        let mut bytes = samples().swap_remove(which);
        for op in ops {
            apply(&mut bytes, op);
        }
        check(&bytes, &Limits::default());
    }

    #[test]
    fn random_bytes_after_the_signature_never_panic(tail in prop::collection::vec(any::<u8>(), 0..2048)) {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend(tail);
        check(&bytes, &Limits::default());
    }

    #[test]
    fn random_bytes_never_panic(bytes in prop::collection::vec(any::<u8>(), 0..2048)) {
        check(&bytes, &Limits::default());
    }

    /// Tight limits are honoured whatever the picture.
    #[test]
    fn small_caps_hold(
        which in 0_usize..6,
        side in 1_u32..16,
        ops in prop::collection::vec((any::<u8>(), any::<u32>(), any::<u32>()), 0..3),
    ) {
        let limits = Limits {
            max_bytes: 400,
            max_side: side,
            max_pixels: u64::from(side) * 4,
            max_time: Duration::from_millis(50),
        };
        let mut bytes = samples().swap_remove(which);
        for op in ops {
            apply(&mut bytes, op);
        }
        check(&bytes, &limits);
    }
}

/// A valid picture decodes (so the properties above are not all refusals).
#[test]
fn the_samples_decode() {
    for (n, bytes) in samples().iter().enumerate() {
        decode_png(bytes, &Limits::default(), BG).unwrap_or_else(|e| panic!("sample {n}: {e}"));
    }
}
