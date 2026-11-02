//! The per-connection SSH handler: public-key auth, the one allowed PTY shell, and a refusal of
//! everything else.
//!
//! **Lockdown policy.** A session gets exactly one `session` channel, with a PTY and a shell.
//! Every other request is refused *explicitly*: russh sends no reply by default, which would
//! leave a client hanging, so each handler below answers. Refused: exec, subsystem (including
//! sftp, for now), env, agent forwarding, X11, direct-tcpip, forwarded-tcpip, streamlocal, and
//! both tcpip-forward and streamlocal-forward. Authentication is public-key only; the method
//! list advertised in `server::russh_config` says so, and the password and keyboard-interactive
//! handlers are the trait defaults, which reject.
//!
//! Logs carry fingerprints, request kinds and sizes. They never carry key material, command
//! lines, environment values or anything the client typed.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

use russh::server::{Auth, Handler, Msg, PublicKeyOffer, Session, VerifiedPublicKey};
use russh::{Channel, ChannelId, ChannelOpenFailure, Pty};
use russh_bbs_core::access::{Access, Fingerprint, decide_access};
use russh_bbs_core::founder::{ClaimForm, ClaimOutcome, MAX_ATTEMPTS_PER_SESSION, claim_founder};
use russh_bbs_core::store::{Account, AccountId, Role};
use russh_bbs_core::terminal_settings::{MemberFlow, MemberStart, TerminalSettings};
use russh_bbs_core::waiting_room::{RoomConfig, Verdict, WaitingRoom};
use tokio::sync::oneshot;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use super::Backend;
use super::error::SessionError;
use super::host_key::fingerprint_of;
use super::registry::SessionGuard;
use super::screen::{Content, Screen};
use super::terminal::{PtySize, parse_keys};
use crate::founder::{CLAIM_USER, Founder};
use crate::waiting_room::Throttle;

/// Session channels one connection may hold open at once. OpenSSH multiplexing and refused
/// probes need more than one; a stranger does not need many.
const MAX_SESSION_CHANNELS: usize = 4;

/// How long a new shell waits for the waiting-room settings before using the built-in
/// defaults. A slow store must not hold a stranger's screen hostage.
const ROOM_CONFIG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// How long a member's session start (settings and login time) may take before the shell
/// falls back to the waiting room. A slow store must not hang the screen.
const MEMBER_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

/// Handler for one client connection.
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
    screen: Option<Screen>,
    throttle: Arc<Throttle>,
    max_form_submissions: u32,
    /// The key this session authenticated with.
    fingerprint: Option<Fingerprint>,
    /// The role of the account behind the key, if it is a member.
    role: Option<Role>,
    /// Whether the SSH user name asked for the founder token prompt (`ssh claim@host`).
    claim_requested: bool,
    /// The account behind a signed-in session, so changed terminal settings can be stored.
    account: Option<AccountId>,
}

impl<D> ClientHandler<D> {
    pub(crate) fn new(
        directory: Arc<D>,
        founder: Arc<Founder>,
        peer: Option<SocketAddr>,
        guard: SessionGuard,
        authenticated: oneshot::Sender<()>,
        throttle: Arc<Throttle>,
        max_form_submissions: u32,
    ) -> Self {
        Self {
            directory,
            founder,
            peer,
            guard,
            authenticated: Some(authenticated),
            open_channels: HashSet::new(),
            shell_channel: None,
            pty: None,
            screen: None,
            throttle,
            max_form_submissions,
            fingerprint: None,
            role: None,
            claim_requested: false,
            account: None,
        }
    }

    fn is_shell_channel(&self, channel: ChannelId) -> bool {
        self.shell_channel == Some(channel)
    }

    /// Log a refused request by kind. Never log its payload (command, variable, path).
    fn refused(&self, kind: &'static str) {
        info!(session = ?self.guard.id(), peer = ?self.peer, kind, "request refused");
    }

    /// Build this session's waiting room from the board's settings. Any failure falls back to
    /// the strictest built-in configuration (`invite` mode, default text), never a looser one.
    async fn waiting_room(&self) -> WaitingRoom
    where
        D: Backend,
    {
        let config = match timeout(ROOM_CONFIG_TIMEOUT, self.directory.load_room_config()).await {
            Ok(Ok(config)) => config,
            Ok(Err(error)) => {
                warn!(session = ?self.guard.id(), %error, "waiting-room settings unreadable; using defaults");
                RoomConfig::default()
            }
            Err(_) => {
                warn!(session = ?self.guard.id(), "waiting-room settings timed out; using defaults");
                RoomConfig::default()
            }
        };
        WaitingRoom::new(&config, self.max_form_submissions)
    }

    /// The signed-in session for a member key, or `None` for everyone else.
    async fn member_content(&mut self) -> Option<Content>
    where
        D: Backend,
    {
        self.role?;
        let fingerprint = self.fingerprint.clone()?;
        let start = self.member_start(&fingerprint).await?;
        self.account = Some(start.account);
        Some(Content::Member(Box::new(MemberFlow::new(
            &start.handle,
            start.role,
            start.previous_login,
            start.settings,
        ))))
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

    /// The session start for a founder who just claimed. If the store fails right after the
    /// claim, the sysop still gets a working session on the defaults (nothing is saved).
    async fn founder_start(&self, account: &Account) -> MemberStart
    where
        D: Backend,
    {
        let started = timeout(
            MEMBER_START_TIMEOUT,
            self.directory.start_for_account(account),
        )
        .await;
        if let Ok(Ok(start)) = started {
            return start;
        }
        warn!(session = ?self.guard.id(), "founder session start failed; using defaults");
        MemberStart {
            account: account.id,
            handle: account.username.clone(),
            role: account.role,
            settings: TerminalSettings::default(),
            previous_login: None,
        }
    }

    /// Store settings the member changed. A failure is logged; the session keeps its choice.
    async fn persist_settings(&self, settings: &TerminalSettings)
    where
        D: Backend,
    {
        let Some(account) = self.account else {
            return;
        };
        if let Err(error) = self.directory.save_settings(account, settings).await {
            warn!(session = ?self.guard.id(), %error, "terminal settings could not be saved");
        }
    }

    /// Check a token typed at the claim prompt and update the screen. Every failure (limited,
    /// wrong token, already claimed, store error) is the same generic message.
    async fn try_claim(
        &mut self,
        channel: ChannelId,
        token: &str,
        session: &mut Session,
    ) -> Result<(), SessionError>
    where
        D: Backend,
    {
        let Some(fingerprint) = self.fingerprint.clone() else {
            return Ok(());
        };
        let peer_ip = self.peer.map(|peer| peer.ip());
        let outcome = if self
            .founder
            .admit_attempt(&fingerprint, peer_ip)
            .is_allowed()
        {
            claim_founder(self.founder.gate(), &*self.directory, token, &fingerprint).await
        } else {
            info!(session = ?self.guard.id(), fingerprint = %fingerprint, "founder claim rate limited");
            ClaimOutcome::Rejected
        };
        match outcome {
            ClaimOutcome::Claimed(account) => {
                self.role = Some(account.role);
                self.guard.set_authenticated(fingerprint, Access::Member);
                let start = self.founder_start(&account).await;
                self.account = Some(start.account);
                let flow = MemberFlow::new(
                    &start.handle,
                    start.role,
                    start.previous_login,
                    start.settings,
                );
                if let Some(screen) = self.screen.as_mut() {
                    session.data(channel, screen.claim_accepted(flow)?)?;
                }
            }
            ClaimOutcome::Rejected => {
                let Some(screen) = self.screen.as_mut() else {
                    return Ok(());
                };
                let (bytes, disconnect) = screen.claim_rejected()?;
                session.data(channel, bytes)?;
                if disconnect {
                    self.finish(channel, session)?;
                }
            }
        }
        Ok(())
    }

    /// End the shell: restore the client's terminal, then close the channel.
    fn finish(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), SessionError> {
        if let Some(mut screen) = self.screen.take() {
            session.data(channel, screen.leave()?)?;
        }
        self.guard.set_shell_open(false);
        session.exit_status_request(channel, 0)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }
}

#[expect(
    clippy::unused_async_trait_impl,
    reason = "russh's Handler methods return futures; most refusals resolve immediately"
)]
impl<D: Backend> Handler for ClientHandler<D> {
    type Error = SessionError;

    // `none`, `password` and `keyboard-interactive` keep the trait defaults, which reject.

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
        let access = decide_access(&*self.directory, &fingerprint).await;
        info!(
            session = ?self.guard.id(),
            peer = ?self.peer,
            fingerprint = %fingerprint,
            access = ?access,
            "public key authenticated"
        );
        // The user name only selects the claim prompt. It is never rendered or logged.
        self.claim_requested = user == CLAIM_USER;
        self.role = match access {
            Access::Member => self.directory.role_of(&fingerprint).await,
            Access::Restricted => None,
        };
        self.guard.set_authenticated(fingerprint.clone(), access);
        self.fingerprint = Some(fingerprint);
        // Both levels authenticate. `Restricted` limits the session to the waiting room;
        // `Member` gets the signed-in placeholder (the member UI is later work).
        match access {
            Access::Restricted | Access::Member => Ok(Auth::Accept),
        }
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
        if self.screen.is_some() {
            self.refused("second-shell");
            session.channel_failure(channel)?;
            return Ok(());
        }
        session.channel_success(channel)?;
        let content = if self.claim_requested {
            // The same prompt for every key on every board, claimed or not (D41).
            Content::Claim(ClaimForm::new(MAX_ATTEMPTS_PER_SESSION))
        } else if let Some(start) = self.member_content().await {
            start
        } else {
            Content::Room(self.waiting_room().await)
        };
        let mut screen = Screen::new(size, content)?;
        session.data(channel, screen.enter()?)?;
        self.screen = Some(screen);
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
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.is_shell_channel(channel) {
            return Ok(());
        }
        let size = PtySize::from_request(col_width, row_height);
        self.pty = Some(size);
        self.guard.set_size(size);
        if let Some(screen) = self.screen.as_mut() {
            session.data(channel, screen.resize(size)?)?;
        }
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.is_shell_channel(channel) {
            return Ok(());
        }
        let Some(screen) = self.screen.as_mut() else {
            return Ok(());
        };
        let update = screen.keys(&parse_keys(data))?;
        session.data(channel, update.bytes)?;
        if let Some(settings) = &update.save_settings {
            self.persist_settings(settings).await;
        }
        if update.disconnect {
            self.finish(channel, session)?;
        } else if let Some(token) = update.claim_token {
            self.try_claim(channel, &token, session).await?;
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.is_shell_channel(channel) {
            self.finish(channel, session)?;
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
            self.screen = None;
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
