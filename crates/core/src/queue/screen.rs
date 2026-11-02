//! The sysop queue and the vouch screen as a pure state machine. It does no I/O and draws
//! nothing. A key either changes the state or asks for a [`Request`] (which the front end runs
//! with [`execute`](super::execute) and answers through [`QueueScreen::deliver`]); what to show
//! comes out as a [`QueueView`] of string-table keys and sanitized content, so any layout can
//! draw it and a theme can change every word.
//!
//! Two modes share the screen. [`QueueMode::Sysop`] (Sysop tools, `members.approve` /
//! `members.deny`) offers approve and deny; [`QueueMode::Vouch`] (Members, `members.vouch`)
//! offers vouching only. A key is *offered* by what the session holds now, but the real gate is
//! [`execute`](super::execute), which checks again against the snapshot of the action.
//!
//! **Content stays content.** The requester's handle and message are returned as plain strings
//! in [`Row`] and [`ViewLine::Content`] and nowhere else; every fixed word is a string-table
//! key. A layout draws content in its content region and never in the status row.
//!
//! While a request is outstanding keys are ignored, so a screen never acts on a list it is
//! about to replace.

use super::keys;
use super::request::{DENIAL_COOLDOWN_SECS, PendingRequest, RequestId, RequestKind};
use super::service::{Request, Response};
use crate::access::{Permission, PermissionCheck};
use crate::store::{AccountId, UnixSeconds};
use crate::terminal_settings::format_utc;

/// Requests per page: four fit an 80x24 terminal with the heading, a notice and the footer.
pub const PAGE_SIZE: usize = 4;

/// Most voucher names listed on a request's page; the rest are counted.
const MAX_NAMES: usize = 2;

/// Which screen this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueMode {
    /// Sysop tools: approve and deny (and vouch, if the session holds it).
    Sysop,
    /// Members: vouch only.
    Vouch,
}

/// A key as the queue screens see it. Quit (Ctrl-C) is the session's rule, not a screen's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueKey {
    /// A printable character.
    Char(char),
    /// Enter.
    Enter,
    /// Escape.
    Escape,
}

/// What the front end does after a key or an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueStep {
    /// Redraw.
    Stay,
    /// Leave the screen, back to the menu.
    Leave,
    /// Leave, and say the action was not permitted.
    Denied,
}

/// One request in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The digit that opens it.
    pub hotkey: char,
    /// The handle asked for (sanitized).
    pub handle: String,
    /// How long ago it was made, like `5m`, `3h` or `2d`.
    pub age: String,
    /// How many members have vouched.
    pub vouches: usize,
    /// Whether the viewer is one of them.
    pub mine: bool,
    /// The message (sanitized, one line).
    pub message: String,
}

/// One line of a screen, for a layout to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewLine {
    /// An empty line.
    Blank,
    /// A heading (string-table key).
    Heading(&'static str),
    /// Fixed text (string-table key).
    Text(&'static str),
    /// Fixed text with `{name}` placeholders filled with values. Values that came from a
    /// requester are sanitized; the rest are numbers and times.
    Filled {
        /// The string-table key.
        key: &'static str,
        /// Placeholder names and their values.
        values: Vec<(&'static str, String)>,
    },
    /// A list entry: two lines, the second the message.
    Entry(Row),
    /// The keys on offer for a request, on one line (string-table keys of their short labels,
    /// like `[a] Approve`). Only what the session may do now is listed.
    Actions(Vec<&'static str>),
    /// A label (key) and a value. `content` says the value came from a requester.
    Pair {
        /// The label's string-table key.
        label: &'static str,
        /// The value.
        value: String,
        /// Whether the value is requester content (drawn as content, never as chrome).
        content: bool,
    },
    /// A paragraph of requester content, on its own lines (wrapped by the layout).
    Content(String),
    /// A notice: `error` chooses the word (Problem / Notice) and the text is a key.
    Notice {
        /// Whether it is a problem.
        error: bool,
        /// The string-table key of the text.
        text: &'static str,
    },
}

/// What a layout draws for the open screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueView {
    /// The lines, top to bottom.
    pub lines: Vec<ViewLine>,
    /// The string-table key of the footer (the keys on offer).
    pub footer: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Act {
    Approve,
    Deny,
    Vouch,
}

impl Act {
    const fn request(self, id: RequestId) -> Request {
        match self {
            Self::Approve => Request::Approve(id),
            Self::Deny => Request::Deny(id),
            Self::Vouch => Request::Vouch(id),
        }
    }

    const fn permission(self) -> Permission {
        match self {
            Self::Approve => Permission::MembersApprove,
            Self::Deny => Permission::MembersDeny,
            Self::Vouch => Permission::MembersVouch,
        }
    }

    const fn confirm(self) -> &'static str {
        match self {
            Self::Approve => keys::CONFIRM_APPROVE,
            Self::Deny => keys::CONFIRM_DENY,
            Self::Vouch => keys::CONFIRM_VOUCH,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Waiting for the first answer.
    Loading,
    List,
    Detail(RequestId),
    Confirm(Act, RequestId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    request: Request,
    sent: bool,
}

/// The queue screens, for one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueScreen {
    mode: QueueMode,
    own: Option<AccountId>,
    state: State,
    rows: Vec<PendingRequest>,
    now: UnixSeconds,
    /// Whether a listing has ever arrived (an empty queue and an unread one look different).
    loaded: bool,
    page: usize,
    pending: Option<Pending>,
    notice: Option<(bool, &'static str)>,
}

impl QueueScreen {
    /// Open the screen and ask for the list.
    #[must_use]
    pub fn open(mode: QueueMode, own: Option<AccountId>) -> Self {
        Self {
            mode,
            own,
            state: State::Loading,
            rows: Vec::new(),
            now: UnixSeconds::from_raw(0),
            loaded: false,
            page: 0,
            pending: Some(Pending {
                request: Request::List,
                sent: false,
            }),
            notice: None,
        }
    }

    fn ask(&mut self, request: Request) {
        self.pending = Some(Pending {
            request,
            sent: false,
        });
    }

    /// The request to run, once. It stays outstanding (keys are ignored) until
    /// [`QueueScreen::deliver`].
    pub fn take_request(&mut self) -> Option<Request> {
        let pending = self.pending.as_mut().filter(|p| !p.sent)?;
        pending.sent = true;
        Some(pending.request)
    }

    /// Whether the session still holds what the open screen needs. The front end closes the
    /// screen when this turns false (a change applies on the next action).
    #[must_use]
    pub fn still_permitted(&self, access: &dyn PermissionCheck) -> bool {
        match self.mode {
            QueueMode::Sysop => {
                access.allows(Permission::MembersApprove) || access.allows(Permission::MembersDeny)
            }
            QueueMode::Vouch => access.allows(Permission::MembersVouch),
        }
    }

    fn row(&self, id: RequestId) -> Option<&PendingRequest> {
        self.rows.iter().find(|row| row.id == id)
    }

    fn vouched_by_me(&self, request: &PendingRequest) -> bool {
        self.own
            .is_some_and(|me| request.vouchers.iter().any(|v| v.account == me))
    }

    /// Whether `act` is offered on `request` to a session holding `access`.
    fn offers(&self, access: &dyn PermissionCheck, act: Act, request: &PendingRequest) -> bool {
        if !access.allows(act.permission()) {
            return false;
        }
        match act {
            Act::Approve | Act::Deny => self.mode == QueueMode::Sysop,
            Act::Vouch => !self.vouched_by_me(request),
        }
    }

    fn pages(&self) -> usize {
        self.rows.len().div_ceil(PAGE_SIZE).max(1)
    }

    /// Handle one key.
    pub fn press(&mut self, access: &dyn PermissionCheck, key: QueueKey) -> QueueStep {
        if self.pending.is_some() {
            // The answer to the last request has not come; acting now would act on a list that
            // is about to change. Only leaving is always possible from the first load.
            return if key == QueueKey::Escape && self.state == State::Loading && !self.loaded {
                QueueStep::Leave
            } else {
                QueueStep::Stay
            };
        }
        self.notice = None;
        match self.state {
            State::Loading => match key {
                QueueKey::Escape => QueueStep::Leave,
                _ => QueueStep::Stay,
            },
            State::List => self.list_key(key),
            State::Detail(id) => self.detail_key(access, key, id),
            State::Confirm(act, id) => {
                if key == QueueKey::Char('y') || key == QueueKey::Char('Y') {
                    self.ask(act.request(id));
                } else {
                    self.state = State::Detail(id);
                }
                QueueStep::Stay
            }
        }
    }

    fn list_key(&mut self, key: QueueKey) -> QueueStep {
        match key {
            QueueKey::Escape => QueueStep::Leave,
            QueueKey::Char(c) => {
                match c.to_ascii_lowercase() {
                    'n' | ' ' => self.page = (self.page + 1).min(self.pages() - 1),
                    'p' | 'b' => self.page = self.page.saturating_sub(1),
                    'q' => return QueueStep::Leave,
                    digit @ '1'..='9' => {
                        let index = usize::from(u8::try_from(digit).unwrap_or(b'1') - b'1');
                        if index < PAGE_SIZE
                            && let Some(row) = self.rows.get(self.page * PAGE_SIZE + index)
                        {
                            self.state = State::Detail(row.id);
                        }
                    }
                    _ => {}
                }
                QueueStep::Stay
            }
            QueueKey::Enter => QueueStep::Stay,
        }
    }

    fn detail_key(
        &mut self,
        access: &dyn PermissionCheck,
        key: QueueKey,
        id: RequestId,
    ) -> QueueStep {
        let Some(request) = self.row(id).cloned() else {
            self.state = State::List;
            return QueueStep::Stay;
        };
        match key {
            QueueKey::Escape => self.state = State::List,
            QueueKey::Char(c) => {
                let act = match c.to_ascii_lowercase() {
                    'a' => Some(Act::Approve),
                    'd' => Some(Act::Deny),
                    'v' => Some(Act::Vouch),
                    _ => None,
                };
                if let Some(act) = act
                    && self.offers(access, act, &request)
                {
                    self.state = State::Confirm(act, id);
                }
            }
            QueueKey::Enter => {}
        }
        QueueStep::Stay
    }

    /// Hand the answer of the outstanding request to the screen.
    pub fn deliver(&mut self, response: Response) -> QueueStep {
        self.pending = None;
        match response {
            Response::Queue(queue) => {
                self.rows = queue.requests;
                self.now = queue.now;
                self.loaded = true;
                self.page = self.page.min(self.pages() - 1);
                // Keep the detail open if the request is still there; otherwise the list.
                self.state = match self.state {
                    State::Detail(id) | State::Confirm(_, id) if self.row(id).is_some() => {
                        State::Detail(id)
                    }
                    _ => State::List,
                };
            }
            Response::Approved(_) => self.done(keys::NOTICE_APPROVED, false),
            Response::Denied => self.done(keys::NOTICE_DENIED, false),
            Response::Vouched => self.done(keys::NOTICE_VOUCHED, false),
            Response::Gone => self.done(keys::NOTICE_GONE, true),
            Response::HandleTaken => self.stay_in_detail(keys::NOTICE_HANDLE_TAKEN),
            Response::KeyHasAccount => self.stay_in_detail(keys::NOTICE_KEY_HAS_ACCOUNT),
            Response::AlreadyVouched => self.stay_in_detail(keys::NOTICE_ALREADY_VOUCHED),
            Response::Failed => {
                self.notice = Some((true, keys::NOTICE_FAILED));
                self.state = if self.loaded {
                    State::List
                } else {
                    State::Loading
                };
            }
            Response::NotPermitted => return QueueStep::Denied,
        }
        QueueStep::Stay
    }

    /// A decision was made (or found gone): say so and read the queue again.
    fn done(&mut self, notice: &'static str, error: bool) {
        self.notice = Some((error, notice));
        self.state = State::Loading;
        self.ask(Request::List);
    }

    fn stay_in_detail(&mut self, notice: &'static str) {
        self.notice = Some((true, notice));
        self.state = match self.state {
            State::Confirm(_, id) | State::Detail(id) => State::Detail(id),
            State::Loading | State::List => State::List,
        };
    }

    /// What to draw now, for a session holding `access`.
    #[must_use]
    pub fn view(&self, access: &dyn PermissionCheck) -> QueueView {
        let mut view = match self.state {
            // Waiting for an answer: just the title. A first load that failed has no answer
            // coming and says so (the list view's "could not be read" line).
            State::Loading if self.pending.is_some() => QueueView {
                lines: vec![self.title(), ViewLine::Blank],
                footer: keys::FOOTER_LIST,
            },
            State::Loading | State::List => self.list_view(),
            State::Detail(id) => self.row(id).map_or_else(
                || self.list_view(),
                |request| self.detail_view(access, request, None),
            ),
            State::Confirm(act, id) => self.row(id).map_or_else(
                || self.list_view(),
                |request| self.detail_view(access, request, Some(act)),
            ),
        };
        if let Some((error, text)) = self.notice {
            view.lines.push(ViewLine::Blank);
            view.lines.push(ViewLine::Notice { error, text });
        }
        view
    }

    fn title(&self) -> ViewLine {
        ViewLine::Heading(match self.mode {
            QueueMode::Sysop => keys::TITLE_SYSOP,
            QueueMode::Vouch => keys::TITLE_VOUCH,
        })
    }

    fn list_view(&self) -> QueueView {
        let mut lines = vec![
            self.title(),
            ViewLine::Text(match self.mode {
                QueueMode::Sysop => keys::INTRO_SYSOP,
                QueueMode::Vouch => keys::INTRO_VOUCH,
            }),
            ViewLine::Blank,
        ];
        if !self.loaded {
            lines.push(ViewLine::Text(keys::UNREAD));
        } else if self.rows.is_empty() {
            lines.push(ViewLine::Text(keys::EMPTY));
        }
        for (index, request) in self
            .rows
            .iter()
            .skip(self.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .enumerate()
        {
            lines.push(ViewLine::Entry(Row {
                hotkey: char::from(b'1' + u8::try_from(index).unwrap_or(0)),
                handle: request.handle.as_str().to_owned(),
                age: age_text(self.now, request.created_at),
                vouches: request.vouchers.len(),
                mine: self.vouched_by_me(request),
                message: request.message.as_str().to_owned(),
            }));
        }
        if self.pages() > 1 {
            lines.push(ViewLine::Filled {
                key: keys::PAGE,
                values: vec![
                    ("page", (self.page + 1).to_string()),
                    ("pages", self.pages().to_string()),
                ],
            });
        }
        QueueView {
            lines,
            footer: keys::FOOTER_LIST,
        }
    }

    fn detail_view(
        &self,
        access: &dyn PermissionCheck,
        request: &PendingRequest,
        confirming: Option<Act>,
    ) -> QueueView {
        let handle = request.handle.as_str().to_owned();
        // At most two names, then a count, so the worst case (a full-width message and a long
        // list of vouchers) still fits an 80x24 terminal with its actions and footer.
        let mut names: Vec<String> = request
            .vouchers
            .iter()
            .take(MAX_NAMES)
            .map(|v| v.handle.as_str().to_owned())
            .collect();
        let extra = request.vouchers.len().saturating_sub(MAX_NAMES);
        let mut lines = vec![
            ViewLine::Filled {
                key: match request.kind {
                    RequestKind::Invite => keys::DETAIL_TITLE,
                    RequestKind::Application => keys::DETAIL_TITLE_APPLICATION,
                },
                values: vec![("id", request.id.to_string()), ("handle", handle.clone())],
            },
            ViewLine::Filled {
                key: keys::DETAIL_WHEN,
                values: vec![
                    ("age", age_text(self.now, request.created_at)),
                    ("until", format_utc(request.expires_at)),
                ],
            },
        ];
        if names.is_empty() {
            lines.push(ViewLine::Text(keys::DETAIL_NO_VOUCHES));
        } else {
            if extra > 0 {
                names.push(format!("+{extra}"));
            }
            lines.push(ViewLine::Pair {
                label: keys::DETAIL_VOUCHES,
                value: names.join(", "),
                content: true,
            });
        }
        lines.push(ViewLine::Pair {
            label: keys::DETAIL_MESSAGE,
            value: request.message.as_str().to_owned(),
            content: true,
        });
        lines.push(ViewLine::Blank);
        if let Some(act) = confirming {
            let mut values = vec![("handle", handle)];
            if act == Act::Deny {
                values.push(("hours", (DENIAL_COOLDOWN_SECS / 3600).to_string()));
            }
            lines.push(ViewLine::Filled {
                key: act.confirm(),
                values,
            });
            return QueueView {
                lines,
                footer: keys::FOOTER_CONFIRM,
            };
        }
        let offered: Vec<&'static str> = [
            (Act::Approve, keys::ACTION_APPROVE),
            (Act::Deny, keys::ACTION_DENY),
            (Act::Vouch, keys::ACTION_VOUCH),
        ]
        .into_iter()
        .filter(|(act, _)| self.offers(access, *act, request))
        .map(|(_, key)| key)
        .collect();
        if !offered.is_empty() {
            lines.push(ViewLine::Actions(offered));
        }
        QueueView {
            lines,
            footer: keys::FOOTER_DETAIL,
        }
    }
}

/// How long before `now` something happened, like `5m`, `3h` or `2d`: the largest whole unit
/// among minutes, hours and days, and `0m` for less than a minute. A time in the future (a
/// clock that moved back) reads `0m`.
#[must_use]
pub fn age_text(now: UnixSeconds, then: UnixSeconds) -> String {
    let secs = now.as_raw().saturating_sub(then.as_raw()).max(0);
    if secs >= 86_400 {
        format!("{}d", secs / 86_400)
    } else if secs >= 3_600 {
        format!("{}h", secs / 3_600)
    } else {
        format!("{}m", secs / 60)
    }
}
