//! Where the slow hashing runs.
//!
//! argon2id costs tens of milliseconds and 19 MiB on purpose. Core is runtime-free, so the
//! front end supplies an [`Offload`] that runs the work off the async executor (and bounds how
//! many run at once). Tests use [`Inline`].

use std::future::Future;

/// The work could not be run (the worker panicked or the pool is gone). Callers fail closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("password hashing could not run")]
pub struct Unavailable;

/// Runs CPU-heavy work away from the async executor.
pub trait Offload: Send + Sync {
    /// Run `work` and give its result.
    fn run<T, F>(&self, work: F) -> impl Future<Output = Result<T, Unavailable>> + Send
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static;
}

/// Runs the work on the calling task. For tests; a server uses a blocking pool.
#[derive(Debug, Clone, Copy, Default)]
pub struct Inline;

impl Offload for Inline {
    fn run<T, F>(&self, work: F) -> impl Future<Output = Result<T, Unavailable>> + Send
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        std::future::ready(Ok(work()))
    }
}
