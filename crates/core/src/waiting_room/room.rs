//! The waiting room as a state machine: pages, menu navigation, form input, and what to show.
//!
//! One [`WaitingRoom`] per SSH session. The front end feeds it [`RoomInput`]s and draws the
//! [`View`] it returns; it holds no rule of its own. The room is built from a [`RoomConfig`]
//! and a submission cap, and it never talks to a store: it cannot learn who is a member, so it
//! cannot reveal it.
//!
//! **Text.** Banner and rules come from the sysop and typed field values come from a stranger.
//! Both pass through the shared sanitizer ([`crate::sanitize`]) before they are stored here, so
//! every string in a [`View`] is either fixed chrome or sanitized.
//!
//! **Forms are honest.** The redeem form (M2-D) really redeems: a valid submission becomes a
//! [`RoomEffect::Redeem`] for the front end, which has the store, and the room shows one fixed
//! failure message whatever went wrong (no oracle). The request-an-invite form (M2-E) hands a
//! valid submission to the front end as [`RoomEffect::RequestInvite`] and shows **one fixed
//! answer at once**, before anything is stored: the queue may keep the request or drop it (a
//! rate limit, a full queue, a repeat), and the person cannot tell which, so the answer cannot
//! be an oracle for the board's state. The application form (`apply` mode, M2-G) does the same
//! with [`RequestKind::Application`]: one fixed answer at once, the queue keeps or drops it. The
//! create-account form (`open` mode, M2-G) is the redeem form's twin: a valid submission is a
//! [`RoomEffect::Redeem`] with [`JoinKind::Open`], and a refusal reads as one fixed sentence.
//! On a day the `open` cap is used up the front end calls [`WaitingRoom::degrade_open`] and the
//! room shows the `apply` screens with an honest notice.

use std::fmt;

use super::screens::{self, Page, QUIT_KEY};
use super::source::RoomConfig;
use super::strings;
use crate::queue::{InviteAsk, RequestKind};
use crate::sanitize::USERNAME_MAX_CHARS;
use crate::sanitize::{MultiLine, SingleLine, Username, UsernameError, sanitize_username};
use crate::store::RegistrationMode;

/// Longest banner kept, in characters (D64). Small enough that the home page's menu fits at
/// 80x24 with the banner at its limit; there is no scrolling in the waiting room.
pub const BANNER_MAX_CHARS: usize = 300;
/// Most banner lines kept (D64).
pub const BANNER_MAX_LINES: usize = 5;
/// Longest rules text kept, in characters (D64). Small enough that the rules page and its
/// footer fit at 80x24.
pub const RULES_MAX_CHARS: usize = 900;
/// Most rules lines kept (D64).
pub const RULES_MAX_LINES: usize = 10;
/// Shown at the end of banner or rules text that was cut to fit (text stored before the caps
/// existed). Fixed chrome.
const CUT_MARK: char = '\u{2026}';

/// Which sysop text a limit applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomText {
    /// The home page banner.
    Banner,
    /// The rules page.
    Rules,
}

impl RoomText {
    const fn limits(self) -> (usize, usize) {
        match self {
            Self::Banner => (BANNER_MAX_CHARS, BANNER_MAX_LINES),
            Self::Rules => (RULES_MAX_CHARS, RULES_MAX_LINES),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Banner => "banner",
            Self::Rules => "rules",
        }
    }
}

/// Sysop text that is longer than the waiting room can show. The message says what to cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TextLimitError {
    /// More characters than allowed.
    #[error("The {} is {actual} characters; the most the waiting room shows is {max}. Shorten it.", .text.name())]
    TooLong {
        /// Which text.
        text: RoomText,
        /// What was given.
        actual: usize,
        /// What is allowed.
        max: usize,
    },
    /// More lines than allowed.
    #[error("The {} is {actual} lines; the most the waiting room shows is {max}. Shorten it.", .text.name())]
    TooManyLines {
        /// Which text.
        text: RoomText,
        /// What was given.
        actual: usize,
        /// What is allowed.
        max: usize,
    },
}

/// Check sysop text against the waiting room's limits (D64). Call this where the text is set,
/// so a too-long text is refused with a reason instead of being cut when shown. Counts the
/// text as typed, before sanitizing, so a pass here never turns into a cut later.
///
/// # Errors
/// [`TextLimitError`] naming the limit that was exceeded.
pub fn check_room_text(text: RoomText, value: &str) -> Result<(), TextLimitError> {
    let (max_chars, max_lines) = text.limits();
    let chars = value.chars().count();
    if chars > max_chars {
        return Err(TextLimitError::TooLong {
            text,
            actual: chars,
            max: max_chars,
        });
    }
    let lines = value.lines().count();
    if lines > max_lines {
        return Err(TextLimitError::TooManyLines {
            text,
            actual: lines,
            max: max_lines,
        });
    }
    Ok(())
}

/// Sanitize sysop text and cut it to the limits, marking the cut with an ellipsis.
fn shown_lines(text: RoomText, raw: &str) -> Vec<String> {
    let (max_chars, max_lines) = text.limits();
    // One character past the cap tells a cut from an exact fit.
    let clean = MultiLine::new(max_chars + 1).sanitize(raw).into_string();
    let mut cut = clean.chars().count() > max_chars;
    let mut lines: Vec<String> = clean
        .chars()
        .take(max_chars)
        .collect::<String>()
        .lines()
        .map(str::to_owned)
        .collect();
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        cut = true;
    }
    if cut {
        match lines.last_mut() {
            Some(last) => last.push(CUT_MARK),
            None => lines.push(CUT_MARK.to_string()),
        }
    }
    lines
}
/// Longest free-text field, in characters.
pub const LINE_MAX_CHARS: usize = 280;
/// Longest invite code, in characters.
pub const CODE_MAX_CHARS: usize = 64;
/// Form submissions one session may make before it is disconnected (the default).
pub const DEFAULT_MAX_SUBMISSIONS: u32 = 5;

/// What the user did, as far as the room cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomInput {
    /// A printable character.
    Char(char),
    /// Enter / Return.
    Enter,
    /// Backspace or Delete.
    Backspace,
    /// A lone Escape.
    Escape,
    /// Ctrl-C or Ctrl-D: leave from anywhere.
    Quit,
}

/// How a visitor asks to become a member at once.
#[derive(Clone, PartialEq, Eq)]
pub enum JoinKind {
    /// With an invite code (`invite` mode, M2-D).
    Invite {
        /// The code as typed (sanitized and length-capped, not yet normalized or hashed).
        code: String,
    },
    /// With nothing but a handle (`open` mode, M2-G).
    Open,
}

impl fmt::Debug for JoinKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The code is a credential: never in a log or a test failure message.
        match self {
            Self::Invite { .. } => f.write_str("Invite { code: <redacted> }"),
            Self::Open => f.write_str("Open"),
        }
    }
}

/// A "become a member now" attempt the front end should make: an invite redemption or an `open`
/// account (the effect keeps the name it had when the redeem form was its only user). Built from
/// a form that passed the room's own checks (every field filled, the handle valid), so the
/// front end only needs the store.
#[derive(Clone, PartialEq, Eq)]
pub struct RedeemRequest {
    /// Which form it came from.
    pub join: JoinKind,
    /// The handle the new account will have.
    pub handle: Username,
}

impl fmt::Debug for RedeemRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedeemRequest")
            .field("join", &self.join)
            .field("handle", &self.handle)
            .finish()
    }
}

/// What the front end should do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomEffect {
    /// Redraw and keep going.
    Stay,
    /// Redraw once, then close the session.
    Disconnect,
    /// Try to redeem an invite or open an account, then call [`WaitingRoom::redeem_refused`] if
    /// it did not work.
    Redeem(RedeemRequest),
    /// Offer a request for an invite, or a membership application (the ask's
    /// [`RequestKind`]), to the queue (M2-E, M2-G). The room has already shown its one fixed
    /// answer; the front end does not report back, because nothing it learns may change what the
    /// person reads.
    RequestInvite(InviteAsk),
}

/// Whether a notice is good news or a problem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// Information, such as "not open yet".
    Info,
    /// A problem with what was typed.
    Error,
}

/// A one-line result shown under a form. Always fixed chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notice {
    /// Good or bad.
    pub kind: NoticeKind,
    /// The words.
    pub text: &'static str,
}

/// One menu line, as drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuItemView {
    /// The key to press.
    pub key: char,
    /// What it does.
    pub label: &'static str,
}

/// One form field, as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldView {
    /// The field's label (chrome).
    pub label: &'static str,
    /// What has been typed so far (sanitized).
    pub value: String,
    /// Whether keystrokes go here.
    pub active: bool,
}

/// Everything the front end needs to draw the current page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// The product name (chrome).
    pub title: &'static str,
    /// The page heading (chrome).
    pub heading: &'static str,
    /// The sysop's banner lines (sanitized). Home only.
    pub banner: Vec<String>,
    /// Body text lines (sanitized). The rules page only.
    pub body: Vec<String>,
    /// A fixed note shown after the body (chrome).
    pub note: Option<&'static str>,
    /// Menu entries. Home only; the disconnect entry is appended by [`View::quit_key`].
    pub menu: Vec<MenuItemView>,
    /// Form fields. Form pages only.
    pub fields: Vec<FieldView>,
    /// The result of the last submission.
    pub notice: Option<Notice>,
    /// Key hints (chrome).
    pub footer: &'static str,
}

impl View {
    /// The key that disconnects from the home page.
    #[must_use]
    pub const fn quit_key() -> MenuItemView {
        MenuItemView {
            key: QUIT_KEY,
            label: strings::MENU_QUIT,
        }
    }
}

/// How a field's text is limited and checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    Handle,
    Line,
    Code,
}

impl FieldKind {
    const fn max_chars(self) -> usize {
        match self {
            Self::Handle => USERNAME_MAX_CHARS,
            Self::Line => LINE_MAX_CHARS,
            Self::Code => CODE_MAX_CHARS,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FieldSpec {
    label: &'static str,
    kind: FieldKind,
}

const HANDLE: FieldSpec = FieldSpec {
    label: strings::LABEL_HANDLE,
    kind: FieldKind::Handle,
};
const ABOUT: FieldSpec = FieldSpec {
    label: strings::LABEL_ABOUT,
    kind: FieldKind::Line,
};
const CODE: FieldSpec = FieldSpec {
    label: strings::LABEL_CODE,
    kind: FieldKind::Code,
};

fn fields_of(page: Page) -> &'static [FieldSpec] {
    match page {
        Page::Home | Page::Rules => &[],
        Page::RequestInvite | Page::Apply => &[HANDLE, ABOUT],
        Page::RedeemInvite => &[CODE, HANDLE],
        Page::CreateAccount => &[HANDLE],
    }
}

/// The words for a valid form that no special case took. Every form is live, so this is the
/// same sentence the live path shows.
fn unavailable(page: Page) -> &'static str {
    match page {
        // The request form is live (M2-E) and never gets here; the words stay honest anyway.
        Page::RequestInvite => strings::REQUEST_INVITE_ANSWER,
        // The redeem form is live (M2-D) and never gets here; the words stay honest anyway.
        Page::RedeemInvite => strings::REDEEM_FAILED,
        // The apply and create-account forms are live (M2-G) and never get here either.
        Page::Apply => strings::APPLY_ANSWER,
        Page::CreateAccount => strings::CREATE_ACCOUNT_FAILED,
        // Not forms; never submitted.
        Page::Home | Page::Rules => strings::ERR_EMPTY,
    }
}

fn username_error(error: UsernameError) -> &'static str {
    match error {
        UsernameError::TooShort => strings::ERR_HANDLE_TOO_SHORT,
        UsernameError::TooLong => strings::ERR_HANDLE_TOO_LONG,
        UsernameError::DisallowedCharacter | UsernameError::BadFirstCharacter => {
            strings::ERR_HANDLE_CHARS
        }
        UsernameError::Reserved => strings::ERR_HANDLE_RESERVED,
    }
}

/// One session's waiting room.
pub struct WaitingRoom {
    mode: RegistrationMode,
    banner: Vec<String>,
    rules: Vec<String>,
    page: Page,
    values: Vec<String>,
    active: usize,
    notice: Option<Notice>,
    submissions: u32,
    max_submissions: u32,
    degraded: bool,
}

impl fmt::Debug for WaitingRoom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Typed text is not logged.
        f.debug_struct("WaitingRoom")
            .field("mode", &self.mode)
            .field("page", &self.page)
            .field("submissions", &self.submissions)
            .finish_non_exhaustive()
    }
}

impl WaitingRoom {
    /// Build a room for `config`, allowing `max_submissions` form submissions per session.
    #[must_use]
    pub fn new(config: &RoomConfig, max_submissions: u32) -> Self {
        let banner = config.banner.as_deref().unwrap_or(strings::DEFAULT_BANNER);
        let rules = config.rules.as_deref().unwrap_or(strings::DEFAULT_RULES);
        Self {
            mode: config.mode,
            banner: shown_lines(RoomText::Banner, banner),
            rules: shown_lines(RoomText::Rules, rules),
            page: Page::Home,
            values: Vec::new(),
            active: 0,
            notice: None,
            submissions: 0,
            max_submissions,
            degraded: false,
        }
    }

    /// Turn an `open` room into an `apply` room for the rest of the UTC day because the daily
    /// cap on new accounts is used up (M2-G, D2). `with_notice` adds the honest notice to the
    /// home and application pages; pass `false` when the cap could not be read and the room
    /// simply takes the stricter path. Does nothing in any other mode, so a room can never be
    /// loosened by it.
    pub fn degrade_open(&mut self, with_notice: bool) {
        if self.mode == RegistrationMode::Open {
            self.mode = RegistrationMode::Apply;
            self.degraded = with_notice;
            self.go(Page::Home);
        }
    }

    /// The registration mode this room was built for.
    #[must_use]
    pub fn mode(&self) -> RegistrationMode {
        self.mode
    }

    /// The page being shown.
    #[must_use]
    pub fn page(&self) -> Page {
        self.page
    }

    /// Apply one input.
    pub fn input(&mut self, input: RoomInput) -> RoomEffect {
        if input == RoomInput::Quit {
            return RoomEffect::Disconnect;
        }
        match self.page {
            Page::Home => self.home_input(input),
            Page::Rules => {
                if matches!(input, RoomInput::Escape | RoomInput::Enter) {
                    self.go(Page::Home);
                }
                RoomEffect::Stay
            }
            _ => self.form_input(input),
        }
    }

    fn home_input(&mut self, input: RoomInput) -> RoomEffect {
        let RoomInput::Char(key) = input else {
            return RoomEffect::Stay;
        };
        if key.eq_ignore_ascii_case(&QUIT_KEY) {
            return RoomEffect::Disconnect;
        }
        if let Some(entry) = screens::entry_for(self.mode, key) {
            self.go(entry.page);
        }
        RoomEffect::Stay
    }

    fn form_input(&mut self, input: RoomInput) -> RoomEffect {
        match input {
            RoomInput::Escape => self.go(Page::Home),
            RoomInput::Char(c) => self.type_char(c),
            RoomInput::Backspace => {
                if let Some(value) = self.values.get_mut(self.active) {
                    value.pop();
                }
            }
            RoomInput::Enter => return self.enter(),
            RoomInput::Quit => return RoomEffect::Disconnect,
        }
        RoomEffect::Stay
    }

    fn type_char(&mut self, c: char) {
        let Some(spec) = fields_of(self.page).get(self.active) else {
            return;
        };
        let Some(value) = self.values.get_mut(self.active) else {
            return;
        };
        // Re-sanitize the whole buffer on each key: it enforces the length cap and keeps the
        // buffer free of controls, escapes and private-use characters whatever the client sent.
        value.push(c);
        *value = SingleLine::new(spec.kind.max_chars())
            .sanitize(value)
            .into_string();
    }

    fn enter(&mut self) -> RoomEffect {
        let last = fields_of(self.page).len().saturating_sub(1);
        if self.active < last {
            self.active += 1;
            return RoomEffect::Stay;
        }
        self.submit()
    }

    fn submit(&mut self) -> RoomEffect {
        self.submissions = self.submissions.saturating_add(1);
        if self.submissions > self.max_submissions {
            self.notice = Some(Notice {
                kind: NoticeKind::Error,
                text: strings::ERR_TOO_MANY,
            });
            return RoomEffect::Disconnect;
        }
        self.active = 0;
        if matches!(self.page, Page::RedeemInvite | Page::CreateAccount)
            && self.validate().is_ok()
            && let Some(request) = self.redeem_request()
        {
            // What was typed is dropped at once: the code does not stay in the room.
            self.clear_form();
            self.notice = None;
            return RoomEffect::Redeem(request);
        }
        if matches!(self.page, Page::RequestInvite | Page::Apply)
            && self.validate().is_ok()
            && let Some(ask) = self.invite_ask()
        {
            // What was typed leaves the room with the effect; the answer is fixed.
            self.clear_form();
            self.notice = Some(Notice {
                kind: NoticeKind::Info,
                text: unavailable(self.page),
            });
            return RoomEffect::RequestInvite(ask);
        }
        self.notice = Some(match self.validate() {
            Ok(()) => {
                // Nothing is stored or sent in M1; say so, and do not keep what was typed.
                self.clear_form();
                Notice {
                    kind: NoticeKind::Info,
                    text: unavailable(self.page),
                }
            }
            Err(text) => Notice {
                kind: NoticeKind::Error,
                text,
            },
        });
        RoomEffect::Stay
    }

    /// The redemption or `open` registration the form describes, if its handle is valid.
    fn redeem_request(&self) -> Option<RedeemRequest> {
        if self.page == Page::CreateAccount {
            let handle = sanitize_username(self.values.first()?).ok()?;
            return Some(RedeemRequest {
                join: JoinKind::Open,
                handle,
            });
        }
        let code = self.values.first()?.trim().to_owned();
        let handle = sanitize_username(self.values.get(1)?).ok()?;
        Some(RedeemRequest {
            join: JoinKind::Invite { code },
            handle,
        })
    }

    /// The request or application the form describes, if its handle is valid.
    fn invite_ask(&self) -> Option<InviteAsk> {
        let handle = sanitize_username(self.values.first()?).ok()?;
        let message = self.values.get(1)?.trim().to_owned();
        let kind = if self.page == Page::Apply {
            RequestKind::Application
        } else {
            RequestKind::Invite
        };
        Some(InviteAsk {
            handle,
            message,
            kind,
        })
    }

    /// The redemption or account creation did not work. Whatever the reason (an unknown, wrong,
    /// used or expired code, a taken handle, a key that already has an account, a rate limit, a
    /// full day, a store failure) the visitor reads the same words for the form they used, so
    /// the answer tells a stranger nothing about the board.
    pub fn redeem_refused(&mut self) {
        self.notice = Some(Notice {
            kind: NoticeKind::Error,
            text: if self.page == Page::CreateAccount {
                strings::CREATE_ACCOUNT_FAILED
            } else {
                strings::REDEEM_FAILED
            },
        });
    }

    /// Check what was typed. The checks depend only on the text, never on the board's members.
    fn validate(&self) -> Result<(), &'static str> {
        for (spec, value) in fields_of(self.page).iter().zip(&self.values) {
            if value.trim().is_empty() {
                return Err(strings::ERR_EMPTY);
            }
            if spec.kind == FieldKind::Handle {
                sanitize_username(value).map_err(username_error)?;
            }
        }
        Ok(())
    }

    fn go(&mut self, page: Page) {
        self.page = page;
        self.notice = None;
        self.clear_form();
    }

    fn clear_form(&mut self) {
        self.values = vec![String::new(); fields_of(self.page).len()];
        self.active = 0;
    }

    /// What to draw for the current page.
    #[must_use]
    pub fn view(&self) -> View {
        let mut view = View {
            title: strings::TITLE,
            heading: self.page.heading(),
            banner: Vec::new(),
            body: Vec::new(),
            note: None,
            menu: Vec::new(),
            fields: Vec::new(),
            notice: self.notice,
            footer: strings::FOOTER_PAGE,
        };
        match self.page {
            Page::Home => {
                view.banner.clone_from(&self.banner);
                view.menu = screens::menu(self.mode)
                    .iter()
                    .map(|entry| MenuItemView {
                        key: entry.key,
                        label: entry.label,
                    })
                    .collect();
                view.footer = strings::FOOTER_HOME;
                if self.degraded {
                    view.note = Some(strings::OPEN_CAPPED_NOTICE);
                }
            }
            Page::Rules => {
                view.body.clone_from(&self.rules);
                view.note = Some(strings::PRIVACY_NOTE);
            }
            page => {
                if self.degraded && page == Page::Apply {
                    view.note = Some(strings::OPEN_CAPPED_NOTICE);
                }
                view.fields = fields_of(self.page)
                    .iter()
                    .zip(&self.values)
                    .enumerate()
                    .map(|(index, (spec, value))| FieldView {
                        label: spec.label,
                        value: value.clone(),
                        active: index == self.active,
                    })
                    .collect();
                view.footer = strings::FOOTER_FORM;
            }
        }
        view
    }
}
