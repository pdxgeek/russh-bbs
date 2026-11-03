//! A way for one session's own tasks to have the connection dropped.
//!
//! A peer that stops reading cannot be told to go away politely: the polite `disconnect` message
//! is itself queued behind the data it will not read. [`Abort`] is the signal the connection
//! task (`Connection::serve`) waits on; when it fires, that task shuts the socket down.

use tokio::sync::Notify;

/// Fired once by a session task to have its connection dropped. Cheap to share behind an `Arc`.
#[derive(Debug, Default)]
pub(crate) struct Abort(Notify);

impl Abort {
    /// Ask for the connection to be dropped. Safe to call before anyone waits: the request is
    /// remembered.
    pub(crate) fn trigger(&self) {
        self.0.notify_one();
    }

    /// Resolves once [`Abort::trigger`] was called.
    pub(crate) async fn triggered(&self) {
        self.0.notified().await;
    }
}
