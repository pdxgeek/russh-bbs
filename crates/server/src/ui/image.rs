//! Image to cells (UI-8): a sysop's logo or a member's avatar turned into half-block or braille
//! cells at the member's colour depth.
//!
//! **Sysop-uploaded files only.** Nothing here fetches anything: it takes bytes the caller
//! already holds from a sysop-controlled place (the data directory, an upload a sysop enabled).
//! No URL is ever opened, so there is no SSRF surface. The decoder is `png`, a maintained
//! pure-Rust crate; there is no C code and no `unsafe`. Only PNG is accepted (JPEG, GIF and the
//! rest are refused with [`ImageError::NotPng`]); a sysop can convert a logo once.
//!
//! **Cells, never protocols.** The result is a [`HalfBlock`] or [`Braille`] canvas, which
//! draws styled cells. Sixel, kitty and iTerm graphics are never sent, so the sanitizer's
//! guarantees hold and the picture works over plain ssh and telnet. In the ASCII tier the
//! canvases draw a brightness ramp of ASCII characters.
//!
//! **Hostile files.** [`Limits`] caps the file size, each side, the pixel count, the memory the
//! decoder may use and the time. The header is read first and a picture over the caps is refused
//! before any pixel buffer is allocated, so a "bomb" (a tiny file that claims a huge picture or
//! inflates to one) costs a header read and an error. A truncated or malformed file is an error,
//! never a panic. The caller runs [`decode_png`] on a blocking thread: it is CPU work, bounded
//! by the caps, but it is not async.

#![allow(
    clippy::many_single_char_names,
    reason = "pixel and colour maths use x, y, w, h, r, g, b by convention"
)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "pixel coordinates are bounded by the size caps"
)]
#![allow(
    dead_code,
    reason = "image-to-cells is consumed by UI-4 and the sysop logo slot"
)]

use std::io::Cursor;
use std::time::{Duration, Instant};

use super::canvas::{Braille, HalfBlock, Rgb, brightness};

const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The caps a picture must stay inside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_field_names,
    reason = "`max_*` reads best for a set of caps"
)]
pub(crate) struct Limits {
    /// The largest file, in bytes.
    pub max_bytes: usize,
    /// The largest width or height, in pixels.
    pub max_side: u32,
    /// The most pixels (width times height).
    pub max_pixels: u64,
    /// The longest a decode may take.
    pub max_time: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 256 * 1024,
            max_side: 1024,
            max_pixels: 1_000_000,
            max_time: Duration::from_millis(250),
        }
    }
}

/// Why a picture was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ImageError {
    /// The file is over the byte cap.
    #[error("the file is {size} bytes; the limit is {max}")]
    TooManyBytes {
        /// The file's size.
        size: usize,
        /// The cap.
        max: usize,
    },
    /// Not a PNG.
    #[error("not a PNG file")]
    NotPng,
    /// A side is over the cap, or the picture is empty.
    #[error("the picture is {width} by {height} pixels; the limit is {max} on a side")]
    TooLarge {
        /// Claimed width.
        width: u32,
        /// Claimed height.
        height: u32,
        /// The cap per side.
        max: u32,
    },
    /// Too many pixels in total.
    #[error("the picture has {pixels} pixels; the limit is {max}")]
    TooManyPixels {
        /// Claimed pixel count.
        pixels: u64,
        /// The cap.
        max: u64,
    },
    /// The decode took longer than the cap.
    #[error("decoding took too long")]
    Timeout,
    /// Truncated, corrupt or unsupported.
    #[error("the picture could not be decoded: {0}")]
    Invalid(String),
}

/// A decoded picture: opaque pixels (alpha already composited over a background).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Raster {
    width: u32,
    height: u32,
    pixels: Vec<Rgb>,
}

/// Decode a PNG within `limits`, compositing transparency over `background`.
pub(crate) fn decode_png(
    bytes: &[u8],
    limits: &Limits,
    background: Rgb,
) -> Result<Raster, ImageError> {
    let started = Instant::now();
    if bytes.len() > limits.max_bytes {
        return Err(ImageError::TooManyBytes {
            size: bytes.len(),
            max: limits.max_bytes,
        });
    }
    if bytes.len() < PNG_MAGIC.len() || bytes[..8] != PNG_MAGIC {
        return Err(ImageError::NotPng);
    }
    // The caps are checked on the header's own numbers first, with no decoder involved: a PNG
    // starts with the 8-byte signature and a 13-byte IHDR chunk whose first two fields are the
    // width and height. `png` only reports them once it has seen pixel data, so a header-only
    // "bomb" must be refused here.
    check_header(bytes, limits)?;
    let invalid = |e: png::DecodingError| ImageError::Invalid(e.to_string());
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    // The decoder's own memory cap: the largest picture the pixel cap allows, at four bytes a
    // pixel, plus slack for chunks.
    let budget = usize::try_from(limits.max_pixels.saturating_mul(4)).unwrap_or(usize::MAX);
    decoder.set_limits(png::Limits {
        bytes: budget.saturating_add(64 * 1024),
    });
    let mut reader = decoder.read_info().map_err(invalid)?;
    let (width, height, interlaced) = {
        let info = reader.info();
        (info.width, info.height, info.interlaced)
    };
    // Checked again on what the decoder parsed (the two must agree before pixels are read).
    check_size(width, height, limits)?;
    let pixels = u64::from(width) * u64::from(height);
    let (color, _) = reader.output_color_type();
    let channels = match color {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => {
            return Err(ImageError::Invalid("unexpanded palette".to_owned()));
        }
    };
    let over_time = || started.elapsed() > limits.max_time;
    let mut out: Vec<Rgb> = Vec::with_capacity(pixels as usize);
    let mut take_row = |row: &[u8]| {
        for px in row.chunks_exact(channels).take(width as usize) {
            out.push(composite(px, background));
        }
    };
    if interlaced {
        // Adam7 passes arrive out of order; the whole frame is read (bounded by the caps).
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| ImageError::Invalid("too large".to_owned()))?;
        let mut frame = vec![0_u8; size];
        reader.next_frame(&mut frame).map_err(invalid)?;
        if over_time() {
            return Err(ImageError::Timeout);
        }
        for row in frame.chunks_exact(width as usize * channels) {
            take_row(row);
        }
    } else {
        let mut rows = 0_u32;
        while let Some(row) = reader.next_row().map_err(invalid)? {
            if over_time() {
                return Err(ImageError::Timeout);
            }
            take_row(row.data());
            rows += 1;
            if rows == height {
                break;
            }
        }
    }
    if out.len() as u64 != pixels {
        return Err(ImageError::Invalid("the picture is incomplete".to_owned()));
    }
    Ok(Raster {
        width,
        height,
        pixels: out,
    })
}

/// Refuse a PNG whose IHDR claims more than `limits` allow, reading only the header bytes.
fn check_header(bytes: &[u8], limits: &Limits) -> Result<(), ImageError> {
    let ihdr = bytes
        .get(8..24)
        .filter(|h| h[..4] == 13_u32.to_be_bytes() && &h[4..8] == b"IHDR")
        .ok_or_else(|| ImageError::Invalid("no image header".to_owned()))?;
    let word = |at: usize| u32::from_be_bytes([ihdr[at], ihdr[at + 1], ihdr[at + 2], ihdr[at + 3]]);
    check_size(word(8), word(12), limits)
}

fn check_size(width: u32, height: u32, limits: &Limits) -> Result<(), ImageError> {
    if width == 0 || height == 0 || width > limits.max_side || height > limits.max_side {
        return Err(ImageError::TooLarge {
            width,
            height,
            max: limits.max_side,
        });
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > limits.max_pixels {
        return Err(ImageError::TooManyPixels {
            pixels,
            max: limits.max_pixels,
        });
    }
    Ok(())
}

/// One pixel of 1 to 4 channels over `background`.
fn composite(px: &[u8], background: Rgb) -> Rgb {
    let (rgb, alpha) = match px {
        [g] => ((*g, *g, *g), 255),
        [g, a] => ((*g, *g, *g), *a),
        [r, g, b] => ((*r, *g, *b), 255),
        [r, g, b, a] => ((*r, *g, *b), *a),
        _ => ((0, 0, 0), 255),
    };
    if alpha == 255 {
        return rgb;
    }
    let a = u16::from(alpha);
    let over =
        |top: u8, under: u8| ((u16::from(top) * a + u16::from(under) * (255 - a)) / 255) as u8;
    (
        over(rgb.0, background.0),
        over(rgb.1, background.1),
        over(rgb.2, background.2),
    )
}

impl Raster {
    /// Width in pixels.
    pub(crate) const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub(crate) const fn height(&self) -> u32 {
        self.height
    }

    /// The picture resampled to `w` by `h` pixels (box filter down, nearest up).
    fn resample(&self, w: u32, h: u32) -> Vec<Rgb> {
        let mut out = Vec::with_capacity((w * h) as usize);
        for dy in 0..h {
            let y0 = dy * self.height / h;
            let y1 = ((dy + 1) * self.height / h).max(y0 + 1).min(self.height);
            for dx in 0..w {
                let x0 = dx * self.width / w;
                let x1 = ((dx + 1) * self.width / w).max(x0 + 1).min(self.width);
                let (mut r, mut g, mut b, mut n) = (0_u32, 0_u32, 0_u32, 0_u32);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let (pr, pg, pb) = self.pixels[(y * self.width + x) as usize];
                        r += u32::from(pr);
                        g += u32::from(pg);
                        b += u32::from(pb);
                        n += 1;
                    }
                }
                let n = n.max(1);
                out.push(((r / n) as u8, (g / n) as u8, (b / n) as u8));
            }
        }
        out
    }

    /// The largest size with this picture's shape inside `w` by `h` units.
    fn fit(&self, w: u32, h: u32) -> (u32, u32) {
        let scale =
            (f64::from(w) / f64::from(self.width)).min(f64::from(h) / f64::from(self.height));
        (
            ((f64::from(self.width) * scale).round() as u32).clamp(1, w.max(1)),
            ((f64::from(self.height) * scale).round() as u32).clamp(1, h.max(1)),
        )
    }

    /// The picture as half-block cells, at most `cells_w` by `cells_h` cells, keeping its shape
    /// (a half-block pixel is square).
    pub(crate) fn to_half_blocks(&self, cells_w: u16, cells_h: u16) -> HalfBlock {
        let (w, h) = self.fit(u32::from(cells_w.max(1)), u32::from(cells_h.max(1)) * 2);
        let pixels = self.resample(w, h);
        let mut canvas = HalfBlock::new(w as u16, h.div_ceil(2) as u16);
        for (i, rgb) in pixels.into_iter().enumerate() {
            canvas.set((i as u32 % w) as u16, (i as u32 / w) as u16, rgb);
        }
        canvas
    }

    /// The picture as braille cells, at most `cells_w` by `cells_h` cells. A dot is lit where
    /// its pixel is brighter than its cell's average, which keeps edges; the cell takes its
    /// average colour.
    pub(crate) fn to_braille(&self, cells_w: u16, cells_h: u16) -> Braille {
        let (w, h) = self.fit(u32::from(cells_w.max(1)) * 2, u32::from(cells_h.max(1)) * 4);
        let pixels = self.resample(w, h);
        let mut canvas = Braille::new(w.div_ceil(2) as u16, h.div_ceil(4) as u16);
        for cy in 0..h.div_ceil(4) {
            for cx in 0..w.div_ceil(2) {
                let mut dots = Vec::with_capacity(8);
                for dy in 0..4 {
                    for dx in 0..2 {
                        let (x, y) = (cx * 2 + dx, cy * 4 + dy);
                        if x < w && y < h {
                            dots.push((x, y, pixels[(y * w + x) as usize]));
                        }
                    }
                }
                let n = dots.len().max(1) as f32;
                let mean = dots.iter().map(|d| brightness(d.2)).sum::<f32>() / n;
                let avg = {
                    let sum = dots.iter().fold((0_u32, 0_u32, 0_u32), |a, d| {
                        (
                            a.0 + u32::from(d.2.0),
                            a.1 + u32::from(d.2.1),
                            a.2 + u32::from(d.2.2),
                        )
                    });
                    let k = dots.len().max(1) as u32;
                    ((sum.0 / k) as u8, (sum.1 / k) as u8, (sum.2 / k) as u8)
                };
                let (lo, hi) = dots.iter().fold((1.0_f32, 0.0_f32), |(lo, hi), d| {
                    let b = brightness(d.2);
                    (lo.min(b), hi.max(b))
                });
                if hi - lo < 0.08 {
                    // A flat cell has no edge to keep: light as many dots as it is bright, in
                    // a fixed dither order, so black stays empty and white is solid.
                    const ORDER: [(u32, u32); 8] = [
                        (0, 0),
                        (1, 2),
                        (0, 2),
                        (1, 0),
                        (0, 3),
                        (1, 1),
                        (0, 1),
                        (1, 3),
                    ];
                    let lit = (mean * 8.0).round() as usize;
                    for (dx, dy) in ORDER.iter().take(lit) {
                        let (x, y) = (cx * 2 + dx, cy * 4 + dy);
                        if x < w && y < h {
                            canvas.set(x as i32, y as i32, avg);
                        }
                    }
                } else {
                    for (x, y, rgb) in &dots {
                        if brightness(*rgb) >= mean {
                            canvas.set(*x as i32, *y as i32, avg);
                        }
                    }
                }
            }
        }
        canvas
    }
}

#[cfg(test)]
mod fuzz_tests;
#[cfg(test)]
mod tests;
