//! Forums over SQLite (M3-E, D83): boards, threads, posts and FTS5 search. The rules are in
//! core's `forums`; this file is SQL and the transactions that must be atomic (the capped post
//! write, the audited board and removal).

use russh_bbs_core::audit::events;
use russh_bbs_core::forums::{
    BoardId, BoardRow, ForumStore, NewBoard, NewPost, NewThread, PostCap, PostId, PostRow,
    PostWrite, RawHit, Removed, ThreadId, ThreadRow,
};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

use super::SqliteStore;
use crate::error::backend;
use crate::forum_rows::{board, cap, fts5_match, hit, int, json_ids, name_key, post, thread};

impl ForumStore for SqliteStore {
    async fn boards(&self) -> Result<Vec<BoardRow>, StoreError> {
        sqlx::query!(
            r#"SELECT b.id AS "id!", b.name, b.description, b.restricted AS "restricted!: bool", b.feed AS "feed!: bool",
                      (SELECT count(*) FROM thread t WHERE t.board_id = b.id) AS "threads!: i64"
               FROM board b ORDER BY b.id"#
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(|r| board(r.id, r.name, r.description, r.restricted, r.feed, r.threads))
        .collect()
    }

    async fn board(&self, id: BoardId) -> Result<Option<BoardRow>, StoreError> {
        let id = id.as_raw();
        sqlx::query!(
            r#"SELECT b.id AS "id!", b.name, b.description, b.restricted AS "restricted!: bool", b.feed AS "feed!: bool",
                      (SELECT count(*) FROM thread t WHERE t.board_id = b.id) AS "threads!: i64"
               FROM board b WHERE b.id = ?"#,
            id
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        .map(|r| board(r.id, r.name, r.description, r.restricted, r.feed, r.threads))
        .transpose()
    }

    async fn create_board(
        &self,
        new: &NewBoard,
        actor: AccountId,
        now: UnixSeconds,
    ) -> Result<Option<BoardId>, StoreError> {
        let name = new.name.as_str();
        let key = name_key(name);
        let description = new.description.as_str();
        let restricted = new.restricted;
        let by = actor.as_raw();
        let at = now.as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let id = sqlx::query_scalar!(
            r#"INSERT INTO board (name, name_key, description, restricted, created_by, created_at)
               VALUES (?, ?, ?, ?, ?, ?)
               ON CONFLICT (name_key) DO NOTHING
               RETURNING id AS "id!""#,
            name,
            key,
            description,
            restricted,
            by,
            at,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(id) = id else {
            return Ok(None);
        };
        let id = BoardId::from_raw(id);
        if let Some(resource) = id.resource() {
            self.audit_in(&mut tx, &events::board_created(actor, resource, restricted))
                .await?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(Some(id))
    }

    async fn threads(
        &self,
        board_id: BoardId,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<ThreadRow>, StoreError> {
        let id = board_id.as_raw();
        let (limit, offset) = (int(limit), int(offset));
        sqlx::query!(
            r#"SELECT t.id AS "id!", t.board_id AS "board_id!", b.name AS board_name, t.subject,
                      a.username AS "author?",
                      (SELECT count(*) FROM post p WHERE p.thread_id = t.id) AS "posts!: i64",
                      t.last_post_at AS "last_post_at!"
               FROM thread t
               JOIN board b ON b.id = t.board_id
               LEFT JOIN accounts a ON a.id = t.author_id
               WHERE t.board_id = ?
               ORDER BY t.last_post_at DESC, t.id DESC LIMIT ? OFFSET ?"#,
            id,
            limit,
            offset,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(|r| {
            thread(
                r.id,
                r.board_id,
                r.board_name,
                r.subject,
                r.author,
                r.posts,
                r.last_post_at,
            )
        })
        .collect()
    }

    async fn thread(&self, id: ThreadId) -> Result<Option<ThreadRow>, StoreError> {
        let id = id.as_raw();
        sqlx::query!(
            r#"SELECT t.id AS "id!", t.board_id AS "board_id!", b.name AS board_name, t.subject,
                      a.username AS "author?",
                      (SELECT count(*) FROM post p WHERE p.thread_id = t.id) AS "posts!: i64",
                      t.last_post_at AS "last_post_at!"
               FROM thread t
               JOIN board b ON b.id = t.board_id
               LEFT JOIN accounts a ON a.id = t.author_id
               WHERE t.id = ?"#,
            id,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        .map(|r| {
            thread(
                r.id,
                r.board_id,
                r.board_name,
                r.subject,
                r.author,
                r.posts,
                r.last_post_at,
            )
        })
        .transpose()
    }

    async fn post_at(
        &self,
        thread_id: ThreadId,
        index: u64,
    ) -> Result<Option<PostRow>, StoreError> {
        let id = thread_id.as_raw();
        let index = i64::try_from(index).unwrap_or(i64::MAX);
        Ok(sqlx::query!(
            r#"SELECT p.id AS "id!", p.thread_id AS "thread_id!", p.board_id AS "board_id!",
                      p.author_id, a.username AS "author?", p.body, p.created_at AS "created_at!"
               FROM post p LEFT JOIN accounts a ON a.id = p.author_id
               WHERE p.thread_id = ? ORDER BY p.id LIMIT 1 OFFSET ?"#,
            id,
            index,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        .map(|r| {
            post(
                r.id,
                r.thread_id,
                r.board_id,
                r.author_id,
                r.author,
                r.body,
                r.created_at,
            )
        }))
    }

    async fn post(&self, id: PostId) -> Result<Option<PostRow>, StoreError> {
        let id = id.as_raw();
        Ok(sqlx::query!(
            r#"SELECT p.id AS "id!", p.thread_id AS "thread_id!", p.board_id AS "board_id!",
                      p.author_id, a.username AS "author?", p.body, p.created_at AS "created_at!"
               FROM post p LEFT JOIN accounts a ON a.id = p.author_id
               WHERE p.id = ?"#,
            id,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        .map(|r| {
            post(
                r.id,
                r.thread_id,
                r.board_id,
                r.author_id,
                r.author,
                r.body,
                r.created_at,
            )
        }))
    }

    async fn add_thread(
        &self,
        new: &NewThread,
        limit: PostCap,
    ) -> Result<PostWrite<(ThreadId, PostId)>, StoreError> {
        let board_id = new.board.as_raw();
        let subject = new.subject.as_str();
        let body = new.body.as_str();
        let author = new.author.as_raw();
        let at = new.now.as_raw();
        let since = limit.since.as_raw();
        let max = cap(limit.max);
        // BEGIN IMMEDIATE takes the write lock first: the count inside the insert cannot go
        // stale, and a second post waits for this transaction instead of failing as busy.
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let thread_id = sqlx::query_scalar!(
            r#"INSERT INTO thread (board_id, subject, author_id, created_at, last_post_at)
               SELECT ?, ?, ?, ?, ?
               WHERE EXISTS (SELECT 1 FROM board WHERE id = ?)
                 AND (? IS NULL
                      OR (SELECT count(*) FROM post WHERE author_id = ? AND created_at >= ?) < ?)
               RETURNING id AS "id!""#,
            board_id,
            subject,
            author,
            at,
            at,
            board_id,
            max,
            author,
            since,
            max,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(thread_id) = thread_id else {
            let exists = sqlx::query_scalar!(
                r#"SELECT count(*) AS "n!: i64" FROM board WHERE id = ?"#,
                board_id
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
            return Ok(if exists == 0 {
                PostWrite::NoParent
            } else {
                PostWrite::Full
            });
        };
        let post_id = sqlx::query_scalar!(
            r#"INSERT INTO post (thread_id, board_id, author_id, subject, body, created_at)
               VALUES (?, ?, ?, ?, ?, ?) RETURNING id AS "id!""#,
            thread_id,
            board_id,
            author,
            subject,
            body,
            at,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(PostWrite::Written((
            ThreadId::from_raw(thread_id),
            PostId::from_raw(post_id),
        )))
    }

    async fn add_reply(
        &self,
        new: &NewPost,
        limit: PostCap,
    ) -> Result<PostWrite<PostId>, StoreError> {
        let thread_id = new.thread.as_raw();
        let body = new.body.as_str();
        let author = new.author.as_raw();
        let at = new.now.as_raw();
        let since = limit.since.as_raw();
        let max = cap(limit.max);
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let post_id = sqlx::query_scalar!(
            r#"INSERT INTO post (thread_id, board_id, author_id, subject, body, created_at)
               SELECT t.id, t.board_id, ?, '', ?, ? FROM thread t
               WHERE t.id = ?
                 AND (? IS NULL
                      OR (SELECT count(*) FROM post WHERE author_id = ? AND created_at >= ?) < ?)
               RETURNING id AS "id!""#,
            author,
            body,
            at,
            thread_id,
            max,
            author,
            since,
            max,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(post_id) = post_id else {
            let exists = sqlx::query_scalar!(
                r#"SELECT count(*) AS "n!: i64" FROM thread WHERE id = ?"#,
                thread_id
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
            return Ok(if exists == 0 {
                PostWrite::NoParent
            } else {
                PostWrite::Full
            });
        };
        sqlx::query!(
            "UPDATE thread SET last_post_at = max(last_post_at, ?) WHERE id = ?",
            at,
            thread_id,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(PostWrite::Written(PostId::from_raw(post_id)))
    }

    async fn remove_post(
        &self,
        id: PostId,
        actor: AccountId,
    ) -> Result<Option<Removed>, StoreError> {
        let raw = id.as_raw();
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        // Deleting first takes the write lock; the row is read back from what it removed.
        let gone = sqlx::query!(
            r#"DELETE FROM post WHERE id = ?
               RETURNING thread_id AS "thread_id!", board_id AS "board_id!", author_id"#,
            raw,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(gone) = gone else {
            return Ok(None);
        };
        // The opening post is the thread's lowest id; with it gone the thread has no subject,
        // so the rest of the thread goes too (deleted explicitly so the index follows).
        let opening = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM thread t
               WHERE t.id = ? AND NOT EXISTS (SELECT 1 FROM post p WHERE p.thread_id = t.id AND p.id < ?)"#,
            gone.thread_id,
            raw,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?
            > 0;
        let thread_removed = if opening {
            sqlx::query!("DELETE FROM post WHERE thread_id = ?", gone.thread_id)
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
            sqlx::query!("DELETE FROM thread WHERE id = ?", gone.thread_id)
                .execute(&mut *tx)
                .await
                .map_err(backend)?;
            true
        } else {
            sqlx::query!(
                "UPDATE thread SET last_post_at = COALESCE(
                     (SELECT max(created_at) FROM post WHERE thread_id = thread.id), created_at)
                 WHERE id = ?",
                gone.thread_id,
            )
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
            false
        };
        let removed = Removed {
            board: BoardId::from_raw(gone.board_id),
            thread: ThreadId::from_raw(gone.thread_id),
            author: gone.author_id.map(AccountId::from_raw),
            thread_removed,
        };
        if let Some(resource) = removed.board.resource() {
            self.audit_in(
                &mut tx,
                &events::post_removed(actor, removed.author, resource, raw, thread_removed),
            )
            .await?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(Some(removed))
    }

    async fn search_posts(
        &self,
        boards: &[BoardId],
        terms: &[String],
        limit: u32,
        offset: u32,
    ) -> Result<Vec<RawHit>, StoreError> {
        let matching = fts5_match(terms);
        if matching.is_empty() || boards.is_empty() {
            return Ok(Vec::new());
        }
        let ids = json_ids(boards);
        let (limit, offset) = (int(limit), int(offset));
        sqlx::query!(
            r#"SELECT p.id AS "id!", p.thread_id AS "thread_id!", p.board_id AS "board_id!",
                      b.name AS board_name, t.subject AS subject, a.username AS "author?",
                      p.body AS body, p.created_at AS "created_at!",
                      (SELECT count(*) FROM post q
                       WHERE q.thread_id = p.thread_id AND q.id < p.id) AS "pos!: i64"
               FROM post_fts
               JOIN post p ON p.id = post_fts.rowid
               JOIN thread t ON t.id = p.thread_id
               JOIN board b ON b.id = p.board_id
               LEFT JOIN accounts a ON a.id = p.author_id
               WHERE post_fts MATCH ?
                 AND p.board_id IN (SELECT CAST(value AS INTEGER) FROM json_each(?))
               ORDER BY post_fts.rank, p.id DESC
               LIMIT ? OFFSET ?"#,
            matching,
            ids,
            limit,
            offset,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(|r| {
            hit(
                r.id,
                r.thread_id,
                r.board_id,
                r.board_name,
                r.subject,
                r.author,
                r.body,
                r.pos,
                r.created_at,
            )
        })
        .collect()
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
