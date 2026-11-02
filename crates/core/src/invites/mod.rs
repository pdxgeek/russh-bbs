//! Invite codes, quotas and the invite tree (M2-D, D67, D68).
//!
//! * [`InviteCode`] / [`CodeHash`]: a 160-bit random code in an easy-to-type alphabet; only its
//!   SHA-256 is stored.
//! * [`InviteStore`]: the persistence trait. A backend runs creation (with the quota check) and
//!   redemption (mark used, create the account and key, audit) each in **one transaction**.
//! * [`create_invite`], [`redeem_invite`], [`my_invites`], [`invite_tree`]: the actions, with
//!   their permission checks (`invites.create`, `invite.redeem`, `invites.view_tree`).
//! * [`Quota`]: the account's `invites_per_month` limit (M2-H) per UTC calendar month.
//! * [`strings`]: the fixed words of the invite screens.
//!
//! **No oracle.** A wrong, unknown, used or expired code, a taken handle and a key that already
//! has an account all come back as [`RedeemError::Rejected`], and the waiting room words them
//! identically. A refused redemption writes nothing, so the code stays usable.

mod code;
mod quota;
mod screens;
mod service;
mod store;
pub mod strings;
mod tree;

pub use code::{CODE_BYTES, CODE_TEXT_MAX_CHARS, CodeHash, InviteCode};
pub use quota::{INVITE_TTL_SECS, Quota, month_start, next_month_start};
pub use screens::{InviteOutcome, InviteRequest, InviteScreen, TREE_PAGE_LINES};
pub use service::{
    CreatedInvite, InviteError, LIST_LIMIT, TREE_LIMIT, create_invite, create_invite_within,
    invite_tree, my_invites, redeem_invite,
};
pub use store::{
    CreateError, InviteId, InviteRecord, InviteStatus, InviteStore, InviteSummary, NewInvite,
    REDEEMED_STATE, RedeemError, Redeemed, RejectReason, TreeNode,
};
pub use tree::{MAX_DEPTH, TreeLine, order_tree};

#[cfg(test)]
mod tests;
