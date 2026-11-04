//! The founder claim over a real russh client: claim with the printed token, reconnect as the
//! sysop, a second key and a reused token fail, the claim survives a restart, and the claim
//! entry reveals nothing in either state. Tokens come from the server's own console output
//! (captured through the start-up writer); keys are generated here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl,
    missing_docs
)]

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use common::{Client, fast_limits, new_client_key};
use russh::ChannelMsg;
use russh::client::{self, AuthResult, Handle};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh_bbs_core::access::{Access, Fingerprint};
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore};
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use tempfile::TempDir;
use tokio::time::timeout;

struct Rig {
    server: SshServer,
    store: SqliteStore,
    token: Option<String>,
    dir: TempDir,
}

fn relaxed() -> Limits {
    Limits {
        ip_rate: russh_bbs_core::waiting_room::RatePolicy::new(1_000, Duration::from_millis(1)),
        key_rate: russh_bbs_core::waiting_room::RatePolicy::new(1_000, Duration::from_millis(1)),
        ..fast_limits()
    }
}

impl Rig {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self::start_in(dir).await
    }

    /// Start on the database in `dir`, as a process start would: print the token if unclaimed.
    async fn start_in(dir: TempDir) -> Self {
        let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
        let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
        let mut console = Vec::new();
        let founder = founder::start(&store, &mut console).await.unwrap();
        let console = String::from_utf8(console).unwrap();
        let token = console
            .lines()
            .find(|l| l.starts_with("    "))
            .map(|l| l.trim().to_owned());
        let config = ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            limits: relaxed(),
        };
        let server = SshServer::start_with_founder(
            &config,
            &host_key,
            Arc::new(store.clone()),
            Arc::new(founder),
        )
        .await
        .unwrap();
        Self {
            server,
            store,
            token,
            dir,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    async fn restart(self) -> Self {
        self.server.shutdown().await;
        self.store.close().await;
        Self::start_in(self.dir).await
    }
}

async fn login(addr: SocketAddr, user: &str, key: &PrivateKey) -> Handle<Client> {
    let config = Arc::new(client::Config::default());
    let mut session = timeout(common::WAIT, client::connect(config, addr, Client))
        .await
        .unwrap()
        .unwrap();
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

/// How long the channel must stay quiet, once the expected text has arrived, before the screen
/// is taken as complete. Only trailing repaints are waited out; the expected text is awaited by
/// content with a deadline (`common::WAIT`), never by this pause.
const SETTLE: Duration = Duration::from_millis(200);

/// Read channel data into `raw` until `done(&raw)` holds, bounded by `common::WAIT`. Returns
/// false when the deadline passed or the channel closed first.
async fn read_while(
    channel: &mut russh::Channel<client::Msg>,
    raw: &mut String,
    done: impl Fn(&str) -> bool,
) -> bool {
    let reached = timeout(common::WAIT, async {
        while !done(raw) {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => raw.push_str(&String::from_utf8_lossy(&data)),
                Some(_) => {}
                None => return false,
            }
        }
        true
    })
    .await;
    reached.unwrap_or(false)
}

/// Keep reading until the channel has been quiet for `SETTLE` (or closes).
async fn read_quiet(channel: &mut russh::Channel<client::Msg>, raw: &mut String) {
    while let Ok(msg) = timeout(SETTLE, channel.wait()).await {
        match msg {
            Some(ChannelMsg::Data { data }) => raw.push_str(&String::from_utf8_lossy(&data)),
            Some(_) => {}
            None => break,
        }
    }
}

/// The first screen: wait for any output, then for the server to finish painting it.
async fn words(channel: &mut russh::Channel<client::Msg>) -> String {
    let mut raw = String::new();
    read_while(channel, &mut raw, |raw| !raw.is_empty()).await;
    read_quiet(channel, &mut raw).await;
    printable(&raw)
}

/// Wait (with a deadline) until the screen shows one of `needles`, then let trailing repaints
/// finish. If none ever shows, the text seen so far is returned and the caller's assertion fails
/// with it.
async fn words_until(channel: &mut russh::Channel<client::Msg>, needles: &[&str]) -> String {
    let mut raw = String::new();
    let seen = read_while(channel, &mut raw, |raw| {
        let text = printable(raw);
        needles.iter().any(|n| text.contains(n))
    })
    .await;
    if seen {
        read_quiet(channel, &mut raw).await;
    }
    printable(&raw)
}

/// The printable text of `raw`, joined as words.
fn printable(raw: &str) -> String {
    // Drop CSI sequences, then collapse box-drawing and whitespace.
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            out.push(' ');
        } else if c.is_control() || "│┌┐└┘─".contains(c) {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Open a shell as `user`, drain the first screen, and return the channel and that screen.
async fn shell(session: &Handle<Client>) -> (russh::Channel<client::Msg>, String) {
    let mut channel = common::open_shell(session, 80, 24).await;
    let first = words(&mut channel).await;
    (channel, first)
}

/// The password every claimant in these tests chooses (the claim also asks for one, D94 (6)).
const FOUNDER_PASSWORD: &str = "a founder's long password";

async fn submit(channel: &mut russh::Channel<client::Msg>, token: &str) -> String {
    for line in [token, FOUNDER_PASSWORD, FOUNDER_PASSWORD] {
        channel
            .data_bytes(format!("{line}\r").into_bytes())
            .await
            .unwrap();
    }
    // Either outcome ends the wait; the callers assert which one it was.
    words_until(channel, &["Signed in as founder.", REJECTED]).await
}

async fn access_of(rig: &Rig, key: &PrivateKey) -> Access {
    let fp =
        Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
    russh_bbs_core::access::decide_access(&rig.store, &fp).await
}

const REJECTED: &str = "That token was not accepted.";

#[tokio::test]
async fn the_full_claim_story() {
    let rig = Rig::start().await;
    let token = rig.token.clone().expect("fresh board prints a token");
    let key_a = new_client_key();
    let key_b = new_client_key();

    // Key A claims.
    let session = login(rig.addr(), "claim", &key_a).await;
    let (mut channel, prompt) = shell(&session).await;
    assert!(prompt.contains("Setup token"), "{prompt}");
    let after = submit(&mut channel, &token).await;
    assert!(after.contains("Signed in as founder."), "{after}");
    assert!(!after.contains(&token));
    assert_eq!(access_of(&rig, &key_a).await, Access::Member);
    common::remember(key_a.public_key().fingerprint(HashAlg::Sha256), "founder");
    // The claim wrote its audit row in its transaction (M2-C): one row, no token in it.
    let rows =
        russh_bbs_core::audit::known_events(rig.store.page(&AuditQuery::default()).await.unwrap());
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].event.action, Action::FounderClaimed);
    assert!(!format!("{rows:?}").contains(&token));

    // Key A reconnects with an ordinary user name and sees the login screen (M1-E), which
    // reports the earlier login from the claim session.
    let again = login(rig.addr(), "anyone", &key_a).await;
    let (_c, screen) = shell(&again).await;
    assert!(screen.contains("Signed in as founder."), "{screen}");
    assert!(screen.contains("Last login:"), "{screen}");
    assert!(!screen.contains("Waiting room"));

    // Key B presents the same token: refused with the generic message, no access.
    let other = login(rig.addr(), "claim", &key_b).await;
    let (mut b_channel, _) = shell(&other).await;
    let refused = submit(&mut b_channel, &token).await;
    assert!(refused.contains(REJECTED), "{refused}");
    assert!(!refused.contains("Signed in"));
    assert_eq!(access_of(&rig, &key_b).await, Access::Restricted);
    // A refused claim writes no audit row.
    assert_eq!(
        russh_bbs_core::audit::known_events(rig.store.page(&AuditQuery::default()).await.unwrap())
            .len(),
        1
    );

    // Restart on the same database: key A is still the sysop, no new token is printed, and
    // the old token still fails.
    let rig = rig.restart().await;
    assert!(rig.token.is_none(), "a claimed board prints no token");
    let back = login(rig.addr(), "anyone", &key_a).await;
    let (_c, screen) = shell(&back).await;
    assert!(screen.contains("Signed in as founder."), "{screen}");
    let late = login(rig.addr(), "claim", &key_b).await;
    let (mut late_channel, _) = shell(&late).await;
    assert!(submit(&mut late_channel, &token).await.contains(REJECTED));
    assert_eq!(access_of(&rig, &key_b).await, Access::Restricted);
    rig.server.shutdown().await;
}

#[tokio::test]
async fn two_keys_racing_with_the_same_token_make_exactly_one_sysop() {
    let rig = Rig::start().await;
    let token = rig.token.clone().unwrap();
    let keys = [new_client_key(), new_client_key()];
    let mut tasks = Vec::new();
    for key in &keys {
        let (addr, key, token) = (rig.addr(), key.clone(), token.clone());
        tasks.push(tokio::spawn(async move {
            let session = login(addr, "claim", &key).await;
            let (mut channel, _) = shell(&session).await;
            submit(&mut channel, &token).await
        }));
    }
    let mut wins = 0;
    for task in tasks {
        let screen = task.await.unwrap();
        if screen.contains("Signed in as founder.") {
            wins += 1;
        } else {
            assert!(screen.contains(REJECTED), "{screen}");
        }
    }
    assert_eq!(wins, 1);
    let sysops = [
        access_of(&rig, &keys[0]).await,
        access_of(&rig, &keys[1]).await,
    ]
    .into_iter()
    .filter(|a| *a == Access::Member)
    .count();
    assert_eq!(sysops, 1);
    rig.server.shutdown().await;
}

#[tokio::test]
async fn a_wrong_token_and_a_claimed_board_answer_with_the_same_words() {
    // Unclaimed board, wrong token.
    let unclaimed = Rig::start().await;
    let key = new_client_key();
    let session = login(unclaimed.addr(), "claim", &key).await;
    let (mut channel, prompt_unclaimed) = shell(&session).await;
    let wrong_unclaimed = submit(&mut channel, "AAAA-BBBB-CCCC-DDDD").await;
    assert!(wrong_unclaimed.contains(REJECTED));
    // A token typed after a rejection still works on an unclaimed board (not a lockout oracle
    // either way): the session simply keeps its attempts.
    let token = unclaimed.token.clone().unwrap();

    // Claimed board, same wrong token, a stranger's key.
    let claimed = Rig::start().await;
    let founder_key = new_client_key();
    let claim_token = claimed.token.clone().unwrap();
    let s = login(claimed.addr(), "claim", &founder_key).await;
    let (mut c, _) = shell(&s).await;
    submit(&mut c, &claim_token).await;
    let stranger = login(claimed.addr(), "claim", &new_client_key()).await;
    let (mut sc, prompt_claimed) = shell(&stranger).await;
    let wrong_claimed = submit(&mut sc, "AAAA-BBBB-CCCC-DDDD").await;

    // The prompt and the failure are word for word the same in both states.
    assert_eq!(prompt_unclaimed, prompt_claimed);
    assert_eq!(wrong_unclaimed, wrong_claimed);

    // And the unclaimed board is still claimable afterwards.
    let ok = submit(&mut channel, &token).await;
    assert!(ok.contains("Signed in as founder"), "{ok}");
    unclaimed.server.shutdown().await;
    claimed.server.shutdown().await;
}

#[tokio::test]
async fn the_waiting_room_shows_nothing_extra_in_either_state() {
    // The ordinary user name gets the ordinary waiting room before and after the claim, and
    // that screen never mentions a claim, a token or the founder.
    let rig = Rig::start().await;
    let stranger = new_client_key();
    let before = {
        let s = login(rig.addr(), "anyone", &stranger).await;
        shell(&s).await.1
    };
    let token = rig.token.clone().unwrap();
    let founder_key = new_client_key();
    let s = login(rig.addr(), "claim", &founder_key).await;
    let (mut c, _) = shell(&s).await;
    submit(&mut c, &token).await;
    let after = {
        let s = login(rig.addr(), "anyone", &stranger).await;
        shell(&s).await.1
    };
    assert_eq!(
        before, after,
        "the waiting room is identical before and after"
    );
    let lower = after.to_lowercase();
    for word in ["claim", "token", "founder", "setup", "sysop", "unclaimed"] {
        assert!(
            !lower.contains(word),
            "waiting room mentions {word:?}: {after}"
        );
    }
    rig.server.shutdown().await;
}

#[tokio::test]
async fn a_session_gets_a_few_attempts_then_is_disconnected() {
    let rig = Rig::start().await;
    let session = login(rig.addr(), "claim", &new_client_key()).await;
    let (mut channel, _) = shell(&session).await;
    for _ in 0..3 {
        submit(&mut channel, "NOPE").await;
    }
    // After the third rejection the server closes the channel.
    let closed = timeout(common::WAIT, async {
        loop {
            if channel.wait().await.is_none() {
                return;
            }
        }
    })
    .await;
    assert!(
        closed.is_ok(),
        "the session was not closed after three attempts"
    );
    rig.server.shutdown().await;
}

#[tokio::test]
async fn claim_attempts_are_rate_limited_per_key_across_connections() {
    let rig = Rig::start().await;
    let token = rig.token.clone().unwrap();
    let key = new_client_key();
    // The default per-key claim limit is a burst of 3. Burn it with wrong tokens on separate
    // connections, then even the right token is answered with the generic failure.
    for _ in 0..3 {
        let s = login(rig.addr(), "claim", &key).await;
        let (mut c, _) = shell(&s).await;
        assert!(submit(&mut c, "NOPE").await.contains(REJECTED));
    }
    let s = login(rig.addr(), "claim", &key).await;
    let (mut c, _) = shell(&s).await;
    let limited = submit(&mut c, &token).await;
    assert!(limited.contains(REJECTED), "{limited}");
    assert!(!limited.contains("Signed in"));
    assert_eq!(access_of(&rig, &key).await, Access::Restricted);
    rig.server.shutdown().await;
}
