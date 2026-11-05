//! What the widgets show: plain, already-sanitized snapshots, fetched through the same services
//! the screens use (UI-10).
//!
//! **No second set of rules.** Every figure here comes from a function that already checks the
//! viewer's permissions: [`community::execute`] (last callers, one-liners, the wall and polls,
//! each with its permission and its poll-result visibility), [`forums::boards`] and
//! [`forums::threads`] (the boards the viewer may list, and the ones they may read), the mail
//! store behind `mail.read`, and the viewer's own credit ledger. A field that is `None` means
//! "the viewer cannot reach it, or there is nothing to show": the widget collapses. Nothing is
//! drawn from a guess, and no widget ever receives a row from a board the viewer cannot read.
//!
//! The pieces that come from the running server rather than the store (who is online, chat room
//! counts, doors, status) are filled in by the server, after it asks [`WidgetId::reachable_by`]
//! with the same snapshot.

use crate::access::{Permission, Snapshot};
use crate::community::{self, CommunityStore, LineKind, Request, Response};
use crate::credits::CreditStore;
use crate::forums::{self, ForumStore};
use crate::mail::MailStore;
use crate::sanitize::SingleLine;
use crate::store::UnixSeconds;

use super::id::WidgetId;

/// Days in the activity and credit series (the last one is today).
pub const SERIES_DAYS: usize = 14;
/// Most rows a list widget keeps; a widget draws as many as its cell holds.
pub const LIST_ROWS: usize = 8;
/// Most boards one fetch looks into.
const BOARDS_LOOKED_AT: usize = 12;
const DAY: i64 = 86_400;

/// One line of a one-liner or wall list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The author's handle.
    pub author: String,
    /// The text.
    pub text: String,
    /// When it was added.
    pub at: UnixSeconds,
}

/// A recent caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
    /// The handle.
    pub handle: String,
    /// When their session began.
    pub at: UnixSeconds,
}

/// Threads with new posts since the viewer's last call, on boards they may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPosts {
    /// Threads with a post since then, across the boards.
    pub threads: u64,
    /// The boards with the most, `(name, threads)`, at most four.
    pub boards: Vec<(String, u64)>,
}

/// The viewer's mailbox counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MailCount {
    /// Messages in all.
    pub total: u64,
    /// Unread messages.
    pub unread: u64,
}

/// A chat room and how many are in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomCount {
    /// The room's name.
    pub name: String,
    /// Members in it now.
    pub members: u32,
    /// Their handles, by name (filled by the server from the live room; the split layout's
    /// chat pane lists them). Empty when nobody is in.
    pub here: Vec<String>,
}

/// One message in the viewer's inbox, for the split layout's mail pane. The subject was opened
/// with the board's mail key by the server; nothing here is the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentMail {
    /// Who sent it (a handle), or `None` when the account is gone.
    pub from: Option<String>,
    /// The subject, or `None` when the message cannot be opened.
    pub subject: Option<String>,
    /// Whether it has not been opened.
    pub unread: bool,
    /// When it was sent.
    pub at: UnixSeconds,
}

/// The viewer's credits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credits {
    /// The balance now.
    pub balance: u64,
    /// The balance at the end of each of the last [`SERIES_DAYS`] days, oldest first.
    pub series: Vec<u64>,
}

/// A poll as the viewer may see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollView {
    /// The question.
    pub question: String,
    /// The options with their votes. `None` while the poll hides the counts from this viewer.
    pub options: Vec<(String, Option<u64>)>,
    /// The option the viewer voted for.
    pub my_vote: Option<usize>,
}

/// A feed headline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Headline {
    /// The board (feed) it is on.
    pub board: String,
    /// The subject.
    pub subject: String,
    /// When it last changed.
    pub at: UnixSeconds,
}

/// A door the viewer can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoorLine {
    /// The door's name.
    pub name: String,
    /// Its current season.
    pub season: u32,
}

/// Sessions and traffic, for staff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    /// Sessions connected.
    pub sessions: u32,
    /// Of those, signed-in members.
    pub members: u32,
    /// The most sessions the board allows (the gauge's top).
    pub capacity: u32,
    /// Frames sent to clients since the server started.
    pub frames: u64,
    /// Bytes in them.
    pub bytes: u64,
    /// Seconds the server has been up.
    pub uptime: u64,
}

/// What every widget shows, as of [`WidgetData::at`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WidgetData {
    /// When it was fetched.
    pub at: Option<UnixSeconds>,
    /// Handles of members online (needs `members.view`).
    pub online: Option<Vec<String>>,
    /// The last callers (needs `community.view`).
    pub callers: Option<Vec<Caller>>,
    /// New posts on readable boards.
    pub new_posts: Option<NewPosts>,
    /// Mail counts (needs `mail.read`).
    pub mail: Option<MailCount>,
    /// The newest messages in the inbox (needs `mail.read`; the server fills it, with the mail
    /// key, for the split layout's mail pane).
    pub recent_mail: Option<Vec<RecentMail>>,
    /// Chat rooms the viewer may join.
    pub chat: Option<Vec<RoomCount>>,
    /// Threads last posted per day on readable boards, oldest first.
    pub activity: Option<Vec<u64>>,
    /// The viewer's own credits.
    pub credits: Option<Credits>,
    /// One-liners, newest first (needs `community.view`).
    pub oneliners: Option<Vec<Line>>,
    /// Graffiti, newest first (needs `community.view`).
    pub wall: Option<Vec<Line>>,
    /// The newest open poll (needs `community.view`).
    pub poll: Option<PollView>,
    /// Headlines from the feed boards the viewer may read.
    pub news: Option<Vec<Headline>>,
    /// Doors the viewer may run.
    pub doors: Option<Vec<DoorLine>>,
    /// Status (needs `sessions.view`).
    pub status: Option<Status>,
}

fn clean(text: &str, max: usize) -> String {
    SingleLine::new(max).sanitize(text).into_string()
}

/// Fetch every store-backed widget in `wanted` for `access`. `since` is the viewer's previous
/// sign-in (the "new since your last call" mark); without one, the last day counts.
///
/// A failure of one widget's reads leaves that widget empty and the rest intact: a slow table
/// must not blank the dashboard.
pub async fn fetch<S>(
    store: &S,
    access: &Snapshot,
    wanted: &[WidgetId],
    now: UnixSeconds,
    since: Option<UnixSeconds>,
) -> WidgetData
where
    S: CommunityStore + ForumStore + MailStore + CreditStore,
{
    let mut data = WidgetData {
        at: Some(now),
        ..WidgetData::default()
    };
    let want = |id: WidgetId| wanted.contains(&id) && id.reachable_by(access);
    if want(WidgetId::Callers) {
        data.callers = callers(store, access, now).await;
    }
    if want(WidgetId::Oneliner) {
        data.oneliners = lines(store, access, now, LineKind::Oneliner).await;
    }
    if want(WidgetId::Wall) {
        data.wall = lines(store, access, now, LineKind::Graffiti).await;
    }
    if want(WidgetId::Poll) {
        data.poll = poll(store, access, now).await;
    }
    if want(WidgetId::Mail) {
        data.mail = mail(store, access).await;
    }
    if want(WidgetId::Credits) {
        data.credits = credits(store, access, now).await;
    }
    if want(WidgetId::NewPosts) || want(WidgetId::Activity) || want(WidgetId::News) {
        let forum = forum(store, access, now, since).await;
        if want(WidgetId::NewPosts) {
            data.new_posts = forum.new_posts;
        }
        if want(WidgetId::Activity) {
            data.activity = forum.activity;
        }
        if want(WidgetId::News) {
            data.news = forum.news;
        }
    }
    data
}

async fn callers<S: CommunityStore>(
    store: &S,
    access: &Snapshot,
    now: UnixSeconds,
) -> Option<Vec<Caller>> {
    match community::execute(store, access, now, Request::Callers).await {
        Response::Callers(rows) if !rows.is_empty() => Some(
            rows.into_iter()
                .take(LIST_ROWS)
                .map(|row| Caller {
                    handle: row.handle,
                    at: row.at,
                })
                .collect(),
        ),
        _ => None,
    }
}

async fn lines<S: CommunityStore>(
    store: &S,
    access: &Snapshot,
    now: UnixSeconds,
    kind: LineKind,
) -> Option<Vec<Line>> {
    match community::execute(store, access, now, Request::Lines(kind)).await {
        Response::Lines { entries, .. } if !entries.is_empty() => Some(
            entries
                .into_iter()
                .take(LIST_ROWS)
                .map(|entry| Line {
                    author: entry.author,
                    text: entry.text,
                    at: entry.at,
                })
                .collect(),
        ),
        _ => None,
    }
}

/// The newest open poll, with counts only when the poll lets this viewer see them.
async fn poll<S: CommunityStore>(
    store: &S,
    access: &Snapshot,
    now: UnixSeconds,
) -> Option<PollView> {
    let Response::Polls { rows, .. } =
        community::execute(store, access, now, Request::Polls { offset: 0 }).await
    else {
        return None;
    };
    let open = rows.iter().find(|row| !row.closed)?;
    let Response::Poll { detail, .. } =
        community::execute(store, access, now, Request::Poll(open.id)).await
    else {
        return None;
    };
    let options = detail
        .options
        .iter()
        .enumerate()
        .map(|(at, text)| {
            let votes = detail
                .counts
                .as_ref()
                .and_then(|counts| counts.get(at).copied());
            (clean(text, 60), votes)
        })
        .collect();
    Some(PollView {
        question: clean(&detail.question, 120),
        options,
        my_vote: detail.my_vote,
    })
}

async fn mail<S: MailStore>(store: &S, access: &Snapshot) -> Option<MailCount> {
    let granted = access.require(Permission::MailRead, None).ok()?;
    let account = granted.account()?;
    match store.inbox_counts(account).await {
        Ok((total, unread)) => Some(MailCount { total, unread }),
        Err(error) => {
            tracing::warn!(%error, "mail count for a widget failed");
            None
        }
    }
}

async fn credits<S: CreditStore>(
    store: &S,
    access: &Snapshot,
    now: UnixSeconds,
) -> Option<Credits> {
    let account = access.account()?;
    let balance = match store.balance(account).await {
        Ok(balance) => balance,
        Err(error) => {
            tracing::warn!(%error, "credit balance for a widget failed");
            return None;
        }
    };
    let entries = store.entries(account, None, 200).await.unwrap_or_default();
    Some(Credits {
        balance,
        series: balance_series(balance, &entries, now),
    })
}

/// The balance at the end of each of the last [`SERIES_DAYS`] days, from the balance now and the
/// ledger rows (newest first). Pure, so it is tested without a store.
#[must_use]
pub fn balance_series(
    balance: u64,
    entries: &[crate::credits::LedgerEntry],
    now: UnixSeconds,
) -> Vec<u64> {
    let today = now.as_raw().div_euclid(DAY);
    let mut running = i128::from(balance);
    let mut out = vec![0_u64; SERIES_DAYS];
    let mut next = entries.iter().peekable();
    for back in 0..SERIES_DAYS {
        let day = today - i64::try_from(back).unwrap_or(0);
        // The balance at the end of `day` is `running` once every row after that day is undone.
        while let Some(entry) = next.peek() {
            if entry.at.as_raw().div_euclid(DAY) > day {
                running -= i128::from(entry.delta);
                next.next();
            } else {
                break;
            }
        }
        out[SERIES_DAYS - 1 - back] = u64::try_from(running.max(0)).unwrap_or(0);
    }
    out
}

struct Forum {
    new_posts: Option<NewPosts>,
    activity: Option<Vec<u64>>,
    news: Option<Vec<Headline>>,
}

/// One pass over the boards the viewer may read: new threads, a per-day activity series and
/// feed headlines. Only boards `forums::boards` lists and `forums::threads` opens are read.
async fn forum<S: ForumStore>(
    store: &S,
    access: &Snapshot,
    now: UnixSeconds,
    since: Option<UnixSeconds>,
) -> Forum {
    let none = Forum {
        new_posts: None,
        activity: None,
        news: None,
    };
    let Ok(boards) = forums::boards(store, access).await else {
        return none;
    };
    let since = since.map_or(now.as_raw() - DAY, UnixSeconds::as_raw);
    let today = now.as_raw().div_euclid(DAY);
    let mut series = vec![0_u64; SERIES_DAYS];
    let mut per_board = Vec::new();
    let mut headlines = Vec::new();
    let mut total = 0_u64;
    let mut looked = 0_u32;
    for board in boards.iter().take(BOARDS_LOOKED_AT) {
        let Ok(page) = forums::threads(store, access, board.id, 0).await else {
            continue;
        };
        looked += 1;
        let mut fresh = 0_u64;
        for thread in &page.threads {
            let at = thread.last_post_at.as_raw();
            if at > since {
                fresh += 1;
            }
            let back = today - at.div_euclid(DAY);
            if let Ok(back) = usize::try_from(back)
                && back < SERIES_DAYS
            {
                series[SERIES_DAYS - 1 - back] += 1;
            }
            if board.feed {
                headlines.push(Headline {
                    board: clean(&board.name, 40),
                    subject: clean(&thread.subject, 120),
                    at: thread.last_post_at,
                });
            }
        }
        if fresh > 0 {
            total += fresh;
            per_board.push((clean(&board.name, 40), fresh));
        }
    }
    if looked == 0 {
        return none;
    }
    per_board.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    per_board.truncate(4);
    headlines.sort_by_key(|h| std::cmp::Reverse(h.at));
    headlines.truncate(LIST_ROWS);
    Forum {
        new_posts: Some(NewPosts {
            threads: total,
            boards: per_board,
        }),
        activity: Some(series),
        news: (!headlines.is_empty()).then_some(headlines),
    }
}
