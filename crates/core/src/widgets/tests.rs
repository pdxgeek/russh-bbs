//! Placement, locks and ids without a store; what the widgets read from the store (and that a
//! viewer who cannot reach a board sees nothing from it) is in `russh-bbs-store`'s shared suite.
#![allow(clippy::unwrap_used, clippy::panic, clippy::assert_is_empty)]

use super::*;
use crate::access::{Permission, Slug, Snapshot, Via};
use crate::credits::{LedgerEntry, LedgerId};
use crate::store::{AccountId, UnixSeconds};
use crate::terminal_settings::LayoutId;

fn member() -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::parse("member").unwrap(),
        10,
        &Via::Account,
        [
            Permission::MembersView,
            Permission::CommunityView,
            Permission::MailRead,
            Permission::AccountEditSettings,
        ],
    )
}

fn staff() -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(2)),
        &Slug::parse("sysop").unwrap(),
        90,
        &Via::Account,
        [Permission::SessionsView, Permission::MembersView],
    )
}

fn free() -> Rules {
    Rules::default()
}

#[test]
fn ids_round_trip_and_are_unique() {
    let mut seen = std::collections::BTreeSet::new();
    for id in WidgetId::ALL {
        assert_eq!(WidgetId::parse(id.as_str()), Some(id));
        assert!(seen.insert(id.as_str()));
    }
    assert_eq!(WidgetId::parse("nope"), None);
    assert!(WidgetId::ALL.len() >= 10, "the launch set is at least ten");
}

#[test]
fn a_waiting_room_key_reaches_no_widget() {
    let waiting = Snapshot::empty();
    for id in WidgetId::ALL {
        assert!(
            !id.reachable_by(&waiting),
            "{id:?} leaked to the waiting room"
        );
    }
    assert!(Rules::default().offered(&waiting).is_empty());
    for layout in LayoutId::ALL {
        assert!(free().resolve(layout, &Choice::new(), &waiting).is_empty());
    }
}

#[test]
fn reach_follows_the_permission_of_the_screen() {
    let m = member();
    assert!(WidgetId::Mail.reachable_by(&m));
    assert!(WidgetId::Callers.reachable_by(&m));
    assert!(WidgetId::Online.reachable_by(&m));
    assert!(!WidgetId::Status.reachable_by(&m), "status is for staff");
    assert!(!WidgetId::Chat.reachable_by(&m));
    assert!(!WidgetId::NewPosts.reachable_by(&m));
    assert!(WidgetId::Status.reachable_by(&staff()));
    assert!(!WidgetId::Mail.reachable_by(&staff()));
}

#[test]
fn the_classic_family_draws_no_widgets() {
    for layout in [LayoutId::Classic, LayoutId::Lightbar, LayoutId::Compact] {
        assert!(
            free()
                .resolve_with(layout, &Choice::new(), |_| true)
                .is_empty()
        );
        assert_eq!(
            Choice::new().toggle(layout, WidgetId::Clock, &free(), true),
            Err(EditError::NoWidgets)
        );
    }
}

#[test]
fn defaults_apply_until_the_member_chooses() {
    let rules = free();
    let all = |_| true;
    let dash = rules.resolve_with(LayoutId::Dashboard, &Choice::new(), all);
    assert_eq!(dash, builtin_default(LayoutId::Dashboard));
    let chosen = Choice::from_lists([(LayoutId::Dashboard, vec![WidgetId::Clock, WidgetId::Mail])]);
    assert_eq!(
        rules.resolve_with(LayoutId::Dashboard, &chosen, all),
        vec![WidgetId::Clock, WidgetId::Mail]
    );
    // A choice for one layout does not touch another.
    assert_eq!(
        rules.resolve_with(LayoutId::Wide, &chosen, all),
        builtin_default(LayoutId::Wide)
    );
    // The member may switch everything off, which is not the same as no choice.
    let none = Choice::from_lists([(LayoutId::Dashboard, vec![])]);
    assert!(
        rules
            .resolve_with(LayoutId::Dashboard, &none, all)
            .is_empty()
    );
    assert_eq!(
        none.reset(LayoutId::Dashboard).get(LayoutId::Dashboard),
        None
    );
}

#[test]
fn the_sysops_defaults_replace_the_built_in_ones() {
    let rules = Rules::new(
        [],
        [],
        [],
        [(
            LayoutId::Wide,
            vec![WidgetId::Motd, WidgetId::Clock, WidgetId::Motd],
        )],
    )
    .unwrap();
    assert_eq!(
        rules.resolve_with(LayoutId::Wide, &Choice::new(), |_| true),
        vec![WidgetId::Motd, WidgetId::Clock],
        "a duplicate is dropped"
    );
    assert_eq!(
        rules.default_for(LayoutId::Dashboard),
        builtin_default(LayoutId::Dashboard)
    );
}

#[test]
fn locks_win_over_what_a_member_stored() {
    let rules = Rules::new([WidgetId::Quote], [WidgetId::Motd], [WidgetId::Wall], []).unwrap();
    let stored = Choice::from_lists([(
        LayoutId::Dashboard,
        vec![WidgetId::Wall, WidgetId::Quote, WidgetId::Clock],
    )]);
    let shown = rules.resolve_with(LayoutId::Dashboard, &stored, |_| true);
    // Off and hidden are removed, the locked-on widget is added though never listed.
    assert_eq!(shown, vec![WidgetId::Clock, WidgetId::Motd]);
    // Hidden and locked off are not offered as free; locked on is offered as such.
    let offered = rules.offered(&member());
    assert!(!offered.iter().any(|(id, _)| *id == WidgetId::Quote));
    assert!(offered.contains(&(WidgetId::Motd, Lock::On)));
    assert!(offered.contains(&(WidgetId::Wall, Lock::Off)));
}

#[test]
fn a_member_cannot_edit_past_a_lock() {
    let rules = Rules::new([WidgetId::Quote], [WidgetId::Motd], [WidgetId::Wall], []).unwrap();
    let c = Choice::new();
    for id in [WidgetId::Quote, WidgetId::Motd, WidgetId::Wall] {
        assert_eq!(
            c.toggle(LayoutId::Wide, id, &rules, true),
            Err(EditError::Locked),
            "{id:?}"
        );
    }
    assert_eq!(
        c.toggle(LayoutId::Wide, WidgetId::Clock, &rules, false),
        Err(EditError::Locked),
        "a widget the member cannot reach cannot be switched on"
    );
}

#[test]
fn toggle_and_shift_edit_a_copy_and_stay_bounded() {
    let rules = free();
    let c = Choice::new();
    let off = c
        .toggle(LayoutId::Focus, WidgetId::Clock, &rules, true)
        .unwrap();
    assert!(!off.get(LayoutId::Focus).unwrap().contains(&WidgetId::Clock));
    assert!(
        c.get(LayoutId::Focus).is_none(),
        "the original is untouched"
    );
    let on = off
        .toggle(LayoutId::Focus, WidgetId::Clock, &rules, true)
        .unwrap();
    assert_eq!(
        on.get(LayoutId::Focus).unwrap().last(),
        Some(&WidgetId::Clock)
    );
    // Moving.
    let shown = vec![WidgetId::Clock, WidgetId::Mail, WidgetId::Online];
    let down = c
        .shift(LayoutId::Focus, WidgetId::Clock, 1, &shown)
        .unwrap();
    assert_eq!(
        down.get(LayoutId::Focus).unwrap(),
        [WidgetId::Mail, WidgetId::Clock, WidgetId::Online]
    );
    let top = c
        .shift(LayoutId::Focus, WidgetId::Clock, -1, &shown)
        .unwrap();
    assert_eq!(
        top.get(LayoutId::Focus).unwrap(),
        shown,
        "the first cannot go higher"
    );
    assert_eq!(
        c.shift(LayoutId::Focus, WidgetId::Doors, 1, &shown),
        Err(EditError::NotListed)
    );
    // A stored list is bounded.
    let long = Choice::from_lists([(LayoutId::Wide, WidgetId::ALL.to_vec())]);
    assert_eq!(long.get(LayoutId::Wide).unwrap().len(), MAX_PER_LAYOUT);
    assert_eq!(
        long.toggle(LayoutId::Wide, WidgetId::Quote, &rules, true),
        Err(EditError::Full)
    );
}

#[test]
fn conflicting_rules_are_refused() {
    assert!(matches!(
        Rules::new([WidgetId::Clock], [WidgetId::Clock], [], []),
        Err(RulesError::Conflict("clock", _, _))
    ));
    assert_eq!(
        Rules::new([], [], [], [(LayoutId::Wide, WidgetId::ALL.to_vec())]),
        Err(RulesError::TooMany)
    );
}

fn entry(id: i64, at: i64, delta: i64) -> LedgerEntry {
    LedgerEntry {
        id: LedgerId::from_raw(id),
        account: AccountId::from_raw(1),
        at: UnixSeconds::from_raw(at),
        delta,
        reason: Slug::parse("test").unwrap(),
    }
}

#[test]
fn the_balance_series_walks_the_ledger_back() {
    let day = 86_400;
    let now = UnixSeconds::from_raw(100 * day + 50);
    // 5 now: +3 today, -2 two days ago, +4 five days ago.
    let rows = [
        entry(3, 100 * day + 10, 3),
        entry(2, 98 * day, -2),
        entry(1, 95 * day, 4),
    ];
    let s = balance_series(5, &rows, now);
    assert_eq!(s.len(), SERIES_DAYS);
    assert_eq!(s[SERIES_DAYS - 1], 5, "today ends at the balance");
    assert_eq!(s[SERIES_DAYS - 2], 2, "yesterday, before today's +3");
    assert_eq!(s[SERIES_DAYS - 3], 2, "two days ago, after the -2");
    assert_eq!(s[SERIES_DAYS - 4], 4, "before the -2");
    assert_eq!(s[SERIES_DAYS - 6], 4);
    assert_eq!(s[SERIES_DAYS - 7], 0, "before the first row");
    assert_eq!(balance_series(0, &[], now), vec![0; SERIES_DAYS]);
}
