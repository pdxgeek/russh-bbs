//! Shared helpers for the SSH integration tests. Keys are generated here, at test time, in
//! temporary directories; nothing is read from or written to the operator's data directory.
#![allow(
    dead_code,
    unreachable_pub,
    clippy::unwrap_used,
    clippy::expect_used,
    missing_docs
)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, AuthResult, Handle, Msg};
use russh::keys::{Algorithm, PrivateKey, PrivateKeyWithHashAlg};
use russh::{Channel, ChannelMsg};
use russh_bbs_core::access::NoMembers;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use tempfile::TempDir;
use tokio::time::timeout;

pub const WAIT: Duration = Duration::from_secs(10);

static HANDLES: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// Remember that the key with this fingerprint belongs to the account `handle`, so a test that
/// signs in "as anyone" with it uses the handle as the SSH user name (D94 (10): a key signs in
/// only under the handle it is bound to).
#[allow(clippy::needless_pass_by_value)]
pub fn remember(fingerprint: impl ToString, handle: &str) {
    HANDLES
        .lock()
        .unwrap()
        .push((fingerprint.to_string(), handle.to_owned()));
}

/// The SSH user name for `key`: `asked` as it is, except the placeholder `anyone`, which becomes
/// the handle of the account the key was remembered for, or `new` (the waiting room) for a key
/// nobody remembered.
pub fn user_for(key: &PrivateKey, asked: &str) -> String {
    if asked != "anyone" {
        return asked.to_owned();
    }
    let fingerprint = key
        .public_key()
        .fingerprint(russh::keys::HashAlg::Sha256)
        .to_string();
    HANDLES
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|(known, _)| *known == fingerprint)
        .map_or_else(|| "new".to_owned(), |(_, handle)| handle.clone())
}

pub struct TestServer {
    pub server: SshServer,
    pub host_key: HostKey,
    pub dir: TempDir,
}

impl TestServer {
    pub fn addr(&self) -> SocketAddr {
        self.server.local_addr()
    }
}

/// Fast limits so failure paths do not slow the suite.
pub fn fast_limits() -> Limits {
    Limits {
        auth_rejection_time: Duration::from_millis(10),
        ..Limits::default()
    }
}

pub async fn start_server(limits: Limits) -> TestServer {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("host_key");
    let host_key = HostKey::load_or_create(&path).unwrap();
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        limits,
    };
    let server = SshServer::start(&config, &host_key, Arc::new(NoMembers))
        .await
        .unwrap();
    TestServer {
        server,
        host_key,
        dir,
    }
}

pub fn new_client_key() -> PrivateKey {
    PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap()
}

pub struct Client;

impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

pub async fn connect(addr: SocketAddr) -> Handle<Client> {
    let config = Arc::new(client::Config::default());
    timeout(WAIT, client::connect(config, addr, Client))
        .await
        .unwrap()
        .unwrap()
}

/// Connect and authenticate with a fresh key. Returns the session and the key.
pub async fn login(addr: SocketAddr) -> (Handle<Client>, PrivateKey) {
    let key = new_client_key();
    let mut session = connect(addr).await;
    let result = session
        .authenticate_publickey(
            "new",
            PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
        )
        .await
        .unwrap();
    assert!(
        matches!(result, AuthResult::Success),
        "pubkey auth: {result:?}"
    );
    (session, key)
}

/// Open a session channel, request a PTY of `cols`x`rows`, and start the shell.
pub async fn open_shell(session: &Handle<Client>, cols: u32, rows: u32) -> Channel<Msg> {
    let mut channel = session.channel_open_session().await.unwrap();
    channel
        .request_pty(true, "xterm-256color", cols, rows, 0, 0, &[])
        .await
        .unwrap();
    expect_reply(&mut channel, true).await;
    channel.request_shell(true).await.unwrap();
    expect_reply(&mut channel, true).await;
    channel
}

/// Wait for the next Success/Failure reply, skipping unrelated messages.
pub async fn expect_reply(channel: &mut Channel<Msg>, success: bool) {
    let reply = next_reply(channel).await;
    assert_eq!(reply, Some(success), "expected success={success}");
}

pub async fn next_reply(channel: &mut Channel<Msg>) -> Option<bool> {
    loop {
        match timeout(WAIT, channel.wait()).await.unwrap() {
            Some(ChannelMsg::Success) => return Some(true),
            Some(ChannelMsg::Failure) => return Some(false),
            Some(_) => {}
            None => return None,
        }
    }
}

/// Read channel data until `needle` shows up, returning everything seen.
pub async fn read_until(channel: &mut Channel<Msg>, needle: &str) -> String {
    let mut seen = String::new();
    while !seen.contains(needle) {
        match timeout(WAIT, channel.wait()).await.unwrap() {
            Some(ChannelMsg::Data { data }) => seen.push_str(&String::from_utf8_lossy(&data)),
            Some(_) => {}
            None => panic!("channel closed before {needle:?}; saw {seen:?}"),
        }
    }
    seen
}

/// Poll the server's session registry until `predicate` holds.
pub async fn eventually(
    server: &SshServer,
    predicate: impl Fn(&[russh_bbs_server::ssh::SessionSnapshot]) -> bool,
) {
    timeout(WAIT, async {
        loop {
            if predicate(&server.sessions()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("condition not reached in time");
}
