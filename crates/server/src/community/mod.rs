//! Running the community requests (M3-J, D86).
//!
//! The rules and the screens are in `russh_bbs_core::community`; the screens are drawn with the
//! members screens' drawing (`crate::members::lines`) because they produce the same view type.
//! This module only runs a screen's request against the store with a time limit, and records a
//! sign-in for last callers when a member session starts.

use std::time::Duration;

use russh_bbs_core::access::Snapshot;
use russh_bbs_core::community::{CommunityStore, Request, Response, execute, record_sign_in};
use russh_bbs_core::limits::{Clock, SystemClock};
use russh_bbs_core::store::AccountId;
use tokio::time::timeout;
use tracing::warn;

/// How long a community request may take. A slow store must not hang the screen; the screen then
/// says it did not work, and nothing is claimed about whether a write happened.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Run `request` for a session holding `access`.
pub(crate) async fn run<S: CommunityStore>(
    store: &S,
    access: &Snapshot,
    request: Request,
) -> Response {
    run_at(store, access, &SystemClock, request).await
}

/// [`run`] with the clock named, for tests.
pub(crate) async fn run_at<S: CommunityStore>(
    store: &S,
    access: &Snapshot,
    clock: &dyn Clock,
    request: Request,
) -> Response {
    if let Ok(response) = timeout(
        REQUEST_TIMEOUT,
        execute(store, access, clock.now(), request),
    )
    .await
    {
        response
    } else {
        warn!("community request timed out");
        Response::Failed
    }
}

/// Record that `account`'s member session began, for last callers. The one call at the point
/// member sessions start (`ssh::handler`, after the limits let the session in). A failure or a
/// timeout is logged and the session goes on: last callers is not worth refusing a login over.
pub(crate) async fn note_sign_in<S: CommunityStore>(store: &S, account: AccountId) {
    match timeout(
        REQUEST_TIMEOUT,
        record_sign_in(store, account, SystemClock.now()),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => warn!(%error, "last callers could not be updated"),
        Err(_) => warn!("last callers update timed out"),
    }
}
