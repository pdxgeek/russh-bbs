//! Chat room definitions over SQLite (M3-F). Rooms only: nothing said in a room is stored.

use russh_bbs_core::access::Slug;
use russh_bbs_core::audit::events;
use russh_bbs_core::chat::{CreateRoomError, Room, RoomId, RoomStore};
use russh_bbs_core::store::{AccountId, StoreError};
use sqlx::error::ErrorKind;

use super::SqliteStore;
use crate::chat_rows::{cap, room};
use crate::error::{backend, kind};

impl RoomStore for SqliteStore {
    async fn create_room(
        &self,
        name: &Slug,
        actor: AccountId,
        max_rooms: usize,
    ) -> Result<Room, CreateRoomError> {
        let now = self.clock().now().as_raw();
        let by = actor.as_raw();
        let text = name.as_str();
        let max = cap(max_rooms);
        let mut tx = self.pool().begin().await.map_err(backend)?;
        // One statement counts and inserts, and it is the transaction's first write, so two
        // creations cannot both take the last place. No row means the cap is reached.
        let id = sqlx::query_scalar!(
            r#"INSERT INTO chat_room (name, created_at, created_by)
               SELECT ?1, ?2, ?3
               WHERE (SELECT count(*) FROM chat_room WHERE closed_at IS NULL) < ?4
               RETURNING id AS "id!""#,
            text,
            now,
            by,
            max,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => CreateRoomError::NameTaken,
            Some(ErrorKind::ForeignKeyViolation) => {
                CreateRoomError::Store(StoreError::AccountNotFound)
            }
            _ => CreateRoomError::Store(backend(error)),
        })?
        .ok_or(CreateRoomError::TooMany)?;
        let created = room(id, text, now)?;
        if let Some(resource) = created.id.resource() {
            self.audit_in(
                &mut tx,
                &events::chat_room_created(actor, resource, &created.name),
            )
            .await?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(created)
    }

    async fn close_room(&self, id: RoomId, actor: AccountId) -> Result<bool, StoreError> {
        let Some(resource) = id.resource() else {
            return Ok(false);
        };
        let now = self.clock().now().as_raw();
        let raw = id.as_raw();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let closed = sqlx::query!(
            "UPDATE chat_room SET closed_at = max(?1, created_at) WHERE id = ?2 AND closed_at IS NULL",
            now,
            raw,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        if closed == 0 {
            return Ok(false);
        }
        self.audit_in(&mut tx, &events::chat_room_closed(actor, resource))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(true)
    }

    async fn open_rooms(&self) -> Result<Vec<Room>, StoreError> {
        sqlx::query!(
            r#"SELECT id AS "id!", name, created_at FROM chat_room
               WHERE closed_at IS NULL ORDER BY name, id"#
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(|row| room(row.id, &row.name, row.created_at))
        .collect()
    }
}
