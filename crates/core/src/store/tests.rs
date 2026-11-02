#![allow(clippy::unwrap_used)]

use super::{AccountState, RegistrationMode, Role, SettingKey};

#[test]
fn enums_round_trip_through_their_stored_text() {
    for role in [Role::Member, Role::Sysop] {
        assert_eq!(Role::parse(role.as_str()), Some(role));
    }
    for state in [
        AccountState::Pending,
        AccountState::Approved,
        AccountState::Active,
        AccountState::Suspended,
    ] {
        assert_eq!(AccountState::parse(state.as_str()), Some(state));
    }
    for mode in [
        RegistrationMode::Invite,
        RegistrationMode::Apply,
        RegistrationMode::Open,
    ] {
        assert_eq!(RegistrationMode::parse(mode.as_str()), Some(mode));
    }
}

#[test]
fn unknown_text_does_not_parse() {
    assert_eq!(Role::parse("root"), None);
    assert_eq!(AccountState::parse("ACTIVE"), None);
    assert_eq!(RegistrationMode::parse(""), None);
}

#[test]
fn only_active_accounts_have_member_access() {
    assert!(AccountState::Active.is_member_access());
    assert!(!AccountState::Pending.is_member_access());
    assert!(!AccountState::Approved.is_member_access());
    assert!(!AccountState::Suspended.is_member_access());
}

#[test]
fn registration_mode_defaults_to_invite() {
    assert_eq!(RegistrationMode::default(), RegistrationMode::Invite);
    assert_eq!(SettingKey::RegistrationMode.as_str(), "registration_mode");
}
