//! Per-account terminal settings and login times over Postgres (M1-E).
//!
//! Rows are plain columns mapped to core's [`TerminalSettings`] in one place. A stored value the
//! code could not have written is [`StoreError::Corrupt`], never a default.

use russh_bbs_core::audit::events;
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};
use russh_bbs_core::terminal_settings::{
    TerminalSettings, TerminalSettingsStore, ThemeName, layout_from_stored, layout_to_stored,
    tier_from_stored, tier_to_stored,
};
use sqlx::error::ErrorKind;

use super::PostgresStore;
use crate::error::{backend, kind};

/// A `terminal_settings` row as read by the checked query.
struct SettingsRow {
    width_override: Option<i32>,
    height_override: Option<i32>,
    color: bool,
    pause: bool,
    icon_tier: Option<String>,
    layout: String,
    theme: String,
}

impl SettingsRow {
    fn into_settings(self) -> Result<TerminalSettings, StoreError> {
        let corrupt = |what: &str| StoreError::Corrupt(format!("stored terminal setting {what}"));
        let size = |value: Option<i32>, what: &str| {
            value
                .map(|n| u16::try_from(n).map_err(|_| corrupt(what)))
                .transpose()
        };
        let tier = self
            .icon_tier
            .map(|text| tier_from_stored(&text).ok_or_else(|| corrupt("icon_tier is not known")))
            .transpose()?;
        let layout = layout_from_stored(&self.layout).ok_or_else(|| corrupt("layout is not known"))?;
        let theme = ThemeName::new(&self.theme).map_err(|_| corrupt("theme is not valid"))?;
        let settings = TerminalSettings::new(
            size(self.width_override, "width is not valid")?,
            size(self.height_override, "height is not valid")?,
            self.color,
            self.pause,
            tier,
        )
        .map_err(|_| corrupt("is out of range"))?;
        Ok(settings.with_layout(layout).with_theme(theme))
    }
}

impl TerminalSettingsStore for PostgresStore {
    async fn terminal_settings(&self, account: AccountId) -> Result<TerminalSettings, StoreError> {
        let id = account.as_raw();
        let row = sqlx::query_as!(
            SettingsRow,
            "SELECT width_override, height_override, color, pause, icon_tier, layout, theme
             FROM terminal_settings WHERE account_id = $1",
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
        let width = settings.width().map(i32::from);
        let height = settings.height().map(i32::from);
        let color = settings.color();
        let pause = settings.pause();
        let tier = settings.chosen_tier().map(tier_to_stored);
        let layout = layout_to_stored(settings.layout());
        let theme = settings.theme();
        let theme = theme.as_str();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        sqlx::query!(
            "INSERT INTO terminal_settings
                 (account_id, width_override, height_override, color, pause, icon_tier, layout, theme)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (account_id) DO UPDATE SET
                 width_override  = excluded.width_override,
                 height_override = excluded.height_override,
                 color           = excluded.color,
                 pause           = excluded.pause,
                 icon_tier       = excluded.icon_tier,
                 layout          = excluded.layout,
                 theme           = excluded.theme,
                 updated_at      = extract(epoch FROM now())::bigint",
            id,
            width,
            height,
            color,
            pause,
            tier,
            layout,
            theme,
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
             VALUES ($1, extract(epoch FROM now())::bigint)
             ON CONFLICT (account_id) DO UPDATE SET
                 previous_login_at = account_logins.last_login_at,
                 last_login_at     = extract(epoch FROM now())::bigint
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
