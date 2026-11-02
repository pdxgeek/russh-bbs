//! The audit log against any backend (M2-C, D58): append and bounded paging, the filters, what
//! the database itself refuses, and that each wired write lands with its row, in its transaction.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use russh_bbs_core::access::{
    AccessError, AccessStore, Child, Granting, GroupChange, NewGrant, Permission, ResourceId,
    ResourceKind, ResourceRef, Scope, Slug, Subject,
};
use russh_bbs_core::audit::{
    Action, Actor, AuditEntry, AuditEvent, AuditQuery, AuditStore, FixedClock, MAX_PAGE, Target,
};
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountState, AccountStore, NewAccount, StoreError, UnixSeconds,
};
use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore};

use super::{Harness, fingerprint};

fn clock_at(h: &mut impl Harness, at: i64) -> Arc<FixedClock> {
    let clock = Arc::new(FixedClock::new(at));
    h.set_clock(clock.clone());
    clock
}

async fn account<S: AccountStore>(store: &S, name: &str) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

async fn all<S: AuditStore>(store: &S) -> Vec<AuditEntry> {
    store.page(&AuditQuery::default()).await.unwrap()
}

fn actions(rows: &[AuditEntry]) -> Vec<Action> {
    rows.iter().map(|row| row.event.action).collect()
}

fn event(actor: Actor, action: Action) -> AuditEvent {
    AuditEvent::new(actor, action, Target::None)
}

pub(crate) async fn append_and_page_round_trip_newest_first_with_the_injected_clock<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, 1_000);
    let store = h.store();
    let resource = ResourceRef {
        kind: ResourceKind::Forum,
        id: ResourceId::new(3).unwrap(),
    };
    let first = AuditEvent::new(
        Actor::Account(russh_bbs_core::store::AccountId::from_raw(5)),
        Action::PermissionGranted,
        Target::Role(Slug::parse("moderator").unwrap()),
    )
    .on(resource)
    .detail("permission forum.read\x1b[31m");
    let second = AuditEvent::new(
        Actor::System,
        Action::GroupCreated,
        Target::Key(fingerprint(9)),
    );
    let a = store.append(&first).await.unwrap();
    clock.set(1_001);
    let b = store.append(&second).await.unwrap();
    assert!(b > a, "ids increase");

    let rows = all(store).await;
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0].id, rows[0].at), (b, UnixSeconds::from_raw(1_001)));
    assert_eq!((rows[1].id, rows[1].at), (a, UnixSeconds::from_raw(1_000)));
    assert_eq!(rows[0].event, second);
    assert_eq!(rows[1].event, first);
    assert_eq!(rows[1].event.detail.as_str(), "permission forum.read");
}

pub(crate) async fn paging_filters_combine_and_time_bounds_are_since_inclusive_until_exclusive<
    H: Harness,
>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, 0);
    let store = h.store();
    let one = Actor::Account(russh_bbs_core::store::AccountId::from_raw(1));
    let two = Actor::Account(russh_bbs_core::store::AccountId::from_raw(2));
    for (at, actor, action) in [
        (100, Actor::System, Action::GroupCreated),
        (200, one, Action::GroupCreated),
        (300, one, Action::PermissionGranted),
        (400, two, Action::PermissionGranted),
    ] {
        clock.set(at);
        store.append(&event(actor, action)).await.unwrap();
    }
    let times =
        |rows: Vec<AuditEntry>| -> Vec<i64> { rows.iter().map(|row| row.at.as_raw()).collect() };
    let q = AuditQuery::default;
    let at = |s| Some(UnixSeconds::from_raw(s));

    let by = |query: AuditQuery| {
        let store = store.clone();
        async move { times(store.page(&query).await.unwrap()) }
    };
    assert_eq!(by(q()).await, [400, 300, 200, 100]);
    assert_eq!(
        by(AuditQuery {
            actor: Some(one),
            ..q()
        })
        .await,
        [300, 200]
    );
    assert_eq!(
        by(AuditQuery {
            actor: Some(Actor::System),
            ..q()
        })
        .await,
        [100]
    );
    assert_eq!(
        by(AuditQuery {
            action: Some(Action::PermissionGranted),
            ..q()
        })
        .await,
        [400, 300]
    );
    assert_eq!(
        by(AuditQuery {
            actor: Some(one),
            action: Some(Action::PermissionGranted),
            ..q()
        })
        .await,
        [300]
    );
    assert_eq!(
        by(AuditQuery {
            since: at(200),
            ..q()
        })
        .await,
        [400, 300, 200]
    );
    assert_eq!(
        by(AuditQuery {
            until: at(300),
            ..q()
        })
        .await,
        [200, 100]
    );
    assert_eq!(
        by(AuditQuery {
            since: at(200),
            until: at(400),
            ..q()
        })
        .await,
        [300, 200]
    );
    assert_eq!(
        by(AuditQuery {
            actor: Some(two),
            action: Some(Action::GroupCreated),
            ..q()
        })
        .await,
        Vec::<i64>::new()
    );
}

pub(crate) async fn pages_are_bounded_and_the_cursor_walks_every_row_once<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let total = MAX_PAGE as usize + 5;
    for _ in 0..total {
        store
            .append(&event(Actor::System, Action::SettingChanged))
            .await
            .unwrap();
    }
    assert_eq!(all(store).await.len(), MAX_PAGE as usize);
    let huge = AuditQuery {
        limit: u32::MAX,
        ..AuditQuery::default()
    };
    assert_eq!(store.page(&huge).await.unwrap().len(), MAX_PAGE as usize);
    let zero = AuditQuery {
        limit: 0,
        ..AuditQuery::default()
    };
    assert_eq!(store.page(&zero).await.unwrap().len(), 1);

    let mut seen = Vec::new();
    let mut before = None;
    loop {
        let page = store
            .page(&AuditQuery {
                before,
                limit: 40,
                ..AuditQuery::default()
            })
            .await
            .unwrap();
        assert!(page.len() <= 40);
        let Some(last) = page.last() else { break };
        before = Some(last.id);
        seen.extend(page.iter().map(|row| row.id));
    }
    assert_eq!(seen.len(), total);
    assert!(
        seen.windows(2).all(|w| w[0] > w[1]),
        "newest first, no repeats"
    );
}

pub(crate) async fn update_and_delete_by_raw_sql_fail_and_leave_the_row<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, 42);
    let store = h.store();
    let original = event(Actor::System, Action::GroupCreated).detail("keep me");
    store.append(&original).await.unwrap();

    for sql in [
        "UPDATE audit_log SET detail = 'tampered'",
        "UPDATE audit_log SET action = 'group.deleted' WHERE id > 0",
        "DELETE FROM audit_log",
        "DELETE FROM audit_log WHERE id > 0",
    ] {
        let error = h.raw(sql).await.unwrap_err();
        assert!(error.contains("append-only"), "{sql}: {error}");
    }
    let rows = all(store).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event, original);
    assert_eq!(rows[0].at, UnixSeconds::from_raw(42));
}

pub(crate) async fn the_founder_claim_writes_its_row_in_its_transaction<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, 5_000);
    let store = h.store();
    let founder = store
        .claim_founder(sanitize_username("founder").unwrap(), &fingerprint(1))
        .await
        .unwrap()
        .unwrap();
    let rows = all(store).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at, UnixSeconds::from_raw(5_000));
    assert_eq!(rows[0].event.action, Action::FounderClaimed);
    assert_eq!(rows[0].event.actor, Actor::Account(founder.id));
    assert_eq!(rows[0].event.target, Target::Account(founder.id));

    // A refused second claim wrote nothing, so it wrote no row.
    let again = store
        .claim_founder(sanitize_username("other").unwrap(), &fingerprint(2))
        .await
        .unwrap();
    assert!(again.is_none());
    assert_eq!(all(store).await.len(), 1);
}

pub(crate) async fn creating_an_account_writes_a_row<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "sam").await;
    store
        .create_account(NewAccount {
            username: sanitize_username("modmod").unwrap(),
            role: Slug::parse("moderator").unwrap(),
            state: AccountState::Pending,
        })
        .await
        .unwrap();
    // A duplicate name fails and writes nothing.
    assert!(matches!(
        store
            .create_account(NewAccount {
                username: sanitize_username("sam").unwrap(),
                role: Slug::MEMBER,
                state: AccountState::Active,
            })
            .await,
        Err(StoreError::UsernameTaken)
    ));
    let rows = all(store).await;
    assert_eq!(
        actions(&rows),
        [Action::AccountCreated, Action::AccountCreated]
    );
    assert_eq!(rows[1].event.target, Target::Account(alice.id));
    assert_eq!(rows[1].event.actor, Actor::System);
    assert_eq!(rows[1].event.detail.as_str(), "role member state active");
    assert_eq!(
        rows[0].event.detail.as_str(),
        "role moderator state pending"
    );
}

pub(crate) async fn group_changes_write_rows_and_no_ops_write_none<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let admin = account(store, "pat").await;
    let alice = account(store, "sam").await;
    let by = Actor::Account(admin.id);
    let lounge = store
        .create_group(&Slug::parse("lounge").unwrap(), by)
        .await
        .unwrap();
    let other = store
        .create_group(&Slug::parse("other").unwrap(), by)
        .await
        .unwrap();
    let add = GroupChange::Add {
        group: lounge,
        child: Child::Account(alice.id),
    };
    store.change_groups(add, by).await.unwrap();
    store.change_groups(add, by).await.unwrap(); // already there: no row
    store
        .change_groups(
            GroupChange::Add {
                group: other,
                child: Child::Group(lounge),
            },
            by,
        )
        .await
        .unwrap();
    // A cycle is refused and writes nothing.
    assert!(
        store
            .change_groups(
                GroupChange::Add {
                    group: lounge,
                    child: Child::Group(other)
                },
                by
            )
            .await
            .is_err()
    );
    let remove = GroupChange::Remove {
        group: lounge,
        child: Child::Account(alice.id),
    };
    store.change_groups(remove, by).await.unwrap();
    store.change_groups(remove, by).await.unwrap(); // already gone: no row
    store
        .change_groups(GroupChange::Delete(lounge), by)
        .await
        .unwrap();
    // Deleting a missing group is refused and writes nothing.
    assert!(
        store
            .change_groups(GroupChange::Delete(lounge), by)
            .await
            .is_err()
    );

    let rows: Vec<_> = all(store)
        .await
        .into_iter()
        .filter(|row| row.event.actor == by)
        .collect();
    assert_eq!(
        actions(&rows),
        [
            Action::GroupDeleted,
            Action::GroupMemberRemoved,
            Action::GroupMemberAdded,
            Action::GroupMemberAdded,
            Action::GroupCreated,
            Action::GroupCreated,
        ]
    );
    assert_eq!(rows[0].event.target, Target::Group(lounge));
    assert_eq!(rows[0].event.detail.as_str(), "name lounge");
    assert_eq!(
        rows[1].event.detail.as_str(),
        format!("account {}", alice.id)
    );
    assert_eq!(
        rows[3].event.detail.as_str(),
        format!("account {}", alice.id)
    );
    assert_eq!(rows[2].event.detail.as_str(), format!("group {lounge}"));
    assert_eq!(rows[5].event.detail.as_str(), "name lounge");
}

pub(crate) async fn a_new_grant_writes_one_row_and_a_repeat_or_a_bad_grant_none<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let admin = account(store, "pat").await;
    let alice = account(store, "sam").await;
    let by = Actor::Account(admin.id);
    let resource = ResourceRef {
        kind: ResourceKind::FileArea,
        id: ResourceId::new(2).unwrap(),
    };
    let grant = NewGrant {
        subject: Subject::Account(alice.id),
        granting: Granting::Permission(Permission::FilesList),
        scope: Scope::Resource(resource),
    };
    store.add_grant(&grant, by).await.unwrap();
    store.add_grant(&grant, by).await.unwrap(); // already held: no row
    assert!(matches!(
        store
            .add_grant(
                &NewGrant {
                    subject: Subject::Account(alice.id),
                    granting: Granting::Role(Slug::parse("nonesuch").unwrap()),
                    scope: Scope::Global,
                },
                by,
            )
            .await,
        Err(AccessError::UnknownRole)
    ));
    assert!(matches!(
        store
            .add_grant(
                &NewGrant {
                    subject: Subject::Account(alice.id),
                    granting: Granting::Role(Slug::WAITING),
                    scope: Scope::Global,
                },
                by,
            )
            .await,
        Err(AccessError::RoleNotGrantable)
    ));
    let rows: Vec<_> = all(store)
        .await
        .into_iter()
        .filter(|row| row.event.action == Action::PermissionGranted)
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event.actor, by);
    assert_eq!(rows[0].event.target, Target::Account(alice.id));
    assert_eq!(rows[0].event.resource, Some(resource));
    assert_eq!(rows[0].event.detail.as_str(), "permission files.list");
}

pub(crate) async fn saving_terminal_settings_writes_a_row<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "sam").await;
    store
        .set_terminal_settings(alice.id, &TerminalSettings::default().with_color(false))
        .await
        .unwrap();
    // An unknown account fails and writes nothing.
    assert!(
        store
            .set_terminal_settings(
                russh_bbs_core::store::AccountId::from_raw(9_999),
                &TerminalSettings::default(),
            )
            .await
            .is_err()
    );
    let rows: Vec<_> = all(store)
        .await
        .into_iter()
        .filter(|row| row.event.action == Action::SettingChanged)
        .collect();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event.actor, Actor::Account(alice.id));
    assert_eq!(rows[0].event.target, Target::Account(alice.id));
    assert_eq!(rows[0].event.detail.as_str(), "terminal settings");
}

/// The "same transaction" proof: when the audit insert fails, the change that wanted it must not
/// land either. Each write is tried against a database that refuses every audit row.
pub(crate) async fn a_change_never_lands_without_its_audit_row<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let admin = account(store, "pat").await;
    let by = Actor::Account(admin.id);
    let lounge = store
        .create_group(&Slug::parse("lounge").unwrap(), by)
        .await
        .unwrap();
    let count = |table: &'static str, column: &'static str| {
        let sql = format!("UPDATE {table} SET {column} = {column}");
        let h = &h;
        async move { h.raw(&sql).await.unwrap() }
    };
    let accounts = count("accounts", "state").await;
    let grants = count("access_grant", "resource_id").await;
    let members = count("group_member", "group_id").await;
    let groups_before = store.groups().await.unwrap().len();
    let rows_before = all(store).await.len();

    h.break_audit_inserts().await;

    assert!(
        store
            .claim_founder(sanitize_username("founder").unwrap(), &fingerprint(1))
            .await
            .is_err()
    );
    assert!(
        !store.founder_claimed().await.unwrap(),
        "the claim must roll back"
    );
    assert!(
        store
            .create_account(NewAccount {
                username: sanitize_username("bob").unwrap(),
                role: Slug::MEMBER,
                state: AccountState::Active,
            })
            .await
            .is_err()
    );
    assert_eq!(
        count("accounts", "state").await,
        accounts,
        "no account, no founder"
    );
    assert!(
        store
            .create_group(&Slug::parse("newgroup").unwrap(), by)
            .await
            .is_err()
    );
    assert_eq!(store.groups().await.unwrap().len(), groups_before);
    assert!(
        store
            .change_groups(
                GroupChange::Add {
                    group: lounge,
                    child: Child::Account(admin.id)
                },
                by,
            )
            .await
            .is_err()
    );
    assert_eq!(count("group_member", "group_id").await, members);
    assert!(
        store
            .add_grant(
                &NewGrant {
                    subject: Subject::Account(admin.id),
                    granting: Granting::Permission(Permission::FilesList),
                    scope: Scope::Global,
                },
                by,
            )
            .await
            .is_err()
    );
    assert_eq!(count("access_grant", "resource_id").await, grants);
    assert!(
        store
            .set_terminal_settings(admin.id, &TerminalSettings::default().with_color(false))
            .await
            .is_err()
    );
    assert_eq!(count("terminal_settings", "color").await, 0);
    assert_eq!(all(store).await.len(), rows_before);
}

pub(crate) async fn audit_rows_survive_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, 77);
    h.store()
        .append(&event(Actor::System, Action::GroupCreated).detail("before restart"))
        .await
        .unwrap();
    h.reopen().await;
    let rows = all(h.store()).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].at, UnixSeconds::from_raw(77));
    assert_eq!(rows[0].event.detail.as_str(), "before restart");
}

/// `audit.view` follows the shipped roles: sysop and cosysop read the log, moderator, member
/// and the waiting room do not (docs/access.md). The read service refuses before the store.
pub(crate) async fn the_read_service_follows_the_role_data<H: Harness>() {
    use russh_bbs_core::audit::{AuditViewError, view_log};

    let h = H::fresh().await;
    let store = h.store();
    let founder = store
        .claim_founder(sanitize_username("founder").unwrap(), &fingerprint(1))
        .await
        .unwrap()
        .unwrap();
    let query = AuditQuery::default();

    let sysop = store.snapshot(Some(founder.id)).await.unwrap();
    let rows = view_log(store, &sysop, &query).await.unwrap();
    assert_eq!(actions(&rows), [Action::FounderClaimed]);

    for (name, role, may) in [
        ("jordan", "cosysop", true),
        ("tracy", "moderator", false),
        ("casey", "member", false),
    ] {
        let who = account(store, name).await;
        if role != "member" {
            store
                .add_grant(
                    &NewGrant {
                        subject: Subject::Account(who.id),
                        granting: Granting::Role(Slug::parse(role).unwrap()),
                        scope: Scope::Global,
                    },
                    Actor::System,
                )
                .await
                .unwrap();
        }
        let snapshot = store.snapshot(Some(who.id)).await.unwrap();
        let result = view_log(store, &snapshot, &query).await;
        assert_eq!(result.is_ok(), may, "{role}: {result:?}");
        if !may {
            assert!(matches!(result, Err(AuditViewError::Denied(_))), "{role}");
        }
    }
    let waiting = store.snapshot(None).await.unwrap();
    assert!(view_log(store, &waiting, &query).await.is_err());
}
