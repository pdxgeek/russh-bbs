#![allow(clippy::unwrap_used, clippy::panic)]

use super::{
    Access, Child, Decision, Fingerprint, GroupChange, GroupError, GroupGraph, GroupId, Held,
    InvalidFingerprint, Kind, MAX_GROUP_DEPTH, MemberDirectory, NoMembers, Permission,
    PermissionCheck, Reach, Reason, ResourceId, ResourceKind, ResourceRef, Scope, Slug, Snapshot,
    Source, Via, decide_access, reach, resolve,
};
use crate::store::AccountId;

const VALID: &str = "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s";

struct OnlyKey(Fingerprint);

impl MemberDirectory for OnlyKey {
    fn is_member(&self, key: &Fingerprint) -> impl Future<Output = bool> + Send {
        std::future::ready(*key == self.0)
    }
}

fn fingerprint(text: &str) -> Fingerprint {
    Fingerprint::parse(text).unwrap()
}

#[tokio::test]
async fn unknown_key_is_restricted() {
    let access = decide_access(&NoMembers, &fingerprint(VALID)).await;
    assert_eq!(access, Access::Restricted);
}

#[tokio::test]
async fn known_member_key_gets_member_access() {
    let directory = OnlyKey(fingerprint(VALID));
    assert_eq!(
        decide_access(&directory, &fingerprint(VALID)).await,
        Access::Member
    );
}

#[tokio::test]
async fn a_different_key_is_still_restricted() {
    let directory = OnlyKey(fingerprint(VALID));
    let other = fingerprint("SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(decide_access(&directory, &other).await, Access::Restricted);
}

#[test]
fn fingerprint_parse_accepts_openssh_sha256() {
    assert_eq!(fingerprint(VALID).as_str(), VALID);
}

#[test]
fn fingerprint_parse_rejects_bad_shapes() {
    for bad in [
        "",
        "SHA256:",
        "MD5:aa:bb",
        "uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s",
        "SHA256:short",
        "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s=",
        "SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2\x1b",
    ] {
        assert_eq!(Fingerprint::parse(bad), Err(InvalidFingerprint), "{bad:?}");
    }
}

// ---- permissions ----------------------------------------------------------------------------

#[test]
fn permission_names_are_unique_area_dot_verb_and_round_trip() {
    let mut seen = std::collections::HashSet::new();
    for &permission in Permission::ALL {
        let name = permission.name();
        assert!(seen.insert(name), "{name} is listed twice");
        let (area, verb) = name.split_once('.').unwrap();
        assert!(!area.is_empty() && !verb.is_empty(), "{name}");
        assert_eq!(Permission::parse(name), Some(permission), "{name}");
    }
    assert_eq!(Permission::parse("forum.nope"), None);
    assert_eq!(Permission::parse(""), None);
    assert_eq!(Permission::parse("FORUM.READ"), None);
}

/// The enum and the table in `docs/access.md` are the same list: a slice that adds one without
/// the other fails here.
#[test]
fn the_enum_equals_the_permissions_table_in_the_docs() {
    let docs = include_str!("../../../../docs/access.md");
    let table = docs
        .split("## Permissions")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .unwrap();
    let mut documented = Vec::new();
    for line in table
        .lines()
        .filter(|l| l.starts_with("| ") && !l.starts_with("| Group") && !l.starts_with("|---"))
    {
        let cell = line.split('|').nth(2).unwrap();
        for name in cell.split(',') {
            documented.push(name.trim().trim_matches('`').to_owned());
        }
    }
    let mut coded: Vec<String> = Permission::ALL
        .iter()
        .map(|p| p.name().to_owned())
        .collect();
    documented.sort();
    coded.sort();
    assert_eq!(documented, coded);
}

// ---- slugs ----------------------------------------------------------------------------------

#[test]
fn slugs_are_short_lowercase_ascii() {
    for good in ["a", "member", "door-admin", "x1", &"a".repeat(32)] {
        assert_eq!(Slug::parse(good).unwrap().as_str(), good);
    }
    for bad in [
        "",
        "-a",
        "a-",
        "A",
        "a b",
        "a_b",
        "é",
        &"a".repeat(33),
        "a\n",
    ] {
        assert!(Slug::parse(bad).is_err(), "{bad:?}");
    }
    assert_eq!(Slug::parse("sysop").unwrap(), Slug::SYSOP);
}

// ---- resolve --------------------------------------------------------------------------------

fn board(n: i64) -> ResourceRef {
    ResourceRef {
        kind: ResourceKind::Forum,
        id: ResourceId::new(n).unwrap(),
    }
}

fn room(n: i64) -> ResourceRef {
    ResourceRef {
        kind: ResourceKind::Room,
        id: ResourceId::new(n).unwrap(),
    }
}

fn role(name: &'static str, rank: u8, via: Via) -> Source {
    Source::Role {
        role: Slug::parse(name).unwrap(),
        rank,
        via,
    }
}

fn group(name: &str) -> Via {
    Via::Group(Slug::parse(name).unwrap())
}

fn held(permission: Permission, scope: Scope, source: Source) -> Held {
    Held {
        permission,
        scope,
        source,
    }
}

fn snapshot(held: Vec<Held>) -> Snapshot {
    Snapshot::new(Some(AccountId::from_raw(1)), held)
}

struct Case {
    name: &'static str,
    held: Vec<Held>,
    ask: Permission,
    on: Option<ResourceRef>,
    /// `None`: denied. `Some(source)`: granted for exactly that reason.
    expect: Option<Source>,
}

#[test]
#[allow(clippy::too_many_lines)] // a table of cases is easier to read whole
fn resolve_table() {
    use Permission::{ForumModerate, ForumRead, MembersView};
    let moderator = || role("moderator", 30, Via::Account);
    let cases = vec![
        Case {
            name: "empty snapshot denies everything",
            held: vec![],
            ask: ForumRead,
            on: None,
            expect: None,
        },
        Case {
            name: "a global role holding grants",
            held: vec![held(ForumRead, Scope::Global, moderator())],
            ask: ForumRead,
            on: None,
            expect: Some(moderator()),
        },
        Case {
            name: "a global role holding answers a check on any resource",
            held: vec![held(ForumRead, Scope::Global, moderator())],
            ask: ForumRead,
            on: Some(board(7)),
            expect: Some(moderator()),
        },
        Case {
            name: "another permission does not grant this one",
            held: vec![held(ForumRead, Scope::Global, moderator())],
            ask: ForumModerate,
            on: None,
            expect: None,
        },
        Case {
            name: "a role granted on another resource is denied",
            held: vec![held(ForumModerate, Scope::Resource(board(1)), moderator())],
            ask: ForumModerate,
            on: Some(board(2)),
            expect: None,
        },
        Case {
            name: "a role granted on this resource grants",
            held: vec![held(ForumModerate, Scope::Resource(board(1)), moderator())],
            ask: ForumModerate,
            on: Some(board(1)),
            expect: Some(moderator()),
        },
        Case {
            name: "same id, different kind is another resource",
            held: vec![held(ForumModerate, Scope::Resource(board(1)), moderator())],
            ask: ForumModerate,
            on: Some(room(1)),
            expect: None,
        },
        Case {
            name: "a scoped holding never answers a check with no resource",
            held: vec![held(ForumModerate, Scope::Resource(board(1)), moderator())],
            ask: ForumModerate,
            on: None,
            expect: None,
        },
        Case {
            name: "an allow entry for the account on this resource grants",
            held: vec![held(
                ForumRead,
                Scope::Resource(board(3)),
                Source::Allow { via: Via::Account },
            )],
            ask: ForumRead,
            on: Some(board(3)),
            expect: Some(Source::Allow { via: Via::Account }),
        },
        Case {
            name: "an allow entry through a group grants, and says which group",
            held: vec![held(
                ForumRead,
                Scope::Resource(board(3)),
                Source::Allow {
                    via: group("lounge"),
                },
            )],
            ask: ForumRead,
            on: Some(board(3)),
            expect: Some(Source::Allow {
                via: group("lounge"),
            }),
        },
        Case {
            name: "an allow entry on another resource is denied",
            held: vec![held(
                ForumRead,
                Scope::Resource(board(3)),
                Source::Allow { via: Via::Account },
            )],
            ask: ForumRead,
            on: Some(board(4)),
            expect: None,
        },
        Case {
            name: "a role through a group grants, and says which group",
            held: vec![held(
                ForumModerate,
                Scope::Global,
                role("moderator", 30, group("staff")),
            )],
            ask: ForumModerate,
            on: None,
            expect: Some(role("moderator", 30, group("staff"))),
        },
        Case {
            name: "the waiting snapshot grants waiting permissions only",
            held: vec![held(
                Permission::WaitingView,
                Scope::Global,
                role("waiting", 0, Via::Waiting),
            )],
            ask: MembersView,
            on: None,
            expect: None,
        },
        Case {
            name: "global role beats scoped role beats allow entry (stable reason)",
            held: vec![
                held(
                    ForumRead,
                    Scope::Resource(board(1)),
                    Source::Allow { via: Via::Account },
                ),
                held(
                    ForumRead,
                    Scope::Resource(board(1)),
                    role("a", 1, Via::Account),
                ),
                held(ForumRead, Scope::Global, role("b", 2, Via::Account)),
            ],
            ask: ForumRead,
            on: Some(board(1)),
            expect: Some(role("b", 2, Via::Account)),
        },
        Case {
            name: "scoped role beats allow entry",
            held: vec![
                held(
                    ForumRead,
                    Scope::Resource(board(1)),
                    Source::Allow { via: Via::Account },
                ),
                held(
                    ForumRead,
                    Scope::Resource(board(1)),
                    role("a", 1, Via::Account),
                ),
            ],
            ask: ForumRead,
            on: Some(board(1)),
            expect: Some(role("a", 1, Via::Account)),
        },
        Case {
            name: "a global allow entry answers a check with no resource",
            held: vec![held(
                ForumRead,
                Scope::Global,
                Source::Allow { via: Via::Account },
            )],
            ask: ForumRead,
            on: None,
            expect: Some(Source::Allow { via: Via::Account }),
        },
    ];
    for case in cases {
        let snap = snapshot(case.held);
        let decision = resolve(&snap, case.ask, case.on);
        match (case.expect, decision) {
            (None, Decision::Denied) => {}
            (Some(source), Decision::Granted(Reason(got))) => {
                assert_eq!(source, got, "{}", case.name);
            }
            (expected, got) => panic!("{}: expected {expected:?}, got {got:?}", case.name),
        }
        // The menu hook is the global check.
        assert_eq!(
            snap.allows(case.ask),
            resolve(&snap, case.ask, None).is_granted(),
            "{}",
            case.name
        );
    }
}

#[test]
fn require_gives_proof_only_when_granted() {
    let snap = snapshot(vec![held(
        Permission::MailRead,
        Scope::Global,
        role("member", 10, group("everyone")),
    )]);
    let granted = snap.require(Permission::MailRead, None).unwrap();
    assert_eq!(granted.permission(), Permission::MailRead);
    assert_eq!(granted.account(), Some(AccountId::from_raw(1)));
    let denied = snap.require(Permission::MailSend, None).unwrap_err();
    assert_eq!(denied.permission, Permission::MailSend);
}

#[test]
fn top_role_is_the_highest_rank() {
    let snap = snapshot(vec![
        held(
            Permission::MailRead,
            Scope::Global,
            role("member", 10, group("everyone")),
        ),
        held(
            Permission::MailRead,
            Scope::Global,
            role("moderator", 30, group("staff")),
        ),
        held(
            Permission::MailRead,
            Scope::Resource(board(1)),
            Source::Allow { via: Via::Account },
        ),
    ]);
    assert_eq!(snap.top_role(), Some(&Slug::MODERATOR));
    assert_eq!(Snapshot::empty().top_role(), None);
}

#[test]
fn reach_table() {
    use Permission::ForumList;
    let m = || role("m", 10, Via::Account);
    let scoped = |n| held(ForumList, Scope::Resource(board(n)), m());
    assert_eq!(
        reach(&snapshot(vec![]), ForumList, ResourceKind::Forum),
        Reach::None
    );
    assert_eq!(
        reach(
            &snapshot(vec![held(ForumList, Scope::Global, m())]),
            ForumList,
            ResourceKind::Forum
        ),
        Reach::All
    );
    assert_eq!(
        reach(
            &snapshot(vec![scoped(5), scoped(2), scoped(5)]),
            ForumList,
            ResourceKind::Forum
        ),
        Reach::Only(vec![
            ResourceId::new(2).unwrap(),
            ResourceId::new(5).unwrap()
        ])
    );
    assert_eq!(
        reach(&snapshot(vec![scoped(5)]), ForumList, ResourceKind::Room),
        Reach::None
    );
    assert_eq!(
        reach(
            &snapshot(vec![scoped(5)]),
            Permission::ForumRead,
            ResourceKind::Forum
        ),
        Reach::None
    );
}

mod properties {
    use proptest::prelude::*;

    use super::*;

    const PERMS: [Permission; 3] = [
        Permission::ForumList,
        Permission::ForumRead,
        Permission::ForumModerate,
    ];

    fn arb_scope() -> impl Strategy<Value = Scope> {
        prop_oneof![
            Just(Scope::Global),
            (0..2usize, 1..4i64).prop_map(|(k, id)| Scope::Resource(ResourceRef {
                kind: [ResourceKind::Forum, ResourceKind::Room][k],
                id: ResourceId::new(id).unwrap(),
            })),
        ]
    }

    fn arb_source() -> impl Strategy<Value = Source> {
        prop_oneof![
            Just(Source::Allow { via: Via::Account }),
            Just(role("r", 10, Via::Account)),
            Just(role("s", 30, group("staff"))),
        ]
    }

    fn arb_held() -> impl Strategy<Value = Held> {
        (0..PERMS.len(), arb_scope(), arb_source()).prop_map(|(p, scope, source)| Held {
            permission: PERMS[p],
            scope,
            source,
        })
    }

    proptest! {
        /// `resolve` is granted exactly when some held fact covers the check (an oracle written
        /// separately from the implementation).
        #[test]
        fn resolve_matches_a_plain_oracle(
            facts in proptest::collection::vec(arb_held(), 0..8),
            p in 0..PERMS.len(),
            on in proptest::option::of((0..2usize, 1..4i64)),
        ) {
            let on = on.map(|(k, id)| ResourceRef {
                kind: [ResourceKind::Forum, ResourceKind::Room][k],
                id: ResourceId::new(id).unwrap(),
            });
            let snap = snapshot(facts.clone());
            let oracle = facts.iter().any(|h| {
                h.permission == PERMS[p]
                    && match (h.scope, on) {
                        (Scope::Global, _) => true,
                        (Scope::Resource(r), Some(asked)) => r == asked,
                        (Scope::Resource(_), None) => false,
                    }
            });
            prop_assert_eq!(resolve(&snap, PERMS[p], on).is_granted(), oracle);
        }

        /// `reach` and `resolve` never disagree: the one visibility mechanism.
        #[test]
        fn reach_agrees_with_resolve(
            facts in proptest::collection::vec(arb_held(), 0..8),
            p in 0..PERMS.len(),
            k in 0..2usize,
            id in 1..4i64,
        ) {
            let snap = snapshot(facts);
            let kind = [ResourceKind::Forum, ResourceKind::Room][k];
            let r = ResourceRef { kind, id: ResourceId::new(id).unwrap() };
            prop_assert_eq!(
                resolve(&snap, PERMS[p], Some(r)).is_granted(),
                reach(&snap, PERMS[p], kind).contains(r.id)
            );
        }
    }
}

// ---- group graph ----------------------------------------------------------------------------

fn g(n: i64) -> GroupId {
    GroupId::from_raw(n)
}

fn a(n: i64) -> AccountId {
    AccountId::from_raw(n)
}

fn graph(count: i64) -> GroupGraph {
    GroupGraph::new((1..=count).map(|n| (g(n), Kind::Custom)), [])
}

fn nest(graph: &mut GroupGraph, outer: i64, inner: i64) -> Result<(), GroupError> {
    graph.apply(GroupChange::Add {
        group: g(outer),
        child: Child::Group(g(inner)),
    })
}

fn add_account(graph: &mut GroupGraph, group: i64, account: i64) {
    graph
        .apply(GroupChange::Add {
            group: g(group),
            child: Child::Account(a(account)),
        })
        .unwrap();
}

#[test]
fn group_cycles_are_rejected() {
    let mut graph = graph(4);
    assert_eq!(nest(&mut graph, 1, 1), Err(GroupError::Cycle), "self loop");
    nest(&mut graph, 1, 2).unwrap();
    assert_eq!(nest(&mut graph, 2, 1), Err(GroupError::Cycle), "two cycle");
    nest(&mut graph, 2, 3).unwrap();
    assert_eq!(
        nest(&mut graph, 3, 1),
        Err(GroupError::Cycle),
        "three cycle"
    );
    assert_eq!(nest(&mut graph, 3, 2), Err(GroupError::Cycle));
    // A diamond is not a cycle.
    nest(&mut graph, 1, 4).unwrap();
    nest(&mut graph, 4, 3).unwrap();
}

#[test]
fn a_rejected_change_leaves_the_graph_unchanged() {
    let mut graph = graph(3);
    nest(&mut graph, 1, 2).unwrap();
    add_account(&mut graph, 2, 9);
    let before = graph.closure();
    assert_eq!(nest(&mut graph, 2, 1), Err(GroupError::Cycle));
    assert_eq!(graph.closure(), before);
}

#[test]
fn nesting_depth_is_capped() {
    let mut graph = graph(i64::try_from(MAX_GROUP_DEPTH).unwrap() + 2);
    // Eight groups in a chain (1 > 2 > ... > 8) is allowed.
    for outer in 1..i64::try_from(MAX_GROUP_DEPTH).unwrap() {
        nest(&mut graph, outer, outer + 1).unwrap();
    }
    // A ninth is not.
    let eighth = i64::try_from(MAX_GROUP_DEPTH).unwrap();
    assert_eq!(
        nest(&mut graph, eighth, eighth + 1),
        Err(GroupError::TooDeep)
    );
    // Putting the whole chain under another group is also too deep.
    assert_eq!(nest(&mut graph, eighth + 2, 1), Err(GroupError::TooDeep));
}

#[test]
fn the_closure_follows_nesting_and_shrinks_on_remove() {
    let mut graph = graph(3);
    nest(&mut graph, 1, 2).unwrap();
    nest(&mut graph, 2, 3).unwrap();
    add_account(&mut graph, 3, 100);
    add_account(&mut graph, 2, 200);
    let rows = graph.closure();
    assert!(rows.contains(&(g(1), a(100))) && rows.contains(&(g(2), a(100))));
    assert!(rows.contains(&(g(3), a(100))));
    assert!(rows.contains(&(g(1), a(200))) && rows.contains(&(g(2), a(200))));
    assert!(!rows.contains(&(g(3), a(200))));
    graph
        .apply(GroupChange::Remove {
            group: g(2),
            child: Child::Group(g(3)),
        })
        .unwrap();
    let rows = graph.closure();
    assert!(!rows.contains(&(g(1), a(100))) && !rows.contains(&(g(2), a(100))));
    assert!(rows.contains(&(g(3), a(100))));
    graph.apply(GroupChange::Delete(g(2))).unwrap();
    assert!(graph.closure().iter().all(|(group, _)| *group != g(2)));
}

#[test]
fn locked_builtin_and_unknown_groups_are_protected() {
    let mut graph = GroupGraph::new(
        [
            (g(1), Kind::Locked),
            (g(2), Kind::Builtin),
            (g(3), Kind::Custom),
        ],
        [],
    );
    assert_eq!(
        nest(&mut graph, 1, 3),
        Err(GroupError::Locked),
        "everyone has no members"
    );
    assert_eq!(
        nest(&mut graph, 3, 1),
        Err(GroupError::Locked),
        "everyone is never nested"
    );
    assert_eq!(
        graph.apply(GroupChange::Delete(g(1))),
        Err(GroupError::Locked)
    );
    assert_eq!(
        graph.apply(GroupChange::Delete(g(2))),
        Err(GroupError::Builtin)
    );
    assert_eq!(nest(&mut graph, 3, 99), Err(GroupError::UnknownGroup));
    assert_eq!(nest(&mut graph, 99, 3), Err(GroupError::UnknownGroup));
    assert_eq!(
        graph.apply(GroupChange::Delete(g(99))),
        Err(GroupError::UnknownGroup)
    );
    // A builtin group (staff) keeps editable membership.
    nest(&mut graph, 2, 3).unwrap();
}
