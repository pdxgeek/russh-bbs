//! The sysop queue and the vouch screen's drawing and request running (M2-E).
//!
//! The rules and the state machine are in `russh_bbs_core::queue`; this module only turns a
//! [`QueueView`] into ratatui lines for the classic hotkey layout (`crate::ui::classic`) and
//! runs the screen's request against the store with a time limit.
//!
//! **Where requester text goes.** The handle, the message and the vouchers' names were typed by
//! people the sysop does not know yet. Every such line is drawn into the content rectangle
//! `ui::classic` hands out. The status row (the last row, where the server-readable marker
//! lives) is outside that rectangle, so a long message cannot reach it. Content is plain: it is
//! never given the reserved marker style, never used as a key or a label, and it is sanitized
//! once more here, on render, whatever core already did. Rows are cut to one line so a list
//! always fits 80x24 with its footer.

use std::time::Duration;

use ratatui::text::{Line as TextLine, Span};
use russh_bbs_core::access::Snapshot;
use russh_bbs_core::queue::{
    MESSAGE_MAX_CHARS, Purged, QueueStore, QueueView, Request, Response, Row, ViewLine, execute,
    keys, purge_expired,
};
use russh_bbs_core::sanitize::SingleLine;
use russh_bbs_core::waiting_room::NoticeKind;
use tokio::time::timeout;
use tracing::warn;

use crate::settings::Look;
use crate::ui::classic;
use crate::ui::strings::StringTable;

/// How long a queue request may take. A slow store must not hang the screen; the screen then
/// says it did not work and changes nothing.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Run `request` for a session holding `access`. A timeout is [`Response::Failed`]: nothing is
/// claimed about whether a write happened beyond what the store reported.
pub(crate) async fn run<S: QueueStore>(store: &S, access: &Snapshot, request: Request) -> Response {
    if let Ok(response) = timeout(REQUEST_TIMEOUT, execute(store, access, request)).await {
        response
    } else {
        warn!("queue request timed out");
        Response::Failed
    }
}

/// How often the server deletes expired requests and denial cooldowns (D72). The store also
/// purges on every submission and listing; this is for a quiet board, so that what has expired
/// does not sit on disk until someone happens to ask.
pub const PURGE_INTERVAL: Duration = Duration::from_secs(3600);

/// Purge once. A failure is logged and returns `None`; the next tick tries again.
pub async fn purge_once<S: QueueStore>(store: &S) -> Option<Purged> {
    match timeout(REQUEST_TIMEOUT, purge_expired(store)).await {
        Ok(Ok(purged)) => Some(purged),
        Ok(Err(error)) => {
            warn!(%error, "purging expired queue rows failed");
            None
        }
        Err(_) => {
            warn!("purging expired queue rows timed out");
            None
        }
    }
}

/// Purge now and then every `period`, forever. Run it as a task and abort it at shutdown.
pub async fn purge_every<S: QueueStore>(store: S, period: Duration) {
    let mut ticker = tokio::time::interval(period);
    loop {
        // The first tick is immediate, so a board that was down past an expiry forgets at boot.
        ticker.tick().await;
        purge_once(&store).await;
    }
}

/// Content text, sanitized once more on its way to the terminal.
fn clean(text: &str) -> String {
    SingleLine::new(MESSAGE_MAX_CHARS)
        .sanitize(text)
        .into_string()
}

/// Cut `text` to at most `width` terminal columns, so a list row never wraps onto a second
/// line and pushes the page off the screen.
fn fit(text: &str, width: usize) -> String {
    let mut out = String::new();
    for c in text.chars() {
        out.push(c);
        if Span::raw(out.as_str()).width() > width {
            out.pop();
            break;
        }
    }
    out
}

/// Indent of a list entry's second line, under the handle.
const INDENT: &str = "    ";

/// The two lines of a list entry: `[1] handle   asked 3h ago   vouches: 2`, then the message.
fn entry(row: &Row, strings: &StringTable, look: Look, width: usize) -> [TextLine<'static>; 2] {
    let (_, dim, _, key) = classic::styles(look);
    let meta = strings.filled(
        if row.mine {
            keys::ROW_META_MINE
        } else {
            keys::ROW_META
        },
        &[
            ("age", row.age.as_str()),
            ("vouches", &row.vouches.to_string()),
        ],
    );
    let first = TextLine::from(vec![
        Span::styled(format!("[{}] ", row.hotkey), key),
        Span::raw(clean(&row.handle)),
        Span::styled(format!("   {meta}"), dim),
    ]);
    let second = TextLine::raw(format!(
        "{INDENT}{}",
        fit(&clean(&row.message), width.saturating_sub(INDENT.len()))
    ));
    [first, second]
}

/// The lines of `view`, ready for [`classic::render_content`]. `width` is the room inside the
/// box, used only to keep list rows on one line.
pub(crate) fn lines(
    view: &QueueView,
    strings: &StringTable,
    look: Look,
    width: usize,
) -> Vec<TextLine<'static>> {
    let (bold, dim, heading, _) = classic::styles(look);
    let mut out = Vec::with_capacity(view.lines.len() + 2);
    for line in &view.lines {
        match line {
            ViewLine::Blank => out.push(TextLine::raw("")),
            ViewLine::Heading(text) => {
                out.push(TextLine::styled(strings.text(text).to_owned(), heading));
            }
            ViewLine::Text(text) => out.push(TextLine::raw(strings.text(text).to_owned())),
            ViewLine::Filled { key, values } => {
                let values: Vec<(&str, String)> =
                    values.iter().map(|(k, v)| (*k, clean(v))).collect();
                let values: Vec<(&str, &str)> =
                    values.iter().map(|(k, v)| (*k, v.as_str())).collect();
                out.push(TextLine::raw(strings.filled(key, &values)));
            }
            ViewLine::Entry(row) => out.extend(entry(row, strings, look, width)),
            ViewLine::Actions(keys_on_offer) => {
                let (_, _, _, key) = classic::styles(look);
                let mut spans = Vec::new();
                for (index, text) in keys_on_offer.iter().enumerate() {
                    if index > 0 {
                        spans.push(Span::raw("   "));
                    }
                    spans.push(Span::styled(strings.text(text).to_owned(), key));
                }
                out.push(TextLine::from(spans));
            }
            ViewLine::Pair { label, value, .. } => out.push(TextLine::from(vec![
                Span::styled(format!("{}: ", strings.text(label)), bold),
                Span::raw(clean(value)),
            ])),
            ViewLine::Content(text) => out.push(TextLine::raw(clean(text))),
            ViewLine::Notice { error, text } => out.push(classic::notice_line(
                strings,
                if *error {
                    NoticeKind::Error
                } else {
                    NoticeKind::Info
                },
                strings.text(text),
                look,
                bold,
            )),
        }
    }
    out.push(TextLine::raw(""));
    out.push(TextLine::styled(strings.text(view.footer).to_owned(), dim));
    out
}

#[cfg(test)]
mod tests;
