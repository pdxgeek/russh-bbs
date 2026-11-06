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
use std::time::Instant;

use ratatui::backend::{Backend, ClearType as RegionType, CrosstermBackend, WindowSize};
use ratatui::buffer::Cell;
use ratatui::crossterm::cursor::{Hide, Show};
use ratatui::crossterm::queue;
use ratatui::crossterm::terminal::{Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::layout::{Position, Rect, Size};
use ratatui::{Terminal, TerminalOptions, Viewport};
use russh_bbs_core::access::{Permission, PermissionCheck, Snapshot};
use russh_bbs_core::chat::{
    ChatEffect, ChatKey, ChatRequest, ChatResponse, ChatScreen, Event as ChatEvent, RoomId,
};
use russh_bbs_core::community::{
    EditorAsk, NewPoll, Request as CommunityRequest, Response as CommunityResponse,
};
use russh_bbs_core::founder::{ClaimEffect, ClaimForm, ClaimInput};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::invites::{InviteOutcome, InviteRequest};
use russh_bbs_core::keys::{Request as KeysRequest, Response as KeysResponse};
use russh_bbs_core::menu::MenuPage;
use russh_bbs_core::passwords::{Request as PasswordRequest, Response as PasswordResponse};
use russh_bbs_core::profiles::{
    Request as MembersRequest, Response as MembersResponse, ScreenView,
};
use russh_bbs_core::queue::{
    InviteAsk, QueueView, Request as QueueRequest, Response as QueueResponse,
};
use russh_bbs_core::sanitize::{MultiLine, SingleLine};
use russh_bbs_core::terminal_settings::strings::{LOGIN_HEADING, TITLE};
use russh_bbs_core::terminal_settings::{
    LayoutId, MemberEffect, MemberFlow, MemberInput, TerminalSettings,
};
use russh_bbs_core::waiting_room::{
    CodeRequest, LinkRequest, Page, RedeemRequest, RoomEffect, RoomInput, View, WaitingRoom,
};

use super::interact::{self, Interact, PaletteStep};
use russh_bbs_core::widgets::{Choice, WidgetData, WidgetId};

use super::terminal::{Key, PtySize, RenderCap};
use crate::forums::{self, DrawCtx, ForumEffect, ForumsScreen};
use crate::mail::{self, MailEffect, MailScreen};
use crate::members;
use crate::settings::{self, Look};
use crate::sysop;
use crate::ui::art::Slot as ArtSlot;
use crate::ui::classic::{self, ContentScreen, MENU_MARKERS, MenuScreen};
use crate::ui::depth::ColorDepth;
use crate::ui::editor::{Editor, EditorEffect, EditorRequest, EditorView, Outcome};
use crate::ui::frame::clock_text;
use crate::ui::fx;
use crate::ui::input::mouse::Mouse;
use crate::ui::palette;
use crate::ui::screens::motion::Motion;
use crate::ui::strings::StringTable;
use crate::ui::toast::{Kind as ToastKind, Text as ToastText};
use crate::ui::widgets::screen::{self as widgets_screen, WidgetsScreen};
use crate::ui::widgets::{self, WidgetView};
use crate::ui::{Ui, backdrop, chrome, layout};
use crate::waiting_room::render;
use dock::{DockState, Pane as DockPane};

/// The markers a staff screen shows while the board is in low-security mode (D27): the mode's
/// own. Outside that mode staff screens show none (D93).
const STAFF_MARKERS_LOW_SECURITY: [SecurityMarker; 1] = [SecurityMarker::LowSecurity];

/// The heading of the screen a refused session sees.
const LIMIT_HEADING: &str = "Limit reached";
/// The heading when a revoked credential ended the session (FIX-M3-14, D99).
const REVOKED_HEADING: &str = "Signed out";
/// What a session whose credential was revoked reads before it closes. Fixed text: it says
/// nothing about who or what changed.
pub(crate) const REVOKED_LINE: &str = "Your sign-in was revoked. Please sign in again.";

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

/// What the palette needs from the session (UI-9).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaletteAsk {
    /// Nothing.
    #[default]
    Nothing,
    /// The names of the boards the member may list.
    Boards,
}

/// What a batch of keypresses asked the session to do besides redraw.
#[derive(Debug, Default)]
pub(crate) struct KeyEffects {
    /// Whether the session should end.
    pub(crate) disconnect: bool,
    /// A token the person submitted at the claim prompt. The session checks it (it needs the
    /// store) and reports back through [`Screen::claim_rejected`] or [`Screen::claim_accepted`].
    pub(crate) claim_token: Option<String>,
    /// The founder's chosen password, with the token, when the prompt asked for one (D94 (6)).
    pub(crate) claim_password: Option<russh_bbs_core::passwords::Secret>,
    /// Terminal settings the member changed. The session stores them.
    pub(crate) save_settings: Option<TerminalSettings>,
    /// A theme-pack request from the sysop's Theme packs screen (UI-7). The session runs it
    /// (files and the audit log) and reports through [`Screen::packs_done`].
    pub(crate) packs: Option<russh_bbs_core::terminal_settings::PackRequest>,
    /// A request from the sysop's Menus and pages screen (UI-6): list or reload. The session
    /// runs it (the files and the audit log) and reports through [`Screen::menu_tools_done`].
    pub(crate) menu_tools: Option<russh_bbs_core::menu::MenuToolsRequest>,
    /// An invite the visitor asked to redeem in the waiting room (M2-D). The session redeems it
    /// (it needs the store) and reports back through [`Screen::redeem_refused`] or
    /// [`Screen::redeemed`].
    pub(crate) redeem: Option<RedeemRequest>,
    /// A handle and password the visitor gave to bind this key to an account they have
    /// (D94 (8)). The session checks them and reports through [`Screen::redeem_refused`] or
    /// [`Screen::redeemed`].
    pub(crate) link: Option<LinkRequest>,
    /// A device-link code the visitor typed (FIX-M3-11): bind this key to the account it was
    /// made for. The session redeems it and reports through [`Screen::redeem_refused`] or
    /// [`Screen::redeemed`].
    pub(crate) link_device: Option<CodeRequest>,
    /// A staff recovery code and a new password the visitor typed (FIX-M3-11), redeemed the same
    /// way.
    pub(crate) recover: Option<CodeRequest>,
    /// A keys request (list, add, remove, make a code) the My keys or recovery screen is waiting
    /// on (FIX-M3-11). The session runs it and gives the answer to [`Screen::deliver_keys`].
    pub(crate) keys_request: Option<KeysRequest>,
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
    /// The member opened the chat screen (M3-F): the session joins the hub before it runs
    /// `chat_request`.
    pub(crate) chat_open: bool,
    /// The member left the chat screen: the session leaves the hub.
    pub(crate) chat_exit: bool,
    /// A chat request (list, join, say, whisper, ...) the chat screen is waiting on. The session
    /// runs it against the hub and the room store and gives the answer to
    /// [`Screen::chat_deliver`].
    pub(crate) chat_request: Option<ChatRequest>,
    /// A community request (a list, a post, a vote) the community screen is waiting on (M3-J).
    /// The session runs it against the store and gives the answer to
    /// [`Screen::deliver_community`].
    pub(crate) community_request: Option<CommunityRequest>,
    /// A Files request (areas, a file, the queue, an approval, ...) the Files screens are waiting
    /// on (M4-F2). The session runs it against the store and the blob store and gives the answer
    /// to [`Screen::deliver_files`].
    pub(crate) files_request: Option<russh_bbs_core::files::Request>,
    /// A forum request (board list, thread, search, post, ...) the forum screens are waiting
    /// on (M3-E). The session runs it against the store and gives the answer to
    /// [`Screen::deliver_forum`].
    pub(crate) forum_request: Option<forums::Request>,
    /// A mail request (inbox, message, send, delete, report) the mail screens are waiting on
    /// (M3-I). The session runs it against the store and the mail key and gives the answer to
    /// [`Screen::deliver_mail`].
    pub(crate) mail_request: Option<mail::Request>,
    /// A door request (open a door, write what it asked to save) the door screens are waiting on
    /// (M4-D0). The session runs it against the store and gives the answer to
    /// [`Screen::deliver_door`].
    pub(crate) door_request: Option<crate::doors::Request>,
    /// The command palette opened (UI-9): the session loads the boards the member may list and
    /// gives them to [`Screen::set_palette_boards`].
    pub(crate) palette: PaletteAsk,
    /// The member changed their widget list for a layout (UI-10): store it (`None` goes back to
    /// the board's default) with `russh_bbs_core::widgets::save`.
    pub(crate) widget_save: Option<(LayoutId, Option<Vec<WidgetId>>)>,
    /// Keys held back because `members_request` has to be answered first. The session applies
    /// them after delivering the answer, so a fast typist's next key is not lost.
    pub(crate) held_back: Vec<Key>,
}

impl KeyEffects {
    /// Fold in what a later batch asked for.
    pub(crate) fn absorb(&mut self, later: Self) {
        self.disconnect |= later.disconnect;
        self.claim_token = later.claim_token.or(self.claim_token.take());
        self.claim_password = later.claim_password.or(self.claim_password.take());
        self.save_settings = later.save_settings.or(self.save_settings.take());
        self.packs = later.packs.or(self.packs.take());
        self.menu_tools = later.menu_tools.or(self.menu_tools.take());
        self.redeem = later.redeem.or(self.redeem.take());
        self.link = later.link.or(self.link.take());
        self.link_device = later.link_device.or(self.link_device.take());
        self.recover = later.recover.or(self.recover.take());
        self.keys_request = later.keys_request;
        self.invite = later.invite.or(self.invite.take());
        self.request_invite = later.request_invite.or(self.request_invite.take());
        self.members_request = later.members_request;
        self.queue_request = later.queue_request;
        self.password_request = later.password_request;
        self.chat_open |= later.chat_open;
        self.chat_exit |= later.chat_exit;
        self.chat_request = later.chat_request;
        self.forum_request = later.forum_request;
        self.mail_request = later.mail_request;
        self.door_request = later.door_request;
        self.community_request = later.community_request;
        self.files_request = later.files_request;
        if later.palette != PaletteAsk::Nothing {
            self.palette = later.palette;
        }
        self.widget_save = later.widget_save.or(self.widget_save.take());
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
    /// The forum screens (M3-E). The member's flow waits inside and comes back when they leave.
    Forums(Box<ForumsContent>),
    /// The mail screens (M3-I). The member's flow waits inside and comes back when they leave.
    Mail(Box<MailContent>),
    /// The door screens (M4-D0). The member's flow waits inside and comes back when they leave.
    Doors(Box<DoorsContent>),
    /// A resource limit refused the session (M2-H): the honest message, then the session ends.
    /// The lines are fixed words and numbers from core's `LimitRefusal`, so they need no
    /// sanitizing.
    Limited(Vec<String>),
}

/// A member in the forums: the screens, and the flow that is given back at the main menu.
pub(crate) struct ForumsContent {
    flow: Box<MemberFlow>,
    screen: ForumsScreen,
}

/// A member in mail: the screens, and the flow that is given back at the main menu.
pub(crate) struct MailContent {
    flow: Box<MemberFlow>,
    screen: MailScreen,
}

impl Content {
    /// The member's flow, while the content is a member's session (at the menu or in the forums).
    fn flow(&self) -> Option<&MemberFlow> {
        match self {
            Self::Member(flow) => Some(flow),
            Self::Forums(session) => Some(&session.flow),
            Self::Mail(session) => Some(&session.flow),
            Self::Doors(session) => Some(&session.flow),
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => None,
        }
    }

    fn flow_mut(&mut self) -> Option<&mut MemberFlow> {
        match self {
            Self::Member(flow) => Some(flow),
            Self::Forums(session) => Some(&mut session.flow),
            Self::Mail(session) => Some(&mut session.flow),
            Self::Doors(session) => Some(&mut session.flow),
            Self::Room(_) | Self::Claim(_) | Self::Limited(_) => None,
        }
    }

    fn view(&self, ui: &Ui) -> View {
        match self {
            Self::Room(room) => room.view(),
            Self::Claim(form) => form.view(),
            Self::Member(flow) => flow.view(ui.theme_for(flow.settings()).strings()),
            Self::Forums(session) => session
                .flow
                .view(ui.theme_for(session.flow.settings()).strings()),
            Self::Mail(session) => session
                .flow
                .view(ui.theme_for(session.flow.settings()).strings()),
            Self::Doors(session) => session
                .flow
                .view(ui.theme_for(session.flow.settings()).strings()),
            Self::Limited(lines) => View {
                title: TITLE,
                heading: if lines.iter().any(|line| line == REVOKED_LINE) {
                    REVOKED_HEADING
                } else {
                    LIMIT_HEADING
                },
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
            Self::Room(_)
            | Self::Claim(_)
            | Self::Forums(_)
            | Self::Mail(_)
            | Self::Doors(_)
            | Self::Limited(_) => None,
        }
    }

    /// Whether the content is a member screen drawn in the framed layout with the status row
    /// but is not the menu (the invite screens, M2-D).
    fn is_framed_screen(&self) -> bool {
        match self {
            Self::Member(flow) => flow.is_invite_screen(),
            Self::Room(_)
            | Self::Claim(_)
            | Self::Forums(_)
            | Self::Mail(_)
            | Self::Doors(_)
            | Self::Limited(_) => false,
        }
    }

    /// The members list, a profile or the profile form, while a member is at one.
    fn members_view(&self) -> Option<ScreenView> {
        match self {
            Self::Member(flow) => flow.members_view(),
            Self::Room(_)
            | Self::Claim(_)
            | Self::Forums(_)
            | Self::Mail(_)
            | Self::Doors(_)
            | Self::Limited(_) => None,
        }
    }

    /// The community screens, while a member is at one (M3-J).
    fn community_view(&self) -> Option<ScreenView> {
        match self {
            Self::Member(flow) => flow.community_view(),
            Self::Room(_)
            | Self::Claim(_)
            | Self::Forums(_)
            | Self::Mail(_)
            | Self::Doors(_)
            | Self::Limited(_) => None,
        }
    }

    /// The sysop queue or the vouch screen, while a member is at one (M2-E).
    fn queue_view(&self) -> Option<QueueView> {
        match self {
            Self::Member(flow) => flow.queue_view(),
            Self::Room(_)
            | Self::Claim(_)
            | Self::Forums(_)
            | Self::Mail(_)
            | Self::Doors(_)
            | Self::Limited(_) => None,
        }
    }

    /// The icon tier markers are drawn in: the member's, which is ASCII until the glyph test
    /// is answered.
    fn tier(&self) -> Tier {
        self.flow()
            .map_or(Tier::Ascii, |flow| flow.settings().icon_tier())
    }

    /// How to draw: a member's settings choose color and borders; everyone else gets the
    /// plain layout (the waiting room and claim prompt predate settings and use no color).
    fn look(&self, ui: &Ui, area: Rect, depth: ColorDepth) -> Option<Look> {
        self.flow()
            .map(|flow| ui.look(flow.settings(), area, depth))
    }

    /// The layout in force for a member at `area` (their choice, or a fallback that fits).
    fn layout(&self, ui: &Ui, area: Rect) -> LayoutId {
        self.flow().map_or(LayoutId::Classic, |flow| {
            layout::effective(ui.layout_for(flow.settings()), area)
        })
    }

    /// The layout the member chose, whatever fits on screen (for the fallback note).
    fn wanted_layout(&self, ui: &Ui) -> LayoutId {
        self.flow()
            .map_or(LayoutId::Classic, |flow| ui.layout_for(flow.settings()))
    }

    /// The style the member draws with, for choosing art (UI-3).
    fn style(&self, ui: &Ui) -> Option<russh_bbs_core::terminal_settings::ThemeName> {
        self.flow().map(|flow| ui.style_for(flow.settings()))
    }

    /// The strings to draw with: the member's theme, or the default for everyone else.
    fn strings<'a>(&self, ui: &'a Ui) -> &'a StringTable {
        self.flow().map_or_else(
            || ui.strings(),
            |flow| ui.theme_for(flow.settings()).strings(),
        )
    }
}

/// Which art slot a plain member screen has: the login banner or the logoff screen.
fn art_slot_of(
    view: &russh_bbs_core::waiting_room::View,
    flow: Option<&MemberFlow>,
) -> Option<ArtSlot> {
    use russh_bbs_core::terminal_settings::strings::LOGIN_HEADING;
    if flow.is_some_and(MemberFlow::is_logged_off) {
        Some(ArtSlot::Logoff)
    } else if view.heading == LOGIN_HEADING {
        Some(ArtSlot::Login)
    } else {
        None
    }
}

/// Whether the focus layout's widget pop-over is showing (UI-2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Popover {
    Closed,
    Open,
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
    /// The effect engine (UI-8): the login intro, the menu wipe and the logoff reveal.
    fx: fx::Fx,
    /// The screen last drawn, so a change of screen starts an effect.
    fx_seen: Option<ScreenKey>,
    /// The time effects are drawn at. `None` is the real clock; tests set it so a frame is drawn
    /// for an exact moment instead of whenever the test happens to run.
    fx_clock: Option<Instant>,
    /// The focus layout's widget pop-over is open (UI-2). Toggled with Tab; any other key
    /// closes it first.
    popover: Popover,
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
    /// The chat screen, while the member is in chat (M3-F). It sits over the member's main
    /// menu: while it is `Some` every key goes to it, and the menu comes back when it is left.
    chat: Option<ChatScreen>,
    /// Chat beside the screen, when the member opened it (UI-W). While `open`, `chat` is the
    /// pane's chat rather than the whole screen.
    dock: DockState,
    /// What the transport adds to the screens (M3-H).
    transport: Transport,
    /// The mouse, the command palette, toasts and the header ticker (UI-9).
    interact: Interact,
    /// The widgets' data and the member's saved lists (UI-10).
    widgets: WidgetState,
    /// The Widgets screen, while the member has it open over the settings (UI-10).
    widgets_page: Option<WidgetsScreen>,
    /// The wipe between screens and the blinking cursor (UI-4).
    motion: Motion,
    /// The members list's highlight, sort and window (UI-4).
    members_nav: crate::ui::screens::list::Nav,
}

/// What the widget panels show and how the member arranged them (UI-10).
#[derive(Debug, Default)]
struct WidgetState {
    /// The last fetch.
    data: WidgetData,
    /// The member's saved lists, loaded when the session starts.
    choice: Choice,
    /// When `data` was fetched.
    fetched: Option<std::time::Instant>,
    /// The widgets `data` was fetched for; a different list fetches again at once.
    fetched_for: Vec<WidgetId>,
    /// The member's saved lists were read (even if the read failed and the defaults stand).
    choice_loaded: bool,
}

/// What the transport around a screen adds to it (M3-H, D27).
#[derive(Debug, Clone, Copy, Default)]
struct Transport {
    /// The row the transport reserves for its own marker, if any (telnet's `UNENCRYPTED`).
    footer: Option<Footer>,
    /// The board is in low-security mode (D27): staff screens add the `LOW-SECURITY` marker.
    low_security: bool,
    /// What the client's terminal can show, from its `TERM`, `COLORTERM` or telnet terminal
    /// type (UI-1). Sixteen colours until the client says otherwise.
    depth: Option<ColorDepth>,
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
        let interact = Interact::new(ui.board(), std::time::Instant::now());
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
            fx: fx::Fx::default(),
            fx_seen: None,
            fx_clock: None,
            popover: Popover::Closed,
            rebuild: false,
            degrade_on_leave: false,
            editor: None,
            chat: None,
            dock: DockState::default(),
            transport: Transport {
                footer,
                low_security: false,
                depth: None,
            },
            interact,
            widgets: WidgetState::default(),
            widgets_page: None,
            motion: Motion::default(),
            members_nav: crate::ui::screens::list::Nav::default(),
        })
    }

    /// Put the board's low-security mode on or off for this session's staff screens.
    pub(crate) fn set_low_security(&mut self, on: bool) {
        self.transport.low_security = on;
    }

    /// Tell the screen what the client's terminal advertised: `term` is `TERM` from the ssh
    /// `pty-req` (or the telnet terminal type) and `colorterm` is `COLORTERM` when the client
    /// sent it. The screens then draw at the depth they imply, lowered by the member's colour
    /// setting (UI-1, D109). Call before the first frame.
    pub(crate) fn set_client_terminal(&mut self, term: Option<&str>, colorterm: Option<&str>) {
        self.transport.depth = Some(ColorDepth::detect(term, colorterm));
        self.dirty = true;
    }

    /// The depth the client advertised, sixteen colours when it said nothing.
    fn depth(&self) -> ColorDepth {
        self.transport.depth.unwrap_or(ColorDepth::Ansi16)
    }

    /// Whether this session holds a staff-rank role (the ranks telnet withholds, D27).
    fn is_staff(&self) -> bool {
        use russh_bbs_core::access::Source;
        self.access().is_some_and(|access| {
            access.held().iter().any(|held| {
                matches!(&held.source, Source::Role { rank, .. } if *rank >= crate::telnet::STAFF_RANK)
            })
        })
    }

    /// The markers the status bar shows on this session's screens (FIX-M3-6): while the board
    /// is in low-security mode, `LOW-SECURITY` on every screen of a staff session and on the
    /// queue and vouch screens; none otherwise (D93).
    fn status_markers(&self) -> &'static [SecurityMarker] {
        if self.transport.low_security && (self.is_staff() || self.content.queue_view().is_some()) {
            &STAFF_MARKERS_LOW_SECURITY
        } else {
            &MENU_MARKERS
        }
    }

    /// Feed keypresses to the current content. Stops at the first key that ends the session or
    /// submits a token; keys after it are ignored. Nothing is drawn: call [`Screen::render`]
    /// once for the whole batch, so a flood of keys costs one redraw, not one per key.
    pub(crate) fn apply_keys(&mut self, keys: &[Key]) -> KeyEffects {
        if let Some(spaced) = self.paste_breaks_to_spaces(keys) {
            return self.apply_keys(&spaced);
        }
        if self.interact.palette_open() || keys.contains(&Key::Ctrl('k')) {
            return self.apply_keys_with_palette(keys);
        }
        self.apply_keys_plain(keys)
    }

    /// The batch has `Ctrl-K` in it or the palette is open: keys go to the palette while it is
    /// up, and what it runs joins the normal keys, in order, so the choice goes through the
    /// ordinary path (the menu, the permission check, the limits) as typed keys do.
    fn apply_keys_with_palette(&mut self, keys: &[Key]) -> KeyEffects {
        let mut normal: Vec<Key> = Vec::with_capacity(keys.len());
        let mut ask = PaletteAsk::Nothing;
        for key in keys.iter().copied() {
            if self.interact.palette_open() {
                match self.interact.palette_key(key) {
                    PaletteStep::Closed | PaletteStep::Open => {}
                    PaletteStep::PassThrough(key) => normal.push(key),
                    PaletteStep::Run(path) => normal.extend(path),
                }
            } else if key == Key::Ctrl('k') && self.palette_available() {
                if self.open_palette() {
                    ask = PaletteAsk::Boards;
                }
            } else {
                // Anywhere else `Ctrl-K` is an ordinary key: the screen gets it as it always
                // did (the editor, chat and the forms ignore it).

                normal.push(key);
            }
        }
        let mut effects = if normal.is_empty() {
            KeyEffects::default()
        } else {
            self.apply_keys_plain(&normal)
        };
        effects.palette = ask;
        self.dirty = true;
        effects
    }

    /// Whether `Ctrl-K` opens the palette now: a signed-in member at the main menu or one of its
    /// submenus, not in chat, not in the editor, with the help overlay down. The waiting room,
    /// the founder claim and every other screen have none.
    fn palette_available(&self) -> bool {
        !self.chat_fullscreen()
            && !self.dock_has_keys()
            && self.editor.is_none()
            && self.content.menu_page().is_some_and(|page| !page.help)
    }

    /// Build the palette from what the member's permissions allow right now and open it.
    /// `true` when the session should also refresh the board names.
    fn open_palette(&mut self) -> bool {
        let Content::Member(flow) = &self.content else {
            return false;
        };
        let Some(page) = flow.menu_page() else {
            return false;
        };
        let entries = palette::entries(&palette::Sources {
            tree: flow.menu_tree(),
            access: flow.access_snapshot(),
            strings: self.content.strings(&self.ui),
            doors: self
                .ui
                .doors()
                .all()
                .iter()
                .map(|door| door.info().title.to_owned())
                .collect(),
            boards: self.interact.boards().to_vec(),
        });
        self.interact.open_palette(entries, page.depth);
        true
    }

    /// The boards the member may list arrived for the open palette (the session asked the forum
    /// service, which has already filtered them through `core::access`).
    pub(crate) fn set_palette_boards(&mut self, names: Vec<String>) {
        self.interact
            .set_boards(names.into_iter().map(palette::BoardName).collect());
        if !self.interact.palette_open() {
            return;
        }
        let Content::Member(flow) = &self.content else {
            return;
        };
        let entries = palette::entries(&palette::Sources {
            tree: flow.menu_tree(),
            access: flow.access_snapshot(),
            strings: self.content.strings(&self.ui),
            doors: self
                .ui
                .doors()
                .all()
                .iter()
                .map(|door| door.info().title.to_owned())
                .collect(),
            boards: self.interact.boards().to_vec(),
        });
        self.interact.refresh_palette(entries);
        self.dirty = true;
    }

    /// The keys a batch of mouse reports stands for, to be applied after the typed keys of the
    /// same batch (`typing`: there were some, so the reports are dropped). Empty when the member
    /// has not turned the mouse on.
    pub(crate) fn mouse_keys(&mut self, events: &[Mouse], typing: bool) -> Vec<Key> {
        // A press on the dock's divider and motion with the button held set the pane's width
        // through the same resize the `<` and `>` keys make; everything else is a click.
        if let Some((column, body)) = self.interact.drag(events, typing) {
            self.dock_drag(column, body);
        }
        self.interact.mouse_keys(events, typing)
    }

    /// Show a toast (UI-9). Rate limited; the words come from the string table or are sanitized.
    pub(crate) fn toast(&mut self, kind: ToastKind, text: ToastText<'_>) {
        let strings = self.content.strings(&self.ui);
        if self
            .interact
            .toasts
            .push(kind, text, strings, std::time::Instant::now())
        {
            self.dirty = true;
        }
    }

    /// Time passed: expire toasts, move the ticker. Marks the screen dirty when a frame is due.
    pub(crate) fn tick(&mut self, now: std::time::Instant) {
        let visible = self.ticker_visible();
        let moving = self.motion_now();
        self.interact.ticker_mut().set_moving(moving);
        if self.interact.tick(now, visible) {
            self.dirty = true;
        }
    }

    /// When the screen next needs a frame although nobody pressed anything (a toast expiring,
    /// the ticker moving). `None`: nothing is pending, so the shell sleeps until real work.
    pub(crate) fn next_wake(&self, now: std::time::Instant) -> Option<std::time::Instant> {
        // A change waiting to be drawn (a toast just queued) needs its frame now.
        if self.dirty {
            return Some(now);
        }
        self.interact.next_wake(now, self.ticker_visible())
    }

    /// Whether the ticker shows on the screen the member is at (a signed-in member's framed
    /// screen).
    fn ticker_visible(&self) -> bool {
        self.interact.ticker().has_text() && self.content.flow().is_some()
    }

    /// Whether motion is allowed for this member now (their choice, the board's, the style's).
    fn motion_now(&self) -> bool {
        self.content
            .look(&self.ui, self.layout_area(), self.depth())
            .is_some_and(Look::motion)
    }

    /// Whether feed headlines should be fetched for the ticker now.
    pub(crate) fn ticker_headlines_due(&self, now: std::time::Instant) -> bool {
        self.content.flow().is_some() && self.interact.ticker().headlines_due(now)
    }

    /// Feed headlines for the ticker, from the boards the member may read.
    pub(crate) fn set_ticker_headlines(&mut self, subjects: &[String], now: std::time::Instant) {
        self.interact
            .ticker_mut()
            .set_headlines(subjects.iter().map(String::as_str), now);
        self.dirty = true;
    }

    fn apply_keys_plain(&mut self, keys: &[Key]) -> KeyEffects {
        let mut effects = if self.dock_keys_in(keys) || self.dock_has_keys() {
            self.apply_keys_docked(keys)
        } else {
            self.apply_keys_main(keys)
        };
        self.close_dock_if_stranded(&mut effects);
        effects
    }

    /// Keys for the screen itself (not the dock pane).
    fn apply_keys_main(&mut self, keys: &[Key]) -> KeyEffects {
        // Any key finishes a running effect at once (UI-8): the key is then handled as usual, and
        // the next frame is the static screen.
        if !keys.is_empty() && self.fx.finish() {
            self.dirty = true;
        }
        if self.focus_menu_showing() {
            // Tab opens the widget pop-over of the focus layout and closes it again; Esc and any
            // other key close it and are then handled as usual, so nothing is swallowed twice.
            let mut rest: Vec<Key> = Vec::with_capacity(keys.len());
            for key in keys.iter().copied() {
                match key {
                    Key::Ctrl('i') => {
                        self.popover = match self.popover {
                            Popover::Closed => Popover::Open,
                            Popover::Open => Popover::Closed,
                        };
                        self.dirty = true;
                    }
                    Key::Escape if self.popover == Popover::Open => {
                        self.popover = Popover::Closed;
                        self.dirty = true;
                    }
                    other => {
                        if self.popover == Popover::Open {
                            self.popover = Popover::Closed;
                            self.dirty = true;
                        }
                        rest.push(other);
                    }
                }
            }
            return self.apply_keys_inner(&rest);
        }
        if self.popover == Popover::Open {
            self.popover = Popover::Closed;
            self.dirty = true;
        }
        self.apply_keys_inner(keys)
    }

    /// Whether the member is at a menu drawn in the focus layout, where Tab means something.
    fn focus_menu_showing(&self) -> bool {
        !self.chat_fullscreen()
            && self.editor.is_none()
            && self.content.menu_page().is_some()
            && self.menu_layout() == LayoutId::Focus
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one match over every screen the session can show; splitting it hides the flow"
    )]
    fn apply_keys_inner(&mut self, keys: &[Key]) -> KeyEffects {
        if self.chat_fullscreen() {
            return self.apply_chat_keys(keys);
        }
        let mut effects = KeyEffects::default();
        if self.widgets_page.is_some() {
            self.widget_keys(keys, &mut effects);
            return effects;
        }
        if self.editor.is_some() {
            self.editor_keys(keys, &mut effects);
            self.dirty = true;
            return effects;
        }
        let area = self.layout_area();
        let mut open_editor_at = None;
        // Set when the keys moved the session into or out of the forum screens (M3-E).
        let mut moved = ForumMove::Stay;
        // The same for the mail screens (M3-I).
        let mut mail_moved = MailMove::Stay;
        // And for the door screens (M4-D0).
        let mut door_moved = DoorMove::Stay;
        match &mut self.content {
            Content::Room(room) => {
                for input in keys.iter().copied().filter_map(room_input) {
                    match room.input(input) {
                        RoomEffect::Stay => {}
                        RoomEffect::Disconnect => {
                            effects.disconnect = true;
                            break;
                        }
                        RoomEffect::LinkKey(request) => {
                            effects.link = Some(request);
                        }
                        RoomEffect::LinkDevice(request) => {
                            effects.link_device = Some(request);
                            break;
                        }
                        RoomEffect::Recover(request) => {
                            effects.recover = Some(request);
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
                        ClaimEffect::SubmitWithPassword { token, password } => {
                            effects.claim_token = Some(token);
                            effects.claim_password = Some(password);
                            break;
                        }
                    }
                }
            }
            Content::Limited(_) => effects.disconnect = true,
            Content::Forums(session) => {
                moved = forum_keys(&mut session.screen, keys, area, &mut effects);
            }
            Content::Mail(session) => {
                mail_moved = mail_keys(&mut session.screen, keys, area, &mut effects);
            }
            Content::Doors(session) => {
                door_moved = doors::keys(&mut session.screen, keys, &mut effects);
            }
            Content::Member(flow) => {
                for (index, key) in keys.iter().copied().enumerate() {
                    // On the members list, arrows move a highlight, `o` sorts and Enter opens
                    // the highlighted row by its own digit; every other key goes to core as is.
                    let key = match flow.members_view() {
                        Some(view) => members::nav_key(&mut self.members_nav, &view, key),
                        None => Some(key),
                    };
                    let Some(input) = key.and_then(member_input) else {
                        continue;
                    };
                    match flow.input(input) {
                        MemberEffect::Stay => {}
                        MemberEffect::Save(settings) => effects.save_settings = Some(settings),
                        MemberEffect::Packs(request) => {
                            effects.packs = Some(request);
                            effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                            break;
                        }
                        MemberEffect::MenuTools(request) => {
                            effects.menu_tools = Some(request);
                            effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                            break;
                        }
                        MemberEffect::Invite(request) => {
                            effects.invite = Some(request);
                            break;
                        }
                        MemberEffect::OpenChat => {
                            let mut chat = ChatScreen::open();
                            chat.set_manage(
                                flow.access_snapshot().allows(Permission::ChatManageRooms),
                            );
                            self.dock.open = false;
                            self.chat = Some(chat);
                            effects.chat_open = true;
                            effects.chat_request = Some(ChatRequest::List);
                            effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                            break;
                        }
                        MemberEffect::Forums => {
                            moved = ForumMove::Open(index);
                            break;
                        }
                        MemberEffect::Mail => {
                            mail_moved = MailMove::Open(index);
                            break;
                        }
                        MemberEffect::Doors => {
                            door_moved = DoorMove::Open(index);
                            break;
                        }
                        MemberEffect::Widgets => {
                            let start = self.ui.layout_for(flow.settings());
                            self.widgets_page = Some(WidgetsScreen::open(start));
                            self.widgets.fetched = None;
                            self.dirty = true;
                            // Keys typed after it belong to the new screen.
                            effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                            break;
                        }
                        MemberEffect::Disconnect => {
                            effects.disconnect = true;
                            break;
                        }
                    }
                    // A members screen that needs the store stops the batch: keys after it
                    // would act on a list that is about to be replaced.
                    if let Some(request) = flow.take_members_request() {
                        effects.members_request = Some(request);
                        effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                        break;
                    }
                    // The same for the queue screens (M2-E).
                    if let Some(request) = flow.take_queue_request() {
                        effects.queue_request = Some(request);
                        effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                        break;
                    }
                    // And the password screen (M3-D).
                    if let Some(request) = flow.take_password_request() {
                        effects.password_request = Some(request);
                        effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                        break;
                    }
                    // And My keys and the recovery screen (FIX-M3-11).
                    if let Some(request) = flow.take_keys_request() {
                        effects.keys_request = Some(request);
                        effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                        break;
                    }
                    // And the community screens (M3-J).
                    if let Some(request) = flow.take_community_request() {
                        effects.community_request = Some(request);
                        effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                        break;
                    }
                    // And the Files screens (M4-F2).
                    if let Some(request) = flow.take_files_request() {
                        effects.files_request = Some(request);
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
        self.forum_transitions(&mut effects, keys, moved);
        self.mail_transitions(&mut effects, keys, mail_moved);
        self.door_transitions(&mut effects, keys, door_moved);
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

    /// Keys while the chat screen is up: they all go to it. Leaving it (Esc at the room list)
    /// drops the overlay and applies the remaining keys to the menu; Ctrl-C/Ctrl-D logs off
    /// from chat like from every page.
    fn apply_chat_keys(&mut self, keys: &[Key]) -> KeyEffects {
        let mut effects = KeyEffects::default();
        let manage = self
            .access()
            .is_some_and(|access| access.allows(Permission::ChatManageRooms));
        for (index, key) in keys.iter().copied().enumerate() {
            let rest = keys.get(index + 1..).unwrap_or_default();
            let Some(chat) = self.chat.as_mut() else {
                break;
            };
            chat.set_manage(manage);
            let outcome = match key {
                Key::Ctrl('c' | 'd') => {
                    // Log off from here: leave chat, let the menu handle the quit.
                    self.drop_chat();
                    effects.chat_exit = true;
                    let later = self.apply_keys(keys.get(index..).unwrap_or_default());
                    effects.absorb(later);
                    break;
                }
                other => match chat_key(other) {
                    Some(chat_key) => chat.input(chat_key),
                    None => continue,
                },
            };
            match outcome {
                ChatEffect::Stay => {}
                ChatEffect::Exit => {
                    self.drop_chat();
                    effects.chat_exit = true;
                    let later = self.apply_keys(rest);
                    effects.absorb(later);
                    break;
                }
                ChatEffect::Request(request) => {
                    effects.chat_request = Some(request);
                    effects.held_back = rest.to_vec();
                    break;
                }
            }
        }
        self.dirty = true;
        effects
    }

    /// The answer to the chat request taken from [`KeyEffects`]. The screen shows it on the next
    /// render.
    pub(super) fn chat_deliver(&mut self, response: ChatResponse) {
        if let Some(chat) = &mut self.chat {
            chat.deliver(response);
        }
        self.dirty = true;
    }

    /// Show what the room sent (M3-F push path). Returns a request when the screen needs one.
    pub(super) fn chat_receive(&mut self, events: Vec<ChatEvent>) -> Option<ChatRequest> {
        self.dirty = true;
        self.chat.as_mut().and_then(|chat| chat.receive(events))
    }

    /// The signed-in member's handle (sanitized), for the chat hub.
    pub(super) fn member_handle(&self) -> Option<&str> {
        self.content.flow().map(MemberFlow::handle)
    }

    /// The room the chat screen shows, if it is in one.
    pub(super) fn chat_room(&self) -> Option<RoomId> {
        self.chat.as_ref().and_then(ChatScreen::room_id)
    }

    /// Keys on the Widgets screen (UI-10). Ctrl-C and Ctrl-D still log off.
    fn widget_keys(&mut self, keys: &[Key], effects: &mut KeyEffects) {
        self.dirty = true;
        for (index, key) in keys.iter().copied().enumerate() {
            if matches!(key, Key::Ctrl('c' | 'd')) {
                effects.disconnect = true;
                return;
            }
            let Some(access) = self.content.flow().map(MemberFlow::access_snapshot) else {
                self.widgets_page = None;
                return;
            };
            let ctx = widgets_screen::Ctx {
                rules: self.ui.widgets().rules(),
                choice: &self.widgets.choice,
                access,
            };
            let Some(screen) = self.widgets_page.as_mut() else {
                return;
            };
            match screen.press(key, &ctx) {
                widgets_screen::Outcome::Stay => {}
                widgets_screen::Outcome::Back => {
                    self.widgets_page = None;
                    // The rest of the batch is for the settings screen again.
                    effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                    return;
                }
                widgets_screen::Outcome::Edit {
                    choice,
                    layout,
                    list,
                } => {
                    self.widgets.choice = choice;
                    effects.widget_save = Some((layout, list));
                }
            }
        }
    }

    /// The member's saved widget lists, read when the session starts and after a save.
    pub(crate) fn set_widget_choice(&mut self, choice: Choice) {
        self.widgets.choice = choice;
        self.widgets.choice_loaded = true;
        self.dirty = true;
    }

    /// Whether the member's saved lists have been read.
    pub(crate) fn widget_choice_loaded(&self) -> bool {
        self.widgets.choice_loaded
    }

    /// The widgets the layout in force shows this member, in order: their saved list or the
    /// board's default, within the sysop's locks and what they may reach. Empty outside the menu
    /// and for the layouts that draw no widget panes.
    fn widgets_shown(&self, layout: LayoutId) -> Vec<WidgetId> {
        let Some(access) = self.access() else {
            return Vec::new();
        };
        self.ui
            .widgets()
            .rules()
            .resolve(layout, &self.widgets.choice, access)
    }

    /// Which widgets the screen needs data for now, if a fetch is due: the menu in a pane layout
    /// (or the Widgets screen, which previews everything on offer) with data older than the
    /// smallest refresh budget among the widgets drawn, or none yet. `None` when nothing shown
    /// needs data, so a classic menu never wakes the store.
    pub(crate) fn widgets_due(&self, now: std::time::Instant) -> Option<Vec<WidgetId>> {
        let wanted = self.widgets_wanted();
        if wanted.is_empty() {
            return None;
        }
        let stale = match (self.widgets.fetched, widgets::refresh_interval(&wanted)) {
            (Some(at), Some(every)) => {
                now.saturating_duration_since(at) >= every || self.widgets.fetched_for != wanted
            }
            _ => true,
        };
        stale.then_some(wanted)
    }

    /// When the next timer-driven refresh is due, if widgets are on screen. The shell wakes at
    /// this instant (no sooner than [`widgets::MIN_REFRESH`] after the last fetch).
    pub(crate) fn widgets_deadline(&self) -> Option<std::time::Instant> {
        let wanted = self.widgets_wanted();
        let every = widgets::refresh_interval(&wanted)?;
        self.widgets.fetched.map(|at| at + every)
    }

    /// The widgets whose data the screen in front of the member draws.
    fn widgets_wanted(&self) -> Vec<WidgetId> {
        if self.chat_fullscreen() || self.editor.is_some() {
            return Vec::new();
        }
        if self.widgets_page.is_some() {
            return self
                .access()
                .map(|access| self.ui.widgets().rules().offered(access))
                .unwrap_or_default()
                .into_iter()
                .map(|(id, _)| id)
                .collect();
        }
        if self.content.menu_page().is_none() {
            return Vec::new();
        }
        // The layout the member chose, not the one the terminal fits: a dashboard too big for
        // the window collapses into a line of figures, and the figures need the data.
        self.widgets_shown(self.content.wanted_layout(&self.ui))
    }

    /// Turn accessibility mode on or off (M5 sets this): motion is always off in it.
    #[allow(
        dead_code,
        reason = "M5 builds accessibility mode; tests drive it today"
    )]
    pub(crate) fn set_accessible(&mut self, on: bool) {
        self.motion.set_accessible(on);
        self.dirty = true;
    }

    /// Whether a text input line is on screen, so its cursor blinks.
    fn has_input(&self) -> bool {
        if let Some(chat) = &self.chat {
            let chat_input = match chat.page() {
                russh_bbs_core::chat::ChatPage::Room(_) => true,
                russh_bbs_core::chat::ChatPage::List(list) => list.naming.is_some(),
            };
            // Beside a screen, either side may have a text field.
            return chat_input || (self.dock.open && self.main_has_input());
        }
        self.main_has_input()
    }

    /// The member's own screen has a text field open (the part of [`Screen::has_input`] that
    /// is not the chat).
    fn main_member_input(&self) -> bool {
        match &self.content {
            Content::Member(_) => self
                .content
                .members_view()
                .or_else(|| self.content.community_view())
                .is_some_and(|view| {
                    view.lines.iter().any(|line| {
                        matches!(
                            line,
                            russh_bbs_core::profiles::Line::Field { active: true, .. }
                        )
                    })
                }),
            Content::Forums(_)
            | Content::Mail(_)
            | Content::Room(_)
            | Content::Claim(_)
            | Content::Doors(_)
            | Content::Limited(_) => false,
        }
    }

    /// When the shell should next wake for motion (a wipe frame or a cursor blink), if
    /// anything is moving. `None` with motion off, which is the common case.
    pub(crate) fn motion_deadline(&self) -> Option<std::time::Instant> {
        let look = self
            .content
            .look(&self.ui, self.layout_area(), self.depth())?;
        self.motion.deadline(look, std::time::Instant::now())
    }

    /// If a motion deadline has passed, ask for a redraw.
    pub(crate) fn motion_tick(&mut self, now: std::time::Instant) {
        let due = self
            .content
            .look(&self.ui, self.layout_area(), self.depth())
            .is_some_and(|look| self.motion.blink_due(look, now));
        if due {
            self.dirty = true;
        }
    }

    /// Note whether an input is on screen for the blink, and say whether the cursor is in its
    /// dark half now. (The wipe between menus is UI-8's effect engine.)
    fn motion_frame(&mut self) -> bool {
        let Some(look) = self
            .content
            .look(&self.ui, self.layout_area(), self.depth())
        else {
            return false;
        };
        let now = std::time::Instant::now();
        let input = self.has_input();
        self.motion.set_input(input, now);
        let dark = self.motion.cursor_dark(look, now);
        self.motion.shown(dark);
        dark
    }

    /// Whether the screen in front of the member draws the split layout's chat and mail panes,
    /// which need the newest messages as well as the widgets' data.
    pub(crate) fn wants_side_panes(&self) -> bool {
        !self.dock_active()
            && !self.chat_fullscreen()
            && self.content.menu_page().is_some()
            && self.menu_layout() == LayoutId::Split
    }

    /// The previous sign-in time, the "since your last call" mark.
    pub(crate) fn last_login(&self) -> Option<russh_bbs_core::store::UnixSeconds> {
        self.content.flow().and_then(MemberFlow::last_login)
    }

    /// Give the screen freshly fetched widget data for `for_widgets`.
    pub(crate) fn deliver_widgets(&mut self, data: WidgetData, for_widgets: Vec<WidgetId>) {
        self.widgets.data = data;
        self.widgets.fetched = Some(std::time::Instant::now());
        self.widgets.fetched_for = for_widgets;
        self.dirty = true;
    }

    /// The doors this board runs, for the doors widget.
    pub(crate) fn doors(&self) -> &crate::doors::Doors {
        self.ui.doors()
    }

    /// What the signed-in session holds, for the front end's own checks. `None` outside a
    /// member session.
    /// The shared screens (the shell hands them to a blocking job).
    pub(crate) fn ui_handle(&self) -> Arc<Ui> {
        Arc::clone(&self.ui)
    }

    /// A theme-pack request finished: show the listing and what happened (UI-7).
    pub(crate) fn packs_done(
        &mut self,
        listing: russh_bbs_core::terminal_settings::PackListing,
        message: Option<&str>,
    ) {
        if let Some(flow) = self.content.flow_mut() {
            flow.packs_done(listing, message);
            self.dirty = true;
        }
    }

    /// A menus-and-pages request finished: show the listing and what happened (UI-6).
    pub(crate) fn menu_tools_done(
        &mut self,
        listing: russh_bbs_core::menu::MenuToolsListing,
        message: Option<&str>,
    ) {
        if let Some(flow) = self.content.flow_mut() {
            flow.menu_tools_done(listing, message);
            self.dirty = true;
        }
    }

    pub(crate) fn access(&self) -> Option<&Snapshot> {
        self.content.flow().map(MemberFlow::access_snapshot)
    }

    /// Replace what a member session holds, for the next action (D33).
    pub(crate) fn set_access(&mut self, access: Snapshot) {
        if let Some(chat) = &mut self.chat {
            chat.set_manage(access.allows(Permission::ChatManageRooms));
        }
        if let Some(flow) = self.content.flow_mut() {
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

    /// Carry out a move into or out of the forum screens that the keys asked for.
    fn forum_transitions(&mut self, effects: &mut KeyEffects, keys: &[Key], moved: ForumMove) {
        match moved {
            ForumMove::Stay => {}
            ForumMove::Open(index) => {
                effects.forum_request = Some(self.open_forums());
                effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
            }
            ForumMove::Leave => self.close_forums(),
            ForumMove::LogOff(save) => {
                self.close_forums();
                if let Content::Member(flow) = &mut self.content {
                    // Ctrl-C and Ctrl-D end the session from anywhere (D54): the flow shows its
                    // goodbye. The editor's changed draft is stored first.
                    let _ = flow.input(MemberInput::Quit);
                }
                effects.disconnect = true;
                effects.forum_request = save.map(forums::Request::SaveDraft);
            }
        }
    }

    /// Carry out a move into or out of the mail screens that the keys asked for.
    fn mail_transitions(&mut self, effects: &mut KeyEffects, keys: &[Key], moved: MailMove) {
        match moved {
            MailMove::Stay => {}
            MailMove::Open(index) => {
                effects.mail_request = Some(self.open_mail());
                effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
            }
            MailMove::Leave => self.close_mail(),
            MailMove::LogOff(save) => {
                self.close_mail();
                if let Content::Member(flow) = &mut self.content {
                    // Ctrl-C and Ctrl-D end the session from anywhere (D54). The editor's
                    // changed draft is stored first.
                    let _ = flow.input(MemberInput::Quit);
                }
                effects.disconnect = true;
                effects.mail_request = save.map(mail::Request::SaveDraft);
            }
        }
    }

    /// Move the member into the mail screens; the request loads the inbox.
    fn open_mail(&mut self) -> mail::Request {
        let placeholder = Content::Limited(Vec::new());
        match std::mem::replace(&mut self.content, placeholder) {
            Content::Member(flow) => {
                let (screen, request) = MailScreen::open(*flow.settings());
                self.content = Content::Mail(Box::new(MailContent { flow, screen }));
                self.rebuild = true;
                request
            }
            other => {
                self.content = other;
                mail::Request::Inbox { page: 0 }
            }
        }
    }

    /// Give the member back to the main menu.
    fn close_mail(&mut self) {
        let placeholder = Content::Limited(Vec::new());
        match std::mem::replace(&mut self.content, placeholder) {
            Content::Mail(session) => {
                self.content = Content::Member(session.flow);
                self.rebuild = true;
            }
            other => self.content = other,
        }
    }

    /// The answer to the mail request taken from [`KeyEffects`]. Returns a follow-up request
    /// when the answer calls for one (the inbox again after a send or a delete).
    pub(super) fn deliver_mail(&mut self, response: mail::Response) -> Option<mail::Request> {
        self.dirty = true;
        match &mut self.content {
            Content::Mail(session) => session.screen.deliver(response),
            Content::Room(_)
            | Content::Claim(_)
            | Content::Member(_)
            | Content::Forums(_)
            | Content::Doors(_)
            | Content::Limited(_) => None,
        }
    }

    /// The mail keys the board loaded at start-up, for running mail requests (`None` on a
    /// board built without them: every mail request then says mail is not available).
    pub(super) fn mail_keys(&self) -> Option<Arc<russh_bbs_core::mail::MailKeys>> {
        self.ui.mail_keys()
    }

    /// The blob store, scanner and SFTP switch the board runs with (`None` on a board built
    /// without them: the Files screens then say file areas are not available).
    pub(super) fn file_services(&self) -> Option<Arc<crate::sftp::FileServices>> {
        self.ui.files()
    }

    /// Move the member into the forum screens; the request loads the board list.
    fn open_forums(&mut self) -> forums::Request {
        let placeholder = Content::Limited(Vec::new());
        match std::mem::replace(&mut self.content, placeholder) {
            Content::Member(flow) => {
                let (screen, request) = ForumsScreen::open(*flow.settings());
                self.content = Content::Forums(Box::new(ForumsContent { flow, screen }));
                self.rebuild = true;
                request
            }
            other => {
                self.content = other;
                forums::Request::Boards
            }
        }
    }

    /// Give the member back to the main menu.
    fn close_forums(&mut self) {
        let placeholder = Content::Limited(Vec::new());
        match std::mem::replace(&mut self.content, placeholder) {
            Content::Forums(session) => {
                self.content = Content::Member(session.flow);
                self.rebuild = true;
            }
            other => self.content = other,
        }
    }

    /// The answer to the forum request taken from [`KeyEffects`]. Returns a follow-up request
    /// when the answer calls for one (the list again after a removal).
    pub(super) fn deliver_forum(&mut self, response: forums::Response) -> Option<forums::Request> {
        self.dirty = true;
        match &mut self.content {
            Content::Forums(session) => session.screen.deliver(response),
            Content::Room(_)
            | Content::Claim(_)
            | Content::Member(_)
            | Content::Mail(_)
            | Content::Doors(_)
            | Content::Limited(_) => None,
        }
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

    /// The answer to the keys request taken from [`KeyEffects`]. The screen shows it on the next
    /// render.
    pub(crate) fn deliver_keys(&mut self, response: KeysResponse) {
        if let Content::Member(flow) = &mut self.content {
            flow.deliver_keys(response);
        }
        self.dirty = true;
    }

    /// The answer to the community request taken from [`KeyEffects`]. The screen shows it on the
    /// next render.
    pub(crate) fn deliver_community(&mut self, response: CommunityResponse) {
        if let Content::Member(flow) = &mut self.content {
            flow.deliver_community(response);
        }
        self.dirty = true;
    }

    /// The answer to the Files request taken from [`KeyEffects`]; the next request (a reload after
    /// a change), if the screen asks for one, comes from [`Screen::take_files_request`].
    pub(crate) fn deliver_files(&mut self, response: russh_bbs_core::files::Response) {
        if let Content::Member(flow) = &mut self.content {
            flow.deliver_files(response);
        }
        self.dirty = true;
    }

    /// The Files request the screen asked for after an answer, once.
    pub(crate) fn take_files_request(&mut self) -> Option<russh_bbs_core::files::Request> {
        match &mut self.content {
            Content::Member(flow) => flow.take_files_request(),
            _ => None,
        }
    }

    /// The community request the screen asked for after an answer (an emptied poll page asks for
    /// the first one), once.
    pub(crate) fn take_community_request(&mut self) -> Option<CommunityRequest> {
        match &mut self.content {
            Content::Member(flow) => flow.take_community_request(),
            Content::Room(_)
            | Content::Claim(_)
            | Content::Forums(_)
            | Content::Mail(_)
            | Content::Doors(_)
            | Content::Limited(_) => None,
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

    /// Draw the open editor, if there is one. `true` when it was drawn.
    fn draw_editor(&mut self) -> io::Result<bool> {
        let markers = self.status_markers();
        let (Some(editor), Content::Member(flow)) = (&mut self.editor, &self.content) else {
            return Ok(false);
        };
        let editor = editor.get_mut().unwrap_or_else(PoisonError::into_inner);
        let view = EditorView {
            ui: &self.ui,
            settings: flow.settings(),
            markers,
        };
        self.terminal.draw(|frame| editor.render(frame, &view))?;
        Ok(true)
    }

    /// The queue request the screen asked for after an answer (a decision asks for the list
    /// again), once.
    pub(crate) fn take_queue_request(&mut self) -> Option<QueueRequest> {
        match &mut self.content {
            Content::Member(flow) => flow.take_queue_request(),
            Content::Room(_)
            | Content::Claim(_)
            | Content::Forums(_)
            | Content::Mail(_)
            | Content::Doors(_)
            | Content::Limited(_) => None,
        }
    }

    /// The submitted token failed. The screen shows the one generic message on the next render;
    /// the bool is whether the session should now end (attempts used up).
    pub(crate) fn claim_rejected(&mut self) -> bool {
        let disconnect = match &mut self.content {
            Content::Claim(form) => form.rejected() == ClaimEffect::Disconnect,
            Content::Room(_)
            | Content::Member(_)
            | Content::Forums(_)
            | Content::Mail(_)
            | Content::Doors(_)
            | Content::Limited(_) => false,
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

    /// The open message editor's unsaved draft, if there is one to keep (FIX-M3-7, D134): what
    /// a lost connection and the periodic autosave store. Only the forum editor has a draft
    /// context. The mail editor has none on purpose (D78 (5), D90: a stored draft would be
    /// readable beside mail kept sealed), and neither has the poll description editor, so
    /// their text is not stored.
    pub(crate) fn unsaved_draft(&self) -> Option<crate::ui::editor::DraftSave> {
        match &self.content {
            Content::Forums(session) => session.screen.pending_draft(),
            _ => None,
        }
    }

    /// Whether a message editor is open (the poll description editor, or one in forums or
    /// mail). Only the editor turns a pasted line break into a new line.
    fn editor_open(&self) -> bool {
        self.editor.is_some()
            || match &self.content {
                Content::Forums(session) => session.screen.editor_open(),
                Content::Mail(session) => session.screen.editor_open(),
                _ => false,
            }
    }

    /// Characters the key parser dropped because they are not ASCII (FIX-M3-7, D134): say so
    /// once, in a toast, so a name like `café` that arrives as `caf` is not a silent surprise.
    pub(crate) fn note_dropped_text(&mut self) {
        self.toast(ToastKind::Notice, ToastText::Key("toast.ascii_only"));
    }

    /// In a single-line input a pasted line break is one space (FIX-M3-7, D134): a run of
    /// breaks is one space, and the break never presses Enter. Everywhere else, and in the
    /// message editor (which keeps lines), the keys are unchanged.
    fn paste_breaks_to_spaces(&self, keys: &[Key]) -> Option<Vec<Key>> {
        if !keys.contains(&Key::PasteBreak) || self.editor_open() || !self.has_input() {
            return None;
        }
        let mut out = Vec::with_capacity(keys.len());
        let mut after_break = false;
        for key in keys.iter().copied() {
            if key == Key::PasteBreak {
                if !after_break {
                    out.push(Key::Char(' '));
                }
                after_break = true;
            } else {
                after_break = false;
                out.push(key);
            }
        }
        Some(out)
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
        // A resize repaints everything; an effect drawn for the old size ends (UI-8).
        self.fx.finish();
        self.rebuild = true;
    }

    /// When the next effect frame is due, if one is running. The shell sleeps until then; with
    /// no effect running this is `None` and no timer is armed (motion off costs nothing).
    pub(crate) fn next_fx_due(&self) -> Option<Instant> {
        self.fx.next_due()
    }

    /// The client is slow to read (a frame took too long to hand over): end any effect and run
    /// no more this session, so no animation frame waits in front of the next key (UI-8).
    pub(crate) fn slow_link(&mut self) {
        self.fx.disable();
        self.dirty = true;
    }

    /// Advance the effects to `now`; a frame of the running effect (or the static frame after it
    /// ended) is then drawn by the next [`Screen::render`].
    #[cfg(test)]
    pub(crate) fn tick_fx(&mut self, now: Instant) {
        self.tick_fx_gated(now, true);
    }

    /// [`Screen::tick_fx`] with the link's say (FIX-M35-3): with `open` false the effect frame
    /// that is due is dropped instead of drawn, because the client has not yet taken the last
    /// ones. The static frame that ends an effect is drawn whatever `open` says.
    pub(crate) fn tick_fx_gated(&mut self, now: Instant, open: bool) {
        if self.fx.tick_gated(now, open) {
            self.dirty = true;
        }
    }

    /// Whether an effect is running, so the next frame drawn is one of its frames. The shell
    /// uses this to probe the client for an acknowledgement (FIX-M35-3).
    pub(crate) fn fx_running(&self) -> bool {
        self.fx.is_running()
    }

    /// Start the effect a change of screen calls for (UI-8). The login screen gets the intro, a
    /// menu entered from anywhere gets a short wipe; every other screen is static (the logoff
    /// goodbye is typed out by [`Screen::leave_typed`] instead, on the main screen). Nothing starts when motion is off, the terminal is too small for
    /// the intro, or the session's frame budget is spent.
    fn start_fx(&mut self, view: &View, menu: Option<&MenuPage>, look: Option<Look>, now: Instant) {
        let key = match (&self.content, menu) {
            (Content::Member(_), Some(page)) => ScreenKey::Menu(page.title, page.depth),
            (Content::Member(_), None) if view.heading == LOGIN_HEADING => ScreenKey::Login,
            (Content::Member(_), None)
                if self.content.members_view().is_some()
                    || self.content.community_view().is_some()
                    || self.content.queue_view().is_some() =>
            {
                ScreenKey::Framed
            }
            (Content::Forums(_), _) => ScreenKey::Forums,
            (Content::Mail(_), _) => ScreenKey::Mail,
            _ => ScreenKey::Other,
        };
        self.start_fx_for(key, look, now);
    }

    /// The effect for `key`, if it is a change of screen. The sweep is colour only (UI-8), so it
    /// is as safe on a table as on a menu; the gate, the budgets and the slow-link stop are the
    /// engine's.
    fn start_fx_for(&mut self, key: ScreenKey, look: Option<Look>, now: Instant) {
        if self.fx_seen == Some(key) {
            return;
        }
        self.fx_seen = Some(key);
        let kind = match key {
            ScreenKey::Login => Some(fx::Kind::Intro),
            ScreenKey::Menu(..) | ScreenKey::Forums | ScreenKey::Mail | ScreenKey::Framed => {
                Some(fx::Kind::Wipe)
            }
            ScreenKey::Other => None,
        };
        let size_ok = kind != Some(fx::Kind::Intro)
            || (self.viewport.width >= fx::intro::MIN_SIZE.0
                && self.viewport.height >= fx::intro::MIN_SIZE.1);
        // `accessible` is the M5 plain path (`Screen::set_accessible`); nothing sets it yet.
        let accessible = self.motion.accessible();
        let allowed = look.is_some_and(|look| fx::allowed(look, accessible)) && size_ok;
        match kind {
            Some(kind) => {
                self.fx.start(kind, now, allowed);
            }
            None => {
                self.fx.finish();
            }
        }
    }

    /// For the forum and mail screens: start the sweep when they were just entered and return
    /// the frame to draw (`None` when no effect runs, which is the static screen).
    fn early_fx(&mut self) -> Option<(fx::Kind, f32)> {
        let (key, settings) = match &self.content {
            Content::Forums(session) => (ScreenKey::Forums, *session.flow.settings()),
            Content::Mail(session) => (ScreenKey::Mail, *session.flow.settings()),
            _ => return None,
        };
        let now = self.fx_clock.unwrap_or_else(Instant::now);
        let look = self.ui.look(&settings, self.layout_area(), self.depth());
        self.start_fx_for(key, Some(look), now);
        self.fx.frame(now)
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
            let frame = self.draw()?;
            // An effect that has sent its byte budget ends; the next frame is the static one.
            if self.fx.charge(frame.len()) {
                self.dirty = true;
            }
            return Ok(Some(frame));
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
        // Mouse reporting off first, while the alternate screen is still up.
        let mouse_off = self.interact.restore();
        self.interact.close_palette();
        queue!(self.terminal.backend_mut(), Show, LeaveAlternateScreen)?;
        let mut bytes = mouse_off.to_vec();
        bytes.extend(self.take_output());
        bytes.extend_from_slice(self.farewell_text().as_bytes());
        Ok(bytes)
    }

    /// The goodbye printed on the main screen after the screen is left, one `\r\n` per line.
    fn farewell_text(&self) -> String {
        let farewell = match &self.content {
            Content::Member(flow) => flow.farewell(self.content.strings(&self.ui)),
            Content::Limited(lines) => Some(lines.clone()),
            Content::Room(_)
            | Content::Claim(_)
            | Content::Forums(_)
            | Content::Mail(_)
            | Content::Doors(_) => None,
        };
        let mut text = String::new();
        for line in farewell.into_iter().flatten() {
            text.push_str(&line);
            text.push_str("\r\n");
        }
        text
    }

    /// Like [`Screen::leave`], but the goodbye of a member who logged off is cut into a few
    /// chunks the shell prints one tick apart, so it types itself out (UI-8). `None` when motion
    /// is off, effects are off for the session, the goodbye is short of words or the budget is
    /// spent: the caller then uses [`Screen::leave`]. The chunks put together are exactly the
    /// text `leave` would print, so the last state is the static one, and the caller prints the
    /// rest at once the moment a key arrives.
    pub(crate) fn leave_typed(&mut self) -> io::Result<Option<Farewell>> {
        let Content::Member(_) = &self.content else {
            return Ok(None);
        };
        let text = self.farewell_text();
        let look = self
            .content
            .look(&self.ui, self.layout_area(), self.depth());
        let accessible = self.motion.accessible();
        let allowed = look.is_some_and(|look| fx::allowed(look, accessible));
        if text.is_empty() || !allowed {
            return Ok(None);
        }
        let now = self.fx_clock.unwrap_or_else(Instant::now);
        if !self.fx.reserve(FAREWELL_CHUNKS, now, allowed) {
            return Ok(None);
        }
        // Mouse reporting off first, as in `leave`.
        let mouse_off = self.interact.restore();
        self.interact.close_palette();
        queue!(self.terminal.backend_mut(), Show, LeaveAlternateScreen)?;
        let mut restore = mouse_off.to_vec();
        restore.extend(self.take_output());
        // Cut on character boundaries, never inside a line break.
        let chars: Vec<char> = text.chars().collect();
        let size = chars.len().div_ceil(usize::from(FAREWELL_CHUNKS)).max(1);
        let mut chunks: Vec<Vec<u8>> = Vec::new();
        let mut at = 0;
        while at < chars.len() {
            let mut end = (at + size).min(chars.len());
            if end < chars.len() && chars[end - 1] == '\r' && chars[end] == '\n' {
                end += 1;
            }
            chunks.push(chars[at..end].iter().collect::<String>().into_bytes());
            at = end;
        }
        Ok(Some(Farewell {
            restore,
            chunks,
            interval: self.fx.interval(),
        }))
    }

    /// Draw the frame, keeping what the widgets recorded as clickable for the mouse, with the
    /// header ticker in place and the client's mouse mode brought in line with the member's
    /// setting.
    fn draw(&mut self) -> io::Result<Vec<u8>> {
        let now = std::time::Instant::now();
        let ascii = self.content.tier() == Tier::Ascii;
        let moving = self.motion_now();
        self.interact.ticker_mut().set_moving(moving);
        let view = if self.ticker_visible() {
            self.interact.ticker().view(now, ascii)
        } else {
            None
        };
        let (drawn, hits) =
            interact::capture_hits(|| interact::with_ticker(view, || self.draw_screen()));
        let mut bytes = drawn?;
        self.interact.set_hits(hits);
        let wanted = self
            .content
            .flow()
            .is_some_and(|flow| interact::mouse_allowed(flow.settings()));
        let strings = self.content.strings(&self.ui);
        let toggle = self.interact.sync_mouse(wanted, strings, now);
        if !toggle.is_empty() {
            // Before the footer row the transport paints, so that row is still the last thing
            // drawn.
            let at = bytes.len() - self.footer_len(&bytes);
            bytes.splice(at..at, toggle.iter().copied());
            // A toast announces the change; it appears with the next frame.
            self.dirty = true;
        }
        Ok(bytes)
    }

    /// How many trailing bytes of `bytes` are the transport's footer row (zero without one).
    fn footer_len(&self, bytes: &[u8]) -> usize {
        let Some(footer) = self.transport.footer else {
            return 0;
        };
        let tail = chrome::footer(
            footer.marker,
            self.content.tier(),
            footer.window.cols(),
            footer.window.rows(),
        );
        if bytes.ends_with(&tail) {
            tail.len()
        } else {
            0
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one match over every screen the session can show; splitting it hides the flow"
    )]
    fn draw_screen(&mut self) -> io::Result<Vec<u8>> {
        if let Some(frame) = self.draw_doors()? {
            return Ok(frame);
        }
        let clock = clock_text(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
        );
        let cursor_dark = self.motion_frame();
        // The forum and mail screens draw before the common path below, so their sweep (UI-8
        // on the UI-4 screens) is started here and applied last in their own draw.
        let early_fx = self.early_fx();
        let area_art = self
            .content
            .style(&self.ui)
            .map(|style| self.ui.art_ctx(style));
        if let Content::Forums(session) = &self.content {
            let settings = session.flow.settings();
            let theme = self.ui.theme_for(settings);
            let ctx = DrawCtx {
                ui: &self.ui,
                strings: theme.strings(),
                settings,
                look: self
                    .ui
                    .look(settings, self.layout_area(), self.depth())
                    .with_cursor_dark(cursor_dark),
                tier: settings.icon_tier(),
                markers: self.status_markers(),
                clock: Some(clock.as_str()),
                art: area_art.as_ref(),
            };
            let tier = ctx.tier;
            let interact = &self.interact;
            let pane = self
                .chat
                .as_ref()
                .filter(|_| self.dock_active())
                .map(|chat| {
                    DockPane::new(
                        chat,
                        crate::chat::ChatCtx {
                            strings: ctx.strings,
                            look: ctx.look,
                            tier: ctx.tier,
                            markers: ctx.markers,
                            clock: ctx.clock,
                            art: None,
                        },
                        self.dock_has_keys(),
                    )
                });
            let request = self.dock_request();
            let title_of_board = self.ui.board().name();
            let (drawn, _) = crate::ui::dock::with(request, || {
                self.terminal.draw(|frame| {
                    forums::render(frame, &session.screen, &ctx);
                    if let Some(pane) = &pane {
                        pane.draw(frame, crate::ui::dock::reserved());
                    }
                    interact.overlay(frame, ctx.look, ctx.strings);
                    backdrop::finish(frame, ctx.look);
                    if let Some((kind, t)) = early_fx {
                        fx::Overlay {
                            kind,
                            t,
                            look: ctx.look,
                            title: title_of_board,
                        }
                        .apply_with(frame, None);
                    }
                })
            });
            drawn?;
            return Ok(self.finish_frame(tier));
        }
        if let Content::Mail(session) = &self.content {
            let settings = session.flow.settings();
            let theme = self.ui.theme_for(settings);
            let ctx = mail::DrawCtx {
                ui: &self.ui,
                strings: theme.strings(),
                settings,
                look: self
                    .ui
                    .look(settings, self.layout_area(), self.depth())
                    .with_cursor_dark(cursor_dark),
                tier: settings.icon_tier(),
                markers: self.status_markers(),
                clock: Some(clock.as_str()),
                art: area_art.as_ref(),
            };
            let tier = ctx.tier;
            let interact = &self.interact;
            let pane = self
                .chat
                .as_ref()
                .filter(|_| self.dock_active())
                .map(|chat| {
                    DockPane::new(
                        chat,
                        crate::chat::ChatCtx {
                            strings: ctx.strings,
                            look: ctx.look,
                            tier: ctx.tier,
                            markers: ctx.markers,
                            clock: ctx.clock,
                            art: None,
                        },
                        self.dock_has_keys(),
                    )
                });
            let request = self.dock_request();
            let title_of_board = self.ui.board().name();
            let (drawn, _) = crate::ui::dock::with(request, || {
                self.terminal.draw(|frame| {
                    mail::render(frame, &session.screen, &ctx);
                    if let Some(pane) = &pane {
                        pane.draw(frame, crate::ui::dock::reserved());
                    }
                    interact.overlay(frame, ctx.look, ctx.strings);
                    backdrop::finish(frame, ctx.look);
                    if let Some((kind, t)) = early_fx {
                        fx::Overlay {
                            kind,
                            t,
                            look: ctx.look,
                            title: title_of_board,
                        }
                        .apply_with(frame, None);
                    }
                })
            });
            drawn?;
            return Ok(self.finish_frame(tier));
        }
        // While the editor is open it is drawn instead of the content (the transport's footer
        // row below still follows it).
        let drew_editor = self.draw_editor()?;
        let mut view = self.content.view(&self.ui);
        // Effects (UI-8): a change of screen starts one, and the one running (if any) is applied
        // last in the draw closure. Done before anything borrows the UI for the frame.
        let look = self
            .content
            .look(&self.ui, self.layout_area(), self.depth())
            .map(|look| look.with_cursor_dark(cursor_dark));
        let menu = self.content.menu_page();
        let now = self.fx_clock.unwrap_or_else(Instant::now);
        self.start_fx(&view, menu.as_ref(), look, now);
        let overlay = look.and_then(|look| {
            self.fx.frame(now).map(|(kind, t)| fx::Overlay {
                kind,
                t,
                look,
                title: self.ui.board().name(),
            })
        });
        let art_ctx = self
            .content
            .style(&self.ui)
            .map(|style| self.ui.art_ctx(style));
        let intro_banner = art_ctx
            .as_ref()
            .and_then(|ctx| ctx.login_banner(look.is_some_and(Look::ascii)));
        let art_slot = art_slot_of(&view, self.content.flow());
        if art_slot == Some(ArtSlot::Logoff) {
            // The board's own parting words, when the sysop wrote some (the clock picks one).
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() / 60);
            if let Some(quote) = self.ui.board().quote_for(seed) {
                view.body.push(String::new());
                view.body.push(quote.as_str().to_owned());
            }
        }
        let layout = self.menu_layout();
        let wanted = self.content.wanted_layout(&self.ui);
        // The community screens (M3-J) produce the members screens' view and are drawn the same.
        let members_view = self
            .content
            .members_view()
            .or_else(|| self.content.community_view());
        let queue_view = self.content.queue_view();
        let preview = self.content.flow().and_then(|flow| flow.preview().cloned());
        let gallery = self.content.flow().and_then(|flow| flow.gallery().cloned());
        let packs = self.content.flow().and_then(|flow| flow.packs().cloned());
        // Sysop pages and the Menus and pages screen (UI-6).
        let bulletin = self
            .content
            .flow()
            .and_then(|flow| flow.bulletin().map(|b| (b.id(), b.scroll())));
        let menu_tools = self
            .content
            .flow()
            .and_then(|flow| flow.menu_tools().cloned());
        let menu_setup = (bulletin.is_some() || menu_tools.is_some()).then(|| self.ui.menus());
        let page_extent = std::cell::Cell::new(0_usize);
        let member_settings = self.content.flow().map(|flow| *flow.settings());
        let tier = self.content.tier();
        let strings = self.content.strings(&self.ui);
        let framed = self.content.is_framed_screen();
        let chat = self.chat.as_ref().filter(|_| self.chat_fullscreen());
        let side_chat = self.chat.as_ref().filter(|_| self.dock_active());
        let dock_focused = self.dock_has_keys();
        let dock_request = self.dock_request();
        let interact = &self.interact;
        // Staff sessions add the low-security marker to every screen while the board is in that
        // mode (D27, FIX-M3-6); the status bar's marker cell draws it.
        let staff_markers = self.status_markers();
        let clock = Some(clock.as_str());
        let now_secs = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
        )
        .unwrap_or(0);
        let shown = self.widgets_shown(layout);
        let widget_view = WidgetView {
            shown: &shown,
            data: &self.widgets.data,
            settings: self.ui.widgets(),
            now: now_secs,
        };
        let members_nav = &self.members_nav;
        let widgets_screen_state = self.widgets_page.as_ref();
        let widget_access = self.content.flow().map(MemberFlow::access_snapshot);
        let widget_choice = &self.widgets.choice;
        if !drew_editor {
            let (drawn, _) = crate::ui::dock::with(dock_request, || {
                self.terminal.draw(|frame| {
                    match (look, &menu) {
                        // The Widgets screen (UI-10), over the settings.
                        (Some(look), _)
                            if widgets_screen_state.is_some() && widget_access.is_some() =>
                        {
                            if let (Some(screen), Some(access)) =
                                (widgets_screen_state, widget_access)
                            {
                                widgets_screen::render(
                                    frame,
                                    screen,
                                    &widgets_screen::DrawIn {
                                        strings,
                                        look,
                                        tier,
                                        markers: staff_markers,
                                        clock,
                                        ctx: widgets_screen::Ctx {
                                            rules: self.ui.widgets().rules(),
                                            choice: widget_choice,
                                            access,
                                        },
                                        data: &self.widgets.data,
                                        board: self.ui.board(),
                                        settings: self.ui.widgets(),
                                        now: now_secs,
                                    },
                                );
                            }
                        }
                        // The chat screen (M3-F): the same box and status row, with the room's lines.
                        (Some(look), _) if chat.is_some() => {
                            if let Some(screen) = chat {
                                crate::chat::render(
                                    frame,
                                    screen,
                                    &crate::chat::ChatCtx {
                                        strings,
                                        look,
                                        tier,
                                        markers: staff_markers,
                                        clock,
                                        art: art_ctx.as_ref(),
                                    },
                                );
                            }
                        }
                        // The member's style gallery and the sysop's Theme packs screen (UI-7).
                        (Some(look), None) if gallery.is_some() || packs.is_some() => {
                            if let Some(settings) = &member_settings {
                                let ctx = crate::ui::style_gallery::GalleryCtx {
                                    ui: &self.ui,
                                    strings,
                                    look,
                                    tier,
                                    markers: staff_markers,
                                    clock,
                                    settings,
                                };
                                if let Some(gallery) = &gallery {
                                    crate::ui::style_gallery::render(frame, gallery, &ctx);
                                } else if let Some(packs) = &packs {
                                    crate::ui::style_gallery::render_packs(frame, packs, &ctx);
                                }
                            }
                        }
                        // A sysop page (UI-6), or the Menus and pages screen under Sysop tools.
                        (Some(look), None) if bulletin.is_some() || menu_tools.is_some() => {
                            if let Some(setup) = &menu_setup {
                                let ctx = crate::ui::sysop_menus::render::DrawCtx {
                                    strings,
                                    look,
                                    tier,
                                    markers: staff_markers,
                                    clock,
                                };
                                let extent = match (&bulletin, &menu_tools) {
                                    (Some((id, scroll)), _) => {
                                        crate::ui::sysop_menus::render::render_bulletin(
                                            frame, setup, id, *scroll, &ctx,
                                        )
                                    }
                                    (None, Some(tools)) => {
                                        crate::ui::sysop_menus::render::render_tools(
                                            frame, tools, setup, &ctx,
                                        )
                                    }
                                    (None, None) => 0,
                                };
                                page_extent.set(extent);
                            }
                        }
                        // The sysop's theme preview (UI-3): a sample screen in a chosen style and size.
                        (Some(look), None) if preview.is_some() => {
                            if let Some(preview) = &preview {
                                crate::ui::art::preview::render(
                                    frame,
                                    preview,
                                    &crate::ui::art::preview::PreviewCtx {
                                        ui: &self.ui,
                                        strings,
                                        look,
                                        tier,
                                        markers: staff_markers,
                                        clock,
                                    },
                                );
                            }
                        }
                        // The sysop queue and the vouch screen (M2-E): the same layout and status row. The
                        // requester's text is drawn in the content box only.
                        (Some(look), None) if queue_view.is_some() => {
                            if let Some(screen) = &queue_view {
                                let width =
                                    usize::from(frame.area().width.min(settings::MAX_WIDTH))
                                        .saturating_sub(2);
                                classic::render_content(
                                    frame,
                                    ContentScreen {
                                        strings,
                                        lines: sysop::queue::lines(screen, strings, look, width),
                                        look,
                                        tier,
                                        markers: staff_markers,
                                        clock,
                                    },
                                );
                            }
                        }
                        // A members screen: the same classic layout and status row, with its own lines.
                        (Some(look), None)
                            if members_view.as_ref().is_some_and(members::is_list) =>
                        {
                            if let Some(screen) = &members_view {
                                members::render_list(
                                    frame,
                                    screen,
                                    &members::ListCtx {
                                        strings,
                                        look,
                                        tier,
                                        markers: staff_markers,
                                        clock,
                                        nav: members_nav,
                                    },
                                );
                            }
                        }
                        (Some(look), None) if members_view.is_some() => {
                            if let Some(screen) = &members_view {
                                members::panes::render(
                                    frame,
                                    screen,
                                    &members::panes::ViewCtx {
                                        strings,
                                        look,
                                        tier,
                                        markers: staff_markers,
                                        clock,
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
                                markers: staff_markers,
                                clock,
                                mail_over_ssh_only: false,
                                board: self.ui.board(),
                                wanted,
                                popover: self.popover == Popover::Open,
                                art: art_ctx.as_ref(),
                                widgets: widget_view,
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
                                markers: staff_markers,
                                clock,
                            },
                        ),
                        // Login, glyph test, settings and the password and key screens: the status bar
                        // (hints and the marker cell) under the box; no header, so the settings screen
                        // keeps its 22 rows at 80x24.
                        (Some(look), None) => {
                            let hints = classic::content_hints(strings);
                            settings::render_framed(
                                frame,
                                &view,
                                &settings::Framed {
                                    strings,
                                    look,
                                    tier,
                                    markers: staff_markers,
                                    hints: &hints,
                                    art: art_ctx.as_ref().zip(art_slot),
                                },
                            );
                        }
                        (None, _) => render(frame, &view),
                    }
                    if let (Some(look), Some(chat)) = (look, side_chat) {
                        // Chat beside the screen (UI-W): into what the frame reserved, below the
                        // bars, so the status bar and its marker cell are never touched.
                        DockPane::new(
                            chat,
                            crate::chat::ChatCtx {
                                strings,
                                look,
                                tier,
                                markers: staff_markers,
                                clock,
                                art: None,
                            },
                            dock_focused,
                        )
                        .draw(frame, crate::ui::dock::reserved());
                    }
                    if let Some(look) = look {
                        interact.overlay(frame, look, strings);
                        backdrop::finish(frame, look);
                    }
                    // Last: an effect rewrites the finished frame; with none running the frame is
                    // exactly the static screen (UI-8).
                    if let Some(overlay) = overlay {
                        overlay.apply_with(frame, intro_banner);
                    }
                })
            });
            drawn?;
            // Tell the open sysop page how far it can scroll, so the next key stops at the end.
            if let Some(flow) = self.content.flow_mut() {
                if flow.bulletin().is_some() {
                    flow.bulletin_extent(page_extent.get());
                } else if flow.menu_tools().is_some() {
                    flow.menu_tools_extent(page_extent.get());
                }
            }
        }
        Ok(self.finish_frame(tier))
    }

    /// The bytes of the frame just drawn, followed by the transport's marker row if it has one.
    /// Every screen ends here (the forum and mail screens too), so none can leave the row bare.
    fn finish_frame(&mut self, tier: Tier) -> Vec<u8> {
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
        bytes
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
    if let Some(flow) = content.flow_mut() {
        flow.set_themes(ui.allowed_styles());
        flow.set_layouts(ui.allowed_layouts());
        flow.set_board_defaults(ui.board_default_layout(), ui.board_default_style());
    }
}

/// Give a member session the real terminal size, so its settings screen can say when a stored
/// size override is cut down to fit (D64). Other content has no overrides.
fn tell_flow_the_terminal(content: &mut Content, pty: PtySize) {
    if let Some(flow) = content.flow_mut() {
        flow.set_terminal(pty.cols(), pty.rows());
    }
}

/// The size to draw at: the client's, with a member's width and height overrides applied.
fn drawn_size(pty: PtySize, content: &Content) -> PtySize {
    match content {
        Content::Member(_) | Content::Forums(_) | Content::Mail(_) | Content::Doors(_) => {
            content.flow().map_or(pty, |flow| {
                let (cols, rows) = flow.settings().size(pty.cols(), pty.rows());
                PtySize::from_request(u32::from(cols), u32::from(rows))
            })
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

/// How many pieces a typed goodbye is cut into (about a second at the effect interval).
const FAREWELL_CHUNKS: u16 = 12;

/// A goodbye to type out: restore the terminal, then print the chunks one `interval` apart.
#[derive(Debug)]
pub(crate) struct Farewell {
    /// Leave the alternate screen and show the cursor.
    pub(crate) restore: Vec<u8>,
    /// The goodbye, in order; together exactly what [`Screen::leave`] prints.
    pub(crate) chunks: Vec<Vec<u8>>,
    /// The time between chunks.
    pub(crate) interval: std::time::Duration,
}

/// Which screen was last drawn, for starting an effect when it changes (UI-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenKey {
    Login,
    /// A menu: its title key and depth (the cursor is not part of it).
    Menu(&'static str, usize),
    /// The forum screens, from the menu to the way back (moving inside them is not a change).
    Forums,
    /// The mail screens, likewise.
    Mail,
    /// A members, community or queue screen, from the menu to the way back.
    Framed,
    Other,
}

/// What a batch of keys did to the forum screens' place in the session (M3-E).
enum ForumMove {
    /// Nothing: the keys stayed where they were.
    Stay,
    /// The menu chose Forums at this key.
    Open(usize),
    /// The forums asked to go back to the main menu.
    Leave,
    /// The member logged off from inside the forums, with the editor's changed draft (if any)
    /// to store first.
    LogOff(Option<crate::ui::editor::DraftSave>),
}

/// What a batch of keys did to the mail screens' place in the session (M3-I).
enum MailMove {
    /// Nothing: the keys stayed where they were.
    Stay,
    /// The menu chose Mail at this key.
    Open(usize),
    /// Mail asked to go back to the main menu.
    Leave,
    /// The member logged off from inside mail, with the editor's changed draft (if any) to
    /// store first.
    LogOff(Option<crate::ui::editor::DraftSave>),
}

/// Feed `keys` to the mail screens until one needs the store (the rest are held back, like the
/// other screens) or leaves.
fn mail_keys(
    screen: &mut MailScreen,
    keys: &[Key],
    area: Rect,
    effects: &mut KeyEffects,
) -> MailMove {
    for (index, key) in keys.iter().copied().enumerate() {
        match screen.input(key, area) {
            MailEffect::Stay => {}
            MailEffect::Exit => return MailMove::Leave,
            MailEffect::Request(request) => {
                effects.mail_request = Some(request);
                effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                break;
            }
            MailEffect::LogOff(save) => return MailMove::LogOff(save),
        }
    }
    MailMove::Stay
}

/// Feed `keys` to the forum screens until one needs the store (the rest are held back, like the
/// other screens) or leaves.
fn forum_keys(
    screen: &mut ForumsScreen,
    keys: &[Key],
    area: Rect,
    effects: &mut KeyEffects,
) -> ForumMove {
    for (index, key) in keys.iter().copied().enumerate() {
        match screen.input(key, area) {
            ForumEffect::Stay => {}
            ForumEffect::Exit => return ForumMove::Leave,
            ForumEffect::Request(request) => {
                effects.forum_request = Some(request);
                effects.held_back = keys.get(index + 1..).unwrap_or_default().to_vec();
                break;
            }
            ForumEffect::LogOff(save) => return ForumMove::LogOff(save),
        }
    }
    ForumMove::Stay
}

/// Translate a keypress for the room. Keys the room has no use for are dropped.
fn room_input(key: Key) -> Option<RoomInput> {
    match key {
        Key::Char(c) => Some(RoomInput::Char(c)),
        Key::Enter => Some(RoomInput::Enter),
        Key::Backspace => Some(RoomInput::Backspace),
        Key::Escape => Some(RoomInput::Escape),
        Key::Ctrl('c' | 'd') => Some(RoomInput::Quit),
        Key::Ctrl(_)
        | Key::Up
        | Key::Down
        | Key::Left
        | Key::Right
        | Key::Home
        | Key::End
        | Key::Delete
        | Key::PageUp
        | Key::PageDown
        | Key::PasteBreak
        | Key::Other => None,
    }
}

/// Translate a keypress for the chat screen. Keys it has no use for are dropped.
fn chat_key(key: Key) -> Option<ChatKey> {
    match key {
        Key::Char(c) => Some(ChatKey::Char(c)),
        Key::Enter => Some(ChatKey::Enter),
        Key::Backspace => Some(ChatKey::Backspace),
        Key::Escape => Some(ChatKey::Escape),
        Key::Up => Some(ChatKey::Up),
        Key::Down => Some(ChatKey::Down),
        // A pasted line break must not send the line (FIX-M3-1, D88); Left, Right, Home, End
        // and the rest do nothing in the one-line chat input.
        Key::Ctrl(_)
        | Key::Left
        | Key::Right
        | Key::Home
        | Key::End
        | Key::Delete
        | Key::PageUp
        | Key::PageDown
        | Key::PasteBreak
        | Key::Other => None,
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
        Key::Ctrl(_)
        | Key::Up
        | Key::Down
        | Key::Left
        | Key::Right
        | Key::Home
        | Key::End
        | Key::Delete
        | Key::PageUp
        | Key::PageDown
        | Key::PasteBreak
        | Key::Other => None,
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
        Key::Left
        | Key::Right
        | Key::Home
        | Key::End
        | Key::Delete
        | Key::PageUp
        | Key::PageDown
        | Key::PasteBreak
        | Key::Ctrl(_)
        | Key::Other => None,
    }
}

mod dock;
mod doors;
use doors::{DoorMove, DoorsContent};

#[cfg(test)]
mod dock_tests;
#[cfg(test)]
mod fx_tests;
#[cfg(test)]
mod marker_tests;
#[cfg(test)]
mod tests;
