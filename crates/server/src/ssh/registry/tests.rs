#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use russh_bbs_core::access::{Access, Fingerprint};

use super::SessionRegistry;
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
