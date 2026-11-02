//! The invite quota: how many codes a role may create per month (M2-D).
//!
//! This is a deliberately small stand-in. The resource-limits slice (M2-H, docs/access.md
//! "Resource limits") owns the real `invites per month` limit kind, with per-role values and a
//! per-member override in the database. Until it lands, the numbers are these constants behind
//! one function, so replacing them is a one-function change and there is no second limits table
//! (`docs/coordination.md`, handoff M2-D to M2-H).
//!
//! "Per month" is a rolling 30 days, counted over every code the account created in that
//! window, whatever became of it: an unused or expired code does not give the slot back, so
//! creating and abandoning codes cannot be used to mint without bound.

use crate::access::Slug;

/// The window a quota is counted over: 30 days.
pub const QUOTA_WINDOW_SECS: i64 = 30 * 24 * 60 * 60;

/// How long a code lives after it is created: 7 days. Fixed, not a setting (D67); if it becomes
/// one it belongs in `docs/config.md`.
pub const INVITE_TTL_SECS: i64 = 7 * 24 * 60 * 60;

/// How many codes an account may create in a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quota {
    /// No limit (the sysop).
    Unlimited,
    /// At most `limit` codes per [`QUOTA_WINDOW_SECS`].
    Limited(u32),
}

impl Quota {
    /// The limit, if there is one.
    #[must_use]
    pub const fn limit(self) -> Option<u32> {
        match self {
            Self::Unlimited => None,
            Self::Limited(limit) => Some(limit),
        }
    }
}

/// The quota for an account whose highest role is `top_role`. A role this function does not
/// know (a custom role) gets the `member` number, never more.
#[must_use]
pub fn invite_quota(top_role: Option<&Slug>) -> Quota {
    match top_role.map(Slug::as_str) {
        Some("sysop") => Quota::Unlimited,
        Some("cosysop") => Quota::Limited(50),
        Some("moderator") => Quota::Limited(20),
        Some("trusted") => Quota::Limited(10),
        _ => Quota::Limited(3),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn quota(role: &str) -> Quota {
        invite_quota(Some(&Slug::parse(role).unwrap()))
    }

    #[test]
    fn quotas_grow_with_the_role_and_only_the_sysop_is_unlimited() {
        assert_eq!(quota("member"), Quota::Limited(3));
        assert_eq!(quota("trusted"), Quota::Limited(10));
        assert_eq!(quota("moderator"), Quota::Limited(20));
        assert_eq!(quota("cosysop"), Quota::Limited(50));
        assert_eq!(quota("sysop"), Quota::Unlimited);
    }

    #[test]
    fn an_unknown_role_or_none_gets_the_member_number_never_more() {
        assert_eq!(quota("custom-role"), Quota::Limited(3));
        assert_eq!(invite_quota(None), Quota::Limited(3));
        assert_eq!(quota("waiting"), Quota::Limited(3));
    }
}
