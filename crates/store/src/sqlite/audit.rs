//! The audit log over SQLite (M2-C, D58): the one write helper, [`SqliteStore::audit_in`],
//! and the bounded read. The table is append-only in the database itself (migration 0004);
//! there is deliberately no update or delete here.

use russh_bbs_core::audit::{AuditEvent, AuditId, AuditQuery, AuditStore, LogEntry};
use russh_bbs_core::store::StoreError;
use sqlx::SqliteConnection;

use super::SqliteStore;
use crate::audit_rows::{AuditRow, Columns, Filter};
use crate::error::backend;

impl SqliteStore {
    /// **The** audit write. Every store method that changes something auditable calls this on
    /// its own transaction's connection, so the row commits or rolls back with the change.
    /// The time is the store's clock, never the caller's.
    pub(crate) async fn audit_in(
        &self,
        conn: &mut SqliteConnection,
        event: &AuditEvent,
    ) -> Result<AuditId, StoreError> {
        let at = self.clock().now().as_raw();
        let c = Columns::of(event);
        sqlx::query_scalar!(
            r#"INSERT INTO audit_log
                   (at, actor_kind, actor_id, action, target_kind, target_ref,
                    resource_kind, resource_id, detail)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
               RETURNING id AS "id!""#,
            at,
            c.actor_kind,
            c.actor_id,
            c.action,
            c.target_kind,
            c.target_ref,
            c.resource_kind,
            c.resource_id,
            c.detail,
        )
        .fetch_one(conn)
        .await
        .map(AuditId::from_raw)
        .map_err(backend)
    }
}

impl AuditStore for SqliteStore {
    async fn append(&self, event: &AuditEvent) -> Result<AuditId, StoreError> {
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let id = self.audit_in(&mut tx, event).await?;
        tx.commit().await.map_err(backend)?;
        Ok(id)
    }

    async fn page(&self, query: &AuditQuery) -> Result<Vec<LogEntry>, StoreError> {
        let f = Filter::of(query);
        let rows = sqlx::query_as!(
            AuditRow,
            r#"SELECT id AS "id!", at, actor_kind, actor_id, action, target_kind, target_ref,
                      resource_kind, resource_id, detail
               FROM audit_log
               WHERE (?1 IS NULL OR actor_kind = ?1)
                 AND (?2 IS NULL OR actor_id = ?2)
                 AND (?3 IS NULL OR action = ?3)
                 AND (?4 IS NULL OR at >= ?4)
                 AND (?5 IS NULL OR at < ?5)
                 AND (?6 IS NULL OR id < ?6)
               ORDER BY id DESC
               LIMIT ?7"#,
            f.actor_kind,
            f.actor_id,
            f.action,
            f.since,
            f.until,
            f.before,
            f.limit,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        rows.into_iter().map(AuditRow::into_entry).collect()
    }
}
