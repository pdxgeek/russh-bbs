//! Authentication and host key behavior against a real russh client over loopback.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl,
    clippy::items_after_statements,
    missing_docs
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{connect, fast_limits, login, start_server};
use russh::MethodKind;
use russh::client::{AuthResult, KeyboardInteractiveAuthResponse};
use russh_bbs_core::access::Access;
use russh_bbs_core::access::NoMembers;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};

#[tokio::test]
async fn public_key_auth_succeeds() {
    let t = start_server(fast_limits()).await;
    let (session, _key) = login(t.addr()).await;
    session
        .disconnect(russh::Disconnect::ByApplication, "", "")
        .await
        .unwrap();
}

#[tokio::test]
async fn password_auth_fails() {
    let t = start_server(fast_limits()).await;
    let mut session = connect(t.addr()).await;
    let result = session
        .authenticate_password("anyone", "hunter2")
        .await
        .unwrap();
    assert!(matches!(result, AuthResult::Failure { .. }), "{result:?}");
}

#[tokio::test]
async fn none_auth_fails_and_the_methods_are_publickey_and_password() {
    let t = start_server(fast_limits()).await;
    let mut session = connect(t.addr()).await;
    let result = session.authenticate_none("anyone").await.unwrap();
    let AuthResult::Failure {
        remaining_methods, ..
    } = result
    else {
        panic!("none auth must not succeed");
    };
    let offered: Vec<_> = remaining_methods.iter().copied().collect();
    assert_eq!(offered, vec![MethodKind::PublicKey, MethodKind::Password]);
}

#[tokio::test]
async fn a_client_with_no_key_reaches_the_waiting_room_by_password_only_under_the_visitor_names() {
    // D94 (7), (10), D96: `none` is never accepted (OpenSSH would then never offer a key). Under
    // `new` and `claim` a password prompt answered with anything lets a key-less client in; under
    // every other name the same attempt is the one refusal.
    let t = start_server(fast_limits()).await;
    for name in ["alice", "new", "claim"] {
        let mut session = connect(t.addr()).await;
        let result = session.authenticate_none(name).await.unwrap();
        assert!(
            matches!(result, AuthResult::Failure { .. }),
            "{name}: {result:?}"
        );
    }
    for name in ["new", "claim", "NEW"] {
        let mut session = connect(t.addr()).await;
        let result = session.authenticate_password(name, "").await.unwrap();
        assert!(matches!(result, AuthResult::Success), "{name}: {result:?}");
    }
    let mut refusals = Vec::new();
    for name in ["alice", "bob", "nobody-at-all", ""] {
        let mut session = connect(t.addr()).await;
        let result = session.authenticate_password(name, "").await.unwrap();
        refusals.push(format!("{result:?}"));
        assert!(matches!(result, AuthResult::Failure { .. }), "{name}");
    }
    assert!(refusals.iter().all(|r| r == &refusals[0]), "{refusals:?}");
}

#[tokio::test]
async fn keyboard_interactive_auth_fails() {
    let t = start_server(fast_limits()).await;
    let mut session = connect(t.addr()).await;
    let result = session
        .authenticate_keyboard_interactive_start("anyone", None::<String>)
        .await
        .unwrap();
    assert!(
        matches!(result, KeyboardInteractiveAuthResponse::Failure { .. }),
        "{result:?}"
    );
}

#[tokio::test]
async fn an_unknown_key_lands_in_a_restricted_session() {
    let t = start_server(fast_limits()).await;
    let (_session, key) = login(t.addr()).await;

    let sessions = t.server.sessions();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].access, Some(Access::Restricted));
    let expected = key
        .public_key()
        .fingerprint(russh::keys::HashAlg::Sha256)
        .to_string();
    assert_eq!(
        sessions[0].fingerprint.as_ref().map(ToString::to_string),
        Some(expected)
    );
}

#[tokio::test]
async fn the_host_key_fingerprint_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("host_key");
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        limits: fast_limits(),
    };

    let first = HostKey::load_or_create(&path).unwrap();
    let server = SshServer::start(&config, &first, Arc::new(NoMembers))
        .await
        .unwrap();
    let before = server.host_fingerprint().clone();
    server.shutdown().await;

    let second = HostKey::load_or_create(&path).unwrap();
    let server = SshServer::start(&config, &second, Arc::new(NoMembers))
        .await
        .unwrap();
    assert_eq!(&before, server.host_fingerprint());
    server.shutdown().await;
}

#[tokio::test]
async fn the_server_presents_the_fingerprint_it_logs() {
    // The fingerprint a sysop publishes must be what a client really sees on the wire.
    let t = start_server(fast_limits()).await;
    let seen = Arc::new(std::sync::Mutex::new(None));

    struct Capture(Arc<std::sync::Mutex<Option<String>>>);
    impl russh::client::Handler for Capture {
        type Error = russh::Error;
        async fn check_server_key(
            &mut self,
            key: &russh::keys::PublicKeyOrCertificate,
        ) -> Result<bool, Self::Error> {
            if let russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } = key {
                *self.0.lock().unwrap() =
                    Some(key.fingerprint(russh::keys::HashAlg::Sha256).to_string());
            }
            Ok(true)
        }
    }
    let _session = russh::client::connect(
        Arc::new(russh::client::Config::default()),
        t.addr(),
        Capture(Arc::clone(&seen)),
    )
    .await
    .unwrap();

    let wire = seen.lock().unwrap().clone();
    assert_eq!(wire, Some(t.server.host_fingerprint().to_string()));
}

#[tokio::test]
async fn too_many_failed_attempts_disconnects() {
    let limits = Limits {
        max_auth_attempts: 3,
        ..fast_limits()
    };
    let t = start_server(limits).await;
    let mut session = connect(t.addr()).await;
    let mut disconnected = false;
    for _ in 0..10 {
        match session.authenticate_password("anyone", "nope").await {
            Ok(AuthResult::Success) => panic!("password must never succeed"),
            Ok(AuthResult::Failure { .. }) => {}
            Err(_) => {
                disconnected = true;
                break;
            }
        }
    }
    assert!(
        disconnected,
        "server kept accepting attempts past the limit"
    );
}

#[tokio::test]
async fn a_silent_connection_is_dropped_at_the_auth_timeout() {
    let limits = Limits {
        auth_timeout: Duration::from_millis(300),
        ..fast_limits()
    };
    let t = start_server(limits).await;
    let session = connect(t.addr()).await; // handshake done, never authenticates
    tokio::time::timeout(Duration::from_secs(10), async {
        while !session.is_closed() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("server did not drop the unauthenticated connection");
}

#[tokio::test]
async fn a_raw_tcp_stranger_is_dropped_at_the_auth_timeout() {
    use tokio::io::AsyncReadExt;
    let limits = Limits {
        auth_timeout: Duration::from_millis(300),
        ..fast_limits()
    };
    let t = start_server(limits).await;
    let mut tcp = tokio::net::TcpStream::connect(t.addr()).await.unwrap();
    // Send nothing. The server writes its id string, then must hang up.
    let mut sink = Vec::new();
    tokio::time::timeout(Duration::from_secs(10), tcp.read_to_end(&mut sink))
        .await
        .expect("server held a silent TCP connection open")
        .unwrap();
    assert!(sink.starts_with(b"SSH-2.0-"));
}

#[tokio::test]
async fn connections_beyond_the_cap_are_dropped() {
    let limits = Limits {
        max_connections: 1,
        ..fast_limits()
    };
    let t = start_server(limits).await;
    let (_first, _) = login(t.addr()).await;

    let config = Arc::new(russh::client::Config::default());
    let second = tokio::time::timeout(
        Duration::from_secs(10),
        russh::client::connect(config, t.addr(), common::Client),
    )
    .await
    .unwrap();
    assert!(
        second.is_err(),
        "second connection should be refused at the cap"
    );
}
