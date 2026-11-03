//! Rendering to the PTY: a ratatui terminal whose backend writes into an in-memory buffer that
//! the SSH handler then sends down the channel (decision D19, resolving Q5).
//!
//! The terminal uses a **fixed viewport** whose size we set from the client's `pty-req` and
//! `window-change`, and draws through [`SessionBackend`], which answers ratatui's size and cursor
//! questions from the session. Nothing in the render path consults the server's own tty (D75), so
//! a session cannot affect another or the server's console, and a server without a tty renders.
//!
//! **Render cost is bounded by the sysop's cap, not the window (D52, D53).** The viewport is the
//! window size capped at the [`RenderCap`] and centered in it; the rest of a bigger window stays
//! blank. A session therefore holds at most two buffers of that many cells whatever size the
//! client asks for. A window change that leaves the drawn rectangle unchanged repaints in place,
//! and one that changes it resizes the buffers in place; the terminal is built once per session.
//!
//! **What is drawn.** One of three [`Content`]s, each a core state machine that produces a
//! [`View`]: the waiting room (M1-C, drawn by `crate::waiting_room`), the founder token prompt
//! (`ssh claim@host`, M1-D), or a member's signed-in session (M1-E, drawn by
//! `crate::settings` according to the member's terminal settings). A member at the main menu
//! is drawn by the screen framework instead (M2-A): the classic hotkey layout in `crate::ui`,
//! with its strings from the string table and security markers in its reserved status row.
//!
//! **Output safety.** Every string is fixed UI chrome or has passed the core sanitizer (the
//! sysop's banner and rules, and what the visitor typed). The SSH username is never rendered.

use std::io::{self, Write};
use std::sync::{Arc, Mutex, PoisonError};

use ratatui::backend::{Backend, ClearType as RegionType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::queue;
use ratatui::crossterm::terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::layout::{Position, Rect, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};
use russh_bbs_core::access::Snapshot;
use russh_bbs_core::community::{
    EditorAsk, NewPoll, Request as CommunityRequest, Response as CommunityResponse,
};
use russh_bbs_core::founder::{ClaimEffect, ClaimForm, ClaimInput};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::invites::{InviteOutcome, InviteRequest};
use russh_bbs_core::menu::MenuPage;
use russh_bbs_core::passwords::{Request as PasswordRequest, Response as PasswordResponse};
use russh_bbs_core::profiles::{
    Request as MembersRequest, Response as MembersResponse, ScreenView,
};
use russh_bbs_core::queue::{
    InviteAsk, QueueView, Request as QueueRequest, Response as QueueResponse,
};
use russh_bbs_core::sanitize::{MultiLine, SingleLine};
use russh_bbs_core::terminal_settings::strings::TITLE;
use russh_bbs_core::terminal_settings::{
    LayoutId, MemberEffect, MemberFlow, MemberInput, TerminalSettings,
};
use russh_bbs_core::waiting_room::{Page, RedeemRequest, RoomEffect, RoomInput, View, WaitingRoom};

use super::terminal::{Key, PtySize, RenderCap};
use crate::members;
use crate::settings::{self, Look};
use crate::sysop;
use crate::ui::classic::{self, ContentScreen, MENU_MARKERS, MenuScreen};
use crate::ui::editor::{Editor, EditorEffect, EditorRequest, EditorView, Outcome};
use crate::ui::strings::StringTable;
use crate::ui::{Ui, chrome, layout};
use crate::waiting_room::render;

/// The markers a staff screen shows while the board is in low-security mode (D27): the usual
/// one, and the mode's own.
const STAFF_MARKERS_LOW_SECURITY: [SecurityMarker; 2] =
    [SecurityMarker::ServerReadable, SecurityMarker::LowSecurity];

/// The heading of the screen a refused session sees.
const LIMIT_HEADING: &str = "Limit reached";

/// Bytes the backend has written and the handler has not yet sent. The backend owns one handle
/// and the screen keeps another to drain it, because ratatui does not expose its writer.
/// The lock is held only for a copy, never across an `.await`.
#[derive(Debug, Clone, Default)]
struct OutputBuffer(Arc<Mutex<Vec<u8>>>);

impl OutputBuffer {
    fn take(&self) -> Vec<u8> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Write for OutputBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The backend a session's ratatui terminal draws through: crossterm's escape sequences into the
/// session's [`OutputBuffer`], with every question about the terminal answered from the session
/// (D75).
///
/// [`CrosstermBackend`] answers `size` and `get_cursor_position` by asking the *server
/// process's* own terminal (`ioctl` on stdout, then `/dev/tty`, then a `tput` child process), which
/// is the wrong terminal and fails without a tty (a container, a service). ratatui asks during
/// `Terminal::resize` (to decide how to clear a fixed viewport), so a window change used to fail
/// the render. Here `size` is the client's window as the session last recorded it, the cursor is
/// where this backend last moved it, and `window_size` (pixels) is unsupported. Nothing in the
/// render path can reach the host terminal, because the crossterm query functions are not called
/// anywhere in this crate.
struct SessionBackend {
    inner: CrosstermBackend<OutputBuffer>,
    /// The client's window (or the member's override of it).
    size: Size,
    /// Where the last cursor move put the cursor.
    cursor: Position,
}

impl SessionBackend {
    fn new(output: &OutputBuffer, size: Size) -> Self {
        Self {
            inner: CrosstermBackend::new(output.clone()),
            size,
            cursor: Position::ORIGIN,
        }
    }
}

impl Write for SessionBackend {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Write::flush(&mut self.inner)
    }
}

impl Backend for SessionBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.inner.draw(content)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        let position = position.into();
        self.cursor = position;
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: RegionType) -> io::Result<()> {
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        Ok(self.size)
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "a session has no pixel size",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.inner)
    }
}

/// What a batch of keypresses asked the session to do besides redraw.
#[derive(Debug, Default)]
pub(crate) struct KeyEffects {
    /// Whether the session should end.
    pub(crate) disconnect: bool,
    /// A token the person submitted at the claim prompt. The session checks it (it needs the
    /// store) and reports back through [`Screen::claim_rejected`] or [`Screen::claim_accepted`].
    pub(crate) claim_token: Option<String>,
    /// Terminal settings the member changed. The session stores them.
    pub(crate) save_settings: Option<TerminalSettings>,
    /// An invite the visitor asked to redeem in the waiting room (M2-D). The session redeems it
    /// (it needs the store) and reports back through [`Screen::redeem_refused`] or
    /// [`Screen::redeemed`].
    pub(crate) redeem: Option<RedeemRequest>,
    /// A request for an invite the visitor made in the waiting room (M2-E). The session offers
    /// it to the queue (it needs the store and the peer address). It never reports back: the
    /// room has already shown its one fixed answer.
    pub(crate) request_invite: Option<InviteAsk>,
    /// An invite screen the member asked for (M2-D). The session carries it out and reports
    /// back through [`Screen::invite_done`].
    pub(crate) invite: Option<InviteRequest>,
    /// A members request (list page, profile, save) the screen is waiting on. The session runs
    /// it against the store and gives the answer to [`Screen::deliver_members`].
    pub(crate) members_request: Option<MembersRequest>,
    /// A queue request (list, approve, deny, vouch) the sysop or vouch screen is waiting on
    /// (M2-E). The session runs it against the store and gives the answer to
    /// [`Screen::deliver_queue`].
    pub(crate) queue_request: Option<QueueRequest>,
    /// A password request (status, set, clear) the password screen is waiting on (M3-D). The
    /// session runs it against the store and gives the answer to [`Screen::deliver_password`].
    pub(crate) password_request: Option<PasswordRequest>,
    /// A community request (a list, a post, a vote) the community screen is waiting on (M3-J).
    /// The session runs it against the store and gives the answer to
    /// [`Screen::deliver_community`].
    pub(crate) community_request: Option<CommunityRequest>,
    /// Keys held back because `members_request` has to be answered first. The session applies
    /// them after delivering the answer, so a fast typist's next key is not lost.
    pub(crate) held_back: Vec<Key>,
}

impl KeyEffects {
    /// Fold in what a later batch asked for.
    pub(crate) fn absorb(&mut self, later: Self) {
        self.disconnect |= later.disconnect;
        self.claim_token = later.claim_token.or(self.claim_token.take());
        self.save_settings = later.save_settings.or(self.save_settings.take());
        self.redeem = later.redeem.or(self.redeem.take());
        self.invite = later.invite.or(self.invite.take());
        self.request_invite = later.request_invite.or(self.request_invite.take());
        self.members_request = later.members_request;
        self.queue_request = later.queue_request;
        self.password_request = later.password_request;
        self.community_request = later.community_request;
        self.held_back = later.held_back;
    }
}

/// What a session is looking at.
pub(crate) enum Content {
    /// The waiting room, for a key that is not a member.
    Room(WaitingRoom),
    /// The founder token prompt.
    Claim(ClaimForm),
    /// A member's signed-in session: login, glyph test, settings, home, logoff.
    Member(Box<MemberFlow>),
    /// A resource limit refused the session (M2-H): the honest message, then the session ends.
    /// The lines are fixed words and numbers from core's `LimitRefusal`, so they need no
    /// sanitizing.
    Limited(Vec<String>),
}

impl Content {
    fn view(&self, ui: &Ui) -> View {
        match self {
            Self::Room(room) => room.view(),
            Self::Claim(form) => form.view(),
            Self::Member(flow) => flow.view(ui.theme(flow.settings().theme()).strings()),
            Self::Limited(lines) => View {
                title: TITLE,
                heading: LIMIT_HEADING,
                banner: Vec::new(),
                body: lines.clone(),
                note: None,
                menu: Vec::new(),
                fields: Vec::new(),
                notice: None,
                footer: "",
            },
        }
    }

    /// The main menu, while a member is at it. Everything else keeps its own drawing.
    fn menu_page(&self) -> Option<MenuPage> {
        match self {
            Self::Member(flow) => flow.menu_page(),
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => None,
        }
    }

    /// Whether the content is a member screen drawn in the framed layout with the status row
    /// but is not the menu (the invite screens, M2-D).
    fn is_framed_screen(&self) -> bool {
        match self {
            Self::Member(flow) => flow.is_invite_screen(),
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => false,
        }
    }

    /// The members list, a profile or the profile form, while a member is at one.
    fn members_view(&self) -> Option<ScreenView> {
        match self {
            Self::Member(flow) => flow.members_view(),
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => None,
        }
    }

    /// The community screens, while a member is at one (M3-J).
    fn community_view(&self) -> Option<ScreenView> {
        match self {
            Self::Member(flow) => flow.community_view(),
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => None,
        }
    }

    /// The sysop queue or the vouch screen, while a member is at one (M2-E).
    fn queue_view(&self) -> Option<QueueView> {
        match self {
            Self::Member(flow) => flow.queue_view(),
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => None,
        }
    }

    /// The icon tier markers are drawn in: the member's, which is ASCII until the glyph test
    /// is answered.
    fn tier(&self) -> Tier {
        match self {
            Self::Member(flow) => flow.settings().icon_tier(),
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => Tier::Ascii,
        }
    }

    /// How to draw: a member's settings choose color and borders; everyone else gets the
    /// plain layout (the waiting room and claim prompt predate settings and use no color).
    fn look(&self, ui: &Ui, area: Rect) -> Option<Look> {
        match self {
            Self::Member(flow) => {
                let settings = flow.settings();
                let theme = ui.theme(settings.theme());
                let layout = layout::effective(settings.layout(), area);
                Some(Look::themed(settings, layout, theme.palette()))
            }
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => None,
        }
    }

    /// The layout in force for a member at `area` (their choice, or a fallback that fits).
    fn layout(&self, area: Rect) -> LayoutId {
        match self {
            Self::Member(flow) => layout::effective(flow.settings().layout(), area),
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => LayoutId::Classic,
        }
    }

    /// The strings to draw with: the member's theme, or the default for everyone else.
    fn strings<'a>(&self, ui: &'a Ui) -> &'a StringTable {
        match self {
            Self::Member(flow) => ui.theme(flow.settings().theme()).strings(),
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => ui.strings(),
        }
    }
}

/// One session's screen.
pub(crate) struct Screen {
    terminal: Terminal<SessionBackend>,
    output: OutputBuffer,
    /// The rectangle `terminal` draws into.
    viewport: Rect,
    /// Widest and tallest `viewport` allowed.
    cap: RenderCap,
    /// How many times `viewport` changed (tests assert an unchanged size changes nothing).
    resizes: u32,
    content: Content,
    /// The string table and menu tree the framework's screens draw with.
    ui: Arc<Ui>,
    /// What the client reported. The drawn size is this with the member's overrides applied.
    pty: PtySize,
    /// The content changed since the last frame; the next [`Screen::render`] draws it.
    dirty: bool,
    /// The window or the drawn size changed (or the screen must be cleared); the next render
    /// clears the client's screen and repaints everything.
    rebuild: bool,
    /// The day's last `open` place went while this visitor's create-account form was open: the
    /// room becomes the `apply` room, with its notice, as soon as they leave the form (FIX-M2-4).
    degrade_on_leave: bool,
    /// The full-screen editor, while the poll form has one open (M3-J, using M3-B's widget).
    /// While it is open every key goes to it and it is drawn instead of the content.
    /// Held in a `Mutex` only to make the session future `Send`: the widget's caches are
    /// `Cell`s. It is always reached with `get_mut`, so nothing ever waits on the lock.
    editor: Option<Mutex<Editor>>,
    /// What the transport adds to the screens (M3-H).
    transport: Transport,
}

/// What the transport around a screen adds to it (M3-H, D27).
#[derive(Debug, Clone, Copy, Default)]
struct Transport {
    /// The row the transport reserves for its own marker, if any (telnet's `UNENCRYPTED`).
    footer: Option<Footer>,
    /// The board is in low-security mode (D27): staff screens add the `LOW-SECURITY` marker.
    low_security: bool,
}

/// A row below the layouts that the transport owns and nothing else draws into (M3-H, D27). The
/// layouts are given a window one row shorter, so no screen or content can reach it; it is
/// painted after every frame with the marker's reserved style.
#[derive(Debug, Clone, Copy)]
struct Footer {
    marker: SecurityMarker,
    /// The whole window, footer row included.
    window: PtySize,
}

impl Footer {
    /// The window the layouts get: all of it but the footer row.
    fn usable(window: PtySize) -> PtySize {
        PtySize::from_request(
            u32::from(window.cols()),
            u32::from(window.rows().saturating_sub(1).max(1)),
        )
    }
}

impl Screen {
    /// Create a screen of `size` showing `content`, drawing into at most `cap`. Nothing is drawn
    /// until [`Screen::enter`].
    pub(crate) fn new(
        size: PtySize,
        cap: RenderCap,
        content: Content,
        ui: Arc<Ui>,
    ) -> io::Result<Self> {
        Self::with_footer(size, cap, content, ui, None)
    }

    /// Like [`Screen::new`], reserving the last row of the window for `footer`'s marker when
    /// one is given. `size` is the whole window; the layouts draw into all but that row.
    pub(crate) fn with_footer(
        window: PtySize,
        cap: RenderCap,
        content: Content,
        ui: Arc<Ui>,
        footer: Option<SecurityMarker>,
    ) -> io::Result<Self> {
        let footer = footer.map(|marker| Footer { marker, window });
        let size = footer.map_or(window, |_| Footer::usable(window));
        let output = OutputBuffer::default();
        let mut content = content;
        prepare_member(&mut content, size, &ui);
        let drawn = drawn_size(size, &content);
        let viewport = viewport(drawn, cap);
        Ok(Self {
            terminal: new_terminal(&output, drawn, viewport)?,
            output,
            viewport,
            cap,
            resizes: 0,
            content,
            ui,
            pty: size,
            dirty: false,
            rebuild: false,
            degrade_on_leave: false,
            editor: None,
            transport: Transport {
                footer,
                low_security: false,
            },
        })
    }

    /// Put the board's low-security mode on or off for this session's staff screens.
    pub(crate) fn set_low_security(&mut self, on: bool) {
        self.transport.low_security = on;
    }

    /// Feed keypresses to the current content. Stops at the first key that ends the session or
    /// submits a token; keys after it are ignored. Nothing is drawn: call [`Screen::render`]
    /// once for the whole batch, so a flood of keys costs one redraw, not one per key.
    pub(crate) fn apply_keys(&mut self, keys: &[Key]) -> KeyEffects {
        let mut effects = KeyEffects::default();
        if self.editor.is_some() {
            self.editor_keys(keys, &mut effects);
            self.dirty = true;
            return effects;
        }
        let mut open_editor_at = None;
        match &mut self.content {
            Content::Room(room) => {
                for input in keys.iter().copied().filter_map(room_input) {
                    match room.input(input) {
                        RoomEffect::Stay => {}
                        RoomEffect::Disconnect => {
                            effects.disconnect = true;
                            break;
                        }
                        RoomEffect::Redeem(request) => {
                            effects.redeem = Some(request);
                            break;
                        }
                        RoomEffect::RequestInvite(ask) => {
                            effects.request_invite = Some(ask);
                            break;
                        }
                    }
                }
                if self.degrade_on_leave && room.page() != Page::CreateAccount {
                    // The visitor left the form: show the room the next connection would get.
                    self.degrade_on_leave = false;
                    room.degrade_open(true);
                    self.dirty = true;
                }
            }
            Content::Claim(form) => {
                for input in keys.iter().copied().filter_map(claim_input) {
                    match form.input(input) {
                        ClaimEffect::Stay => {}
                        ClaimEffect::Disconnect => {
                            effects.disconnect = true;
                            break;
                        }
                        ClaimEffect::Submit(token) => {
                            effects.claim_token = Some(token);
                            break;
                        }
                    }
                }
            }
            Content::Limited(_) => effects.disconnect = true,
            Content::Member(flow) => {
                for (index, input) in keys
                    .iter()
                    .copied()
                    .enumerate()
                    .filter_map(|(index, key)| member_input(key).map(|input| (index, input)))
                {
                    match flow.input(input) {
                        MemberEffect::Stay => {}
                        MemberEffect::Save(settings) => effects.save_settings = Some(settings),
                        MemberEffect::Invite(request) => {
                            effects.invite = Some(request);
                            break;
                        }
                        MemberEffect::Disconnect => {
                            effects.disconnect = true;
                            break;
                        }
                    }
                    // A screen that needs the store stops the batch: keys after it would act on
                    // a list that is about to be replaced.
                    if take_request(flow, &mut effects) {
                        effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                        break;
                    }
                    // The poll form asked for the full-screen editor: the keys after it in
                    // this batch are the editor's.
                    if let Some(ask) = flow.take_community_editor() {
                        open_editor_at = Some((index + 1, ask));
                        break;
                    }
                }
            }
        }
        if let Some((from, ask)) = open_editor_at {
            self.open_editor(&ask);
            let rest = keys.get(from..).unwrap_or_default();
            if self.editor.is_some() && !rest.is_empty() {
                self.editor_keys(rest, &mut effects);
            }
        }
        self.dirty = true;
        if effects.save_settings.is_some() {
            // A size override changes the drawn size; rebuild so the old cells are cleared.
            self.rebuild = true;
        }
        effects
    }

    /// What the signed-in session holds, for the front end's own checks. `None` outside a
    /// member session.
    pub(crate) fn access(&self) -> Option<&Snapshot> {
        match &self.content {
            Content::Member(flow) => Some(flow.access_snapshot()),
            Content::Room(_) | Content::Claim(_) | Content::Limited(_) => None,
        }
    }

    /// Replace what a member session holds, for the next action (D33).
    pub(crate) fn set_access(&mut self, access: Snapshot) {
        if let Content::Member(flow) = &mut self.content {
            flow.set_access(access);
        }
    }

    /// The answer to the members request taken from [`KeyEffects`]. The screen shows it on the
    /// next render.
    pub(crate) fn deliver_members(&mut self, response: MembersResponse) {
        if let Content::Member(flow) = &mut self.content {
            flow.deliver_members(response);
        }
        self.dirty = true;
    }

    /// The answer to the queue request taken from [`KeyEffects`]. The screen shows it on the
    /// next render.
    pub(crate) fn deliver_queue(&mut self, response: QueueResponse) {
        if let Content::Member(flow) = &mut self.content {
            flow.deliver_queue(response);
        }
        self.dirty = true;
    }

    /// The answer to the password request taken from [`KeyEffects`]. The screen shows it on the
    /// next render.
    pub(crate) fn deliver_password(&mut self, response: PasswordResponse) {
        if let Content::Member(flow) = &mut self.content {
            flow.deliver_password(response);
        }
        self.dirty = true;
    }

    /// The answer to the community request taken from [`KeyEffects`]. The screen shows it on the
    /// next render.
    pub(super) fn deliver_community(&mut self, response: CommunityResponse) {
        if let Content::Member(flow) = &mut self.content {
            flow.deliver_community(response);
        }
        self.dirty = true;
    }

    /// The community request the screen asked for after an answer (an emptied poll page asks for
    /// the first one), once.
    pub(super) fn take_community_request(&mut self) -> Option<CommunityRequest> {
        match &mut self.content {
            Content::Member(flow) => flow.take_community_request(),
            Content::Room(_) | Content::Claim(_) | Content::Limited(_) => None,
        }
    }

    /// Open the editor for a poll description. If its text does not fit the editor's bounds
    /// (it cannot: the screen only holds text the editor returned) nothing opens.
    fn open_editor(&mut self, ask: &EditorAsk) {
        let title = SingleLine::new(40).sanitize(
            self.content
                .strings(&self.ui)
                .text(russh_bbs_core::community::keys::FORM_EDITOR_TITLE),
        );
        let initial =
            MultiLine::new(NewPoll::description_limits().max_chars()).sanitize(&ask.initial);
        let request = EditorRequest::new(title)
            .initial(initial)
            .limits(NewPoll::description_limits());
        match Editor::open(request, None) {
            Ok(editor) => self.editor = Some(Mutex::new(editor)),
            Err(_) => {
                if let Content::Member(flow) = &mut self.content {
                    flow.deliver_community_description(None);
                }
            }
        }
    }

    /// Feed keys to the open editor. A finished editor hands its text to the poll form. There
    /// is no draft context, so there is nothing to save: leaving with changes asks only whether
    /// to keep editing or discard.
    fn editor_keys(&mut self, keys: &[Key], effects: &mut KeyEffects) {
        for key in keys.iter().copied() {
            let Some(editor) = &mut self.editor else {
                return;
            };
            let editor = editor.get_mut().unwrap_or_else(PoisonError::into_inner);
            let text = match editor.input(key) {
                EditorEffect::Stay => continue,
                EditorEffect::Done(Outcome::Submitted(body)) => Some(body.trimmed().to_owned()),
                EditorEffect::Done(Outcome::Cancelled | Outcome::SavedDraft)
                | EditorEffect::SaveDraft(_) => None,
                EditorEffect::LogOff(_) => {
                    effects.disconnect = true;
                    return;
                }
            };
            self.editor = None;
            if let Content::Member(flow) = &mut self.content {
                flow.deliver_community_description(text);
            }
            self.rebuild = true;
            return;
        }
    }

    /// The queue request the screen asked for after an answer (a decision asks for the list
    /// again), once.
    pub(crate) fn take_queue_request(&mut self) -> Option<QueueRequest> {
        match &mut self.content {
            Content::Member(flow) => flow.take_queue_request(),
            Content::Room(_) | Content::Claim(_) | Content::Limited(_) => None,
        }
    }

    /// The submitted token failed. The screen shows the one generic message on the next render;
    /// the bool is whether the session should now end (attempts used up).
    pub(crate) fn claim_rejected(&mut self) -> bool {
        let disconnect = match &mut self.content {
            Content::Claim(form) => form.rejected() == ClaimEffect::Disconnect,
            Content::Room(_) | Content::Member(_) | Content::Limited(_) => false,
        };
        self.dirty = true;
        disconnect
    }

    /// A limit ended the session (M2-H): show the message instead of the menu. The caller then
    /// ends the session; the message is also printed on the main screen when it is left, so it
    /// stays in the scrollback.
    pub(crate) fn limit_reached(&mut self, lines: Vec<String>) {
        self.content = Content::Limited(lines);
        self.rebuild = true;
    }

    /// Whether the content has nothing more to offer, so the session should end after the
    /// first frame (a start refused by a limit).
    pub(crate) fn is_ended(&self) -> bool {
        matches!(self.content, Content::Limited(_))
    }

    /// The claim succeeded: the new sysop's session starts at the login screen.
    pub(crate) fn claim_accepted(&mut self, flow: MemberFlow) {
        if let Content::Claim(form) = &mut self.content {
            form.accepted();
        }
        self.content = Content::Member(Box::new(flow));
        prepare_member(&mut self.content, self.pty, &self.ui);
        self.rebuild = true;
    }

    /// The redemption did not work. The room shows its one fixed message on the next render.
    pub(crate) fn redeem_refused(&mut self) {
        if let Content::Room(room) = &mut self.content {
            room.redeem_refused();
        }
        self.dirty = true;
    }

    /// The create-account form was refused because the day's cap is used up. The form shows the
    /// same refusal sentence as for every other failure (no-leak); once the visitor leaves the
    /// form, the room becomes the `apply` room with its notice, without a reconnect. Only an
    /// `open` room is changed (`WaitingRoom::degrade_open` does nothing in any other mode).
    pub(crate) fn open_cap_reached(&mut self) {
        self.redeem_refused();
        if matches!(self.content, Content::Room(_)) {
            self.degrade_on_leave = true;
        }
    }

    /// The redemption worked: the visitor's session continues as the new member, at the login
    /// screen.
    pub(crate) fn redeemed(&mut self, flow: MemberFlow) {
        self.content = Content::Member(Box::new(flow));
        prepare_member(&mut self.content, self.pty, &self.ui);
        self.rebuild = true;
    }

    /// An invite request finished: show its screen.
    pub(crate) fn invite_done(&mut self, outcome: InviteOutcome) {
        if let Content::Member(flow) = &mut self.content {
            flow.invite_done(outcome);
        }
        self.dirty = true;
    }

    /// Switch the client to the alternate screen, hide the cursor and draw. Returns the bytes
    /// to send.
    pub(crate) fn enter(&mut self) -> io::Result<Vec<u8>> {
        queue!(self.terminal.backend_mut(), EnterAlternateScreen, Hide)?;
        self.draw()
    }

    /// Record a new window size. Nothing is drawn; [`Screen::render`] does that once, however
    /// many sizes arrived since the last frame. A size equal to the current one still asks for
    /// a full redraw: clients use a window change to repaint a screen.
    ///
    /// ratatui asks its backend for the terminal size and cursor in `Terminal::resize` (through
    /// `clear_viewport`), `clear` and `autoresize`. The crossterm backend answers by querying the
    /// *server process's* terminal, which is the wrong terminal and fails without a tty (D75; this
    /// comment used to claim `resize` did not ask, and it does). The terminal therefore draws
    /// through [`SessionBackend`], which answers from the session. A changed drawn rectangle
    /// resizes the buffers in place and an unchanged one only forgets what was drawn; neither
    /// builds a terminal.
    pub(crate) fn set_size(&mut self, size: PtySize) {
        let size = match &mut self.transport.footer {
            Some(footer) => {
                footer.window = size;
                Footer::usable(size)
            }
            None => size,
        };
        self.pty = size;
        tell_flow_the_terminal(&mut self.content, size);
        self.rebuild = true;
    }

    /// Draw whatever changed since the last frame, once. `None` when nothing did.
    pub(crate) fn render(&mut self) -> io::Result<Option<Vec<u8>>> {
        if self.rebuild {
            self.rebuild = false;
            self.dirty = false;
            return self.rebuild_now().map(Some);
        }
        if self.dirty {
            self.dirty = false;
            return self.draw().map(Some);
        }
        Ok(None)
    }

    /// Clear the client's screen and draw everything. The buffers are resized in place when the
    /// rectangle to draw into changed (a shrink keeps the allocation); otherwise they are reset.
    fn rebuild_now(&mut self) -> io::Result<Vec<u8>> {
        queue!(self.terminal.backend_mut(), Clear(ClearType::All))?;
        let drawn = drawn_size(self.pty, &self.content);
        self.terminal.backend_mut().size = window_size(drawn);
        let target = viewport(drawn, self.cap);
        if target == self.viewport {
            // `swap_buffers` resets the buffer it leaves behind, so two swaps blank both and the
            // next draw diffs against an empty frame: every cell is written again.
            self.terminal.swap_buffers();
            self.terminal.swap_buffers();
        } else {
            self.terminal.resize(target)?;
            self.viewport = target;
            self.resizes += 1;
        }
        self.draw()
    }

    /// Restore the client's terminal (cursor, main screen). After a logoff the goodbye is
    /// printed on the main screen, so it stays in the scrollback. Returns the bytes to send.
    pub(crate) fn leave(&mut self) -> io::Result<Vec<u8>> {
        queue!(self.terminal.backend_mut(), Show, LeaveAlternateScreen)?;
        let mut bytes = self.take_output();
        let farewell = match &self.content {
            Content::Member(flow) => flow.farewell(self.content.strings(&self.ui)),
            Content::Limited(lines) => Some(lines.clone()),
            Content::Room(_) | Content::Claim(_) => None,
        };
        for line in farewell.into_iter().flatten() {
            bytes.extend_from_slice(line.as_bytes());
            bytes.extend_from_slice(b"\r\n");
        }
        Ok(bytes)
    }

    /// Draw the open editor, if there is one. `true` when it was drawn.
    fn draw_editor(&mut self) -> io::Result<bool> {
        let (Some(editor), Content::Member(flow)) = (&mut self.editor, &self.content) else {
            return Ok(false);
        };
        let editor = editor.get_mut().unwrap_or_else(PoisonError::into_inner);
        let view = EditorView {
            ui: &self.ui,
            settings: flow.settings(),
            markers: &MENU_MARKERS,
        };
        self.terminal.draw(|frame| editor.render(frame, &view))?;
        Ok(true)
    }

    fn draw(&mut self) -> io::Result<Vec<u8>> {
        // While the editor is open it is drawn instead of the content (the transport's footer
        // row below still follows it).
        let drew_editor = self.draw_editor()?;
        let view = self.content.view(&self.ui);
        let look = self.content.look(&self.ui, self.viewport);
        let layout = self.content.layout(self.viewport);
        let menu = self.content.menu_page();
        // The community screens (M3-J) produce the members screens' view and are drawn the same.
        let members_view = self
            .content
            .members_view()
            .or_else(|| self.content.community_view());
        let queue_view = self.content.queue_view();
        let tier = self.content.tier();
        let strings = self.content.strings(&self.ui);
        let framed = self.content.is_framed_screen();
        // Staff screens add the low-security marker while the board is in that mode (D27).
        let staff_markers: &[SecurityMarker] = if self.transport.low_security {
            &STAFF_MARKERS_LOW_SECURITY
        } else {
            &MENU_MARKERS
        };
        if !drew_editor {
            self.terminal.draw(|frame| match (look, &menu) {
                // The sysop queue and the vouch screen (M2-E): the same layout and status row. The
                // requester's text is drawn in the content box only.
                (Some(look), None) if queue_view.is_some() => {
                    if let Some(screen) = &queue_view {
                        let width = usize::from(frame.area().width.min(settings::MAX_WIDTH))
                            .saturating_sub(2);
                        classic::render_content(
                            frame,
                            ContentScreen {
                                strings,
                                lines: sysop::queue::lines(screen, strings, look, width),
                                look,
                                tier,
                                markers: staff_markers,
                            },
                        );
                    }
                }
                // A members screen: the same classic layout and status row, with its own lines.
                (Some(look), None) if members_view.is_some() => {
                    if let Some(screen) = &members_view {
                        let width = usize::from(frame.area().width.min(settings::MAX_WIDTH))
                            .saturating_sub(2);
                        classic::render_content(
                            frame,
                            ContentScreen {
                                strings,
                                lines: members::lines(screen, strings, look, width),
                                look,
                                tier,
                                markers: &MENU_MARKERS,
                            },
                        );
                    }
                }
                // The main menu: the classic hotkey layout, with the reserved status row.
                (Some(look), Some(page)) => layout::render_menu(
                    frame,
                    layout,
                    &MenuScreen {
                        strings,
                        page,
                        body: &view.body,
                        notice: view.notice,
                        look,
                        tier,
                        markers: &MENU_MARKERS,
                    },
                ),
                // The invite screens (M2-D): the same framed layout, with the reserved status row.
                (Some(look), None) if framed => classic::render_content(
                    frame,
                    ContentScreen {
                        strings,
                        lines: classic::view_lines(strings, &view, look),
                        look,
                        tier,
                        markers: &MENU_MARKERS,
                    },
                ),
                (Some(look), None) => settings::render(frame, &view, look, strings),
                (None, _) => render(frame, &view),
            })?;
        }
        let mut bytes = self.take_output();
        if let Some(footer) = self.transport.footer {
            // After the frame, so a repaint or a clear never leaves the row bare.
            bytes.extend(chrome::footer(
                footer.marker,
                tier,
                footer.window.cols(),
                footer.window.rows(),
            ));
        }
        Ok(bytes)
    }

    fn take_output(&mut self) -> Vec<u8> {
        self.output.take()
    }
}

fn new_terminal(
    output: &OutputBuffer,
    window: PtySize,
    area: Rect,
) -> io::Result<Terminal<SessionBackend>> {
    let options = TerminalOptions {
        viewport: Viewport::Fixed(area),
    };
    Terminal::with_options(SessionBackend::new(output, window_size(window)), options)
}

/// A window as the backend reports it.
fn window_size(window: PtySize) -> Size {
    Size::new(window.cols(), window.rows())
}

/// Give a new member session what it needs from the server: the real terminal size and the
/// themes the settings screen can switch between. Every place that puts a member flow on a
/// screen (start, claim, redeem) calls this, so none can forget the theme list.
fn prepare_member(content: &mut Content, pty: PtySize, ui: &Ui) {
    tell_flow_the_terminal(content, pty);
    if let Content::Member(flow) = content {
        flow.set_themes(ui.theme_names());
    }
}

/// Give a member session the real terminal size, so its settings screen can say when a stored
/// size override is cut down to fit (D64). Other content has no overrides.
fn tell_flow_the_terminal(content: &mut Content, pty: PtySize) {
    if let Content::Member(flow) = content {
        flow.set_terminal(pty.cols(), pty.rows());
    }
}

/// The size to draw at: the client's, with a member's width and height overrides applied.
fn drawn_size(pty: PtySize, content: &Content) -> PtySize {
    match content {
        Content::Member(flow) => {
            let (cols, rows) = flow.settings().size(pty.cols(), pty.rows());
            PtySize::from_request(u32::from(cols), u32::from(rows))
        }
        Content::Room(_) | Content::Claim(_) | Content::Limited(_) => pty,
    }
}

/// The rectangle to draw into: `drawn` capped at `cap` and centered in it, so the cost of a
/// session is bounded by the sysop's choice, not the window the client chose (D52, D53).
fn viewport(drawn: PtySize, cap: RenderCap) -> Rect {
    let cols = drawn.cols().min(cap.cols());
    let rows = drawn.rows().min(cap.rows());
    Rect::new(
        (drawn.cols() - cols) / 2,
        (drawn.rows() - rows) / 2,
        cols,
        rows,
    )
}

/// Move the store request a member screen is waiting on (members M2-F, queue M2-E, password
/// M3-D, community M3-J) into `effects`. `true` when there was one.
fn take_request(flow: &mut MemberFlow, effects: &mut KeyEffects) -> bool {
    if let Some(request) = flow.take_members_request() {
        effects.members_request = Some(request);
    } else if let Some(request) = flow.take_queue_request() {
        effects.queue_request = Some(request);
    } else if let Some(request) = flow.take_password_request() {
        effects.password_request = Some(request);
    } else if let Some(request) = flow.take_community_request() {
        effects.community_request = Some(request);
    } else {
        return false;
    }
    true
}

/// Translate a keypress for the room. Keys the room has no use for are dropped.
fn room_input(key: Key) -> Option<RoomInput> {
    match key {
        Key::Char(c) => Some(RoomInput::Char(c)),
        Key::Enter => Some(RoomInput::Enter),
        Key::Backspace => Some(RoomInput::Backspace),
        Key::Escape => Some(RoomInput::Escape),
        Key::Ctrl('c' | 'd') => Some(RoomInput::Quit),
        Key::Ctrl(_) | Key::Up | Key::Down | Key::Other => None,
    }
}

/// Translate a keypress for the claim prompt.
fn claim_input(key: Key) -> Option<ClaimInput> {
    match key {
        Key::Char(c) => Some(ClaimInput::Char(c)),
        Key::Enter => Some(ClaimInput::Enter),
        Key::Backspace => Some(ClaimInput::Backspace),
        Key::Escape => Some(ClaimInput::Escape),
        Key::Ctrl('c' | 'd') => Some(ClaimInput::Quit),
        Key::Ctrl(_) | Key::Up | Key::Down | Key::Other => None,
    }
}

/// Translate a keypress for a member's session.
fn member_input(key: Key) -> Option<MemberInput> {
    match key {
        Key::Char(c) => Some(MemberInput::Char(c)),
        Key::Enter => Some(MemberInput::Enter),
        Key::Backspace => Some(MemberInput::Backspace),
        Key::Escape => Some(MemberInput::Escape),
        Key::Up => Some(MemberInput::Up),
        Key::Down => Some(MemberInput::Down),
        Key::Ctrl('c' | 'd') => Some(MemberInput::Quit),
        Key::Ctrl(_) | Key::Other => None,
    }
}

#[cfg(test)]
mod tests;
