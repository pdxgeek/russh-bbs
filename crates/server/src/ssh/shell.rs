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

use rand::TryRng as _;
use rand::rngs::SysRng;
use russh::ChannelId;
use russh::server::Handle;
use russh_bbs_core::access::{Access, Fingerprint, Permission, Snapshot};
use russh_bbs_core::founder::{ClaimOutcome, claim_founder};
use russh_bbs_core::invites::{
    CODE_BYTES, InviteError, InviteOutcome, InviteRequest, create_invite, invite_tree, my_invites,
    redeem_invite,
};
use russh_bbs_core::limits::{LimitError, MemberMeter, SystemClock};
use russh_bbs_core::queue::{InviteAsk, RequestKind, submit_request};
use russh_bbs_core::registration::{OpenError, open_account, submit_application};
use russh_bbs_core::store::{Account, AccountId};
use russh_bbs_core::terminal_settings::{MemberFlow, MemberStart, TerminalSettings};
use russh_bbs_core::waiting_room::{JoinKind, RedeemRequest};
use tokio::sync::Notify;
use tokio::time::{sleep_until, timeout, timeout_at};
use tracing::{debug, info, warn};

use super::Backend;
use super::abort::Abort;
use super::password::PasswordLogin;
use super::registry::SessionRef;
use super::screen::Screen;
use super::terminal::{KeyParser, PtySize};
use crate::founder::Founder;

/// How long a member's session start (settings and login time) may take before the shell
/// falls back. A slow store must not hang the screen.
pub(super) const MEMBER_START_TIMEOUT: Duration = Duration::from_secs(2);

/// How long one password screen request may take: hashing a new password and writing it. Longer
/// than a store call because argon2id is slow on purpose and waits its turn on the pool.
const PASSWORD_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// Password login's hashing pool, for the password screen (M3-D).
    pub(super) passwords: Arc<PasswordLogin>,
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
            let mut effects = if keys.is_empty() {
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
            // A members screen needs the store before it can take the next key. Answer it, then
            // apply the keys that were held back, with the account's permissions loaded again
            // (every action sees what the account holds now).
            self.run_requests(&mut effects).await;
            if let Some(request) = effects.invite.take() {
                self.run_invite(request).await;
            }
            let mut leaving = effects.disconnect || batch.finish || out_of_time;
            if !leaving && let Some(request) = effects.redeem {
                self.try_redeem(request).await;
            }
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
            // After the frame: the room's answer to a request does not wait for the store, so how
            // long it takes cannot tell a kept request from a dropped one (D71).
            if let Some(ask) = effects.request_invite.take() {
                self.try_request_invite(ask).await;
            }
            if leaving {
                self.finish().await;
                return;
            }
            next_cycle = tokio::time::Instant::now() + self.parts.redraw_interval;
        }
    }

    /// Answer the store requests a screen is waiting on, then apply the keys that were held back
    /// with the account's permissions loaded again (every action sees what the account holds
    /// now), until none is left.
    async fn run_requests(&mut self, effects: &mut super::screen::KeyEffects) {
        loop {
            if let Some(request) = effects.members_request.take() {
                self.run_members(request).await;
            } else if let Some(request) = effects.queue_request.take() {
                self.run_queue(request).await;
            } else if let Some(request) = effects.password_request.take() {
                self.run_password(request).await;
            } else if let Some(request) = effects.community_request.take() {
                self.run_community(request).await;
            } else {
                break;
            }
            let rest = std::mem::take(&mut effects.held_back);
            if rest.is_empty() {
                break;
            }
            self.refresh_access().await;
            let later = self.parts.screen.apply_keys(&rest);
            effects.absorb(later);
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
                let start = self.account_start(&account).await;
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

    /// Redeem an invite typed in the waiting room (M2-D), or create an account in `open` mode
    /// (M2-G). The waiting snapshot must hold `invite.redeem` (or `membership.open`, with the
    /// stored mode `open` now; both checked in core). Success turns the visitor into the new
    /// member; every failure (a bad code, a taken handle, a key that already has an account, a
    /// rate limit, a full day, a slow or broken store) is the same refusal on screen, and the
    /// reason goes to the log only, never the code.
    async fn try_redeem(&mut self, request: RedeemRequest) {
        let Some(fingerprint) = self.parts.fingerprint.clone() else {
            self.parts.screen.redeem_refused();
            return;
        };
        let RedeemRequest { join, handle } = request;
        let work = async {
            let Ok(access) = self.parts.directory.access_for(None).await else {
                return Made::Refused;
            };
            let directory = &*self.parts.directory;
            match join {
                JoinKind::Invite { code } => {
                    match redeem_invite(directory, &access, &code, handle, &fingerprint).await {
                        Ok(redeemed) => Made::Account(redeemed.account),
                        Err(_) => Made::Refused,
                    }
                }
                JoinKind::Open => {
                    match open_account(
                        directory,
                        self.parts.session.open_gate(),
                        &access,
                        handle,
                        &fingerprint,
                        self.parts.peer_ip,
                    )
                    .await
                    {
                        Ok(opened) => Made::Account(opened.account),
                        Err(OpenError::CapReached) => Made::CapReached,
                        Err(_) => Made::Refused,
                    }
                }
            }
        };
        let Ok(made) = timeout(MEMBER_START_TIMEOUT, work).await else {
            warn!(session = ?self.parts.session.id(), "registration timed out");
            self.parts.screen.redeem_refused();
            return;
        };
        let account = match made {
            Made::Account(account) => account,
            Made::CapReached => {
                self.parts.screen.open_cap_reached();
                return;
            }
            Made::Refused => {
                self.parts.screen.redeem_refused();
                return;
            }
        };
        self.parts
            .session
            .set_authenticated(fingerprint, Access::Member);
        let start = self.account_start(&account).await;
        // A member who just joined starts a member session: last callers lists them.
        crate::community::note_sign_in(&*self.parts.directory, start.account).await;
        self.parts.account = Some(start.account);
        self.parts.screen.redeemed(MemberFlow::new(
            &start.handle,
            start.access,
            start.previous_login,
            start.settings,
        ));
    }

    /// Offer a request for an invite, or a membership application, typed in the waiting room to
    /// the queue (M2-E, M2-G). The room
    /// has already shown its one fixed answer, and the frame carrying it is sent before this
    /// runs, so nothing here reaches the screen or delays it: a drop (a
    /// rate limit, a full queue, a repeat, a recent refusal, a slow or broken store) is logged
    /// by core without the request's text and looks like a kept request to the person. The
    /// waiting snapshot must hold `invite.request` (checked in core).
    async fn try_request_invite(&mut self, ask: InviteAsk) {
        let Some(fingerprint) = self.parts.fingerprint.clone() else {
            return;
        };
        let work = async {
            let access = self.parts.directory.access_for(None).await.ok()?;
            let directory = &*self.parts.directory;
            let gate = self.parts.session.request_gate();
            let peer = self.parts.peer_ip;
            Some(match ask.kind {
                RequestKind::Invite => {
                    submit_request(directory, gate, &access, ask, &fingerprint, peer).await
                }
                RequestKind::Application => {
                    submit_application(directory, gate, &access, ask, &fingerprint, peer).await
                }
            })
        };
        if timeout(MEMBER_START_TIMEOUT, work).await.is_err() {
            warn!(session = ?self.parts.session.id(), "invite request timed out; dropped");
        }
    }

    /// Carry out an invite request for the signed-in member and show its screen. The checks
    /// (permission, quota) are core's; this only supplies the session's snapshot and, for a new
    /// code, bytes from the OS random source.
    async fn run_invite(&mut self, request: InviteRequest) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let directory = Arc::clone(&self.parts.directory);
        let work = async {
            match request {
                InviteRequest::Create => {
                    let mut bytes = [0_u8; CODE_BYTES];
                    if SysRng.try_fill_bytes(&mut bytes).is_err() {
                        warn!("the operating system random source failed; no invite made");
                        return InviteOutcome::Failed;
                    }
                    match create_invite(&*directory, &access, bytes).await {
                        Ok(made) => InviteOutcome::Created(made),
                        Err(error) => failed(error),
                    }
                }
                InviteRequest::List => match my_invites(&*directory, &access).await {
                    Ok(items) => InviteOutcome::List(items),
                    Err(error) => failed(error),
                },
                InviteRequest::Tree => match invite_tree(&*directory, &access).await {
                    Ok(lines) => InviteOutcome::Tree(lines),
                    Err(error) => failed(error),
                },
            }
        };
        let outcome = timeout(MEMBER_START_TIMEOUT, work)
            .await
            .unwrap_or_else(|_| {
                warn!(session = ?self.parts.session.id(), "invite request timed out");
                InviteOutcome::Failed
            });
        self.parts.screen.invite_done(outcome);
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

    /// The session start for an account that was just made (the founder claim, an invite
    /// redemption). If the store fails right after, the person still gets a working session on
    /// the defaults (nothing is saved).
    async fn account_start(&self, account: &Account) -> MemberStart {
        let started = timeout(
            MEMBER_START_TIMEOUT,
            self.parts.directory.start_for_account(account),
        )
        .await;
        if let Ok(Ok(start)) = started {
            return start;
        }
        warn!(session = ?self.parts.session.id(), "new account's session start failed; using defaults");
        MemberStart {
            account: account.id,
            handle: account.username.clone(),
            // Nothing is granted from here: the next action loads what the account holds.
            access: Snapshot::empty(),
            settings: TerminalSettings::default(),
            previous_login: None,
        }
    }

    /// Run the members request a screen is waiting on (a list page, a profile, a save) and give
    /// it the answer. Core checks the permission against the snapshot loaded for this action
    /// (`profiles::execute`); a session without one gets "not permitted", and a slow or failing
    /// store gets "failed". Nothing is cached.
    async fn run_members(&mut self, request: russh_bbs_core::profiles::Request) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let response = crate::members::run(&*self.parts.directory, &access, request).await;
        self.parts.screen.deliver_members(response);
    }

    /// Run the queue request a sysop or vouch screen is waiting on and give it the answer.
    /// Core checks the permission against the snapshot loaded for this action
    /// (`queue::execute`); a session without one gets "not permitted", and a slow or failing
    /// store gets "failed". Nothing is cached.
    ///
    /// A decision asks for the list again so the screen shows the queue as it is now; that
    /// follow-up runs here, not at the next key, with the same permissions (a few at most).
    async fn run_queue(&mut self, request: russh_bbs_core::queue::Request) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let mut request = request;
        for _ in 0..3 {
            let response = crate::sysop::queue::run(&*self.parts.directory, &access, request).await;
            self.parts.screen.deliver_queue(response);
            match self.parts.screen.take_queue_request() {
                Some(next) => request = next,
                None => break,
            }
        }
    }

    /// Run the community request a community screen is waiting on and give it the answer. Core
    /// checks the permission against the snapshot loaded for this action (`community::execute`);
    /// a session without one gets "not permitted", and a slow or failing store gets "failed".
    /// A screen that needs a follow-up (an emptied poll page asks for the first one) gets it
    /// here, with the same permissions (a few at most).
    async fn run_community(&mut self, request: russh_bbs_core::community::Request) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let mut request = request;
        for _ in 0..3 {
            let response = crate::community::run(&*self.parts.directory, &access, request).await;
            self.parts.screen.deliver_community(response);
            match self.parts.screen.take_community_request() {
                Some(next) => request = next,
                None => break,
            }
        }
    }

    /// Run the password request the password screen is waiting on and give it the answer.
    /// Core checks `account.set_password` against the snapshot loaded for this action and hashes
    /// on the pool; a slow or failing store, or a timeout, is "failed" and nothing changes. The
    /// request is not logged: it holds what was typed.
    async fn run_password(&mut self, request: russh_bbs_core::passwords::Request) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let response = timeout(
            PASSWORD_REQUEST_TIMEOUT,
            russh_bbs_core::passwords::execute(
                &*self.parts.directory,
                self.parts.passwords.pool(),
                &access,
                request,
            ),
        )
        .await
        .unwrap_or_else(|_| {
            warn!(session = ?self.parts.session.id(), "password request timed out");
            russh_bbs_core::passwords::Response::Refused(russh_bbs_core::passwords::Refusal::Failed)
        });
        self.parts.screen.deliver_password(response);
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

/// What an invite error looks like to the member. A store failure's cause is logged, not shown.
fn failed(error: InviteError) -> InviteOutcome {
    match error {
        InviteError::Denied(_) => InviteOutcome::Denied,
        InviteError::QuotaReached { limit, resets_at } => {
            InviteOutcome::QuotaReached { limit, resets_at }
        }
        InviteError::Store(error) => {
            warn!(%error, "invite request failed in the store");
            InviteOutcome::Failed
        }
    }
}

/// What a registration attempt came to, for [`Shell::try_redeem`]. Everything but a success looks
/// the same to the visitor; only a full day also degrades the room once they leave the form.
enum Made {
    Account(Account),
    CapReached,
    Refused,
}
