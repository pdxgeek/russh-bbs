//! Forums against any backend (M3-E, D83): boards as access resources, posts and replies,
//! `posts_per_hour`, staff removal with its audit row, and search that never reaches a board
//! the reader cannot read.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::assertions_on_constants,
    clippy::too_many_lines
)]

use russh_bbs_core::access::{Permission, Slug, Snapshot};
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore, known_events};
use russh_bbs_core::drafts::Body;
use russh_bbs_core::forums::{
    self, BoardId, ForumError, ForumStore, MAX_TERMS, NewPost, PAGE_SIZE, PostCap, PostWrite,
    ThreadId, body_limits,
};
use russh_bbs_core::limits::{Limit, LimitKind, set_member_limit};
use russh_bbs_core::sanitize::{RESERVED_CHROME, sanitize_username};
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, UnixSeconds,
};

use super::{Harness, TestStore};

const T0: i64 = 1_000_000;

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

/// A sysop, three members, an open board and a restricted one with `ann` let in.
struct World {
    sysop: Account,
    ann: Account,
    bob: Account,
    cat: Account,
    open: BoardId,
    staff: BoardId,
}

async fn world<S: TestStore>(s: &S) -> World {
    let sysop = make(s, "ops", Slug::SYSOP).await;
    let ann = make(s, "ann", Slug::MEMBER).await;
    let bob = make(s, "bob", Slug::MEMBER).await;
    let cat = make(s, "cat", Slug::MEMBER).await;
    let sys = snap(s, &sysop).await;
    let open = forums::create_board(s, &sys, at(0), "General", "Talk", false)
        .await
        .unwrap();
    let staff = forums::create_board(s, &sys, at(0), "Staff room", "Keys and rotas", true)
        .await
        .unwrap();
    forums::allow_member(s, &sys, staff, "ann").await.unwrap();
    World {
        sysop,
        ann,
        bob,
        cat,
        open,
        staff,
    }
}

fn names(boards: &[forums::BoardSummary]) -> Vec<&str> {
    boards.iter().map(|b| b.name.as_str()).collect()
}

pub(crate) async fn boards_are_created_by_staff_only_with_unique_names_and_an_audit_row<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let ann = snap(s, &w.ann).await;
    assert!(matches!(
        forums::create_board(s, &ann, at(1), "Mine", "", false).await,
        Err(ForumError::NotPermitted(Permission::ForumManage))
    ));
    let sys = snap(s, &w.sysop).await;
    assert!(matches!(
        forums::create_board(s, &sys, at(1), "  general ", "", false).await,
        Err(ForumError::NameTaken)
    ));
    assert!(matches!(
        forums::create_board(s, &sys, at(1), "   ", "", false).await,
        Err(ForumError::Text(_))
    ));
    let long = "x".repeat(41);
    assert!(matches!(
        forums::create_board(s, &sys, at(1), &long, "", false).await,
        Err(ForumError::Text(_))
    ));
    let rows = known_events(
        s.page(&AuditQuery {
            action: Some(Action::BoardCreated),
            ..AuditQuery::default()
        })
        .await
        .unwrap(),
    );
    assert_eq!(rows.len(), 2, "one audit row per board, none for refusals");
    for entry in &rows {
        assert!(
            !entry.event.detail.as_str().contains("General"),
            "the name is not written"
        );
    }
}

pub(crate) async fn a_restricted_board_is_absent_for_everyone_without_a_scoped_grant<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let sys = snap(s, &w.sysop).await;
    let t = forums::post_thread(s, &sys, at(1), w.staff, "Rota", &body("who is on call"))
        .await
        .unwrap();
    let (ann, bob) = (snap(s, &w.ann).await, snap(s, &w.bob).await);
    // Lists: ann (allowed) sees both, bob only the open board, the sysop (forum.manage) both.
    assert_eq!(
        names(&forums::boards(s, &ann).await.unwrap()),
        ["General", "Staff room"]
    );
    assert_eq!(names(&forums::boards(s, &bob).await.unwrap()), ["General"]);
    assert_eq!(
        names(&forums::boards(s, &sys).await.unwrap()),
        ["General", "Staff room"]
    );
    // By id: the same answer for a restricted board bob may not see and one that is not there.
    let denied = forums::threads(s, &bob, w.staff, 0).await.unwrap_err();
    let absent = forums::threads(s, &bob, BoardId::from_raw(9_999), 0)
        .await
        .unwrap_err();
    assert!(matches!(denied, ForumError::NotFound));
    assert_eq!(denied.to_string(), absent.to_string());
    for result in [
        forums::thread_post(s, &bob, t.thread, 0).await.map(|_| ()),
        forums::thread_post(s, &bob, ThreadId::from_raw(9_999), 0)
            .await
            .map(|_| ()),
        forums::post_reply(s, &bob, at(2), t.thread, &body("hi"))
            .await
            .map(|_| ()),
        forums::post_thread(s, &bob, at(2), w.staff, "Hi", &body("hi"))
            .await
            .map(|_| ()),
        forums::remove_post(s, &bob, t.post).await.map(|_| ()),
    ] {
        assert!(matches!(result, Err(ForumError::NotFound)), "{result:?}");
    }
    assert!(forums::thread_post(s, &ann, t.thread, 0).await.is_ok());
    // A grant applies on the next snapshot: letting cat in changes cat's next list.
    let cat = snap(s, &w.cat).await;
    assert_eq!(names(&forums::boards(s, &cat).await.unwrap()), ["General"]);
    forums::allow_member(s, &sys, w.staff, "CAT").await.unwrap();
    let cat = snap(s, &w.cat).await;
    assert_eq!(
        names(&forums::boards(s, &cat).await.unwrap()),
        ["General", "Staff room"]
    );
    assert!(matches!(
        forums::allow_member(s, &sys, w.staff, "nobody").await,
        Err(ForumError::UnknownMember)
    ));
    assert!(matches!(
        forums::allow_member(s, &ann, w.staff, "bob").await,
        Err(ForumError::NotPermitted(_))
    ));
}

pub(crate) async fn threads_replies_and_reading_round_trip<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let (ann, bob) = (snap(s, &w.ann).await, snap(s, &w.bob).await);
    let first = forums::post_thread(
        s,
        &ann,
        at(10),
        w.open,
        "  Hello\u{1b}[31m world ",
        &body("opening\nsecond line"),
    )
    .await
    .unwrap();
    forums::post_reply(s, &bob, at(20), first.thread, &body("a reply"))
        .await
        .unwrap();
    let other = forums::post_thread(s, &bob, at(30), w.open, "Newer", &body("x"))
        .await
        .unwrap();
    let page = forums::threads(s, &ann, w.open, 0).await.unwrap();
    assert_eq!(page.threads.len(), 2);
    assert_eq!(page.threads[0].id, other.thread, "newest activity first");
    assert_eq!(
        page.threads[1].subject, "Hello world",
        "subject sanitized and trimmed"
    );
    assert_eq!(page.threads[1].posts, 2);
    assert!(page.can_post && !page.can_manage && !page.more);
    // A reply bumps its thread to the top.
    forums::post_reply(s, &ann, at(40), first.thread, &body("bump"))
        .await
        .unwrap();
    let page = forums::threads(s, &ann, w.open, 0).await.unwrap();
    assert_eq!(page.threads[0].id, first.thread);
    // Reading: post by post, oldest first; past the end shows the last.
    let view = forums::thread_post(s, &bob, first.thread, 0).await.unwrap();
    assert_eq!((view.index, view.total), (0, 3));
    assert_eq!(view.post.body.as_str(), "opening\nsecond line");
    assert_eq!(view.post.author.as_deref(), Some("ann"));
    assert!(view.can_reply && !view.can_moderate);
    let last = forums::thread_post(s, &bob, first.thread, 99)
        .await
        .unwrap();
    assert_eq!(last.index, 2);
    assert_eq!(last.post.body.as_str(), "bump");
    // The quote keeps attribution and prefixes each line.
    let quoted = view.post.quoted();
    assert!(quoted.as_str().contains("> opening"), "{quoted:?}");
    assert!(quoted.as_str().contains("opening"));
    // Paging: more threads than a page.
    for i in 0..PAGE_SIZE {
        forums::post_thread(
            s,
            &ann,
            at(100 + i64::from(i)),
            w.open,
            &format!("T{i}"),
            &body("b"),
        )
        .await
        .unwrap();
    }
    let p0 = forums::threads(s, &ann, w.open, 0).await.unwrap();
    let p1 = forums::threads(s, &ann, w.open, 1).await.unwrap();
    assert_eq!(p0.threads.len(), PAGE_SIZE as usize);
    assert!(p0.more);
    assert_eq!(p1.threads.len(), 2);
    assert!(!p1.more);
    // Bodies over the post bounds are refused, not cut.
    let long = "y".repeat(forums::BODY_MAX_CHARS + 1);
    let too_long = Body::prepare(russh_bbs_core::drafts::BodyLimits::MAX, &long).unwrap();
    assert!(matches!(
        forums::post_reply(s, &ann, at(500), first.thread, &too_long).await,
        Err(ForumError::Text(_))
    ));
    assert!(matches!(
        forums::post_reply(s, &ann, at(500), first.thread, &body("  \n ")).await,
        Err(ForumError::Text(_))
    ));
}

pub(crate) async fn stored_text_is_sanitized<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let ann = snap(s, &w.ann).await;
    let marker: String = RESERVED_CHROME.iter().collect();
    let evil = format!("sub\u{1b}[2Jject {marker}\u{202e} \u{e000}\u{7}");
    let posted = forums::post_thread(
        s,
        &ann,
        at(1),
        w.open,
        &evil,
        &body(&format!("bo\u{1b}]0;x\u{7}dy {marker}")),
    )
    .await
    .unwrap();
    for sql in [
        "SELECT subject FROM thread".to_owned(),
        "SELECT subject FROM post".to_owned(),
        "SELECT body FROM post".to_owned(),
    ] {
        for text in h.texts(&sql).await {
            assert!(
                !text
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t'
                        || RESERVED_CHROME.contains(&c)
                        || c == '\u{202e}'
                        || c == '\u{e000}'),
                "{text:?}"
            );
        }
    }
    let view = forums::thread_post(s, &ann, posted.thread, 0)
        .await
        .unwrap();
    assert!(!view.thread.subject.contains('\u{1b}'));
}

pub(crate) async fn posts_per_hour_is_enforced_from_the_post_rows<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let sys = snap(s, &w.sysop).await;
    let granted = sys.require(Permission::LimitsEdit, None).unwrap();
    set_member_limit(
        s,
        &granted,
        w.ann.id,
        LimitKind::PostsPerHour,
        Limit::AtMost(2),
    )
    .await
    .unwrap();
    let ann = snap(s, &w.ann).await;
    let t = forums::post_thread(s, &ann, at(0), w.open, "One", &body("a"))
        .await
        .unwrap();
    forums::post_reply(s, &ann, at(60), t.thread, &body("b"))
        .await
        .unwrap();
    let refused = forums::post_reply(s, &ann, at(120), t.thread, &body("c"))
        .await
        .unwrap_err();
    let ForumError::Limit(refusal) = refused else {
        panic!("expected a limit, got {refused:?}")
    };
    assert_eq!((refusal.kind, refusal.limit), (LimitKind::PostsPerHour, 2));
    assert!(matches!(
        forums::post_thread(s, &ann, at(120), w.open, "Two", &body("d")).await,
        Err(ForumError::Limit(_))
    ));
    // Others are not held to ann's override; nothing was written for the refusals.
    let bob = snap(s, &w.bob).await;
    forums::post_reply(s, &bob, at(120), t.thread, &body("bob"))
        .await
        .unwrap();
    assert_eq!(
        forums::thread_post(s, &bob, t.thread, 0)
            .await
            .unwrap()
            .total,
        3
    );
    // The window rolls: an hour after her first post she can post again.
    forums::post_reply(s, &ann, at(3_601), t.thread, &body("again"))
        .await
        .unwrap();
}

pub(crate) async fn concurrent_posts_never_pass_the_hourly_cap<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let ann = snap(s, &w.ann).await;
    let t = forums::post_thread(s, &ann, at(0), w.open, "Race", &body("start"))
        .await
        .unwrap();
    // `start` counts: a cap of 4 leaves room for three more.
    let cap = PostCap {
        since: at(-3_600),
        max: Some(4),
    };
    let mut tasks = Vec::new();
    for i in 0..8 {
        let s = s.clone();
        let author = w.ann.id;
        let thread = t.thread;
        tasks.push(tokio::spawn(async move {
            s.add_reply(
                &NewPost {
                    thread,
                    body: body(&format!("r{i}")),
                    author,
                    now: at(10),
                },
                cap,
            )
            .await
            .unwrap()
        }));
    }
    let mut written = 0;
    for task in tasks {
        if matches!(task.await.unwrap(), PostWrite::Written(_)) {
            written += 1;
        }
    }
    assert_eq!(written, 3);
    assert_eq!(
        forums::thread_post(s, &ann, t.thread, 0)
            .await
            .unwrap()
            .total,
        4
    );
}

pub(crate) async fn staff_remove_posts_with_an_audit_row_and_removed_text_is_not_found<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let (ann, bob, sys) = (
        snap(s, &w.ann).await,
        snap(s, &w.bob).await,
        snap(s, &w.sysop).await,
    );
    let t = forums::post_thread(
        s,
        &ann,
        at(1),
        w.open,
        "Spam thread",
        &body("buy zebrafish now"),
    )
    .await
    .unwrap();
    let reply = forums::post_reply(s, &bob, at(2), t.thread, &body("zebrafish too"))
        .await
        .unwrap();
    assert!(
        !forums::search(s, &bob, "zebrafish", 0)
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    // A member cannot remove; staff can; ids only in the audit row.
    assert!(matches!(
        forums::remove_post(s, &ann, reply.post).await,
        Err(ForumError::NotPermitted(Permission::ForumModerate))
    ));
    let gone = forums::remove_post(s, &sys, reply.post).await.unwrap();
    assert!(!gone.thread_removed);
    assert_eq!(
        forums::thread_post(s, &ann, t.thread, 0)
            .await
            .unwrap()
            .total,
        1
    );
    assert_eq!(
        forums::search(s, &bob, "zebrafish", 0)
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    let gone = forums::remove_post(s, &sys, t.post).await.unwrap();
    assert!(gone.thread_removed);
    assert!(
        forums::search(s, &bob, "zebrafish", 0)
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    assert!(matches!(
        forums::thread_post(s, &ann, t.thread, 0).await,
        Err(ForumError::NotFound)
    ));
    assert!(matches!(
        forums::remove_post(s, &sys, t.post).await,
        Err(ForumError::NotFound)
    ));
    let rows = known_events(
        s.page(&AuditQuery {
            action: Some(Action::PostRemoved),
            ..AuditQuery::default()
        })
        .await
        .unwrap(),
    );
    assert_eq!(rows.len(), 2);
    for entry in &rows {
        assert!(entry.event.detail.as_str().starts_with("post "));
        assert!(
            !entry.event.detail.as_str().contains("zebrafish"),
            "no post text in the log"
        );
    }
    // A scoped moderator can remove on their board only.
    let rows = h.texts("SELECT count(*) || '' FROM post").await;
    assert_eq!(rows, ["0"]);
}

pub(crate) async fn search_finds_words_and_never_reaches_unreadable_boards<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let (ann, bob, sys) = (
        snap(s, &w.ann).await,
        snap(s, &w.bob).await,
        snap(s, &w.sysop).await,
    );
    forums::post_thread(
        s,
        &ann,
        at(1),
        w.open,
        "Gardening tips",
        &body("tomatoes need sun and water"),
    )
    .await
    .unwrap();
    let t = forums::post_thread(s, &sys, at(2), w.open, "Other", &body("tomatoes are fruit"))
        .await
        .unwrap();
    forums::post_reply(s, &bob, at(3), t.thread, &body("I prefer cucumbers"))
        .await
        .unwrap();
    forums::post_thread(
        s,
        &sys,
        at(4),
        w.staff,
        "Hidden sauces",
        &body("the secret tomatoes password is hunter2"),
    )
    .await
    .unwrap();
    // Subject and body both match; every term must.
    let page = forums::search(s, &bob, "tomatoes", 0).await.unwrap();
    assert_eq!(page.hits.len(), 2);
    assert!(page.hits.iter().all(|hit| hit.board_name == "General"));
    assert_eq!(
        forums::search(s, &bob, "gardening water", 0)
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    assert!(
        forums::search(s, &bob, "gardening cucumbers", 0)
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        forums::search(s, &bob, "GARDENING", 0).await.unwrap().hits[0].subject,
        "Gardening tips"
    );
    assert!(page.hits[0].snippet.contains("tomatoes"));
    // The restricted board: bob never sees a title, snippet or count; ann and staff do.
    let secret = forums::search(s, &bob, "hunter2", 0).await.unwrap();
    let nothing = forums::search(s, &bob, "zzzzqqqq", 0).await.unwrap();
    assert!(secret.hits.is_empty() && !secret.more);
    assert_eq!(
        (secret.hits.len(), secret.more, secret.searched),
        (nothing.hits.len(), nothing.more, nothing.searched)
    );
    assert!(
        forums::search(s, &bob, "hidden sauces", 0)
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        forums::search(s, &ann, "hunter2", 0).await.unwrap().hits[0].board_name,
        "Staff room"
    );
    assert_eq!(
        forums::search(s, &sys, "tomatoes", 0)
            .await
            .unwrap()
            .hits
            .len(),
        3
    );
    // Let bob in: the next search finds it; the store itself never gets a board it may not read.
    forums::allow_member(s, &sys, w.staff, "bob").await.unwrap();
    let bob = snap(s, &w.bob).await;
    assert_eq!(
        forums::search(s, &bob, "hunter2", 0)
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    // Straight to the store: only the boards given are searched.
    let words = vec!["tomatoes".to_owned()];
    assert_eq!(
        s.search_posts(&[w.staff], &words, 10, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        s.search_posts(&[w.open], &words, 10, 0)
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(s.search_posts(&[], &words, 10, 0).await.unwrap().is_empty());
}

pub(crate) async fn hostile_queries_are_handled_not_errors<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let w = world(s).await;
    let ann = snap(s, &w.ann).await;
    forums::post_thread(
        s,
        &ann,
        at(1),
        w.open,
        "Plain words",
        &body("alpha beta gamma delta"),
    )
    .await
    .unwrap();
    let huge = "alpha ".repeat(50_000);
    let many = (0..500)
        .map(|i| format!("w{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let long_word = "a".repeat(10_000);
    let queries = [
        "\"",
        "\"alpha",
        "alpha\"",
        "\"\"\"\"",
        "*",
        "alpha*",
        "NEAR(alpha beta)",
        "alpha AND",
        "OR beta",
        "NOT",
        "-alpha",
        "alpha:beta",
        "subject:alpha",
        "{alpha}",
        "(alpha",
        ")",
        "^alpha",
        "alpha'; DROP TABLE post; --",
        "alpha) OR (1=1",
        "%",
        "_",
        "\\",
        "\0",
        "\u{1b}[2J",
        "\u{202e}",
        "😀",
        "",
        "   ",
        "!!!",
        "<script>",
        "a b c d e f g h i j k l m n o p",
        &huge,
        &many,
        &long_word,
        "alpha & beta | gamma",
        "alpha <-> beta",
        "!alpha",
        "alpha:*",
    ];
    for query in queries {
        let page = forums::search(s, &ann, query, 0)
            .await
            .unwrap_or_else(|e| panic!("{query:.40?}: {e}"));
        assert!(page.hits.len() <= PAGE_SIZE as usize);
    }
    // Operators are only separators: this is the two words, not a boolean query.
    assert_eq!(
        forums::search(s, &ann, "alpha AND beta", 0)
            .await
            .unwrap()
            .hits
            .len(),
        0,
        "AND is a word that must match"
    );
    assert_eq!(
        forums::search(s, &ann, "\"alpha\" *beta*", 0)
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    assert_eq!(
        forums::search(s, &ann, "alpha-beta", 0)
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    // More than MAX_TERMS words keeps the first ones only.
    assert!(MAX_TERMS < 50);
    assert!(
        forums::search(s, &ann, &format!("alpha beta {many}"), 0)
            .await
            .unwrap()
            .hits
            .len()
            <= 1
    );
    // The tables survived.
    assert_eq!(h.texts("SELECT count(*) || '' FROM post").await, ["1"]);
}

pub(crate) async fn forum_data_survives_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let w = world(h.store()).await;
    let ann = snap(h.store(), &w.ann).await;
    forums::post_thread(
        h.store(),
        &ann,
        at(1),
        w.open,
        "Persisted",
        &body("findable text"),
    )
    .await
    .unwrap();
    h.reopen().await;
    let s = h.store();
    let ann = snap(s, &w.ann).await;
    assert_eq!(forums::boards(s, &ann).await.unwrap().len(), 2);
    assert_eq!(
        forums::search(s, &ann, "findable", 0)
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    let _ = AccountId::from_raw(0);
}
