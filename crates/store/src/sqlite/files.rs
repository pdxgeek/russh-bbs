//! File areas over SQLite (M4-F2): areas, files, tags, search and download lists. The rules are
//! in core's `files`; this file is SQL and the transactions that must be atomic (the audited
//! area, decision and delete, the capped tag and list adds).

use russh_bbs_core::audit::{Actor, AuditEvent, events};
use russh_bbs_core::files::{
    AreaId, AreaRow, Decision, FileId, FileRow, FileState, FileStore, ListOutcome, NewArea,
    NewFile, ScanState, TagOutcome,
};
use russh_bbs_core::sanitize::Sanitized;
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

use super::SqliteStore;
use crate::error::backend;
use crate::file_rows::{
    FileRecord, area, int, json_area_ids, json_file_ids, json_terms, name_key, size,
};

impl SqliteStore {
    /// The tags of each of `ids`, in the same order as `ids`.
    async fn tags_of(&self, ids: &[i64]) -> Result<Vec<Vec<String>>, StoreError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let list = json_file_ids(ids);
        let rows = sqlx::query!(
            r#"SELECT file_id AS "file_id!", tag AS "tag!" FROM file_tag
               WHERE file_id IN (SELECT value FROM json_each(?)) ORDER BY file_id, tag"#,
            list
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        Ok(ids
            .iter()
            .map(|id| {
                rows.iter()
                    .filter(|r| r.file_id == *id)
                    .map(|r| r.tag.clone())
                    .collect()
            })
            .collect())
    }

    async fn with_tags(&self, records: Vec<FileRecord>) -> Result<Vec<FileRow>, StoreError> {
        let ids: Vec<i64> = records.iter().map(|r| r.id).collect();
        let tags = self.tags_of(&ids).await?;
        records
            .into_iter()
            .zip(tags)
            .map(|(record, tags)| record.into_row(tags))
            .collect()
    }
}

impl FileStore for SqliteStore {
    async fn areas(&self) -> Result<Vec<AreaRow>, StoreError> {
        sqlx::query!(
            r#"SELECT a.id AS "id!", a.name, a.description, a.restricted AS "restricted!: bool",
                      a.auto_approve AS "auto_approve!: bool",
                      (SELECT count(*) FROM file f WHERE f.area_id = a.id AND f.state = 'approved') AS "files!: i64"
               FROM file_area a ORDER BY a.id"#
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(|r| area(r.id, r.name, r.description, r.restricted, r.auto_approve, r.files))
        .collect()
    }

    async fn area(&self, id: AreaId) -> Result<Option<AreaRow>, StoreError> {
        let id = id.as_raw();
        sqlx::query!(
            r#"SELECT a.id AS "id!", a.name, a.description, a.restricted AS "restricted!: bool",
                      a.auto_approve AS "auto_approve!: bool",
                      (SELECT count(*) FROM file f WHERE f.area_id = a.id AND f.state = 'approved') AS "files!: i64"
               FROM file_area a WHERE a.id = ?"#,
            id
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        .map(|r| area(r.id, r.name, r.description, r.restricted, r.auto_approve, r.files))
        .transpose()
    }

    async fn create_area(
        &self,
        new: &NewArea,
        actor: AccountId,
        now: UnixSeconds,
    ) -> Result<Option<AreaId>, StoreError> {
        let name = new.name.as_str();
        let key = name_key(name);
        let description = new.description.as_str();
        let (restricted, auto_approve) = (new.restricted, new.auto_approve);
        let by = actor.as_raw();
        let at = now.as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let id = sqlx::query_scalar!(
            r#"INSERT INTO file_area (name, name_key, description, restricted, auto_approve, created_by, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?)
               ON CONFLICT (name_key) DO NOTHING
               RETURNING id AS "id!""#,
            name,
            key,
            description,
            restricted,
            auto_approve,
            by,
            at,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(id) = id else {
            return Ok(None);
        };
        let id = AreaId::from_raw(id);
        if let Some(resource) = id.resource() {
            self.audit_in(
                &mut tx,
                &events::file_area_created(actor, resource, restricted, auto_approve),
            )
            .await?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(Some(id))
    }

    async fn insert_file(&self, new: &NewFile) -> Result<FileId, StoreError> {
        let area = new.area.as_raw();
        let uploader = new.uploader.as_raw();
        let name = new.name.as_str();
        let original = new.original_name.as_str();
        let bytes = size(new.size);
        let sha = new.sha256.as_str();
        let key = new.blob_key.as_str();
        let at = new.now.as_raw();
        let id = sqlx::query_scalar!(
            r#"INSERT INTO file (area_id, uploader_id, name, original_name, size, sha256, blob_key, state, scan, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, 'quarantine', 'unscanned', ?)
               RETURNING id AS "id!""#,
            area,
            uploader,
            name,
            original,
            bytes,
            sha,
            key,
            at,
        )
        .fetch_one(self.pool())
        .await
        .map_err(backend)?;
        Ok(FileId::from_raw(id))
    }

    async fn file(&self, id: FileId) -> Result<Option<FileRow>, StoreError> {
        let id = id.as_raw();
        let record = sqlx::query_as!(
            FileRecord,
            r#"SELECT f.id AS "id!", f.area_id AS "area_id!", a.name AS "area_name!", f.uploader_id,
                      u.username AS "uploader?", f.name, f.original_name, f.description, f.size,
                      f.sha256, f.blob_key, f.state, f.scan, f.scan_note, f.created_at,
                      f.decided_at, f.downloads
               FROM file f JOIN file_area a ON a.id = f.area_id
               LEFT JOIN accounts u ON u.id = f.uploader_id
               WHERE f.id = ?"#,
            id
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        match record {
            Some(record) => Ok(self.with_tags(vec![record]).await?.pop()),
            None => Ok(None),
        }
    }

    async fn files(
        &self,
        area_id: AreaId,
        state: FileState,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<FileRow>, StoreError> {
        let id = area_id.as_raw();
        let state = state.name();
        let (limit, offset) = (int(limit), int(offset));
        let records = sqlx::query_as!(
            FileRecord,
            r#"SELECT f.id AS "id!", f.area_id AS "area_id!", a.name AS "area_name!", f.uploader_id,
                      u.username AS "uploader?", f.name, f.original_name, f.description, f.size,
                      f.sha256, f.blob_key, f.state, f.scan, f.scan_note, f.created_at,
                      f.decided_at, f.downloads
               FROM file f JOIN file_area a ON a.id = f.area_id
               LEFT JOIN accounts u ON u.id = f.uploader_id
               WHERE f.area_id = ? AND f.state = ?
               ORDER BY f.id DESC LIMIT ? OFFSET ?"#,
            id,
            state,
            limit,
            offset
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        self.with_tags(records).await
    }

    async fn pending_in(
        &self,
        areas: &[AreaId],
        limit: u32,
        offset: u32,
    ) -> Result<Vec<FileRow>, StoreError> {
        if areas.is_empty() {
            return Ok(Vec::new());
        }
        let ids = json_area_ids(areas);
        let (limit, offset) = (int(limit), int(offset));
        let records = sqlx::query_as!(
            FileRecord,
            r#"SELECT f.id AS "id!", f.area_id AS "area_id!", a.name AS "area_name!", f.uploader_id,
                      u.username AS "uploader?", f.name, f.original_name, f.description, f.size,
                      f.sha256, f.blob_key, f.state, f.scan, f.scan_note, f.created_at,
                      f.decided_at, f.downloads
               FROM file f JOIN file_area a ON a.id = f.area_id
               LEFT JOIN accounts u ON u.id = f.uploader_id
               WHERE f.state = 'quarantine' AND f.area_id IN (SELECT value FROM json_each(?))
               ORDER BY f.id ASC LIMIT ? OFFSET ?"#,
            ids,
            limit,
            offset
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        self.with_tags(records).await
    }

    async fn own_pending(
        &self,
        account: AccountId,
        limit: u32,
    ) -> Result<Vec<FileRow>, StoreError> {
        let id = account.as_raw();
        let limit = int(limit);
        let records = sqlx::query_as!(
            FileRecord,
            r#"SELECT f.id AS "id!", f.area_id AS "area_id!", a.name AS "area_name!", f.uploader_id,
                      u.username AS "uploader?", f.name, f.original_name, f.description, f.size,
                      f.sha256, f.blob_key, f.state, f.scan, f.scan_note, f.created_at,
                      f.decided_at, f.downloads
               FROM file f JOIN file_area a ON a.id = f.area_id
               LEFT JOIN accounts u ON u.id = f.uploader_id
               WHERE f.uploader_id = ? AND f.state = 'quarantine'
               ORDER BY f.id ASC LIMIT ?"#,
            id,
            limit
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        self.with_tags(records).await
    }

    async fn set_scan(
        &self,
        id: FileId,
        scan: ScanState,
        note: &Sanitized,
    ) -> Result<bool, StoreError> {
        let id = id.as_raw();
        let scan = scan.name();
        let note = note.as_str();
        let done = sqlx::query!(
            "UPDATE file SET scan = ?, scan_note = ? WHERE id = ? AND state = 'quarantine'",
            scan,
            note,
            id
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        Ok(done.rows_affected() > 0)
    }

    async fn decide(
        &self,
        id: FileId,
        decision: &Decision,
        by: Actor,
        now: UnixSeconds,
        audit: &AuditEvent,
    ) -> Result<bool, StoreError> {
        let id = id.as_raw();
        let (state, key) = match decision {
            Decision::Approve { blob_key } => ("approved", Some(blob_key.as_str())),
            Decision::Reject => ("rejected", None),
        };
        let at = now.as_raw();
        let who = match by {
            Actor::Account(account) => Some(account.as_raw()),
            Actor::System => None,
        };
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let done = sqlx::query!(
            "UPDATE file SET state = ?, blob_key = COALESCE(?, blob_key), decided_at = ?, decided_by = ?
             WHERE id = ? AND state = 'quarantine'",
            state,
            key,
            at,
            who,
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

    async fn delete_file(&self, id: FileId, audit: &AuditEvent) -> Result<bool, StoreError> {
        let id = id.as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let done = sqlx::query!("DELETE FROM file WHERE id = ?", id)
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

    async fn add_tag(&self, id: FileId, tag: &str, max: u32) -> Result<TagOutcome, StoreError> {
        let id = id.as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let has = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM file_tag WHERE file_id = ? AND tag = ?"#,
            id,
            tag
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if has > 0 {
            return Ok(TagOutcome::Already);
        }
        let n = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM file_tag WHERE file_id = ?"#,
            id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if n >= i64::from(max) {
            return Ok(TagOutcome::Full);
        }
        sqlx::query!("INSERT INTO file_tag (file_id, tag) VALUES (?, ?)", id, tag)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(TagOutcome::Added)
    }

    async fn remove_tag(&self, id: FileId, tag: &str) -> Result<bool, StoreError> {
        let id = id.as_raw();
        let done = sqlx::query!(
            "DELETE FROM file_tag WHERE file_id = ? AND tag = ?",
            id,
            tag
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        Ok(done.rows_affected() > 0)
    }

    async fn set_description(
        &self,
        id: FileId,
        description: &Sanitized,
    ) -> Result<bool, StoreError> {
        let id = id.as_raw();
        let text = description.as_str();
        let done = sqlx::query!("UPDATE file SET description = ? WHERE id = ?", text, id)
            .execute(self.pool())
            .await
            .map_err(backend)?;
        Ok(done.rows_affected() > 0)
    }

    async fn search(
        &self,
        areas: &[AreaId],
        terms: &[String],
        limit: u32,
        offset: u32,
    ) -> Result<Vec<FileRow>, StoreError> {
        let words = json_terms(terms);
        if words == "[]" || areas.is_empty() {
            return Ok(Vec::new());
        }
        let ids = json_area_ids(areas);
        let (limit, offset) = (int(limit), int(offset));
        let records = sqlx::query_as!(
            FileRecord,
            r#"SELECT f.id AS "id!", f.area_id AS "area_id!", a.name AS "area_name!", f.uploader_id,
                      u.username AS "uploader?", f.name, f.original_name, f.description, f.size,
                      f.sha256, f.blob_key, f.state, f.scan, f.scan_note, f.created_at,
                      f.decided_at, f.downloads
               FROM file f JOIN file_area a ON a.id = f.area_id
               LEFT JOIN accounts u ON u.id = f.uploader_id
               WHERE f.state = 'approved' AND f.area_id IN (SELECT value FROM json_each(?))
                 AND NOT EXISTS (
                     SELECT 1 FROM json_each(?) t WHERE NOT (
                         instr(lower(f.name), t.value) > 0
                         OR instr(lower(f.description), t.value) > 0
                         OR EXISTS (SELECT 1 FROM file_tag g WHERE g.file_id = f.id AND instr(g.tag, t.value) > 0)))
               ORDER BY f.id DESC LIMIT ? OFFSET ?"#,
            ids,
            words,
            limit,
            offset
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        self.with_tags(records).await
    }

    async fn note_download(&self, id: FileId) -> Result<(), StoreError> {
        let id = id.as_raw();
        sqlx::query!("UPDATE file SET downloads = downloads + 1 WHERE id = ?", id)
            .execute(self.pool())
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn list_add(
        &self,
        account: AccountId,
        id: FileId,
        now: UnixSeconds,
        max: u32,
    ) -> Result<ListOutcome, StoreError> {
        let (account, id, at) = (account.as_raw(), id.as_raw(), now.as_raw());
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let has = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM download_list WHERE account_id = ? AND file_id = ?"#,
            account,
            id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if has > 0 {
            return Ok(ListOutcome::Already);
        }
        let n = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM download_list WHERE account_id = ?"#,
            account
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if n >= i64::from(max) {
            return Ok(ListOutcome::Full);
        }
        sqlx::query!(
            "INSERT INTO download_list (account_id, file_id, added_at) VALUES (?, ?, ?)",
            account,
            id,
            at
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(ListOutcome::Added)
    }

    async fn list_remove(&self, account: AccountId, id: FileId) -> Result<bool, StoreError> {
        let (account, id) = (account.as_raw(), id.as_raw());
        let done = sqlx::query!(
            "DELETE FROM download_list WHERE account_id = ? AND file_id = ?",
            account,
            id
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        Ok(done.rows_affected() > 0)
    }

    async fn list_clear(&self, account: AccountId) -> Result<(), StoreError> {
        let account = account.as_raw();
        sqlx::query!("DELETE FROM download_list WHERE account_id = ?", account)
            .execute(self.pool())
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn list_files(&self, account: AccountId) -> Result<Vec<FileRow>, StoreError> {
        let account = account.as_raw();
        let records = sqlx::query_as!(
            FileRecord,
            r#"SELECT f.id AS "id!", f.area_id AS "area_id!", a.name AS "area_name!", f.uploader_id,
                      u.username AS "uploader?", f.name, f.original_name, f.description, f.size,
                      f.sha256, f.blob_key, f.state, f.scan, f.scan_note, f.created_at,
                      f.decided_at, f.downloads
               FROM download_list d JOIN file f ON f.id = d.file_id
               JOIN file_area a ON a.id = f.area_id
               LEFT JOIN accounts u ON u.id = f.uploader_id
               WHERE d.account_id = ? AND f.state = 'approved'
               ORDER BY d.added_at ASC, f.id ASC"#,
            account
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        self.with_tags(records).await
    }

    async fn active_account_by_handle(
        &self,
        handle: &str,
    ) -> Result<Option<AccountId>, StoreError> {
        let key = handle.to_ascii_lowercase();
        Ok(sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM accounts WHERE lower(username) = ? AND state = 'active'"#,
            key,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        .map(AccountId::from_raw))
    }
}
