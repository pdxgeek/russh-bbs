//! `open` registration over SQLite (M2-G, D73, D74).
//!
//! **The cap is enforced where the account is made.** The transaction's first write is a capped
//! upsert on the day's counter (`ON CONFLICT .. DO UPDATE .. WHERE opened < cap`); it takes the
//! database write lock, so two sessions cannot both take the last place. No row affected means
//! the day is full. The account, its key and the audit row follow in the same transaction; a
//! taken handle or a bound key rolls the count back with the rest. The account that takes the
//! day's last place also writes the day's one `registration.capped` row (so the audit log says
//! the cap was reached even if nobody is turned away afterwards).

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::audit::events;
use russh_bbs_core::limits::Day;
use russh_bbs_core::registration::{DailyCap, OpenError, OpenRefusal, Opened, RegistrationStore};
use russh_bbs_core::sanitize::Username;
use russh_bbs_core::store::{
    AccountState, RegistrationMode, SettingKey, SettingsStore as _, StoreError, registration_mode,
};
use sqlx::error::ErrorKind;

use super::SqliteStore;
use crate::error::{backend, kind};
use crate::rows::AccountRow;

/// Counter rows older than this many days are deleted by the next account creation.
const KEEP_DAYS: i64 = 7;

impl SqliteStore {
    /// Record, once per UTC day, that the cap was reached: set the day's `capped_at` and write
    /// the `registration.capped` audit row in the same transaction. A day that is already
    /// flagged writes nothing, so strangers cannot grow the append-only log.
    async fn note_capped(
        &self,
        tx: &mut sqlx::SqliteConnection,
        day: i64,
        at: i64,
        cap: DailyCap,
    ) -> Result<(), StoreError> {
        let first = sqlx::query!(
            "UPDATE registration_day SET capped_at = ? WHERE day = ? AND capped_at IS NULL",
            at,
            day,
        )
        .execute(&mut *tx)
        .await
        .map_err(backend)?
        .rows_affected();
        if first == 1 {
            let shown = cap.limit().unwrap_or(0);
            self.audit_in(tx, &events::registration_capped(day, shown))
                .await?;
        }
        Ok(())
    }
}

impl RegistrationStore for SqliteStore {
    async fn registration_mode(&self) -> Result<RegistrationMode, StoreError> {
        registration_mode(self).await
    }

    async fn open_cap_setting(&self) -> Result<Option<String>, StoreError> {
        self.get_setting(SettingKey::OpenDailyCap).await
    }

    async fn open_cap_reached(&self, cap: DailyCap) -> Result<bool, StoreError> {
        let Some(limit) = cap.limit() else {
            return Ok(false);
        };
        let day = Day::of(self.clock().now()).as_raw();
        let opened = sqlx::query_scalar!(
            r#"SELECT opened AS "opened!: i64" FROM registration_day WHERE day = ?"#,
            day,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        Ok(opened.is_some_and(|n| n >= i64::from(limit)))
    }

    async fn open_account(
        &self,
        username: Username,
        fingerprint: &Fingerprint,
        cap: DailyCap,
    ) -> Result<Opened, OpenError> {
        let now = self.clock().now();
        let day = Day::of(now).as_raw();
        let oldest = day.saturating_sub(KEEP_DAYS);
        let limit = cap.limit().map(i64::from);
        let skeleton = username.skeleton();
        let name = username.as_str();
        let state = AccountState::Active;
        let state_text = state.as_str();
        let fingerprint = fingerprint.as_str();

        let mut tx = self.pool().begin().await.map_err(backend)?;
        sqlx::query!("DELETE FROM registration_day WHERE day < ?", oldest)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        let opened = sqlx::query_scalar!(
            r#"INSERT INTO registration_day (day, opened) VALUES (?1, 1)
               ON CONFLICT (day) DO UPDATE SET opened = registration_day.opened + 1
               WHERE ?2 IS NULL OR registration_day.opened < ?2
               RETURNING opened AS "opened!: i64""#,
            day,
            limit,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(backend)?;
        let at = now.as_raw();
        let Some(opened) = opened else {
            // The day is full (a cap lowered below today's count lands here too). The day's
            // one audit row is written now if the last place did not already write it; the
            // flag commits, nothing else was written.
            self.note_capped(&mut tx, day, at, cap).await?;
            tx.commit().await.map_err(backend)?;
            return Err(OpenError::CapReached);
        };
        let row = sqlx::query_as!(
            AccountRow,
            r#"INSERT INTO accounts (username, username_skeleton, state)
               VALUES (?, ?, ?)
               RETURNING id AS "id!", username, state, created_at"#,
            name,
            skeleton,
            state_text,
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => OpenError::Rejected(OpenRefusal::Handle),
            _ => OpenError::Store(backend(error)),
        })?;
        sqlx::query!(
            "INSERT INTO account_keys (fingerprint, account_id) VALUES (?, ?)",
            fingerprint,
            row.id,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => OpenError::Rejected(OpenRefusal::Key),
            _ => OpenError::Store(backend(error)),
        })?;
        let account = row.into_account()?;
        self.audit_in(
            &mut tx,
            &events::account_created_via_open(account.id, account.state),
        )
        .await?;
        if limit.is_some_and(|limit| opened >= limit) {
            // This account takes the day's last place: the cap is reached now, whether or not
            // anyone is turned away afterwards. Written after the `account.created` row it
            // belongs to, so the log reads in the order things happened (QA M2 L2).
            self.note_capped(&mut tx, day, at, cap).await?;
        }
        tx.commit().await.map_err(backend)?;
        Ok(Opened { account })
    }
}
