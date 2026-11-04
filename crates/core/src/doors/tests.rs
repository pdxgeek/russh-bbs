#![allow(clippy::unwrap_used, clippy::unused_async_trait_impl)]

use std::sync::Mutex;

use super::*;
use crate::access::{Permission, Slug, Snapshot, Via};
use crate::audit::{Action, AuditEvent};
use crate::store::{AccountId, StoreError};

fn id(n: i64) -> AccountId {
    AccountId::from_raw(n)
}

fn staff(n: i64, rank: u8, permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(id(n)),
        &Slug::COSYSOP,
        rank,
        &Via::Account,
        permissions.iter().copied(),
    )
}

fn door() -> DoorId {
    DoorId::parse("coin-toss").unwrap()
}

/// A store that knows account ranks and remembers the audit rows it was asked to write.
#[derive(Default)]
struct Fake {
    ranks: Vec<(i64, u8)>,
    audits: Mutex<Vec<AuditEvent>>,
}

impl Fake {
    fn with(ranks: &[(i64, u8)]) -> Self {
        Self {
            ranks: ranks.to_vec(),
            audits: Mutex::default(),
        }
    }

    fn actions(&self) -> Vec<Action> {
        self.audits
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.action)
            .collect()
    }
}

impl DoorStore for Fake {
    async fn load_save(&self, _: &DoorId, _: AccountId) -> Result<Option<StoredSave>, StoreError> {
        Ok(None)
    }
    async fn write_save(
        &self,
        _: &DoorId,
        _: AccountId,
        _: &SaveWrite<'_>,
    ) -> Result<Revision, DoorStoreError> {
        Ok(Revision::from_raw(1))
    }
    async fn load_world(&self, _: &DoorId) -> Result<Option<StoredWorld>, StoreError> {
        Ok(None)
    }
    async fn write_world(
        &self,
        _: &DoorId,
        _: &WorldWrite<'_>,
    ) -> Result<Revision, DoorStoreError> {
        Ok(Revision::from_raw(1))
    }
    async fn season(&self, _: &DoorId) -> Result<u32, StoreError> {
        Ok(1)
    }
    async fn is_barred(&self, _: &DoorId, _: AccountId) -> Result<bool, StoreError> {
        Ok(false)
    }
    async fn account_rank(&self, account: AccountId) -> Result<Option<u8>, StoreError> {
        Ok(self
            .ranks
            .iter()
            .find(|(n, _)| *n == account.as_raw())
            .map(|(_, rank)| *rank))
    }
    async fn reset_player(
        &self,
        _: &DoorId,
        _: AccountId,
        audit: &AuditEvent,
    ) -> Result<bool, StoreError> {
        self.audits.lock().unwrap().push(audit.clone());
        Ok(true)
    }
    async fn end_season(&self, _: &DoorId, audit: &AuditEvent) -> Result<u32, StoreError> {
        self.audits.lock().unwrap().push(audit.clone());
        Ok(2)
    }
    async fn bar_player(
        &self,
        _: &DoorId,
        _: AccountId,
        _: AccountId,
        audit: &AuditEvent,
    ) -> Result<bool, StoreError> {
        self.audits.lock().unwrap().push(audit.clone());
        Ok(true)
    }
    async fn unbar_player(
        &self,
        _: &DoorId,
        _: AccountId,
        audit: &AuditEvent,
    ) -> Result<bool, StoreError> {
        self.audits.lock().unwrap().push(audit.clone());
        Ok(true)
    }
}

#[test]
fn door_ids_are_short_slugs_and_make_valid_job_names() {
    assert!(DoorId::parse("coin-toss").is_ok());
    assert!(DoorId::parse("").is_err());
    assert!(DoorId::parse("Coin").is_err());
    assert!(DoorId::parse("a b").is_err());
    assert!(DoorId::parse(&"a".repeat(MAX_DOOR_ID_CHARS)).is_ok());
    assert!(DoorId::parse(&"a".repeat(MAX_DOOR_ID_CHARS + 1)).is_err());
    let longest = DoorId::parse(&"a".repeat(MAX_DOOR_ID_CHARS)).unwrap();
    assert_eq!(longest.tick_job().as_str().len(), 5 + MAX_DOOR_ID_CHARS);
    assert_eq!(door().tick_job().as_str(), "door-coin-toss");
}

#[tokio::test]
async fn every_staff_tool_needs_its_own_permission() {
    let store = Fake::with(&[(1, 40), (2, 10)]);
    let none = staff(1, 40, &[Permission::DoorsRun]);
    assert!(matches!(
        reset_player(&store, &none, &door(), id(2)).await,
        Err(DoorStaffError::Denied(_))
    ));
    assert!(matches!(
        end_season(&store, &none, &door()).await,
        Err(DoorStaffError::Denied(_))
    ));
    assert!(matches!(
        bar_player(&store, &none, &door(), id(2)).await,
        Err(DoorStaffError::Denied(_))
    ));
    assert!(matches!(
        unbar_player(&store, &none, &door(), id(2)).await,
        Err(DoorStaffError::Denied(_))
    ));
    // Holding one tool's permission does not open another.
    let only_bar = staff(1, 40, &[Permission::DoorsBarPlayer]);
    assert!(matches!(
        reset_player(&store, &only_bar, &door(), id(2)).await,
        Err(DoorStaffError::Denied(_))
    ));
    assert!(store.actions().is_empty(), "a refusal writes no audit row");
}

#[tokio::test]
async fn staff_act_only_on_players_strictly_below_their_rank() {
    let store = Fake::with(&[(1, 40), (2, 10), (3, 40), (4, 100)]);
    let all = staff(
        1,
        40,
        &[
            Permission::DoorsResetPlayer,
            Permission::DoorsEndSeason,
            Permission::DoorsBarPlayer,
        ],
    );
    assert!(reset_player(&store, &all, &door(), id(2)).await.unwrap());
    assert!(bar_player(&store, &all, &door(), id(2)).await.unwrap());
    assert!(unbar_player(&store, &all, &door(), id(2)).await.unwrap());
    for target in [id(1), id(3), id(4), id(99)] {
        assert!(matches!(
            reset_player(&store, &all, &door(), target).await,
            Err(DoorStaffError::NotBelowYou)
        ));
        assert!(matches!(
            bar_player(&store, &all, &door(), target).await,
            Err(DoorStaffError::NotBelowYou)
        ));
    }
    assert_eq!(end_season(&store, &all, &door()).await.unwrap(), 2);
    assert_eq!(
        store.actions(),
        vec![
            Action::DoorPlayerReset,
            Action::DoorPlayerBarred,
            Action::DoorPlayerUnbarred,
            Action::DoorSeasonEnded,
        ]
    );
}
