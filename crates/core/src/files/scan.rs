//! The scanning hook (M4-F2).
//!
//! Every upload is scanned once, after it is stored in quarantine and before anyone can see it.
//! The hook is an interface with a default that does nothing ([`NoScan`]); the server wires a
//! ClamAV-over-socket adapter behind config. A scanner never decides visibility on its own:
//! [`ScanOutcome::Infected`] rejects the file, [`ScanOutcome::Failed`] leaves it in quarantine
//! (never auto-approved, whatever the area says), and every other outcome follows the area's
//! rule. The outcome is stored with the file so staff see whether it was scanned at all.

use std::future::Future;

use tokio::io::AsyncRead;

/// What the scanner said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanOutcome {
    /// No scanner is configured. The file was **not** scanned; staff and members are told so.
    Skipped,
    /// The scanner found nothing.
    Clean,
    /// The scanner found something; the text is its name for it, untrusted until sanitized.
    Infected(String),
    /// The scanner could not give an answer (down, timed out, protocol error); the text says
    /// why, untrusted until sanitized.
    Failed(String),
}

/// A virus scanner, fed the stored bytes as a stream.
pub trait ScanHook: Send + Sync {
    /// Scan `size` bytes read from `body`. Must read the stream at most once, must not trust
    /// `size` beyond a progress hint, and must answer [`ScanOutcome::Failed`] rather
    /// than hang: callers also apply their own timeout.
    fn scan<R>(&self, size: u64, body: R) -> impl Future<Output = ScanOutcome> + Send
    where
        R: AsyncRead + Send + Unpin;
}

/// The default hook: scans nothing and says so.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoScan;

impl ScanHook for NoScan {
    fn scan<R>(&self, _size: u64, _body: R) -> impl Future<Output = ScanOutcome> + Send
    where
        R: AsyncRead + Send + Unpin,
    {
        std::future::ready(ScanOutcome::Skipped)
    }
}
