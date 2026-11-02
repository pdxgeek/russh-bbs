//! Backend-independent mapping for the limit tables: what a stored row means, written once
//! (D24). Each backend runs its own SQL and hands the rows here.

use russh_bbs_core::access::Slug;
use russh_bbs_core::limits::{Limit, LimitKind, MemberLimit, RoleLimit};
use russh_bbs_core::store::StoreError;

/// One `role_limit` row.
pub(crate) struct RoleLimitRow {
    pub(crate) role: String,
    pub(crate) kind: String,
    pub(crate) value: Option<i64>,
}

/// One `member_limit` row.
pub(crate) struct MemberLimitRow {
    pub(crate) kind: String,
    pub(crate) value: Option<i64>,
}

/// The rows for the named roles. A kind this code does not know is skipped (a newer database):
/// there is nothing here that could enforce it.
pub(crate) fn role_limits(
    rows: Vec<RoleLimitRow>,
    roles: &[Slug],
) -> Result<Vec<RoleLimit>, StoreError> {
    let mut out = Vec::new();
    for row in rows {
        let Some(kind) = LimitKind::parse(&row.kind) else {
            continue;
        };
        let role = Slug::parse(&row.role)
            .map_err(|_| StoreError::Corrupt("stored role name is not valid".into()))?;
        if roles.contains(&role) {
            out.push(RoleLimit {
                role,
                kind,
                limit: Limit::from_stored(row.value)?,
            });
        }
    }
    Ok(out)
}

/// The account's overrides, skipping unknown kinds.
pub(crate) fn member_limits(rows: Vec<MemberLimitRow>) -> Result<Vec<MemberLimit>, StoreError> {
    let mut out = Vec::new();
    for row in rows {
        if let Some(kind) = LimitKind::parse(&row.kind) {
            out.push(MemberLimit {
                kind,
                limit: Limit::from_stored(row.value)?,
            });
        }
    }
    Ok(out)
}

/// A limit as the integer the database stores (`NULL` for unlimited).
pub(crate) fn stored(limit: Limit) -> Result<Option<i64>, StoreError> {
    limit
        .to_stored()
        .ok_or_else(|| StoreError::Corrupt("a limit is too large to store".into()))
}

/// A stored counter as a number.
pub(crate) fn amount(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value)
        .map_err(|_| StoreError::Corrupt("a stored usage amount is negative".into()))
}

/// A number to add to a counter, as the database's integer.
pub(crate) fn to_i64(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::Corrupt("a usage amount is too large".into()))
}
