//! SIGTERM stops the real binary cleanly (D25): exit 0, and the WAL is checkpointed.
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]
#![cfg(unix)]

use std::io::{BufRead as _, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn sigterm_exits_zero_and_checkpoints_the_wal() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("bbs.db");
    // The config rejects port 0, so find a free port first.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_russh-bbs-server"))
        .env("RUSSH_BBS_LISTEN", format!("127.0.0.1:{port}"))
        .env("RUSSH_BBS_HOST_KEY", dir.path().join("host_key"))
        .env("RUSSH_BBS_DB", &db)
        .env("HOME", dir.path())
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("XDG_DATA_HOME")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Wait until the server is listening (tracing's default writer is stdout).
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line.contains("ssh server listening") {
                let _ = tx.send(());
            }
        }
    });
    rx.recv_timeout(Duration::from_secs(30))
        .expect("server did not start listening");

    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());

    let start = Instant::now();
    let exit = loop {
        if let Some(exit) = child.try_wait().unwrap() {
            break exit;
        }
        assert!(start.elapsed() < Duration::from_secs(15), "did not exit");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(exit.success(), "exit status {exit:?}");

    let mut wal = db.clone().into_os_string();
    wal.push("-wal");
    let wal_len = std::fs::metadata(&wal).map_or(0, |m| m.len());
    assert_eq!(wal_len, 0, "WAL not checkpointed");
}
