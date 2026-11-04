//! The words for the key notices a member reads at sign-in.

use super::store::{KeyNotice, NoticeKind};
use super::strings;
use crate::terminal_settings::format_utc;

/// One line per notice, then the "if this was not you" line. Empty for no notices. The times
/// are UTC and the fingerprints are public, validated values, so nothing here needs the
/// sanitizer.
#[must_use]
pub fn notice_lines(notices: &[KeyNotice]) -> Vec<String> {
    if notices.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![strings::NOTICE_HEADING.to_owned()];
    for notice in notices {
        let template = match notice.kind {
            NoticeKind::KeyAdded => strings::NOTICE_KEY_ADDED,
            NoticeKind::KeyRemoved => strings::NOTICE_KEY_REMOVED,
            NoticeKind::KeyLinked => strings::NOTICE_KEY_LINKED,
            NoticeKind::DeviceLinked => strings::NOTICE_DEVICE_LINKED,
            NoticeKind::Recovered => strings::NOTICE_RECOVERED,
        };
        let key = notice
            .fingerprint
            .as_ref()
            .map_or_else(|| "no key".to_owned(), ToString::to_string);
        lines.push(
            template
                .replace("{when}", &format_utc(notice.at))
                .replace("{key}", &key),
        );
    }
    lines.push(strings::NOTICE_IF_NOT_YOU.to_owned());
    lines
}
