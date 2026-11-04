//! The signed-in session as a state machine: login screen, the glyph test on first login, the
//! main menu, the terminal settings screen and logoff (M1-E, M2-A).
//!
//! The home page is the menu framework's (`crate::menu`, M2-A): the flow routes keys to a
//! [`MenuSession`] and carries out the [`Action`] it returns. The screens around it (login,
//! glyph test, settings, logoff) stay here and still use `terminal_settings::strings`.
//! **Universal keys:** Ctrl-C / Ctrl-D log off from every page; Esc is back on the menu, the
//! settings screen and the glyph test; `?` is help on menus. The login, glyph test and number
//! entry pages are not menus, and `?` there would be input or nothing, so it is not offered.

use super::glyph::{GlyphAnswer, GlyphProgress, GlyphTest};
use super::layout::{LayoutId, ThemeName};
use super::settings::{SettingsError, TerminalSettings};
use super::strings::{self, TextSource, fill};
use super::{format_utc, tier_name};
use crate::access::{Permission, PermissionCheck, Slug, Snapshot};
use crate::community::{
    CommunityScreen, EditorAsk, Request as CommunityRequest, Response as CommunityResponse, Section,
};
use crate::icons::Tier;
use crate::invites::{InviteOutcome, InviteRequest, InviteScreen};
use crate::keys::{
    KeysKey, KeysScreen, RecoveryScreen, Request as KeysRequest, Response as KeysResponse,
    Step as KeysStep,
};
use crate::menu::{Action, MenuKey, MenuOutcome, MenuPage, MenuSession, main_tree};
use crate::passwords::{
    LoginMethod, PasswordKey, PasswordScreen, Request as PasswordRequest,
    Response as PasswordResponse, Step as PasswordStep, strings as password_strings,
};
use crate::profiles::{MembersScreen, Request, Response, ScreenKey, ScreenView, Step};
use crate::queue::{
    QueueKey, QueueMode, QueueScreen, QueueStep, QueueView, Request as QueueRequest,
    Response as QueueResponse,
};
use crate::sanitize::{SingleLine, USERNAME_MAX_CHARS, Username};
use crate::store::UnixSeconds;
use crate::waiting_room::{FieldView, MenuItemView, Notice, NoticeKind, View};

/// Most digits a size field accepts. Anything longer is out of range anyway.
const MAX_NUMBER_CHARS: usize = 4;

/// What the person did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberInput {
    /// A printable character.
    Char(char),
    /// Enter.
    Enter,
    /// Backspace.
    Backspace,
    /// Escape.
    Escape,
    /// The up arrow: the cursor of a layout that has one.
    Up,
    /// The down arrow.
    Down,
    /// Ctrl-C / Ctrl-D: log off from anywhere.
    Quit,
}

/// What the front end must do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberEffect {
    /// Redraw.
    Stay,
    /// Store these settings, then redraw. The flow already holds them.
    Save(TerminalSettings),
    /// Carry out an invite request with the store, then report it with
    /// [`MemberFlow::invite_done`]. Nothing is shown until then (M2-D).
    Invite(InviteRequest),
    /// Open the chat screen (M3-F). The front end owns the chat screen and its live state; the
    /// flow stays at the main menu underneath and is shown again when chat is left.
    OpenChat,
    /// The member chose Forums: the front end hands the session to the forum screens (M3-E)
    /// and gives the flow back when they end. The flow stays on its home page meanwhile.
    Forums,
    /// The member chose Mail: the front end hands the session to the mail screens (M3-I) and
    /// gives the flow back when they end, like [`MemberEffect::Forums`].
    Mail,
    /// Redraw (the logoff screen is showing) and close the session.
    Disconnect,
}

#[derive(Debug)]
enum Page {
    Login,
    GlyphTest(GlyphTest),
    Home,
    /// The members list, a profile or the profile form (M2-F).
    Members(Box<MembersScreen>),
    /// The sysop queue or the vouch screen (M2-E).
    Queue(Box<QueueScreen>),
    /// One-liners, the graffiti wall, last callers or the polls (M3-J).
    Community(Box<CommunityScreen>),
    Settings,
    /// The password screen, opened from the settings (M3-D).
    Password(Box<PasswordScreen>),
    /// My keys, opened from the settings (FIX-M3-11).
    Keys(Box<KeysScreen>),
    /// The staff recovery-code screen, under Sysop tools (FIX-M3-11).
    Recovery(Box<RecoveryScreen>),
    EditWidth(String),
    EditHeight(String),
    /// An invite screen (create, list, tree). Not `PartialEq`: it may hold a one-time code.
    Invites(InviteScreen),
    Logoff,
}

/// One signed-in session.
#[derive(Debug)]
pub struct MemberFlow {
    handle: String,
    access: Snapshot,
    last_login: Option<UnixSeconds>,
    settings: TerminalSettings,
    page: Page,
    notice: Option<Notice>,
    /// Where the person is in the menu tree. Kept while the settings screen is open, so
    /// "back" returns to the same menu.
    menu: MenuSession,
    /// The real terminal's size, when the front end has told us (it does at start and on every
    /// window change). Only used to say when an override is cut down to fit.
    terminal: Option<(u16, u16)>,
    /// The themes the server has loaded, in the order "next theme" cycles. Empty until the
    /// front end sets them; then the theme key does nothing.
    themes: Vec<ThemeName>,
    /// How the session signed in. A password session must give the current password again to
    /// set or clear one (M3-D, D81).
    login: LoginMethod,
    /// True where mail is offered. Off where mail is withheld: the welcome then says
    /// "Mail is available over ssh." D93 retired the D78 mail-at-rest sentence everywhere.
    mail_note: bool,
    /// Lines about key changes since the last sign-in, shown on the login screen (FIX-M3-11).
    key_notices: Vec<String>,
    /// Whether the session is over telnet, which has no key to bind (the code page says so).
    telnet: bool,
    /// The sysop turned SSH password login off; the Password screen says so.
    password_login_off: bool,
}

impl MemberFlow {
    /// Start at the login screen. The handle is sanitized again here, so the screen never
    /// relies on the caller having done it.
    #[must_use]
    pub fn new(
        handle: &Username,
        access: Snapshot,
        last_login: Option<UnixSeconds>,
        settings: TerminalSettings,
    ) -> Self {
        Self {
            handle: SingleLine::new(USERNAME_MAX_CHARS)
                .sanitize(handle.as_str())
                .into_string(),
            access,
            last_login,
            settings,
            page: Page::Login,
            notice: None,
            menu: MenuSession::new(main_tree()),
            terminal: None,
            themes: Vec::new(),
            login: LoginMethod::Key,
            mail_note: true,
            key_notices: Vec::new(),
            telnet: false,
            password_login_off: false,
        }
    }

    /// Say "Mail is available over ssh." (the transport withholds mail).
    #[must_use]
    pub const fn without_mail_note(mut self) -> Self {
        self.mail_note = false;
        self
    }

    /// Mark the session as signed in with a password instead of a key (M3-D, D28).
    #[must_use]
    pub const fn signed_in_with_password(mut self) -> Self {
        self.login = LoginMethod::Password;
        self
    }

    /// Show these lines on the login screen: what changed in the account's keys since the last
    /// sign-in (FIX-M3-11, [`crate::keys::notice_lines`]). Empty shows nothing.
    #[must_use]
    pub fn with_key_notices(mut self, lines: Vec<String>) -> Self {
        self.key_notices = lines;
        self
    }

    /// Tell the Password screen the sysop has turned password login off.
    #[must_use]
    pub const fn with_password_login_off(mut self) -> Self {
        self.password_login_off = true;
        self
    }

    /// Mark the session as over telnet, so a device-link code made here says it can be used from
    /// an ssh connection only.
    #[must_use]
    pub const fn over_telnet(mut self) -> Self {
        self.telnet = true;
        self
    }

    /// Tell the flow which themes exist, so the settings screen can switch between them. The
    /// front end calls this once at the start with the server's loaded set.
    pub fn set_themes(&mut self, themes: Vec<ThemeName>) {
        self.themes = themes;
    }

    /// Tell the flow how big the real terminal is, so the settings screen can say when a
    /// stored size override is larger than it and is being cut down (D64).
    pub fn set_terminal(&mut self, cols: u16, rows: u16) {
        self.terminal = Some((cols, rows));
    }

    /// The settings in force, for the renderer (tier, color, size overrides).
    #[must_use]
    pub const fn settings(&self) -> &TerminalSettings {
        &self.settings
    }

    /// The highest role the account holds, for "signed in as ...". `None` only if the store
    /// answered with an empty snapshot (it failed closed).
    #[must_use]
    pub fn top_role(&self) -> Option<&Slug> {
        self.access.top_role()
    }

    /// Replace what the session holds. The front end calls this at the start of every action
    /// with a freshly loaded snapshot (D33), so a change applies on the next action.
    pub fn set_access(&mut self, access: Snapshot) {
        self.access = access;
    }

    /// What the session holds now, for the front end's own checks (saving settings).
    #[must_use]
    pub const fn access_snapshot(&self) -> &Snapshot {
        &self.access
    }

    /// Whether the logoff screen is showing.
    #[must_use]
    pub const fn is_logged_off(&self) -> bool {
        matches!(self.page, Page::Logoff)
    }

    /// The signed-in member's handle, as the screens show it (sanitized).
    #[must_use]
    pub fn handle(&self) -> &str {
        &self.handle
    }

    /// Handle one input.
    pub fn input(&mut self, input: MemberInput) -> MemberEffect {
        if input == MemberInput::Quit {
            return self.log_off();
        }
        self.notice = None;
        if self.settings_page_open() && !self.access.allows(Permission::AccountEditSettings) {
            // The permission went away since the page opened (a role change applies on the next
            // action): leave the page instead of saving something that would be refused.
            self.page = Page::Home;
            self.notice = Some(Notice {
                kind: NoticeKind::Error,
                text: strings::NOT_PERMITTED,
            });
            return MemberEffect::Stay;
        }
        if matches!(self.page, Page::Password(_))
            && !self.access.allows(Permission::AccountSetPassword)
        {
            // Same rule: the permission went away since the screen opened.
            self.page = Page::Home;
            self.notice = Some(Notice {
                kind: NoticeKind::Error,
                text: strings::NOT_PERMITTED,
            });
            return MemberEffect::Stay;
        }
        if (matches!(self.page, Page::Keys(_)) && !self.access.allows(Permission::KeysManage))
            || (matches!(self.page, Page::Recovery(_))
                && !self.access.allows(Permission::KeysRecover))
        {
            // Same rule: the permission went away since the screen opened.
            self.page = Page::Home;
            self.notice = Some(Notice {
                kind: NoticeKind::Error,
                text: strings::MEMBERS_NOT_PERMITTED,
            });
            return MemberEffect::Stay;
        }
        if let Page::Members(screen) = &self.page
            && !screen.still_permitted(&self.access)
        {
            // Same rule as the settings page: the permission went away since the screen opened.
            self.page = Page::Home;
            self.notice = Some(Notice {
                kind: NoticeKind::Error,
                text: strings::MEMBERS_NOT_PERMITTED,
            });
            return MemberEffect::Stay;
        }
        if let Page::Queue(screen) = &self.page
            && !screen.still_permitted(&self.access)
        {
            // Same rule again: the permission went away since the screen opened.
            self.page = Page::Home;
            self.notice = Some(Notice {
                kind: NoticeKind::Error,
                text: strings::MEMBERS_NOT_PERMITTED,
            });
            return MemberEffect::Stay;
        }
        if let Page::Community(screen) = &self.page
            && !screen.still_permitted(&self.access)
        {
            // Same rule again: the permission went away since the screen opened.
            self.page = Page::Home;
            self.notice = Some(Notice {
                kind: NoticeKind::Error,
                text: strings::MEMBERS_NOT_PERMITTED,
            });
            return MemberEffect::Stay;
        }
        match &mut self.page {
            Page::Login => {
                self.page = if self.settings.needs_glyph_test() {
                    Page::GlyphTest(GlyphTest::new(true))
                } else {
                    Page::Home
                };
                MemberEffect::Stay
            }
            Page::GlyphTest(_) => self.glyph_input(input),
            Page::Home => self.home_input(input),
            Page::Members(_) => self.members_input(input),
            Page::Queue(_) => self.queue_input(input),
            Page::Community(_) => self.community_input(input),
            Page::Settings => self.settings_input(input),
            Page::Password(_) => self.password_input(input),
            Page::Keys(_) | Page::Recovery(_) => self.keys_input(input),
            Page::EditWidth(_) | Page::EditHeight(_) => self.edit_input(input),
            Page::Invites(_) => self.invite_input(input),
            Page::Logoff => MemberEffect::Disconnect,
        }
    }

    /// Whether a page that changes the account's terminal settings is showing. The first-run
    /// glyph test is not one: it runs before the menu, and what it picks is kept for the session
    /// (the store refuses to save it without the permission).
    const fn settings_page_open(&self) -> bool {
        match &self.page {
            Page::Settings | Page::EditWidth(_) | Page::EditHeight(_) => true,
            Page::GlyphTest(test) => !test.is_first_run(),
            Page::Login
            | Page::Home
            | Page::Members(_)
            | Page::Queue(_)
            | Page::Community(_)
            | Page::Invites(_)
            | Page::Password(_)
            | Page::Keys(_)
            | Page::Recovery(_)
            | Page::Logoff => false,
        }
    }

    fn log_off(&mut self) -> MemberEffect {
        self.page = Page::Logoff;
        self.notice = None;
        MemberEffect::Disconnect
    }

    fn glyph_input(&mut self, input: MemberInput) -> MemberEffect {
        let Page::GlyphTest(test) = &mut self.page else {
            return MemberEffect::Stay;
        };
        let answer = match input {
            MemberInput::Char('y' | 'Y') => GlyphAnswer::Yes,
            MemberInput::Char('n' | 'N') => GlyphAnswer::No,
            MemberInput::Escape => GlyphAnswer::Skip,
            _ => return MemberEffect::Stay,
        };
        let first_run = test.is_first_run();
        match test.answer(answer, self.settings) {
            GlyphProgress::Next => MemberEffect::Stay,
            GlyphProgress::Cancelled => {
                self.page = Page::Settings;
                MemberEffect::Stay
            }
            GlyphProgress::Chosen(settings) => {
                self.settings = settings;
                self.page = if first_run {
                    Page::Home
                } else {
                    Page::Settings
                };
                self.notice = Some(info(chose_text(settings)));
                MemberEffect::Save(settings)
            }
        }
    }

    fn home_input(&mut self, input: MemberInput) -> MemberEffect {
        let key = match input {
            MemberInput::Char(c) => MenuKey::Char(c),
            MemberInput::Escape => MenuKey::Back,
            // The cursor keys only mean something in a layout that draws a cursor; elsewhere
            // Enter must not act on a selection the member cannot see.
            MemberInput::Up if self.settings.layout().has_cursor() => MenuKey::Up,
            MemberInput::Down if self.settings.layout().has_cursor() => MenuKey::Down,
            MemberInput::Enter if self.settings.layout().has_cursor() => MenuKey::Select,
            MemberInput::Enter
            | MemberInput::Backspace
            | MemberInput::Up
            | MemberInput::Down
            | MemberInput::Quit => {
                return MemberEffect::Stay;
            }
        };
        match self.menu.press(&self.access, key) {
            MenuOutcome::Stay => MemberEffect::Stay,
            MenuOutcome::Action(Action::Forums) => MemberEffect::Forums,
            MenuOutcome::Action(Action::Mail) => MemberEffect::Mail,
            MenuOutcome::Action(Action::TerminalSettings) => {
                self.page = Page::Settings;
                MemberEffect::Stay
            }
            MenuOutcome::Action(Action::InviteCreate) => {
                MemberEffect::Invite(InviteRequest::Create)
            }
            MenuOutcome::Action(Action::InviteList) => MemberEffect::Invite(InviteRequest::List),
            MenuOutcome::Action(Action::InviteTree) => MemberEffect::Invite(InviteRequest::Tree),
            MenuOutcome::Action(Action::SysopQueue) => {
                self.page = Page::Queue(Box::new(QueueScreen::open(
                    QueueMode::Sysop,
                    self.access.account(),
                )));
                MemberEffect::Stay
            }
            MenuOutcome::Action(Action::MyKeys) => {
                self.page = Page::Keys(Box::new(KeysScreen::open(self.telnet)));
                MemberEffect::Stay
            }
            MenuOutcome::Action(Action::RecoveryCode) => {
                self.page = Page::Recovery(Box::new(RecoveryScreen::open()));
                MemberEffect::Stay
            }
            MenuOutcome::Action(Action::VouchQueue) => {
                self.page = Page::Queue(Box::new(QueueScreen::open(
                    QueueMode::Vouch,
                    self.access.account(),
                )));
                MemberEffect::Stay
            }
            MenuOutcome::Action(Action::Chat) => MemberEffect::OpenChat,
            MenuOutcome::Action(Action::LogOff) => self.log_off(),
            MenuOutcome::Action(
                action @ (Action::Oneliners
                | Action::GraffitiWall
                | Action::LastCallers
                | Action::Polls),
            ) => {
                let section = match action {
                    Action::Oneliners => Section::Oneliners,
                    Action::GraffitiWall => Section::Graffiti,
                    Action::LastCallers => Section::LastCallers,
                    _ => Section::Polls,
                };
                self.page = Page::Community(Box::new(CommunityScreen::open(section)));
                MemberEffect::Stay
            }
            MenuOutcome::Action(
                action @ (Action::MemberList | Action::OwnProfile | Action::EditOwnProfile),
            ) => {
                if let Some(screen) = self.open_members(action) {
                    self.page = Page::Members(Box::new(screen));
                }
                MemberEffect::Stay
            }
        }
    }

    /// The members screen an action opens. `None` if the session has no account to be
    /// "own" (it cannot happen after login, and then nothing opens).
    fn open_members(&self, action: Action) -> Option<MembersScreen> {
        let own = self.access.account();
        match action {
            Action::MemberList => Some(MembersScreen::list(own)),
            Action::OwnProfile => own.map(MembersScreen::own_profile),
            Action::EditOwnProfile => own.map(MembersScreen::edit_own),
            Action::Forums
            | Action::Mail
            | Action::TerminalSettings
            | Action::Chat
            | Action::LogOff
            | Action::SysopQueue
            | Action::RecoveryCode
            | Action::MyKeys
            | Action::VouchQueue
            | Action::InviteCreate
            | Action::InviteList
            | Action::InviteTree
            | Action::Oneliners
            | Action::GraffitiWall
            | Action::LastCallers
            | Action::Polls => None,
        }
    }

    fn members_input(&mut self, input: MemberInput) -> MemberEffect {
        let key = match input {
            MemberInput::Char(c) => ScreenKey::Char(c),
            MemberInput::Enter => ScreenKey::Enter,
            MemberInput::Backspace => ScreenKey::Backspace,
            MemberInput::Escape => ScreenKey::Escape,
            MemberInput::Up | MemberInput::Down | MemberInput::Quit => return MemberEffect::Stay,
        };
        let Page::Members(screen) = &mut self.page else {
            return MemberEffect::Stay;
        };
        let step = screen.press(&self.access, key);
        self.after_members(step);
        MemberEffect::Stay
    }

    fn after_members(&mut self, step: Step) {
        match step {
            Step::Stay => {}
            Step::Leave => self.page = Page::Home,
            Step::Denied => {
                self.page = Page::Home;
                self.notice = Some(Notice {
                    kind: NoticeKind::Error,
                    text: strings::MEMBERS_NOT_PERMITTED,
                });
            }
        }
    }

    /// The members request the screen is waiting on, once. The front end runs it with
    /// `profiles::execute` and gives the answer to [`MemberFlow::deliver_members`].
    pub fn take_members_request(&mut self) -> Option<Request> {
        match &mut self.page {
            Page::Members(screen) => screen.take_request(),
            _ => None,
        }
    }

    /// Hand the answer of a members request to the screen.
    pub fn deliver_members(&mut self, response: Response) {
        let step = match &mut self.page {
            Page::Members(screen) => screen.deliver(response),
            _ => return,
        };
        self.after_members(step);
    }

    /// The members screen as a layout draws it, while one is open. `None` on every other page.
    #[must_use]
    pub fn members_view(&self) -> Option<ScreenView> {
        match &self.page {
            Page::Members(screen) => Some(screen.view(&self.access)),
            _ => None,
        }
    }

    fn queue_input(&mut self, input: MemberInput) -> MemberEffect {
        let key = match input {
            MemberInput::Char(c) => QueueKey::Char(c),
            MemberInput::Enter => QueueKey::Enter,
            MemberInput::Escape => QueueKey::Escape,
            MemberInput::Backspace | MemberInput::Up | MemberInput::Down | MemberInput::Quit => {
                return MemberEffect::Stay;
            }
        };
        let Page::Queue(screen) = &mut self.page else {
            return MemberEffect::Stay;
        };
        let step = screen.press(&self.access, key);
        self.after_queue(step);
        MemberEffect::Stay
    }

    fn after_queue(&mut self, step: QueueStep) {
        match step {
            QueueStep::Stay => {}
            QueueStep::Leave => self.page = Page::Home,
            QueueStep::Denied => {
                self.page = Page::Home;
                self.notice = Some(Notice {
                    kind: NoticeKind::Error,
                    text: strings::MEMBERS_NOT_PERMITTED,
                });
            }
        }
    }

    /// The queue request the screen is waiting on, once. The front end runs it with
    /// `queue::execute` and gives the answer to [`MemberFlow::deliver_queue`].
    pub fn take_queue_request(&mut self) -> Option<QueueRequest> {
        match &mut self.page {
            Page::Queue(screen) => screen.take_request(),
            _ => None,
        }
    }

    /// Hand the answer of a queue request to the screen.
    pub fn deliver_queue(&mut self, response: QueueResponse) {
        let step = match &mut self.page {
            Page::Queue(screen) => screen.deliver(response),
            _ => return,
        };
        self.after_queue(step);
    }

    /// The queue screen as a layout draws it, while one is open. `None` on every other page.
    #[must_use]
    pub fn queue_view(&self) -> Option<QueueView> {
        match &self.page {
            Page::Queue(screen) => Some(screen.view(&self.access)),
            _ => None,
        }
    }

    fn community_input(&mut self, input: MemberInput) -> MemberEffect {
        let key = match input {
            MemberInput::Char(c) => ScreenKey::Char(c),
            MemberInput::Enter => ScreenKey::Enter,
            MemberInput::Backspace => ScreenKey::Backspace,
            MemberInput::Escape => ScreenKey::Escape,
            MemberInput::Up | MemberInput::Down | MemberInput::Quit => return MemberEffect::Stay,
        };
        let Page::Community(screen) = &mut self.page else {
            return MemberEffect::Stay;
        };
        let step = screen.press(&self.access, key);
        self.after_members(step);
        MemberEffect::Stay
    }

    /// The community request the screen is waiting on, once. The front end runs it with
    /// `community::execute` and gives the answer to [`MemberFlow::deliver_community`].
    pub fn take_community_request(&mut self) -> Option<CommunityRequest> {
        match &mut self.page {
            Page::Community(screen) => screen.take_request(),
            _ => None,
        }
    }

    /// Hand the answer of a community request to the screen.
    pub fn deliver_community(&mut self, response: CommunityResponse) {
        let step = match &mut self.page {
            Page::Community(screen) => screen.deliver(response),
            _ => return,
        };
        self.after_members(step);
    }

    /// The editor the poll form wants opened, once (M3-B editor, drawn by the front end).
    pub fn take_community_editor(&mut self) -> Option<EditorAsk> {
        match &mut self.page {
            Page::Community(screen) => screen.take_editor(),
            _ => None,
        }
    }

    /// The editor closed: `Some(text)` is the description to keep, `None` leaves it as it was.
    pub fn deliver_community_description(&mut self, text: Option<String>) {
        if let Page::Community(screen) = &mut self.page {
            screen.deliver_description(text);
        }
    }

    /// The community screen as a layout draws it, while one is open.
    #[must_use]
    pub fn community_view(&self) -> Option<ScreenView> {
        match &self.page {
            Page::Community(screen) => Some(screen.view(&self.access)),
            _ => None,
        }
    }

    /// The front end finished an invite request: show what came of it. Any key closes the
    /// screen (a tree also pages), and a code shown once is dropped with it.
    pub fn invite_done(&mut self, outcome: InviteOutcome) {
        self.page = Page::Invites(InviteScreen::new(outcome));
        self.notice = None;
    }

    fn invite_input(&mut self, input: MemberInput) -> MemberEffect {
        let Page::Invites(screen) = &mut self.page else {
            return MemberEffect::Stay;
        };
        if let MemberInput::Char(c) = input
            && screen.page_key(c)
        {
            return MemberEffect::Stay;
        }
        // Dropping the screen drops the code it showed.
        self.page = Page::Home;
        MemberEffect::Stay
    }

    /// Whether an invite screen is showing, so the front end draws it in the framed layout
    /// with the status row.
    #[must_use]
    pub const fn is_invite_screen(&self) -> bool {
        matches!(self.page, Page::Invites(_))
    }

    /// What this session may do. The one place the menus get their access from.
    const fn access(&self) -> &Snapshot {
        &self.access
    }

    /// The main menu as a layout draws it, while the person is at it. `None` on every other
    /// page, which keep their own drawing.
    #[must_use]
    pub fn menu_page(&self) -> Option<MenuPage> {
        matches!(self.page, Page::Home).then(|| self.menu.page(self.access()))
    }

    fn settings_input(&mut self, input: MemberInput) -> MemberEffect {
        match input {
            MemberInput::Char('w' | 'W') => {
                self.page = Page::EditWidth(String::new());
                MemberEffect::Stay
            }
            MemberInput::Char('h' | 'H') => {
                self.page = Page::EditHeight(String::new());
                MemberEffect::Stay
            }
            MemberInput::Char('c' | 'C') => {
                self.save(self.settings.with_color(!self.settings.color()))
            }
            MemberInput::Char('p' | 'P') => {
                self.save(self.settings.with_pause(!self.settings.pause()))
            }
            MemberInput::Char('l' | 'L') => {
                self.save(self.settings.with_layout(self.settings.layout().next()))
            }
            MemberInput::Char('t' | 'T') => self.next_theme(),
            MemberInput::Char('i' | 'I') => {
                self.page = Page::GlyphTest(GlyphTest::new(self.settings.needs_glyph_test()));
                MemberEffect::Stay
            }
            MemberInput::Char('s' | 'S') if self.access.allows(Permission::AccountSetPassword) => {
                self.page = Page::Password(Box::new(PasswordScreen::open(
                    self.login,
                    self.password_login_off,
                )));
                MemberEffect::Stay
            }
            MemberInput::Char('b' | 'B') | MemberInput::Escape => {
                self.page = Page::Home;
                MemberEffect::Stay
            }
            _ => MemberEffect::Stay,
        }
    }

    /// Switch to the theme after the current one in the server's list (the first, if the current
    /// one is not in it). Nothing happens when the server has loaded no themes.
    fn next_theme(&mut self) -> MemberEffect {
        let current = self.settings.theme();
        let next = self
            .themes
            .iter()
            .position(|theme| *theme == current)
            .map_or(self.themes.first(), |at| {
                self.themes.get((at + 1) % self.themes.len())
            })
            .copied();
        match next {
            Some(theme) => self.save(self.settings.with_theme(theme)),
            None => MemberEffect::Stay,
        }
    }

    fn password_input(&mut self, input: MemberInput) -> MemberEffect {
        let key = match input {
            MemberInput::Char(c) => PasswordKey::Char(c),
            MemberInput::Enter => PasswordKey::Enter,
            MemberInput::Backspace => PasswordKey::Backspace,
            MemberInput::Escape => PasswordKey::Escape,
            MemberInput::Up | MemberInput::Down | MemberInput::Quit => return MemberEffect::Stay,
        };
        let Page::Password(screen) = &mut self.page else {
            return MemberEffect::Stay;
        };
        let step = screen.press(key);
        self.after_password(step);
        MemberEffect::Stay
    }

    fn after_password(&mut self, step: PasswordStep) {
        match step {
            PasswordStep::Stay => {}
            PasswordStep::Leave => self.page = Page::Settings,
            PasswordStep::Denied => {
                self.page = Page::Settings;
                self.notice = Some(Notice {
                    kind: NoticeKind::Error,
                    text: strings::NOT_PERMITTED,
                });
            }
        }
    }

    fn keys_input(&mut self, input: MemberInput) -> MemberEffect {
        let key = match input {
            MemberInput::Char(c) => KeysKey::Char(c),
            MemberInput::Enter => KeysKey::Enter,
            MemberInput::Backspace => KeysKey::Backspace,
            MemberInput::Escape => KeysKey::Escape,
            MemberInput::Up | MemberInput::Down | MemberInput::Quit => return MemberEffect::Stay,
        };
        let step = match &mut self.page {
            Page::Keys(screen) => screen.press(key),
            Page::Recovery(screen) => screen.press(key),
            _ => return MemberEffect::Stay,
        };
        self.after_keys(step);
        MemberEffect::Stay
    }

    fn after_keys(&mut self, step: KeysStep) {
        match step {
            KeysStep::Stay => {}
            KeysStep::Leave => self.page = Page::Home,
            KeysStep::Denied => {
                self.page = Page::Home;
                self.notice = Some(Notice {
                    kind: NoticeKind::Error,
                    text: strings::MEMBERS_NOT_PERMITTED,
                });
            }
        }
    }

    /// The keys request the My keys or recovery screen is waiting on, once. The front end runs
    /// it with `keys::execute` and gives the answer to [`MemberFlow::deliver_keys`].
    pub fn take_keys_request(&mut self) -> Option<KeysRequest> {
        match &mut self.page {
            Page::Keys(screen) => screen.take_request(),
            Page::Recovery(screen) => screen.take_request(),
            _ => None,
        }
    }

    /// Hand the answer of a keys request to the screen.
    pub fn deliver_keys(&mut self, response: KeysResponse) {
        let step = match &mut self.page {
            Page::Keys(screen) => screen.deliver(response),
            Page::Recovery(screen) => screen.deliver(response),
            _ => return,
        };
        self.after_keys(step);
    }

    /// The password request the screen is waiting on, once. The front end runs it with
    /// `passwords::execute` and gives the answer to [`MemberFlow::deliver_password`].
    pub fn take_password_request(&mut self) -> Option<PasswordRequest> {
        match &mut self.page {
            Page::Password(screen) => screen.take_request(),
            _ => None,
        }
    }

    /// Hand the answer of a password request to the screen.
    pub fn deliver_password(&mut self, response: PasswordResponse) {
        let step = match &mut self.page {
            Page::Password(screen) => screen.deliver(response),
            _ => return,
        };
        self.after_password(step);
    }

    fn save(&mut self, settings: TerminalSettings) -> MemberEffect {
        self.settings = settings;
        self.notice = Some(info(strings::SAVED));
        MemberEffect::Save(settings)
    }

    fn edit_input(&mut self, input: MemberInput) -> MemberEffect {
        let (is_width, typed) = match &mut self.page {
            Page::EditWidth(typed) => (true, typed),
            Page::EditHeight(typed) => (false, typed),
            _ => return MemberEffect::Stay,
        };
        match input {
            MemberInput::Char(c) if c.is_ascii_digit() => {
                if typed.len() < MAX_NUMBER_CHARS {
                    typed.push(c);
                }
                MemberEffect::Stay
            }
            MemberInput::Backspace => {
                typed.pop();
                MemberEffect::Stay
            }
            MemberInput::Escape => {
                self.page = Page::Settings;
                MemberEffect::Stay
            }
            MemberInput::Enter => {
                // Digits only and at most four of them, so the parse cannot overflow `u16`;
                // if it somehow fails the value counts as out of range.
                let value = if typed.is_empty() {
                    Ok(None)
                } else {
                    typed.parse::<u16>().map(Some).map_err(|_| ())
                };
                self.page = Page::Settings;
                let updated = match (value, is_width) {
                    (Ok(v), true) => self.settings.with_width(v),
                    (Ok(v), false) => self.settings.with_height(v),
                    (Err(()), true) => Err(SettingsError::Width),
                    (Err(()), false) => Err(SettingsError::Height),
                };
                match updated {
                    Ok(settings) => self.save(settings),
                    Err(error) => {
                        self.notice = Some(Notice {
                            kind: NoticeKind::Error,
                            text: match error {
                                SettingsError::Width => strings::ERR_WIDTH,
                                SettingsError::Height => strings::ERR_HEIGHT,
                            },
                        });
                        MemberEffect::Stay
                    }
                }
            }
            MemberInput::Char(_) | MemberInput::Up | MemberInput::Down | MemberInput::Quit => {
                MemberEffect::Stay
            }
        }
    }

    /// Show a notice (a string-table key) under the main menu until the next key: for an action
    /// the front end declines to carry out (mail over telnet, D90).
    pub fn menu_notice(&mut self, key: &'static str) {
        self.menu.set_notice(key);
    }

    /// The lines left on the client's normal screen after it leaves the alternate one, so the
    /// logoff text stays in the scrollback. `None` until the person has logged off.
    #[must_use]
    pub fn farewell(&self, words: &dyn TextSource) -> Option<Vec<String>> {
        self.is_logged_off().then(|| self.view(words).body)
    }

    /// The current page as a [`View`]. The fixed fields (title aside) hold string-table keys
    /// that the renderer resolves; lines with placeholders are filled here from `words`. Every
    /// string is chrome or the sanitized handle.
    #[must_use]
    pub fn view(&self, words: &dyn TextSource) -> View {
        let mut view = View {
            title: strings::TITLE,
            heading: "",
            banner: Vec::new(),
            body: Vec::new(),
            note: None,
            menu: Vec::new(),
            fields: Vec::new(),
            notice: self.notice,
            footer: "",
        };
        match &self.page {
            Page::Login => self.login_view(&mut view, words),
            Page::GlyphTest(test) => glyph_view(&mut view, *test, words),
            Page::Home => self.home_view(&mut view, words),
            Page::Members(_) | Page::Queue(_) | Page::Community(_) => {}
            Page::Settings => self.settings_view(&mut view, words),
            Page::EditWidth(typed) => edit_view(&mut view, strings::FIELD_WIDTH, typed),
            Page::EditHeight(typed) => edit_view(&mut view, strings::FIELD_HEIGHT, typed),
            Page::Invites(screen) => return screen.view(),
            Page::Password(screen) => return screen.view(),
            Page::Keys(screen) => return screen.view(),
            Page::Recovery(screen) => return screen.view(),
            Page::Logoff => self.logoff_view(&mut view, words),
        }
        view
    }

    fn login_view(&self, view: &mut View, words: &dyn TextSource) {
        view.heading = strings::LOGIN_HEADING;
        view.body.push(fill(
            words.text(strings::SIGNED_IN_AS),
            &[("handle", &self.handle)],
        ));
        view.body.push(match self.last_login {
            Some(at) => fill(
                words.text(strings::LAST_LOGIN),
                &[("when", &format_utc(at))],
            ),
            None => words.text(strings::FIRST_LOGIN).to_owned(),
        });
        view.body.push(String::new());
        view.body.push(self.terminal_line(words));
        // D93: no sentence about mail at rest. Only a transport that withholds mail says so.
        if self.access.allows(Permission::MailRead) && !self.mail_note {
            view.body.push(String::new());
            view.body
                .push(words.text(strings::MAIL_SSH_ONLY).to_owned());
        }
        if !self.key_notices.is_empty() {
            view.body.push(String::new());
            view.body.extend(self.key_notices.iter().cloned());
        }
        view.footer = strings::LOGIN_FOOTER;
    }

    fn terminal_line(&self, words: &dyn TextSource) -> String {
        match self.settings.chosen_tier() {
            None => words.text(strings::TERMINAL_LINE_UNSET).to_owned(),
            Some(tier) => fill(
                words.text(strings::TERMINAL_LINE),
                &[
                    ("icons", words.text(tier_name(tier))),
                    ("color", words.text(on_off(self.settings.use_color()))),
                    ("pause", words.text(on_off(self.settings.pause()))),
                ],
            ),
        }
    }

    /// The content part of the home page: who is signed in. The menu itself is
    /// [`MemberFlow::menu_page`].
    fn home_view(&self, view: &mut View, words: &dyn TextSource) {
        view.body.push(fill(
            words.text(strings::SIGNED_IN_AS),
            &[("handle", &self.handle)],
        ));
    }

    fn settings_view(&self, view: &mut View, words: &dyn TextSource) {
        let s = &self.settings;
        let number = |value: Option<u16>| {
            value.map_or_else(
                || words.text(strings::AUTOMATIC).to_owned(),
                |n| n.to_string(),
            )
        };
        view.heading = strings::SETTINGS_HEADING;
        let (cut_width, cut_height) = self.terminal.map_or((false, false), |(cols, rows)| {
            s.overrides_clamped(cols, rows)
        });
        let (cols, rows) = self.terminal.unwrap_or_default();
        let mut width = fill(
            words.text(strings::SETTING_WIDTH),
            &[("value", &number(s.width()))],
        );
        if cut_width {
            width.push_str(&fill(
                words.text(strings::CLAMPED_WIDTH),
                &[("terminal", &cols.to_string())],
            ));
        }
        let mut height = fill(
            words.text(strings::SETTING_HEIGHT),
            &[("value", &number(s.height()))],
        );
        if cut_height {
            height.push_str(&fill(
                words.text(strings::CLAMPED_HEIGHT),
                &[("terminal", &rows.to_string())],
            ));
        }
        let value = |key: &str, value: &str| fill(words.text(key), &[("value", value)]);
        view.body = vec![
            width,
            height,
            value(strings::SETTING_COLOR, words.text(on_off(s.color()))),
            value(strings::SETTING_PAUSE, words.text(on_off(s.pause()))),
            value(
                strings::SETTING_ICONS,
                words.text(s.chosen_tier().map_or(strings::NOT_CHOSEN, tier_name)),
            ),
            fill(
                words.text(strings::SETTING_LOOK),
                &[
                    ("layout", words.text(layout_name(s.layout()))),
                    ("theme", s.theme().as_str()),
                ],
            ),
        ];
        let entry = |key, label| MenuItemView { key, label };
        view.menu = vec![
            entry('w', strings::MENU_WIDTH),
            entry('h', strings::MENU_HEIGHT),
            entry('c', strings::MENU_COLOR),
            entry('p', strings::MENU_PAUSE),
            entry('l', strings::MENU_LAYOUT),
            entry('t', strings::MENU_THEME),
            entry('i', strings::MENU_ICONS),
        ];
        if self.access.allows(Permission::AccountSetPassword) {
            view.menu.push(MenuItemView {
                key: 's',
                label: password_strings::MENU_PASSWORD,
            });
        }
        view.menu.push(MenuItemView {
            key: 'b',
            label: strings::MENU_BACK,
        });
        view.footer = strings::SETTINGS_FOOTER;
    }

    fn logoff_view(&self, view: &mut View, words: &dyn TextSource) {
        view.heading = strings::LOGOFF_HEADING;
        view.body = vec![
            fill(words.text(strings::GOODBYE), &[("handle", &self.handle)]),
            words.text(strings::SETTINGS_SAVED_NOTE).to_owned(),
        ];
    }
}

/// The string-table key of a layout's name.
const fn layout_name(layout: LayoutId) -> &'static str {
    match layout {
        LayoutId::Classic => strings::LAYOUT_CLASSIC,
        LayoutId::Lightbar => strings::LAYOUT_LIGHTBAR,
        LayoutId::Compact => strings::LAYOUT_COMPACT,
    }
}

fn glyph_view(view: &mut View, test: GlyphTest, words: &dyn TextSource) {
    view.heading = strings::GLYPH_HEADING;
    view.body = vec![
        words.text(strings::GLYPH_WHY).to_owned(),
        String::new(),
        fill(
            words.text(strings::GLYPH_STYLE),
            &[("style", words.text(tier_name(test.step().tier())))],
        ),
        // Registry glyphs for this tier; the legend names them for a screen reader.
        test.samples(),
        words.text(strings::GLYPH_LEGEND).to_owned(),
        String::new(),
        words.text(strings::GLYPH_QUESTION).to_owned(),
    ];
    view.footer = if test.is_first_run() {
        strings::GLYPH_FOOTER_FIRST
    } else {
        strings::GLYPH_FOOTER_AGAIN
    };
}

fn edit_view(view: &mut View, label: &'static str, typed: &str) {
    view.heading = strings::SETTINGS_HEADING;
    view.fields = vec![FieldView {
        label,
        value: typed.to_owned(),
        active: true,
    }];
    view.footer = strings::EDIT_FOOTER;
}

const fn info(text: &'static str) -> Notice {
    Notice {
        kind: NoticeKind::Info,
        text,
    }
}

const fn on_off(on: bool) -> &'static str {
    if on { strings::ON } else { strings::OFF }
}

const fn chose_text(settings: TerminalSettings) -> &'static str {
    match settings.chosen_tier() {
        Some(Tier::NerdFont) => strings::CHOSE_NERD,
        Some(Tier::Unicode) => strings::CHOSE_UNICODE,
        Some(Tier::Ascii) | None => strings::CHOSE_ASCII,
    }
}
