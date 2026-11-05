//! The effective menu tree for one permission set, flattened for the sysop preview (UI-6).
//!
//! It walks the same tree a session walks, through the same [`MenuTree::item_visible`], so it can
//! never show an item a session with those permissions could not pick.

use super::{MAX_DEPTH, MenuTree, Target};
use crate::access::PermissionCheck;

/// Most lines an outline holds; the tree caps keep real ones far below this.
const MAX_LINES: usize = 512;

/// What an outline line leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutlineKind {
    /// An action.
    Action,
    /// A submenu (its items follow, one level deeper).
    Menu,
    /// An area that is not built yet.
    Placeholder,
    /// A sysop page.
    Page,
}

/// One item of the effective tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineLine {
    /// How many menus deep (0 for the main menu's items).
    pub depth: usize,
    /// The key that picks it in its menu.
    pub hotkey: char,
    /// String-table key of the label, or the sysop's own sanitized text.
    pub label: &'static str,
    /// What it leads to.
    pub kind: OutlineKind,
}

/// The lines of the whole tree a session with `access` can reach, depth first.
pub type Outline = Vec<OutlineLine>;

/// Flatten `tree` for `access`.
#[must_use]
pub fn outline(tree: &MenuTree, access: &dyn PermissionCheck) -> Outline {
    let mut lines = Vec::new();
    let mut path = vec![tree.root];
    walk(tree, access, &mut path, &mut lines);
    lines
}

fn walk(
    tree: &MenuTree,
    access: &dyn PermissionCheck,
    path: &mut Vec<&'static str>,
    lines: &mut Outline,
) {
    let Some(&here) = path.last() else { return };
    let Some(menu) = tree.menu(here) else { return };
    for item in tree.visible_items(menu, access) {
        if lines.len() >= MAX_LINES {
            return;
        }
        let kind = match item.target {
            Target::Action(_) => OutlineKind::Action,
            Target::Menu(_) => OutlineKind::Menu,
            Target::Placeholder(_) => OutlineKind::Placeholder,
            Target::Page(_) => OutlineKind::Page,
        };
        lines.push(OutlineLine {
            depth: path.len() - 1,
            hotkey: item.hotkey,
            label: item.label,
            kind,
        });
        if let Target::Menu(id) = item.target
            && path.len() < MAX_DEPTH
            && !path.contains(&id)
        {
            path.push(id);
            walk(tree, access, path, lines);
            path.pop();
        }
    }
}
