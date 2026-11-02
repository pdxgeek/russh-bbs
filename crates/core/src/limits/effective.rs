//! The pure rule that turns "which roles, which stored values, which override" into the limit
//! that applies. No clock, no store: the unit tests below are the specification.

use std::collections::BTreeMap;

use crate::access::{Scope, Slug, Snapshot, Source};

use super::kind::{Limit, LimitKind};

/// A stored per-role value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleLimit {
    /// The role's name.
    pub role: Slug,
    /// What it limits.
    pub kind: LimitKind,
    /// The value; [`Limit::Unlimited`] is stored as `NULL`.
    pub limit: Limit,
}

/// A stored per-member override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberLimit {
    /// What it limits.
    pub kind: LimitKind,
    /// The value that replaces the roles' values for this member.
    pub limit: Limit,
}

/// The limit of every kind that applies to one account right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveLimits(BTreeMap<LimitKind, Limit>);

impl EffectiveLimits {
    /// Every kind unlimited.
    #[must_use]
    pub fn unlimited() -> Self {
        Self(
            LimitKind::ALL
                .iter()
                .map(|&kind| (kind, Limit::Unlimited))
                .collect(),
        )
    }

    /// The limit of `kind`.
    #[must_use]
    pub fn get(&self, kind: LimitKind) -> Limit {
        self.0.get(&kind).copied().unwrap_or(Limit::Unlimited)
    }

    fn set(&mut self, kind: LimitKind, limit: Limit) {
        self.0.insert(kind, limit);
    }
}

/// The roles an account holds **globally**, from its snapshot. A role held only on one
/// resource (a per-board moderator) does not raise a limit that is about the whole account.
#[must_use]
pub fn roles_held(snapshot: &Snapshot) -> Vec<Slug> {
    let mut roles: Vec<Slug> = snapshot
        .held()
        .iter()
        .filter(|h| h.scope == Scope::Global)
        .filter_map(|h| match &h.source {
            Source::Role { role, .. } => Some(role.clone()),
            Source::Allow { .. } => None,
        })
        .collect();
    roles.sort();
    roles.dedup();
    roles
}

/// The limit that applies to an account holding `roles`.
///
/// 1. `sysop` is unlimited in every kind, whatever is stored, so the founder cannot lock
///    themselves out with a bad edit.
/// 2. Otherwise a per-member override, if set for the kind, is the limit.
/// 3. Otherwise the **most generous** value among the held roles that have one (additive, like
///    permissions, D33). A role with no stored value for the kind is skipped, so a custom role
///    that only adds a permission does not loosen anything.
/// 4. If no held role has a value, the kind is unlimited.
#[must_use]
pub fn effective_limits(
    roles: &[Slug],
    role_limits: &[RoleLimit],
    overrides: &[MemberLimit],
) -> EffectiveLimits {
    let mut out = EffectiveLimits::unlimited();
    if roles.contains(&Slug::SYSOP) {
        return out;
    }
    for &kind in LimitKind::ALL {
        let from_roles = role_limits
            .iter()
            .filter(|row| row.kind == kind && roles.contains(&row.role))
            .map(|row| row.limit)
            .max();
        let from_member = overrides.iter().find(|o| o.kind == kind).map(|o| o.limit);
        out.set(kind, from_member.or(from_roles).unwrap_or(Limit::Unlimited));
    }
    out
}
