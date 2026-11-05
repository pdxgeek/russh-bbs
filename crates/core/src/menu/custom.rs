//! Sysop menu files (UI-6, D54): the built-in tree, rearranged.
//!
//! A [`Spec`] says how to reorder, rename, re-key, hide and group the **built-in items** and
//! where to put sysop **pages**. [`build`] turns it into a [`MenuTree`] or refuses it with a
//! [`CustomError`] naming the first problem. The file can never add an action, a target or a
//! permission: every item is either one of the tree's own items (found by its id, see
//! [`builtin_ids`]; it keeps its action and its permission), a link to a group the same file
//! defines, or a page (which grants nothing). Which items a session sees is still
//! [`MenuTree::item_visible`], that is `core::access`, so hiding, moving or grouping an item can
//! only make it reachable by fewer people, never by more.
//!
//! **Rules enforced here**, on top of the ones [`MenuTree::validate`] repeats on the result:
//! every menu has a non-empty, bounded item list; hotkeys are unique ASCII letters or digits per
//! menu (`?` stays the help key); each item is placed at most once; every group is linked and
//! every built-in menu is either reachable or pruned because its link was hidden; nesting is at
//! most [`MAX_NESTING`] deep; labels and titles are bounded text the shared sanitizer would not
//! change.
//!
//! **Memory.** A [`MenuTree`] is `'static` data, so a built tree is leaked, once. Strings are
//! interned (at most [`MAX_STRINGS`] of at most [`MAX_STRING_BYTES`] bytes), an identical tree
//! is reused, and at most [`MAX_LOADS`] different trees are built per process: a sysop who
//! reloads many different files is told to restart. The leak is therefore bounded by constants.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, PoisonError};

use super::{Item, Menu, MenuError, MenuTree, Target, main_tree};
use crate::access::Permission;
use crate::sanitize::SingleLine;

/// Most groups (new submenus) a file may define.
pub const MAX_GROUPS: usize = 8;
/// Most items in one menu.
pub const MAX_ITEMS: usize = 24;
/// Longest label or title, in characters.
pub const MAX_LABEL_CHARS: usize = 32;
/// Most menus below the root on one path (the session's stack is [`MAX_DEPTH`](super::MAX_DEPTH)).
pub const MAX_NESTING: usize = 4;
/// Most page items in one file.
pub const MAX_PAGES: usize = 16;
/// Most different trees one process builds.
pub const MAX_LOADS: usize = 64;
/// Most strings kept for all trees.
const MAX_STRINGS: usize = 2048;
/// Longest kept string, in bytes.
const MAX_STRING_BYTES: usize = 160;
/// Longest id of a group or page.
const ID_MAX: usize = 24;

/// How to rearrange the tree: one entry per menu the file mentions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Spec {
    /// The menus, in file order.
    pub menus: Vec<MenuSpec>,
}

/// One menu of a file: a built-in menu to reorder, or a new group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSpec {
    /// A built-in menu id (see [`builtin_menus`]) or the id of a new group.
    pub id: String,
    /// A new title (text, not a string-table key). Groups need one to be linked without a label.
    pub title: Option<String>,
    /// The items, in the order to show. Built-in items left out of every menu keep their place
    /// at the end of their own menu.
    pub items: Vec<ItemSpec>,
}

/// One item in a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSpec {
    /// What it is: a built-in item id (`main.forums`), `group:<id>` or `page:<id>`.
    pub target: String,
    /// A new hotkey. Required for groups and pages.
    pub key: Option<char>,
    /// A new label (text). Pages and groups default to their own title.
    pub label: Option<String>,
    /// Leave the item out of the menu entirely: not shown, and its hotkey does nothing.
    pub hidden: bool,
}

/// Why a file was refused. Messages name the menu and item, never echo more than a clipped
/// `Debug` rendering of what the file said.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CustomError {
    /// A menu is listed twice.
    #[error("menu {0} is listed twice")]
    DuplicateMenu(String),
    /// A menu id is neither built in nor a valid group id.
    #[error(
        "menu id {0} is not a built-in menu or a valid group id (a-z, 0-9, _ and -; up to {ID_MAX})"
    )]
    BadMenuId(String),
    /// More groups than [`MAX_GROUPS`].
    #[error("more than {MAX_GROUPS} groups")]
    TooManyGroups,
    /// A menu has more items than [`MAX_ITEMS`].
    #[error("menu {0} has more than {MAX_ITEMS} items")]
    TooManyItems(String),
    /// More page items than [`MAX_PAGES`].
    #[error("more than {MAX_PAGES} page items")]
    TooManyPages,
    /// An item names nothing known.
    #[error("menu {menu}: {target} is not a built-in item, a group of this file or a page")]
    UnknownTarget {
        /// The menu holding the item.
        menu: String,
        /// What the file said.
        target: String,
    },
    /// An item is placed twice.
    #[error("{0} is placed in more than one menu, or twice in one")]
    DuplicateTarget(String),
    /// A group or page item has no key.
    #[error("menu {menu}: {target} needs a key")]
    MissingKey {
        /// The menu holding the item.
        menu: String,
        /// The item.
        target: String,
    },
    /// A group has no title and its item no label.
    #[error("menu {menu}: {target} needs a label or a title")]
    MissingLabel {
        /// The menu holding the item.
        menu: String,
        /// The item.
        target: String,
    },
    /// A label or title is empty, too long, or has characters the sanitizer would change.
    #[error(
        "{what} must be 1 to {MAX_LABEL_CHARS} plain characters (no control, invisible, private-use or marker characters)"
    )]
    BadText {
        /// Which text.
        what: String,
    },
    /// A hotkey is not an ASCII letter or digit.
    #[error("menu {menu}: key {key:?} is not an ASCII letter or digit (`?` is always help)")]
    BadHotkey {
        /// The menu.
        menu: String,
        /// The key.
        key: char,
    },
    /// Two items of one menu share a hotkey.
    #[error("menu {menu}: key {key:?} is used twice")]
    DuplicateHotkey {
        /// The menu.
        menu: String,
        /// The key.
        key: char,
    },
    /// A menu would have no items.
    #[error("menu {0} would be empty")]
    EmptyMenu(String),
    /// A group is defined but no menu links to it.
    #[error("group {0} is defined but nothing links to it")]
    UnlinkedGroup(String),
    /// A menu cannot be reached from the main menu.
    #[error("menu {0} cannot be reached from the main menu")]
    Unreachable(String),
    /// Menus nest deeper than [`MAX_NESTING`].
    #[error("menus nest deeper than {MAX_NESTING} levels")]
    TooDeep,
    /// A page the file links does not exist.
    #[error("page {0} does not exist")]
    UnknownPage(String),
    /// The built tree failed [`MenuTree::validate`] (a bug in the rules above).
    #[error("the rearranged menu is not valid: {0}")]
    Invalid(MenuError),
    /// More than [`MAX_LOADS`] different trees were built in this process.
    #[error(
        "too many different menu files were loaded in this run; restart the server to load more"
    )]
    TooManyLoads,
    /// The string budget is used up.
    #[error("too many different menu strings were loaded in this run; restart the server")]
    TooManyStrings,
}

/// A tree built from a file.
#[derive(Debug, Clone, Copy)]
pub struct Built {
    /// The tree, ready for [`MenuSession::new`](super::MenuSession::new).
    pub tree: &'static MenuTree,
}

/// The ids of the built-in menus, in tree order.
#[must_use]
pub fn builtin_menus() -> Vec<&'static str> {
    main_tree().menus.iter().map(|menu| menu.id).collect()
}

/// Every built-in item as `(id, home menu id, default hotkey)`. The id is the item's label key
/// without `menu.` and with `.item.` shortened to a dot (`menu.main.item.forums` is
/// `main.forums`), which is stable across releases because it is also the string-table key.
#[must_use]
pub fn builtin_ids() -> Vec<(String, &'static str, char)> {
    let mut out = Vec::new();
    for menu in main_tree().menus {
        for item in menu.items {
            out.push((item_id(item), menu.id, item.hotkey));
        }
    }
    out
}

fn item_id(item: &Item) -> String {
    item.label
        .strip_prefix("menu.")
        .unwrap_or(item.label)
        .replace(".item.", ".")
}

/// Whether `id` is a valid group or page id: 1 to 24 of `a-z 0-9 _ -`, not starting with `_`/`-`.
#[must_use]
pub fn valid_page_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.len() <= ID_MAX
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// A clipped, escaped rendering of file text for an error message.
fn clip(text: &str) -> String {
    let shown: String = text.chars().take(40).flat_map(char::escape_debug).collect();
    if text.chars().count() > 40 {
        format!("\"{shown}...\"")
    } else {
        format!("\"{shown}\"")
    }
}

/// Check one label or title.
fn check_text(what: &str, text: &str) -> Result<String, CustomError> {
    let value = text.trim();
    let bad = || CustomError::BadText {
        what: what.to_owned(),
    };
    if value.is_empty() || value.chars().count() > MAX_LABEL_CHARS {
        return Err(bad());
    }
    if SingleLine::new(MAX_LABEL_CHARS + 1)
        .sanitize(value)
        .as_str()
        != value
    {
        return Err(bad());
    }
    Ok(value.to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Label {
    /// A string-table key.
    Key(&'static str),
    /// The sysop's own text.
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DTarget {
    /// One of the tree's own items, with its action and permission.
    Own(Target, Option<Permission>),
    Group(String),
    Page(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DItem {
    hotkey: char,
    label: Label,
    target: DTarget,
    /// The gate of the built-in menu the item came from, once it is moved out of it.
    also: Option<Permission>,
}

impl DItem {
    /// The menu id this item links to, if it is a link.
    fn link(&self) -> Option<&str> {
        match &self.target {
            DTarget::Own(Target::Menu(id), _) => Some(id),
            DTarget::Group(id) => Some(id),
            DTarget::Own(..) | DTarget::Page(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DMenu {
    id: String,
    title: Label,
    items: Vec<DItem>,
}

type Draft = Vec<DMenu>;

/// Trees built so far: the draft each came from, for reuse.
static BUILT: Mutex<Vec<(Draft, &'static MenuTree)>> = Mutex::new(Vec::new());
static STRINGS: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

/// Build the tree `spec` describes. `page_title` says whether a page exists and what it is
/// called.
///
/// # Errors
///
/// [`CustomError`] for the first rule the file breaks (see the module docs).
pub fn build(
    spec: &Spec,
    page_title: &dyn Fn(&str) -> Option<String>,
) -> Result<Built, CustomError> {
    let draft = draft(spec, page_title)?;
    let tree = intern_tree(draft)?;
    tree.validate().map_err(CustomError::Invalid)?;
    Ok(Built { tree })
}

/// The built-in tree as a draft, before the file changes it.
fn base_draft() -> Draft {
    main_tree()
        .menus
        .iter()
        .map(|menu| DMenu {
            id: menu.id.to_owned(),
            title: Label::Key(menu.title),
            items: menu.items.iter().map(own_item).collect(),
        })
        .collect()
}

fn own_item(item: &Item) -> DItem {
    DItem {
        hotkey: item.hotkey,
        label: Label::Key(item.label),
        target: DTarget::Own(item.target, item.requires),
        also: None,
    }
}

#[allow(clippy::too_many_lines)] // one pass over the rules, top to bottom
fn draft(spec: &Spec, page_title: &dyn Fn(&str) -> Option<String>) -> Result<Draft, CustomError> {
    let base = base_draft();
    let builtin: BTreeMap<String, (usize, DItem)> = main_tree()
        .menus
        .iter()
        .enumerate()
        .flat_map(|(at, menu)| menu.items.iter().map(move |item| (at, item)))
        .map(|(at, item)| (item_id(item), (at, own_item(item))))
        .collect();

    // 1. The menus the file names.
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut groups: BTreeMap<&str, &MenuSpec> = BTreeMap::new();
    for menu in &spec.menus {
        if !seen.insert(menu.id.as_str()) {
            return Err(CustomError::DuplicateMenu(clip(&menu.id)));
        }
        if menu.items.len() > MAX_ITEMS {
            return Err(CustomError::TooManyItems(clip(&menu.id)));
        }
        let is_builtin = base.iter().any(|m| m.id == menu.id);
        if !is_builtin {
            if !valid_page_id(&menu.id) {
                return Err(CustomError::BadMenuId(clip(&menu.id)));
            }
            groups.insert(menu.id.as_str(), menu);
            if groups.len() > MAX_GROUPS {
                return Err(CustomError::TooManyGroups);
            }
        }
    }

    // 2. Resolve every listed item once.
    let mut placed: BTreeSet<String> = BTreeSet::new();
    let mut pages = 0;
    // (menu id, resolved item, hidden)
    let mut listed: Vec<(&str, DItem, bool)> = Vec::new();
    for menu in &spec.menus {
        for item in &menu.items {
            if !placed.insert(item.target.clone()) {
                return Err(CustomError::DuplicateTarget(clip(&item.target)));
            }
            let unknown = || CustomError::UnknownTarget {
                menu: clip(&menu.id),
                target: clip(&item.target),
            };
            let label_text = item
                .label
                .as_deref()
                .map(|t| check_text("a label", t))
                .transpose()?;
            let resolved = if let Some(id) = item.target.strip_prefix("page:") {
                pages += 1;
                if pages > MAX_PAGES {
                    return Err(CustomError::TooManyPages);
                }
                if !valid_page_id(id) {
                    return Err(unknown());
                }
                let title = page_title(id).ok_or_else(|| CustomError::UnknownPage(clip(id)))?;
                let key = need_key(menu, item)?;
                let label = label_text.map_or_else(|| check_text("a page title", &title), Ok)?;
                DItem {
                    hotkey: key,
                    label: Label::Text(label),
                    target: DTarget::Page(id.to_owned()),
                    also: None,
                }
            } else if let Some(id) = item.target.strip_prefix("group:") {
                let group = groups.get(id).ok_or_else(unknown)?;
                let key = need_key(menu, item)?;
                let label = match (label_text, &group.title) {
                    (Some(label), _) => label,
                    (None, Some(title)) => check_text("a group title", title)?,
                    (None, None) => {
                        return Err(CustomError::MissingLabel {
                            menu: clip(&menu.id),
                            target: clip(&item.target),
                        });
                    }
                };
                DItem {
                    hotkey: key,
                    label: Label::Text(label),
                    target: DTarget::Group(id.to_owned()),
                    also: None,
                }
            } else {
                let (home, own) = builtin.get(item.target.as_str()).ok_or_else(unknown)?;
                let mut own = own.clone();
                if base.get(*home).is_some_and(|m| m.id != menu.id) {
                    own.also = home_gate(*home);
                }
                if let Some(key) = item.key {
                    own.hotkey = key;
                }
                if let Some(label) = label_text {
                    own.label = Label::Text(label);
                }
                own
            };
            listed.push((menu.id.as_str(), resolved, item.hidden));
        }
    }
    // A group nobody links is a typo: say so.
    for id in groups.keys() {
        if !placed.contains(&format!("group:{id}")) {
            return Err(CustomError::UnlinkedGroup(clip(id)));
        }
    }

    // 3. Assemble. Built-in menus: listed items, then their own unplaced items in default order.
    let mut menus: Draft = Vec::new();
    for menu in &base {
        let mut title = menu.title.clone();
        if let Some(over) = spec.menus.iter().find(|m| m.id == menu.id)
            && let Some(text) = &over.title
        {
            title = Label::Text(check_text("a menu title", text)?);
        }
        let mut items: Vec<DItem> = listed
            .iter()
            .filter(|(id, _, hidden)| *id == menu.id && !hidden)
            .map(|(_, item, _)| item.clone())
            .collect();
        for (id, (home, own)) in &builtin {
            if base.get(*home).is_some_and(|m| m.id == menu.id) && !placed.contains(id) {
                items.push(own.clone());
            }
        }
        // Default order for the unplaced tail: the registry iterates by id, so restore tree order.
        let tail_from = listed
            .iter()
            .filter(|(id, _, hidden)| *id == menu.id && !hidden)
            .count();
        let (head, tail) = items.split_at(tail_from.min(items.len()));
        let mut tail = tail.to_vec();
        if let Some(original) = main_tree().menu(&menu.id) {
            let position = |item: &DItem| {
                original
                    .items
                    .iter()
                    .position(|o| o.label == label_key(item))
            };
            tail.sort_by_key(position);
        }
        let mut ordered = head.to_vec();
        ordered.extend(tail);
        menus.push(DMenu {
            id: menu.id.clone(),
            title,
            items: ordered,
        });
    }
    for (id, group) in &groups {
        let title = match &group.title {
            Some(text) => Label::Text(check_text("a group title", text)?),
            None => Label::Text(check_text(
                "a group title",
                listed
                    .iter()
                    .find(|(_, item, _)| matches!(&item.target, DTarget::Group(g) if g == id))
                    .and_then(|(_, item, _)| match &item.label {
                        Label::Text(text) => Some(text.as_str()),
                        Label::Key(_) => None,
                    })
                    .unwrap_or(""),
            )?),
        };
        menus.push(DMenu {
            id: (*id).to_owned(),
            title,
            items: listed
                .iter()
                .filter(|(menu_id, _, hidden)| menu_id == id && !hidden)
                .map(|(_, item, _)| item.clone())
                .collect(),
        });
    }

    prune_and_check(&mut menus, &listed)?;
    Ok(menus)
}

/// Fail if menus nest deeper than [`MAX_NESTING`] below `id`.
fn depth(menus: &Draft, id: &str, level: usize) -> Result<(), CustomError> {
    if level > MAX_NESTING {
        return Err(CustomError::TooDeep);
    }
    if let Some(menu) = menus.iter().find(|m| m.id == id) {
        for item in &menu.items {
            if let Some(target) = item.link() {
                depth(menus, target, level + 1)?;
            }
        }
    }
    Ok(())
}

/// The string-table key a built-in item's label came from, or an empty string for text.
fn label_key(item: &DItem) -> &str {
    match &item.label {
        Label::Key(key) => key,
        Label::Text(_) => "",
    }
}

/// The permission that gates the built-in menu at `home` (the `requires` of the main-menu item
/// that links to it), which an item moved out of that menu keeps.
fn home_gate(home: usize) -> Option<Permission> {
    let id = main_tree().menus.get(home)?.id;
    main_tree()
        .menus
        .iter()
        .flat_map(|menu| menu.items.iter())
        .find(|item| item.target == Target::Menu(id))
        .and_then(|link| link.requires)
}

fn need_key(menu: &MenuSpec, item: &ItemSpec) -> Result<char, CustomError> {
    item.key.ok_or_else(|| CustomError::MissingKey {
        menu: clip(&menu.id),
        target: clip(&item.target),
    })
}

/// Drop menus whose only links were hidden, then check every rule on what remains.
fn prune_and_check(menus: &mut Draft, listed: &[(&str, DItem, bool)]) -> Result<(), CustomError> {
    let links = |menus: &Draft, id: &str| -> Vec<String> {
        menus
            .iter()
            .find(|m| m.id == id)
            .map(|m| {
                m.items
                    .iter()
                    .filter_map(|i| i.link().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    // Menus reachable from the root.
    let mut reached: Vec<String> = vec!["main".to_owned()];
    let mut next = 0;
    while let Some(id) = reached.get(next).cloned() {
        next += 1;
        for target in links(menus, &id) {
            if !reached.contains(&target) {
                reached.push(target);
            }
        }
    }
    // Menus whose link the file hid, and everything below them, may disappear. A hidden link
    // can be a built-in submenu item (its target is `Own(Menu)`) or a group.
    let mut doomed: Vec<String> = Vec::new();
    for (_, item, hidden) in listed {
        if *hidden && let Some(target) = item.link() {
            doomed.push(target.to_owned());
        }
    }
    let mut at = 0;
    while let Some(id) = doomed.get(at).cloned() {
        at += 1;
        for target in links(menus, &id) {
            if !doomed.contains(&target) {
                doomed.push(target);
            }
        }
    }
    for menu in menus.iter() {
        if !reached.contains(&menu.id) && !doomed.contains(&menu.id) {
            return Err(CustomError::Unreachable(clip(&menu.id)));
        }
    }
    menus.retain(|menu| reached.contains(&menu.id));

    // Nesting.
    depth(menus, "main", 0)?;

    // Per-menu rules.
    for menu in menus.iter() {
        if menu.items.is_empty() {
            return Err(CustomError::EmptyMenu(clip(&menu.id)));
        }
        if menu.items.len() > MAX_ITEMS {
            return Err(CustomError::TooManyItems(clip(&menu.id)));
        }
        let mut keys: Vec<char> = Vec::new();
        for item in &menu.items {
            if !item.hotkey.is_ascii_alphanumeric() {
                return Err(CustomError::BadHotkey {
                    menu: clip(&menu.id),
                    key: item.hotkey,
                });
            }
            let folded = item.hotkey.to_ascii_lowercase();
            if keys.contains(&folded) {
                return Err(CustomError::DuplicateHotkey {
                    menu: clip(&menu.id),
                    key: item.hotkey,
                });
            }
            keys.push(folded);
        }
    }
    Ok(())
}

fn intern(text: &str) -> Result<&'static str, CustomError> {
    if text.len() > MAX_STRING_BYTES {
        return Err(CustomError::TooManyStrings);
    }
    let mut set = STRINGS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = set.get(text) {
        return Ok(found);
    }
    if set.len() >= MAX_STRINGS {
        return Err(CustomError::TooManyStrings);
    }
    let leaked: &'static str = Box::leak(text.to_owned().into_boxed_str());
    set.insert(leaked);
    Ok(leaked)
}

fn intern_label(label: &Label) -> Result<&'static str, CustomError> {
    match label {
        Label::Key(key) => Ok(key),
        Label::Text(text) => intern(text),
    }
}

/// Turn a checked draft into `'static` data, reusing an identical tree built earlier.
fn intern_tree(draft: Draft) -> Result<&'static MenuTree, CustomError> {
    let mut built = BUILT.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, tree)) = built.iter().find(|(old, _)| *old == draft) {
        return Ok(tree);
    }
    if built.len() >= MAX_LOADS {
        return Err(CustomError::TooManyLoads);
    }
    let mut menus: Vec<Menu> = Vec::with_capacity(draft.len());
    for menu in &draft {
        let mut items: Vec<Item> = Vec::with_capacity(menu.items.len());
        for item in &menu.items {
            let (target, requires) = match &item.target {
                DTarget::Own(target, requires) => (*target, *requires),
                DTarget::Group(id) => (Target::Menu(intern(id)?), None),
                DTarget::Page(id) => (Target::Page(intern(id)?), None),
            };
            items.push(Item {
                hotkey: item.hotkey,
                label: intern_label(&item.label)?,
                target,
                requires,
                also: item.also,
            });
        }
        menus.push(Menu {
            id: intern(&menu.id)?,
            title: intern_label(&menu.title)?,
            items: Box::leak(items.into_boxed_slice()),
        });
    }
    let tree: &'static MenuTree = Box::leak(Box::new(MenuTree {
        root: "main",
        menus: Box::leak(menus.into_boxed_slice()),
    }));
    built.push((draft, tree));
    Ok(tree)
}
