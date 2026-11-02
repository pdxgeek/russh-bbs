//! What a member holds, and the one function that answers "may they?" (D33).
//!
//! The store flattens everything an account holds (its own grants, the groups it is in through
//! the cached closure, and the implicit `everyone` group) into a list of [`Held`] facts, one per
//! permission. [`resolve`] only matches that list: it does no I/O, reads no clock and knows
//! nothing about ranks, groups or account states. Access is **additive only**: there is no deny
//! and no precedence to reason about; the order below only makes the *reason* stable.
//!
//! 1. a role holds the permission **globally**;
//! 2. a role holds it **on this resource** (the role was granted scoped);
//! 3. an **allow entry** names the account or one of its groups (on this resource, or globally).
//!
//! A check with no resource matches only global holdings. [`reach`] answers the list-shaped
//! question ("which resources of this kind?") from the same facts, so menus, search, new-scan,
//! feeds out and the SFTP tree never have a second mechanism.

use std::fmt;

use strum::{EnumString, IntoStaticStr};

use super::PermissionCheck;
use super::permission::Permission;
use super::slug::Slug;
use crate::store::AccountId;

/// The kinds of thing a permission can be scoped to (`docs/access.md`, "Resource security").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, EnumString, IntoStaticStr)]
pub enum ResourceKind {
    /// A forum board.
    #[strum(serialize = "forum")]
    Forum,
    /// A file area.
    #[strum(serialize = "file_area")]
    FileArea,
    /// A chat room.
    #[strum(serialize = "room")]
    Room,
    /// A door game or program.
    #[strum(serialize = "door")]
    Door,
    /// An RSS feed.
    #[strum(serialize = "feed")]
    Feed,
    /// A poll.
    #[strum(serialize = "poll")]
    Poll,
    /// A calendar event.
    #[strum(serialize = "event")]
    Event,
}

impl ResourceKind {
    /// The stored name.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.into()
    }

    /// Parse a stored name.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        text.parse().ok()
    }
}

/// The id of one resource, assigned by the slice that owns that kind. Always positive; `0` is
/// how the database spells "no resource".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceId(i64);

impl ResourceId {
    /// Wrap a resource id. `None` for zero or a negative number.
    #[must_use]
    pub const fn new(raw: i64) -> Option<Self> {
        if raw > 0 { Some(Self(raw)) } else { None }
    }

    /// The raw id, for a query.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }
}

/// One resource: its kind and id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceRef {
    /// What kind of resource.
    pub kind: ResourceKind,
    /// Which one.
    pub id: ResourceId,
}

/// Where a held permission applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Everywhere.
    Global,
    /// On one resource only.
    Resource(ResourceRef),
}

/// Through whom a grant reached the account.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Via {
    /// Granted to the account itself.
    Account,
    /// Granted to a group the account is in (directly or nested). `everyone` is a group.
    Group(Slug),
    /// The role every unknown or inactive key gets (the waiting room).
    Waiting,
}

/// Why a permission is held: which role, or an allow entry, and through whom.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    /// A role that holds the permission.
    Role {
        /// The role's name.
        role: Slug,
        /// The role's rank (`docs/access.md`: ordering only; kept so the telnet slice can drop
        /// `cosysop` and above, D27).
        rank: u8,
        /// Through whom the role was granted.
        via: Via,
    },
    /// An allow-list entry for this permission.
    Allow {
        /// Through whom the entry names the account.
        via: Via,
    },
}

/// One permission an account holds, with its scope and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// The permission held.
    pub permission: Permission,
    /// Where it applies.
    pub scope: Scope,
    /// Why it is held.
    pub source: Source,
}

/// Everything one account holds at one moment, loaded by the store at the start of an action.
///
/// For an unknown key, or an account that is not `active`, the store builds the `waiting`
/// role's snapshot (`account` is `None` for an unknown key). Nothing in here is cached across
/// actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    account: Option<AccountId>,
    held: Vec<Held>,
}

impl Snapshot {
    /// Build a snapshot. The store's loader is the production caller; tests build their own.
    #[must_use]
    pub const fn new(account: Option<AccountId>, held: Vec<Held>) -> Self {
        Self { account, held }
    }

    /// A snapshot of one role held globally: every permission of the role, with the account
    /// holding it through `via`. The store builds the `waiting` snapshot with this; tests and
    /// fixtures use it to say "a session that holds exactly these".
    #[must_use]
    pub fn from_role(
        account: Option<AccountId>,
        role: &Slug,
        rank: u8,
        via: &Via,
        permissions: impl IntoIterator<Item = Permission>,
    ) -> Self {
        let held = permissions
            .into_iter()
            .map(|permission| Held {
                permission,
                scope: Scope::Global,
                source: Source::Role {
                    role: role.clone(),
                    rank,
                    via: via.clone(),
                },
            })
            .collect();
        Self::new(account, held)
    }

    /// A snapshot that holds nothing.
    #[must_use]
    pub const fn empty() -> Self {
        Self::new(None, Vec::new())
    }

    /// The signed-in account, or `None` for the waiting room.
    #[must_use]
    pub const fn account(&self) -> Option<AccountId> {
        self.account
    }

    /// What is held, for tests and the sysop UI.
    #[must_use]
    pub fn held(&self) -> &[Held] {
        &self.held
    }

    /// The highest-ranked role the account holds, for "signed in as ...". `None` when it holds
    /// no role. Ties go to the lexically smaller name, so the answer is stable.
    #[must_use]
    pub fn top_role(&self) -> Option<&Slug> {
        self.held
            .iter()
            .filter_map(|h| match &h.source {
                Source::Role { role, rank, .. } => Some((*rank, role)),
                Source::Allow { .. } => None,
            })
            .max_by(|(ra, na), (rb, nb)| ra.cmp(rb).then_with(|| nb.cmp(na)))
            .map(|(_, role)| role)
    }

    /// The rank of the highest-ranked role held **globally**, or `None` when it holds none (a
    /// role held on one resource gives no standing over roles board-wide). The editing services
    /// compare it with the rank of the role they are asked to change.
    #[must_use]
    pub fn top_rank(&self) -> Option<u8> {
        self.held
            .iter()
            .filter(|h| h.scope == Scope::Global)
            .filter_map(|h| match &h.source {
                Source::Role { rank, .. } => Some(*rank),
                Source::Allow { .. } => None,
            })
            .max()
    }

    /// May the account do `permission` (on `resource`, when given)? See [`resolve`].
    #[must_use]
    pub fn resolve(&self, permission: Permission, resource: Option<ResourceRef>) -> Decision {
        resolve(self, permission, resource)
    }

    /// Which resources of `kind` does the account hold `permission` on? See [`reach`].
    #[must_use]
    pub fn reach(&self, permission: Permission, kind: ResourceKind) -> Reach {
        reach(self, permission, kind)
    }

    /// Check and, when granted, hand back the proof an action function takes. Every action in
    /// core takes a [`Granted`], so an action cannot run without passing the check.
    pub fn require(
        &self,
        permission: Permission,
        resource: Option<ResourceRef>,
    ) -> Result<Granted, Denied> {
        match resolve(self, permission, resource) {
            Decision::Granted(reason) => Ok(Granted {
                permission,
                resource,
                account: self.account,
                reason,
            }),
            Decision::Denied => Err(Denied { permission }),
        }
    }
}

/// Menus ask only this (M2-A's hook): a global check, no resource.
impl PermissionCheck for Snapshot {
    fn allows(&self, permission: Permission) -> bool {
        resolve(self, permission, None).is_granted()
    }
}

/// Why a check was granted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reason(pub Source);

/// The answer to a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Held, and why.
    Granted(Reason),
    /// Not held through any role, group or allow entry.
    Denied,
}

impl Decision {
    /// Whether the check passed.
    #[must_use]
    pub const fn is_granted(&self) -> bool {
        matches!(self, Self::Granted(_))
    }
}

/// How well a held fact answers a check: lower is stronger. `None`: it does not apply.
fn priority(held: &Held, resource: Option<ResourceRef>) -> Option<u8> {
    let applies = match (held.scope, resource) {
        (Scope::Global, _) => true,
        (Scope::Resource(held_on), Some(asked)) => held_on == asked,
        (Scope::Resource(_), None) => false,
    };
    if !applies {
        return None;
    }
    Some(match (&held.source, held.scope) {
        (Source::Role { .. }, Scope::Global) => 0,
        (Source::Role { .. }, Scope::Resource(_)) => 1,
        (Source::Allow { .. }, _) => 2,
    })
}

/// The one resolution function: does `snapshot` hold `permission` (on `resource`)?
///
/// Pure and additive. The first strongest matching fact is the reason.
#[must_use]
pub fn resolve(
    snapshot: &Snapshot,
    permission: Permission,
    resource: Option<ResourceRef>,
) -> Decision {
    snapshot
        .held
        .iter()
        .filter(|h| h.permission == permission)
        .filter_map(|h| priority(h, resource).map(|p| (p, h)))
        .min_by_key(|(p, _)| *p)
        .map_or(Decision::Denied, |(_, h)| {
            Decision::Granted(Reason(h.source.clone()))
        })
}

/// The resources of one kind a permission reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// None.
    None,
    /// All of them (a global holding).
    All,
    /// Exactly these, sorted, no duplicates.
    Only(Vec<ResourceId>),
}

impl Reach {
    /// Whether `id` is reached.
    #[must_use]
    pub fn contains(&self, id: ResourceId) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Only(ids) => ids.binary_search(&id).is_ok(),
        }
    }
}

/// Which resources of `kind` does `snapshot` hold `permission` on? A global holding reaches all
/// of them. Built from the same rules as [`resolve`]: `resolve(p, Some(r))` is granted exactly
/// when `reach(p, r.kind)` contains `r.id` (a property test pins it).
#[must_use]
pub fn reach(snapshot: &Snapshot, permission: Permission, kind: ResourceKind) -> Reach {
    let mut ids = Vec::new();
    for held in snapshot.held.iter().filter(|h| h.permission == permission) {
        match held.scope {
            Scope::Global => return Reach::All,
            Scope::Resource(r) if r.kind == kind => ids.push(r.id),
            Scope::Resource(_) => {}
        }
    }
    if ids.is_empty() {
        return Reach::None;
    }
    ids.sort_unstable();
    ids.dedup();
    Reach::Only(ids)
}

/// Proof that a check passed. Only [`Snapshot::require`] makes one, so a function that takes
/// `&Granted` cannot be called without going through `core::access`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Granted {
    permission: Permission,
    resource: Option<ResourceRef>,
    account: Option<AccountId>,
    reason: Reason,
}

impl Granted {
    /// The permission that was checked.
    #[must_use]
    pub const fn permission(&self) -> Permission {
        self.permission
    }

    /// The resource it was checked on, if any.
    #[must_use]
    pub const fn resource(&self) -> Option<ResourceRef> {
        self.resource
    }

    /// The account it was granted to. `None` for the waiting room.
    #[must_use]
    pub const fn account(&self) -> Option<AccountId> {
        self.account
    }

    /// Why it was granted.
    #[must_use]
    pub const fn reason(&self) -> &Reason {
        &self.reason
    }
}

/// A check that did not pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Denied {
    /// The permission that was missing.
    pub permission: Permission,
}

impl fmt::Display for Denied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "permission {} is not held", self.permission.name())
    }
}

impl std::error::Error for Denied {}
