//! Storing a member's widget lists (UI-10).

use std::future::Future;

use crate::access::{Permission, Snapshot};
use crate::store::{AccountId, StoreError};
use crate::terminal_settings::{LayoutId, SaveError};

use super::id::WidgetId;
use super::place::{Choice, MAX_PER_LAYOUT};

/// Per-member widget lists, one per layout the member changed. Persistence only: the rules are
/// in [`super::Rules`].
pub trait WidgetStore: Send + Sync {
    /// The account's saved lists; empty if it never changed any.
    fn widget_choice(
        &self,
        account: AccountId,
    ) -> impl Future<Output = Result<Choice, StoreError>> + Send;

    /// Replace the account's list for `layout` with `list` (empty means "show none"), or go back
    /// to following the default with `None`. [`StoreError::AccountNotFound`] for an id with no
    /// account.
    fn set_widget_list(
        &self,
        account: AccountId,
        layout: LayoutId,
        list: Option<&[WidgetId]>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

impl WidgetStore for crate::access::NoMembers {
    fn widget_choice(
        &self,
        _account: AccountId,
    ) -> impl Future<Output = Result<Choice, StoreError>> + Send {
        std::future::ready(Ok(Choice::new()))
    }

    fn set_widget_list(
        &self,
        _account: AccountId,
        _layout: LayoutId,
        _list: Option<&[WidgetId]>,
    ) -> impl Future<Output = Result<(), StoreError>> + Send {
        std::future::ready(Err(StoreError::AccountNotFound))
    }
}

/// Save the signed-in member's own list for `layout`: the session must hold
/// `account.edit_settings`, and the list saved is the session's account's, so one member can
/// never write another's. A list longer than a layout holds is refused.
///
/// # Errors
/// [`SaveError::NotPermitted`] without the permission or an account; [`SaveError::Store`].
pub async fn save<S: WidgetStore>(
    store: &S,
    access: &Snapshot,
    layout: LayoutId,
    list: Option<&[WidgetId]>,
) -> Result<(), SaveError> {
    let granted = access
        .require(Permission::AccountEditSettings, None)
        .map_err(|_| SaveError::NotPermitted)?;
    let account = granted.account().ok_or(SaveError::NotPermitted)?;
    if list.is_some_and(|list| list.len() > MAX_PER_LAYOUT) {
        return Err(SaveError::NotPermitted);
    }
    store.set_widget_list(account, layout, list).await?;
    Ok(())
}
