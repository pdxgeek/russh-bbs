//! The input cursor blinks over a real ssh session (FIX-M35-7, QA M3.5 N1, D127): with motion on
//! an open input redraws its cursor about twice a second in a few bytes, never faster than the
//! D49 cap; with motion off, or in a session that wants a still screen, nothing is sent while
//! the screen is idle. Real russh clients against the real SQLite store.
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
use russh::keys::{HashAlg, PrivateKeyWithHashAlg};
use russh::{Channel, ChannelMsg};
use russh_bbs_core::access::{Fingerprint, Slug};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountState, AccountStore, NewAccount};
use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore, ThemeName};
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use tokio::time::timeout;

/// Where an input is open.
#[derive(Clone, Copy, Debug)]
enum Input {
    /// The chat room's line (`c`, Enter on the lobby).
    Chat,
    /// The forum search line (`f`, then `s`).
    ForumSearch,
}

/// One frame the client received: when, and how many bytes.
struct Frame {
    at: Instant,
    bytes: usize,
}

/// Sign in a member with `motion` as given and the neon style, open `input`, and return what
/// arrives over `span` once the screen is idle.
async fn idle_frames(motion: bool, input: Input, span: Duration) -> Vec<Frame> {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
    let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
    let limits = Limits {
        ip_rate: russh_bbs_core::waiting_room::RatePolicy::new(1_000, Duration::from_millis(1)),
        key_rate: russh_bbs_core::waiting_room::RatePolicy::new(1_000, Duration::from_millis(1)),
        ..fast_limits()
    };
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        limits,
    };
    let server = SshServer::start(&config, &host_key, Arc::new(store.clone()))
        .await
        .unwrap();
    let key = new_client_key();
    let account = store
        .create_account(NewAccount {
            username: sanitize_username("ann").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    let fp =
        Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
    store.attach_fingerprint(account.id, &fp).await.unwrap();
    common::remember(fp, account.username.as_str());
    let settings = TerminalSettings::new(None, None, true, true, Some(Tier::Ascii))
        .unwrap()
        .with_theme(ThemeName::new("neon").unwrap())
        .with_motion(motion);
    store
        .set_terminal_settings(account.id, &settings)
        .await
        .unwrap();

    let mut session: Handle<Client> = common::connect(server.local_addr()).await;
    let result = session
        .authenticate_publickey(
            &common::user_for(&key, "anyone"),
            PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
        )
        .await
        .unwrap();
    assert!(matches!(result, AuthResult::Success), "{result:?}");
    let mut channel = common::open_shell(&session, 80, 24).await;
    quiet(&mut channel).await;
    send(&mut channel, "x").await; // past the login screen
    match input {
        Input::Chat => {
            send(&mut channel, "c").await;
            send(&mut channel, "\r").await;
        }
        Input::ForumSearch => {
            send(&mut channel, "f").await;
            send(&mut channel, "s").await;
        }
    }
    let frames = collect(&mut channel, span).await;
    server.shutdown().await;
    frames
}

async fn send(channel: &mut Channel<client::Msg>, keys: &str) {
    channel.data_bytes(keys.as_bytes().to_vec()).await.unwrap();
    quiet(channel).await;
}

/// Read until the link has been quiet for 300 ms (longer than a blink never is: a blink frame is
/// a half second apart, so this returns inside one dark or lit half).
async fn quiet(channel: &mut Channel<client::Msg>) {
    while let Ok(Some(_)) = timeout(Duration::from_millis(300), channel.wait()).await {}
}

/// Every data message over `span`, with its time and size.
async fn collect(channel: &mut Channel<client::Msg>, span: Duration) -> Vec<Frame> {
    let end = Instant::now() + span;
    let mut frames = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Ok(Some(ChannelMsg::Data { data })) = timeout(left, channel.wait()).await {
            frames.push(Frame {
                at: Instant::now(),
                bytes: data.len(),
            });
        }
    }
    frames
}

#[tokio::test]
async fn the_chat_cursor_blinks_with_motion_on_in_a_few_bytes() {
    let frames = idle_frames(true, Input::Chat, Duration::from_millis(2600)).await;
    assert!(
        frames.len() >= 2,
        "the cursor changed {} times in 2.6 s",
        frames.len()
    );
    assert!(
        frames.iter().all(|f| f.bytes <= 128),
        "a blink frame is a few bytes: {:?}",
        frames.iter().map(|f| f.bytes).collect::<Vec<_>>()
    );
    eprintln!(
        "chat: {} frames, bytes {:?}",
        frames.len(),
        frames.iter().map(|f| f.bytes).collect::<Vec<_>>()
    );
    // About twice a second: far under the 20-a-second D49 cap.
    assert!(frames.len() <= 8, "{} frames in 2.6 s", frames.len());
    for pair in frames.windows(2) {
        assert!(pair[1].at - pair[0].at >= Duration::from_millis(50));
    }
}

#[tokio::test]
async fn the_forum_search_cursor_blinks_with_motion_on() {
    let frames = idle_frames(true, Input::ForumSearch, Duration::from_millis(2600)).await;
    assert!(frames.len() >= 2, "{} frames", frames.len());
    assert!(frames.iter().all(|f| f.bytes <= 128));
}

#[tokio::test]
async fn nothing_is_sent_with_motion_off() {
    for input in [Input::Chat, Input::ForumSearch] {
        let frames = idle_frames(false, input, Duration::from_millis(2600)).await;
        assert!(frames.is_empty(), "{input:?}: {} frames", frames.len());
    }
}
