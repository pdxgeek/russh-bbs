//! Feeds against any backend (M3-G, D89): feeds as read-only boards, the items seen, the closed
//! outcomes, removal with its board, and the public marks RSS out reads. The rules are core's;
//! these tests run them over a real database.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

use russh_bbs_core::access::{Permission, Slug, Snapshot};
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore, known_events};
use russh_bbs_core::drafts::Body;
use russh_bbs_core::feeds::{
    self, AddFeed, FeedError, FeedStore, NewFeed, NewItem, Outcome, out, parse,
};
use russh_bbs_core::forums::{self, BoardId, ForumError, ForumStore, body_limits};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, UnixSeconds,
};

use super::{Harness, TestStore};

const T0: i64 = 1_800_000_000;

fn at(n: i64) -> UnixSeconds {
    UnixSeconds::from_raw(T0 + n)
}

async fn make<S: AccountStore>(store: &S, name: &str, role: Slug) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

async fn snap<S: TestStore>(store: &S, who: &Account) -> Snapshot {
    store.snapshot(Some(who.id)).await.unwrap()
}

fn body(text: &str) -> Body {
    Body::prepare(body_limits(), text).unwrap()
}

/// An RSS document with `n` items numbered from `from`, each with its own guid and text.
fn items(from: usize, n: usize) -> Vec<NewItem> {
    let xml: String = (from..from + n)
        .map(|i| {
            format!(
                "<item><title>Item {i}</title><guid>guid-{i}</guid><link>https://example.org/{i}</link>\
                 <description>Text of number {i} quokka</description>\
                 <pubDate>Mon, 02 Jan 2006 15:{:02}:{:02} GMT</pubDate></item>",
                (i / 60) % 60,
                i % 60
            )
        })
        .collect();
    let doc = format!(
        "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>t</title>{xml}</channel></rss>"
    );
    parse(doc.as_bytes(), at(0)).unwrap().items
}

struct World {
    sysop: Account,
    ann: Account,
}

async fn world<S: TestStore>(s: &S) -> World {
    World {
        sysop: make(s, "ops", Slug::SYSOP).await,
        ann: make(s, "ann", Slug::MEMBER).await,
    }
}

async fn add<S: TestStore>(s: &S, sys: &Snapshot, url: &str, name: &str) -> feeds::Added {
    feeds::add_feed(s, sys, at(0), url, name, false, None)
        .await
        .unwrap()
}

pub(crate) async fn staff_add_feeds_as_read_only_boards_with_an_audit_row_and_no_address<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let ann = snap(s, &w.ann).await;
    let sys = snap(s, &w.sysop).await;
    assert!(matches!(
        feeds::add_feed(
            s,
            &ann,
            at(0),
            "https://example.org/rss",
            "News",
            false,
            None
        )
        .await,
        Err(FeedError::NotPermitted)
    ));
    assert!(matches!(
        feeds::add_feed(
            s,
            &Snapshot::empty(),
            at(0),
            "https://example.org/rss",
            "News",
            false,
            None
        )
        .await,
        Err(FeedError::NotSignedIn)
    ));
    // Refused addresses and names write nothing.
    for bad in [
        "http://example.org/rss",
        "ftp://example.org/",
        "https://u:p@example.org/",
        "nonsense",
    ] {
        assert!(
            matches!(
                feeds::add_feed(s, &sys, at(0), bad, "News", false, None).await,
                Err(FeedError::Url(_))
            ),
            "{bad}"
        );
    }
    assert!(matches!(
        feeds::add_feed(
            s,
            &sys,
            at(0),
            "https://example.org/rss",
            "   ",
            false,
            None
        )
        .await,
        Err(FeedError::Text(_))
    ));
    assert!(matches!(
        feeds::add_feed(
            s,
            &sys,
            at(0),
            "https://example.org/rss",
            "News",
            false,
            Some(60)
        )
        .await,
        Err(FeedError::Interval)
    ));
    assert!(s.feeds().await.unwrap().is_empty());

    let added = feeds::add_feed(
        s,
        &sys,
        at(0),
        "https://example.org/rss?token=SECRET",
        "News",
        false,
        None,
    )
    .await
    .unwrap();
    // A plain-http feed needs the flag, and keeps it.
    let plain = feeds::add_feed(
        s,
        &sys,
        at(0),
        "http://example.org/plain",
        "Plain",
        true,
        Some(900),
    )
    .await
    .unwrap();
    let rows = s.feeds().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].allow_http && rows[1].allow_http);
    assert_eq!(rows[0].interval_secs, feeds::DEFAULT_INTERVAL_SECS);
    assert_eq!(rows[1].interval_secs, 900);
    assert_eq!(rows[0].url, "https://example.org/rss?token=SECRET");
    assert_eq!(rows[0].last_attempt_at, None);
    assert_eq!(rows[0].failures, 0);
    assert_eq!(rows[0].board, added.board);
    assert_eq!(rows[1].board, plain.board);
    assert_eq!(
        s.feed_of_board(added.board).await.unwrap().unwrap().id,
        added.feed
    );

    // Names and addresses are unique.
    assert!(matches!(
        feeds::add_feed(
            s,
            &sys,
            at(0),
            "https://example.org/other",
            " news ",
            false,
            None
        )
        .await,
        Err(FeedError::NameTaken)
    ));
    assert!(matches!(
        feeds::add_feed(
            s,
            &sys,
            at(0),
            "https://example.org/rss?token=SECRET",
            "Another",
            false,
            None
        )
        .await,
        Err(FeedError::UrlTaken)
    ));
    // A taken name does not leave a board or a feed behind.
    assert_eq!(s.feeds().await.unwrap().len(), 2);
    assert_eq!(s.boards().await.unwrap().len(), 2);

    // The board is an ordinary board to the member, marked as a feed board.
    let listed = forums::boards(s, &ann).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|b| b.feed && !b.restricted));
    let page = forums::threads(s, &ann, added.board, 0).await.unwrap();
    assert!(page.board.feed && !page.can_post && !page.can_feeds);
    let page = forums::threads(s, &sys, added.board, 0).await.unwrap();
    assert!(!page.can_post && page.can_feeds);

    // The audit log says who and which board, never the address.
    let log = known_events(
        s.page(&AuditQuery {
            action: Some(Action::FeedAdded),
            ..AuditQuery::default()
        })
        .await
        .unwrap(),
    );
    assert_eq!(log.len(), 2);
    for entry in &log {
        let detail = entry.event.detail.as_str();
        assert!(
            !detail.contains("example.org") && !detail.contains("SECRET"),
            "{detail}"
        );
    }
}

pub(crate) async fn the_number_of_feeds_is_limited_even_when_added_at_once<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let mut tasks = Vec::new();
    for i in 0..8 {
        let s = s.clone();
        let actor: AccountId = w.sysop.id;
        tasks.push(tokio::spawn(async move {
            let new = NewFeed {
                url: feeds::FeedUrl::parse(&format!("https://example.org/{i}"), false).unwrap(),
                name: forums::prepare_name(&format!("Feed {i}")).unwrap(),
                description: forums::prepare_description("d").unwrap(),
                allow_http: false,
                interval_secs: 3_600,
            };
            s.add_feed(&new, actor, at(0), 3).await.unwrap()
        }));
    }
    let mut added = 0;
    let mut full = 0;
    for task in tasks {
        match task.await.unwrap() {
            AddFeed::Added(..) => added += 1,
            AddFeed::TooMany => full += 1,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!((added, full), (3, 5));
    assert_eq!(s.feeds().await.unwrap().len(), 3);
}

pub(crate) async fn posting_and_replying_on_a_feed_board_are_refused_by_core<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let sys = snap(s, &w.sysop).await;
    let ann = snap(s, &w.ann).await;
    let added = add(s, &sys, "https://example.org/rss", "News").await;
    let feed = s.feeds().await.unwrap().remove(0);
    s.ingest(feed.id, &items(1, 1), at(10), 200)
        .await
        .unwrap()
        .unwrap();
    let thread = forums::threads(s, &ann, added.board, 0)
        .await
        .unwrap()
        .threads[0]
        .id;
    for who in [&ann, &sys] {
        assert!(matches!(
            forums::post_thread(s, who, at(20), added.board, "Mine", &body("hello")).await,
            Err(ForumError::ReadOnly)
        ));
        assert!(matches!(
            forums::post_reply(s, who, at(20), thread, &body("hello")).await,
            Err(ForumError::ReadOnly)
        ));
    }
    // Nothing was written, and no quota was used.
    let page = forums::threads(s, &ann, added.board, 0).await.unwrap();
    assert_eq!(page.threads.len(), 1);
    assert_eq!(page.threads[0].posts, 1);
    // The raw store would write (core is the rule), but the permission is also false at the
    // rules layer for every session.
    let boards = s.boards().await.unwrap();
    assert!(boards[0].feed);
    assert!(!forums::rules::can(&sys, Permission::ForumPost, &boards[0]));
}

pub(crate) async fn items_become_authorless_threads_once_and_the_newest_are_kept<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let sys = snap(s, &w.sysop).await;
    let ann = snap(s, &w.ann).await;
    let added = add(s, &sys, "https://example.org/rss", "News").await;
    let feed = s.feeds().await.unwrap().remove(0);

    let first = items(1, 3);
    let done = s
        .ingest(feed.id, &first, at(100), 200)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((done.added, done.skipped), (3, 0));
    let row = s.feeds().await.unwrap().remove(0);
    assert_eq!(row.last_attempt_at, Some(at(100)));
    assert_eq!(row.last_success_at, Some(at(100)));
    assert_eq!(row.last_outcome, Some(Outcome::Ok));
    assert_eq!(row.failures, 0);

    // The same items again: nothing new, nothing duplicated.
    let again = s
        .ingest(feed.id, &first, at(200), 200)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((again.added, again.skipped), (0, 3));
    // The same content under new ids is also a duplicate (the content hash).
    let mut renamed = first.clone();
    for item in &mut renamed {
        item.key_hash = format!("{:0>64}", item.key_hash.len() + 1);
    }
    let again = s
        .ingest(feed.id, &renamed, at(300), 200)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((again.added, again.skipped), (0, 3));
    // The same id with new content is also seen (an edited item is not posted twice).
    let mut edited = first.clone();
    for item in &mut edited {
        item.content_hash = format!("{:0>64}", "edited");
    }
    let again = s
        .ingest(feed.id, &edited, at(300), 200)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((again.added, again.skipped), (0, 3));
    // Duplicates inside one batch count once.
    let mut doubled = items(10, 1);
    doubled.extend(items(10, 1));
    let both = s
        .ingest(feed.id, &doubled, at(400), 200)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((both.added, both.skipped), (1, 1));

    let page = forums::threads(s, &ann, added.board, 0).await.unwrap();
    assert_eq!(page.threads.len(), 4);
    assert!(
        page.threads
            .iter()
            .all(|t| t.author.as_deref() == Some("feed"))
    );
    let view = forums::thread_post(s, &ann, page.threads[0].id, 0)
        .await
        .unwrap();
    assert_eq!(view.post.author.as_deref(), Some("feed"));
    assert!(
        view.post
            .body
            .as_str()
            .contains("Link: https://example.org/")
    );
    assert_eq!(view.total, 1);

    // Search finds the text with no change to the index.
    let found = forums::search(s, &ann, "quokka", 0).await.unwrap();
    assert!(found.hits.len() >= 4);
    assert!(
        found
            .hits
            .iter()
            .all(|h| h.author.as_deref() == Some("feed"))
    );

    // Only the newest threads are kept; their items stay "seen", so they do not come back.
    let many = items(100, 10);
    s.ingest(feed.id, &many, at(500), 5).await.unwrap().unwrap();
    let page = forums::threads(s, &ann, added.board, 0).await.unwrap();
    assert_eq!(page.threads.len(), 5);
    let back = s
        .ingest(feed.id, &first, at(600), 5)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((back.added, back.skipped), (0, 3));
    assert_eq!(
        forums::threads(s, &ann, added.board, 0)
            .await
            .unwrap()
            .threads
            .len(),
        5
    );
}

pub(crate) async fn a_failed_read_is_a_closed_outcome_and_a_good_one_resets_it<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let sys = snap(s, &w.sysop).await;
    add(s, &sys, "https://example.org/rss", "News").await;
    let feed = s.feeds().await.unwrap().remove(0);
    for (n, outcome) in [
        Outcome::Refused,
        Outcome::Failed,
        Outcome::Invalid,
        Outcome::TooLarge,
        Outcome::TimedOut,
    ]
    .into_iter()
    .enumerate()
    {
        s.record_failure(feed.id, outcome, at(10 + i64::try_from(n).unwrap()))
            .await
            .unwrap();
        let row = s.feeds().await.unwrap().remove(0);
        assert_eq!(row.last_outcome, Some(outcome));
        assert_eq!(row.failures, u32::try_from(n).unwrap() + 1);
        assert_eq!(row.last_success_at, None);
        assert_eq!(
            row.last_attempt_at,
            Some(at(10 + i64::try_from(n).unwrap()))
        );
    }
    s.ingest(feed.id, &items(1, 1), at(100), 200)
        .await
        .unwrap()
        .unwrap();
    let row = s.feeds().await.unwrap().remove(0);
    assert_eq!((row.failures, row.last_outcome), (0, Some(Outcome::Ok)));
    // Recording a failure for a feed that is gone is harmless.
    s.record_failure(feeds::FeedId::from_raw(9_999), Outcome::Failed, at(1))
        .await
        .unwrap();
}

pub(crate) async fn removing_a_feed_removes_its_board_threads_and_search_hits<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let sys = snap(s, &w.sysop).await;
    let ann = snap(s, &w.ann).await;
    let keep = add(s, &sys, "https://example.org/keep", "Keep").await;
    let gone = add(s, &sys, "https://example.org/gone", "Gone").await;
    let rows = s.feeds().await.unwrap();
    for row in &rows {
        s.ingest(row.id, &items(1, 2), at(10), 200)
            .await
            .unwrap()
            .unwrap();
    }
    forums::create_board(s, &sys, at(0), "General", "", false)
        .await
        .unwrap();
    assert_eq!(
        forums::search(s, &ann, "quokka", 0)
            .await
            .unwrap()
            .hits
            .len(),
        4
    );

    // A member cannot remove one; staff can.
    assert!(matches!(
        feeds::remove_feed(s, &ann, rows[1].id).await,
        Err(FeedError::NotPermitted)
    ));
    assert_eq!(
        feeds::remove_feed(s, &sys, rows[1].id).await.unwrap(),
        gone.board
    );
    assert!(matches!(
        feeds::remove_feed(s, &sys, rows[1].id).await,
        Err(FeedError::NotFound)
    ));
    let names: Vec<_> = forums::boards(s, &ann)
        .await
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(names, ["Keep", "General"]);
    assert!(matches!(
        forums::threads(s, &ann, gone.board, 0).await,
        Err(ForumError::NotFound)
    ));
    // Its text cannot be found any more; the other feed's still can.
    let hits = forums::search(s, &ann, "quokka", 0).await.unwrap().hits;
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| h.board_name == "Keep"));
    assert_eq!(s.feeds().await.unwrap().len(), 1);
    assert_eq!(s.feeds().await.unwrap()[0].board, keep.board);
    // A read that finishes after the removal finds the feed gone.
    assert_eq!(
        s.ingest(rows[1].id, &items(5, 1), at(20), 200)
            .await
            .unwrap(),
        None
    );
    // The name is free again.
    add(s, &sys, "https://example.org/gone2", "Gone").await;

    let log = known_events(
        s.page(&AuditQuery {
            action: Some(Action::FeedRemoved),
            ..AuditQuery::default()
        })
        .await
        .unwrap(),
    );
    assert_eq!(
        log.len(),
        1,
        "one audit row, none for the refused or repeated removal"
    );
    assert!(!log[0].event.detail.as_str().contains("example.org"));
}

pub(crate) async fn a_board_is_public_as_rss_only_when_marked_open_and_readable<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let sys = snap(s, &w.sysop).await;
    let ann = snap(s, &w.ann).await;
    let news = add(s, &sys, "https://example.org/rss", "News").await;
    let other = add(s, &sys, "https://example.org/other", "Other").await;
    let secret = forums::create_board(s, &sys, at(0), "Staff", "", true)
        .await
        .unwrap();
    for row in s.feeds().await.unwrap() {
        s.ingest(row.id, &items(1, 2), at(10), 200)
            .await
            .unwrap()
            .unwrap();
    }
    let public = |boards: Vec<BoardId>| boards;
    // Nothing is public until staff says so, and the anonymous snapshot reads nothing.
    let anon = out::public_snapshot(s).await.unwrap();
    assert!(out::readable_boards(s, &anon).await.unwrap().is_empty());
    for board in [news.board, other.board, secret] {
        assert_eq!(
            out::render_board(s, &anon, board, "https://bbs.example/feeds", at(50))
                .await
                .unwrap(),
            None
        );
    }
    // Only staff may mark a board; a restricted board cannot be marked; a missing one is absent.
    assert!(matches!(
        feeds::publish_board(s, &ann, at(20), news.board, true).await,
        Err(FeedError::NotPermitted)
    ));
    assert!(matches!(
        feeds::publish_board(s, &sys, at(20), secret, true).await,
        Err(FeedError::Restricted)
    ));
    assert!(matches!(
        feeds::publish_board(s, &sys, at(20), BoardId::from_raw(9_999), true).await,
        Err(FeedError::NotFound)
    ));
    assert!(s.public_boards().await.unwrap().is_empty());

    feeds::publish_board(s, &sys, at(20), news.board, true)
        .await
        .unwrap();
    // Marking again changes nothing and writes no second audit row.
    feeds::publish_board(s, &sys, at(21), news.board, true)
        .await
        .unwrap();
    assert_eq!(public(s.public_boards().await.unwrap()), [news.board]);
    assert!(
        s.feeds()
            .await
            .unwrap()
            .iter()
            .any(|f| f.public && f.board == news.board)
    );
    let anon = out::public_snapshot(s).await.unwrap();
    assert_eq!(out::readable_boards(s, &anon).await.unwrap(), [news.board]);

    let xml = out::render_board(s, &anon, news.board, "https://bbs.example/feeds", at(50))
        .await
        .unwrap()
        .expect("a public board is served");
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(xml.contains("<rss version=\"2.0\""));
    assert!(xml.contains("<title>News</title>"));
    assert!(xml.contains("https://bbs.example/feeds/rss/"));
    assert!(xml.contains("Text of number 1 quokka"));
    // It is a valid feed: our own parser reads both items back.
    assert_eq!(parse(xml.as_bytes(), at(60)).unwrap().items.len(), 2);
    // Authors (member handles) are never written.
    assert!(!xml.contains("ann") || !xml.contains("<author>"));

    // The other feed board, the restricted board and a missing id are the same "no".
    for board in [other.board, secret, BoardId::from_raw(9_999)] {
        assert_eq!(
            out::render_board(s, &anon, board, "https://bbs.example/feeds", at(50))
                .await
                .unwrap(),
            None
        );
    }
    // Even if a restricted board were somehow marked, it would not be served (the mark is also
    // refused at the store).
    assert!(
        !s.set_public(secret, true, w.sysop.id, at(22))
            .await
            .unwrap()
    );

    feeds::publish_board(s, &sys, at(30), news.board, false)
        .await
        .unwrap();
    let anon = out::public_snapshot(s).await.unwrap();
    assert_eq!(
        out::render_board(s, &anon, news.board, "https://bbs.example/feeds", at(50))
            .await
            .unwrap(),
        None
    );

    // Marked again, then the feed (and its board) removed: the mark goes with it.
    feeds::publish_board(s, &sys, at(31), news.board, true)
        .await
        .unwrap();
    let id = s.feeds().await.unwrap()[0].id;
    feeds::remove_feed(s, &sys, id).await.unwrap();
    assert!(s.public_boards().await.unwrap().is_empty());

    let log = known_events(s.page(&AuditQuery::default()).await.unwrap());
    let published = log
        .iter()
        .filter(|e| e.event.action == Action::BoardPublished)
        .count();
    let unpublished = log
        .iter()
        .filter(|e| e.event.action == Action::BoardUnpublished)
        .count();
    assert_eq!(
        (published, unpublished),
        (2, 1),
        "changes only: the repeat wrote nothing"
    );
}

pub(crate) async fn feed_data_survives_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    {
        let s = h.store();
        let w = world(s).await;
        let sys = snap(s, &w.sysop).await;
        let added = add(s, &sys, "https://example.org/rss", "News").await;
        let feed = s.feeds().await.unwrap().remove(0);
        s.ingest(feed.id, &items(1, 2), at(10), 200)
            .await
            .unwrap()
            .unwrap();
        feeds::publish_board(s, &sys, at(11), added.board, true)
            .await
            .unwrap();
    }
    h.reopen().await;
    let s = h.store();
    let rows = s.feeds().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].public);
    assert_eq!(rows[0].last_outcome, Some(Outcome::Ok));
    let again = s
        .ingest(rows[0].id, &items(1, 2), at(20), 200)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (again.added, again.skipped),
        (0, 2),
        "what was seen is still seen"
    );
}
