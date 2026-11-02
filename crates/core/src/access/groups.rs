//! Group nesting rules and the cached transitive closure (D33).
//!
//! A group holds accounts and other groups. [`GroupGraph`] is pure: the store loads every group
//! and direct edge inside a transaction, calls [`GroupGraph::apply`] (which **rejects a cycle or
//! a nesting deeper than [`MAX_GROUP_DEPTH`]** before anything is written), and then rewrites
//! `group_closure` from [`GroupGraph::closure`]. The checking path only reads the closure; it
//! never recurses.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::store::AccountId;

use super::slug::Slug;

/// The deepest chain of nested groups, counting the outermost: 8 groups inside one another is
/// allowed, 9 is refused.
pub const MAX_GROUP_DEPTH: usize = 8;

/// Store-assigned identifier of a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(i64);

impl GroupId {
    /// Wrap a raw id. Call this only where a row is mapped (the store).
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The raw id, for a query.
    #[must_use]
    pub const fn as_raw(self) -> i64 {
        self.0
    }
}

impl fmt::Display for GroupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// How much of a role or group the sysop may change.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    strum::EnumString,
    strum::IntoStaticStr,
)]
pub enum Kind {
    /// Shipped and fixed: not editable, not deletable (`sysop`, `waiting`, `everyone`).
    #[strum(serialize = "locked")]
    Locked,
    /// Shipped, editable, not deletable.
    #[strum(serialize = "builtin")]
    Builtin,
    /// Made by a sysop.
    #[strum(serialize = "custom")]
    Custom,
}

impl Kind {
    /// The stored text.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// Parse the stored text.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        text.parse().ok()
    }
}

/// A group as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupInfo {
    /// Its id.
    pub id: GroupId,
    /// Its name.
    pub name: Slug,
    /// How much of it may change.
    pub kind: Kind,
}

/// What can be inside a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Child {
    /// An account.
    Account(AccountId),
    /// Another group.
    Group(GroupId),
}

/// One change to group membership or existence. Every change goes through one store call so
/// the closure is rebuilt in the same transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupChange {
    /// Put `child` inside `group`.
    Add {
        /// The group gaining a member.
        group: GroupId,
        /// The account or group added.
        child: Child,
    },
    /// Take `child` out of `group`.
    Remove {
        /// The group losing a member.
        group: GroupId,
        /// The account or group removed.
        child: Child,
    },
    /// Delete a group. Its memberships and grants go with it.
    Delete(GroupId),
}

/// Why a group change was refused. Nothing was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GroupError {
    /// A group named in the change does not exist.
    #[error("no such group")]
    UnknownGroup,
    /// The group is `locked` (`everyone`): it cannot be edited, deleted or nested.
    #[error("that group cannot be changed")]
    Locked,
    /// The group is built in and cannot be deleted.
    #[error("that group cannot be deleted")]
    Builtin,
    /// The change would put a group inside itself, directly or through other groups.
    #[error("that would put a group inside itself")]
    Cycle,
    /// The change would nest groups deeper than the limit.
    #[error("groups cannot be nested that deep")]
    TooDeep,
}

/// All groups and their direct memberships, as loaded in the transaction that changes them.
#[derive(Debug, Clone, Default)]
pub struct GroupGraph {
    kinds: BTreeMap<GroupId, Kind>,
    edges: BTreeSet<(GroupId, Child)>,
}

impl GroupGraph {
    /// Build the graph from every group and every direct membership.
    #[must_use]
    pub fn new(
        groups: impl IntoIterator<Item = (GroupId, Kind)>,
        edges: impl IntoIterator<Item = (GroupId, Child)>,
    ) -> Self {
        Self {
            kinds: groups.into_iter().collect(),
            edges: edges.into_iter().collect(),
        }
    }

    /// Validate `change` and apply it to this graph. On error the graph is unchanged.
    pub fn apply(&mut self, change: GroupChange) -> Result<(), GroupError> {
        match change {
            GroupChange::Add { group, child } => {
                self.editable(group)?;
                if let Child::Group(inner) = child {
                    self.editable(inner)?;
                    if inner == group || self.reaches(inner, group) {
                        return Err(GroupError::Cycle);
                    }
                }
                let mut next = self.clone();
                next.edges.insert((group, child));
                if next.depth() > MAX_GROUP_DEPTH {
                    return Err(GroupError::TooDeep);
                }
                *self = next;
            }
            GroupChange::Remove { group, child } => {
                self.editable(group)?;
                self.edges.remove(&(group, child));
            }
            GroupChange::Delete(group) => {
                match self.kinds.get(&group) {
                    None => return Err(GroupError::UnknownGroup),
                    Some(Kind::Locked) => return Err(GroupError::Locked),
                    Some(Kind::Builtin) => return Err(GroupError::Builtin),
                    Some(Kind::Custom) => {}
                }
                self.kinds.remove(&group);
                self.edges
                    .retain(|(parent, child)| *parent != group && *child != Child::Group(group));
            }
        }
        Ok(())
    }

    fn editable(&self, group: GroupId) -> Result<(), GroupError> {
        match self.kinds.get(&group) {
            None => Err(GroupError::UnknownGroup),
            Some(Kind::Locked) => Err(GroupError::Locked),
            Some(Kind::Builtin | Kind::Custom) => Ok(()),
        }
    }

    /// The groups directly inside `group`.
    fn inner_groups(&self, group: GroupId) -> impl Iterator<Item = GroupId> + '_ {
        self.edges
            .range((group, Child::Account(AccountId::from_raw(i64::MIN)))..)
            .take_while(move |(parent, _)| *parent == group)
            .filter_map(|(_, child)| match child {
                Child::Group(inner) => Some(*inner),
                Child::Account(_) => None,
            })
    }

    /// Whether `target` is `from` or somewhere inside it.
    fn reaches(&self, from: GroupId, target: GroupId) -> bool {
        let mut seen = BTreeSet::new();
        let mut stack = vec![from];
        while let Some(group) = stack.pop() {
            if group == target {
                return true;
            }
            if seen.insert(group) {
                stack.extend(self.inner_groups(group));
            }
        }
        false
    }

    /// The longest chain of nested groups, counting the outermost. The graph is a DAG when this
    /// is called (cycles are refused before an edge is added), but a visited guard keeps a
    /// hand-edited database with a cycle from looping.
    fn depth(&self) -> usize {
        fn longest(graph: &GroupGraph, group: GroupId, path: &mut Vec<GroupId>, best: &mut usize) {
            path.push(group);
            *best = (*best).max(path.len());
            for inner in graph.inner_groups(group) {
                if !path.contains(&inner) {
                    longest(graph, inner, path, best);
                }
            }
            path.pop();
        }
        let mut best = 0;
        for group in self.kinds.keys() {
            longest(self, *group, &mut Vec::new(), &mut best);
        }
        best
    }

    /// The transitive closure over accounts: every `(group, account)` where the account is in
    /// the group directly or through nested groups.
    #[must_use]
    pub fn closure(&self) -> BTreeSet<(GroupId, AccountId)> {
        let mut rows = BTreeSet::new();
        for group in self.kinds.keys() {
            let mut seen = BTreeSet::new();
            let mut stack = vec![*group];
            while let Some(current) = stack.pop() {
                if !seen.insert(current) {
                    continue;
                }
                for (_, child) in self
                    .edges
                    .range((current, Child::Account(AccountId::from_raw(i64::MIN)))..)
                    .take_while(|(parent, _)| *parent == current)
                {
                    match child {
                        Child::Account(account) => {
                            rows.insert((*group, *account));
                        }
                        Child::Group(inner) => stack.push(*inner),
                    }
                }
            }
        }
        rows
    }
}
