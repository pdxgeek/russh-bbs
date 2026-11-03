//! What a chat line is: sanitized once, here, and bounded.

use crate::sanitize::{Sanitized, SingleLine, Username, sanitize_username};

/// Longest chat line or whisper, in characters. Longer input is cut, not refused, so a paste
/// still says something; the sanitizer does the cutting.
pub const MAX_LINE_CHARS: usize = 300;

/// Sanitize a typed line with the shared sanitizer. `None` when nothing is left (blank, or only
/// characters the sanitizer removes), so an empty line is never sent.
#[must_use]
pub fn clean_line(input: &str) -> Option<Sanitized> {
    let text = SingleLine::new(MAX_LINE_CHARS).sanitize(input.trim());
    (!text.as_str().trim().is_empty()).then_some(text)
}

/// Whether `typed` names the same handle as `handle`, the way the board treats handles as the
/// same (the username skeleton, D17): case and look-alike letters do not matter. Text that is
/// not a valid handle matches nobody.
#[must_use]
pub fn same_handle(typed: &str, handle: &Username) -> bool {
    sanitize_username(typed).is_ok_and(|typed| typed.skeleton() == handle.skeleton())
}
