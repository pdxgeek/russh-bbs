//! The registration actions. Front ends call these; none of the rules live in a front end.

use std::net::IpAddr;
use std::time::Duration;

use tracing::{info, warn};

use super::cap::DailyCap;
use super::store::{OpenError, OpenRefusal, Opened, RegistrationStore};
use crate::access::{Fingerprint, Permission, Snapshot};
use crate::queue::{
    DEFAULT_TRACKED, DropReason, InviteAsk, QueueStore, RequestGate, RequestKind, Submission,
    SubmitLimited, submit_request,
};
use crate::sanitize::Username;
use crate::store::RegistrationMode;
use crate::waiting_room::{Clock, RatePolicy, SystemClock};

/// One key may try to create an account twice, then once an hour: a person who mistypes a
/// handle is never near it, a script is. Separate from the queue's gate (the threat model asks
/// for account creation to be limited separately from waiting-room requests).
pub const DEFAULT_OPEN_KEY_RATE: RatePolicy = RatePolicy::new(2, Duration::from_secs(3600));
/// One network may try five times, then once every ten minutes (6 an hour): a household or a
/// campus behind one address can register a few people, a script cannot register hundreds.
pub const DEFAULT_OPEN_NETWORK_RATE: RatePolicy = RatePolicy::new(5, Duration::from_secs(600));

/// The per-key and per-network limits on creating an `open` account: M1-C's limiter (D37) with
/// its own counters and rates, so account creation is limited separately from waiting-room
/// requests. Memory is bounded (each limiter remembers [`DEFAULT_TRACKED`] sources); the
/// network is kept in memory only.
#[derive(Debug)]
pub struct OpenGate<C = SystemClock>(RequestGate<C>);

impl OpenGate {
    /// A gate with the default `open` limits and the system clock.
    #[must_use]
    pub fn new() -> Self {
        Self::with_rates(
            DEFAULT_OPEN_KEY_RATE,
            DEFAULT_OPEN_NETWORK_RATE,
            DEFAULT_TRACKED,
            &SystemClock::new(),
        )
    }
}

impl Default for OpenGate {
    fn default() -> Self {
        Self::new()
    }
}

impl<C: Clock + Clone> OpenGate<C> {
    /// A gate with explicit limits and clock (tests use a manual clock).
    #[must_use]
    pub fn with_rates(key: RatePolicy, network: RatePolicy, tracked: usize, clock: &C) -> Self {
        Self(RequestGate::with_rates(key, network, tracked, clock))
    }
}

impl<C: Clock> OpenGate<C> {
    /// Count one attempt by `key` from `peer`.
    fn admit(&self, key: &Fingerprint, peer: Option<IpAddr>) -> Result<(), SubmitLimited> {
        self.0.admit(key, peer)
    }
}

/// The cap on `open` accounts per UTC day: the sysop's `open_daily_cap` setting, else
/// [`DEFAULT_OPEN_DAILY_CAP`](super::DEFAULT_OPEN_DAILY_CAP). A store that cannot be read gives
/// the default (logged), never "no cap".
pub async fn daily_cap<S: RegistrationStore>(store: &S) -> DailyCap {
    match store.open_cap_setting().await {
        Ok(stored) => {
            if stored
                .as_deref()
                .is_some_and(|text| DailyCap::parse(text).is_none())
            {
                warn!("open_daily_cap is not a number from 0 to 100000; using the default");
            }
            DailyCap::from_setting(stored.as_deref())
        }
        Err(error) => {
            warn!(%error, "open_daily_cap could not be read; using the default");
            DailyCap::Limited(super::DEFAULT_OPEN_DAILY_CAP)
        }
    }
}

/// Whether `open` registration is full for today. Used when a waiting room is built: `true`
/// (or a store that cannot say) means the room offers the `apply` form instead. Fails toward
/// `apply`, the stricter path.
pub async fn open_cap_reached<S: RegistrationStore>(
    store: &S,
) -> Result<bool, crate::store::StoreError> {
    let cap = daily_cap(store).await;
    store.open_cap_reached(cap).await
}

/// Make an account for a stranger in `open` mode. Needs `membership.open` in `access` (the
/// waiting snapshot), the stored mode to be `open` *now*, the per-key and per-network gate, and
/// the day's cap. Every refusal that is not a store failure is logged with its reason and read
/// by the person as one sentence.
///
/// `peer` is used for the network limit only and is not kept.
///
/// # Errors
/// [`OpenError::Rejected`], [`OpenError::CapReached`] or [`OpenError::Store`].
pub async fn open_account<S: RegistrationStore, C: Clock>(
    store: &S,
    gate: &OpenGate<C>,
    access: &Snapshot,
    username: Username,
    fingerprint: &Fingerprint,
    peer: Option<IpAddr>,
) -> Result<Opened, OpenError> {
    let refuse = |reason: OpenRefusal| {
        info!(fingerprint = %fingerprint, ?reason, "open registration refused");
        Err(OpenError::Rejected(reason))
    };
    if access.require(Permission::MembershipOpen, None).is_err() {
        return refuse(OpenRefusal::NotPermitted);
    }
    match store.registration_mode().await {
        Ok(RegistrationMode::Open) => {}
        Ok(_) => return refuse(OpenRefusal::NotOpen),
        Err(error) => return Err(error.into()),
    }
    if let Err(limited) = gate.admit(fingerprint, peer) {
        return refuse(match limited {
            SubmitLimited::Key => OpenRefusal::KeyRate,
            SubmitLimited::Network => OpenRefusal::NetworkRate,
        });
    }
    let cap = daily_cap(store).await;
    match store.open_account(username, fingerprint, cap).await {
        Ok(opened) => {
            info!(account = %opened.account.id, fingerprint = %fingerprint, "account opened");
            Ok(opened)
        }
        Err(OpenError::CapReached) => {
            warn!(
                "open registration refused: the daily cap is reached; new visitors get the apply form"
            );
            Err(OpenError::CapReached)
        }
        Err(OpenError::Rejected(reason)) => refuse(reason),
        Err(OpenError::Store(error)) => {
            warn!(fingerprint = %fingerprint, %error, "open registration failed in the store");
            Err(OpenError::Store(error))
        }
    }
}

/// Offer a membership application from the waiting room. Needs `membership.apply` in `access`
/// and the stored mode to be `apply` (or `open`, where the form is what a full day shows);
/// then it is M2-E's queue exactly: the per-key and per-network limits, the cap, the
/// one-per-key rule, the cooldown, the expiry. Every outcome is for the log; the front end
/// shows the same words for all of them.
pub async fn submit_application<S, C>(
    store: &S,
    gate: &RequestGate<C>,
    access: &Snapshot,
    mut ask: InviteAsk,
    fingerprint: &Fingerprint,
    peer: Option<IpAddr>,
) -> Submission
where
    S: QueueStore + RegistrationStore,
    C: Clock,
{
    ask.kind = RequestKind::Application;
    match store.registration_mode().await {
        Ok(RegistrationMode::Apply | RegistrationMode::Open) => {}
        Ok(_) => {
            info!("application dropped: the board is not taking applications");
            return Submission::Dropped(DropReason::WrongMode);
        }
        Err(error) => {
            warn!(%error, "application dropped: the mode could not be read");
            return Submission::Dropped(DropReason::Store);
        }
    }
    submit_request(store, gate, access, ask, fingerprint, peer).await
}
