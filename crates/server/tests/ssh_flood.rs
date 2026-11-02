//! What one connected client can make the server do (D46, QA M1 H1/M2): a flood of window
//! changes or keystrokes costs a bounded number of redraws and ends the session; a concurrent
//! login is not held up; a client that never reads is dropped instead of being queued for.
//! Real russh clients throughout. Assertions are on counts and bytes (frames drawn, bytes
//! queued), not on how long the machine took, except one generous latency bound.
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

use common::{
    Client, eventually, expect_reply, fast_limits, login, open_shell, read_until, start_server,
};
use russh::client::{self, AuthResult};
use russh::keys::PrivateKeyWithHashAlg;
use russh_bbs_server::ssh::{Limits, SessionSnapshot};
use tokio::time::timeout;

/// Limits with room for the connection setup of the tests below.
fn limits() -> Limits {
    Limits {
        ip_rate: russh_bbs_core::waiting_room::RatePolicy::new(1_000, Duration::from_millis(1)),
        key_rate: russh_bbs_core::waiting_room::RatePolicy::new(1_000, Duration::from_millis(1)),
        ..fast_limits()
    }
}

fn only(sessions: &[SessionSnapshot]) -> SessionSnapshot {
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    sessions[0].clone()
}

/// The most frames a session may have drawn after `elapsed`: one at entry, one per redraw
/// interval, and slack for the cycle in flight.
fn frame_ceiling(elapsed: Duration, interval: Duration) -> u64 {
    let steps = elapsed.as_millis() / interval.as_millis();
    2 + u64::try_from(steps).unwrap() + 1
}

#[tokio::test]
async fn a_burst_of_window_changes_is_coalesced_and_the_last_size_wins() {
    let t = start_server(limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, 80, 24).await;
    read_until(&mut channel, "Disconnect").await;

    let started = Instant::now();
    // 300 requests is inside the burst allowance (two seconds of 200 a second), so nobody is
    // punished for it; it is still ten times what a person's window drag sends.
    for step in 0..300_u32 {
        channel
            .window_change(60 + step % 40, 20 + step % 10, 0, 0)
            .await
            .unwrap();
    }
    channel.window_change(101, 31, 0, 0).await.unwrap();

    eventually(&t.server, |s| {
        s.first()
            .and_then(|x| x.pty_size)
            .map(|p| (p.cols(), p.rows()))
            == Some((101, 31))
    })
    .await;
    // The trailing size is drawn: a full redraw arrives after the flood.
    let frame = read_until(&mut channel, "Waiting").await;
    assert!(frame.contains("\x1b[2J"), "redrawn from scratch: {frame:?}");

    let shot = only(&t.server.sessions());
    let ceiling = frame_ceiling(
        started.elapsed() + Duration::from_millis(200),
        limits().redraw_interval,
    );
    assert!(
        shot.frames_sent <= ceiling,
        "{} frames for 301 window changes in {:?} (ceiling {ceiling})",
        shot.frames_sent,
        started.elapsed()
    );
    assert!(shot.shell_open, "inside its budget, the session lives");
}

#[tokio::test]
async fn a_window_change_flood_past_the_budget_ends_the_session() {
    let t = start_server(limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, 80, 24).await;
    read_until(&mut channel, "Disconnect").await;

    let started = Instant::now();
    let mut sent = 0_u32;
    // Far more than the session may send; stop when the server hangs up.
    while sent < 200_000 && started.elapsed() < Duration::from_secs(20) {
        if channel
            .window_change(40 + sent % 100, 10 + sent % 40, 0, 0)
            .await
            .is_err()
        {
            break;
        }
        sent += 1;
        if sent.is_multiple_of(256) && t.server.sessions().is_empty() {
            break;
        }
    }
    eventually(&t.server, <[SessionSnapshot]>::is_empty).await;
    assert!(sent < 200_000, "the server never hung up");
}

#[tokio::test]
async fn a_keystroke_flood_is_dropped_and_ends_the_session() {
    let t = start_server(limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, 80, 24).await;
    read_until(&mut channel, "Disconnect").await;

    let started = Instant::now();
    let chunk = vec![b'z'; 4096];
    let mut sent_bytes = 0_usize;
    // 8 MiB of keystrokes is 200 times what the session may send in this time.
    while sent_bytes < 8 * 1024 * 1024 && started.elapsed() < Duration::from_secs(20) {
        if channel.data_bytes(chunk.clone()).await.is_err() {
            break;
        }
        sent_bytes += chunk.len();
        if let Some(shot) = t.server.sessions().first() {
            assert!(
                shot.frames_sent <= frame_ceiling(started.elapsed(), limits().redraw_interval),
                "{} frames after {:?}",
                shot.frames_sent,
                started.elapsed()
            );
        } else {
            break;
        }
    }
    eventually(&t.server, <[SessionSnapshot]>::is_empty).await;
    assert!(sent_bytes < 8 * 1024 * 1024, "the server never hung up");
}

#[tokio::test]
async fn dropped_input_is_counted_and_the_session_survives_a_paste_sized_burst() {
    let t = start_server(limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, 80, 24).await;
    read_until(&mut channel, "Disconnect").await;
    // 20 KiB at once: more than the 16 KiB burst, so the tail is dropped, but only a few
    // strikes are charged.
    channel.data_bytes(vec![b'a'; 20 * 1024]).await.unwrap();
    eventually(&t.server, |s| {
        s.first().is_some_and(|x| x.input_dropped > 0)
    })
    .await;
    let shot = only(&t.server.sessions());
    assert!(shot.input_dropped >= 4 * 1024 - 1, "{}", shot.input_dropped);
    assert!(shot.shell_open);
    // Still usable: a key after the burst is honored (q leaves).
    channel.data_bytes(&b"q"[..]).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_second_login_is_served_promptly_while_one_session_floods() {
    // Tolerate every strike so the flood lasts the whole test: the budgets, not a disconnect,
    // are what is being measured.
    let t = start_server(Limits {
        flood_strikes: u32::MAX,
        ..limits()
    })
    .await;
    let (attacker, _) = login(t.addr()).await;
    let mut channel = open_shell(&attacker, 80, 24).await;
    read_until(&mut channel, "Disconnect").await;
    let started = Instant::now();
    let flood = tokio::spawn(async move {
        let chunk = vec![b'z'; 1024];
        let mut n = 0_u32;
        loop {
            if channel
                .window_change(40 + n % 100, 10 + n % 40, 0, 0)
                .await
                .is_err()
            {
                return;
            }
            if channel.data_bytes(chunk.clone()).await.is_err() {
                return;
            }
            n += 1;
            if n.is_multiple_of(64) {
                tokio::task::yield_now().await;
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut slowest = Duration::ZERO;
    for _ in 0..5 {
        let before = Instant::now();
        let (second, _) = login(t.addr()).await;
        let mut ch = open_shell(&second, 80, 24).await;
        read_until(&mut ch, "Disconnect").await;
        slowest = slowest.max(before.elapsed());
        drop(ch);
        drop(second);
    }
    // Generous: an idle login takes milliseconds. The point is "not seconds".
    assert!(
        slowest < Duration::from_secs(2),
        "slowest login {slowest:?}"
    );

    let elapsed = started.elapsed();
    let flooder = t
        .server
        .sessions()
        .into_iter()
        .min_by_key(|s| s.id)
        .expect("the flooding session is still connected");
    assert!(
        flooder.frames_sent <= frame_ceiling(elapsed, limits().redraw_interval),
        "{} frames in {elapsed:?}",
        flooder.frames_sent
    );
    assert!(flooder.input_dropped > 0, "the input budget was exercised");
    flood.abort();
}

/// A client whose receive window is one byte never lets the server send anything: the SSH
/// layer's "peer does not read".
async fn stalled_client(addr: std::net::SocketAddr) -> client::Handle<Client> {
    let config = Arc::new(client::Config {
        window_size: 1,
        ..client::Config::default()
    });
    let mut session = timeout(common::WAIT, client::connect(config, addr, Client))
        .await
        .unwrap()
        .unwrap();
    let key = common::new_client_key();
    let result = session
        .authenticate_publickey("anyone", PrivateKeyWithHashAlg::new(Arc::new(key), None))
        .await
        .unwrap();
    assert!(matches!(result, AuthResult::Success));
    session
}

#[tokio::test]
async fn a_client_that_never_reads_is_dropped_with_a_bounded_queue() {
    let t = start_server(Limits {
        write_stall: Duration::from_millis(400),
        ..limits()
    })
    .await;
    let session = stalled_client(t.addr()).await;
    let mut channel = session.channel_open_session().await.unwrap();
    channel
        .request_pty(true, "xterm", 80, 24, 0, 0, &[])
        .await
        .unwrap();
    expect_reply(&mut channel, true).await;
    channel.request_shell(true).await.unwrap();
    expect_reply(&mut channel, true).await;

    // A window change every 20 ms: each one is a redraw the peer will never read.
    let started = Instant::now();
    let mut peak_bytes = 0_u64;
    let mut gone = false;
    for step in 0..400_u32 {
        if channel
            .window_change(60 + step % 40, 20 + step % 10, 0, 0)
            .await
            .is_err()
        {
            gone = true;
            break;
        }
        if let Some(shot) = t.server.sessions().first() {
            peak_bytes = peak_bytes.max(shot.bytes_sent);
        } else {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    eventually(&t.server, <[SessionSnapshot]>::is_empty).await;
    assert!(
        gone || t.server.sessions().is_empty(),
        "the stalled session was never dropped"
    );
    // Queue depth: the entry frame plus the ten the session's output queue holds, plus the one
    // being waited on. Frames are a few KiB; 128 KiB is far above that and far below an
    // unbounded queue (400 redraws).
    assert!(
        peak_bytes < 128 * 1024,
        "{peak_bytes} bytes were handed to a peer that reads nothing"
    );
    assert!(started.elapsed() < Duration::from_secs(15));
}
