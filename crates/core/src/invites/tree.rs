//! Putting the invite tree in reading order.

use std::collections::{HashMap, HashSet};

use super::store::TreeNode;
use crate::sanitize::Username;
use crate::store::AccountId;

/// Deepest indent drawn. A chain longer than this keeps its order and stops indenting.
pub const MAX_DEPTH: u16 = 16;

/// One line of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeLine {
    /// How many inviters are above this account (0 for a root).
    pub depth: u16,
    /// The account.
    pub account: AccountId,
    /// Its handle.
    pub handle: Username,
}

/// Order `nodes` depth first: each root (an account with no inviter, or whose inviter is not in
/// `nodes`), then its invitees by id, recursively. An account appears at most once even if the
/// data is malformed (a cycle cannot be produced by redemption, but a hand-edited row must not
/// hang a session).
#[must_use]
pub fn order_tree(nodes: &[TreeNode]) -> Vec<TreeLine> {
    let present: HashSet<AccountId> = nodes.iter().map(|n| n.account).collect();
    let mut children: HashMap<AccountId, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        match node.invited_by {
            Some(parent) if present.contains(&parent) && parent != node.account => {
                children.entry(parent).or_default().push(index);
            }
            Some(_) | None => roots.push(index),
        }
    }
    let by_id = |a: &usize, b: &usize| {
        let key = |i: &usize| nodes.get(*i).map(|n| n.account);
        key(a).cmp(&key(b))
    };
    roots.sort_by(by_id);
    for list in children.values_mut() {
        list.sort_by(by_id);
    }
    let mut out = Vec::with_capacity(nodes.len());
    let mut seen: HashSet<usize> = HashSet::new();
    // Explicit stack, so a long chain cannot overflow the call stack.
    let mut stack: Vec<(usize, u16)> = roots.into_iter().rev().map(|i| (i, 0)).collect();
    while let Some((index, depth)) = stack.pop() {
        let Some(node) = nodes.get(index) else {
            continue;
        };
        if !seen.insert(index) {
            continue;
        }
        out.push(TreeLine {
            depth: depth.min(MAX_DEPTH),
            account: node.account,
            handle: node.handle.clone(),
        });
        if let Some(kids) = children.get(&node.account) {
            stack.extend(kids.iter().rev().map(|&kid| (kid, depth.saturating_add(1))));
        }
    }
    // Anything left over sits in a cycle with no root; list it rather than hide it.
    for (index, node) in nodes.iter().enumerate() {
        if seen.insert(index) {
            out.push(TreeLine {
                depth: 0,
                account: node.account,
                handle: node.handle.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::sanitize::sanitize_username;

    fn node(id: i64, name: &str, by: Option<i64>) -> TreeNode {
        TreeNode {
            account: AccountId::from_raw(id),
            handle: sanitize_username(name).unwrap(),
            invited_by: by.map(AccountId::from_raw),
        }
    }

    fn shape(lines: &[TreeLine]) -> Vec<(u16, &str)> {
        lines.iter().map(|l| (l.depth, l.handle.as_str())).collect()
    }

    #[test]
    fn invitees_sit_under_their_inviter_depth_first() {
        let lines = order_tree(&[
            node(1, "founder", None),
            node(2, "alice", Some(1)),
            node(3, "bob", Some(1)),
            node(4, "carol", Some(2)),
            node(5, "dave", Some(4)),
            node(6, "erin", None),
        ]);
        assert_eq!(
            shape(&lines),
            [
                (0, "founder"),
                (1, "alice"),
                (2, "carol"),
                (3, "dave"),
                (1, "bob"),
                (0, "erin")
            ]
        );
    }

    #[test]
    fn an_inviter_outside_the_set_makes_a_root_and_a_cycle_is_listed_once() {
        let lines = order_tree(&[node(10, "orphan", Some(99)), node(11, "kid", Some(10))]);
        assert_eq!(shape(&lines), [(0, "orphan"), (1, "kid")]);
        let cyclic = order_tree(&[node(1, "aa", Some(2)), node(2, "bb", Some(1))]);
        assert_eq!(cyclic.len(), 2);
        let selfish = order_tree(&[node(1, "aa", Some(1))]);
        assert_eq!(shape(&selfish), [(0, "aa")]);
    }

    #[test]
    fn a_very_long_chain_is_ordered_and_the_indent_stops_growing() {
        let mut nodes = vec![node(1, "origin", None)];
        for id in 2..=5000_i64 {
            nodes.push(node(id, "someone", Some(id - 1)));
        }
        let lines = order_tree(&nodes);
        assert_eq!(lines.len(), 5000);
        assert_eq!(lines.iter().map(|l| l.depth).max(), Some(MAX_DEPTH));
        assert!(lines.windows(2).all(|w| w[0].account < w[1].account));
    }
}
