//! Enforcing limits on a signed-in session: the checks at session start and the time meter.
//!
//! **What is enforced here** (the rest is stored and resolved, and enforced by the slice that
//! owns the action; see `docs/access.md`): concurrent sessions, calls per day and minutes per
//! day for signed-in SSH sessions. The waiting room and the founder claim are not limited by
//! this module (the per-IP and per-key limits of M1 still apply to them).
//!
//! **Order.** Permission first, limit second, both in core: the caller holds a
//! [`crate::access::Snapshot`] that already passed whatever permission the action needs, and
//! calls this to ask whether the limit allows it.
//!
//! **Time.** Minutes are counted per account per UTC day and persisted ([`LimitStore`]), so a
//! restart does not reset them. The meter charges elapsed time to the store at least every
//! [`CHECKPOINT_SECONDS`] and when the session ends, so a crash loses at most that much. A
//! session that is idle is still charged; it is only *told* at its next action (a keypress).

use std::fmt;

use crate::access::Snapshot;
use crate::store::{AccountId, StoreError, UnixSeconds};

use super::clock::{Clock, Day};
use super::effective::{EffectiveLimits, effective_limits, roles_held};
use super::kind::{Limit, LimitKind};
use super::sessions::{SessionCounter, SessionSlot};
use super::store::{LimitStore, UsageOutcome};

/// Elapsed time is written to the store at least this often. Also how soon a limit edit reaches
/// a session that is already open.
pub const CHECKPOINT_SECONDS: i64 = 30;

/// Usage rows older than this many days are deleted when the account starts a session.
/// `invites_per_month` needs a month; this leaves room.
pub const USAGE_RETENTION_DAYS: i64 = 62;

/// A limit said no. The text is for the member: which limit, how much, and when it resets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitRefusal {
    /// The limit that was reached.
    pub kind: LimitKind,
    /// Its value for this account.
    pub limit: u64,
    /// Seconds until it resets, or `None` if it frees up when something else ends (sessions).
    pub resets_in: Option<u64>,
}

impl LimitRefusal {
    /// The refusal as lines for a screen. Fixed words and numbers only, so it needs no
    /// sanitizing.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![self.to_string()];
        lines.push(match self.resets_in {
            Some(secs) => format!(
                "This limit resets at 00:00 UTC, in {}.",
                format_duration(secs)
            ),
            None => {
                "There is no timed reset: it frees up when one of your sessions ends.".to_owned()
            }
        });
        lines
    }
}

impl fmt::Display for LimitRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.limit;
        match self.kind {
            LimitKind::MinutesPerDay => write!(
                f,
                "You have used your {n} {} for today.",
                plural(n, "minute", "minutes")
            ),
            LimitKind::CallsPerDay => write!(
                f,
                "You have used your {n} {} for today.",
                plural(n, "sign-in", "sign-ins")
            ),
            LimitKind::MaxSessions => write!(
                f,
                "Your account may have {n} {} open at once, and that many are open.",
                plural(n, "session", "sessions")
            ),
            LimitKind::PostsPerHour
            | LimitKind::UploadBytesPerDay
            | LimitKind::UploadBytesPerFile
            | LimitKind::MailPerDay
            | LimitKind::InvitesPerMonth => write!(
                f,
                "You have reached your limit of {n} {} ({}).",
                self.kind.unit(),
                self.kind.name()
            ),
        }
    }
}

impl std::error::Error for LimitRefusal {}

fn plural<'a>(n: u64, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 { one } else { many }
}

/// `3h 20m`, `12m`, or `less than a minute`.
fn format_duration(secs: u64) -> String {
    let minutes = secs.div_ceil(60);
    match (minutes / 60, minutes % 60) {
        (0, 0) => "less than a minute".to_owned(),
        (0, m) => format!("{m}m"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// Why a limit check did not pass.
#[derive(Debug, thiserror::Error)]
pub enum LimitError {
    /// A limit was reached. Show it and, for time, log off.
    #[error(transparent)]
    Refused(#[from] LimitRefusal),
    /// The store failed. The caller fails closed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The snapshot is not a signed-in account's.
    #[error("not a signed-in account")]
    NoAccount,
}

/// Resolve the limits that apply to the account behind `snapshot`.
pub async fn load_effective<S: LimitStore>(
    store: &S,
    account: AccountId,
    snapshot: &Snapshot,
) -> Result<EffectiveLimits, StoreError> {
    let roles = roles_held(snapshot);
    let role_limits = store.role_limits(&roles).await?;
    let overrides = store.member_limits(account).await?;
    Ok(effective_limits(&roles, &role_limits, &overrides))
}

/// Start metering a signed-in session, or say which limit refuses it.
///
/// Checks, in this order, so a refusal never costs the member a count: concurrent sessions,
/// minutes left today, then calls today (the call is counted only when it is allowed).
pub async fn begin_member_session<S: LimitStore, C: Clock>(
    store: &S,
    clock: &C,
    sessions: &SessionCounter,
    snapshot: &Snapshot,
) -> Result<MemberMeter, LimitError> {
    let account = snapshot.account().ok_or(LimitError::NoAccount)?;
    let limits = load_effective(store, account, snapshot).await?;
    let now = clock.now();
    let day = Day::of(now);

    let max_sessions = limits.get(LimitKind::MaxSessions);
    let slot = sessions
        .try_acquire(account, max_sessions)
        .ok_or(LimitRefusal {
            kind: LimitKind::MaxSessions,
            limit: max_sessions.cap().unwrap_or(0),
            resets_in: None,
        })?;

    // Old counters are dead weight; a failure to delete them must not stop a sign-in.
    if let Err(error) = store
        .prune_usage(account, day.minus(USAGE_RETENTION_DAYS))
        .await
    {
        tracing::warn!(%error, "old usage rows could not be pruned");
    }

    let used_secs = store.usage(account, LimitKind::MinutesPerDay, day).await?;
    let minutes = limits.get(LimitKind::MinutesPerDay);
    if let Limit::AtMost(m) = minutes
        && used_secs >= m.saturating_mul(60)
    {
        return Err(LimitRefusal {
            kind: LimitKind::MinutesPerDay,
            limit: m,
            resets_in: Some(seconds_until(day.end(), now)),
        }
        .into());
    }

    let calls = limits.get(LimitKind::CallsPerDay);
    if let UsageOutcome::Full(_) = store
        .add_usage(account, LimitKind::CallsPerDay, day, 1, calls.cap())
        .await?
    {
        return Err(LimitRefusal {
            kind: LimitKind::CallsPerDay,
            limit: calls.cap().unwrap_or(0),
            resets_in: Some(seconds_until(day.end(), now)),
        }
        .into());
    }

    Ok(MemberMeter {
        account,
        limits,
        _slot: slot,
        day,
        charged_until: now,
        used_secs,
    })
}

fn seconds_until(later: UnixSeconds, now: UnixSeconds) -> u64 {
    u64::try_from(later.as_raw().saturating_sub(now.as_raw())).unwrap_or(0)
}

/// One signed-in session's place in the limits: its session slot and the time it has used.
/// Dropping it frees the slot; call [`MemberMeter::finish`] first to charge the last stretch.
#[derive(Debug)]
pub struct MemberMeter {
    account: AccountId,
    limits: EffectiveLimits,
    _slot: SessionSlot,
    /// The UTC day time is being charged to.
    day: Day,
    /// Time up to here has been written to the store.
    charged_until: UnixSeconds,
    /// Seconds the account has used today, as of the last write (all its sessions).
    used_secs: u64,
}

impl MemberMeter {
    /// The limits in force as of the last checkpoint.
    #[must_use]
    pub const fn limits(&self) -> &EffectiveLimits {
        &self.limits
    }

    /// Called at each action. Charges elapsed time at a checkpoint (re-resolving the limits
    /// from `snapshot` then, so an edit reaches an open session), and refuses once the day's
    /// minutes are used up. The caller shows the refusal and logs the member off.
    pub async fn tick<S: LimitStore, C: Clock>(
        &mut self,
        store: &S,
        clock: &C,
        snapshot: &Snapshot,
    ) -> Result<(), LimitError> {
        let now = clock.now();
        self.roll_day(store, now).await?;
        if now.as_raw().saturating_sub(self.charged_until.as_raw()) >= CHECKPOINT_SECONDS {
            self.charge(store, now).await?;
            self.limits = load_effective(store, self.account, snapshot).await?;
        }
        if let Limit::AtMost(m) = self.limits.get(LimitKind::MinutesPerDay) {
            let pending = u64::try_from(now.as_raw().saturating_sub(self.charged_until.as_raw()))
                .unwrap_or(0);
            if self.used_secs.saturating_add(pending) >= m.saturating_mul(60) {
                return Err(LimitRefusal {
                    kind: LimitKind::MinutesPerDay,
                    limit: m,
                    resets_in: Some(seconds_until(self.day.end(), now)),
                }
                .into());
            }
        }
        Ok(())
    }

    /// Charge the time since the last write. Call when the session ends, however it ended.
    pub async fn finish<S: LimitStore, C: Clock>(
        mut self,
        store: &S,
        clock: &C,
    ) -> Result<(), StoreError> {
        let now = clock.now();
        self.roll_day(store, now).await?;
        self.charge(store, now).await
    }

    /// When midnight UTC has passed, close the old day's books and start the new day. A session
    /// that stays open for several days is charged for the day it started and the day it is
    /// in; whole days in between are not charged (a day's limit cannot be exceeded by them).
    async fn roll_day<S: LimitStore>(
        &mut self,
        store: &S,
        now: UnixSeconds,
    ) -> Result<(), StoreError> {
        let today = Day::of(now);
        if today <= self.day {
            return Ok(());
        }
        self.charge(store, self.day.end()).await?;
        self.day = today;
        self.charged_until = today.start();
        self.used_secs = store
            .usage(self.account, LimitKind::MinutesPerDay, today)
            .await?;
        Ok(())
    }

    async fn charge<S: LimitStore>(
        &mut self,
        store: &S,
        until: UnixSeconds,
    ) -> Result<(), StoreError> {
        let seconds =
            u64::try_from(until.as_raw().saturating_sub(self.charged_until.as_raw())).unwrap_or(0);
        if seconds == 0 {
            return Ok(());
        }
        let total = match store
            .add_usage(
                self.account,
                LimitKind::MinutesPerDay,
                self.day,
                seconds,
                None,
            )
            .await?
        {
            UsageOutcome::Added(total) | UsageOutcome::Full(total) => total,
        };
        self.used_secs = total;
        self.charged_until = until;
        Ok(())
    }
}
