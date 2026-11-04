//! Roles, groups, grants and the per-action snapshot, against any backend (M2-B, D33).
#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeSet;

use russh_bbs_core::access::{
    AccessError, AccessStore, Child, Decision, Granting, GroupChange, GroupError, GroupId, Kind,
    NewGrant, Permission, Reach, Reason, ResourceId, ResourceKind, ResourceRef, Scope, Slug,
    Source, Subject, Via,
};
use russh_bbs_core::audit::Actor;
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{Account, AccountState, AccountStore, NewAccount};

use super::{Harness, TestStore, fingerprint};

async fn account<S: AccountStore>(store: &S, name: &str, state: AccountState) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role: Slug::MEMBER,
            state,
        })
        .await
        .unwrap()
}

async fn active<S: AccountStore>(store: &S, name: &str) -> Account {
    account(store, name, AccountState::Active).await
}

async fn group<S: AccessStore>(store: &S, name: &str) -> GroupId {
    store
        .create_group(&Slug::parse(name).unwrap(), Actor::System)
        .await
        .unwrap()
}

async fn add<S: AccessStore>(store: &S, outer: GroupId, child: Child) -> Result<(), AccessError> {
    store
        .change_groups(
            GroupChange::Add {
                group: outer,
                child,
            },
            Actor::System,
        )
        .await
}

fn board(n: i64) -> ResourceRef {
    ResourceRef {
        kind: ResourceKind::Forum,
        id: ResourceId::new(n).unwrap(),
    }
}

async fn role_set<S: AccessStore>(store: &S, name: &Slug) -> BTreeSet<Permission> {
    store
        .roles()
        .await
        .unwrap()
        .into_iter()
        .find(|role| role.name == *name)
        .unwrap()
        .permissions
        .into_iter()
        .collect()
}

pub(crate) async fn the_built_in_roles_have_their_ranks_and_kinds<H: Harness>() {
    let h = H::fresh().await;
    let roles = h.store().roles().await.unwrap();
    let got: Vec<(String, u8, Kind)> = roles
        .iter()
        .map(|r| (r.name.to_string(), r.rank, r.kind))
        .collect();
    let mut want = vec![
        ("cosysop".to_owned(), 40, Kind::Builtin),
        ("member".to_owned(), 10, Kind::Builtin),
        ("moderator".to_owned(), 30, Kind::Builtin),
        ("sysop".to_owned(), 100, Kind::Locked),
        ("trusted".to_owned(), 20, Kind::Builtin),
        ("waiting".to_owned(), 0, Kind::Locked),
    ];
    want.sort();
    assert_eq!(got, want);
}

pub(crate) async fn waiting_holds_exactly_the_waiting_room_permissions<H: Harness>() {
    let h = H::fresh().await;
    let set = role_set(h.store(), &Slug::WAITING).await;
    let want: BTreeSet<_> = [
        Permission::WaitingView,
        Permission::InviteRequest,
        Permission::InviteRedeem,
        Permission::MembershipApply,
        Permission::MembershipOpen,
        Permission::KeysRedeem,
    ]
    .into();
    assert_eq!(set, want);
}

pub(crate) async fn each_role_contains_the_one_below_and_sysop_holds_everything<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let member = role_set(store, &Slug::MEMBER).await;
    let trusted = role_set(store, &Slug::TRUSTED).await;
    let moderator = role_set(store, &Slug::MODERATOR).await;
    let cosysop = role_set(store, &Slug::COSYSOP).await;
    let sysop = role_set(store, &Slug::SYSOP).await;
    assert!(member.is_subset(&trusted) && member != trusted);
    assert!(trusted.is_subset(&moderator) && trusted != moderator);
    assert!(moderator.is_subset(&cosysop) && moderator != cosysop);
    assert!(cosysop.is_subset(&sysop) && cosysop != sysop);
    // The sysop role is every permission in the enum: a permission added without its sysop row
    // (the migration that introduces it) fails here.
    let all: BTreeSet<_> = Permission::ALL.iter().copied().collect();
    assert_eq!(sysop, all);
    // The documented exceptions for `member`.
    for missing in [Permission::PollsCreate, Permission::EventsManage] {
        assert!(!member.contains(&missing), "{}", missing.name());
    }
    for held in [
        Permission::AccountEditSettings,
        Permission::MembersView,
        Permission::InvitesCreate,
        Permission::FilesUpload,
        Permission::MailSend,
        Permission::ForumPost,
    ] {
        assert!(member.contains(&held), "{}", held.name());
    }
    // Roles besides sysop never hold the sysop-only permissions.
    for sysop_only in [
        Permission::RolesEdit,
        Permission::SettingsEdit,
        Permission::BackupsRun,
        Permission::MembersDeleteData,
    ] {
        assert!(!cosysop.contains(&sysop_only), "{}", sysop_only.name());
    }
}

pub(crate) async fn the_built_in_groups_exist<H: Harness>() {
    let h = H::fresh().await;
    let groups = h.store().groups().await.unwrap();
    let kind_of = |name: &Slug| groups.iter().find(|g| g.name == *name).map(|g| g.kind);
    assert_eq!(kind_of(&Slug::EVERYONE), Some(Kind::Locked));
    assert_eq!(kind_of(&Slug::STAFF), Some(Kind::Builtin));
    let everyone = groups.iter().find(|g| g.name == Slug::EVERYONE).unwrap().id;
    let staff = groups.iter().find(|g| g.name == Slug::STAFF).unwrap().id;
    let store = h.store();
    assert!(matches!(
        store
            .change_groups(GroupChange::Delete(everyone), Actor::System)
            .await,
        Err(AccessError::Group(GroupError::Locked))
    ));
    assert!(matches!(
        store
            .change_groups(GroupChange::Delete(staff), Actor::System)
            .await,
        Err(AccessError::Group(GroupError::Builtin))
    ));
    let member = active(store, "alice").await;
    assert!(matches!(
        add(store, everyone, Child::Account(member.id)).await,
        Err(AccessError::Group(GroupError::Locked))
    ));
}

pub(crate) async fn an_active_member_holds_member_through_everyone<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = active(store, "alice").await;
    let snapshot = store.snapshot(Some(alice.id)).await.unwrap();
    assert_eq!(snapshot.account(), Some(alice.id));
    let Decision::Granted(Reason(source)) = snapshot.resolve(Permission::ForumRead, None) else {
        panic!("member should read forums");
    };
    assert_eq!(
        source,
        Source::Role {
            role: Slug::MEMBER,
            rank: 10,
            via: Via::Group(Slug::EVERYONE)
        }
    );
    assert!(
        !snapshot
            .resolve(Permission::MembersApprove, None)
            .is_granted()
    );
    assert!(
        !snapshot
            .resolve(Permission::ForumModerate, None)
            .is_granted()
    );
    assert_eq!(snapshot.top_role(), Some(&Slug::MEMBER));
}

pub(crate) async fn the_founder_holds_every_permission_as_sysop<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let founder = store
        .claim_founder(
            sanitize_username(russh_bbs_core::founder::FOUNDER_USERNAME).unwrap(),
            &fingerprint(1),
        )
        .await
        .unwrap()
        .unwrap();
    let snapshot = store.snapshot(Some(founder.id)).await.unwrap();
    for &permission in Permission::ALL {
        assert!(
            snapshot.resolve(permission, None).is_granted(),
            "{}",
            permission.name()
        );
    }
    assert_eq!(snapshot.top_role(), Some(&Slug::SYSOP));
    // The grant is to the account, not to a group.
    let Decision::Granted(Reason(Source::Role { via, .. })) =
        snapshot.resolve(Permission::RolesEdit, None)
    else {
        panic!("sysop edits roles");
    };
    assert_eq!(via, Via::Account);
}

pub(crate) async fn unknown_and_inactive_accounts_get_the_waiting_snapshot<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let pending = account(store, "carol", AccountState::Pending).await;
    let suspended = account(store, "dave", AccountState::Suspended).await;
    // Even a suspended sysop holds nothing but the waiting room.
    store
        .add_grant(
            &NewGrant {
                subject: Subject::Account(suspended.id),
                granting: Granting::Role(Slug::SYSOP),
                scope: Scope::Global,
            },
            Actor::System,
        )
        .await
        .unwrap();
    let missing = russh_bbs_core::store::AccountId::from_raw(9_999);
    for who in [None, Some(missing), Some(pending.id), Some(suspended.id)] {
        let snapshot = store.snapshot(who).await.unwrap();
        assert_eq!(snapshot.account(), None, "{who:?}");
        for &permission in Permission::ALL {
            let want = matches!(
                permission,
                Permission::WaitingView
                    | Permission::InviteRequest
                    | Permission::InviteRedeem
                    | Permission::MembershipApply
                    | Permission::MembershipOpen
                    | Permission::KeysRedeem
            );
            assert_eq!(
                snapshot.resolve(permission, None).is_granted(),
                want,
                "{who:?} {}",
                permission.name()
            );
        }
        assert_eq!(snapshot.top_role(), Some(&Slug::WAITING));
    }
}

pub(crate) async fn staff_members_hold_moderator<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = active(store, "alice").await;
    let staff = store
        .groups()
        .await
        .unwrap()
        .into_iter()
        .find(|g| g.name == Slug::STAFF)
        .unwrap()
        .id;
    let before = store.snapshot(Some(alice.id)).await.unwrap();
    assert!(!before.resolve(Permission::ForumModerate, None).is_granted());
    add(store, staff, Child::Account(alice.id)).await.unwrap();
    let after = store.snapshot(Some(alice.id)).await.unwrap();
    let Decision::Granted(Reason(source)) = after.resolve(Permission::ForumModerate, None) else {
        panic!("staff should moderate");
    };
    assert_eq!(
        source,
        Source::Role {
            role: Slug::MODERATOR,
            rank: 30,
            via: Via::Group(Slug::STAFF)
        }
    );
    assert_eq!(after.top_role(), Some(&Slug::MODERATOR));
    // Removing the member applies on the next snapshot.
    store
        .change_groups(
            GroupChange::Remove {
                group: staff,
                child: Child::Account(alice.id),
            },
            Actor::System,
        )
        .await
        .unwrap();
    let removed = store.snapshot(Some(alice.id)).await.unwrap();
    assert!(
        !removed
            .resolve(Permission::ForumModerate, None)
            .is_granted()
    );
}

pub(crate) async fn nested_groups_reach_accounts_through_the_closure<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = active(store, "alice").await;
    let outer = group(store, "outer").await;
    let inner = group(store, "inner").await;
    store
        .add_grant(
            &NewGrant {
                subject: Subject::Group(outer),
                granting: Granting::Role(Slug::TRUSTED),
                scope: Scope::Global,
            },
            Actor::System,
        )
        .await
        .unwrap();
    add(store, outer, Child::Group(inner)).await.unwrap();
    add(store, inner, Child::Account(alice.id)).await.unwrap();
    let snapshot = store.snapshot(Some(alice.id)).await.unwrap();
    let Decision::Granted(Reason(source)) = snapshot.resolve(Permission::PollsCreate, None) else {
        panic!("nested membership should reach the outer group's role");
    };
    assert_eq!(
        source,
        Source::Role {
            role: Slug::TRUSTED,
            rank: 20,
            via: Via::Group(Slug::parse("outer").unwrap())
        }
    );
    // Taking the inner group out of the outer one removes the role on the next snapshot.
    store
        .change_groups(
            GroupChange::Remove {
                group: outer,
                child: Child::Group(inner),
            },
            Actor::System,
        )
        .await
        .unwrap();
    let snapshot = store.snapshot(Some(alice.id)).await.unwrap();
    assert!(!snapshot.resolve(Permission::PollsCreate, None).is_granted());
    // Deleting a group removes its grants with it.
    add(store, outer, Child::Account(alice.id)).await.unwrap();
    assert!(
        store
            .snapshot(Some(alice.id))
            .await
            .unwrap()
            .resolve(Permission::PollsCreate, None)
            .is_granted()
    );
    store
        .change_groups(GroupChange::Delete(outer), Actor::System)
        .await
        .unwrap();
    assert!(
        !store
            .snapshot(Some(alice.id))
            .await
            .unwrap()
            .resolve(Permission::PollsCreate, None)
            .is_granted()
    );
}

pub(crate) async fn a_rejected_cycle_leaves_everything_unchanged<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = active(store, "alice").await;
    let one = group(store, "one").await;
    let two = group(store, "two").await;
    store
        .add_grant(
            &NewGrant {
                subject: Subject::Group(one),
                granting: Granting::Role(Slug::TRUSTED),
                scope: Scope::Global,
            },
            Actor::System,
        )
        .await
        .unwrap();
    add(store, one, Child::Group(two)).await.unwrap();
    add(store, two, Child::Account(alice.id)).await.unwrap();
    let before = store.snapshot(Some(alice.id)).await.unwrap();
    assert!(matches!(
        add(store, two, Child::Group(one)).await,
        Err(AccessError::Group(GroupError::Cycle))
    ));
    assert!(matches!(
        add(store, one, Child::Group(one)).await,
        Err(AccessError::Group(GroupError::Cycle))
    ));
    assert_eq!(store.snapshot(Some(alice.id)).await.unwrap(), before);
    // The edge that would have closed the cycle is not there: removing `one` from `two` is a
    // no-op, and `two` is still inside `one`.
    assert!(
        store
            .snapshot(Some(alice.id))
            .await
            .unwrap()
            .resolve(Permission::PollsCreate, None)
            .is_granted()
    );
    assert!(matches!(
        add(store, one, Child::Group(GroupId::from_raw(9_999))).await,
        Err(AccessError::Group(GroupError::UnknownGroup))
    ));
}

pub(crate) async fn nesting_deeper_than_the_cap_is_refused<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let mut ids = Vec::new();
    for n in 0..10 {
        ids.push(group(store, &format!("g{n}")).await);
    }
    for pair in ids.windows(2).take(7) {
        add(store, pair[0], Child::Group(pair[1])).await.unwrap();
    }
    // g0 > g1 > ... > g7 is eight groups: allowed. One more is not.
    assert!(matches!(
        add(store, ids[7], Child::Group(ids[8])).await,
        Err(AccessError::Group(GroupError::TooDeep))
    ));
}

pub(crate) async fn concurrent_opposite_nesting_makes_exactly_one_edge<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = active(store, "alice").await;
    for round in 0..8 {
        let left = group(store, &format!("left{round}")).await;
        let right = group(store, &format!("right{round}")).await;
        let (s1, s2) = (store.clone(), store.clone());
        let a = tokio::spawn(async move { add(&s1, left, Child::Group(right)).await });
        let b = tokio::spawn(async move { add(&s2, right, Child::Group(left)).await });
        let results = [a.await.unwrap(), b.await.unwrap()];
        let wins = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(wins, 1, "round {round}: {results:?}");
        assert!(
            results
                .iter()
                .any(|r| matches!(r, Err(AccessError::Group(GroupError::Cycle)))),
            "round {round}: {results:?}"
        );
    }
    // The closure is still consistent: nothing is its own ancestor, so a member added at the
    // bottom is seen by a snapshot.
    let tail = group(store, "tail").await;
    add(store, tail, Child::Account(alice.id)).await.unwrap();
    assert!(store.snapshot(Some(alice.id)).await.is_ok());
}

pub(crate) async fn a_scoped_role_grant_reaches_only_its_resource<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = active(store, "alice").await;
    store
        .add_grant(
            &NewGrant {
                subject: Subject::Account(alice.id),
                granting: Granting::Role(Slug::MODERATOR),
                scope: Scope::Resource(board(5)),
            },
            Actor::System,
        )
        .await
        .unwrap();
    let snapshot = store.snapshot(Some(alice.id)).await.unwrap();
    assert!(
        snapshot
            .resolve(Permission::ForumModerate, Some(board(5)))
            .is_granted()
    );
    assert!(
        !snapshot
            .resolve(Permission::ForumModerate, Some(board(6)))
            .is_granted()
    );
    assert!(
        !snapshot
            .resolve(Permission::ForumModerate, None)
            .is_granted()
    );
    assert_eq!(
        snapshot.reach(Permission::ForumModerate, ResourceKind::Forum),
        Reach::Only(vec![ResourceId::new(5).unwrap()])
    );
    assert_eq!(
        snapshot.reach(Permission::ForumModerate, ResourceKind::Room),
        Reach::None
    );
    // `member` is global, so list-style reach for forum.read is every board.
    assert_eq!(
        snapshot.reach(Permission::ForumRead, ResourceKind::Forum),
        Reach::All
    );
}

pub(crate) async fn allow_entries_name_accounts_and_groups<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = active(store, "alice").await;
    let bob = active(store, "bob").await;
    let lounge = group(store, "lounge").await;
    add(store, lounge, Child::Account(bob.id)).await.unwrap();
    // A private board: nobody holds forum.list on it through a role (member has it only
    // globally, which these two accounts do hold, so use a permission `member` lacks).
    let entry = |subject| NewGrant {
        subject,
        granting: Granting::Permission(Permission::ForumManage),
        scope: Scope::Resource(board(9)),
    };
    store
        .add_grant(&entry(Subject::Account(alice.id)), Actor::System)
        .await
        .unwrap();
    store
        .add_grant(&entry(Subject::Group(lounge)), Actor::System)
        .await
        .unwrap();
    let a = store.snapshot(Some(alice.id)).await.unwrap();
    let b = store.snapshot(Some(bob.id)).await.unwrap();
    for (snapshot, via) in [
        (&a, Via::Account),
        (&b, Via::Group(Slug::parse("lounge").unwrap())),
    ] {
        let Decision::Granted(Reason(source)) =
            snapshot.resolve(Permission::ForumManage, Some(board(9)))
        else {
            panic!("allow entry should grant");
        };
        assert_eq!(source, Source::Allow { via });
        assert!(
            !snapshot
                .resolve(Permission::ForumManage, Some(board(8)))
                .is_granted()
        );
        assert!(!snapshot.resolve(Permission::ForumManage, None).is_granted());
    }
    // A third member is not named.
    let carol = active(store, "carol").await;
    let c = store.snapshot(Some(carol.id)).await.unwrap();
    assert!(
        !c.resolve(Permission::ForumManage, Some(board(9)))
            .is_granted()
    );
}

pub(crate) async fn adding_a_grant_twice_is_not_an_error_and_adds_nothing<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = active(store, "alice").await;
    let grant = NewGrant {
        subject: Subject::Account(alice.id),
        granting: Granting::Role(Slug::TRUSTED),
        scope: Scope::Global,
    };
    store.add_grant(&grant, Actor::System).await.unwrap();
    let once = store.snapshot(Some(alice.id)).await.unwrap().held().len();
    store.add_grant(&grant, Actor::System).await.unwrap();
    assert_eq!(
        store.snapshot(Some(alice.id)).await.unwrap().held().len(),
        once
    );
}

pub(crate) async fn bad_grants_are_refused_by_name<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = active(store, "alice").await;
    let global = Scope::Global;
    assert!(matches!(
        store
            .add_grant(
                &NewGrant {
                    subject: Subject::Account(alice.id),
                    granting: Granting::Role(Slug::parse("nope").unwrap()),
                    scope: global,
                },
                Actor::System
            )
            .await,
        Err(AccessError::UnknownRole)
    ));
    assert!(matches!(
        store
            .add_grant(
                &NewGrant {
                    subject: Subject::Account(alice.id),
                    granting: Granting::Role(Slug::WAITING),
                    scope: global,
                },
                Actor::System
            )
            .await,
        Err(AccessError::RoleNotGrantable)
    ));
    assert!(matches!(
        store
            .add_grant(
                &NewGrant {
                    subject: Subject::Account(russh_bbs_core::store::AccountId::from_raw(9_999)),
                    granting: Granting::Role(Slug::TRUSTED),
                    scope: global,
                },
                Actor::System
            )
            .await,
        Err(AccessError::UnknownAccount)
    ));
    assert!(matches!(
        store
            .add_grant(
                &NewGrant {
                    subject: Subject::Group(GroupId::from_raw(9_999)),
                    granting: Granting::Permission(Permission::ForumRead),
                    scope: global,
                },
                Actor::System
            )
            .await,
        Err(AccessError::Group(GroupError::UnknownGroup))
    ));
    assert!(matches!(
        store.create_group(&Slug::EVERYONE, Actor::System).await,
        Err(AccessError::GroupNameTaken)
    ));
}

pub(crate) async fn access_data_survives_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let alice = active(h.store(), "alice").await;
    let g = group(h.store(), "lounge").await;
    add(h.store(), g, Child::Account(alice.id)).await.unwrap();
    h.store()
        .add_grant(
            &NewGrant {
                subject: Subject::Group(g),
                granting: Granting::Role(Slug::TRUSTED),
                scope: Scope::Global,
            },
            Actor::System,
        )
        .await
        .unwrap();
    h.reopen().await;
    let snapshot = h.store().snapshot(Some(alice.id)).await.unwrap();
    assert!(snapshot.resolve(Permission::PollsCreate, None).is_granted());
}

pub(crate) async fn a_broken_store_fails_the_snapshot_instead_of_granting<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = active(store, "alice").await;
    h.close().await;
    assert!(store.snapshot(Some(alice.id)).await.is_err());
    assert!(store.snapshot(None).await.is_err());
}

/// Keeps `TestStore` in the import list used by the generic helpers above.
#[allow(dead_code)]
fn _assert_test_store<S: TestStore>() {}
