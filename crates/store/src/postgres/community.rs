//! The community features over Postgres (M3-J): one-liners and wall lines, last callers and
//! polls. The rules are in core's `community`; this file is SQL, and each write that must be
//! atomic is one statement or one transaction. Same-member adds queue on the member's account
//! row, and poll creation takes a table lock that still lets readers in, so the counts a
//! limit is checked against cannot go stale before the insert.

use russh_bbs_core::audit::events;
use russh_bbs_core::community::{
    AddLine, CloseOutcome, CommunityStore, CreatePoll, EntryId, LineKind, LineText, NewPoll,
    PollId, PollRules, RemoveOutcome, StoredCaller, StoredLine, StoredPoll, StoredPollRow,
    VoteOutcome,
};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};
use sqlx::error::ErrorKind;

use super::PostgresStore;
use crate::community_rows::{
    DAY, HOUR, PollParts, caller, kind_name, line, num, poll, poll_row, tally,
};
use crate::error::{backend, kind};

impl CommunityStore for PostgresStore {
    async fn add_line(
        &self,
        account: AccountId,
        line_kind: LineKind,
        text: &LineText,
        now: UnixSeconds,
    ) -> Result<AddLine, StoreError> {
        let id = account.as_raw();
        let kind_text = kind_name(line_kind);
        let body = text.as_str();
        let at = now.as_raw();
        let since = at.saturating_sub(HOUR);
        let limit = num(line_kind.per_hour());
        let keep = num(line_kind.keep());
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let locked = sqlx::query_scalar!(
            "SELECT id FROM accounts WHERE id = $1 FOR NO KEY UPDATE",
            id,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if locked.is_none() {
            return Err(StoreError::AccountNotFound);
        }
        let added = sqlx::query_scalar!(
            r#"INSERT INTO community_line (kind, account_id, body, created_at)
               SELECT $1::text, $2::bigint, $3::text, $4::bigint
               WHERE (SELECT count(*) FROM community_line
                      WHERE account_id = $2 AND kind = $1 AND created_at > $5) < $6
               RETURNING id AS "id!""#,
            kind_text,
            id,
            body,
            at,
            since,
            limit,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(added) = added else {
            let oldest = sqlx::query_scalar!(
                "SELECT min(created_at) FROM community_line
                 WHERE account_id = $1 AND kind = $2 AND created_at > $3",
                id,
                kind_text,
                since,
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?
            .unwrap_or(at);
            tx.commit().await.map_err(backend)?;
            return Ok(AddLine::RateLimited {
                retry_at: UnixSeconds::from_raw(oldest.saturating_add(HOUR)),
            });
        };
        sqlx::query!(
            "DELETE FROM community_line WHERE kind = $1
             AND id NOT IN (SELECT id FROM community_line WHERE kind = $1 ORDER BY id DESC LIMIT $2)",
            kind_text,
            keep,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(AddLine::Added(EntryId::from_raw(added)))
    }

    async fn recent_lines(
        &self,
        line_kind: LineKind,
        limit: usize,
    ) -> Result<Vec<StoredLine>, StoreError> {
        let kind_text = kind_name(line_kind);
        let limit = num(limit);
        let rows = sqlx::query!(
            r#"SELECT l.id AS "id!", a.username, l.body, l.created_at
               FROM community_line l JOIN accounts a ON a.id = l.account_id
               WHERE l.kind = $1 ORDER BY l.id DESC LIMIT $2"#,
            kind_text,
            limit,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(|r| line(r.id, r.username, r.body, r.created_at))
            .collect())
    }

    async fn remove_line(
        &self,
        actor: AccountId,
        line_kind: LineKind,
        id: EntryId,
    ) -> Result<RemoveOutcome, StoreError> {
        let kind_text = kind_name(line_kind);
        let row = id.as_raw();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let author = sqlx::query_scalar!(
            "DELETE FROM community_line WHERE id = $1 AND kind = $2 RETURNING account_id",
            row,
            kind_text,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(author) = author else {
            return Ok(RemoveOutcome::NotFound);
        };
        let event =
            events::community_entry_removed(actor, AccountId::from_raw(author), kind_text, row);
        self.audit_in(&mut tx, &event).await?;
        tx.commit().await.map_err(backend)?;
        Ok(RemoveOutcome::Removed)
    }

    async fn record_sign_in(&self, account: AccountId, now: UnixSeconds) -> Result<(), StoreError> {
        let id = account.as_raw();
        let at = now.as_raw();
        sqlx::query!(
            "INSERT INTO last_caller (account_id, last_at) VALUES ($1, $2)
             ON CONFLICT (account_id) DO UPDATE SET last_at = excluded.last_at",
            id,
            at,
        )
        .execute(self.pool())
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })?;
        Ok(())
    }

    async fn last_callers(&self, limit: usize) -> Result<Vec<StoredCaller>, StoreError> {
        let limit = num(limit);
        let rows = sqlx::query!(
            "SELECT a.username, c.last_at FROM last_caller c JOIN accounts a ON a.id = c.account_id
             WHERE a.state = 'active' ORDER BY c.last_at DESC, a.id LIMIT $1",
            limit,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        Ok(rows
            .into_iter()
            .map(|r| caller(r.username, r.last_at))
            .collect())
    }

    async fn create_poll(
        &self,
        creator: AccountId,
        new: &NewPoll,
        rules: PollRules,
        now: UnixSeconds,
    ) -> Result<CreatePoll, StoreError> {
        let who = creator.as_raw();
        let at = now.as_raw();
        let since = at.saturating_sub(DAY);
        let per_day = num(rules.per_day);
        let max = num(rules.max_polls);
        let question = new.question();
        let description = new.description().map_or("", |d| d.trimmed());
        let results = new.visibility().name();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        // Readers keep reading; two creators queue here, so the counts below stay true.
        sqlx::query("LOCK TABLE poll IN SHARE ROW EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        let created = sqlx::query_scalar!(
            r#"INSERT INTO poll (creator_id, question, description, results, created_at)
               SELECT $1::bigint, $2::text, $3::text, $4::text, $5::bigint
               WHERE (SELECT count(*) FROM poll WHERE creator_id = $1 AND created_at > $6) < $7
                 AND (SELECT count(*) FROM poll) < $8
               RETURNING id AS "id!""#,
            who,
            question,
            description,
            results,
            at,
            since,
            per_day,
            max,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })?;
        let Some(poll_id) = created else {
            let row = sqlx::query!(
                r#"SELECT count(*) AS "n!", min(created_at) AS oldest
                   FROM poll WHERE creator_id = $1 AND created_at > $2"#,
                who,
                since,
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
            tx.commit().await.map_err(backend)?;
            return Ok(if row.n >= per_day {
                CreatePoll::RateLimited {
                    retry_at: UnixSeconds::from_raw(row.oldest.unwrap_or(at).saturating_add(DAY)),
                }
            } else {
                CreatePoll::BoardFull
            });
        };
        for (position, text) in new.options().iter().enumerate() {
            let position = num(position);
            sqlx::query!(
                "INSERT INTO poll_option (poll_id, position, text) VALUES ($1, $2, $3)",
                poll_id,
                position,
                text,
            )
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(CreatePoll::Created(PollId::from_raw(poll_id)))
    }

    async fn list_polls(
        &self,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<StoredPollRow>, usize), StoreError> {
        let (offset, limit) = (num(offset), num(limit));
        let rows = sqlx::query!(
            r#"SELECT p.id AS "id!", p.question, p.closed_at IS NOT NULL AS "closed!",
                      (SELECT count(*) FROM poll_vote v WHERE v.poll_id = p.id) AS "votes!"
               FROM poll p ORDER BY p.id DESC LIMIT $1 OFFSET $2"#,
            limit,
            offset,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        let total = sqlx::query_scalar!(r#"SELECT count(*) AS "n!" FROM poll"#)
            .fetch_one(self.pool())
            .await
            .map_err(backend)?;
        let rows = rows
            .into_iter()
            .map(|r| poll_row(r.id, r.question, r.closed, r.votes))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((rows, usize::try_from(total).unwrap_or(0)))
    }

    async fn poll(&self, id: PollId, viewer: AccountId) -> Result<Option<StoredPoll>, StoreError> {
        let (row, who) = (id.as_raw(), viewer.as_raw());
        let Some(p) = sqlx::query!(
            r#"SELECT p.id AS "id!", p.question, p.description, p.creator_id, a.username,
                      p.results, p.closed_at, p.created_at
               FROM poll p JOIN accounts a ON a.id = p.creator_id WHERE p.id = $1"#,
            row,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        else {
            return Ok(None);
        };
        let options = sqlx::query_scalar!(
            "SELECT text FROM poll_option WHERE poll_id = $1 ORDER BY position",
            row,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        let my_vote = sqlx::query_scalar!(
            "SELECT position FROM poll_vote WHERE poll_id = $1 AND account_id = $2",
            row,
            who,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        poll(PollParts {
            id: p.id,
            question: p.question,
            description: p.description,
            creator: p.creator_id,
            creator_handle: p.username,
            visibility: p.results,
            closed_at: p.closed_at,
            created_at: p.created_at,
            options,
            my_vote,
        })
        .map(Some)
    }

    async fn tally(&self, id: PollId) -> Result<Vec<u64>, StoreError> {
        let row = id.as_raw();
        let rows = sqlx::query!(
            r#"SELECT position, count(*) AS "n!" FROM poll_vote WHERE poll_id = $1 GROUP BY position"#,
            row,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        tally(rows.into_iter().map(|r| (r.position, r.n)))
    }

    async fn vote(
        &self,
        id: PollId,
        account: AccountId,
        option: usize,
        now: UnixSeconds,
    ) -> Result<VoteOutcome, StoreError> {
        let (row, who, position, at) = (id.as_raw(), account.as_raw(), num(option), now.as_raw());
        let mut tx = self.pool().begin().await.map_err(backend)?;
        // The primary key (poll, account) is the one-vote rule; DO NOTHING turns a second vote
        // into zero rows rather than an error, and the statement also checks the poll is open
        // and has the option.
        let voted = sqlx::query!(
            "INSERT INTO poll_vote (poll_id, account_id, position, voted_at)
             SELECT p.id, $2::bigint, $3::bigint, $4::bigint FROM poll p
             JOIN poll_option o ON o.poll_id = p.id AND o.position = $3
             WHERE p.id = $1 AND p.closed_at IS NULL
             ON CONFLICT (poll_id, account_id) DO NOTHING",
            row,
            who,
            position,
            at,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })?
        .rows_affected();
        if voted > 0 {
            tx.commit().await.map_err(backend)?;
            return Ok(VoteOutcome::Voted);
        }
        let state = sqlx::query!(
            r#"SELECT p.closed_at IS NOT NULL AS "closed!",
                      EXISTS (SELECT 1 FROM poll_option o WHERE o.poll_id = p.id AND o.position = $2) AS "option!",
                      EXISTS (SELECT 1 FROM poll_vote v WHERE v.poll_id = p.id AND v.account_id = $3) AS "voted!"
               FROM poll p WHERE p.id = $1"#,
            row,
            position,
            who,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(match state {
            None => VoteOutcome::NotFound,
            Some(s) if !s.option => VoteOutcome::NotFound,
            Some(s) if s.voted => VoteOutcome::AlreadyVoted,
            Some(_) => VoteOutcome::Closed,
        })
    }

    async fn close_poll(
        &self,
        actor: AccountId,
        id: PollId,
        staff: bool,
        now: UnixSeconds,
    ) -> Result<CloseOutcome, StoreError> {
        let (row, who, at) = (id.as_raw(), actor.as_raw(), now.as_raw());
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let closed = sqlx::query_scalar!(
            "UPDATE poll SET closed_at = $1 WHERE id = $2 AND closed_at IS NULL
             AND (creator_id = $3 OR $4::bool) RETURNING creator_id",
            at,
            row,
            who,
            staff,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if let Some(creator) = closed {
            if creator != who {
                let event = events::community_poll_closed(actor, AccountId::from_raw(creator), row);
                self.audit_in(&mut tx, &event).await?;
            }
            tx.commit().await.map_err(backend)?;
            return Ok(CloseOutcome::Done);
        }
        let state = sqlx::query_scalar!("SELECT closed_at FROM poll WHERE id = $1", row)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend)?;
        tx.commit().await.map_err(backend)?;
        Ok(match state {
            None => CloseOutcome::NotFound,
            Some(Some(_)) => CloseOutcome::AlreadyClosed,
            Some(None) => CloseOutcome::NotAllowed,
        })
    }

    async fn remove_poll(&self, actor: AccountId, id: PollId) -> Result<CloseOutcome, StoreError> {
        let row = id.as_raw();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        let creator =
            sqlx::query_scalar!("DELETE FROM poll WHERE id = $1 RETURNING creator_id", row)
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?;
        let Some(creator) = creator else {
            return Ok(CloseOutcome::NotFound);
        };
        let event = events::community_poll_removed(actor, AccountId::from_raw(creator), row);
        self.audit_in(&mut tx, &event).await?;
        tx.commit().await.map_err(backend)?;
        Ok(CloseOutcome::Done)
    }
}
