//! Changing an account's state over SQLite (M2-D): read the current state, check
//! [`AccountState::can_become`], write, audit, all in one transaction.

use russh_bbs_core::audit::{Actor, events};
use russh_bbs_core::store::{
    AccountId, AccountState, AccountStateStore, StateChange, StateChangeError, StoreError,
};

use super::SqliteStore;
use crate::error::backend;

impl AccountStateStore for SqliteStore {
    async fn change_state(
        &self,
        account: AccountId,
        to: AccountState,
        actor: Actor,
    ) -> Result<StateChange, StateChangeError> {
        let id = account.as_raw();
        let to_text = to.as_str();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        // A write first, so the transaction holds the write lock before it reads: the state
        // read below cannot change before the update. Affects no row's value.
        let touched = sqlx::query!("UPDATE accounts SET state = state WHERE id = ?", id)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        if touched.rows_affected() == 0 {
            return Err(StateChangeError::NotFound);
        }
        let current = sqlx::query_scalar!("SELECT state FROM accounts WHERE id = ?", id)
            .fetch_one(&mut *tx)
            .await
            .map_err(backend)?;
        let from = AccountState::parse(&current)
            .ok_or_else(|| StoreError::Corrupt("stored account state is not known".into()))?;
        if !from.can_become(to) {
            return Err(StateChangeError::NotAllowed { from, to });
        }
        sqlx::query!("UPDATE accounts SET state = ? WHERE id = ?", to_text, id)
            .execute(&mut *tx)
            .await
            .map_err(backend)?;
        self.audit_in(
            &mut tx,
            &events::account_state_changed(actor, account, from, to),
        )
        .await?;
        tx.commit().await.map_err(backend)?;
        Ok(StateChange { account, from, to })
    }
}
