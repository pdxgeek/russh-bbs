//! Which widgets a member sees in a layout, and in what order: the sysop's limits and the
//! member's choice, in one pure function (UI-10).
//!
//! * The **sysop** ([`Rules`]) can *hide* a widget (it does not exist on this board), lock it
//!   *on* (every member sees it) or *off* (nobody can switch it on), and set the default list for
//!   each layout.
//! * The **member** ([`Choice`]) has an ordered list per layout, or none (follow the default).
//! * [`Rules::resolve`] is the only place the two meet. It is applied when a list is *read*, not
//!   when it is saved, so a lock the sysop adds later wins over what a member stored earlier.
//!
//! What a member *may reach* is a separate gate ([`WidgetId::reachable_by`]): a widget the
//! viewer's permissions do not cover is never offered and never drawn.

use std::collections::{BTreeMap, BTreeSet};

use crate::access::Snapshot;
use crate::terminal_settings::LayoutId;

use super::id::WidgetId;

/// Most widgets one layout holds, so a stored list is bounded.
pub const MAX_PER_LAYOUT: usize = 12;

/// The widgets a layout shows when neither the sysop nor the member chose.
#[must_use]
pub fn builtin_default(layout: LayoutId) -> Vec<WidgetId> {
    use WidgetId::{
        Activity, Callers, Chat, Clock, Credits, Identity, Mail, Motd, NewPosts, News, Oneliner,
        Online, Poll, Status,
    };
    match layout {
        LayoutId::Dashboard => vec![
            Identity, Clock, Online, Callers, NewPosts, Mail, Activity, Oneliner, Poll, Credits,
        ],
        LayoutId::Wide => vec![
            Identity, Clock, Online, NewPosts, Activity, Mail, Callers, News, Motd, Status,
        ],
        LayoutId::Focus => vec![Identity, Clock, Mail, NewPosts, Online, Motd],
        LayoutId::Split => vec![Chat, Online, Mail, NewPosts],
        // The classic family draws no widget panes.
        LayoutId::Classic | LayoutId::Lightbar | LayoutId::Compact => Vec::new(),
    }
}

/// How the sysop limits one widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lock {
    /// The member decides.
    Free,
    /// Every member sees it.
    On,
    /// Nobody can switch it on.
    Off,
    /// It does not exist on this board.
    Hidden,
}

/// The sysop's limits and defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rules {
    hidden: BTreeSet<WidgetId>,
    on: BTreeSet<WidgetId>,
    off: BTreeSet<WidgetId>,
    defaults: BTreeMap<LayoutId, Vec<WidgetId>>,
}

/// Why a set of rules was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RulesError {
    /// A widget is in two of hidden, locked on and locked off.
    #[error("widget `{0}` is both {1} and {2}")]
    Conflict(&'static str, &'static str, &'static str),
    /// A default list is longer than a layout holds.
    #[error("a default list holds at most {MAX_PER_LAYOUT} widgets")]
    TooMany,
}

impl Rules {
    /// Rules from the sysop's lists. A widget may be in only one of `hidden`, `on` and `off`.
    ///
    /// # Errors
    /// [`RulesError`] for a widget that is in two lists, or a default list that is too long.
    pub fn new(
        hidden: impl IntoIterator<Item = WidgetId>,
        on: impl IntoIterator<Item = WidgetId>,
        off: impl IntoIterator<Item = WidgetId>,
        defaults: impl IntoIterator<Item = (LayoutId, Vec<WidgetId>)>,
    ) -> Result<Self, RulesError> {
        let hidden: BTreeSet<_> = hidden.into_iter().collect();
        let on: BTreeSet<_> = on.into_iter().collect();
        let off: BTreeSet<_> = off.into_iter().collect();
        for id in WidgetId::ALL {
            let names = [
                (hidden.contains(&id), "hidden"),
                (on.contains(&id), "locked on"),
                (off.contains(&id), "locked off"),
            ];
            let mut in_lists = names.iter().filter(|(yes, _)| *yes).map(|(_, name)| *name);
            if let (Some(a), Some(b)) = (in_lists.next(), in_lists.next()) {
                return Err(RulesError::Conflict(id.as_str(), a, b));
            }
        }
        let mut kept = BTreeMap::new();
        for (layout, list) in defaults {
            if list.len() > MAX_PER_LAYOUT {
                return Err(RulesError::TooMany);
            }
            kept.insert(layout, dedupe(list));
        }
        Ok(Self {
            hidden,
            on,
            off,
            defaults: kept,
        })
    }

    /// How the sysop limits `id`.
    #[must_use]
    pub fn lock(&self, id: WidgetId) -> Lock {
        if self.hidden.contains(&id) {
            Lock::Hidden
        } else if self.on.contains(&id) {
            Lock::On
        } else if self.off.contains(&id) {
            Lock::Off
        } else {
            Lock::Free
        }
    }

    /// The default list for `layout`: the sysop's, or the built-in one.
    #[must_use]
    pub fn default_for(&self, layout: LayoutId) -> Vec<WidgetId> {
        self.defaults
            .get(&layout)
            .cloned()
            .unwrap_or_else(|| builtin_default(layout))
    }

    /// The widgets `layout` shows `access`, in order: the member's list (or the default), with
    /// hidden and locked-off widgets removed, locked-on widgets added where missing, and
    /// widgets the member cannot reach dropped. The classic family has none.
    #[must_use]
    pub fn resolve(&self, layout: LayoutId, choice: &Choice, access: &Snapshot) -> Vec<WidgetId> {
        self.resolve_with(layout, choice, |id| id.reachable_by(access))
    }

    /// [`Rules::resolve`] with the reach test given (tests, and the Widgets screen).
    #[must_use]
    pub fn resolve_with(
        &self,
        layout: LayoutId,
        choice: &Choice,
        reachable: impl Fn(WidgetId) -> bool,
    ) -> Vec<WidgetId> {
        if matches!(
            layout,
            LayoutId::Classic | LayoutId::Lightbar | LayoutId::Compact
        ) {
            return Vec::new();
        }
        let base = choice
            .get(layout)
            .map_or_else(|| self.default_for(layout), <[WidgetId]>::to_vec);
        let mut list: Vec<WidgetId> = dedupe(base)
            .into_iter()
            .filter(|id| matches!(self.lock(*id), Lock::Free | Lock::On))
            .collect();
        // A lock on wins over a member who never listed (or removed) the widget.
        for id in WidgetId::ALL {
            if self.lock(id) == Lock::On && !list.contains(&id) {
                list.push(id);
            }
        }
        list.retain(|id| reachable(*id));
        list.truncate(MAX_PER_LAYOUT);
        list
    }

    /// Every widget the member could be shown on this board, with its lock, for the Widgets
    /// screen: hidden widgets and widgets the member cannot reach are left out.
    #[must_use]
    pub fn offered(&self, access: &Snapshot) -> Vec<(WidgetId, Lock)> {
        WidgetId::ALL
            .into_iter()
            .filter(|id| self.lock(*id) != Lock::Hidden && id.reachable_by(access))
            .map(|id| (id, self.lock(id)))
            .collect()
    }
}

fn dedupe(list: Vec<WidgetId>) -> Vec<WidgetId> {
    let mut seen = BTreeSet::new();
    list.into_iter().filter(|id| seen.insert(*id)).collect()
}

/// A member's saved lists: one ordered list per layout they changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choice {
    lists: BTreeMap<LayoutId, Vec<WidgetId>>,
}

/// Why an edit was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// The sysop locked the widget on or off, or it is hidden or unreachable.
    #[error("the sysop's settings or the member's access do not allow that")]
    Locked,
    /// The widget is not in the list.
    #[error("the widget is not in the list")]
    NotListed,
    /// The layout has no widget panes.
    #[error("this layout draws no widgets")]
    NoWidgets,
    /// The list is full.
    #[error("a layout holds at most {MAX_PER_LAYOUT} widgets")]
    Full,
}

impl Choice {
    /// A member with no saved list.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The saved list for `layout`, if the member changed it.
    #[must_use]
    pub fn get(&self, layout: LayoutId) -> Option<&[WidgetId]> {
        self.lists.get(&layout).map(Vec::as_slice)
    }

    /// Build a choice from stored lists (the store, tests). Duplicates are dropped and the
    /// length is bounded.
    #[must_use]
    pub fn from_lists(lists: impl IntoIterator<Item = (LayoutId, Vec<WidgetId>)>) -> Self {
        let mut kept = BTreeMap::new();
        for (layout, list) in lists {
            let mut list = dedupe(list);
            list.truncate(MAX_PER_LAYOUT);
            kept.insert(layout, list);
        }
        Self { lists: kept }
    }

    /// The saved lists, for the store.
    pub fn lists(&self) -> impl Iterator<Item = (LayoutId, &[WidgetId])> {
        self.lists
            .iter()
            .map(|(layout, list)| (*layout, list.as_slice()))
    }

    /// Follow the default again in `layout`.
    #[must_use]
    pub fn reset(mut self, layout: LayoutId) -> Self {
        self.lists.remove(&layout);
        self
    }

    /// The list an edit starts from: what the member sees now, before reach is applied (so
    /// switching off a widget they cannot currently reach does not drop it from the saved list).
    fn start(&self, layout: LayoutId, rules: &Rules) -> Vec<WidgetId> {
        self.get(layout)
            .map_or_else(|| rules.default_for(layout), <[WidgetId]>::to_vec)
    }

    /// Switch `id` on (at the end) or off in `layout`.
    ///
    /// # Errors
    /// [`EditError::Locked`] for a widget the sysop locked, hid or the member cannot reach;
    /// [`EditError::NoWidgets`] for a layout without panes; [`EditError::Full`].
    pub fn toggle(
        &self,
        layout: LayoutId,
        id: WidgetId,
        rules: &Rules,
        reachable: bool,
    ) -> Result<Self, EditError> {
        if matches!(
            layout,
            LayoutId::Classic | LayoutId::Lightbar | LayoutId::Compact
        ) {
            return Err(EditError::NoWidgets);
        }
        if rules.lock(id) != Lock::Free || !reachable {
            return Err(EditError::Locked);
        }
        let mut list = dedupe(self.start(layout, rules));
        if let Some(at) = list.iter().position(|have| *have == id) {
            list.remove(at);
        } else {
            if list.len() >= MAX_PER_LAYOUT {
                return Err(EditError::Full);
            }
            list.push(id);
        }
        let mut next = self.clone();
        next.lists.insert(layout, list);
        Ok(next)
    }

    /// Move `id` one place up (`-1`) or down (`1`) in the list `layout` resolves to for the
    /// member (`shown`), keeping the saved list in step. Locked-on widgets can be moved.
    ///
    /// # Errors
    /// [`EditError::NotListed`] when `id` is not in `shown`; [`EditError::NoWidgets`].
    pub fn shift(
        &self,
        layout: LayoutId,
        id: WidgetId,
        by: i8,
        shown: &[WidgetId],
    ) -> Result<Self, EditError> {
        if matches!(
            layout,
            LayoutId::Classic | LayoutId::Lightbar | LayoutId::Compact
        ) {
            return Err(EditError::NoWidgets);
        }
        let mut list = dedupe(shown.to_vec());
        let at = list
            .iter()
            .position(|have| *have == id)
            .ok_or(EditError::NotListed)?;
        let to = if by < 0 {
            at.saturating_sub(1)
        } else {
            (at + 1).min(list.len() - 1)
        };
        list.swap(at, to);
        let mut next = self.clone();
        next.lists.insert(layout, list);
        Ok(next)
    }
}
