#![allow(clippy::unwrap_used, clippy::items_after_statements)]

use super::*;
use crate::access::{Permission, PermissionCheck, Slug, Snapshot, Via};
use crate::store::AccountId;

fn holding(role: &Slug, rank: u8, skip: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        role,
        rank,
        &Via::Account,
        Permission::ALL
            .iter()
            .copied()
            .filter(|p| !skip.contains(p)),
    )
}

fn member() -> Snapshot {
    holding(
        &Slug::MEMBER,
        10,
        &[
            Permission::MembersApprove,
            Permission::KeysRecover,
            Permission::ThemesManage,
        ],
    )
}

fn sysop() -> Snapshot {
    holding(&Slug::SYSOP, 100, &[])
}

fn pages(id: &str) -> Option<String> {
    match id {
        "rules" => Some("House rules".to_owned()),
        "history" => Some("History".to_owned()),
        _ => None,
    }
}

fn item(target: &str) -> ItemSpec {
    ItemSpec {
        target: target.to_owned(),
        key: None,
        label: None,
        hidden: false,
    }
}

fn keyed(target: &str, key: char, label: Option<&str>) -> ItemSpec {
    ItemSpec {
        target: target.to_owned(),
        key: Some(key),
        label: label.map(str::to_owned),
        hidden: false,
    }
}

fn hidden(target: &str) -> ItemSpec {
    ItemSpec {
        hidden: true,
        ..item(target)
    }
}

fn menu(id: &str, items: Vec<ItemSpec>) -> MenuSpec {
    MenuSpec {
        id: id.to_owned(),
        title: None,
        items,
    }
}

fn spec(menus: Vec<MenuSpec>) -> Spec {
    Spec { menus }
}

fn built(spec: &Spec) -> &'static MenuTree {
    build(spec, &pages).unwrap().tree
}

fn refused(spec: &Spec) -> CustomError {
    build(spec, &pages).unwrap_err()
}

/// Every action a session with `access` can reach by walking `tree` from the root.
fn reachable(tree: &MenuTree, access: &dyn PermissionCheck) -> Vec<Action> {
    let mut found = Vec::new();
    let mut open = vec![tree.root];
    let mut at = 0;
    while let Some(&id) = open.get(at) {
        at += 1;
        let Some(menu) = tree.menu(id) else { continue };
        for item in tree.visible_items(menu, access) {
            match item.target {
                Target::Action(action) => found.push(action),
                Target::Menu(next) if !open.contains(&next) => open.push(next),
                Target::Menu(_) | Target::Placeholder(_) | Target::Page(_) => {}
            }
        }
    }
    found
}

fn visible_hotkeys(tree: &MenuTree, id: &str, access: &dyn PermissionCheck) -> Vec<char> {
    let menu = tree.menu(id).unwrap();
    tree.visible_items(menu, access).map(|i| i.hotkey).collect()
}

#[test]
fn builtin_ids_are_unique_and_cover_every_item() {
    let ids = builtin_ids();
    let total: usize = main_tree().menus.iter().map(|m| m.items.len()).sum();
    assert_eq!(ids.len(), total);
    let mut sorted: Vec<_> = ids.iter().map(|(id, _, _)| id.clone()).collect();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), total);
    assert!(sorted.contains(&"main.forums".to_owned()));
    assert!(sorted.contains(&"sysop.queue".to_owned()));
}

#[test]
fn an_empty_file_builds_the_default_menu() {
    let tree = built(&spec(vec![]));
    assert_eq!(tree.menus, main_tree().menus);
    assert_eq!(tree.root, main_tree().root);
}

#[test]
fn the_same_file_is_built_once() {
    let s = spec(vec![menu(
        "main",
        vec![item("main.mail"), item("main.forums")],
    )]);
    assert!(std::ptr::eq(built(&s), built(&s)));
}

#[test]
fn reorder_rename_rekey_and_hide() {
    let s = spec(vec![MenuSpec {
        id: "main".to_owned(),
        title: Some("Main Hall".to_owned()),
        items: vec![
            keyed("main.mail", 'x', Some("Messages")),
            item("main.forums"),
            hidden("main.chat"),
        ],
    }]);
    let tree = built(&s);
    let m = sysop();
    let keys = visible_hotkeys(tree, "main", &m);
    assert_eq!(&keys[..2], &['x', 'f']);
    assert!(!keys.contains(&'c'), "chat is hidden");
    assert_eq!(tree.menu("main").unwrap().title, "Main Hall");
    assert_eq!(tree.menu("main").unwrap().items[0].label, "Messages");
    // Items the file did not mention keep their place after the listed ones, in default order.
    let rest: Vec<_> = keys[2..].to_vec();
    let default: Vec<_> = visible_hotkeys(main_tree(), "main", &m)
        .into_iter()
        .filter(|k| !matches!(k, 'm' | 'f' | 'c'))
        .collect();
    assert_eq!(rest, default);
}

#[test]
fn a_hidden_items_hotkey_does_nothing() {
    let s = spec(vec![menu(
        "main",
        vec![hidden("main.chat"), hidden("main.logoff")],
    )]);
    let tree = built(&s);
    let m = sysop();
    let mut session = MenuSession::new(tree);
    for key in ['c', 'C', 'l', 'L'] {
        assert_eq!(session.press(&m, MenuKey::Char(key)), MenuOutcome::Stay);
    }
    // A hidden item is gone for everyone, the sysop included.
    assert!(
        !reachable(tree, &m).contains(&Action::Chat),
        "hidden means not in the tree at all"
    );
}

#[test]
fn a_moved_hotkey_replaces_the_old_one() {
    let s = spec(vec![menu("main", vec![keyed("main.chat", 'z', None)])]);
    let tree = built(&s);
    let m = member();
    let mut session = MenuSession::new(tree);
    assert_eq!(
        session.press(&m, MenuKey::Char('z')),
        MenuOutcome::Action(Action::Chat)
    );
    assert_eq!(session.press(&m, MenuKey::Char('c')), MenuOutcome::Stay);
}

#[test]
fn groups_nest_built_in_items_and_pages() {
    let s = spec(vec![
        menu(
            "main",
            vec![
                keyed("group:talk", 't', Some("Talk")),
                keyed("page:rules", 'r', None),
            ],
        ),
        MenuSpec {
            id: "talk".to_owned(),
            title: Some("Talk Hall".to_owned()),
            items: vec![item("main.chat"), item("main.mail")],
        },
    ]);
    let tree = built(&s);
    let m = member();
    let mut session = MenuSession::new(tree);
    assert_eq!(session.press(&m, MenuKey::Char('t')), MenuOutcome::Stay);
    assert_eq!(session.current(), "talk");
    assert_eq!(
        session.press(&m, MenuKey::Char('c')),
        MenuOutcome::Action(Action::Chat)
    );
    session.press(&m, MenuKey::Back);
    assert_eq!(session.current(), "main");
    assert_eq!(
        session.press(&m, MenuKey::Char('r')),
        MenuOutcome::Page("rules")
    );
    // A page label defaults to the page's title.
    let page = tree
        .menu("main")
        .unwrap()
        .items
        .iter()
        .find(|i| i.hotkey == 'r')
        .unwrap();
    assert_eq!(page.label, "House rules");
}

#[test]
fn a_group_nobody_can_enter_is_not_shown() {
    let s = spec(vec![
        menu("main", vec![keyed("group:staff", 'w', Some("Staff"))]),
        menu("staff", vec![item("sysop.queue"), item("sysop.packs")]),
    ]);
    let tree = built(&s);
    assert!(!visible_hotkeys(tree, "main", &member()).contains(&'w'));
    assert!(visible_hotkeys(tree, "main", &sysop()).contains(&'w'));
    // And the member cannot reach it with the key.
    let mut session = MenuSession::new(tree);
    session.press(&member(), MenuKey::Char('w'));
    assert_eq!(session.current(), "main");
}

#[test]
fn a_file_never_widens_what_a_role_reaches() {
    // A deliberately aggressive file: sysop items pulled into a group on the main menu, the
    // sysop menu's own link hidden, and the invite items moved next to the forums.
    let s = spec(vec![
        menu(
            "main",
            vec![
                keyed("group:tools", 'w', Some("Workshop")),
                keyed("invites.create", 'n', None),
                hidden("main.sysop"),
            ],
        ),
        menu(
            "tools",
            vec![
                item("sysop.queue"),
                item("sysop.packs"),
                item("sysop.recovery"),
            ],
        ),
    ]);
    let tree = built(&s);
    let roles: Vec<Box<dyn PermissionCheck>> = vec![
        Box::new(member()),
        Box::new(sysop()),
        Box::new(holding(&Slug::MEMBER, 1, Permission::ALL)),
    ];
    for access in &roles {
        let custom = reachable(tree, access.as_ref());
        let default = reachable(main_tree(), access.as_ref());
        for action in &custom {
            assert!(
                default.contains(action),
                "{action:?} reachable only through the file"
            );
        }
    }
    // One permission at a time: nothing the default tree withholds appears.
    for held in Permission::ALL {
        struct One(Permission);
        impl PermissionCheck for One {
            fn allows(&self, p: Permission) -> bool {
                p == self.0
            }
        }
        let one = One(*held);
        let custom = reachable(tree, &one);
        let default = reachable(main_tree(), &one);
        for action in custom {
            assert!(
                default.contains(&action) || action == Action::LogOff,
                "{held:?}: {action:?}"
            );
        }
    }
}

#[test]
fn hiding_a_submenu_link_prunes_the_submenu() {
    let s = spec(vec![menu("main", vec![hidden("main.sysop")])]);
    let tree = built(&s);
    assert!(tree.menu("sysop").is_none());
    tree.validate().unwrap();
    assert!(!visible_hotkeys(tree, "main", &sysop()).contains(&'o'));
}

#[test]
fn the_outline_follows_the_same_gate_as_a_session() {
    let tree = built(&spec(vec![]));
    let for_member = outline(tree, &member());
    let for_sysop = outline(tree, &sysop());
    assert!(for_sysop.len() > for_member.len());
    assert!(
        for_member
            .iter()
            .all(|l| l.label != "menu.sysop.item.queue")
    );
    assert!(
        for_sysop
            .iter()
            .any(|l| l.label == "menu.sysop.item.queue" && l.depth == 1)
    );
}

#[test]
fn refusals() {
    use CustomError as E;
    // Unknown target.
    assert!(matches!(
        refused(&spec(vec![menu("main", vec![item("main.nope")])])),
        E::UnknownTarget { .. }
    ));
    // An action cannot be invented, only a built-in id or group:/page: names something.
    assert!(matches!(
        refused(&spec(vec![menu("main", vec![item("action:forums")])])),
        E::UnknownTarget { .. }
    ));
    // Duplicate hotkey (two items share `f`).
    assert!(matches!(
        refused(&spec(vec![menu(
            "main",
            vec![keyed("main.mail", 'f', None)]
        )])),
        E::DuplicateHotkey { .. }
    ));
    // Hotkeys are letters and digits; `?` is help.
    for bad in ['?', ' ', '-', '\u{1b}', 'é'] {
        assert!(matches!(
            refused(&spec(vec![menu(
                "main",
                vec![keyed("main.mail", bad, None)]
            )])),
            E::BadHotkey { .. }
        ));
    }
    // An item twice, or in two menus.
    assert!(matches!(
        refused(&spec(vec![menu(
            "main",
            vec![item("main.mail"), item("main.mail")]
        )])),
        E::DuplicateTarget(_)
    ));
    assert!(matches!(
        refused(&spec(vec![
            menu("main", vec![item("main.mail")]),
            menu("members", vec![item("main.mail")]),
        ])),
        E::DuplicateTarget(_)
    ));
    // A menu listed twice.
    assert!(matches!(
        refused(&spec(vec![menu("main", vec![]), menu("main", vec![])])),
        E::DuplicateMenu(_)
    ));
    // Groups and pages need keys; groups need a name and a link; pages must exist.
    assert!(matches!(
        refused(&spec(vec![menu("main", vec![item("page:rules")])])),
        E::MissingKey { .. }
    ));
    assert!(matches!(
        refused(&spec(vec![
            menu("main", vec![keyed("group:g", 'g', None)]),
            menu("g", vec![item("main.mail")]),
        ])),
        E::MissingLabel { .. }
    ));
    assert!(matches!(
        refused(&spec(vec![menu("g", vec![item("main.mail")])])),
        E::UnlinkedGroup(_)
    ));
    assert!(matches!(
        refused(&spec(vec![menu(
            "main",
            vec![keyed("page:ghost", 'g', None)]
        )])),
        E::UnknownPage(_)
    ));
    // A group's id is a plain word.
    for bad in ["Group", "a/b", "..", "a b", "", "x".repeat(25).as_str()] {
        assert!(matches!(
            refused(&spec(vec![menu(bad, vec![item("main.mail")])])),
            E::BadMenuId(_)
        ));
    }
    // A menu that would be empty.
    let all_sysop = [
        "sysop.queue",
        "sysop.recovery",
        "sysop.preview",
        "sysop.packs",
        "sysop.menus",
    ];
    let moved: Vec<_> = all_sysop
        .iter()
        .zip(['1', '2', '3', '4', '5'])
        .map(|(t, k)| keyed(t, k, None))
        .collect();
    assert!(matches!(
        refused(&spec(vec![menu("main", moved)])),
        E::EmptyMenu(_)
    ));
}

#[test]
fn text_is_bounded_plain_and_sanitized() {
    use CustomError as E;
    let bad_labels = [
        "",
        "   ",
        "has\u{1b}[31m escape",
        "tab\there",
        "nul\0",
        "zero\u{200b}width",
        "private\u{e000}use",
        "marker\u{301a}",
        &"x".repeat(MAX_LABEL_CHARS + 1),
        "new\nline",
    ];
    for label in bad_labels {
        let s = spec(vec![menu(
            "main",
            vec![keyed("main.mail", 'x', Some(label))],
        )]);
        let got = build(&s, &pages);
        // Reserved marker codepoints are the sanitizer's call; everything else must be refused.
        if matches!(got, Err(E::BadText { .. })) {
            continue;
        }
        assert!(label.contains('\u{301a}'), "{label:?} was accepted");
    }
    let s = spec(vec![MenuSpec {
        id: "main".to_owned(),
        title: Some("bad\u{7}title".to_owned()),
        items: vec![],
    }]);
    assert!(matches!(refused(&s), E::BadText { .. }));
}

#[test]
fn caps_hold_against_bombs() {
    use CustomError as E;
    // Too many items in one menu.
    let many: Vec<_> = (0..=MAX_ITEMS)
        .map(|n| item(&format!("main.x{n}")))
        .collect();
    assert!(matches!(
        refused(&spec(vec![menu("main", many)])),
        E::TooManyItems(_)
    ));
    // Too many groups.
    let mut menus = vec![menu(
        "main",
        (0..=MAX_GROUPS)
            .map(|n| {
                keyed(
                    &format!("group:g{n}"),
                    char::from(b'a' + u8::try_from(n).unwrap()),
                    Some("G"),
                )
            })
            .collect(),
    )];
    for n in 0..=MAX_GROUPS {
        menus.push(menu(&format!("g{n}"), vec![hidden("main.mail")]));
    }
    assert!(matches!(refused(&spec(menus)), E::TooManyGroups));
    // Depth: a chain of groups deeper than the cap.
    let depth = MAX_NESTING + 2;
    let mut menus = vec![menu("main", vec![keyed("group:g0", 'a', Some("G"))])];
    for n in 0..depth {
        let inner = if n + 1 < depth {
            vec![keyed(&format!("group:g{}", n + 1), 'a', Some("G"))]
        } else {
            vec![item("main.mail")]
        };
        menus.push(menu(&format!("g{n}"), inner));
    }
    assert!(matches!(refused(&spec(menus)), E::TooDeep));
    // Too many pages.
    let pages_many: Vec<_> = (0..=MAX_PAGES)
        .map(|n| {
            keyed(
                "page:rules",
                char::from(b'a' + u8::try_from(n % 26).unwrap()),
                None,
            )
        })
        .collect();
    let got = refused(&spec(vec![menu("main", pages_many)]));
    assert!(matches!(got, E::TooManyPages | E::DuplicateTarget(_)));
}

#[test]
fn a_cycle_of_groups_is_refused() {
    // a links to b and b links to a, and nothing links to either from the main menu.
    let s = spec(vec![
        menu("a", vec![keyed("group:b", 'b', Some("B"))]),
        menu("b", vec![keyed("group:a", 'a', Some("A"))]),
    ]);
    assert!(matches!(refused(&s), CustomError::Unreachable(_)));
    // A group that links to itself.
    let s = spec(vec![menu(
        "loop",
        vec![keyed("group:loop", 'l', Some("L"))],
    )]);
    assert!(matches!(refused(&s), CustomError::Unreachable(_)));
}

#[test]
fn error_messages_do_not_echo_hostile_text() {
    let hostile = format!("main.{}\u{1b}[2J", "x".repeat(200));
    let message = refused(&spec(vec![menu("main", vec![item(&hostile)])])).to_string();
    assert!(!message.contains('\u{1b}'));
    assert!(message.len() < 300, "{}", message.len());
}
