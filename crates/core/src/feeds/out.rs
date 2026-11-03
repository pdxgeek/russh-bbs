//! RSS out: a public board as an RSS 2.0 document.
//!
//! **Who may read it.** A board is public only when the sysop marked it ([`super::publish_board`])
//! and it is not restricted. The test is not the mark: the mark only adds scoped `forum.list` and
//! `forum.read` entries to an *anonymous* snapshot (the one an unknown key gets, `Via::Waiting`),
//! and readability is then decided by the same [`rules::can`] every board uses. A board that is
//! absent, not public or restricted gives the same answer (`None`), so the endpoint reveals
//! nothing about boards that are not public.
//!
//! **What it holds.** At most [`MAX_ITEMS`] threads, each with its subject and opening post as
//! the shared sanitizer left them (cleaned again here), XML-escaped, with characters XML 1.0
//! forbids dropped. Authors are never written (a post's author is a member's handle).

use std::fmt::Write as _;

use crate::access::{AccessStore, Held, Permission, ResourceKind, Scope, Snapshot, Source, Via};
use crate::forums::{BoardId, ForumStore, rules};
use crate::sanitize::{MultiLine, SingleLine};
use crate::store::{StoreError, UnixSeconds};

use super::store::FeedStore;

/// Most items in one document.
pub const MAX_ITEMS: u32 = 50;
/// Longest item description, in characters.
const DESCRIPTION_CHARS: usize = 2_000;

/// The anonymous snapshot RSS out resolves against: what an unknown key holds, plus read access
/// to each board the sysop marked public.
///
/// # Errors
/// [`StoreError`] when storage fails.
pub async fn public_snapshot<S: AccessStore + FeedStore>(
    store: &S,
) -> Result<Snapshot, StoreError> {
    let base = store.snapshot(None).await?;
    let mut held = base.held().to_vec();
    for board in store.public_boards().await? {
        let Some(resource) = board.resource() else {
            continue;
        };
        for permission in [Permission::ForumList, Permission::ForumRead] {
            held.push(Held {
                permission,
                scope: Scope::Resource(resource),
                source: Source::Allow { via: Via::Waiting },
            });
        }
    }
    Ok(Snapshot::new(None, held))
}

/// The ids of the boards `snapshot` may read and that are open (not restricted): the public
/// boards.
///
/// # Errors
/// [`StoreError`] when storage fails.
pub async fn readable_boards<S: ForumStore>(
    store: &S,
    snapshot: &Snapshot,
) -> Result<Vec<BoardId>, StoreError> {
    if snapshot.reach(Permission::ForumRead, ResourceKind::Forum) == crate::access::Reach::None {
        return Ok(Vec::new());
    }
    let all = store.boards().await?;
    Ok(rules::reachable(snapshot, Permission::ForumRead, &all)
        .into_iter()
        .filter(|b| !b.restricted)
        .map(|b| b.id)
        .collect())
}

/// The RSS 2.0 document for `board`, or `None` when the board is absent or not public. `base_url`
/// is the sysop's configured public address of this endpoint (no trailing slash).
///
/// # Errors
/// [`StoreError`] when storage fails.
pub async fn render_board<S: ForumStore>(
    store: &S,
    snapshot: &Snapshot,
    board: BoardId,
    base_url: &str,
    now: UnixSeconds,
) -> Result<Option<String>, StoreError> {
    let Some(row) = store.board(board).await? else {
        return Ok(None);
    };
    if row.restricted || !rules::can(snapshot, Permission::ForumRead, &row) {
        return Ok(None);
    }
    let threads = store.threads(board, MAX_ITEMS, 0).await?;
    let mut out = String::new();
    let me = format!("{base_url}/rss/{board}");
    let title = clean_line(&row.name, 40);
    let description = clean_line(&row.description, 160);
    let _ = write!(
        out,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\">\n<channel>\n\
         <title>{}</title>\n<link>{}</link>\n<description>{}</description>\n\
         <atom:link href=\"{}\" rel=\"self\" type=\"application/rss+xml\"/>\n\
         <lastBuildDate>{}</lastBuildDate>\n",
        escape(&title),
        escape(base_url),
        escape(if description.is_empty() {
            &title
        } else {
            &description
        }),
        escape(&me),
        rfc2822(now),
    );
    for thread in threads {
        let Some(post) = store.post_at(thread.id, 0).await? else {
            continue;
        };
        let subject = clean_line(&thread.subject, 120);
        let text = MultiLine::new(DESCRIPTION_CHARS).sanitize(&post.body);
        let _ = write!(
            out,
            "<item>\n<title>{}</title>\n<description>{}</description>\n\
             <pubDate>{}</pubDate>\n<guid isPermaLink=\"false\">{}</guid>\n</item>\n",
            escape(&subject),
            escape(text.as_str()),
            rfc2822(thread.last_post_at),
            escape(&format!("{me}/{}", thread.id)),
        );
    }
    out.push_str("</channel>\n</rss>\n");
    Ok(Some(out))
}

fn clean_line(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

/// Escape text for XML and drop what XML 1.0 cannot carry.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// RFC 822 date, as RSS 2.0 wants (`Thu, 01 Jan 1970 00:00:00 +0000`).
#[must_use]
pub fn rfc2822(at: UnixSeconds) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let secs = at.as_raw().max(0);
    let days = secs.div_euclid(86_400);
    let rest = secs.rem_euclid(86_400);
    // 1970-01-01 was a Thursday (index 4).
    let weekday = usize::try_from((days + 4).rem_euclid(7)).unwrap_or(0);
    // Civil date from days since the epoch (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{}, {day:02} {} {year:04} {:02}:{:02}:{:02} +0000",
        DAYS[weekday],
        MONTHS[usize::try_from(month - 1).unwrap_or(0)],
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60,
    )
}
