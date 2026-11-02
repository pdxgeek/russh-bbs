//! Limits, overrides and usage counters against any backend (M2-H, D62).
#![allow(clippy::unwrap_used, clippy::panic)]

use russh_bbs_core::access::{AccessStore, Permission, Slug};
use russh_bbs_core::audit::{Action, Actor, AuditQuery, AuditStore, Target};
use russh_bbs_core::limits::{
    Day, Limit, LimitKind, LimitStore, LimitWriteError, ManualClock, SessionCounter, UsageOutcome,
    begin_member_session, load_effective,
};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, UnixSeconds,
};

use super::Harness;

async fn active<S: AccountStore>(store: &S, name: &str) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

async fn role_limit<S: LimitStore>(store: &S, role: &Slug, kind: LimitKind) -> Option<Limit> {
    store
        .role_limits(std::slice::from_ref(role))
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.kind == kind)
        .map(|row| row.limit)
}

const MIN: LimitKind = LimitKind::MinutesPerDay;

/// The seed values the migration ships, as listed in `docs/access.md`.
pub(crate) async fn the_built_in_roles_have_the_documented_defaults<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let expect = |role: &Slug, kind, want| {
        let role = role.clone();
        async move {
            assert_eq!(
                role_limit(s, &role, kind).await,
                Some(want),
                "{} {}",
                role.as_str(),
                LimitKind::name(kind)
            );
        }
    };
    let n = Limit::AtMost;
    expect(&Slug::MEMBER, MIN, n(60)).await;
    expect(&Slug::MEMBER, LimitKind::CallsPerDay, n(5)).await;
    expect(&Slug::MEMBER, LimitKind::MaxSessions, n(2)).await;
    expect(&Slug::MEMBER, LimitKind::InvitesPerMonth, n(2)).await;
    expect(&Slug::TRUSTED, MIN, n(120)).await;
    expect(&Slug::MODERATOR, MIN, n(240)).await;
    expect(&Slug::COSYSOP, MIN, Limit::Unlimited).await;
    expect(&Slug::COSYSOP, LimitKind::MaxSessions, n(8)).await;
    expect(&Slug::SYSOP, MIN, Limit::Unlimited).await;
}

/// Every built-in role but `waiting` has a value for every kind, and each layer is at least as
/// generous as the one below it (so promoting someone never tightens a limit).
pub(crate) async fn every_role_has_every_kind_and_the_layers_do_not_tighten<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let layers = [
        Slug::MEMBER,
        Slug::TRUSTED,
        Slug::MODERATOR,
        Slug::COSYSOP,
        Slug::SYSOP,
    ];
    for role in &layers {
        let rows = s.role_limits(std::slice::from_ref(role)).await.unwrap();
        assert_eq!(rows.len(), LimitKind::ALL.len(), "{}", role.as_str());
    }
    assert!(
        s.role_limits(&[Slug::WAITING]).await.unwrap().is_empty(),
        "waiting never signs in"
    );
    for &kind in LimitKind::ALL {
        for pair in layers.windows(2) {
            let lower = role_limit(s, &pair[0], kind).await.unwrap();
            let higher = role_limit(s, &pair[1], kind).await.unwrap();
            assert!(
                lower <= higher,
                "{} should not exceed {} for {}",
                pair[0].as_str(),
                pair[1].as_str(),
                kind.name()
            );
        }
    }
}

/// `limits.edit` is held by `sysop` and `cosysop` and nobody else.
pub(crate) async fn limits_edit_is_held_by_sysop_and_cosysop_only<H: Harness>() {
    let h = H::fresh().await;
    for role in h.store().roles().await.unwrap() {
        let holds = role.permissions.contains(&Permission::LimitsEdit);
        let should = role.name == Slug::SYSOP || role.name == Slug::COSYSOP;
        assert_eq!(holds, should, "{}", role.name.as_str());
    }
}

pub(crate) async fn role_limits_can_be_replaced_and_unlimited_is_a_real_value<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    s.set_role_limit(&Slug::MEMBER, MIN, Limit::AtMost(15), Actor::System)
        .await
        .unwrap();
    assert_eq!(
        role_limit(s, &Slug::MEMBER, MIN).await,
        Some(Limit::AtMost(15))
    );
    s.set_role_limit(&Slug::MEMBER, MIN, Limit::Unlimited, Actor::System)
        .await
        .unwrap();
    assert_eq!(
        role_limit(s, &Slug::MEMBER, MIN).await,
        Some(Limit::Unlimited)
    );
    s.set_role_limit(&Slug::MEMBER, MIN, Limit::AtMost(0), Actor::System)
        .await
        .unwrap();
    assert_eq!(
        role_limit(s, &Slug::MEMBER, MIN).await,
        Some(Limit::AtMost(0))
    );
    let missing = Slug::parse("no-such-role").unwrap();
    assert!(matches!(
        s.set_role_limit(&missing, MIN, Limit::AtMost(1), Actor::System)
            .await,
        Err(LimitWriteError::UnknownRole)
    ));
}

pub(crate) async fn member_overrides_set_clear_and_isolate<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let (a, b) = (active(s, "ann").await, active(s, "bob").await);
    s.set_member_limit(a.id, MIN, Some(Limit::AtMost(5)), Actor::System)
        .await
        .unwrap();
    s.set_member_limit(a.id, MIN, Some(Limit::AtMost(7)), Actor::System)
        .await
        .unwrap();
    s.set_member_limit(
        a.id,
        LimitKind::MailPerDay,
        Some(Limit::Unlimited),
        Actor::System,
    )
    .await
    .unwrap();
    assert_eq!(s.member_limits(a.id).await.unwrap().len(), 2);
    assert_eq!(s.member_limits(b.id).await.unwrap().len(), 0);
    let mine = s.member_limits(a.id).await.unwrap();
    assert!(
        mine.iter()
            .any(|m| m.kind == MIN && m.limit == Limit::AtMost(7))
    );
    s.set_member_limit(a.id, MIN, None, Actor::System)
        .await
        .unwrap();
    s.set_member_limit(a.id, MIN, None, Actor::System)
        .await
        .unwrap();
    assert_eq!(s.member_limits(a.id).await.unwrap().len(), 1);
    assert!(matches!(
        s.set_member_limit(
            AccountId::from_raw(9_999),
            MIN,
            Some(Limit::AtMost(1)),
            Actor::System
        )
        .await,
        Err(LimitWriteError::UnknownAccount)
    ));
}

/// The override beats the roles in the real store, end to end through `load_effective`.
pub(crate) async fn an_override_changes_the_effective_limit<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let a = active(s, "ann").await;
    let snapshot = s.snapshot(Some(a.id)).await.unwrap();
    assert_eq!(
        load_effective(s, a.id, &snapshot).await.unwrap().get(MIN),
        Limit::AtMost(60)
    );
    s.set_member_limit(a.id, MIN, Some(Limit::AtMost(5)), Actor::System)
        .await
        .unwrap();
    assert_eq!(
        load_effective(s, a.id, &snapshot).await.unwrap().get(MIN),
        Limit::AtMost(5)
    );
}

pub(crate) async fn usage_accumulates_per_account_kind_and_day<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let (a, b) = (active(s, "ann").await, active(s, "bob").await);
    let day = Day::from_raw(20_000);
    assert_eq!(s.usage(a.id, MIN, day).await.unwrap(), 0);
    assert_eq!(
        s.add_usage(a.id, MIN, day, 30, None).await.unwrap(),
        UsageOutcome::Added(30)
    );
    assert_eq!(
        s.add_usage(a.id, MIN, day, 12, None).await.unwrap(),
        UsageOutcome::Added(42)
    );
    assert_eq!(s.usage(a.id, MIN, day).await.unwrap(), 42);
    assert_eq!(s.usage(b.id, MIN, day).await.unwrap(), 0, "other account");
    assert_eq!(
        s.usage(a.id, LimitKind::CallsPerDay, day).await.unwrap(),
        0,
        "other kind"
    );
    assert_eq!(
        s.usage(a.id, MIN, Day::from_raw(20_001)).await.unwrap(),
        0,
        "other day"
    );
    assert!(matches!(
        s.add_usage(AccountId::from_raw(9_999), MIN, day, 1, None)
            .await,
        Err(russh_bbs_core::store::StoreError::AccountNotFound)
    ));
}

pub(crate) async fn a_capped_add_stops_at_the_cap_and_changes_nothing_when_full<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let a = active(s, "ann").await;
    let (day, kind) = (Day::from_raw(20_000), LimitKind::CallsPerDay);
    for n in 1..=3 {
        assert_eq!(
            s.add_usage(a.id, kind, day, 1, Some(3)).await.unwrap(),
            UsageOutcome::Added(n)
        );
    }
    assert_eq!(
        s.add_usage(a.id, kind, day, 1, Some(3)).await.unwrap(),
        UsageOutcome::Full(3)
    );
    assert_eq!(s.usage(a.id, kind, day).await.unwrap(), 3);
    assert_eq!(
        s.add_usage(a.id, kind, day, 1, Some(0)).await.unwrap(),
        UsageOutcome::Full(3)
    );
    // A first add that is already over the cap leaves a zero row, not a partial count.
    let fresh = Day::from_raw(20_005);
    assert_eq!(
        s.add_usage(a.id, kind, fresh, 5, Some(3)).await.unwrap(),
        UsageOutcome::Full(0)
    );
    assert_eq!(s.usage(a.id, kind, fresh).await.unwrap(), 0);
}

pub(crate) async fn concurrent_capped_adds_never_pass_the_cap<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let a = active(s, "ann").await;
    let (day, kind) = (Day::from_raw(20_000), LimitKind::CallsPerDay);
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let store = s.clone();
        let id = a.id;
        tasks.push(tokio::spawn(async move {
            store.add_usage(id, kind, day, 1, Some(5)).await.unwrap()
        }));
    }
    let mut added = 0;
    for task in tasks {
        if matches!(task.await.unwrap(), UsageOutcome::Added(_)) {
            added += 1;
        }
    }
    assert_eq!(added, 5);
    assert_eq!(s.usage(a.id, kind, day).await.unwrap(), 5);
}

pub(crate) async fn pruning_deletes_only_older_days_of_that_account<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let (a, b) = (active(s, "ann").await, active(s, "bob").await);
    for id in [a.id, b.id] {
        for day in [10, 20] {
            s.add_usage(id, MIN, Day::from_raw(day), 1, None)
                .await
                .unwrap();
        }
    }
    s.prune_usage(a.id, Day::from_raw(15)).await.unwrap();
    assert_eq!(s.usage(a.id, MIN, Day::from_raw(10)).await.unwrap(), 0);
    assert_eq!(s.usage(a.id, MIN, Day::from_raw(20)).await.unwrap(), 1);
    assert_eq!(s.usage(b.id, MIN, Day::from_raw(10)).await.unwrap(), 1);
}

pub(crate) async fn limits_and_usage_survive_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let a = active(h.store(), "ann").await;
    h.store()
        .set_role_limit(&Slug::MEMBER, MIN, Limit::AtMost(9), Actor::System)
        .await
        .unwrap();
    h.store()
        .set_member_limit(a.id, MIN, Some(Limit::AtMost(3)), Actor::System)
        .await
        .unwrap();
    h.store()
        .add_usage(a.id, MIN, Day::from_raw(20_000), 77, None)
        .await
        .unwrap();
    h.reopen().await;
    let s = h.store();
    assert_eq!(
        role_limit(s, &Slug::MEMBER, MIN).await,
        Some(Limit::AtMost(9))
    );
    assert_eq!(s.member_limits(a.id).await.unwrap().len(), 1);
    assert_eq!(s.usage(a.id, MIN, Day::from_raw(20_000)).await.unwrap(), 77);
}

/// A broken store errors; it never reads as "no limit" or "no usage".
pub(crate) async fn a_broken_store_errors_instead_of_reading_as_unlimited<H: Harness>() {
    let h = H::fresh().await;
    let a = active(h.store(), "ann").await;
    h.close().await;
    let s = h.store();
    assert!(s.role_limits(&[Slug::MEMBER]).await.is_err());
    assert!(s.member_limits(a.id).await.is_err());
    assert!(s.usage(a.id, MIN, Day::from_raw(1)).await.is_err());
    assert!(
        s.add_usage(a.id, MIN, Day::from_raw(1), 1, None)
            .await
            .is_err()
    );
}

/// A whole session start against the real store: the call is counted, a second session over the
/// limit is refused, and a restart (a new counter) still sees the day's calls.
pub(crate) async fn a_session_start_counts_calls_in_the_store<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let a = active(s, "ann").await;
    s.set_role_limit(
        &Slug::MEMBER,
        LimitKind::CallsPerDay,
        Limit::AtMost(1),
        Actor::System,
    )
    .await
    .unwrap();
    let clock = ManualClock::new(UnixSeconds::from_raw(1_793_613_600));
    let snapshot = s.snapshot(Some(a.id)).await.unwrap();
    drop(
        begin_member_session(s, &clock, &SessionCounter::new(), &snapshot)
            .await
            .unwrap(),
    );
    let again = begin_member_session(s, &clock, &SessionCounter::new(), &snapshot).await;
    assert!(again.is_err(), "the day's only call is spent");
}

/// The defaults table in `docs/access.md` and in `docs/config.md` equals the seeded rows, so the
/// documented numbers cannot drift from what a new board gets.
pub(crate) async fn the_documented_defaults_equal_the_seeded_rows<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    for (name, docs) in [
        ("access.md", include_str!("../../../../docs/access.md")),
        ("config.md", include_str!("../../../../docs/config.md")),
    ] {
        let table: Vec<Vec<String>> = docs
            .lines()
            .filter(|l| {
                l.starts_with("| `member`")
                    || l.starts_with("| `trusted`")
                    || l.starts_with("| `moderator`")
                    || l.starts_with("| `cosysop`")
                    || l.starts_with("| `sysop`")
            })
            .filter(|l| l.matches('|').count() == 10)
            .map(|l| {
                l.split('|')
                    .skip(1)
                    .take(9)
                    .map(|c| c.trim().to_owned())
                    .collect()
            })
            .collect();
        assert_eq!(table.len(), 5, "{name}: one row per built-in role");
        for row in table {
            let role = Slug::parse(row[0].trim_matches('`')).unwrap();
            for (cell, &kind) in row[1..].iter().zip(LimitKind::ALL) {
                assert_eq!(
                    role_limit(s, &role, kind).await,
                    Some(parse_cell(cell)),
                    "{name}: {} {}",
                    role.as_str(),
                    kind.name()
                );
            }
        }
    }
}

fn parse_cell(cell: &str) -> Limit {
    if cell == "unlimited" {
        return Limit::Unlimited;
    }
    let mut parts = cell.split(' ');
    let number: u64 = parts.next().unwrap().parse().unwrap();
    Limit::AtMost(match parts.next() {
        None => number,
        Some("MiB") => number * 1024 * 1024,
        Some(other) => panic!("unknown unit {other}"),
    })
}

/// Limit edits write one `limit.changed` row each, in their transaction; a repeat that changes
/// nothing and a refused edit write none.
pub(crate) async fn limit_edits_write_audit_rows_and_no_ops_write_none<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let (admin, ann) = (active(s, "pat").await, active(s, "ann").await);
    let by = Actor::Account(admin.id);
    let rows = || async { s.page(&AuditQuery::default()).await.unwrap() };
    let before = rows().await.len();

    s.set_role_limit(&Slug::MEMBER, MIN, Limit::AtMost(90), by)
        .await
        .unwrap();
    s.set_role_limit(&Slug::MEMBER, MIN, Limit::AtMost(90), by)
        .await
        .unwrap();
    s.set_member_limit(ann.id, LimitKind::CallsPerDay, Some(Limit::Unlimited), by)
        .await
        .unwrap();
    s.set_member_limit(ann.id, LimitKind::CallsPerDay, Some(Limit::Unlimited), by)
        .await
        .unwrap();
    s.set_member_limit(ann.id, LimitKind::CallsPerDay, None, by)
        .await
        .unwrap();
    s.set_member_limit(ann.id, LimitKind::CallsPerDay, None, by)
        .await
        .unwrap();
    let missing = Slug::parse("no-such-role").unwrap();
    assert!(
        s.set_role_limit(&missing, MIN, Limit::AtMost(1), by)
            .await
            .is_err()
    );
    assert!(
        s.set_member_limit(AccountId::from_raw(9_999), MIN, Some(Limit::AtMost(1)), by)
            .await
            .is_err()
    );

    let new: Vec<_> = rows()
        .await
        .into_iter()
        .take(rows().await.len() - before)
        .collect();
    assert_eq!(
        new.len(),
        3,
        "one row per change, none for no-ops or refusals"
    );
    // Newest first: clear, set unlimited, set 90.
    let detail = |i: usize| new[i].event.detail.as_str().to_owned();
    assert_eq!(detail(0), "calls_per_day override cleared");
    assert_eq!(detail(1), "calls_per_day set to unlimited");
    assert_eq!(detail(2), "minutes_per_day set to 90");
    assert_eq!(new[2].event.target, Target::Role(Slug::MEMBER));
    assert_eq!(new[0].event.target, Target::Account(ann.id));
    for row in &new {
        assert_eq!(row.event.action, Action::LimitChanged);
        assert_eq!(row.event.actor, by);
    }
}

/// When the audit insert fails the edit does not land either.
pub(crate) async fn a_limit_edit_never_lands_without_its_audit_row<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = active(s, "ann").await;
    let by = Actor::Account(ann.id);
    h.break_audit_inserts().await;
    assert!(
        s.set_role_limit(&Slug::MEMBER, MIN, Limit::AtMost(1), by)
            .await
            .is_err()
    );
    assert_eq!(
        role_limit(s, &Slug::MEMBER, MIN).await,
        Some(Limit::AtMost(60))
    );
    assert!(
        s.set_member_limit(ann.id, MIN, Some(Limit::AtMost(1)), by)
            .await
            .is_err()
    );
    assert_eq!(s.member_limits(ann.id).await.unwrap().len(), 0);
}
