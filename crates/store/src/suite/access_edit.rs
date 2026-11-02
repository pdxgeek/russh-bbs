//! The editing services (M2-B2) and the store writes under them, against any backend: roles put,
//! changed and deleted; grants removed; the rules end to end on the real role data; and each
//! write's audit row (D58), including that a change never lands without it.
#![allow(clippy::unwrap_used, clippy::panic)]

use russh_bbs_core::access::{
    AccessError, AccessStore, EditError, Granting, Kind, NewGrant, NewRole, Permission, ResourceId,
    ResourceKind, ResourceRef, RoleInfo, Scope, Slug, Snapshot, Subject, create_role, delete_role,
    grant_permission, grant_role, revoke_permission, revoke_role, set_role_permissions,
};
use russh_bbs_core::audit::{Action, Actor, AuditEntry, AuditQuery, AuditStore, Target};
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{Account, AccountState, AccountStore, NewAccount};

use super::{Harness, TestStore, fingerprint};

async fn member<S: AccountStore>(store: &S, name: &str) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

async fn founder<S: FounderStore>(store: &S) -> Account {
    store
        .claim_founder(sanitize_username("founder").unwrap(), &fingerprint(1))
        .await
        .unwrap()
        .unwrap()
}

fn slug(text: &str) -> Slug {
    Slug::parse(text).unwrap()
}

fn board(n: i64) -> Scope {
    Scope::Resource(ResourceRef {
        kind: ResourceKind::Forum,
        id: ResourceId::new(n).unwrap(),
    })
}

async fn log<S: AuditStore>(store: &S) -> Vec<AuditEntry> {
    let mut rows =
        russh_bbs_core::audit::known_events(store.page(&AuditQuery::default()).await.unwrap());
    rows.reverse(); // oldest first
    rows
}

async fn rows_of<S: AuditStore>(store: &S, actions: &[Action]) -> Vec<AuditEntry> {
    log(store)
        .await
        .into_iter()
        .filter(|row| actions.contains(&row.event.action))
        .collect()
}

async fn snap<S: AccessStore>(store: &S, account: &Account) -> Snapshot {
    store.snapshot(Some(account.id)).await.unwrap()
}

fn role_grant(subject: Subject, role: &Slug, scope: Scope) -> NewGrant {
    NewGrant {
        subject,
        granting: Granting::Role(role.clone()),
        scope,
    }
}

fn new_role(name: &str, rank: u8, permissions: &[Permission]) -> NewRole {
    NewRole {
        name: slug(name),
        rank,
        permissions: permissions.to_vec(),
    }
}

pub(crate) async fn a_role_is_put_changed_and_deleted_with_one_audit_row_each<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let by = Actor::Account(member(store, "pat").await.id);
    let curator = slug("curator");
    let made = RoleInfo {
        name: curator.clone(),
        rank: 25,
        kind: Kind::Custom,
        permissions: vec![Permission::ForumRead, Permission::FilesList],
    };
    store.put_role(&made, by).await.unwrap();
    let stored = store.role(&curator).await.unwrap().unwrap();
    assert_eq!((stored.rank, stored.kind), (25, Kind::Custom));
    let mut names: Vec<_> = stored.permissions.iter().map(|p| p.name()).collect();
    names.sort_unstable();
    assert_eq!(names, ["files.list", "forum.read"]);
    assert!(matches!(
        store.put_role(&made, by).await,
        Err(AccessError::RoleNameTaken)
    ));
    assert!(store.role(&slug("nonesuch")).await.unwrap().is_none());

    // Replace: drop files.list, add forum.post. The same set again is a no-op.
    let next = [Permission::ForumRead, Permission::ForumPost];
    store
        .set_role_permissions(&curator, &next, by)
        .await
        .unwrap();
    store
        .set_role_permissions(&curator, &next, by)
        .await
        .unwrap();
    let stored = store.role(&curator).await.unwrap().unwrap();
    let mut names: Vec<_> = stored.permissions.iter().map(|p| p.name()).collect();
    names.sort_unstable();
    assert_eq!(names, ["forum.post", "forum.read"]);
    assert!(matches!(
        store
            .set_role_permissions(&slug("nonesuch"), &next, by)
            .await,
        Err(AccessError::UnknownRole)
    ));

    store.delete_role(&curator, by).await.unwrap();
    assert!(store.role(&curator).await.unwrap().is_none());
    assert!(matches!(
        store.delete_role(&curator, by).await,
        Err(AccessError::UnknownRole)
    ));

    let rows = rows_of(
        store,
        &[
            Action::RoleCreated,
            Action::RolePermissionsSet,
            Action::RoleDeleted,
        ],
    )
    .await;
    let seen: Vec<_> = rows
        .iter()
        .map(|r| (r.event.action, r.event.detail.as_str().to_owned()))
        .collect();
    assert_eq!(
        seen,
        [
            (Action::RoleCreated, "rank 25 permissions 2".to_owned()),
            (Action::RolePermissionsSet, "added 1 removed 1".to_owned()),
            (Action::RoleDeleted, String::new()),
        ]
    );
    for row in &rows {
        assert_eq!(row.event.actor, by);
        assert_eq!(row.event.target, Target::Role(curator.clone()));
    }
}

pub(crate) async fn deleting_a_granted_role_is_refused_and_nothing_cascades<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let by = Actor::Account(member(store, "pat").await.id);
    let alice = member(store, "sam").await;
    let curator = slug("curator");
    let made = RoleInfo {
        name: curator.clone(),
        rank: 25,
        kind: Kind::Custom,
        permissions: vec![Permission::ForumRead],
    };
    store.put_role(&made, by).await.unwrap();
    let grant = role_grant(Subject::Account(alice.id), &curator, board(2));
    store.add_grant(&grant, by).await.unwrap();

    assert!(matches!(
        store.delete_role(&curator, by).await,
        Err(AccessError::RoleInUse)
    ));
    assert!(store.role(&curator).await.unwrap().is_some());
    assert!(
        snap(store, &alice)
            .await
            .held()
            .iter()
            .any(|held| held.scope == board(2) && held.permission == Permission::ForumRead),
        "the grant is still there"
    );
    assert_eq!(rows_of(store, &[Action::RoleDeleted]).await.len(), 0);

    store.remove_grant(&grant, by).await.unwrap();
    store.delete_role(&curator, by).await.unwrap();
}

pub(crate) async fn removing_a_grant_writes_a_row_and_removing_nothing_writes_none<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let by = Actor::Account(member(store, "pat").await.id);
    let alice = member(store, "sam").await;
    let subject = Subject::Account(alice.id);
    let role_scoped = role_grant(subject, &Slug::MODERATOR, board(4));
    let allow = NewGrant {
        subject,
        granting: Granting::Permission(Permission::FilesList),
        scope: Scope::Global,
    };
    store.add_grant(&role_scoped, by).await.unwrap();
    store.add_grant(&allow, by).await.unwrap();

    store.remove_grant(&role_scoped, by).await.unwrap();
    store.remove_grant(&role_scoped, by).await.unwrap(); // already gone
    // The same role granted globally is a different grant and is not touched.
    store
        .remove_grant(&role_grant(subject, &Slug::MODERATOR, Scope::Global), by)
        .await
        .unwrap();
    store.remove_grant(&allow, by).await.unwrap();

    let rows = rows_of(store, &[Action::RoleRevoked, Action::PermissionRevoked]).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].event.action, Action::RoleRevoked);
    assert_eq!(rows[0].event.target, Target::Account(alice.id));
    assert_eq!(rows[0].event.resource.map(|r| r.id.as_raw()), Some(4));
    assert_eq!(rows[0].event.detail.as_str(), "role moderator");
    assert_eq!(rows[1].event.action, Action::PermissionRevoked);
    assert_eq!(rows[1].event.detail.as_str(), "permission files.list");
    // Alice is back to what everyone gets: no scoped grant and no allow entry.
    let held = snap(store, &alice).await;
    assert!(held.held().iter().all(|h| h.scope == Scope::Global
        && matches!(&h.source, russh_bbs_core::access::Source::Role { .. })));
}

pub(crate) async fn the_last_sysop_grant_cannot_be_removed_even_by_two_at_once<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let first = founder(store).await;
    let second = member(store, "sam").await;
    let by = Actor::Account(first.id);
    let one = role_grant(Subject::Account(first.id), &Slug::SYSOP, Scope::Global);
    let two = role_grant(Subject::Account(second.id), &Slug::SYSOP, Scope::Global);

    // Alone, the founder's grant stays.
    assert!(matches!(
        store.remove_grant(&one, by).await,
        Err(AccessError::LastSysop)
    ));
    // With a second one, exactly one of two simultaneous revokes may succeed.
    store.add_grant(&two, by).await.unwrap();
    let (a, b) = tokio::join!(store.remove_grant(&one, by), store.remove_grant(&two, by));
    let outcomes = [a, b];
    assert_eq!(
        outcomes.iter().filter(|r| r.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    assert!(
        outcomes
            .iter()
            .any(|r| matches!(r, Err(AccessError::LastSysop)))
    );
    let sysops = rows_of(store, &[Action::RoleRevoked]).await;
    assert_eq!(sysops.len(), 1);
    let left = [snap(store, &first).await, snap(store, &second).await]
        .iter()
        .filter(|s| s.top_role() == Some(&Slug::SYSOP))
        .count();
    assert_eq!(left, 1, "one sysop remains");
}

/// A board with its founder, a `cosysop` (`pat`) and `sam`, a `lead`: rank 35, holding
/// `roles.edit` and `forum.read` only.
struct Cast {
    boss: Account,
    boss_snap: Snapshot,
    pat: Account,
    sam: Account,
    lead: Snapshot,
}

async fn cast<S: TestStore>(store: &S) -> Cast {
    let boss = founder(store).await;
    let pat = member(store, "pat").await;
    let sam = member(store, "sam").await;
    let boss_snap = snap(store, &boss).await;

    // Plain members and cosysops (no roles.edit) are refused outright.
    let denied = create_role(store, &snap(store, &pat).await, new_role("x", 5, &[])).await;
    assert!(matches!(denied, Err(EditError::Denied(_))), "{denied:?}");
    grant_role(
        store,
        &boss_snap,
        Subject::Account(pat.id),
        &Slug::COSYSOP,
        Scope::Global,
    )
    .await
    .unwrap();
    let cosysop = snap(store, &pat).await;
    assert!(matches!(
        create_role(store, &cosysop, new_role("x", 5, &[])).await,
        Err(EditError::Denied(_))
    ));

    // The sysop creates a role that holds roles.edit and a little else, at rank 35.
    create_role(
        store,
        &boss_snap,
        new_role("lead", 35, &[Permission::RolesEdit, Permission::ForumRead]),
    )
    .await
    .unwrap();
    grant_role(
        store,
        &boss_snap,
        Subject::Account(sam.id),
        &slug("lead"),
        Scope::Global,
    )
    .await
    .unwrap();
    let lead = snap(store, &sam).await;
    assert_eq!(lead.top_rank(), Some(35));
    Cast {
        boss,
        boss_snap,
        pat,
        sam,
        lead,
    }
}

pub(crate) async fn rank_and_locked_roles_hold_on_the_real_role_data<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let Cast {
        boss_snap, lead, ..
    } = cast(store).await;

    // Rank: below 35 only. Locked and built-in roles above are out of reach.
    assert!(matches!(
        create_role(store, &lead, new_role("peer", 35, &[])).await,
        Err(EditError::RankOutOfRange { .. })
    ));
    assert!(matches!(
        set_role_permissions(store, &lead, &Slug::COSYSOP, vec![]).await,
        Err(EditError::RankNotBelow { .. })
    ));
    assert!(matches!(
        set_role_permissions(store, &boss_snap, &Slug::SYSOP, vec![]).await,
        Err(EditError::RoleLocked { .. })
    ));
    assert!(matches!(
        delete_role(store, &boss_snap, &Slug::MEMBER).await,
        Err(EditError::RoleBuiltin { .. })
    ));
}

pub(crate) async fn escalation_is_refused_on_the_real_role_data<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let Cast { pat, lead, .. } = cast(store).await;

    // No escalation: `lead` holds roles.edit and forum.read, not files.approve.
    assert!(matches!(
        set_role_permissions(
            store,
            &lead,
            &Slug::MEMBER,
            vec![Permission::ForumRead, Permission::FilesApprove]
        )
        .await,
        Err(EditError::Escalation {
            permission: Permission::FilesApprove
        })
    ));
    assert!(matches!(
        grant_permission(
            store,
            &lead,
            Subject::Account(pat.id),
            Permission::FilesApprove,
            Scope::Global
        )
        .await,
        Err(EditError::Escalation { .. })
    ));
    assert!(matches!(
        grant_role(
            store,
            &lead,
            Subject::Account(pat.id),
            &Slug::MODERATOR,
            Scope::Global
        )
        .await,
        Err(EditError::Escalation { .. })
    ));
    // ...but it may hand out what it holds, on one board, and take it back.
    grant_permission(
        store,
        &lead,
        Subject::Account(pat.id),
        Permission::ForumRead,
        board(7),
    )
    .await
    .unwrap();
    revoke_permission(
        store,
        &lead,
        Subject::Account(pat.id),
        Permission::ForumRead,
        board(7),
    )
    .await
    .unwrap();
}

pub(crate) async fn protected_grants_hold_on_the_real_role_data<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let Cast {
        boss,
        boss_snap,
        sam,
        ..
    } = cast(store).await;

    // The everyone group keeps member; the last sysop grant stays; a second sysop frees the first.
    let everyone = store
        .groups()
        .await
        .unwrap()
        .into_iter()
        .find(|g| g.name == Slug::EVERYONE)
        .unwrap();
    assert!(matches!(
        revoke_role(
            store,
            &boss_snap,
            Subject::Group(everyone.id),
            &Slug::MEMBER,
            Scope::Global
        )
        .await,
        Err(EditError::ProtectedGrant)
    ));
    assert!(matches!(
        revoke_role(
            store,
            &boss_snap,
            Subject::Account(boss.id),
            &Slug::SYSOP,
            Scope::Global
        )
        .await,
        Err(EditError::Access(AccessError::LastSysop))
    ));
    grant_role(
        store,
        &boss_snap,
        Subject::Account(sam.id),
        &Slug::SYSOP,
        Scope::Global,
    )
    .await
    .unwrap();
    revoke_role(
        store,
        &boss_snap,
        Subject::Account(boss.id),
        &Slug::SYSOP,
        Scope::Global,
    )
    .await
    .unwrap();
}

pub(crate) async fn a_revoked_role_stops_working_on_the_next_snapshot<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let boss = founder(store).await;
    let pat = member(store, "pat").await;
    let boss_snap = snap(store, &boss).await;
    grant_role(
        store,
        &boss_snap,
        Subject::Account(pat.id),
        &Slug::MODERATOR,
        Scope::Global,
    )
    .await
    .unwrap();
    assert!(
        snap(store, &pat)
            .await
            .resolve(Permission::ForumModerate, None)
            .is_granted()
    );
    revoke_role(
        store,
        &boss_snap,
        Subject::Account(pat.id),
        &Slug::MODERATOR,
        Scope::Global,
    )
    .await
    .unwrap();
    assert!(
        !snap(store, &pat)
            .await
            .resolve(Permission::ForumModerate, None)
            .is_granted()
    );

    // A role edit is seen the same way: take a permission out of `member`, then put it back.
    let before = snap(store, &pat).await;
    assert!(before.resolve(Permission::ForumPost, None).is_granted());
    let member_set = store
        .role(&Slug::MEMBER)
        .await
        .unwrap()
        .unwrap()
        .permissions;
    let without: Vec<_> = member_set
        .iter()
        .copied()
        .filter(|&p| p != Permission::ForumPost)
        .collect();
    set_role_permissions(store, &boss_snap, &Slug::MEMBER, without)
        .await
        .unwrap();
    assert!(
        !snap(store, &pat)
            .await
            .resolve(Permission::ForumPost, None)
            .is_granted()
    );
    set_role_permissions(store, &boss_snap, &Slug::MEMBER, member_set)
        .await
        .unwrap();
    assert!(
        snap(store, &pat)
            .await
            .resolve(Permission::ForumPost, None)
            .is_granted()
    );
}

pub(crate) async fn refused_edits_write_no_audit_rows<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let boss = founder(store).await;
    let pat = member(store, "pat").await;
    let boss_snap = snap(store, &boss).await;
    let before = log(store).await.len();
    let _ = create_role(store, &snap(store, &pat).await, new_role("x", 5, &[])).await;
    let _ = set_role_permissions(store, &boss_snap, &Slug::SYSOP, vec![]).await;
    let _ = delete_role(store, &boss_snap, &Slug::MEMBER).await;
    let _ = revoke_role(
        store,
        &boss_snap,
        Subject::Account(boss.id),
        &Slug::SYSOP,
        Scope::Global,
    )
    .await;
    let _ = grant_role(
        store,
        &boss_snap,
        Subject::Account(pat.id),
        &Slug::WAITING,
        Scope::Global,
    )
    .await;
    assert_eq!(log(store).await.len(), before);
}

pub(crate) async fn an_edit_never_lands_without_its_audit_row<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let boss = founder(store).await;
    let by = Actor::Account(boss.id);
    let alice = member(store, "sam").await;
    let curator = slug("curator");
    let made = RoleInfo {
        name: curator.clone(),
        rank: 25,
        kind: Kind::Custom,
        permissions: vec![Permission::ForumRead],
    };
    store.put_role(&made, by).await.unwrap();
    let grant = role_grant(Subject::Account(alice.id), &Slug::MODERATOR, Scope::Global);
    store.add_grant(&grant, by).await.unwrap();
    let rows_before = log(store).await.len();

    h.break_audit_inserts().await;

    let other = RoleInfo {
        name: slug("other"),
        ..made.clone()
    };
    assert!(store.put_role(&other, by).await.is_err());
    assert!(
        store.role(&slug("other")).await.unwrap().is_none(),
        "no half-made role"
    );
    assert!(
        store
            .set_role_permissions(&curator, &[Permission::FilesList], by)
            .await
            .is_err()
    );
    assert_eq!(
        store.role(&curator).await.unwrap().unwrap().permissions,
        [Permission::ForumRead]
    );
    assert!(store.delete_role(&curator, by).await.is_err());
    assert!(
        store.role(&curator).await.unwrap().is_some(),
        "the role is still there"
    );
    assert!(store.remove_grant(&grant, by).await.is_err());
    assert!(
        snap(store, &alice)
            .await
            .resolve(Permission::ForumModerate, None)
            .is_granted(),
        "the grant is still there"
    );
    assert_eq!(log(store).await.len(), rows_before);
}

pub(crate) async fn role_edits_survive_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let by = Actor::Account(member(h.store(), "pat").await.id);
    let curator = slug("curator");
    let made = RoleInfo {
        name: curator.clone(),
        rank: 25,
        kind: Kind::Custom,
        permissions: vec![Permission::ForumRead],
    };
    h.store().put_role(&made, by).await.unwrap();
    h.reopen().await;
    assert_eq!(h.store().role(&curator).await.unwrap(), Some(made));
    assert_eq!(rows_of(h.store(), &[Action::RoleCreated]).await.len(), 1);
}

#[allow(dead_code)]
fn assert_store<S: TestStore>() {}
