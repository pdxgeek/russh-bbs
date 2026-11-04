//! Tie a new account to a key, a password or both inside the transaction that creates it (D94).

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

/// Insert the key and/or password row for `account`.
pub(super) async fn bind(
    conn: &mut SqliteConnection,
    account: i64,
    binding: Binding<'_>,
) -> Result<(), BindFailure> {
    match binding {
        Binding::KeyAndPassword(fingerprint, hash) => {
            bind_key(conn, account, fingerprint).await?;
            bind_password(conn, account, hash).await
        }
        Binding::Key(fingerprint) => bind_key(conn, account, fingerprint).await,
        Binding::Password(hash) => bind_password(conn, account, hash).await,
    }
}

async fn bind_key(
    conn: &mut SqliteConnection,
    account: i64,
    fingerprint: &str,
) -> Result<(), BindFailure> {
    sqlx::query!(
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
    })
}

async fn bind_password(
    conn: &mut SqliteConnection,
    account: i64,
    hash: &str,
) -> Result<(), BindFailure> {
    sqlx::query!(
        "INSERT INTO account_password (account_id, hash) VALUES (?, ?)",
        account,
        hash,
    )
    .execute(&mut *conn)
    .await
    .map(|_| ())
    .map_err(|error| BindFailure::Store(backend(error)))
}
