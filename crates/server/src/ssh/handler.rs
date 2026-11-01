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
use russh_bbs_core::access::{Access, MemberDirectory, decide_access};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use super::error::SessionError;
use super::host_key::fingerprint_of;
use super::registry::SessionGuard;
use super::screen::Screen;
use super::terminal::{Key, PtySize, parse_keys};

/// Session channels one connection may hold open at once. OpenSSH multiplexing and refused
/// probes need more than one; a stranger does not need many.
const MAX_SESSION_CHANNELS: usize = 4;

/// Handler for one client connection.
pub(crate) struct ClientHandler<D> {
    directory: Arc<D>,
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
}

impl<D> ClientHandler<D> {
    pub(crate) fn new(
        directory: Arc<D>,
        peer: Option<SocketAddr>,
        guard: SessionGuard,
        authenticated: oneshot::Sender<()>,
    ) -> Self {
        Self {
            directory,
            peer,
            guard,
            authenticated: Some(authenticated),
            open_channels: HashSet::new(),
            shell_channel: None,
            pty: None,
            screen: None,
        }
    }

    fn is_shell_channel(&self, channel: ChannelId) -> bool {
        self.shell_channel == Some(channel)
    }

    /// Log a refused request by kind. Never log its payload (command, variable, path).
    fn refused(&self, kind: &'static str) {
        info!(session = ?self.guard.id(), peer = ?self.peer, kind, "request refused");
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
impl<D: MemberDirectory + 'static> Handler for ClientHandler<D> {
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
        _user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        // M1-C: per-key rate limiting plugs in here, before the decision.
        let Ok(fingerprint) = fingerprint_of(key.public_key) else {
            warn!(session = ?self.guard.id(), "could not fingerprint client key; rejecting");
            return Ok(Auth::reject());
        };
        let access = decide_access(&*self.directory, &fingerprint).await;
        info!(
            session = ?self.guard.id(),
            peer = ?self.peer,
            fingerprint = %fingerprint,
            access = ?access,
            "public key authenticated"
        );
        self.guard.set_authenticated(fingerprint, access);
        // Both levels authenticate. `Restricted` limits what the session may do, and in this
        // slice that is the placeholder screen for everyone.
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
        let mut screen = Screen::new(size)?;
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
        if !self.is_shell_channel(channel) || self.screen.is_none() {
            return Ok(());
        }
        if parse_keys(data).into_iter().any(Key::is_quit) {
            self.finish(channel, session)?;
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
