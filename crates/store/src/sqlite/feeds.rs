//! Feeds over SQLite (M3-G, D89): feed rows, the items seen, public boards. The rules are in
//! core's `feeds`; this file is SQL and the transactions that must be atomic (a feed with its
//! board, a fetch's items with the success record, an audited change).

use russh_bbs_core::audit::events;
use russh_bbs_core::feeds::{
    AddFeed, FeedId, FeedRow, FeedStore, Ingested, NewFeed, NewItem, Outcome,
};
use russh_bbs_core::forums::BoardId;
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

use super::SqliteStore;
use crate::error::backend;
use crate::feed_rows::{KEEP_SEEN, Raw, feed};
use crate::forum_rows::name_key;

impl FeedStore for SqliteStore {
    async fn feeds(&self) -> Result<Vec<FeedRow>, StoreError> {
        sqlx::query!(
            r#"SELECT f.id AS "id!", f.board_id AS "board_id!", b.name AS board_name, f.url,
                      f.allow_http AS "allow_http!: bool", f.interval_secs AS "interval_secs!",
                      f.last_attempt_at, f.last_success_at, f.last_outcome,
                      f.failures AS "failures!",
                      EXISTS (SELECT 1 FROM board_public p WHERE p.board_id = f.board_id)
                          AS "public!: bool"
               FROM feed f JOIN board b ON b.id = f.board_id ORDER BY f.id"#
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(|r| {
            feed(Raw {
                id: r.id,
                board_id: r.board_id,
                board_name: r.board_name,
                url: r.url,
                allow_http: r.allow_http,
                interval_secs: r.interval_secs,
                last_attempt_at: r.last_attempt_at,
                last_success_at: r.last_success_at,
                last_outcome: r.last_outcome,
                failures: r.failures,
                public: r.public,
            })
        })
        .collect()
    }

    async fn feed_of_board(&self, board: BoardId) -> Result<Option<FeedRow>, StoreError> {
        let board = board.as_raw();
        sqlx::query!(
            r#"SELECT f.id AS "id!", f.board_id AS "board_id!", b.name AS board_name, f.url,
                      f.allow_http AS "allow_http!: bool", f.interval_secs AS "interval_secs!",
                      f.last_attempt_at, f.last_success_at, f.last_outcome,
                      f.failures AS "failures!",
                      EXISTS (SELECT 1 FROM board_public p WHERE p.board_id = f.board_id)
                          AS "public!: bool"
               FROM feed f JOIN board b ON b.id = f.board_id WHERE f.board_id = ?"#,
            board
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        .map(|r| {
            feed(Raw {
                id: r.id,
                board_id: r.board_id,
                board_name: r.board_name,
                url: r.url,
                allow_http: r.allow_http,
                interval_secs: r.interval_secs,
                last_attempt_at: r.last_attempt_at,
                last_success_at: r.last_success_at,
                last_outcome: r.last_outcome,
                failures: r.failures,
                public: r.public,
            })
        })
        .transpose()
    }

    async fn add_feed(
        &self,
        new: &NewFeed,
        actor: AccountId,
        now: UnixSeconds,
        max_feeds: u32,
    ) -> Result<AddFeed, StoreError> {
        let url = new.url.as_str();
        let name = new.name.as_str();
        let key = name_key(name);
        let description = new.description.as_str();
        let allow_http = new.allow_http;
        let interval = i64::from(new.interval_secs);
        let by = actor.as_raw();
        let at = now.as_raw();
        let max = i64::from(max_feeds);
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let count = sqlx::query_scalar!(r#"SELECT count(*) AS "n!: i64" FROM feed"#)
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
        if count >= max {
            return Ok(AddFeed::TooMany);
        }
        let taken = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM feed WHERE url = ?"#,
            url
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if taken > 0 {
            return Ok(AddFeed::UrlTaken);
        }
        let board = sqlx::query_scalar!(
            r#"INSERT INTO board (name, name_key, description, restricted, feed, created_by, created_at)
               VALUES (?, ?, ?, 0, 1, ?, ?)
               ON CONFLICT (name_key) DO NOTHING
               RETURNING id AS "id!""#,
            name,
            key,
            description,
            by,
            at,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(board) = board else {
            return Ok(AddFeed::NameTaken);
        };
        let id = sqlx::query_scalar!(
            r#"INSERT INTO feed (board_id, url, allow_http, interval_secs, created_by, created_at)
               VALUES (?, ?, ?, ?, ?, ?) RETURNING id AS "id!""#,
            board,
            url,
            allow_http,
            interval,
            by,
            at,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        let board = BoardId::from_raw(board);
        if let Some(resource) = board.resource() {
            self.audit_in(&mut tx, &events::feed_added(actor, resource, allow_http))
                .await?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(AddFeed::Added(FeedId::from_raw(id), board))
    }

    async fn remove_feed(
        &self,
        feed_id: FeedId,
        actor: AccountId,
    ) -> Result<Option<BoardId>, StoreError> {
        let id = feed_id.as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let board = sqlx::query_scalar!(r#"SELECT board_id AS "b!" FROM feed WHERE id = ?"#, id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        let Some(board) = board else {
            return Ok(None);
        };
        // The board's threads, posts, the feed, its items and the public mark all cascade.
        sqlx::query!("DELETE FROM board WHERE id = ?", board)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        self.audit_in(&mut tx, &events::feed_removed(actor, board))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(Some(BoardId::from_raw(board)))
    }

    async fn ingest(
        &self,
        feed_id: FeedId,
        items: &[NewItem],
        now: UnixSeconds,
        keep_threads: u32,
    ) -> Result<Option<Ingested>, StoreError> {
        let id = feed_id.as_raw();
        let at = now.as_raw();
        let keep = i64::from(keep_threads);
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let board = sqlx::query_scalar!(r#"SELECT board_id AS "b!" FROM feed WHERE id = ?"#, id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        let Some(board) = board else {
            return Ok(None);
        };
        let mut done = Ingested::default();
        for item in items {
            if add_item(&mut tx, id, board, at, item).await? {
                done.added += 1;
            } else {
                done.skipped += 1;
            }
        }
        // The board keeps its newest threads; the removed threads' items stay "seen".
        sqlx::query!(
            "DELETE FROM thread WHERE board_id = ? AND id NOT IN
                 (SELECT id FROM thread WHERE board_id = ?
                  ORDER BY last_post_at DESC, id DESC LIMIT ?)",
            board,
            board,
            keep,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        sqlx::query!(
            "DELETE FROM feed_item WHERE feed_id = ? AND id NOT IN
                 (SELECT id FROM feed_item WHERE feed_id = ? ORDER BY id DESC LIMIT ?)",
            id,
            id,
            KEEP_SEEN,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        let ok = Outcome::Ok.name();
        sqlx::query!(
            "UPDATE feed SET last_attempt_at = ?, last_success_at = ?, last_outcome = ?,
                             failures = 0 WHERE id = ?",
            at,
            at,
            ok,
            id,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(Some(done))
    }

    async fn record_failure(
        &self,
        feed_id: FeedId,
        outcome: Outcome,
        now: UnixSeconds,
    ) -> Result<(), StoreError> {
        let id = feed_id.as_raw();
        let at = now.as_raw();
        let outcome = outcome.name();
        sqlx::query!(
            "UPDATE feed SET last_attempt_at = ?, last_outcome = ?, failures = min(failures + 1, 1000)
             WHERE id = ?",
            at,
            outcome,
            id,
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn set_public(
        &self,
        board: BoardId,
        public: bool,
        actor: AccountId,
        now: UnixSeconds,
    ) -> Result<bool, StoreError> {
        let id = board.as_raw();
        let by = actor.as_raw();
        let at = now.as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let restricted = sqlx::query_scalar!(
            r#"SELECT restricted AS "r!: bool" FROM board WHERE id = ?"#,
            id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(restricted) = restricted else {
            return Ok(false);
        };
        let changed = if public {
            if restricted {
                return Ok(false);
            }
            sqlx::query!(
                "INSERT INTO board_public (board_id, published_by, published_at)
                 VALUES (?, ?, ?) ON CONFLICT DO NOTHING",
                id,
                by,
                at,
            )
            .execute(&mut *tx)
            .await
            .map_err(backend)?
            .rows_affected()
        } else {
            sqlx::query!("DELETE FROM board_public WHERE board_id = ?", id)
                .execute(&mut *tx)
                .await
                .map_err(backend)?
                .rows_affected()
        };
        if changed > 0
            && let Some(resource) = board.resource()
        {
            self.audit_in(&mut tx, &events::board_published(actor, resource, public))
                .await?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(true)
    }

    async fn public_boards(&self) -> Result<Vec<BoardId>, StoreError> {
        Ok(
            sqlx::query_scalar!(r#"SELECT board_id AS "b!" FROM board_public ORDER BY board_id"#)
                .fetch_all(self.pool())
                .await
                .map_err(backend)?
                .into_iter()
                .map(BoardId::from_raw)
                .collect(),
        )
    }
}

/// Store one item as a thread with its opening post, unless its id or content was seen. `true`
/// when it became a thread.
async fn add_item(
    tx: &mut sqlx::SqliteConnection,
    id: i64,
    board: i64,
    at: i64,
    item: &NewItem,
) -> Result<bool, StoreError> {
    let key = item.key_hash.as_str();
    let content = item.content_hash.as_str();
    let seen = sqlx::query_scalar!(
        r#"INSERT INTO feed_item (feed_id, key_hash, content_hash, thread_id, seen_at)
               VALUES (?, ?, ?, NULL, ?) ON CONFLICT DO NOTHING RETURNING id AS "id!""#,
        id,
        key,
        content,
        at,
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(backend)?;
    let Some(seen) = seen else {
        return Ok(false);
    };
    let subject = item.subject.as_str();
    let body = item.body.as_str();
    let when = item.at.as_raw();
    let thread = sqlx::query_scalar!(
        r#"INSERT INTO thread (board_id, subject, author_id, created_at, last_post_at)
               VALUES (?, ?, NULL, ?, ?) RETURNING id AS "id!""#,
        board,
        subject,
        when,
        when,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(backend)?;
    sqlx::query!(
        "INSERT INTO post (thread_id, board_id, author_id, subject, body, created_at)
             VALUES (?, ?, NULL, ?, ?, ?)",
        thread,
        board,
        subject,
        body,
        when,
    )
    .execute(&mut *tx)
    .await
    .map_err(backend)?;
    sqlx::query!(
        "UPDATE feed_item SET thread_id = ? WHERE id = ?",
        thread,
        seen
    )
    .execute(&mut *tx)
    .await
    .map_err(backend)?;
    Ok(true)
}
