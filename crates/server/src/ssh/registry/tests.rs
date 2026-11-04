#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use russh_bbs_core::access::{Access, Fingerprint};

use russh_bbs_core::store::AccountId;

use super::{Revocation, SessionRegistry};
use crate::ssh::terminal::PtySize;

#[test]
fn guard_registers_updates_and_removes() {
    let registry = Arc::new(SessionRegistry::default());
    let guard = registry.register(None);
    assert_eq!(registry.snapshots().len(), 1);

    let fp = Fingerprint::parse("SHA256:uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s").unwrap();
    guard.set_authenticated(fp.clone(), Access::Restricted);
    guard.set_size(PtySize::from_request(100, 30));
    guard.set_shell_open(true);

    let snap = registry.snapshots().remove(0);
    assert_eq!(snap.fingerprint, Some(fp));
    assert_eq!(snap.access, Some(Access::Restricted));
    assert_eq!(snap.pty_size, Some(PtySize::from_request(100, 30)));
    assert!(snap.shell_open);

    drop(guard);
    assert_eq!(registry.snapshots(), vec![]);
}

#[test]
fn ids_are_unique() {
    let registry = Arc::new(SessionRegistry::default());
    let a = registry.register(None);
    let b = registry.register(None);
    assert_ne!(a.id(), b.id());
}

fn key(seed: u8) -> Fingerprint {
    let mut text = String::from("SHA256:");
    text.push_str(&"uNiVztksCsDhcc0u9e8BujQXVUpKZIDTMczCvj3tD2s"[..43 - 1]);
    text.push(char::from(b'A' + seed));
    Fingerprint::parse(&text).unwrap()
}

#[test]
fn revoking_a_key_ends_only_the_sessions_that_used_it_and_never_the_actor() {
    let registry = Arc::new(SessionRegistry::default());
    let account = AccountId::from_raw(1);
    let other = AccountId::from_raw(2);
    let actor = registry.register(None);
    let same_key = registry.register(None);
    let other_key = registry.register(None);
    let other_account = registry.register(None);
    let by_password = registry.register(None);
    for (guard, who, fp) in [
        (&actor, account, key(1)),
        (&same_key, account, key(1)),
        (&other_key, account, key(2)),
        (&other_account, other, key(1)),
    ] {
        guard.set_authenticated(fp, Access::Member);
        guard.set_account(who);
    }
    by_password.set_authenticated_by_password(Access::Member);
    by_password.set_account(account);

    // `a` removes key 1: `b` goes; `a` (the actor), `c` (another key), `d` (another account)
    // and the password session `e` stay.
    let ended = actor.revoke_others(account, &Revocation::Key(key(1)));
    assert_eq!(ended, vec![same_key.id()]);
    assert!(same_key.revoke_signal().is_revoked());
    for stays in [&actor, &other_key, &other_account, &by_password] {
        assert!(!stays.revoke_signal().is_revoked());
    }
    // Once ended, a session is not reported twice.
    assert_eq!(
        actor.revoke_others(account, &Revocation::Key(key(1))),
        vec![]
    );
}

#[test]
fn a_password_change_ends_password_sessions_and_a_recovery_ends_the_other_keys_too() {
    let registry = Arc::new(SessionRegistry::default());
    let account = AccountId::from_raw(1);
    let actor = registry.register(None);
    let by_key = registry.register(None);
    let by_password = registry.register(None);
    let linked = registry.register(None);
    let new_key = registry.register(None);
    actor.set_authenticated(key(1), Access::Member);
    by_key.set_authenticated(key(2), Access::Member);
    by_password.set_authenticated_by_password(Access::Member);
    linked.set_authenticated(key(3), Access::Member);
    linked.mark_password();
    new_key.set_authenticated(key(9), Access::Member);
    for guard in [&actor, &by_key, &by_password, &linked, &new_key] {
        guard.set_account(account);
    }

    let ended = actor.revoke_others(account, &Revocation::Password);
    assert_eq!(ended, vec![by_password.id(), linked.id()]);
    assert!(!by_key.revoke_signal().is_revoked());

    // Recovery with `new_key`'s key: every other key goes (the earlier ones included), but not
    // a session already on the key recovery bound.
    let ended = new_key.revoke_others(account, &Revocation::Recovery(key(9)));
    assert_eq!(ended, vec![actor.id(), by_key.id()]);
    assert!(!new_key.revoke_signal().is_revoked());
}

#[tokio::test]
async fn the_signal_is_remembered_if_it_fires_before_the_session_waits() {
    let registry = Arc::new(SessionRegistry::default());
    let account = AccountId::from_raw(1);
    let (a, b) = (registry.register(None), registry.register(None));
    for guard in [&a, &b] {
        guard.set_authenticated_by_password(Access::Member);
        guard.set_account(account);
    }
    let _ = a.revoke_others(account, &Revocation::Password);
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        b.revoke_signal().revoked(),
    )
    .await
    .unwrap();
}
