#![allow(clippy::unwrap_used, clippy::items_after_statements)]

use super::*;
use crate::access::{Permission, PermissionCheck, Slug, Snapshot, Via};
use crate::store::AccountId;

/// A session holding the `member` stand-in: everything that exists except `members.approve`.
fn member() -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::MEMBER,
        10,
        &Via::Account,
        Permission::ALL
            .iter()
            .copied()
            .filter(|p| *p != Permission::MembersApprove),
    )
}

/// A session holding every permission.
fn sysop() -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::SYSOP,
        100,
        &Via::Account,
        Permission::ALL.iter().copied(),
    )
}

/// A session holding nothing, for filtering tests.
struct Nothing;
impl PermissionCheck for Nothing {
    fn allows(&self, _: Permission) -> bool {
        false
    }
}

/// A session holding exactly the permissions listed.
struct Only(&'static [Permission]);
impl PermissionCheck for Only {
    fn allows(&self, permission: Permission) -> bool {
        self.0.contains(&permission)
    }
}

const fn leaf(hotkey: char) -> Item {
    Item {
        hotkey,
        label: "label",
        target: Target::Placeholder("note"),
        requires: None,
    }
}

fn tree(root: &'static str, menus: &'static [Menu]) -> MenuTree {
    MenuTree { root, menus }
}

// ---- validation ----

#[test]
fn the_shipped_tree_is_valid() {
    main_tree().validate().unwrap();
}

#[test]
fn duplicate_hotkeys_are_rejected_ignoring_case() {
    static ITEMS: [Item; 2] = [leaf('a'), leaf('A')];
    static MENUS: [Menu; 1] = [Menu {
        id: "m",
        title: "t",
        items: &ITEMS,
    }];
    assert_eq!(
        tree("m", &MENUS).validate(),
        Err(MenuError::DuplicateHotkey {
            menu: "m",
            hotkey: 'A'
        })
    );
}

#[test]
fn the_help_key_and_non_alphanumeric_hotkeys_are_rejected() {
    for bad in [HELP_KEY, ' ', '\u{1b}', 'é', '-'] {
        let items: &'static [Item] = Box::leak(Box::new([leaf(bad)]));
        let menus: &'static [Menu] = Box::leak(Box::new([Menu {
            id: "m",
            title: "t",
            items,
        }]));
        assert_eq!(
            tree("m", menus).validate(),
            Err(MenuError::BadHotkey {
                menu: "m",
                hotkey: bad
            }),
            "{bad:?}"
        );
    }
}

#[test]
fn a_dangling_target_is_rejected() {
    static ITEMS: [Item; 1] = [Item {
        hotkey: 'x',
        label: "l",
        target: Target::Menu("nowhere"),
        requires: None,
    }];
    static MENUS: [Menu; 1] = [Menu {
        id: "m",
        title: "t",
        items: &ITEMS,
    }];
    assert_eq!(
        tree("m", &MENUS).validate(),
        Err(MenuError::DanglingTarget {
            menu: "m",
            hotkey: 'x',
            target: "nowhere"
        })
    );
}

#[test]
fn missing_root_duplicate_empty_and_unreachable_menus_are_rejected() {
    static ITEMS: [Item; 1] = [leaf('a')];
    static ONE: [Menu; 1] = [Menu {
        id: "m",
        title: "t",
        items: &ITEMS,
    }];
    assert_eq!(tree("x", &ONE).validate(), Err(MenuError::MissingRoot("x")));

    static TWICE: [Menu; 2] = [
        Menu {
            id: "m",
            title: "t",
            items: &ITEMS,
        },
        Menu {
            id: "m",
            title: "t",
            items: &ITEMS,
        },
    ];
    assert_eq!(
        tree("m", &TWICE).validate(),
        Err(MenuError::DuplicateMenu("m"))
    );

    static EMPTY: [Menu; 1] = [Menu {
        id: "m",
        title: "t",
        items: &[],
    }];
    assert_eq!(tree("m", &EMPTY).validate(), Err(MenuError::EmptyMenu("m")));

    static ORPHAN: [Menu; 2] = [
        Menu {
            id: "m",
            title: "t",
            items: &ITEMS,
        },
        Menu {
            id: "lost",
            title: "t",
            items: &ITEMS,
        },
    ];
    assert_eq!(
        tree("m", &ORPHAN).validate(),
        Err(MenuError::Unreachable("lost"))
    );
}

#[test]
fn an_action_item_may_not_carry_its_own_permission() {
    static ITEMS: [Item; 1] = [Item {
        hotkey: 'l',
        label: "l",
        target: Target::Action(Action::LogOff),
        requires: Some(Permission::MailRead),
    }];
    static MENUS: [Menu; 1] = [Menu {
        id: "m",
        title: "t",
        items: &ITEMS,
    }];
    assert_eq!(
        tree("m", &MENUS).validate(),
        Err(MenuError::ActionWithRequires {
            menu: "m",
            hotkey: 'l'
        })
    );
}

#[test]
fn every_action_in_the_tree_has_its_permission_except_log_off() {
    for menu in main_tree().menus {
        for item in menu.items {
            if let Target::Action(action) = item.target {
                assert_eq!(item.permission(), action.permission());
                assert_eq!(
                    action.permission().is_none(),
                    action == Action::LogOff,
                    "only log off is permission-free"
                );
            }
        }
    }
}

// ---- access hook ----

fn hotkeys(session: &MenuSession, access: &dyn PermissionCheck) -> String {
    session
        .page(access)
        .items
        .iter()
        .map(|i| i.hotkey)
        .collect()
}

#[test]
fn the_access_hook_filters_what_a_session_sees() {
    let session = MenuSession::new(main_tree());
    assert_eq!(
        hotkeys(&session, &Nothing),
        "l",
        "log off needs no permission"
    );
    let member = member();
    assert_eq!(hotkeys(&session, &member), "fmauisl");
    let sysop = sysop();
    assert_eq!(hotkeys(&session, &sysop), "fmauisol");
    assert_eq!(
        hotkeys(&session, &Only(&[Permission::AccountEditSettings])),
        "sl"
    );
}

#[test]
fn a_hidden_item_cannot_be_picked_by_its_hotkey() {
    let member = member();
    let mut session = MenuSession::new(main_tree());
    // `o` opens the sysop menu for a sysop only.
    assert_eq!(
        session.press(&member, MenuKey::Char('o')),
        MenuOutcome::Stay
    );
    assert_eq!(session.current(), "main");
    // `s` is settings only with the permission.
    assert_eq!(
        session.press(&Nothing, MenuKey::Char('s')),
        MenuOutcome::Stay
    );
    assert_eq!(
        session.press(&member, MenuKey::Char('s')),
        MenuOutcome::Action(Action::TerminalSettings)
    );
    assert_eq!(
        session.press(&Nothing, MenuKey::Char('l')),
        MenuOutcome::Action(Action::LogOff)
    );
}

#[test]
fn a_permission_lost_mid_session_hides_the_menu_you_are_in() {
    let sysop = sysop();
    let mut session = MenuSession::new(main_tree());
    session.press(&sysop, MenuKey::Char('o'));
    assert_eq!(session.current(), "sysop");
    let member = member();
    assert!(session.page(&member).items.is_empty(), "nothing is offered");
    assert_eq!(
        session.press(&member, MenuKey::Char('q')),
        MenuOutcome::Stay
    );
    assert_eq!(session.notice_for_tests(), None);
}

// ---- navigation and universal keys ----

#[test]
fn hotkeys_ignore_case_and_menus_nest_and_unwind() {
    let sysop = sysop();
    let mut session = MenuSession::new(main_tree());
    session.press(&sysop, MenuKey::Char('O'));
    assert_eq!(session.current(), "sysop");
    assert_eq!(session.page(&sysop).depth, 1);
    session.press(&sysop, MenuKey::Back);
    assert_eq!(session.current(), "main");
}

#[test]
fn a_placeholder_says_so_and_the_notice_clears_on_the_next_key() {
    let member = member();
    let mut session = MenuSession::new(main_tree());
    session.press(&member, MenuKey::Char('f'));
    assert_eq!(session.page(&member).notice, Some("placeholder.forums"));
    session.press(&member, MenuKey::Char('x'));
    assert_eq!(session.page(&member).notice, None);
}

/// Every menu of the tree, as the ids a sysop can reach by walking the hotkeys.
fn paths_to_every_menu(access: &dyn PermissionCheck) -> Vec<(&'static str, Vec<char>)> {
    let mut found: Vec<(&'static str, Vec<char>)> = vec![(main_tree().root, Vec::new())];
    let mut next = 0;
    while let Some((id, path)) = found.get(next).cloned() {
        next += 1;
        let menu = main_tree().menu(id).unwrap();
        for item in MenuTree::visible_items(menu, access) {
            if let Target::Menu(target) = item.target
                && !found.iter().any(|(seen, _)| *seen == target)
            {
                let mut longer = path.clone();
                longer.push(item.hotkey);
                found.push((target, longer));
            }
        }
    }
    found
}

#[test]
fn universal_keys_work_identically_in_every_menu() {
    let sysop = sysop();
    let paths = paths_to_every_menu(&sysop);
    assert_eq!(
        paths.len(),
        main_tree().menus.len(),
        "every menu is reachable"
    );
    for (id, path) in paths {
        let open = || {
            let mut session = MenuSession::new(main_tree());
            for key in &path {
                session.press(&sysop, MenuKey::Char(*key));
            }
            assert_eq!(session.current(), id);
            session
        };
        // Help opens, shows, and any key closes it without doing what the key would do.
        let mut session = open();
        assert_eq!(
            session.press(&sysop, MenuKey::Char(HELP_KEY)),
            MenuOutcome::Stay
        );
        assert!(session.page(&sysop).help, "{id}");
        assert_eq!(session.press(&sysop, MenuKey::Char('l')), MenuOutcome::Stay);
        assert!(!session.page(&sysop).help, "{id}: a key closes help only");
        assert_eq!(session.current(), id);
        // Back goes up one level, or stays at the root.
        let mut session = open();
        session.press(&sysop, MenuKey::Back);
        let expected = if path.is_empty() { id } else { "main" };
        assert_eq!(session.current(), expected, "{id}");
        // No item of any menu can take the help key.
        let menu = main_tree().menu(id).unwrap();
        assert!(menu.items.iter().all(|item| item.hotkey != HELP_KEY));
    }
}

#[test]
fn the_stack_is_bounded_even_if_a_tree_loops() {
    static ITEMS: [Item; 1] = [Item {
        hotkey: 'a',
        label: "l",
        target: Target::Menu("m"),
        requires: None,
    }];
    static MENUS: [Menu; 1] = [Menu {
        id: "m",
        title: "t",
        items: &ITEMS,
    }];
    static LOOP: MenuTree = MenuTree {
        root: "m",
        menus: &MENUS,
    };
    LOOP.validate().unwrap();
    let mut session = MenuSession::new(&LOOP);
    for _ in 0..100 {
        session.press(&Nothing, MenuKey::Char('a'));
    }
    assert_eq!(session.page(&Nothing).depth, MAX_DEPTH - 1);
}

#[test]
fn text_keys_cover_titles_labels_and_placeholder_notes() {
    let keys: Vec<_> = main_tree().text_keys().collect();
    for expected in [
        "menu.main.title",
        "menu.main.item.settings",
        "placeholder.forums",
        "menu.sysop.title",
    ] {
        assert!(keys.contains(&expected), "{expected}");
    }
}

#[test]
fn the_invite_actions_need_their_permissions_and_a_hidden_one_cannot_be_picked() {
    use super::Action;
    assert_eq!(
        Action::InviteCreate.permission(),
        Some(Permission::InvitesCreate)
    );
    assert_eq!(
        Action::InviteList.permission(),
        Some(Permission::InvitesCreate)
    );
    assert_eq!(
        Action::InviteTree.permission(),
        Some(Permission::InvitesViewTree)
    );

    // A member holds `invites.create` but not `invites.view_tree` (trusted and up): the
    // submenu opens, and the tree item is neither listed nor pickable.
    let member = Only(&[Permission::InvitesCreate]);
    let mut session = MenuSession::new(main_tree());
    assert_eq!(
        session.press(&member, MenuKey::Char('i')),
        MenuOutcome::Stay
    );
    assert_eq!(session.current(), "invites");
    assert_eq!(hotkeys(&session, &member), "cy");
    assert_eq!(
        session.press(&member, MenuKey::Char('t')),
        MenuOutcome::Stay,
        "a hidden item does nothing"
    );
    assert_eq!(
        session.press(&member, MenuKey::Char('c')),
        MenuOutcome::Action(Action::InviteCreate)
    );
    let trusted = Only(&[Permission::InvitesCreate, Permission::InvitesViewTree]);
    assert_eq!(hotkeys(&session, &trusted), "cyt");
    assert_eq!(
        session.press(&trusted, MenuKey::Char('t')),
        MenuOutcome::Action(Action::InviteTree)
    );
    // Without `invites.create` the submenu itself is hidden from the main menu.
    let mut other = MenuSession::new(main_tree());
    assert_eq!(other.press(&Nothing, MenuKey::Char('i')), MenuOutcome::Stay);
    assert_eq!(other.current(), "main");
}
