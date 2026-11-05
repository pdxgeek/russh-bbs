//! Small drawing helpers the widgets share: sparklines, bars, a braille area chart, seven-segment
//! digits and the calendar's date arithmetic (UI-10).
//!
//! Everything returns plain text. The caller styles it with the theme's palette roles, so none
//! of it can carry an escape sequence, a reverse-video marker or a Nerd Font glyph. Each helper
//! has an ASCII form (`ascii = true`): seven-bit characters only, readable on a monochrome
//! terminal.

/// Unicode sparkline levels, low to high.
const SPARK: [char; 8] = [
    '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}',
];
/// ASCII sparkline levels, low to high.
const SPARK_ASCII: [char; 8] = ['_', '.', ':', '-', '=', '+', '*', '#'];
/// Left-aligned partial blocks, one eighth to seven eighths.
const PARTIAL: [char; 7] = [
    '\u{258f}', '\u{258e}', '\u{258d}', '\u{258c}', '\u{258b}', '\u{258a}', '\u{2589}',
];

/// A sparkline of `values` as `width` cells (the newest values when there are more than fit,
/// left-padded with the lowest level when fewer). The scale runs from zero to the largest value,
/// so an all-zero series draws flat.
#[must_use]
pub(crate) fn spark(values: &[u64], width: usize, ascii: bool) -> String {
    let levels = if ascii { &SPARK_ASCII } else { &SPARK };
    let max = values.iter().copied().max().unwrap_or(0);
    let shown = &values[values.len().saturating_sub(width)..];
    let pad = width.saturating_sub(shown.len());
    let mut out = String::with_capacity(width * 3);
    out.extend(std::iter::repeat_n(levels[0], pad));
    for &v in shown {
        out.push(level(v, max, levels));
    }
    out
}

fn level(v: u64, max: u64, levels: &[char; 8]) -> char {
    if max == 0 || v == 0 {
        return levels[0];
    }
    // Round up so any non-zero value shows above the floor.
    let step = (v.saturating_mul(7)).div_ceil(max).min(7);
    levels[usize::try_from(step).unwrap_or(7)]
}

/// A horizontal bar `width` cells wide filled to `part` of `whole`: `(filled, rest)`, so the
/// caller can colour the two parts differently. The filled part may end in a partial block.
/// `part` beyond `whole` is full; a `whole` of zero is empty.
#[must_use]
pub(crate) fn bar(part: u64, whole: u64, width: usize, ascii: bool) -> (String, String) {
    let part = u128::from(part.min(whole));
    let whole = u128::from(whole.max(1));
    let cells = width as u128;
    if ascii {
        let full = usize::try_from((part * cells * 2 + whole) / (whole * 2)).unwrap_or(width);
        let full = full.min(width);
        return ("#".repeat(full), "-".repeat(width - full));
    }
    let eighths = usize::try_from((part * cells * 8 + whole / 2) / whole).unwrap_or(width * 8);
    let full = (eighths / 8).min(width);
    let partial = eighths % 8;
    let mut filled = "\u{2588}".repeat(full);
    let mut used = full;
    if partial > 0 && full < width {
        filled.push(PARTIAL[partial - 1]);
        used += 1;
    }
    (filled, "\u{2591}".repeat(width - used))
}

/// A braille area chart `cols` cells wide and `rows` tall: two columns of dots per cell and
/// four rows of dots per cell, filled from the bottom up to each value (scaled to the largest).
/// Lines are returned top row first. In ASCII the chart is drawn as stacked `#` columns.
#[must_use]
pub(crate) fn area_chart(values: &[u64], cols: usize, rows: usize, ascii: bool) -> Vec<String> {
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let max = values.iter().copied().max().unwrap_or(0);
    let dots_wide = if ascii { cols } else { cols * 2 };
    let dots_high = if ascii { rows } else { rows * 4 };
    // One height (in dots) per dot column, resampled by taking the nearest source value.
    let heights: Vec<usize> = (0..dots_wide)
        .map(|x| {
            if values.is_empty() || max == 0 {
                return 0;
            }
            let at = x * values.len() / dots_wide;
            let v = values[at.min(values.len() - 1)];
            if v == 0 {
                0
            } else {
                usize::try_from(v.saturating_mul(dots_high as u64).div_ceil(max))
                    .unwrap_or(dots_high)
                    .clamp(1, dots_high)
            }
        })
        .collect();
    let mut out = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut line = String::with_capacity(cols * 3);
        for col in 0..cols {
            if ascii {
                // Row 0 is the top; a column is filled in this row when its height reaches it.
                line.push(if heights[col] > rows - 1 - row {
                    '#'
                } else {
                    ' '
                });
                continue;
            }
            let mut bits = 0_u32;
            for (dx, column_bits) in BITS.iter().enumerate() {
                let height = heights[col * 2 + dx];
                for (dy, bit) in column_bits.iter().enumerate() {
                    // Dot rows count from the bottom of the whole chart.
                    let from_bottom = (rows - 1 - row) * 4 + (3 - dy);
                    if height > from_bottom {
                        bits |= bit;
                    }
                }
            }
            line.push(char::from_u32(0x2800 + bits).unwrap_or(' '));
        }
        out.push(line);
    }
    out
}

/// Bit values of the braille dot at (column in cell, row in cell).
const BITS: [[u32; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// Which of the seven segments (a top, b upper right, c lower right, d bottom, e lower left,
/// f upper left, g middle) each digit lights.
const SEGMENTS: [[bool; 7]; 10] = [
    [true, true, true, true, true, true, false],
    [false, true, true, false, false, false, false],
    [true, true, false, true, true, false, true],
    [true, true, true, true, false, false, true],
    [false, true, true, false, false, true, true],
    [true, false, true, true, false, true, true],
    [true, false, true, true, true, true, true],
    [true, true, true, false, false, false, false],
    [true, true, true, true, true, true, true],
    [true, true, true, true, false, true, true],
];

/// Large seven-segment digits for `text` (digits and `:`), three rows tall. Unicode uses heavy
/// box-drawing strokes, ASCII uses `_` and `|`. Anything else becomes a blank column.
#[must_use]
pub(crate) fn big_digits(text: &str, ascii: bool) -> [String; 3] {
    let (h, v) = if ascii {
        ('_', '|')
    } else {
        ('\u{2501}', '\u{2503}')
    };
    let mut rows = [String::new(), String::new(), String::new()];
    for ch in text.chars() {
        if ch == ':' {
            let dot = if ascii { '.' } else { '\u{2022}' };
            rows[0].push_str("  ");
            rows[1].push(dot);
            rows[1].push(' ');
            rows[2].push(dot);
            rows[2].push(' ');
            continue;
        }
        let Some(seg) = ch.to_digit(10).map(|d| SEGMENTS[d as usize]) else {
            for row in &mut rows {
                row.push_str("   ");
            }
            continue;
        };
        let pick = |on: bool, c: char| if on { c } else { ' ' };
        // The top row has no corners, the others have two uprights and a stroke between.
        rows[0].push(' ');
        rows[0].push(pick(seg[0], h));
        rows[0].push(' ');
        rows[1].push(pick(seg[5], v));
        rows[1].push(pick(seg[6], h));
        rows[1].push(pick(seg[1], v));
        rows[2].push(pick(seg[4], v));
        rows[2].push(pick(seg[3], h));
        rows[2].push(pick(seg[2], v));
        for row in &mut rows {
            row.push(' ');
        }
    }
    rows
}

/// A calendar date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Date {
    pub(crate) year: i64,
    /// 1 to 12.
    pub(crate) month: u32,
    /// 1 to 31.
    pub(crate) day: u32,
    /// 0 is Monday, 6 is Sunday.
    pub(crate) weekday: u32,
}

/// The UTC date of `unix` seconds (the proleptic Gregorian calendar; days-from-civil inverse).
#[must_use]
pub(crate) fn date_of(unix: i64) -> Date {
    let days = unix.div_euclid(86_400);
    // 1970-01-01 was a Thursday (3, with Monday as 0).
    let weekday = u32::try_from((days + 3).rem_euclid(7)).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    Date {
        year,
        month,
        day,
        weekday,
    }
}

/// Days in `month` of `year`.
#[must_use]
pub(crate) const fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
    }
}

/// The three-letter month name.
#[must_use]
pub(crate) const fn month_name(month: u32) -> &'static str {
    match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        _ => "Dec",
    }
}

/// The two-letter weekday heads, Monday first.
pub(crate) const WEEKDAYS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

/// The three-letter weekday name (0 is Monday).
#[must_use]
pub(crate) const fn weekday_name(weekday: u32) -> &'static str {
    match weekday {
        0 => "Mon",
        1 => "Tue",
        2 => "Wed",
        3 => "Thu",
        4 => "Fri",
        5 => "Sat",
        _ => "Sun",
    }
}

/// The weeks of `date`'s month: each row seven optional days, Monday first.
#[must_use]
pub(crate) fn month_weeks(date: Date) -> Vec<[Option<u32>; 7]> {
    let first_weekday = (date.weekday + 35 - (date.day - 1) % 7) % 7;
    let days = days_in_month(date.year, date.month);
    let mut weeks = Vec::new();
    let mut row = [None; 7];
    let mut col = usize::try_from(first_weekday).unwrap_or(0);
    for day in 1..=days {
        row[col] = Some(day);
        col += 1;
        if col == 7 {
            weeks.push(row);
            row = [None; 7];
            col = 0;
        }
    }
    if col > 0 {
        weeks.push(row);
    }
    weeks
}

/// How long ago `at` was, as a word-free figure like `now`, `5m`, `2h` or `3d`: returns the
/// unit key suffix and the number. `None` for "now" (under a minute) so the caller picks the word.
#[must_use]
pub(crate) fn ago(now: i64, at: i64) -> Option<(Unit, i64)> {
    let secs = (now - at).max(0);
    match secs {
        0..=59 => None,
        60..=3_599 => Some((Unit::Minute, secs / 60)),
        3_600..=86_399 => Some((Unit::Hour, secs / 3_600)),
        _ => Some((Unit::Day, secs / 86_400)),
    }
}

/// The unit of an [`ago`] figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unit {
    Minute,
    Hour,
    Day,
}

/// Bytes as a short figure: `812B`, `3.4K`, `12M`.
#[must_use]
pub(crate) fn bytes(n: u64) -> String {
    const UNITS: [char; 3] = ['K', 'M', 'G'];
    let (mut value, mut rest, mut unit) = (n, 0, 0);
    while value >= 1024 && unit < UNITS.len() {
        rest = value % 1024;
        value /= 1024;
        unit += 1;
    }
    if unit == 0 {
        format!("{n}B")
    } else if value < 10 {
        format!("{value}.{}{}", rest * 10 / 1024, UNITS[unit - 1])
    } else {
        format!("{value}{}", UNITS[unit - 1])
    }
}

/// Seconds as `3d 4h`, `4h 05m` or `5m`.
#[must_use]
pub(crate) fn span(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3_600, secs % 3_600 / 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else {
        format!("{m}m")
    }
}

/// Cut `text` to `width` characters, ending with an ellipsis (or `~` in ASCII) when it was cut.
#[must_use]
pub(crate) fn clip(text: &str, width: usize, ascii: bool) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mark = if ascii { '~' } else { '\u{2026}' };
    let mut out: String = text.chars().take(width - 1).collect();
    out.push(mark);
    out
}

/// Wrap `text` to `width` columns on spaces, at most `max_lines` lines (the last ends with an
/// ellipsis when text was left). Words longer than a line are cut.
#[must_use]
pub(crate) fn wrap(text: &str, width: usize, max_lines: usize, ascii: bool) -> Vec<String> {
    if width == 0 || max_lines == 0 {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut cut = false;
    for word in text.split_whitespace() {
        let word: String = word.chars().take(width).collect();
        if current.is_empty() {
            current = word;
        } else if current.chars().count() + 1 + word.chars().count() <= width {
            current.push(' ');
            current.push_str(&word);
        } else {
            lines.push(std::mem::take(&mut current));
            if lines.len() == max_lines {
                cut = true;
                break;
            }
            current = word;
        }
    }
    if !cut && !current.is_empty() {
        lines.push(current);
    }
    if cut && let Some(last) = lines.last_mut() {
        let mark = if ascii { '~' } else { '\u{2026}' };
        *last = clip(&format!("{last}{mark}"), width, ascii);
    }
    lines
}
