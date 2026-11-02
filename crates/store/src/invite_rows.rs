//! Row types for invites and the tree, and their one-time mapping to core's domain types.

use russh_bbs_core::invites::{InviteId, InviteStatus, InviteSummary, TreeNode};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

/// An invite joined to the account that used it, as read by the checked queries.
pub(crate) struct InviteRow {
    pub(crate) id: i64,
    pub(crate) created_at: i64,
    pub(crate) expires_at: i64,
    pub(crate) redeemed_at: Option<i64>,
    pub(crate) redeemed_username: Option<String>,
}

impl InviteRow {
    /// `now` is the store clock's reading, so an unused invite past its expiry reads `Expired`.
    pub(crate) fn into_summary(self, now: i64) -> Result<InviteSummary, StoreError> {
        let status = if self.redeemed_at.is_some() {
            let handle = self
                .redeemed_username
                .map(|name| {
                    sanitize_username(&name)
                        .map_err(|_| StoreError::Corrupt("stored username is not valid".into()))
                })
                .transpose()?;
            InviteStatus::Redeemed(handle)
        } else if self.expires_at <= now {
            InviteStatus::Expired
        } else {
            InviteStatus::Open
        };
        Ok(InviteSummary {
            id: InviteId::from_raw(self.id),
            created_at: UnixSeconds::from_raw(self.created_at),
            expires_at: UnixSeconds::from_raw(self.expires_at),
            status,
        })
    }
}

/// An account and its inviter, as read for the tree.
pub(crate) struct TreeRow {
    pub(crate) id: i64,
    pub(crate) username: String,
    pub(crate) invited_by: Option<i64>,
}

impl TreeRow {
    pub(crate) fn into_node(self) -> Result<TreeNode, StoreError> {
        Ok(TreeNode {
            account: AccountId::from_raw(self.id),
            handle: sanitize_username(&self.username)
                .map_err(|_| StoreError::Corrupt("stored username is not valid".into()))?,
            invited_by: self.invited_by.map(AccountId::from_raw),
        })
    }
}
