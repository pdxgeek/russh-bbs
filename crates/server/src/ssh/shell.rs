//! The shell of one session: the task that owns its screen, parses its keys and writes its
//! output (D49).
//!
//! The SSH handler (`handler.rs`) only *receives*: it records what the client sent in an
//! [`Inbox`] and wakes this task. The task does the work, at a pace the server sets:
//!
//! - **Coalescing.** Input bytes pile up in the inbox and only the latest window size is kept.
//!   Each cycle takes everything that arrived, applies it, and draws **once**. A thousand
//!   window changes or a megabyte of keys between two cycles cost one redraw.
//! - **Rate.** A cycle starts no sooner than `Limits::redraw_interval` after the previous one,
//!   so a session redraws at most `1 / redraw_interval` times a second. The last size and the
//!   last key always take effect, one interval later at most.
//! - **Key timing.** The [`KeyParser`] keeps partial sequences between packets; when only a
//!   lone `Esc` is waiting, the task wakes at the parser's deadline to turn it into Escape.
//! - **Output bound.** Every frame goes out through the session's bounded queue. If the peer
//!   stops reading so that queue stays full for `Limits::write_stall`, the task asks for the
//!   connection to be dropped instead of waiting without limit.
//!
//! Everything the old handler did in response to keys (the founder claim, saving terminal
//! settings, ending the shell) happens here too, because it follows from keys.

use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use russh::ChannelId;
use russh::server::Handle;
use russh_bbs_core::access::{Access, Fingerprint, Permission, Snapshot};
use russh_bbs_core::founder::{ClaimOutcome, claim_founder};
use russh_bbs_core::limits::{LimitError, MemberMeter, SystemClock};
use russh_bbs_core::store::{Account, AccountId};
use russh_bbs_core::terminal_settings::{MemberFlow, MemberStart, TerminalSettings};
use tokio::sync::Notify;
use tokio::time::{sleep_until, timeout, timeout_at};
use tracing::{debug, info, warn};

use super::Backend;
use super::abort::Abort;
use super::registry::SessionRef;
use super::screen::Screen;
use super::terminal::{KeyParser, PtySize};
use crate::founder::Founder;

/// How long a member's session start (settings and login time) may take before the shell
/// falls back. A slow store must not hang the screen.
pub(super) const MEMBER_START_TIMEOUT: Duration = Duration::from_secs(2);

/// Most input bytes the inbox holds between two cycles. The input budget is far below this; it
/// is a hard ceiling on memory in case the cycle is delayed.
const MAX_PENDING_INPUT: usize = 64 * 1024;

/// What the handler has received for the shell since the last cycle.
#[derive(Debug, Default)]
struct InboxState {
    input: Vec<u8>,
    resize: Option<PtySize>,
    finish: bool,
    closed: bool,
}

/// Everything taken from the inbox in one cycle.
#[derive(Debug)]
struct Batch {
    input: Vec<u8>,
    resize: Option<PtySize>,
    finish: bool,
    closed: bool,
}

/// The handler's side of the hand-off to the shell task. Filling it is a lock, a copy and a
/// wake-up; it never draws, parses or waits.
#[derive(Debug, Default)]
pub(super) struct Inbox {
    state: Mutex<InboxState>,
    wake: Notify,
}

impl Inbox {
    fn lock(&self) -> MutexGuard<'_, InboxState> {
        // The state is plain data; a panic elsewhere must not turn into an outage here.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queue input bytes for the next cycle. Bytes beyond the ceiling are dropped.
    pub(super) fn push_input(&self, bytes: &[u8]) {
        {
            let mut state = self.lock();
            let room = MAX_PENDING_INPUT.saturating_sub(state.input.len());
            let take = bytes.len().min(room);
            if let Some(part) = bytes.get(..take) {
                state.input.extend_from_slice(part);
            }
        }
        self.wake.notify_one();
    }

    /// Record a window size. Only the latest one is kept.
    pub(super) fn set_resize(&self, size: PtySize) {
        self.lock().resize = Some(size);
        self.wake.notify_one();
    }

    /// Ask the shell to restore the client's terminal and close the channel (the client sent
    /// EOF).
    pub(super) fn finish(&self) {
        self.lock().finish = true;
        self.wake.notify_one();
    }

    /// The channel or connection is gone: stop without sending anything.
    pub(super) fn close(&self) {
        self.lock().closed = true;
        self.wake.notify_one();
    }

    fn take(&self) -> Batch {
        let mut state = self.lock();
        Batch {
            input: std::mem::take(&mut state.input),
            resize: state.resize.take(),
            finish: state.finish,
            closed: state.closed,
        }
    }
}

/// What a shell needs from the connection around it.
pub(super) struct ShellParts<D> {
    pub(super) handle: Handle,
    pub(super) channel: ChannelId,
    pub(super) screen: Screen,
    pub(super) inbox: Arc<Inbox>,
    pub(super) directory: Arc<D>,
    pub(super) founder: Arc<Founder>,
    pub(super) session: SessionRef,
    pub(super) abort: Arc<Abort>,
    pub(super) fingerprint: Option<Fingerprint>,
    pub(super) peer_ip: Option<IpAddr>,
    pub(super) account: Option<AccountId>,
    /// The signed-in session's place in the resource limits (M2-H): its session slot and the
    /// minutes it has used. `None` for the waiting room, the founder claim and a refused start.
    pub(super) meter: Option<MemberMeter>,
    pub(super) redraw_interval: Duration,
    pub(super) write_stall: Duration,
}

/// One session's shell task.
pub(super) struct Shell<D> {
    parts: ShellParts<D>,
    keys: KeyParser,
}

impl<D: Backend> Shell<D> {
    pub(super) fn new(parts: ShellParts<D>) -> Self {
        Self {
            parts,
            keys: KeyParser::new(),
        }
    }

    /// Run until the shell ends: the person logs off, the client leaves, or the connection is
    /// dropped.
    pub(super) async fn run(mut self) {
        self.run_loop().await;
        self.settle_meter().await;
    }

    async fn run_loop(&mut self) {
        let first = self.parts.screen.enter();
        let Ok(first) = first else {
            warn!(session = ?self.parts.session.id(), "first frame could not be drawn");
            self.parts.abort.trigger();
            return;
        };
        if !self.send(first).await {
            return;
        }
        if self.parts.screen.is_ended() {
            // A limit refused the session: the message is up, nothing more to do.
            self.finish().await;
            return;
        }
        let mut next_cycle = tokio::time::Instant::now() + self.parts.redraw_interval;
        loop {
            self.wait_for_work().await;
            // The redraw rate cap. Whatever arrives while we wait is folded into this cycle.
            sleep_until(next_cycle).await;
            let batch = self.parts.inbox.take();
            if batch.closed {
                return;
            }
            let now = Instant::now();
            let mut keys = if batch.input.is_empty() {
                Vec::new()
            } else {
                self.keys.feed(&batch.input, now)
            };
            if batch.input.is_empty() && self.keys.deadline().is_some_and(|d| d <= now) {
                keys = self.keys.expire();
            }
            if let Some(size) = batch.resize {
                self.parts.screen.set_size(size);
            }
            let effects = if keys.is_empty() {
                super::screen::KeyEffects::default()
            } else {
                // One action: load what the account holds now (D33), so a role or group change
                // applies to the very next key batch. Nothing is cached between actions.
                self.refresh_access().await;
                self.parts.screen.apply_keys(&keys)
            };
            // Permission first (above), limit second: minutes are checked at each action.
            let out_of_time = !keys.is_empty() && self.check_minutes().await;
            if let Some(settings) = &effects.save_settings {
                self.persist_settings(settings).await;
            }
            let mut leaving = effects.disconnect || batch.finish || out_of_time;
            if !leaving
                && let Some(token) = effects.claim_token
                && self.try_claim(&token).await
            {
                leaving = true;
            }
            // The last frame goes out before the terminal is restored, so a final message (the
            // too-many-attempts notice) is seen. A client that already left gets no frame.
            if !batch.finish {
                match self.parts.screen.render() {
                    Ok(Some(frame)) => {
                        if !self.send(frame).await {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        warn!(session = ?self.parts.session.id(), %error, "frame could not be drawn");
                        self.parts.abort.trigger();
                        return;
                    }
                }
            }
            if leaving {
                self.finish().await;
                return;
            }
            next_cycle = tokio::time::Instant::now() + self.parts.redraw_interval;
        }
    }

    /// Charge the time used so far and check that the day's minutes are not spent. When they
    /// are, the screen shows which limit and when it resets, and the answer is `true` (log
    /// off). If the store cannot be reached the session carries on and the failure is logged:
    /// an outage must not log every member off, and the time is charged at the next checkpoint.
    async fn check_minutes(&mut self) -> bool {
        let Some(meter) = self.parts.meter.as_mut() else {
            return false;
        };
        let Some(access) = self.parts.screen.access() else {
            return false;
        };
        let checked = timeout(
            MEMBER_START_TIMEOUT,
            meter.tick(&*self.parts.directory, &SystemClock, access),
        )
        .await;
        match checked {
            Ok(Ok(())) => false,
            Ok(Err(LimitError::Refused(refusal))) => {
                info!(session = ?self.parts.session.id(), limit = refusal.kind.name(), "minutes used up; logging off");
                let mut lines = refusal.lines();
                lines.push("You are being signed off.".to_owned());
                self.parts.screen.limit_reached(lines);
                true
            }
            Ok(Err(error)) => {
                warn!(session = ?self.parts.session.id(), %error, "time could not be charged");
                false
            }
            Err(_) => {
                warn!(session = ?self.parts.session.id(), "charging time timed out");
                false
            }
        }
    }

    /// Charge the last stretch of time when the session ends, however it ended, and free the
    /// session slot.
    async fn settle_meter(&mut self) {
        let Some(meter) = self.parts.meter.take() else {
            return;
        };
        let settled = timeout(
            MEMBER_START_TIMEOUT,
            meter.finish(&*self.parts.directory, &SystemClock),
        )
        .await;
        match settled {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                warn!(session = ?self.parts.session.id(), %error, "final time could not be charged");
            }
            Err(_) => {
                warn!(session = ?self.parts.session.id(), "final time charge timed out");
            }
        }
    }

    /// Wait for the handler to hand over something, or for the parser's deadline.
    async fn wait_for_work(&self) {
        let woken = self.parts.inbox.wake.notified();
        match self.keys.deadline() {
            Some(deadline) => {
                // Elapsing is the point: the cycle then expires the held `Esc`.
                let _ = timeout_at(tokio::time::Instant::from_std(deadline), woken).await;
            }
            None => woken.await,
        }
    }

    /// Hand a frame to the session's bounded output queue. `false` means the shell must stop:
    /// the connection is gone, or the peer has not read for `write_stall` and was dropped.
    async fn send(&self, bytes: Vec<u8>) -> bool {
        if bytes.is_empty() {
            return true;
        }
        let len = bytes.len();
        match timeout(
            self.parts.write_stall,
            self.parts.handle.data(self.parts.channel, bytes),
        )
        .await
        {
            Ok(Ok(())) => {
                self.parts.session.add_frame(len);
                true
            }
            Ok(Err(_)) => {
                debug!(session = ?self.parts.session.id(), "connection closed while sending");
                false
            }
            Err(_) => {
                info!(
                    session = ?self.parts.session.id(),
                    stalled_secs = self.parts.write_stall.as_secs(),
                    "peer is not reading; dropping the connection"
                );
                self.parts.abort.trigger();
                false
            }
        }
    }

    /// End the shell: restore the client's terminal, then close the channel.
    async fn finish(&mut self) {
        match self.parts.screen.leave() {
            Ok(bytes) => {
                self.send(bytes).await;
            }
            Err(error) => {
                warn!(session = ?self.parts.session.id(), %error, "terminal could not be restored");
            }
        }
        self.parts.session.set_shell_open(false);
        let handle = &self.parts.handle;
        let channel = self.parts.channel;
        // Errors only mean the connection is already gone.
        let _ = handle.exit_status_request(channel, 0).await;
        let _ = handle.eof(channel).await;
        let _ = handle.close(channel).await;
    }

    /// Check a token typed at the claim prompt and update the screen. Every failure (limited,
    /// wrong token, already claimed, store error) is the same generic message. Returns whether
    /// the session should now end.
    async fn try_claim(&mut self, token: &str) -> bool {
        let Some(fingerprint) = self.parts.fingerprint.clone() else {
            return false;
        };
        let outcome = if self
            .parts
            .founder
            .admit_attempt(&fingerprint, self.parts.peer_ip)
            .is_allowed()
        {
            claim_founder(
                self.parts.founder.gate(),
                &*self.parts.directory,
                token,
                &fingerprint,
            )
            .await
        } else {
            info!(session = ?self.parts.session.id(), fingerprint = %fingerprint, "founder claim rate limited");
            ClaimOutcome::Rejected
        };
        match outcome {
            ClaimOutcome::Claimed(account) => {
                self.parts
                    .session
                    .set_authenticated(fingerprint, Access::Member);
                let start = self.founder_start(&account).await;
                self.parts.account = Some(start.account);
                self.parts.screen.claim_accepted(MemberFlow::new(
                    &start.handle,
                    start.access,
                    start.previous_login,
                    start.settings,
                ));
                false
            }
            ClaimOutcome::Rejected => self.parts.screen.claim_rejected(),
        }
    }

    /// Reload the signed-in account's permissions for the action about to run. If the store
    /// fails or is slow the session holds nothing for this action (menus show only what needs no
    /// permission): access fails closed.
    async fn refresh_access(&mut self) {
        let Some(account) = self.parts.account else {
            return;
        };
        let loaded = timeout(
            MEMBER_START_TIMEOUT,
            self.parts.directory.access_for(Some(account)),
        )
        .await;
        let access = match loaded {
            Ok(Ok(access)) => access,
            Ok(Err(error)) => {
                warn!(session = ?self.parts.session.id(), %error, "permissions could not be loaded; holding none");
                Snapshot::empty()
            }
            Err(_) => {
                warn!(session = ?self.parts.session.id(), "permission load timed out; holding none");
                Snapshot::empty()
            }
        };
        self.parts.screen.set_access(access);
    }

    /// The session start for a founder who just claimed. If the store fails right after the
    /// claim, the sysop still gets a working session on the defaults (nothing is saved).
    async fn founder_start(&self, account: &Account) -> MemberStart {
        let started = timeout(
            MEMBER_START_TIMEOUT,
            self.parts.directory.start_for_account(account),
        )
        .await;
        if let Ok(Ok(start)) = started {
            return start;
        }
        warn!(session = ?self.parts.session.id(), "founder session start failed; using defaults");
        MemberStart {
            account: account.id,
            handle: account.username.clone(),
            // Nothing is granted from here: the next action loads what the account holds.
            access: Snapshot::empty(),
            settings: TerminalSettings::default(),
            previous_login: None,
        }
    }

    /// Store settings the member changed. A failure is logged; the session keeps its choice.
    async fn persist_settings(&self, settings: &TerminalSettings) {
        // Saving settings is an action (`account.edit_settings`): no proof, no write.
        let Some(granted) = self
            .parts
            .screen
            .access()
            .and_then(|access| access.require(Permission::AccountEditSettings, None).ok())
        else {
            info!(session = ?self.parts.session.id(), "terminal settings not saved: permission not held");
            return;
        };
        let saved = timeout(
            MEMBER_START_TIMEOUT,
            self.parts.directory.save_settings(&granted, settings),
        )
        .await;
        match saved {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                warn!(session = ?self.parts.session.id(), %error, "terminal settings could not be saved");
            }
            Err(_) => {
                warn!(session = ?self.parts.session.id(), "terminal settings save timed out");
            }
        }
    }
}
