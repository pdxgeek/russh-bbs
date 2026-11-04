//! Tie a new account to a key or a password inside the transaction that creates it (D94).

use russh_bbs_core::store::StoreError;
use sqlx::SqliteConnection;
use sqlx::error::ErrorKind;

use crate::binding::Binding;
use crate::error::{backend, kind};

/// Why binding failed. The transaction rolls back either way.
pub(super) enum BindFailure {
    /// The key is already bound to an account.
    KeyTaken,
    /// The store failed.
    Store(StoreError),
}

/// Insert the key or password row for `account`.
pub(super) async fn bind(
    conn: &mut SqliteConnection,
    account: i64,
    binding: Binding<'_>,
) -> Result<(), BindFailure> {
    match binding {
        Binding::Key(fingerprint) => sqlx::query!(
            "INSERT INTO account_keys (fingerprint, account_id) VALUES (?, ?)",
            fingerprint,
            account,
        )
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(|error| match kind(&error) {
            Some(ErrorKind::UniqueViolation) => BindFailure::KeyTaken,
            _ => BindFailure::Store(backend(error)),
        }),
        Binding::Password(hash) => sqlx::query!(
            "INSERT INTO account_password (account_id, hash) VALUES (?, ?)",
            account,
            hash,
        )
        .execute(&mut *conn)
        .await
        .map(|_| ())
        .map_err(|error| BindFailure::Store(backend(error))),
    }
}
