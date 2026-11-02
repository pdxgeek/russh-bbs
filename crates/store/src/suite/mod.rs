//! One behaviour suite for every backend (D24, D32).
//!
//! Each check is a function generic over a [`Harness`] that supplies a fresh database of one
//! backend. [`shared_suite!`] turns the whole list into `#[tokio::test]`s for a harness, so
//! SQLite and Postgres run exactly the same checks. A check that needs backend SQL (corrupt
//! rows, PRAGMAs, TLS) does not belong here; it lives in that backend's own test module.
#![allow(clippy::unwrap_used)]

pub(crate) mod access;
pub(crate) mod access_edit;
pub(crate) mod accounts;
pub(crate) mod audit;
pub(crate) mod founder;
pub(crate) mod invites;
pub(crate) mod limits;
pub(crate) mod profiles;
pub(crate) mod terminal_settings;

use std::sync::Arc;

use russh_bbs_core::access::{AccessStore, Fingerprint, MemberDirectory, Slug};
use russh_bbs_core::audit::{AuditStore, WallClock};
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::invites::InviteStore;
use russh_bbs_core::limits::LimitStore;
use russh_bbs_core::profiles::ProfileStore;
use russh_bbs_core::store::{AccountStateStore, AccountStore, SettingsStore};
use russh_bbs_core::terminal_settings::{MemberSessions, TerminalSettingsStore};

/// Everything the suite needs from a store: all the core traits, and cheap to clone so
/// concurrent checks can share it.
pub(crate) trait TestStore:
    AccountStore
    + AccountStateStore
    + InviteStore
    + AccessStore
    + AuditStore
    + SettingsStore
    + FounderStore
    + LimitStore
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
        + AccountStateStore
        + InviteStore
        + AccessStore
        + AuditStore
        + SettingsStore
        + FounderStore
        + LimitStore
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

    /// Make the store stamp audit rows from `clock`.
    fn set_clock(&mut self, clock: Arc<dyn WallClock>);

    /// Run one raw SQL statement on the database, returning the database's error text if it
    /// fails. For proving what the database itself refuses.
    async fn raw(&self, sql: &str) -> Result<u64, String>;

    /// Run one raw `SELECT` whose first column is text (cast it in the query) and return that
    /// column of every row. For looking at what is really stored.
    async fn texts(&self, sql: &str) -> Vec<String>;

    /// Make every insert into `audit_log` fail, so a test can prove the change that wanted to
    /// write the row did not land either.
    async fn break_audit_inserts(&self);
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
            use $crate::suite::{
                access, access_edit, accounts, audit, founder, invites, limits, profiles,
                terminal_settings,
            };

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
                access_edit::a_role_is_put_changed_and_deleted_with_one_audit_row_each,
                access_edit::deleting_a_granted_role_is_refused_and_nothing_cascades,
                access_edit::removing_a_grant_writes_a_row_and_removing_nothing_writes_none,
                access_edit::the_last_sysop_grant_cannot_be_removed_even_by_two_at_once,
                access_edit::rank_and_locked_roles_hold_on_the_real_role_data,
                access_edit::escalation_is_refused_on_the_real_role_data,
                access_edit::protected_grants_hold_on_the_real_role_data,
                access_edit::a_revoked_role_stops_working_on_the_next_snapshot,
                access_edit::refused_edits_write_no_audit_rows,
                access_edit::an_edit_never_lands_without_its_audit_row,
                access_edit::role_edits_survive_a_reopen,
                audit::append_and_page_round_trip_newest_first_with_the_injected_clock,
                audit::paging_filters_combine_and_time_bounds_are_since_inclusive_until_exclusive,
                audit::pages_are_bounded_and_the_cursor_walks_every_row_once,
                audit::update_and_delete_by_raw_sql_fail_and_leave_the_row,
                audit::the_founder_claim_writes_its_row_in_its_transaction,
                audit::creating_an_account_writes_a_row,
                audit::group_changes_write_rows_and_no_ops_write_none,
                audit::a_new_grant_writes_one_row_and_a_repeat_or_a_bad_grant_none,
                audit::saving_terminal_settings_writes_a_row,
                audit::a_change_never_lands_without_its_audit_row,
                audit::audit_rows_survive_a_reopen,
                audit::the_read_service_follows_the_role_data,
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
                invites::a_created_invite_is_stored_hashed_bound_to_the_inviter_and_audited,
                invites::redeeming_makes_an_active_member_with_the_inviter_recorded,
                invites::a_code_works_once,
                invites::every_kind_of_bad_code_is_the_same_refusal,
                invites::expiry_is_exact_and_uses_the_store_clock,
                invites::a_refused_redemption_consumes_nothing,
                invites::a_suspended_inviter_voids_their_open_codes,
                invites::concurrent_redemptions_of_one_code_make_exactly_one_account,
                invites::the_quota_counts_the_utc_calendar_month_and_a_missing_inviter_is_an_error,
                invites::concurrent_creations_cannot_exceed_the_quota,
                invites::the_tree_lists_members_with_their_inviters_and_never_applicants,
                invites::my_invites_are_newest_first_with_their_status,
                invites::deleting_accounts_keeps_invites_and_the_tree_intact,
                invites::invite_and_state_writes_never_land_without_their_audit_row,
                invites::invites_survive_a_reopen,
                invites::allowed_state_changes_are_made_and_audited,
                invites::refused_state_changes_write_nothing,
                invites::racing_state_changes_apply_once,
                limits::the_built_in_roles_have_the_documented_defaults,
                limits::the_documented_defaults_equal_the_seeded_rows,
                limits::limit_edits_write_audit_rows_and_no_ops_write_none,
                limits::a_limit_edit_never_lands_without_its_audit_row,
                limits::every_role_has_every_kind_and_the_layers_do_not_tighten,
                limits::limits_edit_is_held_by_sysop_and_cosysop_only,
                limits::role_limits_can_be_replaced_and_unlimited_is_a_real_value,
                limits::member_overrides_set_clear_and_isolate,
                limits::an_override_changes_the_effective_limit,
                limits::usage_accumulates_per_account_kind_and_day,
                limits::a_capped_add_stops_at_the_cap_and_changes_nothing_when_full,
                limits::concurrent_capped_adds_never_pass_the_cap,
                limits::pruning_deletes_only_older_days_of_that_account,
                limits::limits_and_usage_survive_a_reopen,
                limits::a_broken_store_errors_instead_of_reading_as_unlimited,
                limits::a_session_start_counts_calls_in_the_store,
                profiles::an_active_member_without_a_profile_has_an_empty_one,
                profiles::a_saved_profile_round_trips_and_a_later_save_replaces_it,
                profiles::saving_a_profile_for_an_unknown_account_fails,
                profiles::only_active_accounts_have_visible_profiles,
                profiles::the_list_is_sorted_by_handle_ignoring_case,
                profiles::a_prefix_matches_the_start_of_a_handle_only_and_has_no_wildcards,
                profiles::paging_by_cursor_neither_skips_nor_repeats,
                profiles::the_list_carries_the_display_name,
                profiles::profiles_survive_a_reopen,
                profiles::a_staff_edit_writes_its_audit_row_and_a_failed_audit_rolls_the_edit_back,
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
