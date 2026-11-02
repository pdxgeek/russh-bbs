//! No log line carries the setup token or a guess. This is its own test binary with a single
//! test because it installs the process-wide tracing subscriber; sharing a binary with other
//! tests made the capture flaky (tracing caches per-callsite interest).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use std::sync::{Arc, Mutex};

use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::founder::claim_founder;
use russh_bbs_server::founder;
use russh_bbs_store::SqliteStore;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl MakeWriter<'_> for Capture {
    type Writer = Capture;

    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

fn fingerprint(n: u32) -> Fingerprint {
    Fingerprint::parse(&format!("SHA256:{n:A>43}")).unwrap()
}

#[tokio::test]
async fn no_log_line_carries_the_token() {
    let logs = Capture::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(logs.clone())
            .finish(),
    )
    .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
    let mut out = Vec::new();
    let founder = founder::start(&store, &mut out).await.unwrap();
    let announcement = String::from_utf8(out).unwrap();
    let token = announcement
        .lines()
        .find(|l| l.starts_with("    "))
        .unwrap()
        .trim()
        .to_owned();

    let _ = claim_founder(founder.gate(), &store, "WRONG-GUESS", &fingerprint(2)).await;
    let _ = claim_founder(founder.gate(), &store, &token, &fingerprint(1)).await;
    let _ = claim_founder(founder.gate(), &store, &token, &fingerprint(3)).await;

    let logged = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(
        logged.contains("founder claimed by fingerprint SHA256:"),
        "{logged}"
    );
    assert!(logged.contains("setup token was printed to the console"));
    assert!(!logged.contains(&token) && !logged.contains(&token.replace('-', "")));
    assert!(!logged.contains("WRONG-GUESS"));
    assert!(!logged.contains(token.split('-').next().unwrap()));
}
