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

use zeroize::Zeroize as _;

use super::screens::{self, Page, QUIT_KEY};
use super::source::RoomConfig;
use super::strings;
use crate::passwords::{
    MAX_CHARS as PASSWORD_MAX_CHARS, PolicyError, Secret, check_new, strings as password_strings,
};
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
    /// The password the person chose, on a room built [`WaitingRoom::with_password_signup`]
    /// (telnet, D94); the account is then bound to it and to no key. `None` on a key room.
    pub password: Option<Secret>,
}

impl fmt::Debug for RedeemRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedeemRequest")
            .field("join", &self.join)
            .field("handle", &self.handle)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// "I already have an account" (D94 (8)): the handle and password to check, and bind this key
/// to the account they open. The front end applies the password limits first.
#[derive(Clone)]
pub struct LinkRequest {
    /// The handle as typed.
    pub handle: String,
    /// The password as typed, wiped when dropped. It is a login attempt, not a new password, so
    /// it is not held to the password rules.
    pub password: zeroize::Zeroizing<String>,
}

impl PartialEq for LinkRequest {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle && self.password.as_str() == other.password.as_str()
    }
}

impl Eq for LinkRequest {}

impl fmt::Debug for LinkRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkRequest").finish_non_exhaustive()
    }
}

/// A device-link code or a staff recovery code to redeem, and for a recovery code the new
/// password. The code and the password leave the room only inside the effect.
#[derive(Clone)]
pub struct CodeRequest {
    /// The code as typed (sanitized and length-capped, not yet normalized or hashed).
    pub code: String,
    /// The new password, for a recovery code; `None` for a device-link code.
    pub password: Option<Secret>,
}

impl PartialEq for CodeRequest {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
            && self.password.as_ref().map(Secret::expose)
                == other.password.as_ref().map(Secret::expose)
    }
}

impl Eq for CodeRequest {}

impl fmt::Debug for CodeRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Both are credentials: never in a log or a test failure message.
        f.debug_struct("CodeRequest").finish_non_exhaustive()
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
    /// Check a handle and password and bind this key to the account (see [`LinkRequest`]), then
    /// call [`WaitingRoom::redeem_refused`] if it did not work (one sentence for every cause).
    LinkKey(LinkRequest),
    /// Redeem a device-link code and bind this key to the account it was made for, then call
    /// [`WaitingRoom::redeem_refused`] if it did not work (one sentence for every cause).
    LinkDevice(CodeRequest),
    /// Redeem a staff recovery code: bind this key and set the new password, then call
    /// [`WaitingRoom::redeem_refused`] if it did not work.
    Recover(CodeRequest),
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
    /// A password or its repeat: printable ASCII only, shown as `*`, never trimmed (D81).
    Secret,
}

impl FieldKind {
    const fn max_chars(self) -> usize {
        match self {
            Self::Handle => USERNAME_MAX_CHARS,
            Self::Line => LINE_MAX_CHARS,
            Self::Code => CODE_MAX_CHARS,
            Self::Secret => PASSWORD_MAX_CHARS,
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

const LINK_CODE: FieldSpec = FieldSpec {
    label: crate::keys::strings::LABEL_LINK_CODE,
    kind: FieldKind::Code,
};
const RECOVERY_CODE: FieldSpec = FieldSpec {
    label: crate::keys::strings::LABEL_RECOVERY_CODE,
    kind: FieldKind::Code,
};
const NEW_PASSWORD: FieldSpec = FieldSpec {
    label: crate::keys::strings::LABEL_NEW_PASSWORD,
    kind: FieldKind::Secret,
};
const NEW_PASSWORD_AGAIN: FieldSpec = FieldSpec {
    label: crate::keys::strings::LABEL_NEW_PASSWORD_AGAIN,
    kind: FieldKind::Secret,
};
const LINK_HANDLE: FieldSpec = FieldSpec {
    label: strings::LABEL_LINK_HANDLE,
    kind: FieldKind::Line,
};
const LINK_PASSWORD: FieldSpec = FieldSpec {
    label: strings::LABEL_LINK_PASSWORD,
    kind: FieldKind::Secret,
};
const PASSWORD: FieldSpec = FieldSpec {
    label: strings::LABEL_PASSWORD,
    kind: FieldKind::Secret,
};
const PASSWORD_AGAIN: FieldSpec = FieldSpec {
    label: strings::LABEL_PASSWORD_AGAIN,
    kind: FieldKind::Secret,
};

/// The fields of `page`. With `passwords` (a room for a person with no SSH key, D94) every form
/// that makes or asks for an account ends with a password and its repeat.
fn fields_of(page: Page, passwords: bool) -> &'static [FieldSpec] {
    match (page, passwords) {
        (Page::Home | Page::Rules, _) => &[],
        (Page::RequestInvite | Page::Apply, false) => &[HANDLE, ABOUT],
        (Page::RequestInvite | Page::Apply, true) => &[HANDLE, ABOUT, PASSWORD, PASSWORD_AGAIN],
        (Page::RedeemInvite, false) => &[CODE, HANDLE],
        (Page::RedeemInvite, true) => &[CODE, HANDLE, PASSWORD, PASSWORD_AGAIN],
        (Page::CreateAccount, false) => &[HANDLE],
        (Page::CreateAccount, true) => &[HANDLE, PASSWORD, PASSWORD_AGAIN],
        (Page::LinkKey, _) => &[LINK_HANDLE, LINK_PASSWORD],
        (Page::LinkDevice, _) => &[LINK_CODE],
        (Page::Recover, _) => &[RECOVERY_CODE, NEW_PASSWORD, NEW_PASSWORD_AGAIN],
    }
}

/// The words for a valid form that no special case took. Every form is live, so this is the
/// same sentence the live path shows.
fn unavailable(page: Page, passwords: bool, key: bool) -> &'static str {
    match page {
        Page::RequestInvite if passwords && key => strings::REQUEST_INVITE_ANSWER_KEY_PASSWORD,
        Page::Apply if passwords && key => strings::APPLY_ANSWER_KEY_PASSWORD,
        // The request form is live (M2-E) and never gets here; the words stay honest anyway.
        Page::RequestInvite if passwords => strings::REQUEST_INVITE_ANSWER_PASSWORD,
        Page::Apply if passwords => strings::APPLY_ANSWER_PASSWORD,
        Page::RequestInvite => strings::REQUEST_INVITE_ANSWER,
        // The redeem form is live (M2-D) and never gets here; the words stay honest anyway.
        Page::RedeemInvite => strings::REDEEM_FAILED,
        // The apply and create-account forms are live (M2-G) and never get here either.
        Page::Apply => strings::APPLY_ANSWER,
        Page::CreateAccount => strings::CREATE_ACCOUNT_FAILED,
        Page::LinkKey => strings::LINK_KEY_FAILED,
        Page::LinkDevice | Page::Recover => crate::keys::strings::CODE_REFUSED,
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
    /// Whether the forms ask for a password instead of relying on an SSH key (D94).
    passwords: bool,
    /// Whether the room offers "I already have an account" (a visitor with an SSH key).
    key_link: bool,
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
            passwords: false,
            key_link: false,
        }
    }

    /// Offer "I already have an account" (D94 (8)): a handle and password that bind **this
    /// connection's SSH key** to an account. Only a room for a visitor who came with a key is
    /// built this way.
    #[must_use]
    pub fn with_key_link(mut self) -> Self {
        self.key_link = true;
        self
    }

    /// Build the room for a person with **no SSH key** (telnet, D94). Every form that makes an
    /// account or asks for one also asks for a password and its repeat; the password leaves the
    /// room only inside the effect, as a [`Secret`], and the [`View`] shows `*` for it. The
    /// pages, the menu and the no-leak rule are the key room's.
    #[must_use]
    pub fn with_password_signup(mut self) -> Self {
        self.passwords = true;
        self.clear_form();
        self
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
        if self.key_link && key.eq_ignore_ascii_case(&screens::LINK_KEY) {
            self.go(Page::LinkKey);
        } else if self.key_link && key.eq_ignore_ascii_case(&screens::LINK_DEVICE_KEY) {
            self.go(Page::LinkDevice);
        } else if self.key_link && key.eq_ignore_ascii_case(&screens::RECOVER_KEY) {
            self.go(Page::Recover);
        } else if let Some(entry) = screens::entry_for(self.mode, key) {
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
        let Some(spec) = fields_of(self.page, self.passwords).get(self.active) else {
            return;
        };
        let Some(value) = self.values.get_mut(self.active) else {
            return;
        };
        if spec.kind == FieldKind::Secret {
            // Printable ASCII only (what a telnet login can type back), exactly as typed: no
            // sanitizer pass that would trim or fold it.
            if (c.is_ascii_graphic() || c == ' ') && value.chars().count() < PASSWORD_MAX_CHARS {
                value.push(c);
            }
            return;
        }
        // Re-sanitize the whole buffer on each key: it enforces the length cap and keeps the
        // buffer free of controls, escapes and private-use characters whatever the client sent.
        value.push(c);
        *value = SingleLine::new(spec.kind.max_chars())
            .sanitize(value)
            .into_string();
    }

    fn enter(&mut self) -> RoomEffect {
        let last = fields_of(self.page, self.passwords).len().saturating_sub(1);
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
        if self.page == Page::LinkKey && self.validate().is_ok() {
            let handle = self.values.first().cloned().unwrap_or_default();
            let password = zeroize::Zeroizing::new(self.values.get(1).cloned().unwrap_or_default());
            self.clear_form();
            self.notice = None;
            return RoomEffect::LinkKey(LinkRequest {
                handle: handle.trim().to_owned(),
                password,
            });
        }
        if matches!(self.page, Page::LinkDevice | Page::Recover) && self.validate().is_ok() {
            let code = self.values.first().cloned().unwrap_or_default();
            let password = (self.page == Page::Recover)
                .then(|| self.values.get(1).cloned().map(Secret::new))
                .flatten();
            self.clear_form();
            self.notice = None;
            let request = CodeRequest {
                code: code.trim().to_owned(),
                password,
            };
            return if self.page == Page::Recover {
                RoomEffect::Recover(request)
            } else {
                RoomEffect::LinkDevice(request)
            };
        }
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
                text: unavailable(self.page, self.passwords, self.key_link),
            });
            return RoomEffect::RequestInvite(ask);
        }
        self.notice = Some(match self.validate() {
            Ok(()) => {
                // Nothing is stored or sent in M1; say so, and do not keep what was typed.
                self.clear_form();
                Notice {
                    kind: NoticeKind::Info,
                    text: unavailable(self.page, self.passwords, self.key_link),
                }
            }
            Err(text) => {
                // A secret is not kept across a refusal: the person types it again.
                self.clear_secrets();
                Notice {
                    kind: NoticeKind::Error,
                    text,
                }
            }
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
                password: self.chosen_password(),
            });
        }
        let code = self.values.first()?.trim().to_owned();
        let handle = sanitize_username(self.values.get(1)?).ok()?;
        Some(RedeemRequest {
            join: JoinKind::Invite { code },
            handle,
            password: self.chosen_password(),
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
            password: self.chosen_password(),
        })
    }

    /// The password typed on a password form (the first of the two entries; `validate` has
    /// checked they match), `None` on a key form.
    fn chosen_password(&self) -> Option<Secret> {
        if !self.passwords {
            return None;
        }
        let index = fields_of(self.page, true)
            .iter()
            .position(|spec| spec.kind == FieldKind::Secret)?;
        self.values.get(index).cloned().map(Secret::new)
    }

    /// Wipe the secret fields and put the cursor on the first of them.
    fn clear_secrets(&mut self) {
        let mut first = None;
        for (index, (spec, value)) in fields_of(self.page, self.passwords)
            .iter()
            .zip(self.values.iter_mut())
            .enumerate()
        {
            if spec.kind == FieldKind::Secret {
                value.zeroize();
                first.get_or_insert(index);
            }
        }
        if let Some(first) = first {
            self.active = first;
        }
    }

    /// The redemption or account creation did not work. Whatever the reason (an unknown, wrong,
    /// used or expired code, a taken handle, a key that already has an account, a rate limit, a
    /// full day, a store failure) the visitor reads the same words for the form they used, so
    /// the answer tells a stranger nothing about the board.
    pub fn redeem_refused(&mut self) {
        self.notice = Some(Notice {
            kind: NoticeKind::Error,
            text: if self.page == Page::LinkKey {
                strings::LINK_KEY_FAILED
            } else if matches!(self.page, Page::LinkDevice | Page::Recover) {
                crate::keys::strings::CODE_REFUSED
            } else if self.page == Page::CreateAccount {
                strings::CREATE_ACCOUNT_FAILED
            } else {
                strings::REDEEM_FAILED
            },
        });
    }

    /// Check what was typed. The checks depend only on the text, never on the board's members.
    fn validate(&self) -> Result<(), &'static str> {
        let fields = fields_of(self.page, self.passwords);
        for (spec, value) in fields.iter().zip(&self.values) {
            if value.trim().is_empty() {
                return Err(strings::ERR_EMPTY);
            }
            if spec.kind == FieldKind::Handle {
                sanitize_username(value).map_err(username_error)?;
            }
        }
        let mut secrets = fields
            .iter()
            .zip(&self.values)
            .filter(|(spec, _)| spec.kind == FieldKind::Secret)
            .map(|(_, value)| value);
        if let (Some(chosen), Some(again)) = (secrets.next(), secrets.next()) {
            if chosen != again {
                return Err(strings::ERR_PASSWORD_MISMATCH);
            }
            check_new(chosen).map_err(|policy| match policy {
                PolicyError::TooShort => password_strings::ERR_SHORT,
                PolicyError::TooLong => password_strings::ERR_LONG,
                PolicyError::ControlCharacter => password_strings::ERR_CONTROL,
            })?;
        }
        Ok(())
    }

    fn go(&mut self, page: Page) {
        self.page = page;
        self.notice = None;
        self.clear_form();
    }

    fn clear_form(&mut self) {
        for value in &mut self.values {
            value.zeroize();
        }
        self.values = fields_of(self.page, self.passwords)
            .iter()
            .map(|spec| {
                if spec.kind == FieldKind::Secret {
                    // Room for the longest password, so typing never reallocates (and leaves a
                    // copy of the text behind).
                    String::with_capacity(PASSWORD_MAX_CHARS)
                } else {
                    String::new()
                }
            })
            .collect();
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
                if self.key_link {
                    view.menu.push(MenuItemView {
                        key: screens::LINK_KEY,
                        label: strings::MENU_LINK_KEY,
                    });
                    view.menu.push(MenuItemView {
                        key: screens::LINK_DEVICE_KEY,
                        label: crate::keys::strings::MENU_LINK_DEVICE,
                    });
                    view.menu.push(MenuItemView {
                        key: screens::RECOVER_KEY,
                        label: crate::keys::strings::MENU_RECOVER,
                    });
                }
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
                if page == Page::Recover {
                    view.note = Some(crate::keys::strings::RECOVER_NOTE);
                }
                view.fields = fields_of(self.page, self.passwords)
                    .iter()
                    .zip(&self.values)
                    .enumerate()
                    .map(|(index, (spec, value))| FieldView {
                        label: spec.label,
                        // A password never leaves the room in a view.
                        value: if spec.kind == FieldKind::Secret {
                            "*".repeat(value.chars().count())
                        } else {
                            value.clone()
                        },
                        active: index == self.active,
                    })
                    .collect();
                view.footer = strings::FOOTER_FORM;
            }
        }
        view
    }
}
