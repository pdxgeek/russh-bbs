//! A link that cannot carry the login intro (FIX-M35-3, QA M3.5 M3, D124): the intro is paced
//! by the client's own acknowledgement of each frame, gives up within a bounded number of bytes
//! and a bounded time when the client is behind or silent, and a key reaches the static screen
//! promptly. A client that keeps up still gets the whole intro. Real russh clients; the "slow"
//! client is one that answers the server's cursor-position requests late, or never.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl,
    missing_docs
)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Client, fast_limits, new_client_key};
use russh::client::{self, AuthResult, Handle};
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg};
use russh::{Channel, ChannelMsg};
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use tempfile::TempDir;
use tokio::time::timeout;

/// The request the server ends each effect frame with, and the reply a terminal gives.
const PROBE: &[u8] = b"\x1b[6n";
const REPLY: &[u8] = b"\x1b[24;1R";

struct Rig {
    server: SshServer,
    token: String,
    _dir: TempDir,
}

impl Rig {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
        let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
        let mut console = Vec::new();
        let gate = founder::start(&store, &mut console).await.unwrap();
        let token = String::from_utf8(console)
            .unwrap()
            .lines()
            .find(|l| l.starts_with("    "))
            .map(|l| l.trim().to_owned())
            .expect("a fresh board prints a token");
        let limits = Limits {
            ip_rate: russh_bbs_core::waiting_room::RatePolicy::new(1_000, Duration::from_millis(1)),
            key_rate: russh_bbs_core::waiting_room::RatePolicy::new(
                1_000,
                Duration::from_millis(1),
            ),
            ..fast_limits()
        };
        let config = ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            limits,
        };
        let server = SshServer::start_with_founder(
            &config,
            &host_key,
            Arc::new(store.clone()),
            Arc::new(gate),
        )
        .await
        .unwrap();
        Self {
            server,
            token,
            _dir: dir,
        }
    }
}

/// How the test client treats the server's cursor-position requests.
#[derive(Clone, Copy)]
enum Answers {
    /// Reply at once: a link that keeps up.
    Promptly,
    /// Reply after this long: a slow link (the reply is queued behind the data on a real one).
    After(Duration),
    /// Never reply: a terminal that ignores the request.
    Never,
}

/// What the client saw.
#[derive(Default)]
struct Seen {
    bytes: usize,
    raw: Vec<u8>,
    probes: usize,
}

impl Seen {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.raw).into_owned()
    }
}

async fn connect_as(rig: &Rig, user: &str, key: &PrivateKey) -> Handle<Client> {
    let mut session = common::connect(rig.server.local_addr()).await;
    let result = session
        .authenticate_publickey(
            &common::user_for(key, user),
            PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
        )
        .await
        .unwrap();
    assert!(matches!(result, AuthResult::Success), "{result:?}");
    session
}

/// Read for up to `span` (or until `until` returns true), answering probes as `answers` says.
async fn watch(
    channel: &mut Channel<client::Msg>,
    seen: &mut Seen,
    answers: Answers,
    span: Duration,
    until: impl Fn(&Seen) -> bool,
) {
    let end = Instant::now() + span;
    let mut pending: Vec<Instant> = Vec::new();
    while Instant::now() < end && !until(seen) {
        let wake = pending.first().map_or(end, |due| (*due).min(end));
        let wait = wake.saturating_duration_since(Instant::now());
        if let Ok(Some(ChannelMsg::Data { data })) = timeout(wait, channel.wait()).await {
            seen.bytes += data.len();
            seen.raw.extend_from_slice(&data);
            let asked = data.windows(PROBE.len()).filter(|w| *w == PROBE).count();
            seen.probes += asked;
            match answers {
                Answers::Promptly => {
                    for _ in 0..asked {
                        channel.data_bytes(REPLY.to_vec()).await.unwrap();
                    }
                }
                Answers::After(delay) => {
                    pending.extend(std::iter::repeat_n(Instant::now() + delay, asked));
                }
                Answers::Never => {}
            }
        }
        while pending.first().is_some_and(|due| *due <= Instant::now()) {
            pending.remove(0);
            channel.data_bytes(REPLY.to_vec()).await.unwrap();
        }
    }
}

/// Claim the board and return the channel just after the last password is sent, with the
/// intro starting.
async fn claim(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<client::Msg>) {
    let session = connect_as(rig, "claim", key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    let mut seen = Seen::default();
    watch(
        &mut channel,
        &mut seen,
        Answers::Promptly,
        Duration::from_millis(600),
        |_| false,
    )
    .await;
    for line in [
        &rig.token[..],
        "founder password one",
        "founder password one",
    ] {
        channel
            .data_bytes(format!("{line}\r").into_bytes())
            .await
            .unwrap();
        watch(
            &mut channel,
            &mut seen,
            Answers::Promptly,
            Duration::from_millis(500),
            |_| false,
        )
        .await;
    }
    (session, channel)
}

/// Set the board up once (claim, icon check) with a fast client, log off, and sign in again:
/// the second sign-in shows the login screen and its intro from the first byte.
async fn second_sign_in(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<client::Msg>) {
    let (session, mut channel) = claim(rig, key).await;
    let mut skip = Seen::default();
    for reply in [" ", "n", "y", "l"] {
        channel.data_bytes(reply.as_bytes().to_vec()).await.unwrap();
        watch(
            &mut channel,
            &mut skip,
            Answers::Promptly,
            Duration::from_millis(500),
            |_| false,
        )
        .await;
    }
    drop(channel);
    drop(session);
    let session = connect_as(rig, "founder", key).await;
    let channel = common::open_shell(&session, 80, 24).await;
    (session, channel)
}

/// Sign in a second time with a client that treats probes as `answers` says, and watch the
/// intro for `span`.
async fn run_intro(answers: Answers, span: Duration) -> Seen {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = second_sign_in(&rig, &key).await;
    let mut seen = Seen::default();
    watch(&mut channel, &mut seen, answers, span, |_| false).await;
    rig.server.shutdown().await;
    seen
}

#[tokio::test]
async fn a_client_that_keeps_up_gets_the_whole_intro() {
    let seen = run_intro(Answers::Promptly, Duration::from_secs(3)).await;
    assert!(seen.probes >= 5, "frames were probed: {}", seen.probes);
    assert!(
        seen.bytes > 20 * 1024,
        "normal speed is unchanged: {} bytes",
        seen.bytes
    );
}

#[tokio::test]
async fn a_client_that_never_answers_gets_a_bounded_intro_then_the_static_screen() {
    let seen = run_intro(Answers::Never, Duration::from_secs(4)).await;
    // The first frame or two, then nothing more is queued behind a silent client: far below the
    // 84 KB the whole intro takes at 80x24.
    assert!(seen.bytes < 24 * 1024, "{} bytes", seen.bytes);
    assert!(seen.probes >= 1, "the intro did start");
}

#[tokio::test]
async fn a_late_client_is_given_up_on_within_a_bounded_time() {
    // Replies come 5 s late: the server must stop drawing effects after its deadline and not
    // keep sending for the length of the intro on every tick.
    let seen = run_intro(
        Answers::After(Duration::from_secs(5)),
        Duration::from_secs(4),
    )
    .await;
    assert!(seen.bytes < 24 * 1024, "{} bytes", seen.bytes);
}

#[tokio::test]
async fn a_key_during_the_intro_reaches_the_static_screen_promptly_on_a_slow_link() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = second_sign_in(&rig, &key).await;
    // The intro is running and the client has stopped answering. A key is handled in one cycle,
    // and what follows is the static screen, not more frames.
    let mut seen = Seen::default();
    watch(
        &mut channel,
        &mut seen,
        Answers::Never,
        Duration::from_millis(400),
        |_| false,
    )
    .await;
    assert!(seen.probes >= 1, "the intro was running");
    let before = seen.bytes;
    channel.data_bytes(b" ".to_vec()).await.unwrap();
    let pressed = Instant::now();
    watch(
        &mut channel,
        &mut seen,
        Answers::Never,
        Duration::from_secs(3),
        |s| s.text().contains("Main menu"),
    )
    .await;
    assert!(seen.text().contains("Main menu"), "{}", seen.text());
    assert!(
        pressed.elapsed() < Duration::from_millis(1500),
        "{:?}",
        pressed.elapsed()
    );
    assert!(
        seen.bytes - before < 16 * 1024,
        "{} bytes after the key",
        seen.bytes - before
    );
    rig.server.shutdown().await;
}
