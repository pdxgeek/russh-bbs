//! Widgets against any backend (UI-10): a member's per-layout lists round-trip, and what the
//! widgets fetch never includes a board, poll, mail or ledger the viewer cannot reach.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::too_many_lines
)]

use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::community::{
    CommunityStore, CreatePoll, LineKind, LineText, NewPoll, PollRules, Request, ResultsVisibility,
    execute,
};
use russh_bbs_core::credits::{Amount, Movement, post};
use russh_bbs_core::drafts::Body;
use russh_bbs_core::feeds::{self, FeedStore};
use russh_bbs_core::forums::{self, body_limits};
use russh_bbs_core::mail::{self, KeyId, MailKey, MailKeys};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, StoreError, UnixSeconds,
};
use russh_bbs_core::terminal_settings::{LayoutId, MemberSessions};
use russh_bbs_core::widgets::{self, Choice, SERIES_DAYS, WidgetData, WidgetId, WidgetStore};

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

async fn list<S: WidgetStore>(s: &S, who: AccountId) -> Choice {
    s.widget_choice(who).await.unwrap()
}

// ---- the stored lists -----------------------------------------------------------------------

pub(crate) async fn lists_round_trip_in_order_and_none_follows_the_default<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = make(s, "ann", Slug::MEMBER).await;
    let bob = make(s, "bob", Slug::MEMBER).await;
    assert_eq!(list(s, ann.id).await, Choice::new(), "no row, no choice");

    let dash = [WidgetId::Mail, WidgetId::Clock, WidgetId::NewPosts];
    s.set_widget_list(ann.id, LayoutId::Dashboard, Some(&dash))
        .await
        .unwrap();
    assert_eq!(
        list(s, ann.id).await.get(LayoutId::Dashboard),
        Some(&dash[..])
    );
    assert_eq!(list(s, ann.id).await.get(LayoutId::Wide), None);

    // All off is a row with no widgets: not the same as following the default.
    s.set_widget_list(ann.id, LayoutId::Wide, Some(&[]))
        .await
        .unwrap();
    assert_eq!(list(s, ann.id).await.get(LayoutId::Wide), Some(&[][..]));

    // A later save replaces; every layout has its own row; other accounts are untouched.
    let reordered = [WidgetId::NewPosts, WidgetId::Mail];
    s.set_widget_list(ann.id, LayoutId::Dashboard, Some(&reordered))
        .await
        .unwrap();
    for layout in LayoutId::ALL {
        s.set_widget_list(ann.id, layout, Some(&WidgetId::ALL[..6]))
            .await
            .unwrap();
    }
    let all = list(s, ann.id).await;
    for layout in LayoutId::ALL {
        assert_eq!(all.get(layout), Some(&WidgetId::ALL[..6]), "{layout:?}");
    }
    assert_eq!(list(s, bob.id).await, Choice::new());

    // `None` goes back to the default, for that layout only.
    s.set_widget_list(ann.id, LayoutId::Wide, None)
        .await
        .unwrap();
    let after = list(s, ann.id).await;
    assert_eq!(after.get(LayoutId::Wide), None);
    assert!(after.get(LayoutId::Dashboard).is_some());
    s.set_widget_list(ann.id, LayoutId::Wide, None)
        .await
        .unwrap();
}

pub(crate) async fn a_save_for_an_unknown_account_fails_and_a_delete_takes_the_lists<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ghost = AccountId::from_raw(4242);
    assert!(matches!(
        s.set_widget_list(ghost, LayoutId::Wide, Some(&[WidgetId::Clock]))
            .await,
        Err(StoreError::AccountNotFound)
    ));
    assert_eq!(list(s, ghost).await, Choice::new());

    let ann = make(s, "ann", Slug::MEMBER).await;
    s.set_widget_list(ann.id, LayoutId::Wide, Some(&[WidgetId::Clock]))
        .await
        .unwrap();
    assert_eq!(
        h.texts("SELECT layout FROM widget_list").await,
        ["wide".to_owned()]
    );
    // Deleting the account (a hand-run delete here: `members.delete_data` is not built) cascades.
    h.raw(&format!(
        "DELETE FROM accounts WHERE id = {}",
        ann.id.as_raw()
    ))
    .await
    .unwrap();
    assert!(h.texts("SELECT layout FROM widget_list").await.is_empty());
}

pub(crate) async fn a_stored_list_the_code_could_not_have_written_is_refused_by_the_database<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = make(s, "ann", Slug::MEMBER).await;
    let id = ann.id.as_raw();
    for (layout, widgets) in [
        ("sideways", "clock"),
        ("wide", "Clock"),
        ("wide", "clock;drop"),
        ("wide", &"a".repeat(401)),
    ] {
        let sql = format!(
            "INSERT INTO widget_list (account_id, layout, widgets) VALUES ({id}, '{layout}', '{widgets}')"
        );
        assert!(h.raw(&sql).await.is_err(), "{layout} {widgets}");
    }
    // An id a newer build wrote is skipped, not an error.
    h.raw(&format!(
        "INSERT INTO widget_list (account_id, layout, widgets) VALUES ({id}, 'wide', 'clock,from_the_future,mail')"
    ))
    .await
    .unwrap();
    assert_eq!(
        list(s, ann.id).await.get(LayoutId::Wide),
        Some(&[WidgetId::Clock, WidgetId::Mail][..])
    );
}

pub(crate) async fn a_save_writes_one_audit_row_and_no_widget_names<H: Harness>() {
    use russh_bbs_core::audit::{Action, AuditQuery, AuditStore, known_events};
    let h = H::fresh().await;
    let s = h.store();
    let ann = make(s, "ann", Slug::MEMBER).await;
    s.set_widget_list(ann.id, LayoutId::Dashboard, Some(&[WidgetId::Mail]))
        .await
        .unwrap();
    let rows = known_events(
        s.page(&AuditQuery {
            action: Some(Action::SettingChanged),
            limit: 10,
            ..AuditQuery::default()
        })
        .await
        .unwrap(),
    );
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event.detail.as_str(), "widget placement");
}

// ---- what a widget may show -----------------------------------------------------------------

const RULES: PollRules = PollRules {
    max_polls: 90,
    per_day: 3,
};

fn everything() -> Vec<WidgetId> {
    WidgetId::ALL.to_vec()
}

/// A snapshot holding exactly `permissions`, globally.
fn holding(who: &Account, permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(who.id),
        &Slug::MEMBER,
        10,
        &Via::Account,
        permissions.iter().copied(),
    )
}

/// The same data with the fetch time cleared, to compare "nothing at all".
fn nothing(mut data: WidgetData) -> WidgetData {
    data.at = None;
    data
}

pub(crate) async fn a_viewer_sees_nothing_from_a_board_they_cannot_read<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let sysop = make(s, "ops", Slug::SYSOP).await;
    let ann = make(s, "ann", Slug::MEMBER).await;
    let bob = make(s, "bob", Slug::MEMBER).await;
    let sys = snap(s, &sysop).await;
    let open = forums::create_board(s, &sys, at(0), "General", "Talk", false)
        .await
        .unwrap();
    let staff = forums::create_board(s, &sys, at(0), "Staff room", "Keys", true)
        .await
        .unwrap();
    forums::allow_member(s, &sys, staff, "ann").await.unwrap();
    forums::post_thread(s, &sys, at(10), open, "Welcome all", &body("hello"))
        .await
        .unwrap();
    forums::post_thread(s, &sys, at(20), staff, "Pager rota secret", &body("who"))
        .await
        .unwrap();
    // A feed board everyone may read, with items.
    let added = feeds::add_feed(
        s,
        &sys,
        at(0),
        "https://example.org/rss",
        "Wire",
        false,
        None,
    )
    .await
    .unwrap();
    let xml = "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>t</title>\
        <item><title>Quokka found</title><guid>g1</guid><link>https://example.org/1</link>\
        <description>x</description></item></channel></rss>";
    let items = feeds::parse(xml.as_bytes(), at(0)).unwrap().items;
    s.ingest(added.feed, &items, at(30), 50).await.unwrap();

    let wanted = [WidgetId::NewPosts, WidgetId::Activity, WidgetId::News];
    let now = at(100);
    let ann_data = widgets::fetch(s, &snap(s, &ann).await, &wanted, now, Some(at(-5))).await;
    let bob_data = widgets::fetch(s, &snap(s, &bob).await, &wanted, now, Some(at(-5))).await;

    let ann_posts = ann_data.new_posts.clone().unwrap();
    let bob_posts = bob_data.new_posts.clone().unwrap();
    assert!(
        ann_posts
            .boards
            .iter()
            .any(|(name, _)| name == "Staff room")
    );
    assert!(
        !bob_posts
            .boards
            .iter()
            .any(|(name, _)| name == "Staff room")
    );
    assert_eq!(
        ann_posts.threads,
        bob_posts.threads + 1,
        "ann reads one more"
    );
    assert_eq!(
        ann_data.activity.as_ref().unwrap().iter().sum::<u64>(),
        bob_data.activity.as_ref().unwrap().iter().sum::<u64>() + 1
    );
    assert_eq!(bob_data.activity.as_ref().unwrap().len(), SERIES_DAYS);
    // Nothing of the restricted board, in any field, reaches bob.
    let shown = format!("{bob_data:?}");
    assert!(!shown.contains("Staff room") && !shown.contains("Pager rota secret"));
    // The feed board is readable by both and shows as a headline.
    for data in [&ann_data, &bob_data] {
        let news = data.news.as_ref().unwrap();
        assert!(news.iter().any(|h| h.board == "Wire"), "{news:?}");
    }
    // A viewer who can list no board at all gets nothing from the forums.
    let blind = holding(&bob, &[Permission::AccountEditSettings]);
    let blind_data = widgets::fetch(s, &blind, &wanted, now, None).await;
    assert_eq!(nothing(blind_data), WidgetData::default());
}

pub(crate) async fn community_widgets_need_community_view_and_polls_keep_their_visibility<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = make(s, "ann", Slug::MEMBER).await;
    let bob = make(s, "bob", Slug::MEMBER).await;
    s.add_line(
        ann.id,
        LineKind::Oneliner,
        &LineText::parse(LineKind::Oneliner, "hello there").unwrap(),
        at(5),
    )
    .await
    .unwrap();
    s.add_line(
        ann.id,
        LineKind::Graffiti,
        &LineText::parse(LineKind::Graffiti, "ann was here").unwrap(),
        at(6),
    )
    .await
    .unwrap();
    s.record_sign_in(ann.id, at(7)).await.unwrap();
    let question = NewPoll::build(
        "Tea or coffee?",
        &["tea".to_owned(), "coffee".to_owned()],
        "",
        ResultsVisibility::AfterVote,
    )
    .unwrap();
    let CreatePoll::Created(poll_id) = s
        .create_poll(ann.id, &question, RULES, at(8))
        .await
        .unwrap()
    else {
        panic!("poll not created");
    };

    let wanted = [
        WidgetId::Callers,
        WidgetId::Oneliner,
        WidgetId::Wall,
        WidgetId::Poll,
    ];
    let now = at(100);
    // A session without community.view is shown none of it.
    let no_view = holding(
        &bob,
        &[Permission::ForumList, Permission::AccountEditSettings],
    );
    let data = widgets::fetch(s, &no_view, &wanted, now, None).await;
    assert_eq!(nothing(data), WidgetData::default());

    // With it: the lines, the caller and the poll, but the counts wait for bob's vote.
    let bob_snap = snap(s, &bob).await;
    let data = widgets::fetch(s, &bob_snap, &wanted, now, None).await;
    assert_eq!(data.callers.unwrap()[0].handle, "ann");
    assert_eq!(data.oneliners.unwrap()[0].text, "hello there");
    assert_eq!(data.wall.unwrap()[0].text, "ann was here");
    let before = data.poll.unwrap();
    assert_eq!(before.question, "Tea or coffee?");
    assert!(before.options.iter().all(|(_, votes)| votes.is_none()));
    assert_eq!(before.my_vote, None);

    execute(s, &bob_snap, now, Request::Vote(poll_id, 1)).await;
    let after = widgets::fetch(s, &bob_snap, &[WidgetId::Poll], now, None)
        .await
        .poll
        .unwrap();
    assert_eq!(after.my_vote, Some(1));
    assert_eq!(
        after.options.iter().map(|(_, v)| *v).collect::<Vec<_>>(),
        [Some(0), Some(1)]
    );
}

pub(crate) async fn mail_and_credits_are_only_the_viewers_own<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = make(s, "ann", Slug::MEMBER).await;
    let bob = make(s, "bob", Slug::MEMBER).await;
    let keys = MailKeys::single(KeyId::FIRST, MailKey::generate().unwrap());
    let ann_snap = snap(s, &ann).await;
    mail::send(s, &keys, &ann_snap, at(1), "bob", "Hi", &body("hello"))
        .await
        .unwrap();
    post(
        s,
        ann.id,
        Movement::Earn(Amount::new(7).unwrap()),
        &Slug::parse("test").unwrap(),
    )
    .await
    .unwrap();
    post(
        s,
        bob.id,
        Movement::Earn(Amount::new(3).unwrap()),
        &Slug::parse("test").unwrap(),
    )
    .await
    .unwrap();

    let wanted = [WidgetId::Mail, WidgetId::Credits];
    let now = at(100);
    let ann_data = widgets::fetch(s, &ann_snap, &wanted, now, None).await;
    let bob_data = widgets::fetch(s, &snap(s, &bob).await, &wanted, now, None).await;
    assert_eq!(ann_data.mail.unwrap().unread, 0, "ann sent it, bob got it");
    let bob_mail = bob_data.mail.unwrap();
    assert_eq!((bob_mail.total, bob_mail.unread), (1, 1));
    assert_eq!(ann_data.credits.as_ref().unwrap().balance, 7);
    assert_eq!(bob_data.credits.as_ref().unwrap().balance, 3);
    assert_eq!(bob_data.credits.unwrap().series.len(), SERIES_DAYS);

    // Without mail.read there is no mail count at all.
    let no_mail = holding(&bob, &[Permission::AccountEditSettings]);
    let data = widgets::fetch(s, &no_mail, &wanted, now, None).await;
    assert_eq!(data.mail, None);
}

pub(crate) async fn the_waiting_room_is_shown_no_widget_data<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let sysop = make(s, "ops", Slug::SYSOP).await;
    let ann = make(s, "ann", Slug::MEMBER).await;
    let sys = snap(s, &sysop).await;
    let board = forums::create_board(s, &sys, at(0), "General", "Talk", false)
        .await
        .unwrap();
    forums::post_thread(s, &sys, at(1), board, "Welcome", &body("hi"))
        .await
        .unwrap();
    s.record_sign_in(ann.id, at(2)).await.unwrap();
    s.add_line(
        ann.id,
        LineKind::Oneliner,
        &LineText::parse(LineKind::Oneliner, "hello").unwrap(),
        at(3),
    )
    .await
    .unwrap();
    post(
        s,
        ann.id,
        Movement::Earn(Amount::new(5).unwrap()),
        &Slug::parse("test").unwrap(),
    )
    .await
    .unwrap();

    let waiting = s.access_for(None).await.unwrap();
    assert!(waiting.account().is_none());
    for id in WidgetId::ALL {
        assert!(!id.reachable_by(&waiting), "{id:?}");
    }
    let data = widgets::fetch(s, &waiting, &everything(), at(100), None).await;
    assert_eq!(nothing(data), WidgetData::default());
    let empty = widgets::fetch(s, &Snapshot::empty(), &everything(), at(100), None).await;
    assert_eq!(nothing(empty), WidgetData::default());
}
