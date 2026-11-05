//! One telnet connection: negotiate, sign in, then run the shared shell.
//!
//! The tasks mirror SSH's. The connection task reads the socket, decodes the protocol and hands
//! the shell its input and window size through the shell's [`Inbox`]; the shell task draws and
//! sends frames through [`Output::Frames`]; a writer task owns the socket's write half and
//! encodes what both of them send. A peer that stops reading fills the frame queue, the shell's
//! write timeout fires, and the shell asks for the connection to be dropped (D49).

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use russh_bbs_core::access::Access;
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::limits::{LimitError, SystemClock, begin_member_session};
use russh_bbs_core::terminal_settings::MemberFlow;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout};
use tracing::{debug, info, warn};

use super::Slot;
use super::login::Entry;
use super::protocol::{Decoded, Decoder, OPENING, encode};
use super::staff::without_staff;
use crate::ssh::abort::Abort;
use crate::ssh::budget::SessionBudget;
use crate::ssh::password::PasswordLogin;
use crate::ssh::registry::{SessionRef, SessionRegistry};
use crate::ssh::room::{build_room, room_allowed};
use crate::ssh::screen::{Content, Screen};
use crate::ssh::shell::{Inbox, MEMBER_START_TIMEOUT, Output, Shell, ShellParts};
use crate::ssh::terminal::{KeyParser, PtySize};
use crate::ssh::{Backend, Limits};
use crate::ui::Ui;

/// How long a connection may take to wind down before we stop waiting for it.
const DISCONNECT_GRACE: Duration = Duration::from_secs(5);

/// How long to wait for the client's first window size before drawing the login prompt. A
/// client answers `DO NAWS` within a round trip; one that never does gets 80x24.
const FIRST_WINDOW_WAIT: Duration = Duration::from_millis(250);

/// Bytes read from the socket at once. Small on purpose: the decoder and the budget see the
/// input in bounded pieces.
const READ_CHUNK: usize = 1024;

/// Frames queued for the writer. A shell frame is one redraw, so a few are plenty.
const FRAME_QUEUE: usize = 8;
/// Wire-ready messages queued for the writer (prompts and protocol refusals).
const RAW_QUEUE: usize = 32;

/// Smallest window honored. The last row is the transport's, and the screens need a few more
/// than that; a client reporting less is given these.
const MIN_COLS: u16 = 40;
const MIN_ROWS: u16 = 10;
/// Size used when the client reports none.
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// How long one password check may take, queueing for a hashing turn included.
pub(super) const PASSWORD_CHECK_TIMEOUT: Duration = Duration::from_secs(10);

/// The marker every telnet screen carries.
pub(super) const TRANSPORT_MARKER: SecurityMarker = SecurityMarker::Unencrypted;

/// One accepted connection and what it needs to run.
pub(super) struct Connection<D> {
    pub(super) directory: Arc<D>,
    pub(super) registry: Arc<SessionRegistry>,
    pub(super) passwords: Arc<PasswordLogin>,
    pub(super) limits: Limits,
    pub(super) allow_login: bool,
    pub(super) ui: Arc<Ui>,
    /// Released when the connection ends, freeing a slot under `max_connections`.
    pub(super) _permit: Slot,
}

/// Stops the shell when the connection ends in any way, so it settles its meter and frees its
/// session slot (the SSH handler does the same in its `Drop`).
struct CloseInbox(Arc<Inbox>);

impl Drop for CloseInbox {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// Aborts a task when dropped.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl<D: Backend> Connection<D> {
    pub(super) async fn serve(self, stream: TcpStream, peer: SocketAddr) {
        let guard = self.registry.register(Some(peer));
        // Small writes (keystroke echoes) must not wait for a full segment.
        if let Err(error) = stream.set_nodelay(true) {
            debug!(%peer, %error, "could not set TCP_NODELAY");
        }
        let (read, write) = stream.into_split();
        let (raw_tx, raw_rx) = mpsc::channel(RAW_QUEUE);
        let (frames_tx, frames_rx) = mpsc::channel(FRAME_QUEUE);
        let mut writer = AbortOnDrop(tokio::spawn(write_loop(write, raw_rx, frames_rx)));
        let mut link = Link::new(read, raw_tx, &self.limits, guard.reference());
        let started = self.sign_in(&mut link, peer).await;
        if let Some(started) = started {
            self.run_shell(link, frames_tx, started, &guard.reference(), peer)
                .await;
        } else {
            drop(link);
            drop(frames_tx);
        }
        // Let the writer flush the last frame, then close the socket.
        if timeout(DISCONNECT_GRACE, &mut writer.0).await.is_err() {
            debug!(%peer, "telnet writer did not finish; dropping the connection");
        }
        debug!(%peer, "telnet connection closed");
    }

    /// Negotiate and sign in. `None` means the connection ends (a refusal was shown, the client
    /// left or the time ran out).
    async fn sign_in(&self, link: &mut Link, peer: SocketAddr) -> Option<Started> {
        link.send_wire(OPENING.to_vec()).await;
        // Learn the window before drawing, so the marker lands on the real last row.
        let _ = timeout(FIRST_WINDOW_WAIT, link.wait_for_window()).await;
        if !self.allow_login {
            link.screen_text(&[
                "Telnet login is not enabled on this board.",
                "Connect with ssh instead.",
            ])
            .await;
            return None;
        }
        let deadline = Instant::now() + self.limits.auth_timeout;
        let entry = match tokio::time::timeout_at(deadline, self.login(link, peer)).await {
            Ok(Some(entry)) => entry,
            Ok(None) => return None,
            Err(_) => {
                info!(%peer, "telnet login timed out; disconnecting");
                link.screen_text(&["Login timed out."]).await;
                return None;
            }
        };
        match entry {
            Entry::Member(account) => self.start_member(link, account).await,
            Entry::Join => self.start_room(link).await,
        }
    }

    /// The waiting room for a visitor who typed `new` at the login prompt (D94): the same room
    /// the ssh visitor gets, in every registration mode, with the sign-up forms asking for a
    /// password because there is no key to bind. It reveals nothing about the board; it is the
    /// room's own rule, and this path adds nothing to it. The authentication deadline does not
    /// run here: filling in a form is not a login attempt.
    async fn start_room(&self, link: &mut Link) -> Option<Started> {
        let session = link.session.id();
        if !room_allowed(&*self.directory, &session).await {
            // Nothing is offered that could not be confirmed (the store failed, or the
            // permission was removed): the visitor gets the refusal every failure gets.
            link.screen_text(&["Your session could not be started."])
                .await;
            return None;
        }
        let room = build_room(&*self.directory, self.limits.max_form_submissions, &session)
            .await
            .with_password_signup();
        Some(Started {
            account: None,
            content: Content::Room(room),
            meter: None,
        })
    }

    /// The signed-in session for `account`: its access with staff roles dropped, the session
    /// limits, and the content to show.
    async fn start_member(
        &self,
        link: &mut Link,
        account: russh_bbs_core::store::Account,
    ) -> Option<Started> {
        let started = timeout(
            MEMBER_START_TIMEOUT,
            self.directory.start_for_account(&account),
        )
        .await;
        let start = match started {
            // The account is read again as the session starts: one that is no longer an active
            // member gets nothing.
            Ok(Ok(start)) if start.access.account() == Some(account.id) => start,
            Ok(Ok(_)) => {
                info!(session = ?link.session.id(), "telnet session no longer holds member access");
                link.screen_text(&["Your session could not be started."])
                    .await;
                return None;
            }
            Ok(Err(error)) => {
                warn!(session = ?link.session.id(), %error, "telnet session could not start");
                link.screen_text(&["Your session could not be started."])
                    .await;
                return None;
            }
            Err(_) => {
                warn!(session = ?link.session.id(), "telnet session start timed out");
                link.screen_text(&["Your session could not be started."])
                    .await;
                return None;
            }
        };
        // Before anything resolves a permission, and before the limits are read from it.
        let access = without_staff(&start.access);
        let begun = timeout(
            MEMBER_START_TIMEOUT,
            begin_member_session(
                &*self.directory,
                &SystemClock,
                &link.session.member_sessions(),
                &access,
            ),
        )
        .await;
        let (content, meter) = match begun {
            Ok(Ok(meter)) => {
                // Last callers (M3-J): a telnet member session is recorded like an ssh one,
                // at the same point (after the limits admit it).
                crate::community::note_sign_in(&*self.directory, start.account).await;
                let notices =
                    russh_bbs_core::keys::notices_for_sign_in(&*self.directory, start.account)
                        .await;
                let flow =
                    MemberFlow::new(&start.handle, access, start.previous_login, start.settings)
                        .with_menu_tree(self.ui.menus().tree())
                        .signed_in_with_password()
                        .over_telnet()
                        .with_key_notices(russh_bbs_core::keys::notice_lines(&notices));
                (Content::Member(Box::new(flow)), Some(meter))
            }
            Ok(Err(LimitError::Refused(refusal))) => {
                info!(session = ?link.session.id(), limit = refusal.kind.name(), "telnet session refused by a limit");
                (Content::Limited(refusal.lines()), None)
            }
            Ok(Err(error)) => {
                warn!(session = ?link.session.id(), %error, "limits could not be checked");
                link.screen_text(&["Your session could not be started."])
                    .await;
                return None;
            }
            Err(_) => {
                warn!(session = ?link.session.id(), "limit check timed out");
                link.screen_text(&["Your session could not be started."])
                    .await;
                return None;
            }
        };
        link.session.set_authenticated_by_password(Access::Member);
        link.session.set_account(start.account);
        Some(Started {
            account: Some(start.account),
            content,
            meter,
        })
    }

    /// Run the shared shell for a signed-in member until it ends.
    async fn run_shell(
        &self,
        mut link: Link,
        frames: mpsc::Sender<Vec<u8>>,
        started: Started,
        session: &SessionRef,
        peer: SocketAddr,
    ) {
        let Started {
            account,
            content,
            meter,
        } = started;
        let screen = Screen::with_footer(
            link.window,
            self.limits.render_cap,
            content,
            Arc::clone(&self.ui),
            Some(TRANSPORT_MARKER),
        );
        let mut screen = match screen {
            Ok(screen) => screen,
            Err(error) => {
                warn!(%peer, %error, "telnet screen could not be built");
                return;
            }
        };
        // The board is in low-security mode whenever this transport runs (D27).
        screen.set_low_security(true);
        screen.set_client_terminal(link.terminal.as_deref(), None);
        let inbox = Arc::new(Inbox::default());
        let _close = CloseInbox(Arc::clone(&inbox));
        let abort = Arc::new(Abort::default());
        let shell = Shell::new(ShellParts {
            output: Output::Frames(frames),
            screen,
            inbox: Arc::clone(&inbox),
            directory: Arc::clone(&self.directory),
            // No founder claim over telnet: the claim needs a key and an SSH `claim` login
            // (D94 (4)).
            founder: Arc::new(crate::founder::Founder::closed()),
            session: session.clone(),
            abort: Arc::clone(&abort),
            fingerprint: None,
            peer_ip: Some(peer.ip()),
            account,
            passwords: Arc::clone(&self.passwords),
            chat: Arc::clone(self.registry.chat()),
            meter,
            redraw_interval: self.limits.redraw_interval,
            max_connections: self.limits.max_connections,
            write_stall: self.limits.write_stall,
            access_filter: Some(without_staff),
        });
        let mut shell = tokio::spawn(shell.run());
        session.set_shell_open(true);
        let idle = self.limits.idle_timeout;
        let mut shell_done = false;
        loop {
            tokio::select! {
                _ = &mut shell => {
                    shell_done = true;
                    break;
                }
                () = abort.triggered() => {
                    info!(%peer, "telnet peer is not reading; dropping the connection");
                    break;
                }
                read = timeout(idle, link.next()) => match read {
                    Ok(Some(decoded)) => {
                        if let Some(window) = decoded.window_size() {
                            inbox.set_resize(window);
                        }
                        if !decoded.data.is_empty() {
                            inbox.push_input(&decoded.data);
                        }
                    }
                    // The client left, or went over its budget.
                    Ok(None) => break,
                    Err(_) => {
                        info!(%peer, "telnet session idle; disconnecting");
                        break;
                    }
                },
            }
        }
        session.set_shell_open(false);
        inbox.close();
        // The shell settles its time and frees its slot when it sees the inbox closed.
        if !shell_done && timeout(DISCONNECT_GRACE, &mut shell).await.is_err() {
            warn!(%peer, "telnet shell did not stop; abandoning it");
            shell.abort();
        }
    }
}

/// The signed-in session, ready to hand to the shell.
struct Started {
    /// The signed-in account; `None` for a visitor in the waiting room.
    account: Option<russh_bbs_core::store::AccountId>,
    content: Content,
    meter: Option<russh_bbs_core::limits::MemberMeter>,
}

/// The connection's two directions: decoded input in, wire-ready bytes out.
pub(super) struct Link {
    read: OwnedReadHalf,
    decoder: Decoder,
    buffer: Vec<u8>,
    raw: mpsc::Sender<Vec<u8>>,
    pub(super) keys: KeyParser,
    /// Keys parsed but not yet used: typing ahead of a prompt.
    pub(super) pending: VecDeque<crate::ssh::terminal::Key>,
    /// The client's window, clamped.
    pub(super) window: PtySize,
    /// The terminal type the client reported, if it did (RFC 1091); picks the colour depth.
    pub(super) terminal: Option<String>,
    budget: SessionBudget,
    reported_dropped: u64,
    write_stall: Duration,
    pub(super) session: SessionRef,
}

impl Link {
    fn new(
        read: OwnedReadHalf,
        raw: mpsc::Sender<Vec<u8>>,
        limits: &Limits,
        session: SessionRef,
    ) -> Self {
        let window = clamp_window(0, 0);
        session.set_size(window);
        Self {
            read,
            decoder: Decoder::new(),
            buffer: vec![0; READ_CHUNK],
            raw,
            keys: KeyParser::new(),
            pending: VecDeque::new(),
            window,
            terminal: None,
            budget: SessionBudget::new(limits, std::time::Instant::now()),
            reported_dropped: 0,
            write_stall: limits.write_stall,
            session,
        }
    }

    /// Read and decode the next piece of input. `None`: the client left, or it went over its
    /// request and input budgets (D49) and is dropped.
    pub(super) async fn next(&mut self) -> Option<Decoded> {
        let read = self.read.read(&mut self.buffer).await.ok()?;
        if read == 0 {
            return None;
        }
        let now = std::time::Instant::now();
        let mut decoded = Decoded::default();
        // Over the request budget the chunk is dropped unread, as an excess SSH data packet
        // is; keeping up with a flood is not our job, ending it is.
        if self.budget.request(now) {
            if let Some(chunk) = self.buffer.get(..read) {
                self.decoder.feed(chunk, &mut decoded);
            }
            let allowed = self.budget.input(now, decoded.data.len());
            decoded.data.truncate(allowed);
        }
        let dropped = self.budget.dropped_bytes();
        if dropped != self.reported_dropped {
            self.reported_dropped = dropped;
            self.session.set_input_dropped(dropped);
        }
        if self.budget.exceeded() {
            info!(session = ?self.session.id(), dropped_input_bytes = dropped, "telnet session is over its request budget; disconnecting");
            return None;
        }
        if !decoded.reply.is_empty() {
            // Refusals are tiny and sent once per option; a full queue means the peer is not
            // reading, which the writer's owner handles.
            let _ = self.raw.try_send(std::mem::take(&mut decoded.reply));
        }
        if let Some((cols, rows)) = decoded.window {
            self.window = clamp_window(cols, rows);
            self.session.set_size(self.window);
        }
        if self.terminal.is_none() {
            self.terminal.clone_from(&decoded.terminal);
        }
        Some(decoded)
    }

    /// Read until the client has reported a window size (or left).
    async fn wait_for_window(&mut self) {
        while let Some(decoded) = self.next().await {
            if decoded.window.is_some() {
                self.feed_back(&decoded);
                return;
            }
            self.feed_back(&decoded);
        }
    }

    /// Keep input that arrived while we were waiting for something else, as keys.
    pub(super) fn feed_back(&mut self, decoded: &Decoded) {
        if !decoded.data.is_empty() {
            let keys = self.keys.feed(&decoded.data, std::time::Instant::now());
            self.pending.extend(keys);
        }
    }

    /// Queue wire-ready bytes. `false`: the peer is not reading or has gone.
    pub(super) async fn send_wire(&self, bytes: Vec<u8>) -> bool {
        matches!(
            timeout(self.write_stall, self.raw.send(bytes)).await,
            Ok(Ok(()))
        )
    }

    /// Queue screen text (encoded for the wire).
    pub(super) async fn send_text(&self, text: &str) -> bool {
        self.send_wire(encode(text.as_bytes())).await
    }

    /// Clear the screen, draw the `UNENCRYPTED` row and print `lines` from the top. Used by the
    /// login prompt and the refusals, which are not ratatui screens.
    pub(super) async fn screen_text(&self, lines: &[&str]) {
        let mut bytes = b"\x1b[2J".to_vec();
        bytes.extend(crate::ui::chrome::footer(
            TRANSPORT_MARKER,
            Tier::Ascii,
            self.window.cols(),
            self.window.rows(),
        ));
        bytes.extend_from_slice(b"\x1b[1;1H");
        for line in lines {
            bytes.extend_from_slice(line.as_bytes());
            bytes.extend_from_slice(b"\r\n");
        }
        self.send_wire(encode(&bytes)).await;
    }
}

/// A window as reported by NAWS, clamped like SSH's `pty-req` (D53) and to the sizes the screens
/// and the transport's marker row need. Zero means unknown.
pub(super) fn clamp_window(cols: u16, rows: u16) -> PtySize {
    let cols = if cols == 0 {
        DEFAULT_COLS
    } else {
        cols.max(MIN_COLS)
    };
    let rows = if rows == 0 {
        DEFAULT_ROWS
    } else {
        rows.max(MIN_ROWS)
    };
    PtySize::from_request(u32::from(cols), u32::from(rows))
}

impl Decoded {
    /// The window this input reported, clamped.
    fn window_size(&self) -> Option<PtySize> {
        self.window.map(|(cols, rows)| clamp_window(cols, rows))
    }
}

/// Owns the socket's write half. Wire-ready bytes go out first; shell frames are encoded here.
async fn write_loop(
    mut write: OwnedWriteHalf,
    mut raw: mpsc::Receiver<Vec<u8>>,
    mut frames: mpsc::Receiver<Vec<u8>>,
) {
    loop {
        let bytes = tokio::select! {
            biased;
            Some(bytes) = raw.recv() => bytes,
            Some(frame) = frames.recv() => encode(&frame),
            else => break,
        };
        if write.write_all(&bytes).await.is_err() {
            return;
        }
    }
    let _ = write.shutdown().await;
}
