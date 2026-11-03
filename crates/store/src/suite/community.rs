//! The community features against any backend (M3-J, D86): one-liners and the wall (rate limit,
//! roll-off, staff removal), last callers, and polls (one vote each, who may close, when counts
//! show), through the same `execute` a front end calls.
#![allow(clippy::unwrap_used, clippy::panic, clippy::assert_is_empty)]

use russh_bbs_core::access::AccessStore;
use russh_bbs_core::access::{Permission, Slug};
use russh_bbs_core::audit::{Action, AuditEntry, AuditQuery, AuditStore, known_events};
use russh_bbs_core::community::{
    AddLine, CloseOutcome, CommunityStore, CreatePoll, EntryId, LineKind, LineText, NewPoll,
    PollId, PollRules, RemoveOutcome, Request, Response, ResultsVisibility, VoteOutcome, execute,
    keys,
};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStateStore, AccountStore, NewAccount, StoreError,
    UnixSeconds,
};

use super::{Harness, TestStore};

async fn member<S: AccountStore>(
    store: &S,
    name: &str,
    role: Slug,
    state: AccountState,
) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role,
            state,
        })
        .await
        .unwrap()
}

async fn active<S: AccountStore>(store: &S, name: &str) -> Account {
    member(store, name, Slug::MEMBER, AccountState::Active).await
}

fn at(seconds: i64) -> UnixSeconds {
    UnixSeconds::from_raw(seconds)
}

fn text(kind: LineKind, line: &str) -> LineText {
    LineText::parse(kind, line).unwrap()
}

async fn audit_rows<S: AuditStore>(store: &S, action: Action) -> Vec<AuditEntry> {
    known_events(
        store
            .page(&AuditQuery {
                action: Some(action),
                limit: 100,
                ..AuditQuery::default()
            })
            .await
            .unwrap(),
    )
}

fn poll(question: &str, options: &[&str], visibility: ResultsVisibility) -> NewPoll {
    let options: Vec<String> = options.iter().map(|o| (*o).to_owned()).collect();
    NewPoll::build(question, &options, "", visibility).unwrap()
}

const RULES: PollRules = PollRules {
    max_polls: 90,
    per_day: 3,
};

async fn new_poll<S: CommunityStore>(s: &S, who: AccountId, n: i64) -> PollId {
    let poll = poll(
        &format!("Question {n}?"),
        &["yes", "no"],
        ResultsVisibility::Always,
    );
    match s.create_poll(who, &poll, RULES, at(n)).await.unwrap() {
        CreatePoll::Created(id) => id,
        other => panic!("expected a poll, got {other:?}"),
    }
}

pub(crate) async fn lines_come_back_newest_first_and_roll_off_at_the_list_size<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let bob = active(s, "bob").await;
    assert!(
        s.recent_lines(LineKind::Oneliner, 50)
            .await
            .unwrap()
            .is_empty()
    );
    // Two members add 20 lines at 5 an hour each: 4 hours apart so the limit never bites.
    for n in 0..20_i64 {
        let who = if n % 2 == 0 { ann.id } else { bob.id };
        let added = s
            .add_line(
                who,
                LineKind::Oneliner,
                &text(LineKind::Oneliner, &format!("line {n}")),
                at(n * 3_600),
            )
            .await
            .unwrap();
        assert!(matches!(added, AddLine::Added(_)));
    }
    let kept = s.recent_lines(LineKind::Oneliner, 50).await.unwrap();
    assert_eq!(
        kept.len(),
        LineKind::Oneliner.keep(),
        "the oldest rolled off"
    );
    assert_eq!(kept[0].text, "line 19");
    assert_eq!(kept[0].author, "bob");
    assert_eq!(kept.last().unwrap().text, "line 5");
    // The wall is its own list with its own size.
    for n in 0..15_i64 {
        s.add_line(
            ann.id,
            LineKind::Graffiti,
            &text(LineKind::Graffiti, &format!("wall {n}")),
            at(n * 3_600),
        )
        .await
        .unwrap();
    }
    let wall = s.recent_lines(LineKind::Graffiti, 50).await.unwrap();
    assert_eq!(wall.len(), LineKind::Graffiti.keep());
    assert_eq!(wall[0].text, "wall 14");
    assert_eq!(
        s.recent_lines(LineKind::Oneliner, 50).await.unwrap().len(),
        LineKind::Oneliner.keep(),
        "the wall did not touch the one-liners"
    );
    // A smaller limit shows the newest.
    assert_eq!(
        s.recent_lines(LineKind::Oneliner, 3).await.unwrap().len(),
        3
    );
}

pub(crate) async fn the_hourly_limit_refuses_the_next_line_and_resets_with_the_hour<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let bob = active(s, "bob").await;
    let limit = LineKind::Oneliner.per_hour();
    for n in 0..limit {
        let added = s
            .add_line(
                ann.id,
                LineKind::Oneliner,
                &text(LineKind::Oneliner, "hi"),
                at(10_000 + i64::try_from(n).unwrap()),
            )
            .await
            .unwrap();
        assert!(matches!(added, AddLine::Added(_)), "line {n}");
    }
    let refused = s
        .add_line(
            ann.id,
            LineKind::Oneliner,
            &text(LineKind::Oneliner, "one too many"),
            at(10_100),
        )
        .await
        .unwrap();
    assert_eq!(
        refused,
        AddLine::RateLimited {
            retry_at: at(10_000 + 3_600)
        },
        "told when the oldest counted line leaves the hour"
    );
    assert_eq!(
        s.recent_lines(LineKind::Oneliner, 50).await.unwrap().len(),
        limit
    );
    // The limit is per member and per kind.
    assert!(matches!(
        s.add_line(
            bob.id,
            LineKind::Oneliner,
            &text(LineKind::Oneliner, "mine"),
            at(10_100)
        )
        .await
        .unwrap(),
        AddLine::Added(_)
    ));
    assert!(matches!(
        s.add_line(
            ann.id,
            LineKind::Graffiti,
            &text(LineKind::Graffiti, "wall"),
            at(10_100)
        )
        .await
        .unwrap(),
        AddLine::Added(_)
    ));
    // An hour after the oldest line, one more fits.
    assert!(matches!(
        s.add_line(
            ann.id,
            LineKind::Oneliner,
            &text(LineKind::Oneliner, "later"),
            at(10_000 + 3_601)
        )
        .await
        .unwrap(),
        AddLine::Added(_)
    ));
}

pub(crate) async fn concurrent_adds_never_pass_the_hourly_limit<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let mut tasks = Vec::new();
    for n in 0..12_i64 {
        let s = s.clone();
        tasks.push(tokio::spawn(async move {
            s.add_line(
                ann.id,
                LineKind::Oneliner,
                &text(LineKind::Oneliner, "race"),
                at(500 + n),
            )
            .await
            .unwrap()
        }));
    }
    let mut added = 0;
    for task in tasks {
        if matches!(task.await.unwrap(), AddLine::Added(_)) {
            added += 1;
        }
    }
    assert_eq!(added, LineKind::Oneliner.per_hour());
}

pub(crate) async fn adding_a_line_for_an_unknown_account_fails<H: Harness>() {
    let h = H::fresh().await;
    let result = h
        .store()
        .add_line(
            AccountId::from_raw(999),
            LineKind::Oneliner,
            &text(LineKind::Oneliner, "x"),
            at(1),
        )
        .await;
    assert!(
        matches!(result, Err(StoreError::AccountNotFound)),
        "{result:?}"
    );
    let result = h
        .store()
        .record_sign_in(AccountId::from_raw(999), at(1))
        .await;
    assert!(
        matches!(result, Err(StoreError::AccountNotFound)),
        "{result:?}"
    );
}

pub(crate) async fn removing_a_line_is_audited_and_a_wrong_id_or_kind_is_not_found<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let staff = member(s, "maude", Slug::MODERATOR, AccountState::Active).await;
    let AddLine::Added(one) = s
        .add_line(
            ann.id,
            LineKind::Oneliner,
            &text(LineKind::Oneliner, "remove me"),
            at(1),
        )
        .await
        .unwrap()
    else {
        panic!("not added");
    };
    // Right id, wrong list: nothing happens.
    assert_eq!(
        s.remove_line(staff.id, LineKind::Graffiti, one)
            .await
            .unwrap(),
        RemoveOutcome::NotFound
    );
    assert_eq!(
        s.recent_lines(LineKind::Oneliner, 9).await.unwrap().len(),
        1
    );
    assert!(
        audit_rows(s, Action::CommunityEntryRemoved)
            .await
            .is_empty()
    );
    assert_eq!(
        s.remove_line(staff.id, LineKind::Oneliner, one)
            .await
            .unwrap(),
        RemoveOutcome::Removed
    );
    assert!(
        s.recent_lines(LineKind::Oneliner, 9)
            .await
            .unwrap()
            .is_empty()
    );
    let rows = audit_rows(s, Action::CommunityEntryRemoved).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].event.actor,
        russh_bbs_core::audit::Actor::Account(staff.id)
    );
    assert_eq!(
        rows[0].event.target,
        russh_bbs_core::audit::Target::Account(ann.id)
    );
    assert_eq!(
        rows[0].event.detail.as_str(),
        format!("oneliner {}", one.as_raw())
    );
    assert!(
        !rows[0].event.detail.as_str().contains("remove me"),
        "the text is not logged"
    );
    assert_eq!(
        s.remove_line(staff.id, LineKind::Oneliner, EntryId::from_raw(12_345))
            .await
            .unwrap(),
        RemoveOutcome::NotFound
    );
    // A removal never lands without its audit row.
    let AddLine::Added(two) = s
        .add_line(
            ann.id,
            LineKind::Oneliner,
            &text(LineKind::Oneliner, "keep me"),
            at(2),
        )
        .await
        .unwrap()
    else {
        panic!("not added");
    };
    h.break_audit_inserts().await;
    assert!(
        s.remove_line(staff.id, LineKind::Oneliner, two)
            .await
            .is_err()
    );
    assert_eq!(
        s.recent_lines(LineKind::Oneliner, 9).await.unwrap().len(),
        1,
        "it rolled back"
    );
}

pub(crate) async fn last_callers_are_active_members_newest_first_one_row_each<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let bob = active(s, "bob").await;
    let cy = member(s, "cy", Slug::MEMBER, AccountState::Pending).await;
    let dee = member(s, "dee", Slug::MEMBER, AccountState::Suspended).await;
    assert!(s.last_callers(10).await.unwrap().is_empty());
    s.record_sign_in(ann.id, at(100)).await.unwrap();
    s.record_sign_in(bob.id, at(200)).await.unwrap();
    s.record_sign_in(cy.id, at(300)).await.unwrap();
    s.record_sign_in(dee.id, at(400)).await.unwrap();
    s.record_sign_in(ann.id, at(500)).await.unwrap();
    let rows = s.last_callers(10).await.unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.handle.as_str()).collect();
    assert_eq!(
        names,
        ["ann", "bob"],
        "newest first; one row per member; only active accounts"
    );
    assert_eq!(rows[0].at, at(500));
    assert_eq!(
        s.last_callers(1).await.unwrap().len(),
        1,
        "bounded by the limit"
    );
    // A member suspended later drops out.
    s.change_state(
        ann.id,
        AccountState::Suspended,
        russh_bbs_core::audit::Actor::System,
    )
    .await
    .unwrap();
    assert_eq!(s.last_callers(10).await.unwrap().len(), 1);
}

pub(crate) async fn a_poll_round_trips_with_its_options_and_setting<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let bob = active(s, "bob").await;
    let options: Vec<String> = ["red", "green", "blue"]
        .iter()
        .map(|o| (*o).to_owned())
        .collect();
    let described = NewPoll::build(
        "Best colour?",
        &options,
        "Pick the one you like.\nSecond line.",
        ResultsVisibility::AfterClose,
    )
    .unwrap();
    let CreatePoll::Created(id) = s
        .create_poll(ann.id, &described, RULES, at(50))
        .await
        .unwrap()
    else {
        panic!("not created");
    };
    let stored = s.poll(id, bob.id).await.unwrap().unwrap();
    assert_eq!(stored.question, "Best colour?");
    assert_eq!(stored.description, "Pick the one you like.\nSecond line.");
    assert_eq!(stored.options, ["red", "green", "blue"]);
    assert_eq!(stored.visibility, "after_close");
    assert_eq!(stored.creator, ann.id);
    assert_eq!(stored.creator_handle, "ann");
    assert_eq!(stored.my_vote, None);
    assert_eq!(stored.closed_at, None);
    assert!(
        s.poll(PollId::from_raw(777), bob.id)
            .await
            .unwrap()
            .is_none()
    );
    let (rows, total) = s.list_polls(0, 9).await.unwrap();
    assert_eq!((rows.len(), total), (1, 1));
    assert!(!rows[0].closed);
    assert_eq!(rows[0].votes, 0);
}

pub(crate) async fn a_member_votes_once_and_the_second_vote_is_refused<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let bob = active(s, "bob").await;
    let id = new_poll(s, ann.id, 1).await;
    assert_eq!(
        s.vote(id, bob.id, 1, at(10)).await.unwrap(),
        VoteOutcome::Voted
    );
    // Same option, or another: refused, and the count does not move.
    assert_eq!(
        s.vote(id, bob.id, 1, at(11)).await.unwrap(),
        VoteOutcome::AlreadyVoted
    );
    assert_eq!(
        s.vote(id, bob.id, 0, at(12)).await.unwrap(),
        VoteOutcome::AlreadyVoted
    );
    assert_eq!(s.tally(id).await.unwrap(), [0, 1]);
    assert_eq!(s.poll(id, bob.id).await.unwrap().unwrap().my_vote, Some(1));
    assert_eq!(s.poll(id, ann.id).await.unwrap().unwrap().my_vote, None);
    // Another member, an option the poll lacks, and a poll that is not there.
    assert_eq!(
        s.vote(id, ann.id, 0, at(13)).await.unwrap(),
        VoteOutcome::Voted
    );
    assert_eq!(
        s.vote(id, ann.id, 5, at(14)).await.unwrap(),
        VoteOutcome::NotFound
    );
    let other = active(s, "cy").await;
    assert_eq!(
        s.vote(id, other.id, 2, at(15)).await.unwrap(),
        VoteOutcome::NotFound
    );
    assert_eq!(
        s.vote(PollId::from_raw(404), other.id, 0, at(16))
            .await
            .unwrap(),
        VoteOutcome::NotFound
    );
    assert_eq!(s.tally(id).await.unwrap(), [1, 1]);
    assert_eq!(s.list_polls(0, 9).await.unwrap().0[0].votes, 2);
    // The database holds the rule itself, not only the code above.
    let direct = h
        .raw(&format!(
            "INSERT INTO poll_vote (poll_id, account_id, position, voted_at) VALUES ({}, {}, 0, 1)",
            id.as_raw(),
            bob.id.as_raw()
        ))
        .await;
    assert!(
        direct.is_err(),
        "a second row for the same member and poll is refused by the key"
    );
    let direct = h
        .raw(&format!(
            "INSERT INTO poll_vote (poll_id, account_id, position, voted_at) VALUES ({}, {}, 4, 1)",
            id.as_raw(),
            other.id.as_raw()
        ))
        .await;
    assert!(
        direct.is_err(),
        "a vote for an option the poll lacks is refused"
    );
}

pub(crate) async fn concurrent_votes_from_one_member_count_once<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let bob = active(s, "bob").await;
    let id = new_poll(s, ann.id, 1).await;
    let mut tasks = Vec::new();
    for n in 0..8_usize {
        let s = s.clone();
        tasks.push(tokio::spawn(async move {
            s.vote(id, bob.id, n % 2, at(20)).await.unwrap()
        }));
    }
    let mut voted = 0;
    for task in tasks {
        if task.await.unwrap() == VoteOutcome::Voted {
            voted += 1;
        }
    }
    assert_eq!(voted, 1, "exactly one of eight simultaneous votes counts");
    assert_eq!(s.tally(id).await.unwrap().iter().sum::<u64>(), 1);
}

pub(crate) async fn only_the_creator_or_staff_closes_a_poll_and_a_closed_poll_takes_no_votes<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let bob = active(s, "bob").await;
    let staff = member(s, "maude", Slug::MODERATOR, AccountState::Active).await;
    let id = new_poll(s, ann.id, 1).await;
    assert_eq!(
        s.close_poll(bob.id, id, false, at(30)).await.unwrap(),
        CloseOutcome::NotAllowed
    );
    assert!(
        s.poll(id, bob.id)
            .await
            .unwrap()
            .unwrap()
            .closed_at
            .is_none()
    );
    assert_eq!(
        s.close_poll(ann.id, id, false, at(31)).await.unwrap(),
        CloseOutcome::Done
    );
    assert_eq!(
        s.poll(id, bob.id).await.unwrap().unwrap().closed_at,
        Some(at(31))
    );
    assert!(
        audit_rows(s, Action::CommunityPollClosed).await.is_empty(),
        "closing your own poll is not audited"
    );
    assert_eq!(
        s.close_poll(ann.id, id, false, at(32)).await.unwrap(),
        CloseOutcome::AlreadyClosed
    );
    assert_eq!(
        s.vote(id, bob.id, 0, at(33)).await.unwrap(),
        VoteOutcome::Closed
    );
    assert_eq!(s.tally(id).await.unwrap().iter().sum::<u64>(), 0);
    assert!(s.list_polls(0, 9).await.unwrap().0[0].closed);
    // Staff close someone else's poll, and it is audited with the actor and the creator.
    let second = new_poll(s, ann.id, 2).await;
    assert_eq!(
        s.close_poll(staff.id, second, true, at(40)).await.unwrap(),
        CloseOutcome::Done
    );
    let rows = audit_rows(s, Action::CommunityPollClosed).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].event.actor,
        russh_bbs_core::audit::Actor::Account(staff.id)
    );
    assert_eq!(
        rows[0].event.target,
        russh_bbs_core::audit::Target::Account(ann.id)
    );
    assert_eq!(
        s.close_poll(staff.id, PollId::from_raw(404), true, at(41))
            .await
            .unwrap(),
        CloseOutcome::NotFound
    );
}

pub(crate) async fn removing_a_poll_deletes_its_votes_and_is_audited<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let bob = active(s, "bob").await;
    let staff = member(s, "maude", Slug::MODERATOR, AccountState::Active).await;
    let id = new_poll(s, ann.id, 1).await;
    s.vote(id, bob.id, 0, at(5)).await.unwrap();
    assert_eq!(
        s.remove_poll(staff.id, id).await.unwrap(),
        CloseOutcome::Done
    );
    assert!(s.poll(id, bob.id).await.unwrap().is_none());
    assert_eq!(
        h.texts("SELECT CAST(count(*) AS TEXT) FROM poll_vote")
            .await,
        ["0"]
    );
    assert_eq!(
        h.texts("SELECT CAST(count(*) AS TEXT) FROM poll_option")
            .await,
        ["0"]
    );
    let rows = audit_rows(s, Action::CommunityPollRemoved).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].event.target,
        russh_bbs_core::audit::Target::Account(ann.id)
    );
    assert_eq!(
        s.remove_poll(staff.id, id).await.unwrap(),
        CloseOutcome::NotFound
    );
}

pub(crate) async fn poll_creation_is_limited_per_member_per_day_and_in_all<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let bob = active(s, "bob").await;
    for n in 0..3 {
        new_poll(s, ann.id, 1_000 + n).await;
    }
    let fourth = poll("Fourth?", &["a", "b"], ResultsVisibility::Always);
    assert_eq!(
        s.create_poll(ann.id, &fourth, RULES, at(2_000))
            .await
            .unwrap(),
        CreatePoll::RateLimited {
            retry_at: at(1_000 + 86_400)
        }
    );
    assert!(matches!(
        s.create_poll(bob.id, &fourth, RULES, at(2_000))
            .await
            .unwrap(),
        CreatePoll::Created(_)
    ));
    // A day later ann may make another.
    assert!(matches!(
        s.create_poll(ann.id, &fourth, RULES, at(1_000 + 86_401))
            .await
            .unwrap(),
        CreatePoll::Created(_)
    ));
    // The board keeps a fixed number of polls: with the cap at 5 and 5 held, the next is refused.
    let small = PollRules {
        max_polls: 5,
        per_day: 100,
    };
    assert_eq!(
        s.create_poll(bob.id, &fourth, small, at(3_000_000))
            .await
            .unwrap(),
        CreatePoll::BoardFull
    );
    assert_eq!(s.list_polls(0, 9).await.unwrap().1, 5);
    // Pages walk the list newest first.
    let (first, total) = s.list_polls(0, 2).await.unwrap();
    let (second, _) = s.list_polls(2, 2).await.unwrap();
    assert_eq!((first.len(), second.len(), total), (2, 2, 5));
    assert!(first[0].id > first[1].id && first[1].id > second[0].id);
}

pub(crate) async fn concurrent_poll_creation_never_passes_the_day_limit<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let mut tasks = Vec::new();
    for n in 0..8_i64 {
        let s = s.clone();
        tasks.push(tokio::spawn(async move {
            let p = poll(
                &format!("Race {n}?"),
                &["a", "b"],
                ResultsVisibility::Always,
            );
            s.create_poll(ann.id, &p, RULES, at(100 + n)).await.unwrap()
        }));
    }
    let mut created = 0;
    for task in tasks {
        if matches!(task.await.unwrap(), CreatePoll::Created(_)) {
            created += 1;
        }
    }
    assert_eq!(created, RULES.per_day);
    assert_eq!(s.list_polls(0, 9).await.unwrap().1, RULES.per_day);
}

/// A session snapshot for `account`, as the front end would hold it.
async fn snapshot<S: TestStore>(s: &S, account: &Account) -> russh_bbs_core::access::Snapshot {
    s.snapshot(Some(account.id)).await.unwrap()
}

pub(crate) async fn results_show_as_the_polls_setting_says<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = member(s, "ann", Slug::TRUSTED, AccountState::Active).await;
    let bob = active(s, "bob").await;
    let (ann_s, bob_s) = (snapshot(s, &ann).await, snapshot(s, &bob).await);
    let mut ids = Vec::new();
    for (n, v) in [
        ResultsVisibility::Always,
        ResultsVisibility::AfterVote,
        ResultsVisibility::AfterClose,
    ]
    .into_iter()
    .enumerate()
    {
        let created = execute(
            s,
            &ann_s,
            at(10 + i64::try_from(n).unwrap()),
            Request::Create {
                question: format!("Q{n}?"),
                options: vec!["yes".into(), "no".into(), String::new()],
                description: String::new(),
                visibility: v,
            },
        )
        .await;
        let Response::Poll { detail, .. } = created else {
            panic!("not created: {created:?}");
        };
        ids.push(detail.id);
    }
    let counts = |response: Response| match response {
        Response::Poll { detail, .. } => detail.counts,
        other => panic!("not a poll: {other:?}"),
    };
    // Before voting: always shows (zeroes), the other two hide.
    assert_eq!(
        counts(execute(s, &bob_s, at(20), Request::Poll(ids[0])).await),
        Some(vec![0, 0])
    );
    assert_eq!(
        counts(execute(s, &bob_s, at(20), Request::Poll(ids[1])).await),
        None
    );
    assert_eq!(
        counts(execute(s, &bob_s, at(20), Request::Poll(ids[2])).await),
        None
    );
    // After voting: after-vote shows; after-close still hides.
    for id in &ids {
        let voted = execute(s, &bob_s, at(21), Request::Vote(*id, 1)).await;
        assert!(
            matches!(voted, Response::Poll { note: Some(n), .. } if !n.error),
            "{voted:?}"
        );
    }
    assert_eq!(
        counts(execute(s, &bob_s, at(22), Request::Poll(ids[1])).await),
        Some(vec![0, 1])
    );
    assert_eq!(
        counts(execute(s, &bob_s, at(22), Request::Poll(ids[2])).await),
        None
    );
    // The second vote is refused in words, through the same path.
    let again = execute(s, &bob_s, at(23), Request::Vote(ids[0], 0)).await;
    assert!(
        matches!(again, Response::Poll { note: Some(n), .. } if n.error && n.key == keys::NOTE_ALREADY_VOTED),
        "{again:?}"
    );
    // Closing shows the counts to everyone; the creator closes, bob may not.
    let refused = execute(s, &bob_s, at(30), Request::Close(ids[2])).await;
    assert!(
        matches!(refused, Response::Poll { note: Some(n), .. } if n.key == keys::NOTE_POLL_NOT_YOURS),
        "{refused:?}"
    );
    let closed = execute(s, &ann_s, at(31), Request::Close(ids[2])).await;
    assert!(matches!(closed, Response::Poll { .. }));
    let after = counts(execute(s, &bob_s, at(32), Request::Poll(ids[2])).await);
    assert_eq!(after, Some(vec![0, 1]));
    // Voting on the closed poll is refused in words.
    let late = execute(s, &ann_s, at(33), Request::Vote(ids[2], 0)).await;
    assert!(
        matches!(late, Response::Poll { note: Some(n), .. } if n.key == keys::NOTE_POLL_CLOSED),
        "{late:?}"
    );
}

pub(crate) async fn the_permissions_decide_who_can_do_what_through_execute<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let staff = member(s, "maude", Slug::MODERATOR, AccountState::Active).await;
    let (ann_s, staff_s) = (snapshot(s, &ann).await, snapshot(s, &staff).await);
    // A waiting-room snapshot holds none of it.
    let waiting = s.snapshot(None).await.unwrap();
    for request in [
        Request::Lines(LineKind::Oneliner),
        Request::Callers,
        Request::Polls { offset: 0 },
        Request::Post(LineKind::Oneliner, "hello".into()),
    ] {
        assert_eq!(
            execute(s, &waiting, at(1), request).await,
            Response::NotPermitted
        );
    }
    // A member reads and posts, and cannot moderate or create polls.
    let posted = execute(
        s,
        &ann_s,
        at(2),
        Request::Post(LineKind::Oneliner, "hello".into()),
    )
    .await;
    assert!(
        matches!(&posted, Response::Lines { entries, note: Some(n), .. } if entries.len() == 1 && !n.error),
        "{posted:?}"
    );
    let Response::Lines { entries, .. } = posted else {
        panic!()
    };
    let id = entries[0].id;
    assert_eq!(
        execute(s, &ann_s, at(3), Request::Remove(LineKind::Oneliner, id)).await,
        Response::NotPermitted
    );
    let create = Request::Create {
        question: "Q?".into(),
        options: vec!["a".into(), "b".into()],
        description: String::new(),
        visibility: ResultsVisibility::Always,
    };
    assert_eq!(
        execute(s, &ann_s, at(4), create.clone()).await,
        Response::NotPermitted
    );
    // Staff remove, and a made poll can be removed by staff only.
    let removed = execute(s, &staff_s, at(5), Request::Remove(LineKind::Oneliner, id)).await;
    assert!(
        matches!(&removed, Response::Lines { entries, note: Some(n), .. } if entries.is_empty() && !n.error),
        "{removed:?}"
    );
    // Too-long and blank lines are refused in words and store nothing.
    let long = "x".repeat(LineKind::Oneliner.max_chars() + 1);
    let r = execute(s, &ann_s, at(6), Request::Post(LineKind::Oneliner, long)).await;
    assert!(
        matches!(r, Response::Refused(n) if n.key == keys::NOTE_TOO_LONG),
        "{r:?}"
    );
    let r = execute(
        s,
        &ann_s,
        at(7),
        Request::Post(LineKind::Oneliner, "   \u{1b}[31m ".into()),
    )
    .await;
    assert!(
        matches!(r, Response::Refused(_) | Response::Lines { .. }),
        "{r:?}"
    );
    // Taking the permission away takes effect on the next request.
    h.raw("DELETE FROM role_permission WHERE role = 'member' AND permission = 'oneliners.post'")
        .await
        .unwrap();
    let ann_s = snapshot(s, &ann).await;
    assert_eq!(
        execute(
            s,
            &ann_s,
            at(8),
            Request::Post(LineKind::Oneliner, "again".into())
        )
        .await,
        Response::NotPermitted
    );
    let _ = Permission::CommunityView;
}

pub(crate) async fn stored_text_is_sanitized_again_when_read<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let ann_s = snapshot(s, &ann).await;
    // A row written behind the code's back, with an escape sequence and a bell in it.
    h.raw(&format!(
        "INSERT INTO community_line (kind, account_id, body, created_at) \
         VALUES ('oneliner', {}, 'a\u{1b}[31mred\u{7}', 5)",
        ann.id.as_raw()
    ))
    .await
    .unwrap();
    let Response::Lines { entries, .. } =
        execute(s, &ann_s, at(9), Request::Lines(LineKind::Oneliner)).await
    else {
        panic!("not lines");
    };
    assert_eq!(entries.len(), 1);
    assert!(
        !entries[0].text.chars().any(char::is_control),
        "{:?}",
        entries[0].text
    );
    assert!(entries[0].text.contains("red"));
}

pub(crate) async fn the_community_tables_refuse_what_core_would_never_write<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let who = ann.id.as_raw();
    for sql in [
        format!("INSERT INTO community_line (kind, account_id, body, created_at) VALUES ('shout', {who}, 'x', 1)"),
        format!("INSERT INTO community_line (kind, account_id, body, created_at) VALUES ('oneliner', {who}, '', 1)"),
        format!(
            "INSERT INTO community_line (kind, account_id, body, created_at) VALUES ('oneliner', {who}, '{}', 1)",
            "x".repeat(71)
        ),
        format!("INSERT INTO poll (creator_id, question, results, created_at) VALUES ({who}, 'q', 'sometimes', 1)"),
        format!("INSERT INTO poll (creator_id, question, results, created_at) VALUES ({who}, '', 'always', 1)"),
        "INSERT INTO community_line (kind, account_id, body, created_at) VALUES ('oneliner', 424242, 'x', 1)".to_owned(),
    ] {
        assert!(h.raw(&sql).await.is_err(), "accepted: {sql}");
    }
}

pub(crate) async fn deleting_an_account_deletes_its_community_rows<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let bob = active(s, "bob").await;
    s.add_line(
        ann.id,
        LineKind::Oneliner,
        &text(LineKind::Oneliner, "bye"),
        at(1),
    )
    .await
    .unwrap();
    s.record_sign_in(ann.id, at(2)).await.unwrap();
    let id = new_poll(s, ann.id, 3).await;
    s.vote(id, bob.id, 0, at(4)).await.unwrap();
    h.raw(&format!(
        "DELETE FROM accounts WHERE id = {}",
        ann.id.as_raw()
    ))
    .await
    .unwrap();
    assert!(
        s.recent_lines(LineKind::Oneliner, 9)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        h.texts("SELECT CAST(count(*) AS TEXT) FROM last_caller")
            .await,
        ["0"]
    );
    assert_eq!(
        h.texts("SELECT CAST(count(*) AS TEXT) FROM poll").await,
        ["0"],
        "their polls go with them"
    );
    assert_eq!(
        h.texts("SELECT CAST(count(*) AS TEXT) FROM poll_vote")
            .await,
        ["0"]
    );
}

pub(crate) async fn the_roles_hold_the_community_permissions_the_docs_say<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let rows = |role: &'static str| {
        format!(
            "SELECT permission FROM role_permission WHERE role = '{role}' AND permission LIKE 'community.%' ORDER BY 1"
        )
    };
    assert!(
        h.texts(&rows("waiting")).await.is_empty(),
        "the waiting room sees none of it"
    );
    assert_eq!(h.texts(&rows("member")).await, ["community.view"]);
    assert_eq!(h.texts(&rows("trusted")).await, ["community.view"]);
    for role in ["moderator", "cosysop", "sysop"] {
        assert_eq!(
            h.texts(&rows(role)).await,
            ["community.moderate", "community.view"],
            "{role}"
        );
    }
    // Posting and voting stay member permissions; creating a poll starts at trusted.
    let member_perms = |p: &'static str, role: &'static str| {
        format!(
            "SELECT permission FROM role_permission WHERE role = '{role}' AND permission = '{p}'"
        )
    };
    for p in ["oneliners.post", "graffiti.post", "polls.vote"] {
        assert_eq!(h.texts(&member_perms(p, "member")).await.len(), 1, "{p}");
    }
    assert!(
        h.texts(&member_perms("polls.create", "member"))
            .await
            .is_empty()
    );
    assert_eq!(
        h.texts(&member_perms("polls.create", "trusted"))
            .await
            .len(),
        1
    );
    let _ = s;
}

pub(crate) async fn community_data_survives_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    s.add_line(
        ann.id,
        LineKind::Graffiti,
        &text(LineKind::Graffiti, "still here"),
        at(1),
    )
    .await
    .unwrap();
    s.record_sign_in(ann.id, at(2)).await.unwrap();
    let id = new_poll(s, ann.id, 3).await;
    s.vote(id, ann.id, 1, at(4)).await.unwrap();
    h.reopen().await;
    let s = h.store();
    assert_eq!(
        s.recent_lines(LineKind::Graffiti, 9).await.unwrap()[0].text,
        "still here"
    );
    assert_eq!(s.last_callers(9).await.unwrap().len(), 1);
    assert_eq!(s.tally(id).await.unwrap(), [0, 1]);
}

pub(crate) async fn a_broken_store_errors_instead_of_pretending_the_lists_are_empty<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let ann_s = snapshot(s, &ann).await;
    h.close().await;
    assert!(s.recent_lines(LineKind::Oneliner, 9).await.is_err());
    assert!(s.last_callers(9).await.is_err());
    assert!(s.list_polls(0, 9).await.is_err());
    assert_eq!(
        execute(s, &ann_s, at(1), Request::Callers).await,
        Response::Failed
    );
}
