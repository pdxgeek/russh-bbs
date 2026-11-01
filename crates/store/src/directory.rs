//! The access rule over the store: a key is a member only if it is bound to an **active**
//! account. Anything else, including any error, is "not a member" (fail closed).

use russh_bbs_core::access::{Fingerprint, MemberDirectory};
use russh_bbs_core::store::AccountStore;
use tracing::warn;

use crate::SqliteStore;

impl MemberDirectory for SqliteStore {
    async fn is_member(&self, key: &Fingerprint) -> bool {
        match self.account_by_fingerprint(key).await {
            Ok(Some(account)) => account.state.is_member_access(),
            Ok(None) => false,
            Err(error) => {
                // Fail closed: a broken store must never grant access.
                warn!(%error, fingerprint = %key, "member lookup failed; treating key as unknown");
                false
            }
        }
    }
}
