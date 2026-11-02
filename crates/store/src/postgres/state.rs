//! Changing an account's state over Postgres (M2-D): lock the row, check
//! [`AccountState::can_become`] against the state it holds now, write, audit, in one transaction.

use russh_bbs_core::audit::{Actor, events};
use russh_bbs_core::store::{
    AccountId, AccountState, AccountStateStore, StateChange, StateChangeError, StoreError,
};

use super::PostgresStore;
use crate::error::backend;

impl AccountStateStore for PostgresStore {
    async fn change_state(
        &self,
        account: AccountId,
        to: AccountState,
        actor: Actor,
    ) -> Result<StateChange, StateChangeError> {
        let id = account.as_raw();
        let to_text = to.as_str();
        let mut tx = self.pool().begin().await.map_err(backend)?;
        // Locked until commit, so a racing caller reads the state this one leaves behind.
        let current =
            sqlx::query_scalar!("SELECT state FROM accounts WHERE id = $1 FOR UPDATE", id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(backend)?
                .ok_or(StateChangeError::NotFound)?;
        let from = AccountState::parse(&current)
            .ok_or_else(|| StoreError::Corrupt("stored account state is not known".into()))?;
        if !from.can_become(to) {
            return Err(StateChangeError::NotAllowed { from, to });
        }
        sqlx::query!("UPDATE accounts SET state = $1 WHERE id = $2", to_text, id)
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
