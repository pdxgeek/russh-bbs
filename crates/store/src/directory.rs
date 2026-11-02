//! Backend-independent rules over the account store, shared by every backend's trait impls:
//! a key is a member only if it is bound to an **active** account. Anything else, including
//! any error, is "not a member" (fail closed).

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::store::{AccountStore, Role, SettingKey, SettingsStore, StoreError};
use tracing::warn;

pub(crate) async fn is_member<S: AccountStore>(store: &S, key: &Fingerprint) -> bool {
    match store.account_by_fingerprint(key).await {
        Ok(Some(account)) => account.state.is_member_access(),
        Ok(None) => false,
        Err(error) => {
            // Fail closed: a broken store must never grant access.
            warn!(%error, fingerprint = %key, "member lookup failed; treating key as unknown");
            false
        }
    }
}

pub(crate) async fn role_of<S: AccountStore>(store: &S, key: &Fingerprint) -> Option<Role> {
    match store.account_by_fingerprint(key).await {
        Ok(Some(account)) if account.state.is_member_access() => Some(account.role),
        Ok(_) => None,
        Err(error) => {
            warn!(%error, fingerprint = %key, "role lookup failed; treating key as unknown");
            None
        }
    }
}

/// Any stored value counts as claimed: fail closed on a hand-edited row.
pub(crate) async fn founder_claimed<S: SettingsStore>(store: &S) -> Result<bool, StoreError> {
    Ok(store
        .get_setting(SettingKey::FounderClaimed)
        .await?
        .is_some())
}
