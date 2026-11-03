//! Mail over SQLite (M3-I, D78, D90): sealed rows and reports. The rules and the sealing are in
//! core's `mail`; this file is SQL and the transactions that must be atomic (the capped write
//! and the audited report). It never sees a key or a plaintext subject or body.

use russh_bbs_core::audit::events;
use russh_bbs_core::mail::{
    KeyId, MailRow, MailStore, MailWrite, MessageId, NewMail, NewReport, ReportId, ReportRow,
    ReportWrite,
};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

use super::SqliteStore;
use crate::error::backend;
use crate::forum_rows::{cap, int};
use crate::mail_rows::{RawMail, RawReport, count, key_id, mail, report};

fn to_mail(r: RawMail) -> Result<MailRow, StoreError> {
    mail(r)
}

impl MailStore for SqliteStore {
    async fn mail_recipient(&self, handle: &str) -> Result<Option<AccountId>, StoreError> {
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

    async fn mail_account_active(&self, account: AccountId) -> Result<bool, StoreError> {
        let id = account.as_raw();
        let n = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM accounts WHERE id = ? AND state = 'active'"#,
            id
        )
        .fetch_one(self.pool())
        .await
        .map_err(backend)?;
        Ok(n > 0)
    }

    async fn inbox(
        &self,
        recipient: AccountId,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<MailRow>, StoreError> {
        let to = recipient.as_raw();
        let (limit, offset) = (int(limit), int(offset));
        sqlx::query!(
            r#"SELECT m.id AS "id!", m.recipient_id AS "recipient_id!", m.sender_id,
                      a.username AS "sender?", m.sent_at AS "sent_at!", m.read_at,
                      m.key_id AS "key_id!", m.subject AS "subject!: Vec<u8>",
                      m.body AS "body!: Vec<u8>"
               FROM mail_message m LEFT JOIN accounts a ON a.id = m.sender_id
               WHERE m.recipient_id = ?
               ORDER BY m.sent_at DESC, m.id DESC LIMIT ? OFFSET ?"#,
            to,
            limit,
            offset,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(|r| {
            to_mail(RawMail {
                id: r.id,
                recipient_id: r.recipient_id,
                sender_id: r.sender_id,
                sender: r.sender,
                sent_at: r.sent_at,
                read_at: r.read_at,
                key_id: r.key_id,
                subject: r.subject,
                body: r.body,
            })
        })
        .collect()
    }

    async fn inbox_counts(&self, recipient: AccountId) -> Result<(u64, u64), StoreError> {
        let to = recipient.as_raw();
        let row = sqlx::query!(
            r#"SELECT count(*) AS "total!: i64",
                      coalesce(sum(read_at IS NULL), 0) AS "unread!: i64"
               FROM mail_message WHERE recipient_id = ?"#,
            to
        )
        .fetch_one(self.pool())
        .await
        .map_err(backend)?;
        Ok((
            count(row.total, "a mailbox count")?,
            count(row.unread, "an unread count")?,
        ))
    }

    async fn message(
        &self,
        recipient: AccountId,
        id: MessageId,
    ) -> Result<Option<MailRow>, StoreError> {
        let (to, id) = (recipient.as_raw(), id.as_raw());
        sqlx::query!(
            r#"SELECT m.id AS "id!", m.recipient_id AS "recipient_id!", m.sender_id,
                      a.username AS "sender?", m.sent_at AS "sent_at!", m.read_at,
                      m.key_id AS "key_id!", m.subject AS "subject!: Vec<u8>",
                      m.body AS "body!: Vec<u8>"
               FROM mail_message m LEFT JOIN accounts a ON a.id = m.sender_id
               WHERE m.id = ? AND m.recipient_id = ?"#,
            id,
            to,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?
        .map(|r| {
            to_mail(RawMail {
                id: r.id,
                recipient_id: r.recipient_id,
                sender_id: r.sender_id,
                sender: r.sender,
                sent_at: r.sent_at,
                read_at: r.read_at,
                key_id: r.key_id,
                subject: r.subject,
                body: r.body,
            })
        })
        .transpose()
    }

    async fn add_mail(&self, new: &NewMail, max_inbox: u64) -> Result<MailWrite, StoreError> {
        let id = new.id.as_raw();
        let to = new.recipient.as_raw();
        let from = new.sender.as_raw();
        let at = new.sent_at.as_raw();
        let key = i64::from(new.key_id.as_raw());
        let subject = new.subject.as_slice();
        let body = new.body.as_slice();
        let max = cap(Some(max_inbox));
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let active = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM accounts WHERE id = ? AND state = 'active'"#,
            to
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if active == 0 {
            return Ok(MailWrite::RecipientGone);
        }
        let written = sqlx::query_scalar!(
            r#"INSERT INTO mail_message
                   (id, recipient_id, sender_id, sent_at, read_at, key_id, subject, body)
               SELECT ?, ?, ?, ?, NULL, ?, ?, ?
               WHERE (SELECT count(*) FROM mail_message WHERE recipient_id = ?) < ?
                 AND NOT EXISTS (SELECT 1 FROM mail_message WHERE id = ?)
               RETURNING id AS "id!""#,
            id,
            to,
            from,
            at,
            key,
            subject,
            body,
            to,
            max,
            id,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        if written.is_some() {
            tx.commit().await.map_err(backend)?;
            return Ok(MailWrite::Written);
        }
        let taken = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM mail_message WHERE id = ?"#,
            id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        Ok(if taken > 0 {
            MailWrite::IdTaken
        } else {
            MailWrite::Full
        })
    }

    async fn mark_read(
        &self,
        recipient: AccountId,
        id: MessageId,
        now: UnixSeconds,
    ) -> Result<(), StoreError> {
        let (to, id, at) = (recipient.as_raw(), id.as_raw(), now.as_raw());
        sqlx::query!(
            "UPDATE mail_message SET read_at = ? WHERE id = ? AND recipient_id = ? AND read_at IS NULL",
            at,
            id,
            to,
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        Ok(())
    }

    async fn delete_mail(&self, recipient: AccountId, id: MessageId) -> Result<bool, StoreError> {
        let (to, id) = (recipient.as_raw(), id.as_raw());
        let done = sqlx::query!(
            "DELETE FROM mail_message WHERE id = ? AND recipient_id = ?",
            id,
            to
        )
        .execute(self.pool())
        .await
        .map_err(backend)?;
        Ok(done.rows_affected() > 0)
    }

    async fn key_ids_in_use(&self) -> Result<Vec<KeyId>, StoreError> {
        sqlx::query_scalar!(r#"SELECT DISTINCT key_id AS "key_id!" FROM mail_message ORDER BY 1"#)
            .fetch_all(self.pool())
            .await
            .map_err(backend)?
            .into_iter()
            .map(key_id)
            .collect()
    }

    async fn sample_mail(&self, limit: u32) -> Result<Vec<MailRow>, StoreError> {
        let limit = int(limit);
        sqlx::query!(
            r#"SELECT m.id AS "id!", m.recipient_id AS "recipient_id!", m.sender_id,
                      a.username AS "sender?", m.sent_at AS "sent_at!", m.read_at,
                      m.key_id AS "key_id!", m.subject AS "subject!: Vec<u8>",
                      m.body AS "body!: Vec<u8>"
               FROM mail_message m LEFT JOIN accounts a ON a.id = m.sender_id
               ORDER BY m.id LIMIT ?"#,
            limit,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(|r| {
            to_mail(RawMail {
                id: r.id,
                recipient_id: r.recipient_id,
                sender_id: r.sender_id,
                sender: r.sender,
                sent_at: r.sent_at,
                read_at: r.read_at,
                key_id: r.key_id,
                subject: r.subject,
                body: r.body,
            })
        })
        .collect()
    }

    async fn add_report(&self, new: &NewReport, max_open: u64) -> Result<ReportWrite, StoreError> {
        let reporter = new.reporter.as_raw();
        let sender = new.sender.map(AccountId::as_raw);
        let message = new.message.as_raw();
        let at = new.at.as_raw();
        let subject = new.copy_subject.as_str();
        let body = new.copy_body.as_str();
        let max = cap(Some(max_open));
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let already = sqlx::query_scalar!(
            r#"SELECT count(*) AS "n!: i64" FROM mail_report
               WHERE reporter_id = ? AND message_id = ?"#,
            reporter,
            message
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(backend)?;
        if already > 0 {
            return Ok(ReportWrite::Duplicate);
        }
        let id = sqlx::query_scalar!(
            r#"INSERT INTO mail_report
                   (reporter_id, sender_id, message_id, reported_at, copy_subject, copy_body)
               SELECT ?, ?, ?, ?, ?, ?
               WHERE (SELECT count(*) FROM mail_report
                      WHERE reporter_id = ? AND closed_at IS NULL) < ?
               RETURNING id AS "id!""#,
            reporter,
            sender,
            message,
            at,
            subject,
            body,
            reporter,
            max,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let Some(id) = id else {
            return Ok(ReportWrite::TooMany);
        };
        self.audit_in(
            &mut tx,
            &events::mail_reported(new.reporter, new.sender, id, message),
        )
        .await?;
        tx.commit().await.map_err(backend)?;
        Ok(ReportWrite::Written(ReportId::from_raw(id)))
    }

    async fn reports(&self, limit: u32, offset: u32) -> Result<Vec<ReportRow>, StoreError> {
        let (limit, offset) = (int(limit), int(offset));
        Ok(sqlx::query!(
            r#"SELECT r.id AS "id!", ra.username AS "reporter?", sa.username AS "sender?",
                      r.message_id AS "message_id!", r.reported_at AS "reported_at!",
                      r.copy_subject, r.copy_body, r.closed_at
               FROM mail_report r
               LEFT JOIN accounts ra ON ra.id = r.reporter_id
               LEFT JOIN accounts sa ON sa.id = r.sender_id
               ORDER BY (r.closed_at IS NOT NULL), r.reported_at DESC, r.id DESC
               LIMIT ? OFFSET ?"#,
            limit,
            offset,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?
        .into_iter()
        .map(|r| {
            report(RawReport {
                id: r.id,
                reporter: r.reporter,
                sender: r.sender,
                message_id: r.message_id,
                reported_at: r.reported_at,
                copy_subject: r.copy_subject,
                copy_body: r.copy_body,
                closed_at: r.closed_at,
            })
        })
        .collect())
    }

    async fn close_report(&self, id: ReportId, actor: AccountId) -> Result<bool, StoreError> {
        let (id, now_actor) = (id.as_raw(), actor);
        let mut tx = self
            .pool()
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(backend)?;
        let closed = sqlx::query!(
            "UPDATE mail_report SET closed_at = unixepoch() WHERE id = ? AND closed_at IS NULL",
            id
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?;
        if closed.rows_affected() == 0 {
            return Ok(false);
        }
        self.audit_in(&mut tx, &events::mail_report_closed(now_actor, id))
            .await?;
        tx.commit().await.map_err(backend)?;
        Ok(true)
    }
}
