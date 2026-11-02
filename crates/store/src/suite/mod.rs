//! One behaviour suite for every backend (D24, D32).
//!
//! Each check is a function generic over a [`Harness`] that supplies a fresh database of one
//! backend. [`shared_suite!`] turns the whole list into `#[tokio::test]`s for a harness, so
//! SQLite and Postgres run exactly the same checks. A check that needs backend SQL (corrupt
//! rows, PRAGMAs, TLS) does not belong here; it lives in that backend's own test module.
#![allow(clippy::unwrap_used)]

pub(crate) mod access;
pub(crate) mod accounts;
pub(crate) mod founder;
pub(crate) mod profiles;
pub(crate) mod terminal_settings;

use russh_bbs_core::access::{AccessStore, Fingerprint, MemberDirectory, Slug};
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::profiles::ProfileStore;
use russh_bbs_core::store::{AccountStore, SettingsStore};
use russh_bbs_core::terminal_settings::{MemberSessions, TerminalSettingsStore};

/// Everything the suite needs from a store: all the core traits, and cheap to clone so
/// concurrent checks can share it.
pub(crate) trait TestStore:
    AccountStore
    + AccessStore
    + SettingsStore
    + FounderStore
    + TerminalSettingsStore
    + ProfileStore
    + MemberDirectory
    + MemberSessions
    + Clone
    + Send
    + Sync
    + 'static
{
}

impl<T> TestStore for T where
    T: AccountStore
        + AccessStore
        + SettingsStore
        + FounderStore
        + TerminalSettingsStore
        + ProfileStore
        + MemberDirectory
        + MemberSessions
        + Clone
        + Send
        + Sync
        + 'static
{
}

/// A fresh, empty, migrated database of one backend, owned for the length of a check.
pub(crate) trait Harness: Sized {
    /// The backend's store.
    type Store: TestStore;

    /// Create a brand-new empty database and open a store on it.
    async fn fresh() -> Self;

    /// The open store.
    fn store(&self) -> &Self::Store;

    /// Close the store and open the same database again (a restart).
    async fn reopen(&mut self);

    /// Close the pool so every later call fails (to prove callers fail closed).
    async fn close(&self);
}

/// The highest role the account behind `key` holds right now: `None` for an unknown key,
/// `waiting` for an account that is not active (it gets the waiting snapshot).
pub(crate) async fn top_role_of<S: TestStore>(store: &S, key: &Fingerprint) -> Option<Slug> {
    let account = store.account_by_fingerprint(key).await.unwrap()?;
    store
        .snapshot(Some(account.id))
        .await
        .unwrap()
        .top_role()
        .cloned()
}

/// A valid-shaped fingerprint that differs per `n`.
pub(crate) fn fingerprint(n: u32) -> Fingerprint {
    Fingerprint::parse(&format!("SHA256:{n:A>43}")).unwrap()
}

/// Generate the shared suite as tests in a module named `$name`, for harness `$harness`.
/// `$attr` is applied to every test (for example `ignore = "..."`).
macro_rules! shared_suite {
    ($name:ident, $harness:ty, $attr:meta) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;
            use $crate::suite::{access, accounts, founder, profiles, terminal_settings};

            $crate::suite::shared_suite!(@tests $harness, $attr;
                access::the_built_in_roles_have_their_ranks_and_kinds,
                access::waiting_holds_exactly_the_waiting_room_permissions,
                access::each_role_contains_the_one_below_and_sysop_holds_everything,
                access::the_built_in_groups_exist,
                access::an_active_member_holds_member_through_everyone,
                access::the_founder_holds_every_permission_as_sysop,
                access::unknown_and_inactive_accounts_get_the_waiting_snapshot,
                access::staff_members_hold_moderator,
                access::nested_groups_reach_accounts_through_the_closure,
                access::a_rejected_cycle_leaves_everything_unchanged,
                access::nesting_deeper_than_the_cap_is_refused,
                access::concurrent_opposite_nesting_makes_exactly_one_edge,
                access::a_scoped_role_grant_reaches_only_its_resource,
                access::allow_entries_name_accounts_and_groups,
                access::adding_a_grant_twice_is_not_an_error_and_adds_nothing,
                access::bad_grants_are_refused_by_name,
                access::access_data_survives_a_reopen,
                access::a_broken_store_fails_the_snapshot_instead_of_granting,
                accounts::creates_an_account_with_its_fields,
                accounts::a_known_fingerprint_returns_its_account_and_an_unknown_one_returns_none,
                accounts::an_account_can_have_several_keys,
                accounts::two_accounts_cannot_share_a_fingerprint,
                accounts::attaching_to_a_missing_account_is_an_error,
                accounts::usernames_are_unique_including_lookalikes,
                accounts::registration_mode_defaults_to_invite_and_can_change,
                accounts::settings_get_set_and_overwrite,
                accounts::an_unrecognised_registration_mode_is_an_error_not_a_default,
                accounts::only_active_accounts_are_members,
                accounts::the_directory_fails_closed_when_the_store_is_broken,
                accounts::data_survives_a_reopen,
                founder::a_claim_creates_an_active_sysop_bound_to_the_key,
                founder::a_second_claim_changes_nothing,
                founder::two_concurrent_claims_produce_exactly_one_sysop,
                founder::a_failed_claim_rolls_back_and_leaves_the_board_unclaimed,
                founder::the_claim_survives_reopening_the_database,
                founder::role_lookup_is_only_for_active_accounts,
                profiles::an_active_member_without_a_profile_has_an_empty_one,
                profiles::a_saved_profile_round_trips_and_a_later_save_replaces_it,
                profiles::saving_a_profile_for_an_unknown_account_fails,
                profiles::only_active_accounts_have_visible_profiles,
                profiles::the_list_is_sorted_by_handle_ignoring_case,
                profiles::a_prefix_matches_the_start_of_a_handle_only_and_has_no_wildcards,
                profiles::paging_by_cursor_neither_skips_nor_repeats,
                profiles::the_list_carries_the_display_name,
                profiles::profiles_survive_a_reopen,
                profiles::a_broken_store_fails_instead_of_answering,
                terminal_settings::defaults_when_missing,
                terminal_settings::set_then_get_round_trips,
                terminal_settings::every_tier_and_unset_round_trip,
                terminal_settings::a_later_set_replaces_and_clears_overrides,
                terminal_settings::accounts_are_isolated,
                terminal_settings::saving_for_an_unknown_account_fails,
                terminal_settings::login_times_report_the_previous_login,
                terminal_settings::sessions_start_from_stored_state,
                terminal_settings::only_active_accounts_start_a_session,
                terminal_settings::settings_survive_reopening_the_database,
                terminal_settings::concurrent_logins_each_see_a_consistent_previous_time,
            );
        }
    };
    (@tests $harness:ty, $attr:meta; $($module:ident :: $test:ident),+ $(,)?) => {
        $(
            #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
            #[$attr]
            async fn $test() {
                $module::$test::<$harness>().await;
            }
        )+
    };
}

pub(crate) use shared_suite;
