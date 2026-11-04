//! The door tables against any backend (M4-D0, D98): one save per player per door with
//! compare-and-swap writes, the shared world and its tick day, seasons, bars, the size caps,
//! the staff tools' audit rows and the role rows migration 0023 inserts.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeSet;

use russh_bbs_core::access::{AccessStore, Permission, Slug};
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore, known_events};
use russh_bbs_core::doors::{
    DoorId, DoorStaffError, DoorStore, DoorStoreError, MAX_SAVE_BYTES, MAX_WORLD_BYTES, Revision,
    SaveWrite, WorldWrite, bar_player, end_season, reset_player, unbar_player,
};
use russh_bbs_core::limits::Day;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{Account, AccountState, AccountStore, NewAccount, StoreError};

use super::Harness;

fn door() -> DoorId {
    DoorId::parse("coin-toss").unwrap()
}

fn other_door() -> DoorId {
    DoorId::parse("other").unwrap()
}

async fn member<S: AccountStore>(store: &S, name: &str, role: Slug) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

fn save(expected: u64, bytes: &[u8]) -> SaveWrite<'_> {
    SaveWrite {
        expected: Revision::from_raw(expected),
        season: 1,
        version: 3,
        bytes,
    }
}

fn world(expected: u64, bytes: &[u8], tick: Option<i64>) -> WorldWrite<'_> {
    WorldWrite {
        expected: Revision::from_raw(expected),
        version: 2,
        bytes,
        tick: tick.map(Day::from_raw),
    }
}

async fn actions<S: AuditStore>(store: &S) -> Vec<Action> {
    known_events(store.page(&AuditQuery::default()).await.unwrap())
        .into_iter()
        .map(|row| row.event.action)
        .filter(|action| action.name().starts_with("door."))
        .collect()
}

pub(crate) async fn a_save_round_trips_and_each_write_bumps_the_revision<H: Harness>() {
    let mut h = H::fresh().await;
    let s = h.store();
    let ann = member(s, "ann", Slug::MEMBER).await;
    assert_eq!(s.load_save(&door(), ann.id).await.unwrap(), None);

    let first = s
        .write_save(&door(), ann.id, &save(0, b"one"))
        .await
        .unwrap();
    assert_eq!(first, Revision::from_raw(1));
    let got = s.load_save(&door(), ann.id).await.unwrap().unwrap();
    assert_eq!(got.bytes, b"one");
    assert_eq!((got.season, got.version, got.revision), (1, 3, first));

    let second = s
        .write_save(&door(), ann.id, &save(1, b"two"))
        .await
        .unwrap();
    assert_eq!(second, Revision::from_raw(2));
    assert_eq!(
        s.load_save(&door(), ann.id).await.unwrap().unwrap().bytes,
        b"two"
    );

    // An empty save is a save. A door's saves are separate from another door's.
    s.write_save(&other_door(), ann.id, &save(0, b""))
        .await
        .unwrap();
    assert!(
        s.load_save(&other_door(), ann.id)
            .await
            .unwrap()
            .unwrap()
            .bytes
            .is_empty()
    );

    h.reopen().await;
    let got = h.store().load_save(&door(), ann.id).await.unwrap().unwrap();
    assert_eq!((got.bytes.as_slice(), got.revision), (&b"two"[..], second));
}

pub(crate) async fn a_stale_or_repeated_first_write_is_a_conflict_and_changes_nothing<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = member(s, "ann", Slug::MEMBER).await;
    s.write_save(&door(), ann.id, &save(0, b"a")).await.unwrap();

    // A second "first" write, a write against an old revision, and one from the future.
    for expected in [0, 5] {
        let refused = s.write_save(&door(), ann.id, &save(expected, b"b")).await;
        assert!(
            matches!(refused, Err(DoorStoreError::Conflict)),
            "{expected}"
        );
    }
    s.write_save(&door(), ann.id, &save(1, b"c")).await.unwrap();
    let stale = s.write_save(&door(), ann.id, &save(1, b"d")).await;
    assert!(matches!(stale, Err(DoorStoreError::Conflict)));
    let got = s.load_save(&door(), ann.id).await.unwrap().unwrap();
    assert_eq!(
        (got.bytes.as_slice(), got.revision),
        (&b"c"[..], Revision::from_raw(2))
    );
}

pub(crate) async fn concurrent_writers_of_one_revision_exactly_one_wins<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = member(s, "ann", Slug::MEMBER).await;
    s.write_save(&door(), ann.id, &save(0, b"base"))
        .await
        .unwrap();
    let mut tasks = Vec::new();
    for n in 0..8u8 {
        let s = s.clone();
        let id = ann.id;
        tasks.push(tokio::spawn(async move {
            s.write_save(&door(), id, &save(1, &[n])).await
        }));
    }
    let mut won = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => won += 1,
            Err(DoorStoreError::Conflict) => {}
            Err(other) => panic!("{other}"),
        }
    }
    assert_eq!(won, 1);
    assert_eq!(
        s.load_save(&door(), ann.id)
            .await
            .unwrap()
            .unwrap()
            .revision,
        Revision::from_raw(2)
    );
}

pub(crate) async fn the_caps_are_enforced_in_code_and_by_the_database<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = member(s, "ann", Slug::MEMBER).await;
    let big = vec![7u8; MAX_SAVE_BYTES];
    s.write_save(&door(), ann.id, &save(0, &big)).await.unwrap();
    let too_big = vec![7u8; MAX_SAVE_BYTES + 1];
    assert!(matches!(
        s.write_save(&door(), ann.id, &save(1, &too_big)).await,
        Err(DoorStoreError::TooLarge { cap }) if cap == MAX_SAVE_BYTES
    ));
    let world_ok = vec![1u8; MAX_WORLD_BYTES];
    s.write_world(&door(), &world(0, &world_ok, None))
        .await
        .unwrap();
    let world_big = vec![1u8; MAX_WORLD_BYTES + 1];
    assert!(matches!(
        s.write_world(&door(), &world(1, &world_big, None)).await,
        Err(DoorStoreError::TooLarge { cap }) if cap == MAX_WORLD_BYTES
    ));
    assert_eq!(
        s.load_save(&door(), ann.id)
            .await
            .unwrap()
            .unwrap()
            .bytes
            .len(),
        MAX_SAVE_BYTES
    );
    // The database has the same caps, so a bug that skipped the check still cannot store more.
    let id = ann.id.as_raw();
    for statement in [
        format!(
            "INSERT INTO door_save (door, account_id, season, version, revision, data, updated_at) \
             VALUES ('x', {id}, 1, 1, 1, {}, 0)",
            h.blob_sql(MAX_SAVE_BYTES + 1)
        ),
        format!(
            "INSERT INTO door_state (door, world, updated_at) VALUES ('x', {}, 0)",
            h.blob_sql(MAX_WORLD_BYTES + 1)
        ),
    ] {
        assert!(h.raw(&statement).await.is_err(), "over-cap row accepted");
    }
}

pub(crate) async fn an_unknown_account_cannot_have_a_save<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ghost = russh_bbs_core::store::AccountId::from_raw(9999);
    let refused = s.write_save(&door(), ghost, &save(0, b"x")).await;
    assert!(matches!(
        refused,
        Err(DoorStoreError::Store(StoreError::AccountNotFound))
    ));
    assert_eq!(s.load_save(&door(), ghost).await.unwrap(), None);
}

pub(crate) async fn the_world_has_its_own_revision_and_remembers_the_tick_day<H: Harness>() {
    let mut h = H::fresh().await;
    let s = h.store();
    assert_eq!(s.load_world(&door()).await.unwrap(), None);
    let r1 = s
        .write_world(&door(), &world(0, b"w1", Some(100)))
        .await
        .unwrap();
    assert_eq!(r1, Revision::from_raw(1));
    let got = s.load_world(&door()).await.unwrap().unwrap();
    assert_eq!(
        (got.bytes.as_slice(), got.version, got.last_tick),
        (&b"w1"[..], 2, Some(Day::from_raw(100)))
    );
    // A write with no tick keeps the last tick day.
    s.write_world(&door(), &world(1, b"w2", None))
        .await
        .unwrap();
    let got = s.load_world(&door()).await.unwrap().unwrap();
    assert_eq!(
        (got.bytes.as_slice(), got.last_tick),
        (&b"w2"[..], Some(Day::from_raw(100)))
    );
    // A stale write is a conflict; another door's world is separate.
    assert!(matches!(
        s.write_world(&door(), &world(1, b"w3", Some(101))).await,
        Err(DoorStoreError::Conflict)
    ));
    assert_eq!(s.load_world(&other_door()).await.unwrap(), None);
    h.reopen().await;
    let got = h.store().load_world(&door()).await.unwrap().unwrap();
    assert_eq!(got.revision, Revision::from_raw(2));
}

pub(crate) async fn a_season_row_made_first_does_not_hide_the_world_and_the_other_way_round<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let sysop = member(s, "sam", Slug::SYSOP).await;
    let snap = s.snapshot(Some(sysop.id)).await.unwrap();
    assert_eq!(s.season(&door()).await.unwrap(), 1);
    assert_eq!(end_season(s, &snap, &door()).await.unwrap(), 2);
    // The row exists with no world yet.
    assert_eq!(s.load_world(&door()).await.unwrap(), None);
    s.write_world(&door(), &world(0, b"w", Some(5)))
        .await
        .unwrap();
    assert_eq!(
        s.season(&door()).await.unwrap(),
        2,
        "writing a world keeps the season"
    );
    assert_eq!(end_season(s, &snap, &door()).await.unwrap(), 3);
    let got = s.load_world(&door()).await.unwrap().unwrap();
    assert_eq!(got.bytes, b"w", "ending a season keeps the world");
    assert_eq!(s.season(&other_door()).await.unwrap(), 1);
}

pub(crate) async fn staff_tools_change_things_and_write_one_audit_row_each<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let cosysop = member(s, "cora", Slug::COSYSOP).await;
    let ann = member(s, "ann", Slug::MEMBER).await;
    let snap = s.snapshot(Some(cosysop.id)).await.unwrap();

    s.write_save(&door(), ann.id, &save(0, b"x")).await.unwrap();
    assert!(reset_player(s, &snap, &door(), ann.id).await.unwrap());
    assert_eq!(s.load_save(&door(), ann.id).await.unwrap(), None);
    assert!(!reset_player(s, &snap, &door(), ann.id).await.unwrap());

    assert!(!s.is_barred(&door(), ann.id).await.unwrap());
    assert!(bar_player(s, &snap, &door(), ann.id).await.unwrap());
    assert!(!bar_player(s, &snap, &door(), ann.id).await.unwrap());
    assert!(s.is_barred(&door(), ann.id).await.unwrap());
    assert!(!s.is_barred(&other_door(), ann.id).await.unwrap());
    assert!(unbar_player(s, &snap, &door(), ann.id).await.unwrap());
    assert!(!unbar_player(s, &snap, &door(), ann.id).await.unwrap());
    assert!(!s.is_barred(&door(), ann.id).await.unwrap());
    assert_eq!(end_season(s, &snap, &door()).await.unwrap(), 2);

    // Newest first; the repeats that changed nothing wrote nothing.
    assert_eq!(
        actions(s).await,
        [
            Action::DoorSeasonEnded,
            Action::DoorPlayerUnbarred,
            Action::DoorPlayerBarred,
            Action::DoorPlayerReset,
        ]
    );
}

pub(crate) async fn staff_rank_and_permission_rules_hold_against_the_real_roles<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let moderator = member(s, "mona", Slug::MODERATOR).await;
    let cosysop = member(s, "cora", Slug::COSYSOP).await;
    let sysop = member(s, "sam", Slug::SYSOP).await;
    let ann = member(s, "ann", Slug::MEMBER).await;
    let m = s.snapshot(Some(moderator.id)).await.unwrap();
    let c = s.snapshot(Some(cosysop.id)).await.unwrap();

    // A moderator may bar (and lift) but not reset or end a season.
    assert!(bar_player(s, &m, &door(), ann.id).await.unwrap());
    assert!(matches!(
        reset_player(s, &m, &door(), ann.id).await,
        Err(DoorStaffError::Denied(_))
    ));
    assert!(matches!(
        end_season(s, &m, &door()).await,
        Err(DoorStaffError::Denied(_))
    ));
    // Nobody acts on themselves, an equal or a higher rank, or an account that is not there.
    for target in [
        moderator.id,
        cosysop.id,
        sysop.id,
        russh_bbs_core::store::AccountId::from_raw(9999),
    ] {
        assert!(
            matches!(
                bar_player(s, &m, &door(), target).await,
                Err(DoorStaffError::NotBelowYou)
            ),
            "{target}"
        );
    }
    assert!(matches!(
        bar_player(s, &c, &door(), sysop.id).await,
        Err(DoorStaffError::NotBelowYou)
    ));
    assert!(bar_player(s, &c, &door(), moderator.id).await.unwrap());
    // A member holds none of the staff tools.
    let a = s.snapshot(Some(ann.id)).await.unwrap();
    assert!(matches!(
        bar_player(s, &a, &door(), moderator.id).await,
        Err(DoorStaffError::Denied(_))
    ));
}

pub(crate) async fn a_staff_change_never_lands_without_its_audit_row<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let cosysop = member(s, "cora", Slug::COSYSOP).await;
    let ann = member(s, "ann", Slug::MEMBER).await;
    let snap = s.snapshot(Some(cosysop.id)).await.unwrap();
    s.write_save(&door(), ann.id, &save(0, b"x")).await.unwrap();
    h.break_audit_inserts().await;
    assert!(reset_player(s, &snap, &door(), ann.id).await.is_err());
    assert!(bar_player(s, &snap, &door(), ann.id).await.is_err());
    assert!(end_season(s, &snap, &door()).await.is_err());
    assert!(
        s.load_save(&door(), ann.id).await.unwrap().is_some(),
        "the save survived"
    );
    assert!(!s.is_barred(&door(), ann.id).await.unwrap());
    assert_eq!(s.season(&door()).await.unwrap(), 1);
}

pub(crate) async fn deleting_an_account_deletes_its_door_data<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let cosysop = member(s, "cora", Slug::COSYSOP).await;
    let ann = member(s, "ann", Slug::MEMBER).await;
    let snap = s.snapshot(Some(cosysop.id)).await.unwrap();
    s.write_save(&door(), ann.id, &save(0, b"x")).await.unwrap();
    bar_player(s, &snap, &door(), ann.id).await.unwrap();
    h.raw(&format!("DELETE FROM accounts WHERE id = {}", ann.id))
        .await
        .unwrap();
    for table in ["door_save", "door_bar"] {
        let n = h
            .texts(&format!("SELECT CAST(count(*) AS TEXT) FROM {table}"))
            .await;
        assert_eq!(n, ["0"], "{table}");
    }
}

pub(crate) async fn the_roles_hold_the_door_staff_permissions_the_docs_say<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let holds = |role: Slug| async move {
        let info = store.role(&role).await.unwrap().unwrap();
        [
            Permission::DoorsRun,
            Permission::DoorsResetPlayer,
            Permission::DoorsEndSeason,
            Permission::DoorsBarPlayer,
        ]
        .into_iter()
        .filter(|p| info.permissions.contains(p))
        .collect::<BTreeSet<_>>()
    };
    let play: BTreeSet<_> = [Permission::DoorsRun].into();
    assert_eq!(holds(Slug::WAITING).await, BTreeSet::new());
    for role in [Slug::MEMBER, Slug::TRUSTED] {
        assert_eq!(holds(role).await, play);
    }
    let bar: BTreeSet<_> = [Permission::DoorsRun, Permission::DoorsBarPlayer].into();
    assert_eq!(holds(Slug::MODERATOR).await, bar);
    let all: BTreeSet<_> = [
        Permission::DoorsRun,
        Permission::DoorsResetPlayer,
        Permission::DoorsEndSeason,
        Permission::DoorsBarPlayer,
    ]
    .into();
    assert_eq!(holds(Slug::COSYSOP).await, all);
    assert_eq!(holds(Slug::SYSOP).await, all);
}

pub(crate) async fn a_broken_store_errors_instead_of_reading_as_empty<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let ann = member(s, "ann", Slug::MEMBER).await;
    h.close().await;
    assert!(s.load_save(&door(), ann.id).await.is_err());
    assert!(s.load_world(&door()).await.is_err());
    assert!(s.season(&door()).await.is_err());
    assert!(s.is_barred(&door(), ann.id).await.is_err());
    assert!(s.write_save(&door(), ann.id, &save(0, b"x")).await.is_err());
}
