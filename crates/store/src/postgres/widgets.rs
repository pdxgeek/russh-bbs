//! A member's widget lists over postgres (UI-10). One row per layout the member changed; the text
//! mapping is shared (`widget_rows`), and an id this build does not know is skipped.

use russh_bbs_core::audit::events;
use russh_bbs_core::store::{AccountId, StoreError};
use russh_bbs_core::terminal_settings::{LayoutId, layout_to_stored};
use russh_bbs_core::widgets::{Choice, WidgetId, WidgetStore};
use sqlx::error::ErrorKind;

use super::PostgresStore;
use crate::error::{backend, kind};
use crate::widget_rows;

impl WidgetStore for PostgresStore {
    async fn widget_choice(&self, account: AccountId) -> Result<Choice, StoreError> {
        let id = account.as_raw();
        let rows = sqlx::query!(
            "SELECT layout, widgets FROM widget_list WHERE account_id = $1",
            id,
        )
        .fetch_all(self.pool())
        .await
        .map_err(backend)?;
        Ok(widget_rows::choice(
            rows.into_iter().map(|row| (row.layout, row.widgets)),
        ))
    }

    async fn set_widget_list(
        &self,
        account: AccountId,
        layout: LayoutId,
        list: Option<&[WidgetId]>,
    ) -> Result<(), StoreError> {
        let id = account.as_raw();
        let layout = layout_to_stored(layout);
        let mut tx = self.pool().begin().await.map_err(backend)?;
        if let Some(list) = list {
            let widgets = widget_rows::encode(list);
            sqlx::query!(
                "INSERT INTO widget_list (account_id, layout, widgets) VALUES ($1, $2, $3)
                 ON CONFLICT (account_id, layout) DO UPDATE SET
                     widgets    = excluded.widgets,
                     updated_at = extract(epoch FROM now())::bigint",
                id,
                layout,
                widgets,
            )
            .execute(&mut *tx)
            .await
            .map_err(|error| match kind(&error) {
                Some(ErrorKind::ForeignKeyViolation) => StoreError::AccountNotFound,
                _ => backend(error),
            })?;
        } else {
            sqlx::query!(
                "DELETE FROM widget_list WHERE account_id = $1 AND layout = $2",
                id,
                layout,
            )
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        }
        self.audit_in(&mut tx, &events::widget_placement_changed(account))
            .await?;
        tx.commit().await.map_err(backend)
    }
}
