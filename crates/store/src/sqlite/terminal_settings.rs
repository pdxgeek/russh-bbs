//! Per-account terminal settings and login times over SQLite (M1-E).
//!
//! Rows are plain columns mapped to core's [`TerminalSettings`] in one place. A stored value the
//! code could not have written is [`StoreError::Corrupt`], never a default.

use russh_bbs_core::audit::events;
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};
use russh_bbs_core::terminal_settings::{
    TerminalSettings, TerminalSettingsStore, tier_from_stored, tier_to_stored,
};
use sqlx::error::ErrorKind;

use super::SqliteStore;
use crate::error::{backend, kind};

/// A `terminal_settings` row as read by the checked query.
struct SettingsRow {
    width_override: Option<i64>,
    height_override: Option<i64>,
    color: i64,
    pause: i64,
    icon_tier: Option<String>,
}

impl SettingsRow {
    fn into_settings(self) -> Result<TerminalSettings, StoreError> {
        let corrupt = |what: &str| StoreError::Corrupt(format!("stored terminal setting {what}"));
        let flag = |value: i64, what: &str| match value {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(corrupt(what)),
        };
        let size = |value: Option<i64>, what: &str| {
            value
                .map(|n| u16::try_from(n).map_err(|_| corrupt(what)))
                .transpose()
        };
        let tier = self
            .icon_tier
            .map(|text| tier_from_stored(&text).ok_or_else(|| corrupt("icon_tier is not known")))
            .transpose()?;
        TerminalSettings::new(
            size(self.width_override, "width is not valid")?,
            size(self.height_override, "height is not valid")?,
            flag(self.color, "color is not valid")?,
            flag(self.pause, "pause is not valid")?,
            tier,
        )
        .map_err(|_| corrupt("is out of range"))
    }
}

impl TerminalSettingsStore for SqliteStore {
    async fn terminal_settings(&self, account: AccountId) -> Result<TerminalSettings, StoreError> {
        let id = account.as_raw();
        let row = sqlx::query_as!(
            SettingsRow,
            "SELECT width_override, height_override, color, pause, icon_tier
             FROM terminal_settings WHERE account_id = ?",
            id,
        )
        .fetch_optional(self.pool())
        .await
        .map_err(backend)?;
        row.map_or_else(
            || Ok(TerminalSettings::default()),
            SettingsRow::into_settings,
        )
    }

    async fn set_terminal_settings(
        &self,
        account: AccountId,
        settings: &TerminalSettings,
    ) -> Result<(), StoreError> {
        let id = account.as_raw();
        let width = settings.width().map(i64::from);
        let height = settings.height().map(i64::from);
        let color = i64::from(settings.color());
        let pause = i64::from(settings.pause());
        let tier = settings.chosen_tier().map(tier_to_stored);
        let mut tx = self.pool().begin().await.map_err(backend)?;
        sqlx::query!(
            "INSERT INTO terminal_settings
                 (account_id, width_override, height_override, color, pause, icon_tier)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (account_id) DO UPDATE SET
                 width_override  = excluded.width_override,
                 height_override = excluded.height_override,
                 color           = excluded.color,
                 pause           = excluded.pause,
                 icon_tier       = excluded.icon_tier,
                 updated_at      = unixepoch()",
            id,
            width,
            height,
            color,
            pause,
            tier,
        )
        .execute(&mut *tx)
        .await
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })?;
        self.audit_in(&mut tx, &events::terminal_settings_changed(account))
            .await?;
        tx.commit().await.map_err(backend)
    }

    async fn record_login(&self, account: AccountId) -> Result<Option<UnixSeconds>, StoreError> {
        let id = account.as_raw();
        // One statement, so two sessions signing in together each see a consistent previous
        // time: on conflict the old `last_login_at` becomes `previous_login_at`.
        sqlx::query_scalar!(
            "INSERT INTO account_logins (account_id, last_login_at)
             VALUES (?, unixepoch())
             ON CONFLICT (account_id) DO UPDATE SET
                 previous_login_at = account_logins.last_login_at,
                 last_login_at     = unixepoch()
             RETURNING previous_login_at",
            id,
        )
        .fetch_one(self.pool())
        .await
        .map(|previous| previous.map(UnixSeconds::from_raw))
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
            _ => backend(error),
        })
    }
}
