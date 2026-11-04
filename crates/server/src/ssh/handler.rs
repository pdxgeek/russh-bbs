//! The per-connection SSH handler: public-key auth, the one allowed PTY shell, and a refusal of
//! everything else.
//!
//! **Lockdown policy.** A session gets exactly one `session` channel, with a PTY and a shell.
//! Every other request is refused *explicitly*: russh sends no reply by default, which would
//! leave a client hanging, so each handler below answers. Refused: exec, subsystem (including
//! sftp, for now), env, agent forwarding, X11, direct-tcpip, forwarded-tcpip, streamlocal, and
//! both tcpip-forward and streamlocal-forward. Authentication is public-key, plus `password` only
//! when the sysop opted in (D28); the method list advertised in `server::russh_config` says so.
//! Keyboard-interactive is the trait default, which rejects (D81).
//!
//! Logs carry fingerprints, request kinds and sizes. They never carry key material, command
//! lines, environment values or anything the client typed.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use russh::server::{Auth, Handler, Msg, PublicKeyOffer, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId, ChannelOpenFailure, MethodKind, MethodSet, Pty};
use russh_bbs_core::access::{Access, Fingerprint};
use russh_bbs_core::founder::{ClaimForm, MAX_ATTEMPTS_PER_SESSION};
use russh_bbs_core::limits::{LimitError, MemberMeter, SystemClock, begin_member_session};
use russh_bbs_core::passwords::{Login, authenticate};
use russh_bbs_core::signin::{Route, names_the_handle, route};
use russh_bbs_core::store::{Account, AccountId};
use russh_bbs_core::terminal_settings::{MemberFlow, MemberStart};
use russh_bbs_core::waiting_room::{Verdict, WaitingRoom};
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use super::Backend;
use super::abort::Abort;
use super::budget::SessionBudget;
use super::error::SessionError;
use super::host_key::fingerprint_of;
use super::limits::Limits;
use super::password::PasswordLogin;
use super::registry::SessionGuard;
use super::room::{build_room, room_allowed};
use super::screen::{Content, Screen};
use super::shell::{Inbox, MEMBER_START_TIMEOUT, Output, Shell, ShellParts};
use super::terminal::PtySize;
use crate::chat::Hub;
use crate::founder::Founder;
use crate::ui::Ui;
use crate::waiting_room::Throttle;

/// Session channels one connection may hold open at once. OpenSSH multiplexing and refused
/// probes need more than one; a stranger does not need many.
const MAX_SESSION_CHANNELS: usize = 4;

/// How long one password check may take, queueing for a hashing turn included. Inside the
/// 30 s authentication deadline; a busy or failing store refuses the login instead of holding it.
const PASSWORD_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// What a connection's handler is built from.
pub(crate) struct HandlerParts<D> {
    pub(crate) directory: Arc<D>,
    pub(crate) founder: Arc<Founder>,
    pub(crate) throttle: Arc<Throttle>,
    /// SSH password login: the switch, its limiters and the hashing pool (M3-D).
    pub(crate) passwords: Arc<PasswordLogin>,
    /// The chat hub (M3-F).
    pub(crate) chat: Arc<Hub>,
    pub(crate) limits: Limits,
    /// The validated string table and menu tree (M2-A).
    pub(crate) ui: Arc<Ui>,
    /// Fired by the session's own tasks to have the connection dropped.
    pub(crate) abort: Arc<Abort>,
}

/// Handler for one client connection.
///
/// It receives and refuses; the work a shell session causes (parsing keys, drawing, the founder
/// claim) is done by the session's [`Shell`] task, which the handler feeds through an [`Inbox`]
/// and a per-session budget (D49).
pub(crate) struct ClientHandler<D> {
    directory: Arc<D>,
    founder: Arc<Founder>,
    peer: Option<SocketAddr>,
    guard: SessionGuard,
    /// Fired once when authentication succeeds, to cancel the auth deadline.
    authenticated: Option<oneshot::Sender<()>>,
    /// Open session channels (capped). Most are probes that only ever get refusals.
    open_channels: HashSet<ChannelId>,
    /// The one channel that was granted a PTY and may run the shell.
    shell_channel: Option<ChannelId>,
    pty: Option<PtySize>,
    /// Whether the shell task has been started on `shell_channel`.
    shell_started: bool,
    throttle: Arc<Throttle>,
    limits: Limits,
    ui: Arc<Ui>,
    inbox: Arc<Inbox>,
    budget: SessionBudget,
    /// Input bytes dropped as last reported to the registry.
    reported_dropped: u64,
    abort: Arc<Abort>,
    /// The key this session authenticated with.
    fingerprint: Option<Fingerprint>,
    /// Whether the key is bound to an active account (a member).
    member: bool,
    /// Whether the SSH user name asked for the founder token prompt (`ssh claim@host`).
    claim_requested: bool,
    /// The account behind a signed-in session, so changed terminal settings can be stored.
    account: Option<AccountId>,
    passwords: Arc<PasswordLogin>,
    chat: Arc<Hub>,
    /// The account a password login matched. Set only by `auth_password`; such a session has no
    /// key, and starts from this account instead of a fingerprint.
    password_account: Option<Account>,
}

impl<D> ClientHandler<D> {
    pub(crate) fn new(
        parts: HandlerParts<D>,
        peer: Option<SocketAddr>,
        guard: SessionGuard,
        authenticated: oneshot::Sender<()>,
    ) -> Self {
        let HandlerParts {
            directory,
            founder,
            throttle,
            passwords,
            chat,
            limits,
            ui,
            abort,
        } = parts;
        Self {
            directory,
            founder,
            peer,
            guard,
            authenticated: Some(authenticated),
            open_channels: HashSet::new(),
            shell_channel: None,
            pty: None,
            shell_started: false,
            throttle,
            budget: SessionBudget::new(&limits, Instant::now()),
            limits,
            ui,
            inbox: Arc::new(Inbox::default()),
            reported_dropped: 0,
            abort,
            fingerprint: None,
            member: false,
            claim_requested: false,
            account: None,
            passwords,
            chat,
            password_account: None,
        }
    }

    fn is_shell_channel(&self, channel: ChannelId) -> bool {
        self.shell_channel == Some(channel)
    }

    /// Log a refused request by kind. Never log its payload (command, variable, path).
    fn refused(&self, kind: &'static str) {
        info!(session = ?self.guard.id(), peer = ?self.peer, kind, "request refused");
    }

    /// Build this session's waiting room from the board's settings (see [`build_room`]).
    async fn waiting_room(&self) -> WaitingRoom
    where
        D: Backend,
    {
        // Every join form asks for a password, with a key or without one (D94 (6)); a visitor
        // with a key can also bind it to an account they already have (D94 (8)).
        let room = build_room(
            &*self.directory,
            self.limits.max_form_submissions,
            &self.guard.id(),
        )
        .await
        .with_password_signup();
        if self.fingerprint.is_some() {
            room.with_key_link()
        } else {
            room
        }
    }

    /// Whether the waiting room may be shown (see [`room_allowed`]).
    async fn waiting_room_allowed(&self) -> bool
    where
        D: Backend,
    {
        room_allowed(&*self.directory, &self.guard.id()).await
    }

    /// The signed-in session for a member key, or `None` for everyone else. Core's limits
    /// decide whether the session may start (concurrent sessions, calls and minutes per day,
    /// M2-H): a refusal becomes a screen that says which limit, then ends. If the limits cannot
    /// be checked the member path fails closed, like every other store failure here.
    async fn member_content(&mut self) -> Option<(Content, Option<MemberMeter>)>
    where
        D: Backend,
    {
        if !self.member {
            return None;
        }
        let start = if let Some(account) = self.password_account.clone() {
            self.password_start(&account).await?
        } else {
            let fingerprint = self.fingerprint.clone()?;
            self.member_start(&fingerprint).await?
        };
        let begun = timeout(
            MEMBER_START_TIMEOUT,
            begin_member_session(
                &*self.directory,
                &SystemClock,
                &self.guard.reference().member_sessions(),
                &start.access,
            ),
        )
        .await;
        let meter = match begun {
            Ok(Ok(meter)) => meter,
            Ok(Err(LimitError::Refused(refusal))) => {
                info!(session = ?self.guard.id(), limit = refusal.kind.name(), "session refused by a limit");
                return Some((Content::Limited(refusal.lines()), None));
            }
            Ok(Err(error)) => {
                warn!(session = ?self.guard.id(), %error, "limits could not be checked");
                return None;
            }
            Err(_) => {
                warn!(session = ?self.guard.id(), "limit check timed out");
                return None;
            }
        };
        // Last callers: the one place a member session starts (the limits let it in). Neither
        // the waiting room nor a founder-claim session comes through here (M3-J).
        crate::community::note_sign_in(&*self.directory, start.account).await;
        self.account = Some(start.account);
        self.guard.set_account(start.account);
        let mut flow = MemberFlow::new(
            &start.handle,
            start.access,
            start.previous_login,
            start.settings,
        );
        if self.password_account.is_some() {
            flow = flow.signed_in_with_password();
        }
        // Key changes since the last sign-in (FIX-M3-11), told once.
        let notices =
            russh_bbs_core::keys::notices_for_sign_in(&*self.directory, start.account).await;
        flow = flow.with_key_notices(russh_bbs_core::keys::notice_lines(&notices));
        Some((Content::Member(Box::new(flow)), Some(meter)))
    }

    /// Start a signed-in session for the account behind this session's key. Any failure (no
    /// account, a store error, a timeout) is logged and answers `None`, which sends the shell
    /// back to the waiting room: the member path fails closed, like the directory.
    async fn member_start(&self, fingerprint: &Fingerprint) -> Option<MemberStart>
    where
        D: Backend,
    {
        match timeout(
            MEMBER_START_TIMEOUT,
            self.directory.start_for_key(fingerprint),
        )
        .await
        {
            Ok(Ok(start)) => start,
            Ok(Err(error)) => {
                warn!(session = ?self.guard.id(), %error, "member session could not start");
                None
            }
            Err(_) => {
                warn!(session = ?self.guard.id(), "member session start timed out");
                None
            }
        }
    }

    /// Start the signed-in session for an account a password login just matched. The account
    /// is read again as the session starts (its role and state may have changed since the
    /// password was checked): one that is no longer an active member gets nothing, and the
    /// session ends instead of falling back to a waiting room it has no key for.
    async fn password_start(&self, account: &Account) -> Option<MemberStart>
    where
        D: Backend,
    {
        let started = timeout(
            MEMBER_START_TIMEOUT,
            self.directory.start_for_account(account),
        )
        .await;
        match started {
            Ok(Ok(start)) if start.access.account() == Some(account.id) => Some(start),
            Ok(Ok(_)) => {
                info!(session = ?self.guard.id(), "password session no longer holds member access");
                None
            }
            Ok(Err(error)) => {
                warn!(session = ?self.guard.id(), %error, "password session could not start");
                None
            }
            Err(_) => {
                warn!(session = ?self.guard.id(), "password session start timed out");
                None
            }
        }
    }

    /// Drop the connection if this session has pushed past its budgets (D49). Nothing it sent
    /// is logged, only the fact.
    fn check_budget(&self) -> Result<(), SessionError> {
        if self.budget.exceeded() {
            info!(
                session = ?self.guard.id(),
                peer = ?self.peer,
                dropped_input_bytes = self.budget.dropped_bytes(),
                "session is over its request budget; disconnecting"
            );
            return Err(SessionError::OverBudget);
        }
        Ok(())
    }
}

impl<D> Drop for ClientHandler<D> {
    fn drop(&mut self) {
        // The connection is over: tell the shell task to stop without sending anything.
        self.inbox.close();
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "russh's Handler methods return futures; most refusals resolve immediately"
)]
impl<D: Backend> Handler for ClientHandler<D> {
    type Error = SessionError;

    // `none` and `keyboard-interactive` keep the trait defaults, which reject. `password` is
    // answered below, and only when the sysop opted in (D28).

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        // The method is not advertised when off, so a client has no reason to try it; one that
        // does anyway is refused without a single hash.
        // `new` and `claim` are not accounts, and there is nothing to check: a client with no
        // key to offer reaches the waiting room (or the founder claim) by answering the
        // password prompt with anything, even nothing (D96). OpenSSH tries `none` first and
        // would never offer a key if that succeeded, so `none` is refused and this is how a
        // key-less visitor gets in. Nothing typed is read, hashed or logged.
        if route(user) != Route::SignIn {
            self.claim_requested = route(user) == Route::Claim;
            self.member = false;
            self.fingerprint = None;
            self.guard.set_visitor();
            return Ok(Auth::Accept);
        }
        // Signing in by password can be turned off; the visitor path above is not a sign-in.
        if !self.passwords.enabled() {
            return Ok(retry_methods(false));
        }
        // Limits first, for every handle alike (known or not), so a limited reply is the same
        // plain refusal and says nothing about members. Nothing typed is logged.
        if let Verdict::Limited { retry_after } =
            self.passwords.admit(self.peer.map(|peer| peer.ip()), user)
        {
            info!(
                session = ?self.guard.id(),
                peer = ?self.peer,
                ?retry_after,
                "password rate limit; rejecting"
            );
            return Ok(retry_methods(self.passwords.enabled()));
        }
        let checked = timeout(
            PASSWORD_CHECK_TIMEOUT,
            authenticate(&*self.directory, self.passwords.pool(), user, password),
        )
        .await;
        let peer_ip = self.peer.map(|peer| peer.ip());
        match checked {
            Ok(Ok(Login::Accepted(account))) => {
                self.passwords.succeeded(user);
                info!(
                    session = ?self.guard.id(),
                    peer = ?self.peer,
                    account = %account.id,
                    "password authenticated"
                );
                self.claim_requested = false;
                self.member = true;
                self.guard.set_authenticated_by_password(Access::Member);
                self.password_account = Some(account);
                Ok(Auth::Accept)
            }
            Ok(Ok(Login::Denied)) => {
                self.passwords.failed(peer_ip, user);
                info!(session = ?self.guard.id(), peer = ?self.peer, "password rejected");
                Ok(retry_methods(self.passwords.enabled()))
            }
            Ok(Err(error)) => {
                warn!(session = ?self.guard.id(), %error, "password check failed in the store; rejecting");
                Ok(retry_methods(self.passwords.enabled()))
            }
            Err(_) => {
                warn!(session = ?self.guard.id(), "password check timed out; rejecting");
                Ok(retry_methods(self.passwords.enabled()))
            }
        }
    }

    async fn auth_publickey_offered(
        &mut self,
        _user: &str,
        _offer: PublicKeyOffer<'_>,
    ) -> Result<Auth, Self::Error> {
        // Accepting the offer only tells the client to send a signature. Membership is decided
        // after the signature is verified, in `auth_publickey`.
        Ok(Auth::Accept)
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        let Ok(fingerprint) = fingerprint_of(key.public_key) else {
            warn!(session = ?self.guard.id(), "could not fingerprint client key; rejecting");
            return Ok(Auth::reject());
        };
        // Per-key limit, before the membership lookup and for every key alike, so a throttled
        // reply cannot tell a member from a stranger. The client sees an ordinary refusal.
        if let Verdict::Limited { retry_after } = self.throttle.admit_key(&fingerprint) {
            info!(
                session = ?self.guard.id(),
                peer = ?self.peer,
                fingerprint = %fingerprint,
                ?retry_after,
                "key rate limit; rejecting"
            );
            return Ok(Auth::reject());
        }
        // The user name picks the path (D94 (10)); it is never rendered or logged.
        match route(user) {
            Route::Join | Route::Claim => {
                // The waiting room, with any key: a member's key too (it can join nothing
                // new, and the room treats every visitor alike).
                self.claim_requested = route(user) == Route::Claim;
                self.member = false;
                info!(session = ?self.guard.id(), peer = ?self.peer, fingerprint = %fingerprint, "public key authenticated for the waiting room");
                self.guard
                    .set_authenticated(fingerprint.clone(), Access::Restricted);
                self.fingerprint = Some(fingerprint);
                Ok(Auth::Accept)
            }
            Route::SignIn => {
                // Only a key bound to the account with this very handle signs in. Every other
                // key is refused the same way, so the client falls back to a password and the
                // answer is the same whether or not the handle exists.
                let bound = self
                    .member_start(&fingerprint)
                    .await
                    .is_some_and(|start| names_the_handle(user, start.handle.as_str()));
                if !bound {
                    info!(session = ?self.guard.id(), peer = ?self.peer, "public key not bound to that handle; refusing");
                    return Ok(retry_methods(self.passwords.enabled()));
                }
                info!(session = ?self.guard.id(), peer = ?self.peer, fingerprint = %fingerprint, "public key authenticated");
                // When the key last signed in, for My keys (FIX-M3-11). A failure is only logged.
                if let Err(error) = self.directory.note_key_use(&fingerprint).await {
                    warn!(session = ?self.guard.id(), %error, "key use could not be recorded");
                }
                self.claim_requested = false;
                self.member = true;
                self.guard
                    .set_authenticated(fingerprint.clone(), Access::Member);
                self.fingerprint = Some(fingerprint);
                Ok(Auth::Accept)
            }
        }
    }

    async fn auth_none(&mut self, _user: &str) -> Result<Auth, Self::Error> {
        // Never accepted: a client that succeeds here is never asked for its key (OpenSSH sends
        // `none` first), so a visitor could not present the key to bind or link (D96).
        Ok(retry_methods(self.passwords.enabled()))
    }

    async fn auth_succeeded(&mut self, _session: &mut Session) -> Result<(), Self::Error> {
        if let Some(signal) = self.authenticated.take() {
            // The receiver is gone only when the connection is already being torn down.
            let _ = signal.send(());
        }
        Ok(())
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.open_channels.len() >= MAX_SESSION_CHANNELS {
            self.refused("too-many-session-channels");
            reply.reject(ChannelOpenFailure::ResourceShortage).await;
            return Ok(());
        }
        self.open_channels.insert(channel.id());
        reply.accept().await;
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        _channel: Channel<Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.refused("direct-tcpip");
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }

    async fn channel_open_forwarded_tcpip(
        &mut self,
        _channel: Channel<Msg>,
        _host_to_connect: &str,
        _port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.refused("forwarded-tcpip");
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }

    async fn channel_open_direct_streamlocal(
        &mut self,
        _channel: Channel<Msg>,
        _socket_path: &str,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.refused("direct-streamlocal");
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }

    async fn channel_open_x11(
        &mut self,
        _channel: Channel<Msg>,
        _originator_address: &str,
        _originator_port: u32,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.refused("x11-channel");
        reply
            .reject(ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.open_channels.contains(&channel) || self.shell_channel.is_some() {
            self.refused("pty-req");
            session.channel_failure(channel)?;
            return Ok(());
        }
        let size = PtySize::from_request(col_width, row_height);
        self.pty = Some(size);
        self.shell_channel = Some(channel);
        self.guard.set_size(size);
        debug!(session = ?self.guard.id(), cols = size.cols(), rows = size.rows(), "pty granted");
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // A shell needs a PTY (`ssh -T` is refused) and may start only once.
        let Some(size) = self.pty.filter(|_| self.is_shell_channel(channel)) else {
            self.refused("shell-without-pty");
            session.channel_failure(channel)?;
            return Ok(());
        };
        if self.shell_started {
            self.refused("second-shell");
            session.channel_failure(channel)?;
            return Ok(());
        }
        let mut meter = None;
        let content = if self.claim_requested {
            // The same prompt for every key on every board, claimed or not (D41). The setup
            // token is its gate; there is no account yet to hold a permission.
            Content::Claim(ClaimForm::new(MAX_ATTEMPTS_PER_SESSION).with_password())
        } else if let Some((content, metered)) = self.member_content().await {
            meter = metered;
            content
        } else if self.password_account.is_some() {
            // A password login has no key to show the waiting room to. If the member session
            // cannot start, the shell is refused; nothing looser is offered.
            self.refused("no-access");
            session.channel_failure(channel)?;
            return Ok(());
        } else if self.waiting_room_allowed().await {
            Content::Room(self.waiting_room().await)
        } else {
            // `waiting.view` could not be confirmed (the store failed, or the permission was
            // removed): the session gets nothing, not a looser screen.
            self.refused("no-access");
            session.channel_failure(channel)?;
            return Ok(());
        };
        session.channel_success(channel)?;
        // The window may have changed while the content was loading.
        let size = self.pty.unwrap_or(size);
        let mut screen = Screen::new(size, self.limits.render_cap, content, Arc::clone(&self.ui))?;
        screen.set_low_security(self.limits.low_security);
        let shell = Shell::new(ShellParts {
            output: Output::Ssh {
                handle: session.handle(),
                channel,
            },
            screen,
            inbox: Arc::clone(&self.inbox),
            directory: Arc::clone(&self.directory),
            founder: Arc::clone(&self.founder),
            session: self.guard.reference(),
            abort: Arc::clone(&self.abort),
            fingerprint: self.fingerprint.clone(),
            peer_ip: self.peer.map(|peer| peer.ip()),
            account: self.account,
            passwords: Arc::clone(&self.passwords),
            chat: Arc::clone(&self.chat),
            meter,
            redraw_interval: self.limits.redraw_interval,
            write_stall: self.limits.write_stall,
            access_filter: None,
        });
        tokio::spawn(shell.run());
        self.shell_started = true;
        self.guard.set_shell_open(true);
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.is_shell_channel(channel) {
            return Ok(());
        }
        let size = PtySize::from_request(col_width, row_height);
        self.pty = Some(size);
        self.guard.set_size(size);
        // Over budget or not, only the latest size is kept, so the trailing size always takes
        // effect and a flood costs a store and a wake-up, not a redraw.
        let _ = self.budget.request(Instant::now());
        if self.shell_started {
            self.inbox.set_resize(size);
        }
        self.check_budget()
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.is_shell_channel(channel) || !self.shell_started {
            return Ok(());
        }
        let now = Instant::now();
        if self.budget.request(now) {
            let allowed = self.budget.input(now, data.len());
            if let Some(part) = data.get(..allowed) {
                self.inbox.push_input(part);
            }
            let dropped = self.budget.dropped_bytes();
            if dropped != self.reported_dropped {
                self.reported_dropped = dropped;
                self.guard.set_input_dropped(dropped);
            }
        }
        self.check_budget()
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.is_shell_channel(channel) && self.shell_started {
            // The shell restores the client's terminal and closes the channel.
            self.inbox.finish();
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.open_channels.remove(&channel);
        if self.is_shell_channel(channel) {
            // The client closed first; there is no terminal left to restore.
            self.inbox.close();
            self.inbox = Arc::new(Inbox::default());
            self.shell_started = false;
            self.shell_channel = None;
            self.pty = None;
            self.guard.set_shell_open(false);
        }
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        _data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.refused("exec");
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn subsystem_request(
        &mut self,
        channel: ChannelId,
        _name: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Includes `sftp`: SFTP arrives in M4 with its own virtual filesystem.
        self.refused("subsystem");
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn env_request(
        &mut self,
        channel: ChannelId,
        _variable_name: &str,
        _variable_value: &str,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        debug!(session = ?self.guard.id(), "env request refused");
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn x11_request(
        &mut self,
        channel: ChannelId,
        _single_connection: bool,
        _x11_auth_protocol: &str,
        _x11_auth_cookie: &str,
        _x11_screen_number: u32,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.refused("x11-req");
        session.channel_failure(channel)?;
        Ok(())
    }

    async fn agent_request(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        // `false` makes russh send the channel failure reply.
        self.refused("agent-forwarding");
        Ok(false)
    }

    async fn tcpip_forward(
        &mut self,
        _address: &str,
        _port: &mut u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        self.refused("tcpip-forward");
        Ok(false)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        _address: &str,
        _port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    async fn streamlocal_forward(
        &mut self,
        _socket_path: &str,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        self.refused("streamlocal-forward");
        Ok(false)
    }

    async fn cancel_streamlocal_forward(
        &mut self,
        _socket_path: &str,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }
}

/// The reply to a refused password. A bare `Auth::reject()` makes russh drop `password` from
/// the methods the client is told remain, so the client gives up after one try (QA M3 L9). This
/// keeps both methods on offer; russh's `max_auth_attempts` still ends the connection, and the
/// per-account and per-IP limits (D81) are checked before every try.
fn retry_methods(_password: bool) -> Auth {
    // `password` is always on offer: it is also how a key-less visitor reaches the waiting
    // room under `new` and `claim`; a sign-in by password is refused when the sysop turned it off.
    Auth::Reject {
        proceed_with_methods: Some(MethodSet::from(
            &[MethodKind::PublicKey, MethodKind::Password][..],
        )),
        partial_success: false,
    }
}
