//! The profile and member-list actions. Each takes the [`Granted`] proof that
//! [`Snapshot::require`] returns, so a front end cannot call one without passing `core::access`
//! (docs/access.md, "Rules"). A proof for the wrong permission is refused as well.
//!
//! * [`list_members`] and [`view_profile`] need `members.view`.
//! * [`edit_profile`] needs `account.edit_profile` for the member's own profile and
//!   `members.edit` for anyone else's.
//!
//! A target that is absent and a target that is not an active member are the same answer
//! ([`ProfileError::NotFound`]), so a pending or suspended account cannot be probed.

use crate::access::{Granted, Permission, PermissionCheck, Snapshot};
use crate::audit::{Actor, events};
use crate::store::{AccountId, StoreError};

use super::fields::{Field, Profile, RawProfile};
use super::store::{Cursor, MemberQuery, MemberRecord, MemberSummary, PAGE_SIZE, ProfileStore};

/// Why an action did not happen.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// The proof is for a different permission, or for editing someone it does not cover.
    #[error("not permitted")]
    NotPermitted,
    /// The target's top rank is not strictly below the editor's (D60 (6)): staff cannot edit the
    /// profile of someone of equal or higher rank.
    #[error("target ranks at or above the editor")]
    RankTooLow,
    /// No active member has this id.
    #[error("no such member")]
    NotFound,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// One page of the member list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberPage {
    /// The rows, in handle order, at most [`PAGE_SIZE`].
    pub members: Vec<MemberSummary>,
    /// Where the page after this one starts; `None` on the last page.
    pub next: Option<Cursor>,
}

/// What a saved edit did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// The member whose profile was written.
    pub target: AccountId,
    /// The profile as stored (sanitized, capped).
    pub profile: Profile,
    /// Which fields changed. Names only, for an audit row that copies no content.
    pub changed: Vec<Field>,
    /// Whether someone other than the owner made the change (`members.edit`). Such a change is
    /// audited (`profile.edited`).
    pub by_staff: bool,
}

fn require_proof(granted: &Granted, permission: Permission) -> Result<(), ProfileError> {
    if granted.permission() == permission {
        Ok(())
    } else {
        Err(ProfileError::NotPermitted)
    }
}

/// A page of active members (needs `members.view`).
pub async fn list_members<S: ProfileStore>(
    store: &S,
    granted: &Granted,
    query: &MemberQuery,
) -> Result<MemberPage, ProfileError> {
    require_proof(granted, Permission::MembersView)?;
    let mut members = store.list_members(query, PAGE_SIZE + 1).await?;
    let more = members.len() > PAGE_SIZE;
    members.truncate(PAGE_SIZE);
    let next = if more {
        members.last().map(Cursor::after)
    } else {
        None
    };
    Ok(MemberPage { members, next })
}

/// One active member's profile (needs `members.view`).
pub async fn view_profile<S: ProfileStore>(
    store: &S,
    granted: &Granted,
    target: AccountId,
) -> Result<MemberRecord, ProfileError> {
    require_proof(granted, Permission::MembersView)?;
    store.member(target).await?.ok_or(ProfileError::NotFound)
}

/// Whether `granted` lets its holder edit `target`'s profile: their own with
/// `account.edit_profile`, anyone's with `members.edit`. `Ok(true)` means the edit is staff
/// editing someone else's profile.
fn covers(granted: &Granted, target: AccountId) -> Result<bool, ProfileError> {
    match granted.permission() {
        Permission::AccountEditProfile if granted.account() == Some(target) => Ok(false),
        Permission::MembersEdit => Ok(granted.account() != Some(target)),
        _ => Err(ProfileError::NotPermitted),
    }
}

/// Load a profile to start editing it (needs the edit permission that covers `target`).
pub async fn load_for_edit<S: ProfileStore>(
    store: &S,
    granted: &Granted,
    target: AccountId,
) -> Result<MemberRecord, ProfileError> {
    covers(granted, target)?;
    store.member(target).await?.ok_or(ProfileError::NotFound)
}

/// Write `target`'s profile from typed text. The text is sanitized and capped here, whatever
/// the caller did. Needs `account.edit_profile` (own profile) or `members.edit` (anyone's).
pub async fn edit_profile<S: ProfileStore>(
    store: &S,
    granted: &Granted,
    target: AccountId,
    raw: &RawProfile,
) -> Result<Saved, ProfileError> {
    let by_staff = covers(granted, target)?;
    let before = store.member(target).await?.ok_or(ProfileError::NotFound)?;
    let profile = Profile::from_raw(raw);
    let changed = profile.changed_from(&before.profile);
    if !changed.is_empty() {
        // A staff edit of someone else's profile is audited in the write's own transaction,
        // with the names of the fields and never their values. The owner's edits are not.
        let event = by_staff.then(|| {
            let names: Vec<&str> = changed.iter().map(|field| field.name()).collect();
            let actor = granted.account().map_or(Actor::System, Actor::Account);
            events::profile_edited(actor, target, &names)
        });
        store.save_profile(target, &profile, event.as_ref()).await?;
    }
    Ok(Saved {
        target,
        profile,
        changed,
        by_staff,
    })
}

/// Which edit permission applies to `target`: `account.edit_profile` for the member's own
/// profile, `members.edit` for anyone else's. There is no fallback between them, so removing
/// `account.edit_profile` from every role stops everyone, staff included, from editing their
/// own, and `members.edit` is only ever about other people.
///
/// **Rank rule (D60 (6), D77):** editing someone else's profile is also refused unless the
/// target's top global rank is strictly below the editor's ([`Snapshot::top_rank`]); a target
/// that holds no role counts as below. An editor with no global rank edits nobody else's. The
/// permission is checked first, so a session without it never learns anything about ranks. The
/// target's rank is read at this moment; a promotion between this check and the write is not
/// guarded (the edit is audited, and only text is at stake).
pub async fn edit_grant<S: ProfileStore>(
    store: &S,
    access: &Snapshot,
    target: AccountId,
) -> Result<Granted, ProfileError> {
    let own = access.account() == Some(target);
    let permission = if own {
        Permission::AccountEditProfile
    } else {
        Permission::MembersEdit
    };
    let granted = access
        .require(permission, None)
        .map_err(|_| ProfileError::NotPermitted)?;
    if !outranks(store, access, target).await? {
        return Err(ProfileError::RankTooLow);
    }
    Ok(granted)
}

/// Whether the rank rule lets `access`'s holder edit `target`'s profile (FIX-M2-7): always for
/// their own profile, otherwise only if their top global rank is strictly above the target's.
/// The same test [`edit_grant`] makes, so the profile page can leave out the `e` key the
/// refusal would only turn down.
async fn outranks<S: ProfileStore>(
    store: &S,
    access: &Snapshot,
    target: AccountId,
) -> Result<bool, ProfileError> {
    if access.account() == Some(target) {
        return Ok(true);
    }
    let Some(editor) = access.top_rank() else {
        return Ok(false);
    };
    Ok(store
        .standing(target)
        .await?
        .is_none_or(|rank| rank < editor))
}

/// What a screen asks for. The front end runs it with [`execute`] and gives the answer back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// A page of the member list.
    List(MemberQuery),
    /// A profile to look at.
    View(AccountId),
    /// A profile to edit.
    Edit(AccountId),
    /// Write a profile.
    Save(AccountId, RawProfile),
}

/// The answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// A page of members.
    Page(MemberQuery, MemberPage),
    /// A profile to show, and whether the viewer's rank allows editing it (always for their own;
    /// for another member's, whether the member's top rank is below the viewer's). The screen
    /// offers `e` only when it does (FIX-M2-7); the refusal in [`edit_grant`] stays.
    Profile(MemberRecord, bool),
    /// A profile to edit.
    EditForm(MemberRecord),
    /// A profile was written (or nothing changed).
    Saved(Saved),
    /// The member is gone or not an active member.
    NotFound,
    /// The permission is held, but the member's top rank is not below the editor's.
    RankTooLow,
    /// The permission is not held.
    NotPermitted,
    /// The store failed or timed out; nothing is claimed about what was written.
    Failed,
}

/// Run a request for a session holding `access`. Every request starts with
/// [`Snapshot::require`]; a store failure is [`Response::Failed`], never a guess.
pub async fn execute<S: ProfileStore>(store: &S, access: &Snapshot, request: Request) -> Response {
    match run(store, access, request).await {
        Ok(response) => response,
        Err(ProfileError::NotPermitted) => Response::NotPermitted,
        Err(ProfileError::RankTooLow) => Response::RankTooLow,
        Err(ProfileError::NotFound) => Response::NotFound,
        Err(ProfileError::Store(error)) => {
            tracing::warn!(%error, "profile request failed");
            Response::Failed
        }
    }
}

async fn run<S: ProfileStore>(
    store: &S,
    access: &Snapshot,
    request: Request,
) -> Result<Response, ProfileError> {
    let view = || {
        access
            .require(Permission::MembersView, None)
            .map_err(|_| ProfileError::NotPermitted)
    };
    Ok(match request {
        Request::List(query) => {
            let page = list_members(store, &view()?, &query).await?;
            Response::Page(query, page)
        }
        Request::View(id) => {
            let record = view_profile(store, &view()?, id).await?;
            // Only worth a rank lookup when the permission to edit others is held; without it
            // the key is hidden by the permission anyway.
            let rankable = access.account() == Some(id) || access.allows(Permission::MembersEdit);
            let editable = rankable && outranks(store, access, id).await?;
            Response::Profile(record, editable)
        }
        Request::Edit(id) => {
            let granted = edit_grant(store, access, id).await?;
            Response::EditForm(load_for_edit(store, &granted, id).await?)
        }
        Request::Save(id, raw) => {
            let granted = edit_grant(store, access, id).await?;
            Response::Saved(edit_profile(store, &granted, id, &raw).await?)
        }
    })
}
