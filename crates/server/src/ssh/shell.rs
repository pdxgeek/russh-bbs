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

/// A frame that takes longer than this to hand to the connection means the client's link is slow.
/// Effects switch off for the session when that happens (UI-8).
const SLOW_SEND: Duration = Duration::from_millis(250);

use rand::TryRng as _;
use rand::rngs::SysRng;
use russh::ChannelId;
use russh::server::Handle;
use russh_bbs_core::access::{Access, Fingerprint, Permission, PermissionCheck as _, Snapshot};
use russh_bbs_core::chat::{ChatNotice, ChatRequest, ChatResponse};
use russh_bbs_core::founder::{
    ClaimOutcome, FOUNDER_USERNAME, claim_founder, claim_founder_with_password,
};
use russh_bbs_core::invites::{
    CODE_BYTES, InviteError, InviteOutcome, InviteRequest, create_invite, invite_tree, my_invites,
    redeem_invite, redeem_invite_with_password,
};
use russh_bbs_core::keys::{redeem_link_code, redeem_recovery_code};
use russh_bbs_core::limits::{LimitError, MemberMeter, SystemClock};
use russh_bbs_core::passwords::hash_new;
use russh_bbs_core::passwords::{Login, link_key_by_password};
use russh_bbs_core::queue::{
    InviteAsk, RequestKind, password_identity, submit_request, submit_request_with_password,
};
use russh_bbs_core::registration::{
    OpenError, open_account, open_account_with_password, submit_application,
    submit_application_with_password,
};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{Account, AccountId};
use russh_bbs_core::terminal_settings::{MemberFlow, MemberStart, TerminalSettings};
use russh_bbs_core::waiting_room::{CodeRequest, JoinKind, LinkRequest, RedeemRequest};
use tokio::sync::{Notify, mpsc};
use tokio::time::{sleep_until, timeout, timeout_at};
use tracing::{debug, info, warn};

use super::Backend;
use super::abort::Abort;
use super::live;
use super::pace::{Gate, PROBE, Pacer};
use super::password::PasswordLogin;
use super::registry::{Revocation, SessionRef};
use super::screen::{REVOKED_LINE, Screen};
use super::terminal::{KeyParser, PtySize};
use crate::chat::{ChatSession, Hub};
use crate::founder::Founder;

/// How long a member's session start (settings and login time) may take before the shell
/// falls back. A slow store must not hang the screen.
pub(crate) const MEMBER_START_TIMEOUT: Duration = Duration::from_secs(2);

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
pub(crate) struct Inbox {
    state: Mutex<InboxState>,
    wake: Notify,
}

impl Inbox {
    fn lock(&self) -> MutexGuard<'_, InboxState> {
        // The state is plain data; a panic elsewhere must not turn into an outage here.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queue input bytes for the next cycle. Bytes beyond the ceiling are dropped.
    pub(crate) fn push_input(&self, bytes: &[u8]) {
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
    pub(crate) fn set_resize(&self, size: PtySize) {
        self.lock().resize = Some(size);
        self.wake.notify_one();
    }

    /// Ask the shell to restore the client's terminal and close the channel (the client sent
    /// EOF).
    pub(crate) fn finish(&self) {
        self.lock().finish = true;
        self.wake.notify_one();
    }

    /// The channel or connection is gone: stop without sending anything.
    pub(crate) fn close(&self) {
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

/// Where a shell's frames go. The shell does not know which transport carries them (M3-H).
pub(crate) enum Output {
    /// An SSH channel: frames are channel data, and ending the shell closes the channel.
    Ssh {
        /// The connection's handle.
        handle: Handle,
        /// The session channel.
        channel: ChannelId,
    },
    /// A bounded queue drained by the transport's writer task (telnet). The transport encodes
    /// the bytes for its wire; ending the shell drops the sender, which ends the writer.
    Frames(mpsc::Sender<Vec<u8>>),
}

impl Output {
    /// Queue `bytes`. `Err` means the connection is gone.
    async fn send(&self, bytes: Vec<u8>) -> Result<(), ()> {
        match self {
            Self::Ssh { handle, channel } => handle.data(*channel, bytes).await.map_err(|_| ()),
            Self::Frames(frames) => frames.send(bytes).await.map_err(|_| ()),
        }
    }

    /// Close the output after the last frame. Errors only mean the connection is already gone.
    async fn close(&self) {
        if let Self::Ssh { handle, channel } = self {
            let _ = handle.exit_status_request(*channel, 0).await;
            let _ = handle.eof(*channel).await;
            let _ = handle.close(*channel).await;
        }
    }
}

/// What a shell needs from the connection around it.
pub(crate) struct ShellParts<D> {
    pub(crate) output: Output,
    pub(crate) screen: Screen,
    pub(crate) inbox: Arc<Inbox>,
    pub(crate) directory: Arc<D>,
    pub(crate) founder: Arc<Founder>,
    pub(crate) session: SessionRef,
    pub(crate) abort: Arc<Abort>,
    pub(crate) fingerprint: Option<Fingerprint>,
    pub(crate) peer_ip: Option<IpAddr>,
    pub(crate) account: Option<AccountId>,
    /// Password login's hashing pool, for the password screen (M3-D).
    pub(crate) passwords: Arc<PasswordLogin>,
    /// The chat hub (M3-F).
    pub(crate) chat: Arc<Hub>,
    /// The signed-in session's place in the resource limits (M2-H): its session slot and the
    /// minutes it has used. `None` for the waiting room, the founder claim and a refused start.
    pub(crate) meter: Option<MemberMeter>,
    pub(crate) redraw_interval: Duration,
    /// The most connections the board takes (the status widget's gauge, UI-10).
    pub(crate) max_connections: usize,
    pub(crate) write_stall: Duration,
    /// Applied to every permission snapshot the shell loads, after the store answers (M3-H,
    /// D27): the telnet transport uses it to drop what a staff role holds. `None` for SSH.
    pub(crate) access_filter: Option<fn(&Snapshot) -> Snapshot>,
}

/// One session's shell task.
pub(crate) struct Shell<D> {
    parts: ShellParts<D>,
    keys: KeyParser,
    /// Effect bytes the client has not yet acknowledged (FIX-M35-3).
    pacer: Pacer,
    /// The session's place in chat while the chat screen is up (M3-F). Dropping it leaves the
    /// room, so every way the shell can end (log off, client gone, write timeout, panic
    /// unwinding) takes the member out of chat.
    chat: Option<ChatSession>,
    /// When the permissions were last loaded, so a room's events can re-check them now and then
    /// without a key being pressed.
    loaded_access: Instant,
    /// What the editor autosave last stored, so the same text is not stored again.
    autosaved: Option<Autosaved>,
}

/// The last editor draft the autosave stored (FIX-M3-7).
struct Autosaved {
    key: (russh_bbs_core::drafts::ContextKey, String),
    at: Instant,
}

/// The least time between two periodic editor autosaves (FIX-M3-7, D133).
const AUTOSAVE_EVERY: Duration = Duration::from_secs(20);

/// How long a member keeps receiving a room's events after losing `chat.join` on it when they
/// press no key. A key re-checks at once; this bounds the silent case (D55 loads per action, and
/// a push has no action).
const PUSH_RECHECK: Duration = Duration::from_secs(10);

impl<D: Backend> Shell<D> {
    pub(crate) fn new(parts: ShellParts<D>) -> Self {
        if let Some(account) = parts.account {
            parts.session.set_account(account);
        }
        Self {
            parts,
            keys: KeyParser::new(),
            pacer: Pacer::default(),
            chat: None,
            loaded_access: Instant::now(),
            autosaved: None,
        }
    }

    /// Run until the shell ends: the person logs off, the client leaves, or the connection is
    /// dropped.
    pub(crate) async fn run(mut self) {
        self.run_loop().await;
        // However the loop ended (the client left, the connection dropped, a limit), the open
        // editor's text is kept (FIX-M3-7, D133).
        self.autosave_draft(true).await;
        self.settle_meter().await;
    }

    async fn run_loop(&mut self) {
        self.refresh_widgets().await;
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
        self.announce_unread_mail().await;
        let mut next_cycle = tokio::time::Instant::now() + self.parts.redraw_interval;
        loop {
            self.wait_for_work().await;
            // The redraw rate cap. Whatever arrives while we wait is folded into this cycle.
            sleep_until(next_cycle).await;
            let batch = self.parts.inbox.take();
            if batch.closed {
                return;
            }
            if self.parts.session.revoke_signal().is_revoked() {
                self.end_revoked().await;
                return;
            }
            self.pump_chat().await; // what the room sent, before the keys act (M3-F)
            let now = Instant::now();
            let mut keys = if batch.input.is_empty() {
                Vec::new()
            } else {
                self.keys.feed(&batch.input, now)
            };
            // The client's answers to the probes after effect frames say how much it has taken
            // (FIX-M35-3): a frame is drawn only if it is not too far behind.
            self.take_parser_reports();
            self.pace_effects(now);
            if batch.input.is_empty() && self.keys.deadline().is_some_and(|d| d <= now) {
                keys = self.keys.expire();
            }
            if let Some(size) = batch.resize {
                self.parts.screen.set_size(size);
            }
            // The mouse (UI-9): reports become the keys they stand for and join the batch, so
            // they take the ordinary path. Reports that arrive with typed keys are dropped.
            let mouse = self.keys.take_mouse();
            let clicks = self.parts.screen.mouse_keys(&mouse, !keys.is_empty());
            keys.extend(clicks);
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
            if let Some((layout, list)) = effects.widget_save.take() {
                self.persist_widgets(layout, list.as_deref()).await;
            }
            self.apply_chat_effects(&effects);
            self.answer_requests(&mut effects).await;
            self.run_member_requests(&mut effects).await;
            self.autosave_draft(false).await;
            let mut leaving = effects.disconnect || batch.finish || out_of_time;
            if !leaving && let Some(request) = effects.redeem {
                self.try_redeem(request).await;
            }
            if !leaving && let Some(request) = effects.link.take() {
                self.try_link(request).await;
            }
            if !leaving && let Some(request) = effects.link_device.take() {
                self.try_code(request).await;
            }
            if !leaving && let Some(request) = effects.recover.take() {
                self.try_code(request).await;
            }
            if !leaving
                && let Some(token) = effects.claim_token
                && self.try_claim(&token, effects.claim_password.take()).await
            {
                leaving = true;
            }
            self.interaction_upkeep(effects.palette).await;
            if !batch.finish {
                self.refresh_widgets().await;
                self.parts.screen.motion_tick(Instant::now());
            }
            if !batch.finish && !self.draw_frame().await {
                return;
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

    /// Advance the effect clock by what the link has carried (FIX-M35-3). While the client has
    /// not taken the last effect bytes the due frame is dropped; when it has taken none of a
    /// probe for too long, motion is over for the session and the static screen is drawn. Called
    /// before the keys are applied, so a key in the same batch still ends the effect itself.
    fn pace_effects(&mut self, now: Instant) {
        match self.pacer.gate(now) {
            Gate::Open => self.parts.screen.tick_fx_gated(now, true),
            Gate::Held => self.parts.screen.tick_fx_gated(now, false),
            Gate::TooSlow => {
                debug!(session = ?self.parts.session.id(), "client is behind on effect frames; static screen");
                self.pacer.clear();
                self.parts.screen.slow_link();
            }
        }
    }

    /// Draw what changed and send it. The last frame goes out before the terminal is restored,
    /// so a final message (the too-many-attempts notice) is seen. `false` means the shell must
    /// stop: the connection is gone or the frame could not be drawn.
    async fn draw_frame(&mut self) -> bool {
        let effect = self.parts.screen.fx_running();
        match self.parts.screen.render() {
            Ok(Some(mut frame)) => {
                // An effect frame ends with a request the terminal answers once it has taken
                // every byte before it (FIX-M35-3).
                let probed = effect;
                if probed {
                    frame.extend_from_slice(PROBE);
                }
                let started = Instant::now();
                let len = frame.len();
                let sent = self.send(frame).await;
                if sent && probed {
                    self.pacer.sent(started, len);
                }
                // A send that had to wait for the client is a slow link. Effects stop for the
                // rest of the session (UI-8), so no frame queues up in front of the next key.
                if sent && started.elapsed() > SLOW_SEND {
                    self.parts.screen.slow_link();
                }
                sent
            }
            Ok(None) => true,
            Err(error) => {
                warn!(session = ?self.parts.session.id(), %error, "frame could not be drawn");
                self.parts.abort.trigger();
                false
            }
        }
    }

    /// A members, queue, password or forum screen needs the store before it can take the next
    /// key. Answer it, then apply the keys that were held back, with the account's
    /// permissions loaded again (every action sees what the account holds now).
    async fn answer_requests(&mut self, effects: &mut super::screen::KeyEffects) {
        loop {
            if let Some(request) = effects.members_request.take() {
                self.run_members(request).await;
            } else if let Some(request) = effects.chat_request.take() {
                self.run_chat(request).await;
            } else if let Some(request) = effects.queue_request.take() {
                self.run_queue(request).await;
            } else if let Some(request) = effects.password_request.take() {
                self.run_password(request).await;
            } else if let Some(request) = effects.keys_request.take() {
                self.run_keys(request).await;
            } else if let Some(request) = effects.forum_request.take() {
                self.run_forums(request).await;
            } else if let Some(request) = effects.mail_request.take() {
                self.run_mail(request).await;
            } else if let Some(request) = effects.community_request.take() {
                self.run_community(request).await;
            } else if let Some(request) = effects.door_request.take() {
                self.run_doors(request).await;
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

    /// What the parser noticed in the input that just arrived: the client's answers to the
    /// effect probes, and non-ASCII characters it had to drop, which are said once in a toast
    /// so the loss is not silent (FIX-M3-7, D133 (1)).
    fn take_parser_reports(&mut self) {
        self.pacer.acked(self.keys.take_acks());
        if self.keys.take_dropped() > 0 {
            self.parts.screen.note_dropped_text();
        }
    }

    /// Keep the open editor's text as a draft (FIX-M3-7, D133). `force` is the end of the
    /// session; otherwise it is the periodic autosave, which runs each cycle before the frame (the
    /// text only changes on keys, and unchanged text is skipped), at most every [`AUTOSAVE_EVERY`], and only when the text is not
    /// what was last stored. Stored through the existing drafts store inside D80's caps; a
    /// refusal (the cap, storage) is logged and the editor carries on, as the member's own
    /// save would show it.
    async fn autosave_draft(&mut self, force: bool) {
        let Some(save) = self.parts.screen.unsaved_draft() else {
            return;
        };
        let now = Instant::now();
        if !force
            && self
                .autosaved
                .as_ref()
                .is_some_and(|last| last.at + AUTOSAVE_EVERY > now)
        {
            return;
        }
        let key = (save.context().clone(), save.text().to_owned());
        if self.autosaved.as_ref().is_some_and(|last| last.key == key) {
            return;
        }
        let Some(account) = self.parts.account else {
            return;
        };
        let stored = timeout(
            MEMBER_START_TIMEOUT,
            save.persist(
                &*self.parts.directory,
                account,
                russh_bbs_core::limits::Clock::now(&SystemClock),
            ),
        )
        .await;
        match stored {
            Ok(Ok(_)) => self.autosaved = Some(Autosaved { key, at: now }),
            Ok(Err(error)) => {
                info!(session = ?self.parts.session.id(), %error, "editor draft was not autosaved");
            }
            Err(_) => {
                warn!(session = ?self.parts.session.id(), "editor draft autosave timed out");
            }
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

    /// Wait for the handler to hand over something, for a room to send something (the push
    /// path, M3-F), or for the parser's deadline.
    async fn wait_for_work(&self) {
        let woken = async {
            let inbox = self.parts.inbox.wake.notified();
            let room = async {
                match &self.chat {
                    Some(chat) => chat.woken().await,
                    None => std::future::pending().await,
                }
            };
            // A widget on screen wants fresher data at its budget (never sooner than five
            // seconds apart, far under the D49 redraw cap); none on screen, no timer.
            let widgets = async {
                match self.parts.screen.widgets_deadline() {
                    Some(at) => sleep_until(tokio::time::Instant::from_std(at)).await,
                    None => std::future::pending().await,
                }
            };
            // A screen change is wiping in, or an input's cursor is blinking (UI-4): wake for
            // the next frame. Under the redraw cap by construction; none with motion off.
            let motion = async {
                match self.parts.screen.motion_deadline() {
                    Some(at) => sleep_until(tokio::time::Instant::from_std(at)).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                () = inbox => {}
                () = room => {}
                () = widgets => {}
                () = motion => {}
                () = self.parts.session.revoke_signal().revoked() => {}
            }
        };
        // The earliest of the parser's deadline (a held `Esc`), the screen's own (a toast
        // expiring, the ticker moving) and the next effect frame (UI-8). Each only wakes the loop
        // the one time it is due and the loop's redraw cap still applies; with nothing pending
        // there is no deadline, so motion off arms no timer.
        let deadline = [
            self.keys.deadline(),
            self.parts.screen.next_wake(Instant::now()),
            self.parts.screen.next_fx_due(),
        ]
        .into_iter()
        .flatten()
        .min();
        match deadline {
            Some(deadline) => {
                // Elapsing is the point: the cycle then expires the held `Esc`, the toast or
                // draws the next effect frame.
                let _ = timeout_at(tokio::time::Instant::from_std(deadline), woken).await;
            }
            None => woken.await,
        }
    }

    /// The chat screen opened or was left by these keys: join or leave the hub.
    fn apply_chat_effects(&mut self, effects: &super::screen::KeyEffects) {
        if effects.chat_exit {
            self.chat = None;
        }
        if effects.chat_open {
            self.connect_chat();
        }
    }

    /// Put the member in chat (the chat screen just opened). A full hub leaves the session
    /// without one; the screen's first request then says chat is full.
    fn connect_chat(&mut self) {
        self.chat = None;
        let (Some(account), Ok(handle)) = (
            self.parts.account,
            sanitize_username(self.parts.screen.member_handle().unwrap_or_default()),
        ) else {
            return;
        };
        match self.parts.chat.connect(account, handle) {
            Ok(session) => self.chat = Some(session),
            Err(error) => {
                info!(session = ?self.parts.session.id(), %error, "chat is full; not joined");
            }
        }
    }

    /// Show what the member's room sent since the last cycle, before the keys act, so a key
    /// meets the page as it is now (a closed room has already sent the member back to the
    /// list); and now and then check that they may still be in the room.
    async fn pump_chat(&mut self) {
        let Some(events) = self.chat.as_ref().map(ChatSession::drain) else {
            return;
        };
        if events.is_empty() {
            return;
        }
        let follow_up = self.parts.screen.chat_receive(events);
        if let Some(request) = follow_up {
            self.run_chat(request).await;
        }
        if self.chat.is_some()
            && self.parts.screen.chat_room().is_some()
            && self.loaded_access.elapsed() >= PUSH_RECHECK
        {
            self.refresh_access().await;
            // Any in-room request re-checks `chat.join` on the room. `Who` is the lightest.
            self.run_chat(ChatRequest::Who).await;
        }
    }

    /// Run the chat request the chat screen is waiting on and give it the answer. Core checks the
    /// permissions against the snapshot loaded for this action; nothing is cached, and nothing
    /// said is logged.
    async fn run_chat(&mut self, request: ChatRequest) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let response = match &self.chat {
            Some(session) => {
                session
                    .execute(&*self.parts.directory, &access, request)
                    .await
            }
            None => ChatResponse::Refused(ChatNotice::ChatFull),
        };
        self.parts.screen.chat_deliver(response);
    }

    /// Hand a frame to the session's bounded output queue. `false` means the shell must stop:
    /// the connection is gone, or the peer has not read for `write_stall` and was dropped.
    async fn send(&self, bytes: Vec<u8>) -> bool {
        if bytes.is_empty() {
            return true;
        }
        let len = bytes.len();
        match timeout(self.parts.write_stall, self.parts.output.send(bytes)).await {
            Ok(Ok(())) => {
                self.parts.session.add_frame(len);
                true
            }
            Ok(Err(())) => {
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
    /// Record the account this session became (FIX-M3-14), and whether it also used the
    /// password, so a later revocation can find it.
    fn sign_in_as(&self, account: AccountId, by_password: bool) {
        self.parts.session.set_account(account);
        if by_password {
            self.parts.session.mark_password();
        }
    }

    /// The credential this session signed in with was revoked (FIX-M3-14, D99): show the one
    /// fixed line and close like a log off, the same over ssh and telnet.
    async fn end_revoked(&mut self) {
        info!(session = ?self.parts.session.id(), "session ended: its sign-in was revoked");
        self.parts
            .screen
            .limit_reached(vec![REVOKED_LINE.to_owned()]);
        self.draw_frame().await;
        self.finish().await;
    }

    /// End this account's other live sessions that used the credential `what` revoked, and
    /// write one `session.revoked` row for each (ids and a reason only). This session stays
    /// open. In this process only: see `docs/access.md`.
    async fn revoke_others(&self, what: &Revocation) {
        let Some(account) = self.parts.account else {
            return;
        };
        for ended in self.parts.session.revoke_others(account, what) {
            let event =
                russh_bbs_core::audit::events::session_revoked(account, ended.as_raw(), what.tag());
            if let Err(error) =
                russh_bbs_core::audit::AuditStore::append(&*self.parts.directory, &event).await
            {
                warn!(session = ?self.parts.session.id(), %error, "session.revoked could not be audited");
            }
        }
    }

    async fn finish(&mut self) {
        // A member who logged off gets their goodbye typed out (UI-8). Any key, or the
        // connection closing, prints the rest at once; the text is the same either way.
        match self.parts.screen.leave_typed() {
            Ok(Some(farewell)) => {
                if self.type_farewell(farewell).await {
                    self.parts.session.set_shell_open(false);
                    self.parts.output.close().await;
                    return;
                }
            }
            Ok(None) => {}
            Err(error) => {
                warn!(session = ?self.parts.session.id(), %error, "terminal could not be restored");
            }
        }
        match self.parts.screen.leave() {
            Ok(bytes) => {
                self.send(bytes).await;
            }
            Err(error) => {
                warn!(session = ?self.parts.session.id(), %error, "terminal could not be restored");
            }
        }
        self.parts.session.set_shell_open(false);
        self.parts.output.close().await;
    }

    /// Print a typed goodbye: the restore bytes, then the chunks one interval apart. A key (or
    /// the connection closing) ends the wait and sends what is left at once. `false` means the
    /// connection is gone.
    async fn type_farewell(&mut self, farewell: super::screen::Farewell) -> bool {
        if !self.send(farewell.restore).await {
            return false;
        }
        let mut chunks = farewell.chunks.into_iter();
        let mut skipping = false;
        while let Some(chunk) = chunks.next() {
            if skipping {
                // Everything left, in one write: the same text, with no more waiting.
                let mut rest = chunk;
                for more in chunks.by_ref() {
                    rest.extend(more);
                }
                return self.send(rest).await;
            }
            if !self.send(chunk).await {
                return false;
            }
            let woken = self.parts.inbox.wake.notified();
            skipping = timeout(farewell.interval, woken).await.is_ok();
        }
        true
    }

    /// Check a token typed at the claim prompt and update the screen. Every failure (limited,
    /// wrong token, already claimed, store error) is the same generic message. Returns whether
    /// the session should now end.
    async fn try_claim(
        &mut self,
        token: &str,
        password: Option<russh_bbs_core::passwords::Secret>,
    ) -> bool {
        // A claimant with no key (`ssh claim@host` with none auth) is counted under a fixed
        // stand-in, so keyless attempts share one allowance and the per-network one.
        let key = self.parts.fingerprint.clone();
        let identity = key.clone().unwrap_or_else(|| {
            password_identity(
                &sanitize_username(FOUNDER_USERNAME).unwrap_or_else(|_| unreachable!()),
            )
        });
        let outcome = if self
            .parts
            .founder
            .admit_attempt(&identity, self.parts.peer_ip)
            .is_allowed()
        {
            match password {
                Some(password) => match hash_new(self.parts.passwords.pool(), &password).await {
                    Ok(hash) => {
                        claim_founder_with_password(
                            self.parts.founder.gate(),
                            &*self.parts.directory,
                            token,
                            &hash,
                            key.as_ref(),
                            &identity,
                        )
                        .await
                    }
                    Err(_) => ClaimOutcome::Rejected,
                },
                None => match &key {
                    Some(key) => {
                        claim_founder(
                            self.parts.founder.gate(),
                            &*self.parts.directory,
                            token,
                            key,
                        )
                        .await
                    }
                    None => ClaimOutcome::Rejected,
                },
            }
        } else {
            info!(session = ?self.parts.session.id(), fingerprint = %identity, "founder claim rate limited");
            ClaimOutcome::Rejected
        };
        match outcome {
            ClaimOutcome::Claimed(account) => {
                match key {
                    Some(key) => self.parts.session.set_authenticated(key, Access::Member),
                    None => self
                        .parts
                        .session
                        .set_authenticated_by_password(Access::Member),
                }
                let start = self.account_start(&account).await;
                self.parts.account = Some(start.account);
                self.sign_in_as(start.account, true);
                let flow = self.member_flow(start);
                self.parts.screen.claim_accepted(flow);
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
        let RedeemRequest {
            join,
            handle,
            password,
        } = request;
        let fingerprint = self.parts.fingerprint.clone();
        let work = async {
            let Ok(access) = self.parts.directory.access_for(None).await else {
                return Made::Refused;
            };
            let directory = &*self.parts.directory;
            // Every join form asks for a password (D94 (6)): the account is bound to it and,
            // when the visitor came with an SSH key, to that key too. A form with no password
            // (not offered any more) falls back to the key alone; with neither, nothing is made.
            let bound = match (&fingerprint, &password) {
                (_, Some(password)) => {
                    match hash_new(self.parts.passwords.pool(), password).await {
                        Ok(hash) => Bound::Password(hash, fingerprint.as_ref()),
                        Err(error) => {
                            info!(session = ?self.parts.session.id(), %error, "password not usable; registration refused");
                            return Made::Refused;
                        }
                    }
                }
                (Some(fingerprint), None) => Bound::Key(fingerprint),
                (None, None) => return Made::Refused,
            };
            match (join, bound) {
                (JoinKind::Invite { code }, Bound::Key(fingerprint)) => {
                    match redeem_invite(directory, &access, &code, handle, fingerprint).await {
                        Ok(redeemed) => Made::Account(redeemed.account),
                        Err(_) => Made::Refused,
                    }
                }
                (JoinKind::Invite { code }, Bound::Password(hash, key)) => {
                    match redeem_invite_with_password(directory, &access, &code, handle, &hash, key)
                        .await
                    {
                        Ok(redeemed) => Made::Account(redeemed.account),
                        Err(_) => Made::Refused,
                    }
                }
                (JoinKind::Open, Bound::Key(fingerprint)) => {
                    let opened = open_account(
                        directory,
                        self.parts.session.open_gate(),
                        &access,
                        handle,
                        fingerprint,
                        self.parts.peer_ip,
                    )
                    .await;
                    made_by_open(opened)
                }
                (JoinKind::Open, Bound::Password(hash, key)) => {
                    let opened = open_account_with_password(
                        directory,
                        self.parts.session.open_gate(),
                        &access,
                        handle,
                        &hash,
                        key,
                        self.parts.peer_ip,
                    )
                    .await;
                    made_by_open(opened)
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
        match fingerprint {
            Some(fingerprint) => self
                .parts
                .session
                .set_authenticated(fingerprint, Access::Member),
            None => self
                .parts
                .session
                .set_authenticated_by_password(Access::Member),
        }
        let start = self.account_start(&account).await;
        // A member who just joined starts a member session: last callers lists them.
        crate::community::note_sign_in(&*self.parts.directory, start.account).await;
        self.parts.account = Some(start.account);
        self.sign_in_as(start.account, true);
        let flow = self.member_flow(start);
        self.parts.screen.redeemed(flow);
    }

    /// "I already have an account" (D94 (8)): check the handle and password as a login is
    /// checked (the same per-handle and per-network limits, the same dummy-hash path) and bind
    /// this session's key to the account. Every failure is the room's one refusal.
    async fn try_link(&mut self, request: russh_bbs_core::waiting_room::LinkRequest) {
        let Some(fingerprint) = self.parts.fingerprint.clone() else {
            self.parts.screen.redeem_refused();
            return;
        };
        let LinkRequest { handle, password } = request;
        if let russh_bbs_core::waiting_room::Verdict::Limited { retry_after } =
            self.parts.passwords.admit(self.parts.peer_ip, &handle)
        {
            info!(session = ?self.parts.session.id(), ?retry_after, "key link rate limited");
            self.parts.screen.redeem_refused();
            return;
        }
        let work = link_key_by_password(
            &*self.parts.directory,
            self.parts.passwords.pool(),
            &handle,
            &password,
            &fingerprint,
        );
        let checked = timeout(MEMBER_START_TIMEOUT, work).await;
        if matches!(checked, Ok(Ok(Login::Denied))) {
            self.parts.passwords.failed(self.parts.peer_ip, &handle);
        }
        let Ok(Ok(Login::Accepted(account))) = checked else {
            self.parts.screen.redeem_refused();
            return;
        };
        self.parts.passwords.succeeded(&handle);
        self.parts
            .session
            .set_authenticated(fingerprint, Access::Member);
        let start = self.account_start(&account).await;
        crate::community::note_sign_in(&*self.parts.directory, start.account).await;
        self.parts.account = Some(start.account);
        self.sign_in_as(start.account, true);
        let flow = self.member_flow(start);
        self.parts.screen.redeemed(flow);
    }

    /// The signed-in flow for a session start. A session with no SSH key signed in (or joined)
    /// by password, so changing the password asks for the current one (D81); the transport's
    /// access filter (staff roles dropped over telnet, D27) is applied before the flow sees the
    /// snapshot.
    fn member_flow(&self, start: MemberStart) -> MemberFlow {
        let access = match self.parts.access_filter {
            Some(filter) => filter(&start.access),
            None => start.access,
        };
        let flow = MemberFlow::new(&start.handle, access, start.previous_login, start.settings)
            .with_menu_tree(self.parts.screen.ui_handle().menus().tree());
        // The telnet transport is the one with an access filter (D27): it has no key to bind.
        let flow = if self.parts.access_filter.is_some() {
            flow.over_telnet()
        } else {
            flow
        };
        let flow = if self.parts.passwords.enabled() {
            flow
        } else {
            flow.with_password_login_off()
        };
        if self.parts.fingerprint.is_none() {
            flow.signed_in_with_password()
        } else {
            flow
        }
    }

    /// A device-link code or a staff recovery code typed in the waiting room (FIX-M3-11): bind
    /// this connection's key to the account the code was made for (a recovery code also sets
    /// the new password), subject to the same per-network limit as a password attempt and one of
    /// its own per key. Every failure is the room's one refusal.
    async fn try_code(&mut self, request: CodeRequest) {
        let Some(fingerprint) = self.parts.fingerprint.clone() else {
            self.parts.screen.redeem_refused();
            return;
        };
        let attempt = format!("code-{fingerprint}");
        if let russh_bbs_core::waiting_room::Verdict::Limited { retry_after } =
            self.parts.passwords.admit(self.parts.peer_ip, &attempt)
        {
            info!(session = ?self.parts.session.id(), ?retry_after, "code redemption rate limited");
            self.parts.screen.redeem_refused();
            return;
        }
        let directory = &*self.parts.directory;
        let work = async {
            let access = directory.access_for(None).await.ok()?;
            match &request.password {
                Some(password) => redeem_recovery_code(
                    directory,
                    self.parts.passwords.pool(),
                    &access,
                    &request.code,
                    &fingerprint,
                    password,
                )
                .await
                .ok(),
                None => redeem_link_code(directory, &access, &request.code, &fingerprint)
                    .await
                    .ok(),
            }
        };
        let Ok(Some(account)) = timeout(PASSWORD_REQUEST_TIMEOUT, work).await else {
            self.parts.passwords.failed(self.parts.peer_ip, &attempt);
            self.parts.screen.redeem_refused();
            return;
        };
        self.parts.passwords.succeeded(&attempt);
        self.parts
            .session
            .set_authenticated(fingerprint, Access::Member);
        let start = self.account_start(&account).await;
        crate::community::note_sign_in(&*self.parts.directory, start.account).await;
        self.parts.account = Some(start.account);
        self.sign_in_as(start.account, request.password.is_some());
        if request.password.is_some()
            && let Some(fingerprint) = self.parts.fingerprint.clone()
        {
            self.revoke_others(&Revocation::Recovery(fingerprint)).await;
        }
        let flow = self.member_flow(start);
        self.parts.screen.redeemed(flow);
    }

    /// Offer a request for an invite, or a membership application, typed in the waiting room to
    /// the queue (M2-E, M2-G). The room
    /// has already shown its one fixed answer, and the frame carrying it is sent before this
    /// runs, so nothing here reaches the screen or delays it: a drop (a
    /// rate limit, a full queue, a repeat, a recent refusal, a slow or broken store) is logged
    /// by core without the request's text and looks like a kept request to the person. The
    /// waiting snapshot must hold `invite.request` (checked in core). A visitor with no SSH key
    /// (telnet, D94) is queued with the hash of the password they chose.
    async fn try_request_invite(&mut self, mut ask: InviteAsk) {
        let fingerprint = self.parts.fingerprint.clone();
        let password = ask.password.take();
        let work = async {
            let access = self.parts.directory.access_for(None).await.ok()?;
            let directory = &*self.parts.directory;
            let gate = self.parts.session.request_gate();
            let peer = self.parts.peer_ip;
            if let (Some(fingerprint), None) = (&fingerprint, &password) {
                return Some(match ask.kind {
                    RequestKind::Invite => {
                        submit_request(directory, gate, &access, ask, fingerprint, peer).await
                    }
                    RequestKind::Application => {
                        submit_application(directory, gate, &access, ask, fingerprint, peer).await
                    }
                });
            }
            let password = password?;
            let hash = hash_new(self.parts.passwords.pool(), &password)
                .await
                .ok()?;
            Some(match ask.kind {
                RequestKind::Invite => {
                    submit_request_with_password(
                        directory,
                        gate,
                        &access,
                        ask,
                        hash,
                        fingerprint.as_ref(),
                        peer,
                    )
                    .await
                }
                RequestKind::Application => {
                    submit_application_with_password(
                        directory,
                        gate,
                        &access,
                        ask,
                        hash,
                        fingerprint.as_ref(),
                        peer,
                    )
                    .await
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
    /// The requests a member's keys made that need the store or the disk: an invite action, then
    /// a theme-pack action (UI-7). At most one of each per batch, in that order.
    async fn run_member_requests(&mut self, effects: &mut super::screen::KeyEffects) {
        if let Some(request) = effects.invite.take() {
            self.run_invite(request).await;
        }
        if let Some(request) = effects.packs.take() {
            self.run_packs(request).await;
        }
        if let Some(request) = effects.menu_tools.take() {
            self.run_menu_tools(request).await;
        }
    }

    /// Carry out a theme-pack request (UI-7): the files on a blocking thread, then the audit row
    /// for a change. The permission is checked again where the work is done.
    async fn run_packs(&mut self, request: russh_bbs_core::terminal_settings::PackRequest) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let ui = self.parts.screen.ui_handle();
        let job = tokio::task::spawn_blocking(move || {
            crate::ui::theme::service::run(&ui, &access, request)
        });
        let Ok(Ok(outcome)) = timeout(MEMBER_START_TIMEOUT, job).await else {
            warn!(session = ?self.parts.session.id(), "theme pack request failed or timed out");
            return;
        };
        if let Some(event) = &outcome.event
            && let Err(error) =
                russh_bbs_core::audit::AuditStore::append(&*self.parts.directory, event).await
        {
            // The files are already written; say so rather than pretend nothing happened.
            warn!(session = ?self.parts.session.id(), %error, "theme pack change could not be audited");
        }
        self.parts
            .screen
            .packs_done(outcome.listing, Some(&outcome.message));
    }

    /// Carry out a menus-and-pages request (UI-6): the roles from the store, the files on a
    /// blocking thread, then the audit row for a change. The permission is checked again where
    /// the work is done.
    async fn run_menu_tools(&mut self, request: russh_bbs_core::menu::MenuToolsRequest) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let roles = russh_bbs_core::access::AccessStore::roles(&*self.parts.directory)
            .await
            .unwrap_or_default();
        let ui = self.parts.screen.ui_handle();
        let job = tokio::task::spawn_blocking(move || {
            crate::ui::sysop_menus::service::run(&ui, &access, &roles, request)
        });
        let Ok(Ok(outcome)) = timeout(MEMBER_START_TIMEOUT, job).await else {
            warn!(session = ?self.parts.session.id(), "menus request failed or timed out");
            return;
        };
        if let Some(event) = &outcome.event
            && let Err(error) =
                russh_bbs_core::audit::AuditStore::append(&*self.parts.directory, event).await
        {
            warn!(session = ?self.parts.session.id(), %error, "menu reload could not be audited");
        }
        self.parts
            .screen
            .menu_tools_done(outcome.listing, Some(&outcome.message));
    }

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
    /// Read the member's widget lists once, and fetch widget data when the screen shows widgets
    /// whose budget has run out (UI-10). Everything goes through `core::widgets::fetch` and the
    /// live pieces in [`live`], each gated by what the session holds right now. A slow or failed
    /// read gives the screen what could be read, and the next attempt waits a full budget.
    async fn refresh_widgets(&mut self) {
        if let Some(account) = self.parts.screen.access().and_then(Snapshot::account)
            && !self.parts.screen.widget_choice_loaded()
        {
            let loaded = timeout(
                MEMBER_START_TIMEOUT,
                russh_bbs_core::widgets::WidgetStore::widget_choice(
                    &*self.parts.directory,
                    account,
                ),
            )
            .await;
            match loaded {
                Ok(Ok(choice)) => self.parts.screen.set_widget_choice(choice),
                Ok(Err(error)) => {
                    warn!(session = ?self.parts.session.id(), %error, "widget lists could not be loaded; using the defaults");
                    self.parts
                        .screen
                        .set_widget_choice(russh_bbs_core::widgets::Choice::new());
                }
                Err(_) => {
                    warn!(session = ?self.parts.session.id(), "widget lists timed out; using the defaults");
                    self.parts
                        .screen
                        .set_widget_choice(russh_bbs_core::widgets::Choice::new());
                }
            }
        }
        let Some(wanted) = self.parts.screen.widgets_due(Instant::now()) else {
            return;
        };
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let now = russh_bbs_core::limits::Clock::now(&SystemClock);
        let since = self.parts.screen.last_login();
        let fetched = timeout(
            MEMBER_START_TIMEOUT,
            russh_bbs_core::widgets::fetch(&*self.parts.directory, &access, &wanted, now, since),
        )
        .await;
        let mut data = fetched.unwrap_or_else(|_| {
            warn!(session = ?self.parts.session.id(), "widget data timed out");
            russh_bbs_core::widgets::WidgetData::default()
        });
        let sources = live::Sources {
            directory: &*self.parts.directory,
            session: &self.parts.session,
            hub: &self.parts.chat,
            doors: self.parts.screen.doors(),
            capacity: self.parts.max_connections,
        };
        let live_part = live::fill(&mut data, &wanted, &access, &sources);
        if timeout(MEMBER_START_TIMEOUT, live_part).await.is_err() {
            warn!(session = ?self.parts.session.id(), "live widget data timed out");
        }
        // The split layout's mail pane lists the newest messages. The inbox call checks
        // `mail.read` itself and opens subjects with the board's mail key.
        if self.parts.screen.wants_side_panes()
            && let Some(keys) = self.parts.screen.mail_keys()
        {
            let page = timeout(
                MEMBER_START_TIMEOUT,
                russh_bbs_core::mail::inbox(&*self.parts.directory, &keys, &access, 0),
            )
            .await;
            if let Ok(Ok(page)) = page {
                data.recent_mail = Some(
                    page.entries
                        .into_iter()
                        .take(8)
                        .map(|entry| russh_bbs_core::widgets::RecentMail {
                            from: entry.from,
                            subject: entry.subject,
                            unread: entry.unread,
                            at: entry.at,
                        })
                        .collect(),
                );
            }
        }
        self.parts.screen.deliver_widgets(data, wanted);
    }

    /// Store the member's widget list for `layout` (`None`: follow the default again).
    async fn persist_widgets(
        &self,
        layout: russh_bbs_core::terminal_settings::LayoutId,
        list: Option<&[russh_bbs_core::widgets::WidgetId]>,
    ) {
        let Some(access) = self.parts.screen.access() else {
            return;
        };
        let saved = timeout(
            MEMBER_START_TIMEOUT,
            russh_bbs_core::widgets::save(&*self.parts.directory, access, layout, list),
        )
        .await;
        match saved {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                warn!(session = ?self.parts.session.id(), %error, "widget list could not be saved");
            }
            Err(_) => {
                warn!(session = ?self.parts.session.id(), "saving the widget list timed out");
            }
        }
    }

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
        let access = match self.parts.access_filter {
            Some(filter) => filter(&access),
            None => access,
        };
        self.parts.screen.set_access(access);
        self.loaded_access = Instant::now();
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

    /// Run the forum request a screen is waiting on and give it the answer (M3-E). Core checks
    /// every permission against the snapshot loaded for this action; a slow or failing store
    /// is a plain failure on screen. A removal or a post asks for the list or thread again, so
    /// the follow-up runs here with the same permissions (a few at most).
    async fn run_forums(&mut self, request: crate::forums::Request) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let mut request = request;
        for _ in 0..3 {
            let response = crate::forums::run(&*self.parts.directory, &access, request).await;
            match self.parts.screen.deliver_forum(response) {
                Some(next) => request = next,
                None => break,
            }
        }
    }

    /// Before each frame (UI-9): load what the palette and the ticker asked for, and let time
    /// pass for toasts and the ticker.
    async fn interaction_upkeep(&mut self, palette: super::screen::PaletteAsk) {
        // The editor's text only changes on keys, so this is where the keys' result is kept.
        self.autosave_draft(false).await;
        if palette == super::screen::PaletteAsk::Boards {
            self.load_palette_boards().await;
        }
        let now = Instant::now();
        if self.parts.screen.ticker_headlines_due(now) {
            self.load_ticker_headlines(now).await;
        }
        self.parts.screen.tick(now);
    }

    /// Tell a member who signs in with unread mail so, with a toast (UI-9). Only when they may
    /// read mail on this transport (telnet withholds it, D90) and the board can open it; a slow
    /// or failing store just means no toast.
    async fn announce_unread_mail(&mut self) {
        let Some(account) = self.parts.account else {
            return;
        };
        let may_read = self
            .parts
            .screen
            .access()
            .is_some_and(|access| access.allows(Permission::MailRead));
        if !may_read || self.parts.screen.mail_keys().is_none() {
            return;
        }
        let counts = timeout(
            MEMBER_START_TIMEOUT,
            russh_bbs_core::mail::MailStore::inbox_counts(&*self.parts.directory, account),
        )
        .await;
        if let Ok(Ok((_, unread))) = counts
            && unread > 0
        {
            self.parts.screen.toast(
                crate::ui::toast::Kind::Mail,
                crate::ui::toast::Text::Count("toast.new_mail", unread),
            );
        }
    }

    /// The palette opened: ask the forum service which boards this member may list (it filters
    /// through `core::access`) and hand the names to the palette. A slow or failing store leaves
    /// the palette without boards; nothing else changes.
    async fn load_palette_boards(&mut self) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let listed = timeout(
            MEMBER_START_TIMEOUT,
            russh_bbs_core::forums::boards(&*self.parts.directory, &access),
        )
        .await;
        if let Ok(Ok(boards)) = listed {
            self.parts
                .screen
                .set_palette_boards(boards.into_iter().map(|board| board.name).collect());
        }
    }

    /// Fetch the newest headlines of the feed boards this member may read, for the header
    /// ticker. Only when the sysop turned feed headlines on; a failure keeps the old ones and
    /// asks again after the usual interval.
    async fn load_ticker_headlines(&mut self, now: Instant) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let store = &*self.parts.directory;
        let gather = async {
            let mut subjects = Vec::new();
            for board in russh_bbs_core::forums::boards(store, &access).await? {
                if !board.feed {
                    continue;
                }
                let page = russh_bbs_core::forums::threads(store, &access, board.id, 0).await?;
                subjects.extend(page.threads.into_iter().map(|thread| thread.subject));
                if subjects.len() >= crate::ui::ticker::MAX_HEADLINES {
                    break;
                }
            }
            Ok::<_, russh_bbs_core::forums::ForumError>(subjects)
        };
        match timeout(MEMBER_START_TIMEOUT, gather).await {
            Ok(Ok(subjects)) => self.parts.screen.set_ticker_headlines(&subjects, now),
            Ok(Err(error)) => {
                warn!(session = ?self.parts.session.id(), %error, "ticker headlines could not be read");
                self.parts.screen.set_ticker_headlines(&[], now);
            }
            Err(_) => {
                warn!(session = ?self.parts.session.id(), "ticker headlines timed out");
                self.parts.screen.set_ticker_headlines(&[], now);
            }
        }
    }

    /// Run the door request a screen is waiting on and give it the answer (M4-D0, D98): open a
    /// door, or write what it asked to save. `doors.run` is checked against the snapshot loaded
    /// for this action; a slow or failing store closes the door with a plain message.
    async fn run_doors(&mut self, request: crate::doors::Request) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let clock = self.parts.screen.door_clock();
        let response = crate::doors::run(&*self.parts.directory, &access, &clock, request).await;
        self.parts.screen.deliver_door(response);
    }

    /// Run the mail request a screen is waiting on and give it the answer (M3-I). Core checks
    /// every permission against the snapshot loaded for this action and does the sealing and
    /// opening with the board's mail key; a slow or failing store is a plain failure on screen.
    /// A send, a delete or a report asks for the inbox again, so the follow-up runs here with the
    /// same permissions (a few at most).
    async fn run_mail(&mut self, request: crate::mail::Request) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let keys = self.parts.screen.mail_keys();
        let mut request = request;
        for _ in 0..3 {
            let response =
                crate::mail::run(&*self.parts.directory, keys.as_deref(), &access, request).await;
            match self.parts.screen.deliver_mail(response) {
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
        // A password session proves its current password again to set or clear one (D81). That
        // is a password guess like any other, so it goes through the same gate (D105), keyed by
        // the account rather than a handle: this session already knows whose it is.
        let guess = match (&request.method, &request.kind) {
            (
                russh_bbs_core::passwords::LoginMethod::Password,
                russh_bbs_core::passwords::Kind::Set { .. }
                | russh_bbs_core::passwords::Kind::Clear { .. },
            ) => self
                .parts
                .account
                .map(|account| format!("account-{account}")),
            _ => None,
        };
        if let Some(key) = &guess
            && let russh_bbs_core::waiting_room::Verdict::Limited { retry_after } =
                self.parts.passwords.admit(self.parts.peer_ip, key)
        {
            info!(session = ?self.parts.session.id(), ?retry_after, "password re-entry rate limited");
            self.parts
                .screen
                .deliver_password(russh_bbs_core::passwords::Response::Refused(
                    russh_bbs_core::passwords::Refusal::WrongCurrent,
                ));
            return;
        }
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
        if let Some(key) = &guess {
            match response {
                russh_bbs_core::passwords::Response::Refused(
                    russh_bbs_core::passwords::Refusal::WrongCurrent,
                ) => self.parts.passwords.failed(self.parts.peer_ip, key),
                russh_bbs_core::passwords::Response::Done(_) => self.parts.passwords.succeeded(key),
                _ => {}
            }
        }
        if matches!(
            response,
            russh_bbs_core::passwords::Response::Done(
                russh_bbs_core::passwords::Done::Set | russh_bbs_core::passwords::Done::Cleared
            )
        ) {
            self.revoke_others(&Revocation::Password).await;
        }
        self.parts.screen.deliver_password(response);
    }

    /// Run the keys request the My keys or recovery screen is waiting on and give it the answer.
    /// Core checks the permission against the snapshot loaded for this action
    /// (`keys::execute`); a slow or failing store gets "failed".
    async fn run_keys(&mut self, request: russh_bbs_core::keys::Request) {
        let Some(access) = self.parts.screen.access().cloned() else {
            return;
        };
        let removing = match &request {
            russh_bbs_core::keys::Request::Remove(fingerprint) => Some(fingerprint.clone()),
            _ => None,
        };
        let response = timeout(
            PASSWORD_REQUEST_TIMEOUT,
            russh_bbs_core::keys::execute(&*self.parts.directory, &access, request),
        )
        .await
        .unwrap_or_else(|_| {
            warn!(session = ?self.parts.session.id(), "keys request timed out");
            russh_bbs_core::keys::Response::Refused(russh_bbs_core::keys::Refusal::Failed)
        });
        if let (Some(fingerprint), russh_bbs_core::keys::Response::Keys { changed, .. }) =
            (removing, &response)
            && *changed == Some(russh_bbs_core::keys::Change::Removed)
        {
            self.revoke_others(&Revocation::Key(fingerprint)).await;
        }
        self.parts.screen.deliver_keys(response);
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

/// What a new account is bound to: the presenting key, or the hash of a chosen password.
enum Bound<'a> {
    Key(&'a Fingerprint),
    /// The hash of the chosen password, and the visitor's key if they have one.
    Password(String, Option<&'a Fingerprint>),
}

/// The outcome of `open` registration as the room reads it.
fn made_by_open(opened: Result<russh_bbs_core::registration::Opened, OpenError>) -> Made {
    match opened {
        Ok(opened) => Made::Account(opened.account),
        Err(OpenError::CapReached) => Made::CapReached,
        Err(_) => Made::Refused,
    }
}
