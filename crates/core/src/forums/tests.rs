//! Pure rules: query parsing, text bounds and the board access rule. The stores' behaviour
//! (posting, limits, removal, search over real FTS) is in the shared suite, which runs on both
//! backends.
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

use super::rules::{can, reachable, reachable_ids};
use super::*;
use crate::access::{
    Held, Permission, ResourceId, ResourceKind, ResourceRef, Scope, Slug, Snapshot, Source, Via,
};
use crate::store::AccountId;

fn board(id: i64, restricted: bool) -> BoardRow {
    BoardRow {
        id: BoardId::from_raw(id),
        name: format!("b{id}"),
        description: String::new(),
        restricted,
        threads: 0,
        feed: false,
    }
}

fn resource(id: i64) -> ResourceRef {
    ResourceRef {
        kind: ResourceKind::Forum,
        id: ResourceId::new(id).unwrap(),
    }
}

fn role(scope: Scope, permission: Permission) -> Held {
    Held {
        permission,
        scope,
        source: Source::Role {
            role: Slug::MEMBER,
            rank: 10,
            via: Via::Account,
        },
    }
}

fn allow(scope: Scope, permission: Permission) -> Held {
    Held {
        permission,
        scope,
        source: Source::Allow { via: Via::Account },
    }
}

fn member() -> Vec<Held> {
    [
        Permission::ForumList,
        Permission::ForumRead,
        Permission::ForumPost,
        Permission::ForumReply,
    ]
    .into_iter()
    .map(|p| role(Scope::Global, p))
    .collect()
}

fn snapshot(held: Vec<Held>) -> Snapshot {
    Snapshot::new(Some(AccountId::from_raw(1)), held)
}

#[test]
fn a_global_holding_reaches_open_boards_but_not_restricted_ones() {
    let boards = [board(1, false), board(2, true)];
    let ann = snapshot(member());
    assert!(can(&ann, Permission::ForumRead, &boards[0]));
    assert!(!can(&ann, Permission::ForumRead, &boards[1]));
    assert_eq!(
        reachable_ids(&ann, Permission::ForumList, &boards),
        [BoardId::from_raw(1)]
    );
}

#[test]
fn a_grant_scoped_to_the_board_reaches_it_and_only_it() {
    let boards = [board(1, true), board(2, true)];
    let mut held = member();
    held.push(allow(Scope::Resource(resource(2)), Permission::ForumRead));
    held.push(allow(Scope::Resource(resource(2)), Permission::ForumList));
    let ann = snapshot(held);
    assert!(!can(&ann, Permission::ForumRead, &boards[0]));
    assert!(can(&ann, Permission::ForumRead, &boards[1]));
    // Reading is not posting: only the permissions that were scoped apply.
    assert!(!can(&ann, Permission::ForumPost, &boards[1]));
    assert_eq!(
        reachable_ids(&ann, Permission::ForumList, &boards),
        [BoardId::from_raw(2)]
    );
}

#[test]
fn forum_manage_reaches_every_board_and_nothing_else_does() {
    let boards = [board(1, true), board(2, false)];
    let mut held = member();
    held.push(role(Scope::Global, Permission::ForumManage));
    let staff = snapshot(held);
    assert_eq!(reachable(&staff, Permission::ForumRead, &boards).len(), 2);
    // Manage does not grant what is not held: no moderate means no moderate.
    assert!(!can(&staff, Permission::ForumModerate, &boards[0]));
    // A waiting-room session holds nothing.
    assert!(reachable(&Snapshot::empty(), Permission::ForumList, &boards).is_empty());
}

#[test]
fn a_scoped_moderator_moderates_their_board_only() {
    let boards = [board(1, false), board(2, false)];
    let mut held = member();
    held.push(role(
        Scope::Resource(resource(2)),
        Permission::ForumModerate,
    ));
    let moderator = snapshot(held);
    assert!(!can(&moderator, Permission::ForumModerate, &boards[0]));
    assert!(can(&moderator, Permission::ForumModerate, &boards[1]));
}

#[test]
fn a_query_becomes_plain_lowercase_words() {
    let terms = |raw: &str| SearchQuery::parse(raw).map(|q| q.terms().to_vec());
    assert_eq!(
        terms("Hello, WORLD!"),
        Some(vec!["hello".into(), "world".into()])
    );
    assert_eq!(
        terms("\"quoted phrase\" NEAR(a b) -x *"),
        Some(
            ["quoted", "phrase", "near", "a", "b", "x"]
                .map(String::from)
                .to_vec()
        )
    );
    assert_eq!(
        terms("a:b OR c"),
        Some(["a", "b", "or", "c"].map(String::from).to_vec())
    );
    assert_eq!(terms("same same"), Some(vec!["same".into()]));
    for none in ["", "   ", "\"\"", "*:-()", "\u{1b}[", "\0"] {
        assert_eq!(terms(none), None, "{none:?}");
    }
    assert_eq!(
        terms("çafé ÉCOLE"),
        Some(vec!["çafé".into(), "école".into()])
    );
}

#[test]
fn a_query_is_bounded() {
    let many = (0..100)
        .map(|i| format!("w{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let query = SearchQuery::parse(&many).unwrap();
    assert!(query.terms().len() <= MAX_TERMS);
    let long = "a".repeat(100_000);
    let query = SearchQuery::parse(&long).unwrap();
    assert_eq!(query.terms().len(), 1);
    assert_eq!(query.terms()[0].chars().count(), MAX_TERM_CHARS);
    // Input past the query ceiling is ignored.
    let late = format!("{} needle", " ".repeat(MAX_QUERY_CHARS));
    assert_eq!(SearchQuery::parse(&late), None);
}

#[test]
fn subjects_and_names_are_sanitized_and_refused_not_cut() {
    assert_eq!(
        prepare_subject("  Hi\u{1b}[31m there\nfriend ")
            .unwrap()
            .as_str(),
        "Hi there friend"
    );
    assert_eq!(prepare_subject("   "), Err(TextError::Empty));
    assert_eq!(
        prepare_subject(&"x".repeat(SUBJECT_MAX_CHARS))
            .unwrap()
            .as_str()
            .len(),
        SUBJECT_MAX_CHARS
    );
    assert_eq!(
        prepare_subject(&"x".repeat(SUBJECT_MAX_CHARS + 1)),
        Err(TextError::TooLong {
            max: SUBJECT_MAX_CHARS
        })
    );
    assert_eq!(prepare_description("").unwrap().as_str(), "");
    assert!(prepare_name(&"n".repeat(NAME_MAX_CHARS + 1)).is_err());
    let marker: String = crate::sanitize::RESERVED_CHROME.iter().collect();
    let cleaned = prepare_name(&format!("a{marker}b")).unwrap();
    assert!(
        !cleaned
            .as_str()
            .chars()
            .any(|c| crate::sanitize::RESERVED_CHROME.contains(&c))
    );
}

#[test]
fn a_hit_snippet_is_one_line_around_the_first_term() {
    let raw = RawHit {
        post: PostId::from_raw(1),
        thread: ThreadId::from_raw(1),
        board: BoardId::from_raw(1),
        board_name: "B".into(),
        subject: "S".into(),
        author: Some("ann".into()),
        body: format!(
            "{}\nneedle in the middle\u{1b}[2J {}",
            "pad ".repeat(60),
            "tail ".repeat(60)
        ),
        index: 0,
        created_at: crate::store::UnixSeconds::from_raw(1),
    };
    let query = SearchQuery::parse("NEEDLE").unwrap();
    let hit = search::hit(&raw, &query);
    assert!(
        hit.snippet.contains("needle in the middle"),
        "{}",
        hit.snippet
    );
    assert!(!hit.snippet.contains('\n') && !hit.snippet.contains('\u{1b}'));
    assert!(hit.snippet.starts_with("...") && hit.snippet.ends_with("..."));
    assert!(hit.snippet.chars().count() <= 106);
}
