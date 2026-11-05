#![allow(clippy::unwrap_used, clippy::panic, clippy::items_after_statements)]

use std::fs;
use std::path::Path;

use russh_bbs_core::access::{Permission, PermissionCheck, Slug, Snapshot, Via};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::menu::{
    Action, MenuKey, MenuOutcome, MenuSession, MenuTree, Target, main_tree, outline,
};
use russh_bbs_core::sanitize::RESERVED_CHROME;
use russh_bbs_core::store::AccountId;
use russh_bbs_core::terminal_settings::LayoutId;

use super::render::{DrawCtx, render_bulletin, render_page, text_lines};
use super::{MenuSetup, load, page};
use crate::ui::Ui;
use crate::ui::screens::matrix::{SIZES, check_cells, shot};
use crate::ui::strings::StringTable;

fn strings() -> StringTable {
    Ui::standard().unwrap().strings().clone()
}

fn member() -> Snapshot {
    holding(&[
        Permission::MembersApprove,
        Permission::KeysRecover,
        Permission::ThemesManage,
    ])
}

fn sysop() -> Snapshot {
    holding(&[])
}

fn holding(skip: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        &Slug::MEMBER,
        10,
        &Via::Account,
        Permission::ALL
            .iter()
            .copied()
            .filter(|p| !skip.contains(p)),
    )
}

const GOOD_MENUS: &str = r#"
version = 1

[[menu]]
id = "main"
title = "Main Hall"

  [[menu.item]]
  target = "page:rules"
  key = "r"

  [[menu.item]]
  target = "main.mail"
  key = "x"
  label = "Messages"

  [[menu.item]]
  target = "group:talk"
  key = "t"
  label = "Talk"

  [[menu.item]]
  target = "main.doors"
  hidden = true

[[menu]]
id = "talk"
title = "Talk Hall"

  [[menu.item]]
  target = "main.chat"

  [[menu.item]]
  target = "page:history"
  key = "h"
"#;

const RULES: &str = r#"
version = 1
title = "House rules"
text = """
# Be kind
Plain paragraphs wrap to the room there is, and a long line is wrapped by the page and not cut.
- no spam
- no flame wars
> a quotation
---
The end.
"""
"#;

const HISTORY: &str = r#"
version = 1
title = "A short history"
art = "mark.art.toml"
text = "Founded in a closet.\n\nStill there."
"#;

const ART: &str = r#"
format = 1
name = "mark"

[[piece]]
slot = "area"
rows = ["+-----+", "| BBS |", "+-----+"]
"#;

fn write(dir: &Path, name: &str, text: &str) {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn board() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "menus.toml", GOOD_MENUS);
    write(dir.path(), "pages/rules.toml", RULES);
    write(dir.path(), "pages/history.toml", HISTORY);
    write(dir.path(), "pages/mark.art.toml", ART);
    dir
}

fn built_in(setup: &MenuSetup) -> bool {
    std::ptr::eq(setup.tree(), main_tree()) && !setup.custom()
}

#[test]
fn no_file_is_the_built_in_menu_with_no_complaint() {
    let dir = tempfile::tempdir().unwrap();
    let setup = load(dir.path(), &strings());
    assert!(built_in(&setup));
    assert!(setup.problem().is_none());
    // A directory that does not exist is the same.
    let setup = load(&dir.path().join("nope"), &strings());
    assert!(built_in(&setup) && setup.problem().is_none());
}

#[test]
fn a_good_file_reorders_renames_hides_nests_and_adds_pages() {
    let dir = board();
    let setup = load(dir.path(), &strings());
    assert_eq!(setup.problem(), None);
    assert!(setup.custom());
    let tree = setup.tree();
    tree.validate().unwrap();
    assert_eq!(tree.menu("main").unwrap().title, "Main Hall");
    let m = member();
    let keys: Vec<char> = tree
        .visible_items(tree.menu("main").unwrap(), &m)
        .map(|i| i.hotkey)
        .collect();
    assert_eq!(
        &keys[..3],
        &['r', 'x', 't'],
        "listed items come first, in order"
    );
    assert!(!keys.contains(&'d'), "doors is hidden");
    // Pages loaded, and a member can open them and run an action inside the group.
    assert_eq!(setup.pages().count(), 2);
    let mut session = MenuSession::new(tree);
    assert_eq!(
        session.press(&m, MenuKey::Char('r')),
        MenuOutcome::Page("rules")
    );
    assert_eq!(
        session.press(&m, MenuKey::Char('x')),
        MenuOutcome::Action(Action::Mail)
    );
    session.press(&m, MenuKey::Char('t'));
    assert_eq!(session.current(), "talk");
    assert_eq!(
        session.press(&m, MenuKey::Char('h')),
        MenuOutcome::Page("history")
    );
    assert_eq!(
        session.press(&m, MenuKey::Char('c')),
        MenuOutcome::Action(Action::Chat)
    );
    // The hidden item's hotkey does nothing, on the main menu.
    session.press(&m, MenuKey::Back);
    assert_eq!(session.press(&m, MenuKey::Char('d')), MenuOutcome::Stay);
}

#[test]
fn a_member_never_sees_what_their_role_cannot_reach() {
    let dir = board();
    let setup = load(dir.path(), &strings());
    let tree = setup.tree();
    let reach = |tree: &MenuTree, access: &dyn PermissionCheck| -> Vec<Action> {
        let mut found = Vec::new();
        let mut open = vec![tree.root];
        let mut at = 0;
        while let Some(&id) = open.get(at) {
            at += 1;
            let menu = tree.menu(id).unwrap();
            for item in tree.visible_items(menu, access) {
                match item.target {
                    Target::Action(action) => found.push(action),
                    Target::Menu(next) if !open.contains(&next) => open.push(next),
                    _ => {}
                }
            }
        }
        found
    };
    for access in [&member() as &dyn PermissionCheck, &sysop()] {
        for action in reach(tree, access) {
            assert!(reach(main_tree(), access).contains(&action), "{action:?}");
        }
    }
    // The member's outline has no sysop item, the sysop's still has (the file never touched it).
    let lines = outline(tree, &member());
    assert!(lines.iter().all(|l| !l.label.starts_with("menu.sysop")));
    assert!(
        outline(tree, &sysop())
            .iter()
            .any(|l| l.label == "menu.sysop.item.queue")
    );
}

#[test]
fn a_changed_file_gives_a_different_tree_and_the_same_file_the_same_one() {
    let dir = board();
    let a = load(dir.path(), &strings());
    let b = load(dir.path(), &strings());
    assert!(std::ptr::eq(a.tree(), b.tree()));
    assert_eq!(a, b);
    write(
        dir.path(),
        "menus.toml",
        &GOOD_MENUS.replace("Main Hall", "Front Hall"),
    );
    let c = load(dir.path(), &strings());
    assert!(!std::ptr::eq(a.tree(), c.tree()));
    assert_ne!(a, c);
}

/// Every hostile `menus.toml` below must leave the built-in menu and a logged reason.
#[test]
fn hostile_menu_files_are_refused_and_the_default_menu_stays() {
    let long_line = format!("version = 1 # {}", "x".repeat(300));
    let many_lines = "# c\n".repeat(700);
    let big = format!("version = 1\n# {}", "x".repeat(40_000));
    let depth: String = {
        let mut s = String::from(
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"group:g0\"\nkey = \"a\"\nlabel = \"G\"\n",
        );
        for n in 0..8 {
            s += &format!("[[menu]]\nid = \"g{n}\"\n[[menu.item]]\n");
            if n < 7 {
                s += &format!(
                    "target = \"group:g{}\"\nkey = \"a\"\nlabel = \"G\"\n",
                    n + 1
                );
            } else {
                s += "target = \"main.mail\"\n";
            }
        }
        s
    };
    let many_menus: String = {
        let mut s = String::from("version = 1\n");
        for n in 0..20 {
            s += &format!("[[menu]]\nid = \"g{n}\"\n");
        }
        s
    };
    let cases: Vec<(&str, String)> = vec![
        ("oversized", big),
        ("too many lines", many_lines),
        ("a long line", long_line),
        ("not toml", "this is [not toml".to_owned()),
        ("wrong version", "version = 2".to_owned()),
        ("no version", "[[menu]]\nid = \"main\"".to_owned()),
        ("unknown field", "version = 1\nsurprise = true".to_owned()),
        (
            "unknown item field",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"main.mail\"\naction = \"x\"".to_owned(),
        ),
        (
            "unknown target",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"main.shell\"".to_owned(),
        ),
        (
            "invented action",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"action:logoff\"\nkey = \"z\"\nlabel = \"Z\"".to_owned(),
        ),
        (
            "duplicate hotkey",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"main.mail\"\nkey = \"f\"".to_owned(),
        ),
        (
            "help key",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"main.mail\"\nkey = \"?\"".to_owned(),
        ),
        (
            "two characters for a key",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"main.mail\"\nkey = \"ab\"".to_owned(),
        ),
        (
            "control characters in a label",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"main.mail\"\nkey = \"x\"\nlabel = \"\\u001b[31mred\"".to_owned(),
        ),
        (
            "a marker glyph in a label",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"main.mail\"\nkey = \"x\"\nlabel = \"\\u27e6mail\\u27e7\"".to_owned(),
        ),
        (
            "a privacy claim in a label",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"main.mail\"\nkey = \"x\"\nlabel = \"Private mail\"".to_owned(),
        ),
        (
            "a label that is a string-table key",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"main.mail\"\nkey = \"x\"\nlabel = \"ui.status.unavailable\"".to_owned(),
        ),
        (
            "a group cycle",
            "version = 1\n[[menu]]\nid = \"a\"\n[[menu.item]]\ntarget = \"group:b\"\nkey = \"b\"\nlabel = \"B\"\n[[menu]]\nid = \"b\"\n[[menu.item]]\ntarget = \"group:a\"\nkey = \"a\"\nlabel = \"A\"".to_owned(),
        ),
        ("a depth bomb", depth),
        ("too many menus", many_menus),
        (
            "a page that does not exist",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"page:ghost\"\nkey = \"g\"".to_owned(),
        ),
        (
            "a page id that climbs out",
            "version = 1\n[[menu]]\nid = \"main\"\n[[menu.item]]\ntarget = \"page:../../etc/passwd\"\nkey = \"g\"".to_owned(),
        ),
    ];
    let words = strings();
    for (name, text) in cases {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "menus.toml", &text);
        let setup = load(dir.path(), &words);
        assert!(built_in(&setup), "{name}: the default menu must stay");
        let reason = setup
            .problem()
            .unwrap_or_else(|| panic!("{name}: no reason recorded"));
        assert!(
            !reason.is_empty() && reason.len() <= 400,
            "{name}: {reason}"
        );
        assert!(
            reason.chars().all(|c| !c.is_control()),
            "{name}: the reason carries a control character"
        );
    }
}

#[cfg(unix)]
#[test]
fn a_linked_or_endless_menu_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("/dev/zero", dir.path().join("menus.toml")).unwrap();
    let setup = load(dir.path(), &strings());
    assert!(built_in(&setup));
    assert!(setup.problem().unwrap().contains("not a regular file"));
}

#[test]
fn one_bad_page_refuses_the_whole_load() {
    for (name, text) in [
        (
            "pages/rules.toml",
            "version = 1\ntitle = \"x\"\ntext = \"hi\"\nextra = 1",
        ),
        (
            "pages/rules.toml",
            "version = 3\ntitle = \"x\"\ntext = \"hi\"",
        ),
        (
            "pages/rules.toml",
            "version = 1\ntitle = \"\"\ntext = \"hi\"",
        ),
        (
            "pages/rules.toml",
            "version = 1\ntitle = \"Private place\"\ntext = \"hi\"",
        ),
        (
            "pages/rules.toml",
            "version = 1\ntitle = \"ui.title\"\ntext = \"hi\"",
        ),
        (
            "pages/rules.toml",
            "version = 1\ntitle = \"a\\u001bb\"\ntext = \"hi\"",
        ),
        ("pages/rules.toml", "not toml ["),
    ] {
        let dir = board();
        write(dir.path(), name, text);
        let setup = load(dir.path(), &strings());
        assert!(built_in(&setup), "{text}");
        assert!(setup.problem().unwrap().starts_with("page rules"), "{text}");
    }
    // A missing page file.
    let dir = board();
    fs::remove_file(dir.path().join("pages/history.toml")).unwrap();
    assert!(built_in(&load(dir.path(), &strings())));
}

#[test]
fn hostile_page_files_hit_their_caps() {
    let words = strings();
    let dir = tempfile::tempdir().unwrap();
    let tooling = |text: &str| page::parse("p", text, dir.path(), &words);
    // Too many lines of text.
    let lines = "line\n".repeat(page::MAX_LINES + 1);
    assert!(
        tooling(&format!(
            "version = 1\ntitle = \"t\"\ntext = \"\"\"\n{lines}\"\"\""
        ))
        .is_err()
    );
    // A line too long.
    let long = "x".repeat(page::MAX_LINE_CHARS + 1);
    assert!(tooling(&format!("version = 1\ntitle = \"t\"\ntext = \"{long}\"")).is_err());
    // Too big a file.
    write(
        dir.path(),
        "big.toml",
        &format!(
            "version = 1\ntitle = \"t\"\ntext = \"{}\"",
            "a ".repeat(20_000)
        ),
    );
    assert!(page::load(dir.path(), "big", &words).is_err());
    // Art that is not a plain file name, or that does not exist.
    for art in [
        "../mark.art.toml",
        "/etc/passwd",
        ".hidden.ans",
        "a b.ans",
        "x.png",
        "mark.ans.exe",
    ] {
        let text = format!("version = 1\ntitle = \"t\"\nart = \"{art}\"\ntext = \"x\"");
        assert!(tooling(&text).is_err(), "{art}");
    }
    assert!(tooling("version = 1\ntitle = \"t\"\nart = \"missing.art.toml\"").is_err());
    // A title over the cap.
    assert!(tooling(&format!("version = 1\ntitle = \"{}\"", "t".repeat(33))).is_err());
}

#[test]
fn art_over_the_cap_or_a_bomb_is_refused_by_the_art_loader() {
    let words = strings();
    let dir = tempfile::tempdir().unwrap();
    // 100 columns is wider than the 80-column cap of the area slot.
    let wide = "x".repeat(100);
    write(
        dir.path(),
        "wide.art.toml",
        &format!("format = 1\nname = \"wide\"\n[[piece]]\nslot = \"area\"\nrows = [\"{wide}\"]"),
    );
    let text = "version = 1\ntitle = \"t\"\nart = \"wide.art.toml\"";
    assert!(page::parse("p", text, dir.path(), &words).is_err());
    // An .ans that is far over the byte cap.
    fs::write(dir.path().join("bomb.ans"), vec![b'A'; 600_000]).unwrap();
    let text = "version = 1\ntitle = \"t\"\nart = \"bomb.ans\"";
    assert!(page::parse("p", text, dir.path(), &words).is_err());
    // An .ans with escape sequences parses to cells only, and draws.
    fs::write(
        dir.path().join("ok.ans"),
        b"\x1b[31mhi\x1b[0m\r\nthere\x1b]0;title\x07",
    )
    .unwrap();
    let text = "version = 1\ntitle = \"t\"\nart = \"ok.ans\"";
    let page = page::parse("p", text, dir.path(), &words).unwrap();
    assert!(page.art().is_some());
}

#[test]
fn page_text_is_sanitized_and_cannot_hold_markers_or_private_use_glyphs() {
    let mut hostile = String::from("version = 1\ntitle = \"t\"\ntext = \"\"\"\n");
    hostile += "red \u{1b}[31mtext\u{1b}[0m and \u{7} bell\n";
    hostile += "# \u{1b}]0;title\u{7}head\n";
    hostile += "- \u{200b}zero\u{202e}width\n";
    hostile += "> marker \u{27e6}\u{2299}\u{27e7} here\n";
    hostile += "glyphs \u{e0b0}\u{f101}\u{f0001} done\n";
    hostile += "\"\"\"\n";
    let dir = tempfile::tempdir().unwrap();
    let page = page::parse(
        "p",
        &hostile
            .replace('\u{1b}', "\\u001b")
            .replace('\u{7}', "\\u0007"),
        dir.path(),
        &strings(),
    )
    .unwrap();
    let mut seen = 0;
    for block in page.blocks() {
        let text = match block {
            page::Block::Heading(t)
            | page::Block::Text(t)
            | page::Block::Bullet(t)
            | page::Block::Quote(t) => t,
            page::Block::Rule | page::Block::Blank => continue,
        };
        seen += 1;
        for c in text.chars() {
            assert!(!c.is_control(), "{text:?}");
            assert!(!RESERVED_CHROME.contains(&c), "{text:?}");
            assert!(
                !matches!(c, '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{10ffff}'),
                "{text:?}"
            );
            assert!(
                !matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'),
                "{text:?}"
            );
        }
    }
    assert_eq!(seen, 5);
}

/// A page, drawn in every layout at three sizes, in the Unicode and the ASCII tier: the marker
/// cell is exactly the reserved style, nothing else is reversed, mono draws no colour, ASCII
/// draws seven-bit text only. The text and the art are in the body and nothing else changes.
#[test]
fn pages_render_in_every_layout_and_tier_and_never_cover_the_marker() {
    let dir = board();
    let setup = load(dir.path(), &strings());
    let ui = Ui::standard().unwrap();
    for id in ["rules", "history"] {
        let page = setup.page(id).unwrap();
        for layout in LayoutId::ALL {
            for tier in [Tier::Unicode, Tier::Ascii] {
                for size in SIZES {
                    for scroll in [0, 5, 1000] {
                        let label = format!("{id} {layout:?} {tier:?} {size:?} scroll {scroll}");
                        let buffer = shot(&ui, layout, tier, size, &|frame, d| {
                            let ctx = DrawCtx {
                                strings: d.strings,
                                look: d.look,
                                tier: d.tier,
                                markers: d.markers,
                                clock: d.clock,
                            };
                            render_page(frame, page, scroll, &ctx);
                        });
                        check_cells(&buffer, tier, &label);
                    }
                }
            }
        }
    }
    // A page the board no longer has draws a line and the same bars.
    let buffer = shot(
        &ui,
        LayoutId::Classic,
        Tier::Ascii,
        (80, 24),
        &|frame, d| {
            let ctx = DrawCtx {
                strings: d.strings,
                look: d.look,
                tier: d.tier,
                markers: d.markers,
                clock: d.clock,
            };
            render_bulletin(frame, &setup, "gone", 0, &ctx);
        },
    );
    check_cells(&buffer, Tier::Ascii, "page gone");
}

#[test]
fn the_page_text_scrolls_to_its_end_and_reports_how_far() {
    let dir = board();
    let setup = load(dir.path(), &strings());
    let page = setup.page("rules").unwrap();
    let ui = Ui::standard().unwrap();
    let far = std::cell::Cell::new(0);
    let _ = shot(
        &ui,
        LayoutId::Classic,
        Tier::Unicode,
        (80, 12),
        &|frame, d| {
            let ctx = DrawCtx {
                strings: d.strings,
                look: d.look,
                tier: d.tier,
                markers: d.markers,
                clock: d.clock,
            };
            far.set(render_page(frame, page, 0, &ctx));
        },
    );
    assert!(far.get() > 0, "a long page in a short window scrolls");
    assert!(text_lines(page, 40, look()).len() > 5);
}

fn look() -> crate::settings::Look {
    let ui = Ui::standard().unwrap();
    let settings = russh_bbs_core::terminal_settings::TerminalSettings::default().with_color(false);
    crate::settings::Look::for_theme(
        &settings,
        LayoutId::Classic,
        ui.theme(russh_bbs_core::terminal_settings::ThemeName::new("default").unwrap()),
    )
}

#[test]
fn reload_swaps_the_setup_and_reports_whether_it_changed() {
    let dir = board();
    let ui = Ui::standard().unwrap().with_menu_dir(dir.path());
    assert!(ui.menus().custom());
    let (_, changed) = ui.reload_menus().unwrap();
    assert!(!changed, "nothing changed");
    // A bad edit falls back to the default menu and says why; the board keeps running.
    write(dir.path(), "menus.toml", "version = 9");
    let (setup, changed) = ui.reload_menus().unwrap();
    assert!(changed && built_in(&setup) && setup.problem().is_some());
    assert!(built_in(&ui.menus()));
    // Fixing it brings the custom menu back.
    write(dir.path(), "menus.toml", GOOD_MENUS);
    let (setup, changed) = ui.reload_menus().unwrap();
    assert!(changed && setup.custom());
    // A board with no data directory has nothing to reload.
    assert!(Ui::standard().unwrap().reload_menus().is_none());
}

fn actor(with: bool) -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(7)),
        &Slug::SYSOP,
        100,
        &Via::Account,
        Permission::ALL
            .iter()
            .copied()
            .filter(|p| with || *p != Permission::ThemesManage),
    )
}

fn roles() -> Vec<russh_bbs_core::access::RoleInfo> {
    use russh_bbs_core::access::{Kind, RoleInfo};
    vec![
        RoleInfo {
            name: Slug::SYSOP,
            rank: 100,
            kind: Kind::Locked,
            permissions: Permission::ALL.to_vec(),
        },
        RoleInfo {
            name: Slug::MEMBER,
            rank: 10,
            kind: Kind::Locked,
            permissions: Permission::ALL
                .iter()
                .copied()
                .filter(|p| {
                    !matches!(
                        p,
                        Permission::MembersApprove
                            | Permission::KeysRecover
                            | Permission::ThemesManage
                    )
                })
                .collect(),
        },
    ]
}

#[test]
fn the_service_needs_themes_manage_and_audits_only_a_change() {
    use russh_bbs_core::audit::Action as AuditAction;
    use russh_bbs_core::menu::MenuToolsRequest;
    let dir = board();
    let ui = Ui::standard().unwrap().with_menu_dir(dir.path());
    // Without the permission: nothing is read, nothing audited, nothing listed.
    for request in [MenuToolsRequest::List, MenuToolsRequest::Reload] {
        let outcome = super::service::run(&ui, &actor(false), &roles(), request);
        assert!(outcome.event.is_none());
        assert!(outcome.message.contains("not allowed"));
        assert!(outcome.listing.roles.is_empty() && outcome.listing.pages.is_empty());
    }
    // With it: a listing per role, the member's first (lowest rank), without sysop items.
    let outcome = super::service::run(&ui, &actor(true), &roles(), MenuToolsRequest::List);
    assert!(outcome.event.is_none());
    let rows = &outcome.listing.roles;
    assert_eq!(rows[0].name, "member");
    assert!(
        rows[0]
            .lines
            .iter()
            .all(|l| !l.label.starts_with("menu.sysop"))
    );
    assert!(
        rows[1]
            .lines
            .iter()
            .any(|l| l.label == "menu.sysop.item.menus")
    );
    assert_eq!(outcome.listing.pages.len(), 2);
    // A reload that changes nothing is not audited.
    let outcome = super::service::run(&ui, &actor(true), &roles(), MenuToolsRequest::Reload);
    assert!(outcome.event.is_none());
    assert!(outcome.message.contains("nothing changed"));
    // A reload after an edit is, and its note carries no file text.
    write(
        dir.path(),
        "menus.toml",
        &GOOD_MENUS.replace("Main Hall", "Front Hall"),
    );
    let outcome = super::service::run(&ui, &actor(true), &roles(), MenuToolsRequest::Reload);
    let event = outcome.event.unwrap();
    assert_eq!(event.action, AuditAction::MenusReloaded);
    assert!(!format!("{event:?}").contains("Front Hall"));
    // A refused file reloads to the built-in menu, says why, and is audited as a change.
    write(dir.path(), "menus.toml", "version = 9");
    let outcome = super::service::run(&ui, &actor(true), &roles(), MenuToolsRequest::Reload);
    assert!(outcome.event.is_some());
    assert!(outcome.message.contains("refused"));
    assert!(outcome.listing.problem.is_some() && !outcome.listing.custom);
}

#[test]
fn a_board_with_no_data_directory_says_so_and_does_nothing() {
    use russh_bbs_core::menu::MenuToolsRequest;
    let ui = Ui::standard().unwrap();
    let outcome = super::service::run(&ui, &actor(true), &roles(), MenuToolsRequest::Reload);
    assert!(outcome.listing.unavailable && outcome.event.is_none());
}
