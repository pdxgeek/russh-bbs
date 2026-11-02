//! The SSH server with the real SQLite member directory: a stored member key gets
//! `Access::Member`, any other key stays `Restricted`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl,
    missing_docs
)]

mod common;

use std::sync::Arc;

use common::{connect, eventually, fast_limits, new_client_key};
use russh::client::AuthResult;
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh_bbs_core::access::Slug;
use russh_bbs_core::access::{Access, Fingerprint};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountState, AccountStore, NewAccount};
use russh_bbs_server::ssh::{HostKey, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;

fn fingerprint_of(key: &PrivateKey) -> Fingerprint {
    Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap()
}

async fn authenticate(
    addr: std::net::SocketAddr,
    key: &PrivateKey,
) -> russh::client::Handle<common::Client> {
    let mut session = connect(addr).await;
    let result = session
        .authenticate_publickey(
            "anyone",
            PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
        )
        .await
        .unwrap();
    assert!(matches!(result, AuthResult::Success), "{result:?}");
    session
}

async fn access_for(server: &SshServer, key: &PrivateKey) -> Option<Access> {
    let want = fingerprint_of(key);
    eventually(server, |s| {
        s.iter()
            .any(|x| x.fingerprint.as_ref() == Some(&want) && x.access.is_some())
    })
    .await;
    server
        .sessions()
        .into_iter()
        .find(|s| s.fingerprint.as_ref() == Some(&want))
        .and_then(|s| s.access)
}

#[tokio::test]
async fn a_stored_member_key_is_member_and_an_unknown_key_is_restricted() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();

    let member_key = new_client_key();
    let account = store
        .create_account(NewAccount {
            username: sanitize_username("alice").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    store
        .attach_fingerprint(account.id, &fingerprint_of(&member_key))
        .await
        .unwrap();

    let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        limits: fast_limits(),
    };
    let server = SshServer::start(&config, &host_key, Arc::new(store.clone()))
        .await
        .unwrap();

    let unknown_key = new_client_key();
    let _member = authenticate(server.local_addr(), &member_key).await;
    let _unknown = authenticate(server.local_addr(), &unknown_key).await;

    assert_eq!(access_for(&server, &member_key).await, Some(Access::Member));
    assert_eq!(
        access_for(&server, &unknown_key).await,
        Some(Access::Restricted)
    );

    server.shutdown().await;
    store.close().await;
}

#[tokio::test]
async fn a_suspended_members_key_is_restricted() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
    let key = new_client_key();
    let account = store
        .create_account(NewAccount {
            username: sanitize_username("mallory").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Suspended,
        })
        .await
        .unwrap();
    store
        .attach_fingerprint(account.id, &fingerprint_of(&key))
        .await
        .unwrap();

    let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        limits: fast_limits(),
    };
    let server = SshServer::start(&config, &host_key, Arc::new(store.clone()))
        .await
        .unwrap();
    let _session = authenticate(server.local_addr(), &key).await;
    assert_eq!(access_for(&server, &key).await, Some(Access::Restricted));
    server.shutdown().await;
    store.close().await;
}
