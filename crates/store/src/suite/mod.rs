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
pub(crate) mod chat;
pub(crate) mod community;
pub(crate) mod credits;
pub(crate) mod doors;
pub(crate) mod drafts;
pub(crate) mod feeds;
pub(crate) mod forums;
pub(crate) mod founder;
pub(crate) mod invites;
pub(crate) mod keys;
pub(crate) mod limits;
pub(crate) mod mail;
pub(crate) mod password_accounts;
pub(crate) mod passwords;
pub(crate) mod profiles;
pub(crate) mod queue;
pub(crate) mod registration;
pub(crate) mod scheduler;
pub(crate) mod terminal_settings;
pub(crate) mod widgets;

use std::sync::Arc;

use russh_bbs_core::access::{AccessStore, Fingerprint, MemberDirectory, Slug};
use russh_bbs_core::audit::{AuditStore, WallClock};
use russh_bbs_core::chat::RoomStore;
use russh_bbs_core::community::CommunityStore;
use russh_bbs_core::credits::CreditStore;
use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::drafts::DraftStore;
use russh_bbs_core::feeds::FeedStore;
use russh_bbs_core::forums::ForumStore;
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::invites::InviteStore;
use russh_bbs_core::keys::KeyStore;
use russh_bbs_core::limits::LimitStore;
use russh_bbs_core::mail::MailStore;
use russh_bbs_core::passwords::PasswordStore;
use russh_bbs_core::profiles::ProfileStore;
use russh_bbs_core::queue::QueueStore;
use russh_bbs_core::registration::RegistrationStore;
use russh_bbs_core::scheduler::SchedulerStore;
use russh_bbs_core::store::{AccountStateStore, AccountStore, SettingsStore};
use russh_bbs_core::terminal_settings::{MemberSessions, TerminalSettingsStore};
use russh_bbs_core::widgets::WidgetStore;

/// Everything the suite needs from a store: all the core traits, and cheap to clone so
/// concurrent checks can share it.
pub(crate) trait TestStore:
    AccountStore
    + AccountStateStore
    + InviteStore
    + KeyStore
    + AccessStore
    + AuditStore
    + CommunityStore
    + CreditStore
    + DoorStore
    + SchedulerStore
    + SettingsStore
    + FounderStore
    + LimitStore
    + DraftStore
    + ForumStore
    + FeedStore
    + MailStore
    + TerminalSettingsStore
    + ProfileStore
    + PasswordStore
    + QueueStore
    + RoomStore
    + RegistrationStore
    + MemberDirectory
    + MemberSessions
    + WidgetStore
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
        + KeyStore
        + AccessStore
        + AuditStore
        + CommunityStore
        + CreditStore
        + DoorStore
        + SchedulerStore
        + SettingsStore
        + FounderStore
        + LimitStore
        + DraftStore
        + ForumStore
        + FeedStore
        + MailStore
        + TerminalSettingsStore
        + ProfileStore
        + PasswordStore
        + QueueStore
        + RoomStore
        + RegistrationStore
        + MemberDirectory
        + MemberSessions
        + WidgetStore
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

    /// The raw bytes of one column of `mail_message`, as lower-case hex, one string per row.
    async fn hex(&self, column: &str) -> Vec<String>;

    /// A SQL expression for a blob of `len` zero bytes, for proving a size cap in the database.
    fn blob_sql(&self, len: usize) -> String;

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
                access, access_edit, accounts, audit, chat, community, credits, doors, drafts, feeds, forums, founder, invites, keys, limits,
                mail, password_accounts, passwords, profiles,
                queue, registration, scheduler, terminal_settings, widgets,
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
                audit::an_unknown_action_reads_as_a_labelled_entry_beside_known_rows,
                audit::paging_across_known_and_unknown_rows_neither_skips_nor_repeats,
                audit::filters_apply_to_unknown_rows_by_actor_and_time_and_never_by_action,
                audit::an_unknown_name_is_sanitized_when_read,
                audit::structurally_invalid_rows_are_still_corrupt,
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
                community::lines_come_back_newest_first_and_roll_off_at_the_list_size,
                community::the_hourly_limit_refuses_the_next_line_and_resets_with_the_hour,
                community::concurrent_adds_never_pass_the_hourly_limit,
                community::adding_a_line_for_an_unknown_account_fails,
                community::removing_a_line_is_audited_and_a_wrong_id_or_kind_is_not_found,
                community::last_callers_are_active_members_newest_first_one_row_each,
                community::a_poll_round_trips_with_its_options_and_setting,
                community::a_member_votes_once_and_the_second_vote_is_refused,
                community::concurrent_votes_from_one_member_count_once,
                community::only_the_creator_or_staff_closes_a_poll_and_a_closed_poll_takes_no_votes,
                community::removing_a_poll_deletes_its_votes_and_is_audited,
                community::poll_creation_is_limited_per_member_per_day_and_in_all,
                community::concurrent_poll_creation_never_passes_the_day_limit,
                community::results_show_as_the_polls_setting_says,
                community::the_permissions_decide_who_can_do_what_through_execute,
                community::stored_text_is_sanitized_again_when_read,
                community::the_community_tables_refuse_what_core_would_never_write,
                community::deleting_an_account_deletes_its_community_rows,
                community::the_roles_hold_the_community_permissions_the_docs_say,
                community::community_data_survives_a_reopen,
                community::a_broken_store_errors_instead_of_pretending_the_lists_are_empty,
                drafts::a_saved_draft_reads_back_and_a_second_save_replaces_it,
                drafts::drafts_are_per_member_and_per_context,
                drafts::the_list_is_newest_first_without_text,
                drafts::the_cap_refuses_a_new_context_but_always_allows_a_replacement,
                drafts::concurrent_saves_never_pass_the_cap,
                drafts::the_text_is_sanitized_on_save_and_bounded,
                drafts::blank_text_removes_the_draft,
                drafts::saving_a_draft_for_an_unknown_account_fails,
                forums::boards_are_created_by_staff_only_with_unique_names_and_an_audit_row,
                forums::a_restricted_board_is_absent_for_everyone_without_a_scoped_grant,
                forums::threads_replies_and_reading_round_trip,
                forums::stored_text_is_sanitized,
                forums::posts_per_hour_is_enforced_from_the_post_rows,
                forums::concurrent_posts_never_pass_the_hourly_cap,
                forums::staff_remove_posts_with_an_audit_row_and_removed_text_is_not_found,
                forums::search_finds_words_and_never_reaches_unreadable_boards,
                forums::hostile_queries_are_handled_not_errors,
                forums::forum_data_survives_a_reopen,
                feeds::staff_add_feeds_as_read_only_boards_with_an_audit_row_and_no_address,
                feeds::the_number_of_feeds_is_limited_even_when_added_at_once,
                feeds::posting_and_replying_on_a_feed_board_are_refused_by_core,
                feeds::items_become_authorless_threads_once_and_the_newest_are_kept,
                feeds::a_failed_read_is_a_closed_outcome_and_a_good_one_resets_it,
                feeds::removing_a_feed_removes_its_board_threads_and_search_hits,
                feeds::a_board_is_public_as_rss_only_when_marked_open_and_readable,
                feeds::feed_data_survives_a_reopen,
                mail::mail_round_trips_and_the_stored_row_holds_no_plaintext,
                mail::a_row_is_only_ever_reachable_through_its_recipients_mailbox,
                mail::a_tampered_moved_or_swapped_row_is_an_error_entry_not_a_crash,
                mail::a_different_key_cannot_open_and_start_up_checks_say_so,
                mail::unknown_and_not_active_handles_get_one_answer,
                mail::mail_per_day_is_enforced_per_utc_day,
                mail::the_inbox_cap_refuses_new_mail_and_deleting_makes_room,
                mail::replies_go_to_the_sender_by_account_and_only_while_they_are_active,
                mail::reports_copy_what_the_reporter_sends_and_are_audited_by_id,
                mail::the_open_report_cap_counts_open_reports_only,
                mail::mail_cannot_be_read_back_through_a_report_or_the_audit_log,
                mail::mail_survives_a_reopen_and_a_closed_store_fails_closed,
                mail::deleting_an_account_deletes_its_mailbox_and_keeps_the_senders_mail,
                drafts::deleting_an_account_deletes_its_drafts,
                drafts::the_database_refuses_what_core_would_never_write,
                drafts::a_stored_row_the_code_could_not_have_written_is_corrupt_not_shown,
                drafts::drafts_survive_a_reopen,
                drafts::a_broken_store_errors_instead_of_pretending_there_is_no_draft,
                founder::a_claim_creates_an_active_sysop_bound_to_the_key,
                founder::a_second_claim_changes_nothing,
                founder::two_concurrent_claims_produce_exactly_one_sysop,
                founder::a_failed_claim_rolls_back_and_leaves_the_board_unclaimed,
                founder::the_claim_survives_reopening_the_database,
                founder::role_lookup_is_only_for_active_accounts,
                password_accounts::redeeming_with_a_password_makes_a_keyless_member_with_the_key_paths_audit_rows,
                password_accounts::a_refused_password_redemption_leaves_no_password_and_consumes_nothing,
                password_accounts::open_with_a_password_makes_a_keyless_account_and_counts_against_the_cap,
                password_accounts::open_with_a_taken_handle_rolls_back_the_count_and_the_password,
                password_accounts::an_approved_password_request_makes_a_keyless_member_and_the_hash_moves,
                password_accounts::a_password_request_is_one_per_handle_and_a_denial_removes_the_hash,
                password_accounts::approving_a_password_request_for_a_taken_handle_keeps_the_request,
                password_accounts::has_key_is_true_for_a_key_account_and_false_for_a_password_account,
                password_accounts::the_database_refuses_a_password_hash_that_is_not_argon2id,
                password_accounts::a_visitor_with_a_key_gets_the_key_and_the_password_on_every_path,
                password_accounts::the_founder_can_be_claimed_with_a_password_with_or_without_a_key,
                password_accounts::a_key_is_linked_once_audited_and_a_taken_key_changes_nothing,
                keys::keys_are_listed_added_and_removed_with_audit_rows_and_notices,
                keys::the_last_way_in_is_never_removed,
                keys::two_removals_racing_for_the_last_two_keys_cannot_both_succeed,
                keys::a_key_that_has_an_account_cannot_be_added_and_nothing_is_written,
                keys::a_link_code_binds_a_key_once_and_expires_to_the_second,
                keys::a_code_is_used_once_when_two_redeem_it_at_the_same_time,
                keys::the_hourly_limit_holds_and_a_newer_code_voids_the_last,
                keys::a_newer_code_voids_every_older_unused_code_of_its_kind,
                keys::a_recovery_code_binds_a_key_and_replaces_the_password_in_one_step,
                keys::a_recovery_for_an_expired_or_inactive_account_is_refused,
                keys::rank_follows_the_roles,
                keys::key_use_is_remembered_and_a_linked_key_is_told,
                keys::a_key_change_never_lands_without_its_audit_row,
                keys::only_the_hash_of_a_code_is_stored_and_nothing_secret_is_audited,
                keys::the_roles_hold_the_key_permissions_the_docs_say,
                keys::deleting_an_account_deletes_its_codes_and_notices,
                keys::keys_and_codes_survive_a_reopen,
                keys::a_broken_store_errors_instead_of_pretending_there_is_nothing,
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
                passwords::a_new_account_has_no_password,
                passwords::a_hash_round_trips_and_a_later_one_replaces_it,
                passwords::the_login_lookup_ignores_case_and_finds_every_state,
                passwords::clearing_removes_the_hash_and_audits_only_a_real_change,
                passwords::the_audit_row_holds_no_hash,
                passwords::an_unknown_account_is_not_found_on_every_call,
                passwords::an_upgrade_replaces_only_the_hash_it_was_read_from,
                passwords::a_failed_audit_write_leaves_the_password_unchanged,
                passwords::the_database_refuses_anything_but_an_argon2id_string,
                passwords::a_hash_survives_a_reopen_and_goes_with_its_account,
                profiles::an_active_member_without_a_profile_has_an_empty_one,
                profiles::a_saved_profile_round_trips_and_a_later_save_replaces_it,
                profiles::saving_a_profile_for_an_unknown_account_fails,
                profiles::only_active_accounts_have_visible_profiles,
                profiles::the_list_is_sorted_by_handle_ignoring_case,
                profiles::a_prefix_matches_the_start_of_a_handle_only_and_has_no_wildcards,
                profiles::paging_by_cursor_neither_skips_nor_repeats,
                profiles::the_list_carries_the_display_name,
                profiles::profiles_survive_a_reopen,
                profiles::standing_is_the_top_global_rank_and_none_for_inactive_accounts,
                profiles::a_staff_edit_writes_its_audit_row_and_a_failed_audit_rolls_the_edit_back,
                profiles::a_broken_store_fails_instead_of_answering,
                queue::a_request_stores_only_the_documented_fields_and_writes_no_audit_row,
                queue::one_request_per_key_and_a_full_queue_refuses_with_the_slot_freed_by_a_decision,
                queue::a_key_that_already_has_an_account_is_refused,
                queue::concurrent_submissions_never_pass_the_cap_or_the_one_per_key_rule,
                queue::approving_makes_an_active_member_bound_to_the_key_and_audits_without_content,
                queue::approval_refused_for_a_taken_handle_or_a_bound_key_changes_nothing,
                queue::denying_deletes_the_request_and_keeps_only_a_hash_and_an_expiry,
                queue::a_denied_key_is_turned_away_until_the_cooldown_ends_and_then_nothing_remains,
                queue::requests_expire_after_the_ttl_and_the_purge_removes_every_trace,
                queue::vouching_adds_a_name_once_and_the_queue_shows_active_vouchers_only,
                queue::the_queue_lists_oldest_first_and_a_limit_cuts_the_newest,
                queue::racing_decisions_on_one_request_have_exactly_one_winner,
                queue::queue_writes_never_land_without_their_audit_row,
                queue::the_queue_survives_a_reopen,
                queue::a_broken_store_errors_instead_of_answering,
                registration::open_makes_an_active_account_with_no_inviter_and_audits_it,
                registration::the_cap_degrades_open_for_the_rest_of_the_utc_day_and_the_next_day_starts_fresh,
                registration::the_capped_row_follows_the_account_created_row_that_used_the_last_place,
                registration::an_unlimited_cap_never_refuses,
                registration::a_refused_open_registration_leaves_no_trace_and_costs_no_place,
                registration::concurrent_opens_never_pass_the_cap,
                registration::old_counter_rows_are_pruned_by_the_next_creation,
                registration::open_registration_never_lands_without_its_audit_row,
                registration::the_mode_and_the_cap_setting_are_read_on_every_call,
                registration::an_application_is_a_queue_row_of_its_kind_with_one_audit_row_and_no_content,
                registration::an_application_never_lands_without_its_audit_row,
                registration::registration_data_survives_a_reopen,
                chat::a_new_board_has_a_lobby_and_the_roles_hold_the_chat_permissions,
                chat::creating_and_closing_a_room_are_audited_with_the_name_and_nothing_else,
                chat::a_name_is_unique_among_open_rooms_and_ids_are_never_reused,
                chat::the_room_cap_refuses_and_concurrent_creations_never_pass_it,
                chat::an_unknown_actor_is_refused_and_nothing_is_written,
                chat::room_writes_never_land_without_their_audit_row,
                chat::the_database_refuses_names_the_code_would_never_write,
                chat::a_stored_name_the_code_could_not_have_written_is_corrupt_not_shown,
                chat::nothing_a_member_says_has_a_table_to_land_in,
                chat::rooms_survive_a_reopen,
                chat::a_broken_store_errors_instead_of_listing_no_rooms,
                credits::earn_and_spend_derive_the_balance,
                credits::an_overdraft_is_refused_and_changes_nothing,
                credits::concurrent_spends_never_overdraw,
                credits::accounts_are_separate_and_unknown_accounts_are_refused,
                credits::history_pages_newest_first_without_skipping,
                credits::the_database_refuses_to_change_or_remove_a_row,
                credits::the_ledger_survives_a_reopen,
                credits::a_broken_store_errors_instead_of_reading_as_zero,
                doors::a_save_round_trips_and_each_write_bumps_the_revision,
                doors::a_stale_or_repeated_first_write_is_a_conflict_and_changes_nothing,
                doors::concurrent_writers_of_one_revision_exactly_one_wins,
                doors::the_caps_are_enforced_in_code_and_by_the_database,
                doors::an_unknown_account_cannot_have_a_save,
                doors::the_world_has_its_own_revision_and_remembers_the_tick_day,
                doors::a_season_row_made_first_does_not_hide_the_world_and_the_other_way_round,
                doors::staff_tools_change_things_and_write_one_audit_row_each,
                doors::staff_rank_and_permission_rules_hold_against_the_real_roles,
                doors::a_staff_change_never_lands_without_its_audit_row,
                doors::deleting_an_account_deletes_its_door_data,
                doors::the_roles_hold_the_door_staff_permissions_the_docs_say,
                doors::a_broken_store_errors_instead_of_reading_as_empty,
                scheduler::a_last_run_round_trips_and_a_later_one_replaces_it,
                scheduler::the_database_refuses_an_unknown_outcome_and_a_stored_one_is_not_misread,
                scheduler::a_scheduler_over_the_store_remembers_its_runs_across_a_restart,
                scheduler::a_broken_store_errors_instead_of_reading_as_never_run,
                terminal_settings::defaults_when_missing,
                terminal_settings::set_then_get_round_trips,
                terminal_settings::every_tier_and_unset_round_trip,
                terminal_settings::layout_and_theme_round_trip_and_default_for_new_rows,
                terminal_settings::ui_settings_round_trip_and_none_follows_the_board,
                terminal_settings::a_later_set_replaces_and_clears_overrides,
                terminal_settings::accounts_are_isolated,
                terminal_settings::saving_for_an_unknown_account_fails,
                terminal_settings::login_times_report_the_previous_login,
                terminal_settings::sessions_start_from_stored_state,
                terminal_settings::only_active_accounts_start_a_session,
                terminal_settings::settings_survive_reopening_the_database,
                terminal_settings::concurrent_logins_each_see_a_consistent_previous_time,
                widgets::lists_round_trip_in_order_and_none_follows_the_default,
                widgets::a_save_for_an_unknown_account_fails_and_a_delete_takes_the_lists,
                widgets::a_stored_list_the_code_could_not_have_written_is_refused_by_the_database,
                widgets::a_save_writes_one_audit_row_and_no_widget_names,
                widgets::a_viewer_sees_nothing_from_a_board_they_cannot_read,
                widgets::community_widgets_need_community_view_and_polls_keep_their_visibility,
                widgets::mail_and_credits_are_only_the_viewers_own,
                widgets::the_waiting_room_is_shown_no_widget_data,
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
