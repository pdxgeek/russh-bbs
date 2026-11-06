//! Running a door's store work: opening (permission, bar, load, migrate) and flushing (save and
//! world edits). Every request has a time limit; a request that times out or fails closes the
//! door with a plain message, and nothing is half-written because each write is one compare-and-
//! swap in the store.

use std::sync::Arc;
use std::time::Duration;

use rand::TryRng as _;
use rand::rngs::SysRng;
use russh_bbs_core::access::{Permission, Snapshot};
use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::limits::Clock;
use russh_bbs_door::host::{FlushError, OpenError, Running};
use russh_bbs_door::{Door, DoorEnv, DoorKey, Flow};
use tracing::warn;

/// How long one request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Why a door was not opened or was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The member does not hold `doors.run`.
    Denied,
    /// Staff barred the member from this door.
    Barred,
    /// The save is from a newer door version.
    TooNew,
    /// Another session of the same member saved first.
    Conflict,
    /// The door tried to save over the cap.
    TooLarge,
    /// The store failed or timed out while opening.
    Failed,
    /// The store failed or timed out while saving.
    SaveFailed,
}

/// What the screen asks the session to run.
pub(crate) enum Request {
    /// Open this door for the signed-in member.
    Open {
        door: Arc<dyn Door>,
        env: Box<DoorEnv>,
    },
    /// Give the door one key after re-reading its world, which has gone stale (older than
    /// `WORLD_REFRESH_SECS`): one store read, so a door sees other players' edits made since its
    /// last read. Then write whatever the key queued. Answered like a [`Request::Flush`].
    Key { running: Box<Running>, key: DoorKey },
    /// Write what the door asked for; `leave` hands the session back afterwards.
    Flush { running: Box<Running>, leave: bool },
}

/// The answer.
pub(crate) enum Response {
    /// The door opened, or why not.
    Opened(Result<Box<Running>, Refusal>),
    /// The writes are done (the door comes back), or the door was closed. `leave` repeats the
    /// request's.
    Flushed {
        result: Result<Box<Running>, Refusal>,
        leave: bool,
    },
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open { door, .. } => write!(f, "Open({})", door.info().id),
            Self::Key { key, .. } => write!(f, "Key({key:?})"),
            Self::Flush { leave, .. } => write!(f, "Flush(leave: {leave})"),
        }
    }
}

impl Request {
    /// The answer to give when the request cannot be run at all (a timeout).
    fn failure(&self) -> Response {
        match self {
            Self::Open { .. } => Response::Opened(Err(Refusal::Failed)),
            Self::Flush { leave, .. } => Response::Flushed {
                result: Err(Refusal::SaveFailed),
                leave: *leave,
            },
            Self::Key { .. } => Response::Flushed {
                result: Err(Refusal::SaveFailed),
                leave: false,
            },
        }
    }
}

/// Run `request` with the member's permissions for this action.
pub(crate) async fn run<S: DoorStore>(
    store: &S,
    access: &Snapshot,
    clock: &Arc<dyn Clock>,
    request: Request,
) -> Response {
    let on_timeout = request.failure();
    if let Ok(response) =
        tokio::time::timeout(REQUEST_TIMEOUT, execute(store, access, clock, request)).await
    {
        response
    } else {
        warn!("a door request timed out");
        on_timeout
    }
}

/// A fresh seed from the operating system. `None` if it has no randomness: the door is then not
/// opened (a predictable seed would let a player foresee the game).
fn seed() -> Option<u64> {
    let mut bytes = [0u8; 8];
    SysRng.try_fill_bytes(&mut bytes).ok()?;
    Some(u64::from_le_bytes(bytes))
}

async fn execute<S: DoorStore>(
    store: &S,
    access: &Snapshot,
    clock: &Arc<dyn Clock>,
    request: Request,
) -> Response {
    match request {
        Request::Open { door, env } => {
            if access.require(Permission::DoorsRun, None).is_err() {
                return Response::Opened(Err(Refusal::Denied));
            }
            let Some(account) = access.account() else {
                return Response::Opened(Err(Refusal::Denied));
            };
            let Some(seed) = seed() else {
                warn!("the system random source failed; a door was not opened");
                return Response::Opened(Err(Refusal::Failed));
            };
            let opened = Running::open(store, door, account, &env, Arc::clone(clock), seed).await;
            Response::Opened(match opened {
                Ok(running) => Ok(Box::new(running)),
                Err(OpenError::Barred) => Err(Refusal::Barred),
                Err(OpenError::TooNew { .. }) => Err(Refusal::TooNew),
                Err(error) => {
                    warn!(%error, "a door could not be opened");
                    Err(Refusal::Failed)
                }
            })
        }
        Request::Key { mut running, key } => {
            if access.require(Permission::DoorsRun, None).is_err() {
                return Response::Flushed {
                    result: Err(Refusal::Denied),
                    leave: false,
                };
            }
            // A failed read is not fatal: the door keeps the world it had and the next key
            // tries again.
            if let Err(error) = running.refresh_world_if_stale(store).await {
                warn!(%error, "a door's world could not be re-read");
            }
            let leave = running.key(key) == Flow::Exit;
            let result = if leave || running.has_pending() {
                flush(store, running).await
            } else {
                Ok(running)
            };
            Response::Flushed { result, leave }
        }
        Request::Flush { running, leave } => {
            // The permission is checked again on every write: a member whose access was taken
            // away mid-session stops saving on the next key.
            if access.require(Permission::DoorsRun, None).is_err() {
                return Response::Flushed {
                    result: Err(Refusal::Denied),
                    leave,
                };
            }
            Response::Flushed {
                result: flush(store, running).await,
                leave,
            }
        }
    }
}

async fn flush<S: DoorStore>(
    store: &S,
    mut running: Box<Running>,
) -> Result<Box<Running>, Refusal> {
    match running.flush(store).await {
        Ok(()) => Ok(running),
        Err(FlushError::Conflict) => Err(Refusal::Conflict),
        Err(FlushError::TooLarge) => Err(Refusal::TooLarge),
        Err(FlushError::Store(error)) => {
            warn!(%error, "a door could not save");
            Err(Refusal::SaveFailed)
        }
    }
}
