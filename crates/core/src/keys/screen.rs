//! The My keys screen and the staff recovery screen as pure state machines. They do no I/O: a
//! key either changes the state or asks for a [`Request`] the front end runs with
//! [`execute`](super::execute) and answers through `deliver`. While a request is outstanding
//! keys are ignored. A code is held only while it is drawn and is dropped on the next key.

use super::code::LoginCode;
use super::exec::{Change, Refusal, Request, Response};
use super::pubkey::PASTE_MAX_CHARS;
use super::store::KeyRecord;
use super::strings;
use crate::access::Fingerprint;
use crate::sanitize::USERNAME_MAX_CHARS;
use crate::store::UnixSeconds;
use crate::terminal_settings::format_utc;
use crate::waiting_room::{FieldView, MenuItemView, Notice, NoticeKind, View};

/// A key as a keys screen sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeysKey {
    /// A printable character.
    Char(char),
    /// Enter.
    Enter,
    /// Backspace.
    Backspace,
    /// Escape.
    Escape,
}

/// What the front end does after a key or an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Redraw.
    Stay,
    /// Leave the screen.
    Leave,
    /// Leave, and say the action was not permitted.
    Denied,
}

/// How many keys can be picked by number.
const PICKABLE: usize = 9;

#[derive(Debug)]
enum Stage {
    Loading,
    List,
    Add(String),
    Pick,
    Confirm(Fingerprint),
    Working,
    Code(CodeShown),
}

#[derive(Debug)]
struct CodeShown {
    code: LoginCode,
    expires_at: UnixSeconds,
}

/// The My keys screen.
#[derive(Debug)]
pub struct KeysScreen {
    stage: Stage,
    keys: Vec<KeyRecord>,
    request: Option<Request>,
    notice: Option<Notice>,
    /// Telnet has no key to bind, so a code made there says so.
    telnet: bool,
}

impl KeysScreen {
    /// Open the screen. It asks for the keys first. `telnet` adds a note to the link-code page.
    #[must_use]
    pub fn open(telnet: bool) -> Self {
        Self {
            stage: Stage::Loading,
            keys: Vec::new(),
            request: Some(Request::List),
            notice: None,
            telnet,
        }
    }

    /// The request the screen is waiting on, once.
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }

    /// Whether a request is out and keys are ignored.
    #[must_use]
    pub const fn is_waiting(&self) -> bool {
        matches!(self.stage, Stage::Loading | Stage::Working)
    }

    /// Handle one key.
    pub fn press(&mut self, key: KeysKey) -> Step {
        self.notice = None;
        match &self.stage {
            Stage::Loading | Stage::Working => Step::Stay,
            Stage::List => self.list_key(key),
            Stage::Add(_) => self.add_key(key),
            Stage::Pick => self.pick_key(key),
            Stage::Confirm(_) => self.confirm_key(key),
            // Any key ends the code page; the code is dropped with the stage.
            Stage::Code(_) => {
                self.stage = Stage::List;
                Step::Stay
            }
        }
    }

    fn list_key(&mut self, key: KeysKey) -> Step {
        match key {
            KeysKey::Char('a' | 'A') => self.stage = Stage::Add(String::new()),
            KeysKey::Char('r' | 'R') if !self.keys.is_empty() => self.stage = Stage::Pick,
            KeysKey::Char('l' | 'L') => self.ask(Request::LinkCode),
            KeysKey::Char('b' | 'B') | KeysKey::Escape => return Step::Leave,
            _ => {}
        }
        Step::Stay
    }

    fn add_key(&mut self, key: KeysKey) -> Step {
        let Stage::Add(typed) = &mut self.stage else {
            return Step::Stay;
        };
        match key {
            KeysKey::Char(c) => {
                // A public key line is printable ASCII; anything else is not part of one.
                if (c.is_ascii_graphic() || c == ' ') && typed.chars().count() < PASTE_MAX_CHARS {
                    typed.push(c);
                }
            }
            KeysKey::Backspace => {
                typed.pop();
            }
            KeysKey::Escape => self.stage = Stage::List,
            KeysKey::Enter => {
                let text = std::mem::take(typed);
                if text.trim().is_empty() {
                    self.stage = Stage::List;
                } else {
                    self.ask(Request::Add(text));
                }
            }
        }
        Step::Stay
    }

    fn pick_key(&mut self, key: KeysKey) -> Step {
        match key {
            KeysKey::Escape => self.stage = Stage::List,
            KeysKey::Char(c) => {
                let index = c
                    .to_digit(10)
                    .and_then(|digit| usize::try_from(digit).ok())
                    .and_then(|digit| digit.checked_sub(1));
                if let Some(record) = index
                    .filter(|index| *index < PICKABLE)
                    .and_then(|index| self.keys.get(index))
                {
                    self.stage = Stage::Confirm(record.fingerprint.clone());
                }
            }
            _ => {}
        }
        Step::Stay
    }

    fn confirm_key(&mut self, key: KeysKey) -> Step {
        let Stage::Confirm(fingerprint) = &self.stage else {
            return Step::Stay;
        };
        match key {
            KeysKey::Char('y' | 'Y') => {
                let fingerprint = fingerprint.clone();
                self.ask(Request::Remove(fingerprint));
            }
            KeysKey::Char('n' | 'N') | KeysKey::Escape => self.stage = Stage::List,
            _ => {}
        }
        Step::Stay
    }

    fn ask(&mut self, request: Request) {
        self.request = Some(request);
        self.stage = Stage::Working;
    }

    /// Take the answer to the request.
    pub fn deliver(&mut self, response: Response) -> Step {
        self.notice = None;
        match response {
            Response::Keys { keys, changed } => {
                self.keys = keys;
                self.stage = Stage::List;
                self.notice = changed.map(|change| {
                    info(match change {
                        Change::Added => strings::DONE_ADDED,
                        Change::Removed => strings::DONE_REMOVED,
                    })
                });
            }
            Response::Code(shown) => {
                self.stage = Stage::Code(CodeShown {
                    code: shown.code,
                    expires_at: shown.expires_at,
                });
            }
            Response::Refused(Refusal::Denied) => return Step::Denied,
            Response::Refused(refusal) => {
                let was_loading = matches!(self.stage, Stage::Loading);
                self.stage = Stage::List;
                self.notice = Some(error(refusal_text(refusal)));
                if was_loading {
                    return Step::Leave;
                }
            }
        }
        Step::Stay
    }

    /// The page as a layout draws it.
    #[must_use]
    pub fn view(&self) -> View {
        let mut view = base_view(strings::HEADING, self.notice, strings::FOOTER_LIST);
        match &self.stage {
            Stage::Loading | Stage::Working => view.footer = strings::FOOTER_WORKING,
            Stage::List => {
                view.body = self.list_lines();
                view.menu.push(menu('a', strings::MENU_ADD));
                if !self.keys.is_empty() {
                    view.menu.push(menu('r', strings::MENU_REMOVE));
                }
                view.menu.push(menu('l', strings::MENU_LINK));
                view.menu.push(menu('b', strings::MENU_BACK));
            }
            Stage::Add(typed) => {
                view.body = vec![strings::HINT_ADD.to_owned()];
                view.fields = vec![FieldView {
                    label: strings::PROMPT_KEY,
                    value: typed.clone(),
                    active: true,
                }];
                view.footer = strings::FOOTER_ENTRY;
            }
            Stage::Pick => {
                view.body = std::iter::once(strings::HINT_REMOVE.to_owned())
                    .chain(self.list_lines())
                    .collect();
                view.footer = strings::FOOTER_PICK;
            }
            Stage::Confirm(fingerprint) => {
                view.body = vec![format!("Remove {fingerprint}?")];
                view.footer = strings::FOOTER_CONFIRM;
            }
            Stage::Code(shown) => {
                view.heading = strings::CODE_HEADING;
                view.body = strings::CODE_STEPS_LINK
                    .iter()
                    .map(|line| (*line).to_owned())
                    .chain([
                        String::new(),
                        format!("{} {}", strings::CODE_LABEL, shown.code.reveal()),
                        format!("Expires {}.", format_utc(shown.expires_at)),
                    ])
                    .chain(self.telnet.then(|| strings::CODE_NOTE_TELNET.to_owned()))
                    .collect();
                view.footer = strings::FOOTER_CODE;
            }
        }
        view
    }

    fn list_lines(&self) -> Vec<String> {
        let mut lines = vec![strings::NOTE_KEYS.to_owned(), String::new()];
        if self.keys.is_empty() {
            lines.push(strings::NO_KEYS.to_owned());
        }
        for (index, record) in self.keys.iter().take(PICKABLE).enumerate() {
            let last = record
                .last_used_at
                .map_or_else(|| "never".to_owned(), format_utc);
            lines.push(format!(
                "{}. {}  added {}, last used {}",
                index + 1,
                record.fingerprint,
                format_utc(record.added_at),
                last,
            ));
        }
        if self.keys.len() > PICKABLE {
            lines.push(strings::MORE_KEYS.to_owned());
        }
        lines
    }
}

#[derive(Debug)]
enum StaffStage {
    Entry(String),
    Working,
    Code(CodeShown),
}

/// The staff recovery screen under Sysop tools: a handle in, a one-time code out.
#[derive(Debug)]
pub struct RecoveryScreen {
    stage: StaffStage,
    request: Option<Request>,
    notice: Option<Notice>,
}

impl RecoveryScreen {
    /// Open the screen at the handle prompt.
    #[must_use]
    pub fn open() -> Self {
        Self {
            stage: StaffStage::Entry(String::new()),
            request: None,
            notice: None,
        }
    }

    /// The request the screen is waiting on, once.
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }

    /// Whether a request is out and keys are ignored.
    #[must_use]
    pub const fn is_waiting(&self) -> bool {
        matches!(self.stage, StaffStage::Working)
    }

    /// Handle one key.
    pub fn press(&mut self, key: KeysKey) -> Step {
        self.notice = None;
        match &mut self.stage {
            StaffStage::Working => {}
            StaffStage::Code(_) => {
                self.stage = StaffStage::Entry(String::new());
            }
            StaffStage::Entry(typed) => match key {
                KeysKey::Char(c) => {
                    if (c.is_ascii_graphic()) && typed.chars().count() < USERNAME_MAX_CHARS {
                        typed.push(c);
                    }
                }
                KeysKey::Backspace => {
                    typed.pop();
                }
                KeysKey::Escape => return Step::Leave,
                KeysKey::Enter => {
                    let handle = std::mem::take(typed);
                    if !handle.trim().is_empty() {
                        self.request = Some(Request::Recovery(handle));
                        self.stage = StaffStage::Working;
                    }
                }
            },
        }
        Step::Stay
    }

    /// Take the answer to the request.
    pub fn deliver(&mut self, response: Response) -> Step {
        self.notice = None;
        match response {
            Response::Code(shown) => {
                self.stage = StaffStage::Code(CodeShown {
                    code: shown.code,
                    expires_at: shown.expires_at,
                });
            }
            Response::Refused(Refusal::Denied) => return Step::Denied,
            Response::Refused(refusal) => {
                self.stage = StaffStage::Entry(String::new());
                self.notice = Some(error(refusal_text(refusal)));
            }
            Response::Keys { .. } => self.stage = StaffStage::Entry(String::new()),
        }
        Step::Stay
    }

    /// The page as a layout draws it.
    #[must_use]
    pub fn view(&self) -> View {
        let mut view = base_view(
            strings::RECOVERY_HEADING,
            self.notice,
            strings::FOOTER_RECOVERY,
        );
        match &self.stage {
            StaffStage::Working => view.footer = strings::FOOTER_WORKING,
            StaffStage::Entry(typed) => {
                view.body = strings::RECOVERY_RULES
                    .iter()
                    .map(|line| (*line).to_owned())
                    .collect();
                view.fields = vec![FieldView {
                    label: strings::PROMPT_HANDLE,
                    value: typed.clone(),
                    active: true,
                }];
            }
            StaffStage::Code(shown) => {
                view.body = vec![
                    strings::RECOVERY_DONE.to_owned(),
                    String::new(),
                    format!("{} {}", strings::CODE_LABEL, shown.code.reveal()),
                    format!("Expires {}.", format_utc(shown.expires_at)),
                ];
                view.footer = strings::FOOTER_CODE;
            }
        }
        view
    }
}

fn base_view(heading: &'static str, notice: Option<Notice>, footer: &'static str) -> View {
    View {
        title: crate::terminal_settings::strings::TITLE,
        heading,
        banner: Vec::new(),
        body: Vec::new(),
        note: None,
        menu: Vec::new(),
        fields: Vec::new(),
        notice,
        footer,
    }
}

const fn menu(key: char, label: &'static str) -> MenuItemView {
    MenuItemView { key, label }
}

const fn info(text: &'static str) -> Notice {
    Notice {
        kind: NoticeKind::Info,
        text,
    }
}

const fn error(text: &'static str) -> Notice {
    Notice {
        kind: NoticeKind::Error,
        text,
    }
}

const fn refusal_text(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::NotAKey => strings::ERR_NOT_A_KEY,
        Refusal::CannotAdd => strings::ERR_CANNOT_ADD,
        Refusal::NoSuchKey => strings::ERR_NO_SUCH_KEY,
        Refusal::LastWayIn => strings::ERR_LAST_WAY_IN,
        Refusal::RateLimited => strings::ERR_RATE_LIMITED,
        Refusal::NotYours => strings::ERR_NOT_YOURS,
        Refusal::Denied => strings::ERR_DENIED,
        Refusal::Failed => strings::ERR_FAILED,
    }
}
