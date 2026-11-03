//! Quoting already-sanitized text.
//!
//! A quote is ordinary body text: each quoted line starts with `> `. That is deliberate and
//! honest about what it protects. Anyone can type `> ` themselves, so a quote mark proves
//! nothing about who wrote the line and is never security chrome. What the rule *does*
//! guarantee is that a quote cannot become chrome: [`quote`] only accepts [`Sanitized`] text
//! (the sanitizer already replaced the reserved markers, escape sequences and private-use
//! glyphs), returns [`Sanitized`] text again, and the editor draws quote lines in a plain dim
//! style that is not the reserved marker style (D39, D45).

use crate::sanitize::{MultiLine, Sanitized};

/// The deepest quote [`quote`] produces. A message that already quotes is quoted again once
/// (`> x` becomes `> > x`); its lines that are already this deep are dropped, so a long thread
/// does not snowball into a staircase of marks.
pub const MAX_QUOTE_DEPTH: usize = 2;

/// How much of a message is quoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuoteLimits {
    /// Most quoted lines kept; the rest are replaced by one `> [...]` line.
    pub max_lines: usize,
    /// Most characters in the result, attribution included. The result is cut at a line
    /// boundary, never inside a line.
    pub max_chars: usize,
}

impl Default for QuoteLimits {
    fn default() -> Self {
        Self {
            max_lines: 40,
            max_chars: 4_000,
        }
    }
}

/// How many `>` marks start `line` (a space may follow each: `> > x` and `>> x` are depth 2).
#[must_use]
pub fn quote_depth(line: &str) -> usize {
    let mut depth = 0;
    let mut rest = line.trim_start_matches(' ');
    while let Some(after) = rest.strip_prefix('>') {
        depth += 1;
        rest = after.trim_start_matches(' ');
    }
    depth
}

/// Whether `line` is a quoted line.
#[must_use]
pub fn is_quote_line(line: &str) -> bool {
    quote_depth(line) > 0
}

/// Quote `text`: an optional `attribution` line (for example `alice wrote:`, built by the
/// caller and sanitized), then every line of `text` prefixed with `> `, then a blank line so
/// the reply starts below the quote.
///
/// Lines already quoted [`MAX_QUOTE_DEPTH`] deep are dropped (the result is never deeper). Leading and trailing blank lines
/// are dropped. The result is bounded by `limits` and sanitized again, so it is [`Sanitized`]
/// like its inputs.
#[must_use]
pub fn quote(attribution: Option<&Sanitized>, text: &Sanitized, limits: QuoteLimits) -> Sanitized {
    let mut out = String::new();
    if let Some(attribution) = attribution {
        let line = attribution.as_str().lines().next().unwrap_or_default();
        if !line.trim().is_empty() {
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    let kept: Vec<&str> = text
        .as_str()
        .lines()
        .filter(|line| quote_depth(line) < MAX_QUOTE_DEPTH)
        .collect();
    let first = kept.iter().position(|l| !l.trim().is_empty());
    let last = kept.iter().rposition(|l| !l.trim().is_empty());
    if let (Some(first), Some(last)) = (first, last) {
        let body = kept.get(first..=last).unwrap_or_default();
        let truncated = body.len() > limits.max_lines;
        for line in body.iter().take(limits.max_lines) {
            let quoted = if line.trim().is_empty() {
                ">".to_owned()
            } else {
                // A line that already quotes gets one more mark: `> x` becomes `> > x`.
                format!("> {}", line.trim().trim_start_matches(' '))
            };
            if out.chars().count() + quoted.chars().count() + 1 > limits.max_chars {
                out.push_str("> [...]\n");
                return finish(out);
            }
            out.push_str(&quoted);
            out.push('\n');
        }
        if truncated {
            out.push_str("> [...]\n");
        }
    }
    finish(out)
}

/// End with the blank line the reply starts on, and run the shared sanitizer once more so the
/// result carries the same proof as its inputs.
fn finish(mut text: String) -> Sanitized {
    if !text.is_empty() {
        text.push('\n');
    }
    MultiLine::new(usize::MAX / 8).sanitize(&text)
}
