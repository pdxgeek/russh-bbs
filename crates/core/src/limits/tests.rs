//! Unit tests: the pure resolution rule, the session counter, and the meter against an
//! in-memory store and a manual clock (no sleeping).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl,
    clippy::too_many_lines
)]

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::access::{Permission, Slug, Snapshot, Via};
use crate::store::{AccountId, StoreError, UnixSeconds};

use super::*;

fn slug(name: &str) -> Slug {
    Slug::parse(name).unwrap()
}

fn row(role: &str, kind: LimitKind, limit: Limit) -> RoleLimit {
    RoleLimit {
        role: slug(role),
        kind,
        limit,
    }
}

const MIN: LimitKind = LimitKind::MinutesPerDay;

// ---- effective_limits ------------------------------------------------------------------------

#[test]
fn effective_limit_table() {
    use Limit::{AtMost, Unlimited};
    struct Case {
        name: &'static str,
        roles: Vec<&'static str>,
        rows: Vec<RoleLimit>,
        overrides: Vec<MemberLimit>,
        want: Limit,
    }
    let ovr = |limit| vec![MemberLimit { kind: MIN, limit }];
    let cases = [
        Case {
            name: "one role, one value",
            roles: vec!["member"],
            rows: vec![row("member", MIN, AtMost(60))],
            overrides: vec![],
            want: AtMost(60),
        },
        Case {
            name: "most generous of two roles",
            roles: vec!["member", "trusted"],
            rows: vec![
                row("member", MIN, AtMost(60)),
                row("trusted", MIN, AtMost(120)),
            ],
            overrides: vec![],
            want: AtMost(120),
        },
        Case {
            name: "unlimited beats any number",
            roles: vec!["member", "cosysop"],
            rows: vec![
                row("member", MIN, AtMost(60)),
                row("cosysop", MIN, Unlimited),
            ],
            overrides: vec![],
            want: Unlimited,
        },
        Case {
            name: "zero is a real limit",
            roles: vec!["member"],
            rows: vec![row("member", MIN, AtMost(0))],
            overrides: vec![],
            want: AtMost(0),
        },
        Case {
            name: "override lowers",
            roles: vec!["trusted"],
            rows: vec![row("trusted", MIN, AtMost(120))],
            overrides: ovr(AtMost(10)),
            want: AtMost(10),
        },
        Case {
            name: "override raises",
            roles: vec!["member"],
            rows: vec![row("member", MIN, AtMost(60))],
            overrides: ovr(Unlimited),
            want: Unlimited,
        },
        Case {
            name: "a role with no value is skipped, not unlimited",
            roles: vec!["member", "door-admin"],
            rows: vec![row("member", MIN, AtMost(60))],
            overrides: vec![],
            want: AtMost(60),
        },
        Case {
            name: "rows of roles not held are ignored",
            roles: vec!["member"],
            rows: vec![
                row("member", MIN, AtMost(60)),
                row("trusted", MIN, Unlimited),
            ],
            overrides: vec![],
            want: AtMost(60),
        },
        Case {
            name: "no held role has a value: unlimited",
            roles: vec!["door-admin"],
            rows: vec![row("member", MIN, AtMost(60))],
            overrides: vec![],
            want: Unlimited,
        },
        Case {
            name: "sysop is unlimited whatever is stored",
            roles: vec!["member", "sysop"],
            rows: vec![row("sysop", MIN, AtMost(1)), row("member", MIN, AtMost(1))],
            overrides: ovr(AtMost(1)),
            want: Unlimited,
        },
        Case {
            name: "other kinds do not leak",
            roles: vec!["member"],
            rows: vec![row("member", LimitKind::CallsPerDay, AtMost(3))],
            overrides: vec![],
            want: Unlimited,
        },
    ];
    for case in cases {
        let roles: Vec<Slug> = case.roles.iter().map(|r| slug(r)).collect();
        let got = effective_limits(&roles, &case.rows, &case.overrides).get(MIN);
        assert_eq!(got, case.want, "{}", case.name);
    }
}

#[test]
fn every_kind_resolves_independently() {
    let roles = [slug("member")];
    let rows: Vec<RoleLimit> = LimitKind::ALL
        .iter()
        .enumerate()
        .map(|(i, &k)| row("member", k, Limit::AtMost(i as u64 + 1)))
        .collect();
    let got = effective_limits(&roles, &rows, &[]);
    for (i, &kind) in LimitKind::ALL.iter().enumerate() {
        assert_eq!(
            got.get(kind),
            Limit::AtMost(i as u64 + 1),
            "{}",
            kind.name()
        );
    }
}

#[test]
fn limit_order_puts_unlimited_last() {
    assert!(Limit::AtMost(u64::MAX) < Limit::Unlimited);
    assert!(Limit::AtMost(1) < Limit::AtMost(2));
}

#[test]
fn stored_values_round_trip() {
    assert_eq!(Limit::from_stored(None).unwrap(), Limit::Unlimited);
    assert_eq!(Limit::from_stored(Some(5)).unwrap(), Limit::AtMost(5));
    assert!(Limit::from_stored(Some(-1)).is_err());
    assert_eq!(Limit::AtMost(5).to_stored(), Some(Some(5)));
    assert_eq!(Limit::Unlimited.to_stored(), Some(None));
    assert_eq!(Limit::AtMost(u64::MAX).to_stored(), None);
}

#[test]
fn kind_names_round_trip_and_are_unique() {
    let mut names: Vec<&str> = LimitKind::ALL.iter().map(|k| k.name()).collect();
    for &kind in LimitKind::ALL {
        assert_eq!(LimitKind::parse(kind.name()), Some(kind));
    }
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), LimitKind::ALL.len());
    assert_eq!(LimitKind::parse("credits_earn_rate"), None);
}

#[test]
fn roles_held_counts_global_roles_only() {
    use crate::access::{Held, ResourceId, ResourceKind, ResourceRef, Scope, Source};
    let global = Held {
        permission: Permission::ForumPost,
        scope: Scope::Global,
        source: Source::Role {
            role: slug("member"),
            rank: 10,
            via: Via::Account,
        },
    };
    let scoped = Held {
        permission: Permission::ForumModerate,
        scope: Scope::Resource(ResourceRef {
            kind: ResourceKind::Forum,
            id: ResourceId::new(1).unwrap(),
        }),
        source: Source::Role {
            role: slug("moderator"),
            rank: 30,
            via: Via::Account,
        },
    };
    let allow = Held {
        permission: Permission::ForumRead,
        scope: Scope::Global,
        source: Source::Allow { via: Via::Account },
    };
    let snapshot = Snapshot::new(
        Some(AccountId::from_raw(1)),
        vec![global.clone(), scoped, allow, global],
    );
    assert_eq!(roles_held(&snapshot), vec![slug("member")]);
}

// ---- sessions --------------------------------------------------------------------------------

#[test]
fn session_slots_are_counted_and_released() {
    let counter = SessionCounter::new();
    let a = AccountId::from_raw(1);
    let b = AccountId::from_raw(2);
    let first = counter.try_acquire(a, Limit::AtMost(2)).unwrap();
    let second = counter.try_acquire(a, Limit::AtMost(2)).unwrap();
    assert!(counter.try_acquire(a, Limit::AtMost(2)).is_none());
    assert!(
        counter.try_acquire(b, Limit::AtMost(1)).is_some(),
        "per account"
    );
    drop(first);
    assert_eq!(counter.open_for(a), 1);
    let third = counter.try_acquire(a, Limit::AtMost(2)).unwrap();
    drop((second, third));
    assert_eq!(counter.open_for(a), 0);
}

#[test]
fn zero_sessions_means_none_and_unlimited_means_any() {
    let counter = SessionCounter::new();
    let a = AccountId::from_raw(1);
    assert!(counter.try_acquire(a, Limit::AtMost(0)).is_none());
    assert_eq!(counter.open_for(a), 0);
    let slots: Vec<_> = (0..50)
        .map(|_| counter.try_acquire(a, Limit::Unlimited).unwrap())
        .collect();
    assert_eq!(counter.open_for(a), 50);
    drop(slots);
    assert_eq!(counter.open_for(a), 0);
}

// ---- an in-memory store ----------------------------------------------------------------------

#[derive(Default)]
struct Fake {
    role: Mutex<Vec<RoleLimit>>,
    member: Mutex<BTreeMap<(i64, LimitKind), Limit>>,
    usage: Mutex<BTreeMap<(i64, LimitKind, i64), u64>>,
}

impl LimitStore for Fake {
    async fn role_limits(&self, roles: &[Slug]) -> Result<Vec<RoleLimit>, StoreError> {
        Ok(self
            .role
            .lock()
            .unwrap()
            .iter()
            .filter(|r| roles.contains(&r.role))
            .cloned()
            .collect())
    }

    async fn member_limits(&self, account: AccountId) -> Result<Vec<MemberLimit>, StoreError> {
        Ok(self
            .member
            .lock()
            .unwrap()
            .iter()
            .filter(|((id, _), _)| *id == account.as_raw())
            .map(|((_, kind), limit)| MemberLimit {
                kind: *kind,
                limit: *limit,
            })
            .collect())
    }

    async fn set_role_limit(
        &self,
        role: &Slug,
        kind: LimitKind,
        limit: Limit,
    ) -> Result<(), LimitWriteError> {
        let mut rows = self.role.lock().unwrap();
        rows.retain(|r| !(r.role == *role && r.kind == kind));
        rows.push(RoleLimit {
            role: role.clone(),
            kind,
            limit,
        });
        Ok(())
    }

    async fn set_member_limit(
        &self,
        account: AccountId,
        kind: LimitKind,
        limit: Option<Limit>,
    ) -> Result<(), LimitWriteError> {
        let mut rows = self.member.lock().unwrap();
        match limit {
            Some(limit) => rows.insert((account.as_raw(), kind), limit),
            None => rows.remove(&(account.as_raw(), kind)),
        };
        Ok(())
    }

    async fn usage(
        &self,
        account: AccountId,
        kind: LimitKind,
        day: Day,
    ) -> Result<u64, StoreError> {
        Ok(self
            .usage
            .lock()
            .unwrap()
            .get(&(account.as_raw(), kind, day.as_raw()))
            .copied()
            .unwrap_or(0))
    }

    async fn add_usage(
        &self,
        account: AccountId,
        kind: LimitKind,
        day: Day,
        amount: u64,
        cap: Option<u64>,
    ) -> Result<UsageOutcome, StoreError> {
        let mut usage = self.usage.lock().unwrap();
        let entry = usage
            .entry((account.as_raw(), kind, day.as_raw()))
            .or_insert(0);
        if cap.is_some_and(|cap| *entry + amount > cap) {
            return Ok(UsageOutcome::Full(*entry));
        }
        *entry += amount;
        Ok(UsageOutcome::Added(*entry))
    }

    async fn prune_usage(&self, account: AccountId, before: Day) -> Result<(), StoreError> {
        self.usage
            .lock()
            .unwrap()
            .retain(|(id, _, day), _| *id != account.as_raw() || *day >= before.as_raw());
        Ok(())
    }
}

const ACCOUNT: AccountId = AccountId::from_raw(7);

fn member_snapshot() -> Snapshot {
    Snapshot::from_role(
        Some(ACCOUNT),
        &Slug::MEMBER,
        10,
        &Via::Account,
        [Permission::ForumPost],
    )
}

async fn fake_with(member: &[(LimitKind, Limit)]) -> Fake {
    let fake = Fake::default();
    for &(kind, limit) in member {
        fake.set_role_limit(&Slug::MEMBER, kind, limit)
            .await
            .unwrap();
    }
    fake
}

/// 2026-11-02 10:00:00 UTC.
const T0: i64 = 1_793_613_600;

fn clock() -> ManualClock {
    ManualClock::new(UnixSeconds::from_raw(T0))
}

// ---- begin_member_session --------------------------------------------------------------------

#[tokio::test]
async fn a_session_starts_and_counts_one_call() {
    let store = fake_with(&[(LimitKind::CallsPerDay, Limit::AtMost(2))]).await;
    let (clock, counter) = (clock(), SessionCounter::new());
    let meter = begin_member_session(&store, &clock, &counter, &member_snapshot())
        .await
        .unwrap();
    assert_eq!(meter.limits().get(LimitKind::CallsPerDay), Limit::AtMost(2));
    let day = Day::of(clock.now());
    assert_eq!(
        store
            .usage(ACCOUNT, LimitKind::CallsPerDay, day)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn calls_per_day_refuse_with_the_reset_time_and_do_not_count_the_refusal() {
    let store = fake_with(&[(LimitKind::CallsPerDay, Limit::AtMost(1))]).await;
    let (clock, counter) = (clock(), SessionCounter::new());
    let first = begin_member_session(&store, &clock, &counter, &member_snapshot())
        .await
        .unwrap();
    drop(first);
    let Err(LimitError::Refused(refusal)) =
        begin_member_session(&store, &clock, &counter, &member_snapshot()).await
    else {
        panic!("the second call of the day must be refused");
    };
    assert_eq!(refusal.kind, LimitKind::CallsPerDay);
    assert_eq!(refusal.limit, 1);
    // 10:00 UTC: 14 hours to midnight.
    assert_eq!(refusal.resets_in, Some(14 * 3600));
    let text = refusal.lines().join(" ");
    assert!(text.contains("1 sign-in for today"), "{text}");
    assert!(text.contains("14h 0m"), "{text}");
    let day = Day::of(clock.now());
    assert_eq!(
        store
            .usage(ACCOUNT, LimitKind::CallsPerDay, day)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn calls_reset_the_next_utc_day() {
    let store = fake_with(&[(LimitKind::CallsPerDay, Limit::AtMost(1))]).await;
    let (clock, counter) = (clock(), SessionCounter::new());
    drop(
        begin_member_session(&store, &clock, &counter, &member_snapshot())
            .await
            .unwrap(),
    );
    clock.advance(14 * 3600);
    begin_member_session(&store, &clock, &counter, &member_snapshot())
        .await
        .expect("a new UTC day starts a new count");
}

#[tokio::test]
async fn concurrent_sessions_are_refused_and_freed_on_drop() {
    let store = fake_with(&[(LimitKind::MaxSessions, Limit::AtMost(1))]).await;
    let (clock, counter) = (clock(), SessionCounter::new());
    let first = begin_member_session(&store, &clock, &counter, &member_snapshot())
        .await
        .unwrap();
    let Err(LimitError::Refused(refusal)) =
        begin_member_session(&store, &clock, &counter, &member_snapshot()).await
    else {
        panic!("a second session must be refused");
    };
    assert_eq!(refusal.kind, LimitKind::MaxSessions);
    assert_eq!(refusal.resets_in, None);
    assert!(refusal.lines().join(" ").contains("no timed reset"));
    // The refused attempt took no slot and no call.
    assert_eq!(counter.open_for(ACCOUNT), 1);
    drop(first);
    begin_member_session(&store, &clock, &counter, &member_snapshot())
        .await
        .expect("the slot is free again");
}

#[tokio::test]
async fn a_refused_start_releases_its_slot() {
    let store = fake_with(&[(LimitKind::CallsPerDay, Limit::AtMost(0))]).await;
    let (clock, counter) = (clock(), SessionCounter::new());
    assert!(
        begin_member_session(&store, &clock, &counter, &member_snapshot())
            .await
            .is_err()
    );
    assert_eq!(counter.open_for(ACCOUNT), 0);
}

#[tokio::test]
async fn no_minutes_left_refuses_the_start_without_counting_a_call() {
    let store = fake_with(&[
        (MIN, Limit::AtMost(1)),
        (LimitKind::CallsPerDay, Limit::AtMost(5)),
    ])
    .await;
    let (clock, counter) = (clock(), SessionCounter::new());
    let day = Day::of(clock.now());
    store.add_usage(ACCOUNT, MIN, day, 60, None).await.unwrap();
    let Err(LimitError::Refused(refusal)) =
        begin_member_session(&store, &clock, &counter, &member_snapshot()).await
    else {
        panic!("a spent day must refuse the start");
    };
    assert_eq!(refusal.kind, MIN);
    assert_eq!(
        store
            .usage(ACCOUNT, LimitKind::CallsPerDay, day)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn an_override_applies_and_unlimited_roles_are_not_metered() {
    let store = fake_with(&[(LimitKind::CallsPerDay, Limit::AtMost(1))]).await;
    store
        .set_member_limit(ACCOUNT, LimitKind::CallsPerDay, Some(Limit::AtMost(3)))
        .await
        .unwrap();
    let (clock, counter) = (clock(), SessionCounter::new());
    for _ in 0..3 {
        drop(
            begin_member_session(&store, &clock, &counter, &member_snapshot())
                .await
                .unwrap(),
        );
    }
    assert!(
        begin_member_session(&store, &clock, &counter, &member_snapshot())
            .await
            .is_err()
    );

    let sysop = Snapshot::from_role(
        Some(ACCOUNT),
        &Slug::SYSOP,
        100,
        &Via::Account,
        [Permission::LimitsEdit],
    );
    store
        .set_role_limit(&Slug::SYSOP, MIN, Limit::AtMost(0))
        .await
        .unwrap();
    begin_member_session(&store, &clock, &counter, &sysop)
        .await
        .expect("sysop is unlimited whatever is stored");
}

#[tokio::test]
async fn a_snapshot_without_an_account_is_not_metered() {
    let store = Fake::default();
    let waiting = Snapshot::empty();
    assert!(matches!(
        begin_member_session(&store, &clock(), &SessionCounter::new(), &waiting).await,
        Err(LimitError::NoAccount)
    ));
}

// ---- the time meter --------------------------------------------------------------------------

#[tokio::test]
async fn minutes_run_out_and_time_is_persisted() {
    let store = fake_with(&[(MIN, Limit::AtMost(2))]).await;
    let (clock, counter) = (clock(), SessionCounter::new());
    let snapshot = member_snapshot();
    let mut meter = begin_member_session(&store, &clock, &counter, &snapshot)
        .await
        .unwrap();
    clock.advance(60);
    meter.tick(&store, &clock, &snapshot).await.unwrap();
    clock.advance(59);
    meter.tick(&store, &clock, &snapshot).await.unwrap();
    clock.advance(1);
    let Err(LimitError::Refused(refusal)) = meter.tick(&store, &clock, &snapshot).await else {
        panic!("two minutes are used");
    };
    assert_eq!(refusal.kind, MIN);
    assert_eq!(refusal.limit, 2);
    assert!(refusal.lines().join(" ").contains("2 minutes for today"));
    meter.finish(&store, &clock).await.unwrap();
    let day = Day::of(clock.now());
    assert_eq!(
        store.usage(ACCOUNT, MIN, day).await.unwrap(),
        120,
        "the stretch was charged"
    );
}

#[tokio::test]
async fn a_restart_does_not_reset_usage() {
    let store = fake_with(&[(MIN, Limit::AtMost(1))]).await;
    let (clock, counter) = (clock(), SessionCounter::new());
    let snapshot = member_snapshot();
    let meter = begin_member_session(&store, &clock, &counter, &snapshot)
        .await
        .unwrap();
    clock.advance(60);
    meter.finish(&store, &clock).await.unwrap();
    // A new counter and a new meter stand in for a new process: only the store remembers.
    assert!(
        begin_member_session(&store, &clock, &SessionCounter::new(), &snapshot)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_limit_edit_reaches_an_open_session_at_the_next_checkpoint() {
    let store = fake_with(&[(MIN, Limit::AtMost(100))]).await;
    let (clock, counter) = (clock(), SessionCounter::new());
    let snapshot = member_snapshot();
    let mut meter = begin_member_session(&store, &clock, &counter, &snapshot)
        .await
        .unwrap();
    store
        .set_role_limit(&Slug::MEMBER, MIN, Limit::AtMost(1))
        .await
        .unwrap();
    clock.advance(CHECKPOINT_SECONDS);
    meter.tick(&store, &clock, &snapshot).await.unwrap();
    clock.advance(30);
    assert!(matches!(
        meter.tick(&store, &clock, &snapshot).await,
        Err(LimitError::Refused(_))
    ));
}

#[tokio::test]
async fn the_meter_rolls_over_at_midnight_utc() {
    let store = fake_with(&[(MIN, Limit::AtMost(30))]).await;
    let (clock, counter) = (clock(), SessionCounter::new());
    let snapshot = member_snapshot();
    let start_day = Day::of(clock.now());
    let mut meter = begin_member_session(&store, &clock, &counter, &snapshot)
        .await
        .unwrap();
    // Ten hours later it is midnight; the old day is charged only up to midnight.
    clock.advance(14 * 3600 + 60);
    // Yesterday's use does not count against today.
    meter.tick(&store, &clock, &snapshot).await.unwrap();
    assert_eq!(
        store.usage(ACCOUNT, MIN, start_day).await.unwrap(),
        14 * 3600
    );
    meter.finish(&store, &clock).await.unwrap();
    assert_eq!(
        store
            .usage(ACCOUNT, MIN, start_day.minus(-1))
            .await
            .unwrap(),
        60
    );
}

#[tokio::test]
async fn old_usage_rows_are_pruned_at_session_start() {
    let store = fake_with(&[]).await;
    let (clock, counter) = (clock(), SessionCounter::new());
    let today = Day::of(clock.now());
    let old = today.minus(USAGE_RETENTION_DAYS + 1);
    store.add_usage(ACCOUNT, MIN, old, 5, None).await.unwrap();
    drop(
        begin_member_session(&store, &clock, &counter, &member_snapshot())
            .await
            .unwrap(),
    );
    assert_eq!(store.usage(ACCOUNT, MIN, old).await.unwrap(), 0);
}

#[test]
fn durations_read_naturally() {
    for (secs, want) in [
        (0, "in less than a minute."),
        (61, "in 2m."),
        (3 * 3600 + 20 * 60, "in 3h 20m."),
    ] {
        let line = LimitRefusal {
            kind: MIN,
            limit: 1,
            resets_in: Some(secs),
        }
        .lines()
        .remove(1);
        assert!(line.ends_with(want), "{line}");
    }
}

// ---- editing ---------------------------------------------------------------------------------

fn sysop_snapshot() -> Snapshot {
    Snapshot::from_role(
        Some(ACCOUNT),
        &Slug::SYSOP,
        100,
        &Via::Account,
        Permission::ALL.iter().copied(),
    )
}

#[tokio::test]
async fn editing_needs_the_limits_edit_proof() {
    let store = Fake::default();
    let granted = sysop_snapshot()
        .require(Permission::LimitsEdit, None)
        .unwrap();
    set_role_limit(&store, &granted, &Slug::MEMBER, MIN, Limit::AtMost(5))
        .await
        .unwrap();
    set_member_limit(&store, &granted, ACCOUNT, MIN, Limit::AtMost(9))
        .await
        .unwrap();
    assert_eq!(store.member_limits(ACCOUNT).await.unwrap().len(), 1);
    clear_member_limit(&store, &granted, ACCOUNT, MIN)
        .await
        .unwrap();
    assert_eq!(store.member_limits(ACCOUNT).await.unwrap().len(), 0);

    // A proof for some other permission is not enough, and nothing is written.
    let other = sysop_snapshot()
        .require(Permission::GroupsEdit, None)
        .unwrap();
    assert!(matches!(
        set_role_limit(&store, &other, &Slug::MEMBER, MIN, Limit::AtMost(1)).await,
        Err(LimitEditError::NotPermitted)
    ));
    assert_eq!(store.role.lock().unwrap().len(), 1);
    assert!(matches!(
        set_member_limit(&store, &other, ACCOUNT, MIN, Limit::AtMost(1)).await,
        Err(LimitEditError::NotPermitted)
    ));
    assert!(matches!(
        clear_member_limit(&store, &other, ACCOUNT, MIN).await,
        Err(LimitEditError::NotPermitted)
    ));
}

#[tokio::test]
async fn a_number_the_database_cannot_hold_is_refused() {
    let store = Fake::default();
    let granted = sysop_snapshot()
        .require(Permission::LimitsEdit, None)
        .unwrap();
    assert!(matches!(
        set_role_limit(
            &store,
            &granted,
            &Slug::MEMBER,
            MIN,
            Limit::AtMost(u64::MAX)
        )
        .await,
        Err(LimitEditError::TooLarge)
    ));
}
