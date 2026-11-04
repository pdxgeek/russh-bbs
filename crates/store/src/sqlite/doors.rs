//! Door saves, the shared world, seasons, bars and the staff tools over SQLite (M4-D0, D98). The
//! rules are in core's `doors`; the row mapping and the caps in `door_rows`. Writes are one
//! upsert whose `WHERE` is the compare-and-swap, so a stale revision changes nothing.

use russh_bbs_core::access::AccessStore as _;
use russh_bbs_core::audit::AuditEvent;
use russh_bbs_core::doors::{
    DoorId, DoorStore, DoorStoreError, Revision, SaveWrite, StoredSave, StoredWorld, WorldWrite,
};
use russh_bbs_core::store::{AccountId, StoreError};

use super::SqliteStore;
use crate::door_rows::{SaveRow, WorldRow, check_save, check_world, next, revision, small};
use crate::error::{backend, kind};

/// A foreign-key failure on a save or a bar means the account does not exist.
fn account_missing(error: sqlx::Error) -> StoreError {
    if kind(&error) == Some(sqlx::error::ErrorKind::ForeignKeyViolation) {
        StoreError::AccountNotFound
    } else {
        backend(error)
    }
}

impl DoorStore for SqliteStore {
    async fn load_save(
        &self,
        door: &DoorId,
        account: AccountId,
    ) -> Result<Option<StoredSave>, StoreError> {
        let door = door.as_str();
        let id = account.as_raw();
        let row = sqlx::query_as!(
            SaveRow,
            r#"SELECT season, version, revision, data, updated_at
               FROM door_save WHERE door = ? AND account_id = ?"#,
            door,
            id,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        row.map(SaveRow::into_save).transpose()
    }

    async fn write_save(
        &self,
        door: &DoorId,
        account: AccountId,
        write: &SaveWrite<'_>,
    ) -> Result<Revision, DoorStoreError> {
        check_save(write)?;
        let door = door.as_str();
        let id = account.as_raw();
        let new = next(write.expected)?;
        let season = i64::from(write.season);
        let version = i64::from(write.version);
        let bytes = write.bytes;
        let at = self.clock().now().as_raw();
        let done = sqlx::query!(
            r#"INSERT INTO door_save (door, account_id, season, version, revision, data, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
               ON CONFLICT (door, account_id) DO UPDATE SET
                   season = excluded.season, version = excluded.version,
                   revision = excluded.revision, data = excluded.data,
                   updated_at = excluded.updated_at
               WHERE door_save.revision = excluded.revision - 1"#,
            door,
            id,
            season,
            version,
            new,
            bytes,
            at,
        )
        .execute(self.pool())
        .await
        .map_err(account_missing)?;
        if done.rows_affected() == 0 {
            return Err(DoorStoreError::Conflict);
        }
        Ok(revision(new)?)
    }

    async fn load_world(&self, door: &DoorId) -> Result<Option<StoredWorld>, StoreError> {
        let door = door.as_str();
        let row = sqlx::query_as!(
            WorldRow,
            r#"SELECT world_revision, world_version, world, last_tick_day
               FROM door_state WHERE door = ?"#,
            door,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        row.map_or(Ok(None), WorldRow::into_world)
    }

    async fn write_world(
        &self,
        door: &DoorId,
        write: &WorldWrite<'_>,
    ) -> Result<Revision, DoorStoreError> {
        check_world(write)?;
        let door = door.as_str();
        let new = next(write.expected)?;
        let version = i64::from(write.version);
        let bytes = write.bytes;
        let tick = write.tick.map(russh_bbs_core::limits::Day::as_raw);
        let at = self.clock().now().as_raw();
        let done = sqlx::query!(
            r#"INSERT INTO door_state
                   (door, season, world_revision, world_version, world, last_tick_day, updated_at)
               VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6)
               ON CONFLICT (door) DO UPDATE SET
                   world_revision = excluded.world_revision,
                   world_version = excluded.world_version, world = excluded.world,
                   last_tick_day = COALESCE(excluded.last_tick_day, door_state.last_tick_day),
                   updated_at = excluded.updated_at
               WHERE door_state.world_revision = excluded.world_revision - 1"#,
            door,
            new,
            version,
            bytes,
            tick,
            at,
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        if done.rows_affected() == 0 {
            return Err(DoorStoreError::Conflict);
        }
        Ok(revision(new)?)
    }

    async fn season(&self, door: &DoorId) -> Result<u32, StoreError> {
        let door = door.as_str();
        let season = sqlx::query_scalar!(r#"SELECT season FROM door_state WHERE door = ?"#, door)
            .fetch_optional(self.pool())
            .await
            .map_err(backend)?;
        season.map_or(Ok(1), |season| small(season, "season"))
    }

    async fn is_barred(&self, door: &DoorId, account: AccountId) -> Result<bool, StoreError> {
        let door = door.as_str();
        let id = account.as_raw();
        let found = sqlx::query_scalar!(
            r#"SELECT 1 AS "one!: i64" FROM door_bar WHERE door = ? AND account_id = ?"#,
            door,
            id,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        Ok(found.is_some())
    }

    async fn account_rank(&self, account: AccountId) -> Result<Option<u8>, StoreError> {
        let id = account.as_raw();
        let exists =
            sqlx::query_scalar!(r#"SELECT 1 AS "one!: i64" FROM accounts WHERE id = ?"#, id)
                .fetch_optional(self.pool())
                .await
                .map_err(backend)?;
        if exists.is_none() {
            return Ok(None);
        }
        Ok(Some(
            self.snapshot(Some(account)).await?.top_rank().unwrap_or(0),
        ))
    }

    async fn reset_player(
        &self,
        door: &DoorId,
        account: AccountId,
        audit: &AuditEvent,
    ) -> Result<bool, StoreError> {
        let door = door.as_str();
        let id = account.as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let done = sqlx::query!(
            "DELETE FROM door_save WHERE door = ? AND account_id = ?",
            door,
            id
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if done.rows_affected() == 0 {
            return Ok(false);
        }
        self.audit_in(&mut tx, audit).await?;
        tx.commit().await.map_err(backend)?;
        Ok(true)
    }

    async fn end_season(&self, door: &DoorId, audit: &AuditEvent) -> Result<u32, StoreError> {
        let door = door.as_str();
        let at = self.clock().now().as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        // A door with no row yet is in season 1, so ending it makes season 2.
        let season = sqlx::query_scalar!(
            r#"INSERT INTO door_state (door, season, updated_at) VALUES (?1, 2, ?2)
               ON CONFLICT (door) DO UPDATE SET season = door_state.season + 1, updated_at = ?2
               RETURNING season AS "season!""#,
            door,
            at,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        self.audit_in(&mut tx, audit).await?;
        tx.commit().await.map_err(backend)?;
        small(season, "season")
    }

    async fn bar_player(
        &self,
        door: &DoorId,
        account: AccountId,
        by: AccountId,
        audit: &AuditEvent,
    ) -> Result<bool, StoreError> {
        let door = door.as_str();
        let id = account.as_raw();
        let by = by.as_raw();
        let at = self.clock().now().as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let done = sqlx::query!(
            r#"INSERT INTO door_bar (door, account_id, barred_by, at) VALUES (?, ?, ?, ?)
               ON CONFLICT (door, account_id) DO NOTHING"#,
            door,
            id,
            by,
            at,
        )
        .execute(&mut *tx)
        .await
        .map_err(account_missing)?;
        if done.rows_affected() == 0 {
            return Ok(false);
        }
        self.audit_in(&mut tx, audit).await?;
        tx.commit().await.map_err(backend)?;
        Ok(true)
    }

    async fn unbar_player(
        &self,
        door: &DoorId,
        account: AccountId,
        audit: &AuditEvent,
    ) -> Result<bool, StoreError> {
        let door = door.as_str();
        let id = account.as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let done = sqlx::query!(
            "DELETE FROM door_bar WHERE door = ? AND account_id = ?",
            door,
            id
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if done.rows_affected() == 0 {
            return Ok(false);
        }
        self.audit_in(&mut tx, audit).await?;
        tx.commit().await.map_err(backend)?;
        Ok(true)
    }
}
