//! The restricted session: what it shows, what it refuses, and how it handles the PTY.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl,
    clippy::items_after_statements,
    missing_docs
)]

mod common;

use common::{
    WAIT, eventually, expect_reply, fast_limits, login, next_reply, open_shell, read_until,
    start_server,
};
use russh::ChannelMsg;
use russh_bbs_core::access::Access;
use russh_bbs_server::ssh::terminal::{MAX_COLS, MAX_ROWS, PtySize, RENDER_COLS, RENDER_ROWS};
use tokio::time::timeout;

// ---- the waiting-room screen ----

#[tokio::test]
async fn shell_shows_the_waiting_room() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, 80, 24).await;

    let screen = read_until(&mut channel, "Disconnect").await;
    assert!(screen.contains("Waiting"));
    assert!(screen.contains("\x1b[?1049h"), "uses the alternate screen");
    eventually(&t.server, |s| s.len() == 1 && s[0].shell_open).await;
    assert_eq!(t.server.sessions()[0].access, Some(Access::Restricted));
}

#[tokio::test]
async fn q_disconnects_cleanly_and_restores_the_terminal() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, 80, 24).await;
    read_until(&mut channel, "Disconnect").await;

    channel.data_bytes(&b"q"[..]).await.unwrap();

    let tail = read_until(&mut channel, "\x1b[?1049l").await;
    assert!(tail.contains("\x1b[?25h"), "cursor shown again");
    let mut exit = None;
    while let Ok(Some(msg)) = timeout(WAIT, channel.wait()).await {
        if let ChannelMsg::ExitStatus { exit_status } = msg {
            exit = Some(exit_status);
        }
    }
    assert_eq!(exit, Some(0));
    eventually(&t.server, |s| s.iter().all(|x| !x.shell_open)).await;
}

#[tokio::test]
async fn other_keys_do_not_disconnect() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, 80, 24).await;
    read_until(&mut channel, "Disconnect").await;

    channel.data_bytes(&b"hello\x1b[A"[..]).await.unwrap();
    // Still alive: a following q is honored.
    channel.data_bytes(&b"q"[..]).await.unwrap();
    read_until(&mut channel, "\x1b[?1049l").await;
}

// ---- PTY and resize ----

#[tokio::test]
async fn pty_size_is_recorded() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let _channel = open_shell(&session, 132, 43).await;
    eventually(&t.server, |s| {
        s.first().and_then(|x| x.pty_size) == Some(PtySize::from_request(132, 43))
    })
    .await;
}

#[tokio::test]
async fn window_change_updates_the_session_size_and_redraws() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, 80, 24).await;
    read_until(&mut channel, "Disconnect").await;

    channel.window_change(150, 50, 0, 0).await.unwrap();

    eventually(&t.server, |s| {
        s.first().and_then(|x| x.pty_size) == Some(PtySize::from_request(150, 50))
    })
    .await;
    read_until(&mut channel, "Disconnect").await; // redrawn for the new size
}

#[tokio::test]
async fn hostile_window_sizes_are_clamped_not_allocated() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, u32::MAX, u32::MAX).await;
    read_until(&mut channel, "Disconnect").await;
    let max = PtySize::from_request(u32::from(MAX_COLS), u32::from(MAX_ROWS));
    eventually(&t.server, |s| {
        s.first().and_then(|x| x.pty_size) == Some(max)
    })
    .await;

    channel.window_change(0, 1, 0, 0).await.unwrap();
    channel.window_change(65_535, 65_535, 0, 0).await.unwrap();
    channel.window_change(1, 1, 0, 0).await.unwrap();
    // Still responsive after the abuse.
    channel.data_bytes(&b"q"[..]).await.unwrap();
    read_until(&mut channel, "\x1b[?1049l").await;
}

#[tokio::test]
async fn a_shell_without_a_pty_is_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = session.channel_open_session().await.unwrap();
    channel.request_shell(true).await.unwrap();
    expect_reply(&mut channel, false).await;
}

#[tokio::test]
async fn a_second_shell_on_the_same_channel_is_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, 80, 24).await;
    read_until(&mut channel, "Disconnect").await;
    channel.request_shell(true).await.unwrap();
    expect_reply(&mut channel, false).await;
}

// ---- lockdown: everything except the PTY shell is refused ----

#[tokio::test]
async fn exec_is_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = session.channel_open_session().await.unwrap();
    channel.exec(true, "id").await.unwrap();
    expect_reply(&mut channel, false).await;
}

#[tokio::test]
async fn subsystems_including_sftp_are_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    for name in ["sftp", "netconf", ""] {
        let mut channel = session.channel_open_session().await.unwrap();
        channel.request_subsystem(true, name).await.unwrap();
        expect_reply(&mut channel, false).await;
    }
}

#[tokio::test]
async fn env_is_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = session.channel_open_session().await.unwrap();
    channel
        .set_env(true, "LD_PRELOAD", "/tmp/evil.so")
        .await
        .unwrap();
    expect_reply(&mut channel, false).await;
}

#[tokio::test]
async fn agent_forwarding_is_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = session.channel_open_session().await.unwrap();
    channel.agent_forward(true).await.unwrap();
    expect_reply(&mut channel, false).await;
}

#[tokio::test]
async fn x11_forwarding_is_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = session.channel_open_session().await.unwrap();
    channel
        .request_x11(true, false, "MIT-MAGIC-COOKIE-1", "00", 0)
        .await
        .unwrap();
    expect_reply(&mut channel, false).await;
}

#[tokio::test]
async fn direct_tcpip_local_forwarding_is_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let result = session
        .channel_open_direct_tcpip("127.0.0.1", 22, "127.0.0.1", 12345)
        .await;
    assert!(result.is_err(), "direct-tcpip channel must not open");
}

#[tokio::test]
async fn tcpip_forward_remote_forwarding_is_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let result = session.tcpip_forward("127.0.0.1", 0).await;
    assert!(result.is_err(), "tcpip-forward must be denied: {result:?}");
    let result = session.tcpip_forward("0.0.0.0", 8080).await;
    assert!(result.is_err(), "tcpip-forward must be denied: {result:?}");
}

#[tokio::test]
async fn refusals_leave_the_connection_usable() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut probe = session.channel_open_session().await.unwrap();
    probe.exec(true, "id").await.unwrap();
    expect_reply(&mut probe, false).await;
    let _ = session.tcpip_forward("127.0.0.1", 0).await;

    let mut channel = open_shell(&session, 80, 24).await;
    read_until(&mut channel, "Disconnect").await;
}

#[tokio::test]
async fn session_channels_are_capped() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut open = Vec::new();
    for _ in 0..4 {
        open.push(session.channel_open_session().await.unwrap());
    }
    let fifth = session.channel_open_session().await;
    assert!(fifth.is_err(), "channels beyond the cap must be refused");
}

#[tokio::test]
async fn a_second_pty_on_another_channel_is_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let _shell = open_shell(&session, 80, 24).await;
    let mut other = session.channel_open_session().await.unwrap();
    other
        .request_pty(true, "xterm", 80, 24, 0, 0, &[])
        .await
        .unwrap();
    assert_eq!(next_reply(&mut other).await, Some(false));
}

#[tokio::test]
async fn pty_request_twice_is_refused() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = session.channel_open_session().await.unwrap();
    channel
        .request_pty(true, "xterm", 80, 24, 0, 0, &[])
        .await
        .unwrap();
    assert_eq!(next_reply(&mut channel).await, Some(true));
    channel
        .request_pty(true, "xterm", 80, 24, 0, 0, &[])
        .await
        .unwrap();
    assert_eq!(next_reply(&mut channel).await, Some(false));
}

// ---- the waiting room reveals nothing ----

#[tokio::test]
async fn the_waiting_room_names_no_members_boards_or_stats() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, 80, 24).await;
    let screen = read_until(&mut channel, "Disconnect").await.to_lowercase();
    for word in ["member list", "online", "board", "post", "caller", "users"] {
        assert!(!screen.contains(word), "screen leaks {word:?}: {screen:?}");
    }
}

/// The (row, column) of every cursor-position escape (`ESC [ row ; col H`, 1-based) in `frame`.
fn cursor_moves(frame: &str) -> Vec<(u16, u16)> {
    frame
        .split("\x1b[")
        .skip(1)
        .filter_map(|rest| {
            let params = rest.split('H').next().filter(|_| rest.contains('H'))?;
            let (row, col) = params.split_once(';')?;
            Some((row.parse().ok()?, col.parse().ok()?))
        })
        .collect()
}

/// D52 (QA M1 N1): a 500x200 window is drawn into a capped, centered rectangle, so a session's
/// render buffers (and the per-frame work) do not follow the size the client picked. Fails on a
/// server that draws the box across the whole window.
#[tokio::test]
async fn a_huge_window_is_drawn_into_a_bounded_rectangle() {
    let t = start_server(fast_limits()).await;
    let (session, _) = login(t.addr()).await;
    let mut channel = open_shell(&session, u32::from(MAX_COLS), u32::from(MAX_ROWS)).await;
    let first = read_until(&mut channel, "Disconnect").await;
    let moves = cursor_moves(&first);
    assert!(!moves.is_empty(), "no cursor moves in {first:?}");
    let span = |f: fn(&(u16, u16)) -> u16, limit: u16, what: &str| {
        let lo = moves.iter().map(f).min().unwrap();
        let hi = moves.iter().map(f).max().unwrap();
        assert!(hi - lo < limit, "{what} drawn over {lo}..={hi}, limit {limit}");
    };
    span(|m| m.0, RENDER_ROWS, "rows");
    span(|m| m.1, RENDER_COLS, "columns");

    // A window change to another huge size still redraws, inside the same bound.
    channel.window_change(400, 150, 0, 0).await.unwrap();
    let again = read_until(&mut channel, "Disconnect").await;
    assert!(
        cursor_moves(&again)
            .iter()
            .all(|m| m.0 <= MAX_ROWS && m.1 <= MAX_COLS),
        "{again:?}"
    );
}
