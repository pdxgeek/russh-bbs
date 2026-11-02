//! Rules of the editing services against an in-memory store that mirrors the real stores'
//! behaviour (a fake, not a mock: it keeps state and enforces the same two store-side rules).
#![allow(clippy::unwrap_used, clippy::panic, clippy::unused_async_trait_impl)]

use std::sync::Mutex;

use super::{
    AccessError, AccessStore, EditError, Granting, GroupChange, GroupId, GroupInfo, Kind, NewGrant,
    NewRole, Permission, ResourceId, ResourceKind, ResourceRef, RoleInfo, Scope, Slug, Snapshot,
    Subject, Via, create_role, delete_role, grant_permission, grant_role, revoke_permission,
    revoke_role, set_role_permissions,
};
use crate::audit::Actor;
use crate::store::{AccountId, StoreError};

#[derive(Default)]
struct State {
    roles: Vec<RoleInfo>,
    grants: Vec<NewGrant>,
    groups: Vec<GroupInfo>,
    writes: usize,
}

struct Fake(Mutex<State>);

fn role(name: &Slug, rank: u8, kind: Kind, permissions: &[Permission]) -> RoleInfo {
    RoleInfo {
        name: name.clone(),
        rank,
        kind,
        permissions: permissions.to_vec(),
    }
}

fn slug(text: &str) -> Slug {
    Slug::parse(text).unwrap()
}

impl Fake {
    fn new() -> Self {
        use Permission::{ForumModerate, ForumPost, ForumRead, GroupsEdit, RolesEdit};
        let state = State {
            roles: vec![
                role(&Slug::WAITING, 0, Kind::Locked, &[Permission::WaitingView]),
                role(&Slug::MEMBER, 10, Kind::Builtin, &[ForumRead, ForumPost]),
                role(
                    &Slug::MODERATOR,
                    30,
                    Kind::Builtin,
                    &[ForumRead, ForumPost, ForumModerate],
                ),
                role(
                    &Slug::COSYSOP,
                    40,
                    Kind::Builtin,
                    &[ForumRead, ForumPost, ForumModerate, GroupsEdit],
                ),
                role(&Slug::SYSOP, 100, Kind::Locked, Permission::ALL),
                // A custom role that may edit roles but holds little else.
                role(&slug("manager"), 50, Kind::Custom, &[RolesEdit, GroupsEdit]),
            ],
            grants: vec![],
            groups: vec![
                GroupInfo {
                    id: GroupId::from_raw(1),
                    name: Slug::EVERYONE,
                    kind: Kind::Locked,
                },
                GroupInfo {
                    id: GroupId::from_raw(2),
                    name: slug("lounge"),
                    kind: Kind::Custom,
                },
            ],
            writes: 0,
        };
        Self(Mutex::new(state))
    }

    fn writes(&self) -> usize {
        self.0.lock().unwrap().writes
    }

    fn grants(&self) -> Vec<NewGrant> {
        self.0.lock().unwrap().grants.clone()
    }

    fn role(&self, name: &Slug) -> Option<RoleInfo> {
        let state = self.0.lock().unwrap();
        state.roles.iter().find(|r| r.name == *name).cloned()
    }
}

impl AccessStore for Fake {
    async fn snapshot(&self, _: Option<AccountId>) -> Result<Snapshot, StoreError> {
        Ok(Snapshot::empty())
    }
    async fn roles(&self) -> Result<Vec<RoleInfo>, StoreError> {
        Ok(self.0.lock().unwrap().roles.clone())
    }
    async fn groups(&self) -> Result<Vec<GroupInfo>, StoreError> {
        Ok(self.0.lock().unwrap().groups.clone())
    }
    async fn create_group(&self, _: &Slug, _: Actor) -> Result<GroupId, AccessError> {
        Err(AccessError::UnknownRole)
    }
    async fn change_groups(&self, _: GroupChange, _: Actor) -> Result<(), AccessError> {
        Ok(())
    }
    async fn put_role(&self, new: &RoleInfo, _: Actor) -> Result<(), AccessError> {
        let mut state = self.0.lock().unwrap();
        if state.roles.iter().any(|r| r.name == new.name) {
            return Err(AccessError::RoleNameTaken);
        }
        state.roles.push(new.clone());
        state.writes += 1;
        Ok(())
    }
    async fn set_role_permissions(
        &self,
        name: &Slug,
        permissions: &[Permission],
        _: Actor,
    ) -> Result<(), AccessError> {
        let mut state = self.0.lock().unwrap();
        let found = state.roles.iter_mut().find(|r| r.name == *name);
        let found = found.ok_or(AccessError::UnknownRole)?;
        found.permissions = permissions.to_vec();
        state.writes += 1;
        Ok(())
    }
    async fn delete_role(&self, name: &Slug, _: Actor) -> Result<(), AccessError> {
        let mut state = self.0.lock().unwrap();
        if state
            .grants
            .iter()
            .any(|g| matches!(&g.granting, Granting::Role(r) if r == name))
        {
            return Err(AccessError::RoleInUse);
        }
        state.roles.retain(|r| r.name != *name);
        state.writes += 1;
        Ok(())
    }
    async fn add_grant(&self, grant: &NewGrant, _: Actor) -> Result<(), AccessError> {
        let mut state = self.0.lock().unwrap();
        if !state.grants.contains(grant) {
            state.grants.push(grant.clone());
            state.writes += 1;
        }
        Ok(())
    }
    async fn remove_grant(&self, grant: &NewGrant, _: Actor) -> Result<(), AccessError> {
        let mut state = self.0.lock().unwrap();
        let before = state.grants.len();
        let is_sysop = matches!(&grant.granting, Granting::Role(r) if *r == Slug::SYSOP)
            && grant.scope == Scope::Global;
        let sysops = state
            .grants
            .iter()
            .filter(|g| {
                matches!(&g.granting, Granting::Role(r) if *r == Slug::SYSOP)
                    && g.scope == Scope::Global
            })
            .count();
        if is_sysop && sysops == 1 && state.grants.contains(grant) {
            return Err(AccessError::LastSysop);
        }
        state.grants.retain(|g| g != grant);
        if state.grants.len() != before {
            state.writes += 1;
        }
        Ok(())
    }
}

/// An actor account 1 holding `name` (rank `rank`) globally with `permissions`.
fn actor(name: &Slug, rank: u8, permissions: &[Permission]) -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(1)),
        name,
        rank,
        &Via::Account,
        permissions.iter().copied(),
    )
}

fn sysop() -> Snapshot {
    actor(&Slug::SYSOP, 100, Permission::ALL)
}

/// Rank 50 with `roles.edit`, `groups.edit` and two forum permissions.
fn manager() -> Snapshot {
    use Permission::{ForumPost, ForumRead, GroupsEdit, RolesEdit};
    actor(
        &slug("manager"),
        50,
        &[RolesEdit, GroupsEdit, ForumRead, ForumPost],
    )
}

fn account(n: i64) -> Subject {
    Subject::Account(AccountId::from_raw(n))
}

fn lounge() -> Subject {
    Subject::Group(GroupId::from_raw(2))
}

fn everyone() -> Subject {
    Subject::Group(GroupId::from_raw(1))
}

fn board() -> Scope {
    Scope::Resource(ResourceRef {
        kind: ResourceKind::Forum,
        id: ResourceId::new(3).unwrap(),
    })
}

fn new_role(name: &str, rank: u8, permissions: &[Permission]) -> NewRole {
    NewRole {
        name: slug(name),
        rank,
        permissions: permissions.to_vec(),
    }
}

#[tokio::test]
async fn every_service_is_refused_without_roles_edit_and_writes_nothing() {
    let store = Fake::new();
    let nobody = actor(
        &Slug::MODERATOR,
        30,
        &[Permission::ForumRead, Permission::GroupsEdit],
    );
    let denied =
        |r: Result<(), EditError>| assert!(matches!(r, Err(EditError::Denied(_))), "{r:?}");
    denied(create_role(&store, &nobody, new_role("x", 5, &[])).await);
    denied(set_role_permissions(&store, &nobody, &Slug::MEMBER, vec![]).await);
    denied(delete_role(&store, &nobody, &slug("manager")).await);
    denied(grant_role(&store, &nobody, account(2), &Slug::MEMBER, Scope::Global).await);
    denied(revoke_role(&store, &nobody, account(2), &Slug::MEMBER, Scope::Global).await);
    denied(
        grant_permission(
            &store,
            &nobody,
            account(2),
            Permission::ForumRead,
            Scope::Global,
        )
        .await,
    );
    denied(
        revoke_permission(
            &store,
            &nobody,
            account(2),
            Permission::ForumRead,
            Scope::Global,
        )
        .await,
    );
    assert_eq!(store.writes(), 0);
}

#[tokio::test]
async fn a_session_with_no_account_is_not_signed_in_even_holding_the_permission() {
    let store = Fake::new();
    let ghost = Snapshot::from_role(
        None,
        &Slug::SYSOP,
        100,
        &Via::Waiting,
        Permission::ALL.iter().copied(),
    );
    let result = create_role(&store, &ghost, new_role("x", 5, &[])).await;
    assert!(matches!(result, Err(EditError::NotSignedIn)), "{result:?}");
}

#[tokio::test]
async fn a_role_is_created_below_the_actors_rank_and_never_at_or_above() {
    let store = Fake::new();
    let actor = manager(); // rank 50
    create_role(
        &store,
        &actor,
        new_role("curator", 49, &[Permission::ForumRead]),
    )
    .await
    .unwrap();
    let made = store.role(&slug("curator")).unwrap();
    assert_eq!((made.rank, made.kind), (49, Kind::Custom));
    for rank in [0, 50, 51, 100] {
        let result = create_role(&store, &actor, new_role("other", rank, &[])).await;
        assert!(
            matches!(result, Err(EditError::RankOutOfRange { rank: r }) if r == rank),
            "rank {rank}: {result:?}"
        );
    }
    assert_eq!(store.writes(), 1);
}

#[tokio::test]
async fn a_permission_goes_into_a_role_only_if_the_actor_holds_it() {
    let store = Fake::new();
    let actor = manager();
    let result = create_role(
        &store,
        &actor,
        new_role("curator", 20, &[Permission::FilesApprove]),
    )
    .await;
    assert!(
        matches!(
            result,
            Err(EditError::Escalation {
                permission: Permission::FilesApprove
            })
        ),
        "{result:?}"
    );
    assert!(store.role(&slug("curator")).is_none());
}

#[tokio::test]
async fn creating_over_an_existing_name_is_refused_and_a_locked_name_says_locked() {
    let store = Fake::new();
    let actor = manager();
    let taken = create_role(&store, &actor, new_role("manager", 20, &[])).await;
    assert!(
        matches!(taken, Err(EditError::Access(AccessError::RoleNameTaken))),
        "{taken:?}"
    );
    let locked = create_role(&store, &actor, new_role("sysop", 20, &[])).await;
    assert!(
        matches!(locked, Err(EditError::RoleLocked { .. })),
        "{locked:?}"
    );
}

#[tokio::test]
async fn changing_a_role_needs_rank_below_and_refuses_locked_roles() {
    let store = Fake::new();
    let actor = manager(); // rank 50
    // member (10) is below: removing a permission works.
    set_role_permissions(&store, &actor, &Slug::MEMBER, vec![Permission::ForumRead])
        .await
        .unwrap();
    assert_eq!(
        store.role(&Slug::MEMBER).unwrap().permissions,
        [Permission::ForumRead]
    );
    // The actor's own role (50) and above are refused, whatever the permissions.
    let own = set_role_permissions(&store, &actor, &slug("manager"), vec![]).await;
    assert!(
        matches!(own, Err(EditError::RankNotBelow { .. })),
        "{own:?}"
    );
    // Locked roles are refused even for the sysop.
    for locked in [Slug::SYSOP, Slug::WAITING] {
        let result = set_role_permissions(&store, &sysop(), &locked, vec![]).await;
        assert!(
            matches!(result, Err(EditError::RoleLocked { .. })),
            "{locked}: {result:?}"
        );
    }
    let unknown = set_role_permissions(&store, &actor, &slug("nonesuch"), vec![]).await;
    assert!(
        matches!(unknown, Err(EditError::Access(AccessError::UnknownRole))),
        "{unknown:?}"
    );
}

#[tokio::test]
async fn editing_a_role_cannot_add_what_the_actor_lacks_but_may_keep_or_remove_it() {
    let store = Fake::new();
    let actor = manager(); // lacks forum.moderate
    // Adding it is escalation.
    let add = set_role_permissions(
        &store,
        &actor,
        &Slug::MEMBER,
        vec![Permission::ForumRead, Permission::ForumModerate],
    )
    .await;
    assert!(
        matches!(
            add,
            Err(EditError::Escalation {
                permission: Permission::ForumModerate
            })
        ),
        "{add:?}"
    );
    // moderator (30) already holds it: keeping it while removing another is fine, and so is
    // removing it.
    set_role_permissions(
        &store,
        &actor,
        &Slug::MODERATOR,
        vec![Permission::ForumRead, Permission::ForumModerate],
    )
    .await
    .unwrap();
    set_role_permissions(
        &store,
        &actor,
        &Slug::MODERATOR,
        vec![Permission::ForumRead],
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn only_custom_unlocked_roles_below_rank_that_are_not_granted_can_be_deleted() {
    let store = Fake::new();
    let actor = manager();
    create_role(&store, &actor, new_role("curator", 20, &[]))
        .await
        .unwrap();
    let builtin = delete_role(&store, &actor, &Slug::MEMBER).await;
    assert!(
        matches!(builtin, Err(EditError::RoleBuiltin { .. })),
        "{builtin:?}"
    );
    let locked = delete_role(&store, &sysop(), &Slug::SYSOP).await;
    assert!(
        matches!(locked, Err(EditError::RoleLocked { .. })),
        "{locked:?}"
    );
    // Own rank: refused.
    let own = delete_role(&store, &actor, &slug("manager")).await;
    assert!(
        matches!(own, Err(EditError::RankNotBelow { .. })),
        "{own:?}"
    );
    // Granted: refused until revoked.
    grant_role(&store, &actor, account(2), &slug("curator"), Scope::Global)
        .await
        .unwrap();
    let granted = delete_role(&store, &actor, &slug("curator")).await;
    assert!(
        matches!(granted, Err(EditError::Access(AccessError::RoleInUse))),
        "{granted:?}"
    );
    revoke_role(&store, &actor, account(2), &slug("curator"), Scope::Global)
        .await
        .unwrap();
    delete_role(&store, &actor, &slug("curator")).await.unwrap();
    assert!(store.role(&slug("curator")).is_none());
}

#[tokio::test]
async fn granting_a_role_follows_the_rank_rule_and_never_grants_waiting() {
    let store = Fake::new();
    let actor = manager(); // rank 50; holds roles.edit, groups.edit, forum.read, forum.post
    grant_role(&store, &actor, account(2), &Slug::MEMBER, Scope::Global)
        .await
        .unwrap();
    // cosysop is rank 40, below 50, but holds forum.moderate which the actor lacks.
    let escalate = grant_role(&store, &actor, account(2), &Slug::COSYSOP, Scope::Global).await;
    assert!(
        matches!(escalate, Err(EditError::Escalation { .. })),
        "{escalate:?}"
    );
    // At or above the actor's rank.
    let above = grant_role(&store, &actor, account(2), &slug("manager"), Scope::Global).await;
    assert!(
        matches!(above, Err(EditError::RankNotBelow { .. })),
        "{above:?}"
    );
    let waiting = grant_role(&store, &sysop(), account(2), &Slug::WAITING, Scope::Global).await;
    assert!(
        matches!(
            waiting,
            Err(EditError::Access(AccessError::RoleNotGrantable))
        ),
        "{waiting:?}"
    );
    assert_eq!(store.grants().len(), 1);
}

#[tokio::test]
async fn a_sysop_may_grant_and_revoke_sysop_and_no_one_else_may() {
    let store = Fake::new();
    grant_role(&store, &sysop(), account(2), &Slug::SYSOP, Scope::Global)
        .await
        .unwrap();
    grant_role(&store, &sysop(), account(3), &Slug::SYSOP, Scope::Global)
        .await
        .unwrap();
    revoke_role(&store, &sysop(), account(3), &Slug::SYSOP, Scope::Global)
        .await
        .unwrap();
    let other = grant_role(&store, &manager(), account(3), &Slug::SYSOP, Scope::Global).await;
    assert!(
        matches!(other, Err(EditError::RankNotBelow { .. })),
        "{other:?}"
    );
}

#[tokio::test]
async fn the_last_sysop_grant_cannot_be_revoked() {
    let store = Fake::new();
    grant_role(&store, &sysop(), account(1), &Slug::SYSOP, Scope::Global)
        .await
        .unwrap();
    let last = revoke_role(&store, &sysop(), account(1), &Slug::SYSOP, Scope::Global).await;
    assert!(
        matches!(last, Err(EditError::Access(AccessError::LastSysop))),
        "{last:?}"
    );
    assert_eq!(store.grants().len(), 1);
    // A scoped sysop grant does not count as another one.
    grant_role(&store, &sysop(), account(2), &Slug::SYSOP, board())
        .await
        .unwrap();
    let still = revoke_role(&store, &sysop(), account(1), &Slug::SYSOP, Scope::Global).await;
    assert!(
        matches!(still, Err(EditError::Access(AccessError::LastSysop))),
        "{still:?}"
    );
}

#[tokio::test]
async fn everyones_member_grant_cannot_be_revoked_but_other_groups_can() {
    let store = Fake::new();
    let result = revoke_role(&store, &sysop(), everyone(), &Slug::MEMBER, Scope::Global).await;
    assert!(
        matches!(result, Err(EditError::ProtectedGrant)),
        "{result:?}"
    );
    grant_role(&store, &sysop(), lounge(), &Slug::MEMBER, Scope::Global)
        .await
        .unwrap();
    revoke_role(&store, &sysop(), lounge(), &Slug::MEMBER, Scope::Global)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_grant_to_a_group_also_needs_groups_edit() {
    let store = Fake::new();
    let roles_only = actor(
        &slug("manager"),
        50,
        &[
            Permission::RolesEdit,
            Permission::ForumRead,
            Permission::ForumPost,
        ],
    );
    let result = grant_role(&store, &roles_only, lounge(), &Slug::MEMBER, Scope::Global).await;
    assert!(
        matches!(
            result,
            Err(EditError::Denied(Denied {
                permission: Permission::GroupsEdit
            }))
        ),
        "{result:?}"
    );
    grant_role(
        &store,
        &roles_only,
        account(2),
        &Slug::MEMBER,
        Scope::Global,
    )
    .await
    .unwrap();
}

use super::Denied;

#[tokio::test]
async fn an_allow_entry_needs_the_permission_held_where_it_applies() {
    let store = Fake::new();
    let actor = manager();
    grant_permission(&store, &actor, account(2), Permission::ForumRead, board())
        .await
        .unwrap();
    let lacking = grant_permission(
        &store,
        &actor,
        account(2),
        Permission::ForumModerate,
        board(),
    )
    .await;
    assert!(
        matches!(
            lacking,
            Err(EditError::Escalation {
                permission: Permission::ForumModerate
            })
        ),
        "{lacking:?}"
    );
    // Holding it on one board is not holding it everywhere.
    let one_board = Snapshot::new(
        Some(AccountId::from_raw(1)),
        [Permission::RolesEdit]
            .into_iter()
            .map(|permission| super::Held {
                permission,
                scope: Scope::Global,
                source: super::Source::Role {
                    role: slug("manager"),
                    rank: 50,
                    via: Via::Account,
                },
            })
            .chain(std::iter::once(super::Held {
                permission: Permission::ForumModerate,
                scope: board(),
                source: super::Source::Allow { via: Via::Account },
            }))
            .collect(),
    );
    grant_permission(
        &store,
        &one_board,
        account(2),
        Permission::ForumModerate,
        board(),
    )
    .await
    .unwrap();
    let wider = grant_permission(
        &store,
        &one_board,
        account(2),
        Permission::ForumModerate,
        Scope::Global,
    )
    .await;
    assert!(
        matches!(wider, Err(EditError::Escalation { .. })),
        "{wider:?}"
    );
    revoke_permission(&store, &actor, account(2), Permission::ForumRead, board())
        .await
        .unwrap();
}

#[tokio::test]
async fn repeating_a_grant_or_a_revoke_changes_nothing() {
    let store = Fake::new();
    let actor = manager();
    grant_role(&store, &actor, account(2), &Slug::MEMBER, Scope::Global)
        .await
        .unwrap();
    grant_role(&store, &actor, account(2), &Slug::MEMBER, Scope::Global)
        .await
        .unwrap();
    revoke_role(&store, &actor, account(2), &Slug::MEMBER, Scope::Global)
        .await
        .unwrap();
    revoke_role(&store, &actor, account(2), &Slug::MEMBER, Scope::Global)
        .await
        .unwrap();
    assert_eq!(store.writes(), 2);
}

#[test]
fn top_rank_counts_only_global_roles() {
    let global = actor(&Slug::MODERATOR, 30, &[Permission::ForumRead]);
    assert_eq!(global.top_rank(), Some(30));
    assert_eq!(Snapshot::empty().top_rank(), None);
    let scoped = Snapshot::new(
        None,
        vec![super::Held {
            permission: Permission::ForumRead,
            scope: board(),
            source: super::Source::Role {
                role: Slug::SYSOP,
                rank: 100,
                via: Via::Account,
            },
        }],
    );
    assert_eq!(scoped.top_rank(), None);
}
