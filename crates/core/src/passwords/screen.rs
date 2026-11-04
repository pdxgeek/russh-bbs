//! The password screen as a pure state machine: the status page, the entry prompts and the
//! confirmation. It does no I/O. A key either changes the state or asks for a [`Request`] that
//! the front end runs with [`execute`](super::execute) and answers through
//! [`PasswordScreen::deliver`].
//!
//! What was typed is held in [`Secret`]s and drawn as one `*` per character; the text never
//! reaches a [`View`]. While a request is outstanding keys are ignored.

use zeroize::Zeroizing;

use super::exec::{Done, Kind, Refusal, Request, Response};
use super::secret::{MAX_CHARS, PolicyError, Secret, check_new};
use super::service::LoginMethod;
use super::strings;
use crate::waiting_room::{FieldView, MenuItemView, Notice, NoticeKind, View};

/// A key as the password screen sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordKey {
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
    /// Leave the screen, back to the settings.
    Leave,
    /// Leave, and say the action was not permitted.
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prompt {
    Current,
    New,
    Repeat,
}

impl Prompt {
    const fn label(self) -> &'static str {
        match self {
            Self::Current => strings::PROMPT_CURRENT,
            Self::New => strings::PROMPT_NEW,
            Self::Repeat => strings::PROMPT_REPEAT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Set,
    Clear,
}

#[derive(Debug)]
struct Entry {
    purpose: Purpose,
    /// Prompts still to answer, first is active.
    ahead: Vec<Prompt>,
    current: Option<Secret>,
    new: Option<Secret>,
    /// What is being typed. Wiped when submitted, abandoned or dropped. Reserved at full size so
    /// typing never reallocates and leaves an unwiped copy behind.
    typed: Zeroizing<String>,
}

#[derive(Debug)]
enum Stage {
    /// Waiting for the status answer.
    Loading,
    Menu,
    Entry(Entry),
    ConfirmClear,
    /// A set or clear is with the front end.
    Working,
}

/// The password screen.
#[derive(Debug)]
pub struct PasswordScreen {
    method: LoginMethod,
    stage: Stage,
    /// Whether the account has a password, as last reported.
    set: bool,
    request: Option<Request>,
    notice: Option<Notice>,
}

impl PasswordScreen {
    /// Open the screen. It asks for the status first.
    #[must_use]
    pub fn open(method: LoginMethod) -> Self {
        Self {
            method,
            stage: Stage::Loading,
            set: false,
            request: Some(Request {
                method,
                kind: Kind::Status,
            }),
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
        matches!(self.stage, Stage::Loading | Stage::Working)
    }

    /// Handle one key.
    pub fn press(&mut self, key: PasswordKey) -> Step {
        self.notice = None;
        match &self.stage {
            Stage::Loading | Stage::Working => Step::Stay,
            Stage::Menu => self.menu_key(key),
            Stage::ConfirmClear => self.confirm_key(key),
            Stage::Entry(_) => self.entry_key(key),
        }
    }

    fn menu_key(&mut self, key: PasswordKey) -> Step {
        match key {
            PasswordKey::Char('s' | 'S') => {
                let mut ahead = Vec::new();
                if self.method == LoginMethod::Password {
                    ahead.push(Prompt::Current);
                }
                ahead.push(Prompt::New);
                ahead.push(Prompt::Repeat);
                self.stage = Stage::Entry(Entry::new(Purpose::Set, ahead));
                Step::Stay
            }
            PasswordKey::Char('c' | 'C') if self.set => {
                if self.method == LoginMethod::Password {
                    self.stage = Stage::Entry(Entry::new(Purpose::Clear, vec![Prompt::Current]));
                } else {
                    self.stage = Stage::ConfirmClear;
                }
                Step::Stay
            }
            PasswordKey::Char('b' | 'B') | PasswordKey::Escape => Step::Leave,
            _ => Step::Stay,
        }
    }

    fn confirm_key(&mut self, key: PasswordKey) -> Step {
        match key {
            PasswordKey::Char('y' | 'Y') => {
                self.ask(Kind::Clear { current: None });
            }
            PasswordKey::Char('n' | 'N') | PasswordKey::Escape => self.stage = Stage::Menu,
            _ => {}
        }
        Step::Stay
    }

    fn entry_key(&mut self, key: PasswordKey) -> Step {
        let Stage::Entry(entry) = &mut self.stage else {
            return Step::Stay;
        };
        match key {
            PasswordKey::Char(c) => {
                if !c.is_control() && entry.typed.chars().count() < MAX_CHARS {
                    entry.typed.push(c);
                }
            }
            PasswordKey::Backspace => {
                entry.typed.pop();
            }
            PasswordKey::Escape => self.stage = Stage::Menu,
            PasswordKey::Enter => self.entry_enter(),
        }
        Step::Stay
    }

    fn entry_enter(&mut self) {
        let Stage::Entry(entry) = &mut self.stage else {
            return;
        };
        let typed = Secret::new(std::mem::take(&mut *entry.typed));
        // The next prompt starts with a fresh full-size buffer, so it never reallocates either.
        entry.typed = Zeroizing::new(String::with_capacity(MAX_CHARS * 4));
        let Some(&prompt) = entry.ahead.first() else {
            return;
        };
        match prompt {
            Prompt::Current => {
                entry.current = Some(typed);
                entry.ahead.remove(0);
            }
            Prompt::New => {
                if let Err(error) = check_new(typed.expose()) {
                    self.notice = Some(policy_notice(error));
                    return;
                }
                entry.new = Some(typed);
                entry.ahead.remove(0);
            }
            Prompt::Repeat => {
                let same = entry
                    .new
                    .as_ref()
                    .is_some_and(|first| first.expose() == typed.expose());
                if !same {
                    entry.new = None;
                    entry.ahead = vec![Prompt::New, Prompt::Repeat];
                    self.notice = Some(error(strings::ERR_MISMATCH));
                    return;
                }
                entry.ahead.remove(0);
            }
        }
        if !entry.ahead.is_empty() {
            return;
        }
        let (current, new, purpose) = (entry.current.take(), entry.new.take(), entry.purpose);
        match (purpose, new) {
            (Purpose::Set, Some(new)) => self.ask(Kind::Set { current, new }),
            (Purpose::Clear, _) => self.ask(Kind::Clear { current }),
            (Purpose::Set, None) => self.stage = Stage::Menu,
        }
    }

    fn ask(&mut self, kind: Kind) {
        self.request = Some(Request {
            method: self.method,
            kind,
        });
        self.stage = Stage::Working;
    }

    /// Take the answer to the request.
    pub fn deliver(&mut self, response: Response) -> Step {
        self.notice = None;
        match response {
            Response::Status(set) => {
                self.set = set;
                self.stage = Stage::Menu;
            }
            Response::Done(Done::Set) => {
                self.set = true;
                self.stage = Stage::Menu;
                self.notice = Some(info(strings::DONE_SET));
            }
            Response::Done(Done::Cleared) => {
                self.set = false;
                self.stage = Stage::Menu;
                self.notice = Some(info(strings::DONE_CLEARED));
            }
            Response::Refused(Refusal::Denied) => return Step::Denied,
            Response::Refused(refusal) => {
                // Status failing leaves nothing to show; an action failing returns to the menu.
                let was_loading = matches!(self.stage, Stage::Loading);
                self.stage = Stage::Menu;
                self.notice = Some(match refusal {
                    Refusal::WrongCurrent => error(strings::ERR_CURRENT),
                    Refusal::Policy(policy) => policy_notice(policy),
                    Refusal::LastCredential => error(strings::ERR_LAST_CREDENTIAL),
                    Refusal::Failed | Refusal::Denied => error(strings::ERR_FAILED),
                });
                if was_loading {
                    return Step::Leave;
                }
            }
        }
        Step::Stay
    }

    /// The page as a layout draws it. Typed text appears only as `*`s.
    #[must_use]
    pub fn view(&self) -> View {
        let mut view = View {
            title: crate::terminal_settings::strings::TITLE,
            heading: strings::HEADING,
            banner: Vec::new(),
            body: Vec::new(),
            note: None,
            menu: Vec::new(),
            fields: Vec::new(),
            notice: self.notice,
            footer: strings::FOOTER_MENU,
        };
        match &self.stage {
            Stage::Loading | Stage::Working => view.footer = strings::FOOTER_WORKING,
            Stage::Menu => {
                view.body = vec![
                    if self.set {
                        strings::STATUS_SET
                    } else {
                        strings::STATUS_NOT_SET
                    }
                    .to_owned(),
                    String::new(),
                    strings::NOTE_USE.to_owned(),
                ];
                view.menu.push(MenuItemView {
                    key: 's',
                    label: strings::MENU_SET,
                });
                if self.set {
                    view.menu.push(MenuItemView {
                        key: 'c',
                        label: strings::MENU_CLEAR,
                    });
                }
                view.menu.push(MenuItemView {
                    key: 'b',
                    label: strings::MENU_BACK,
                });
            }
            Stage::ConfirmClear => {
                view.body = vec![strings::CONFIRM_CLEAR.to_owned()];
                view.footer = strings::FOOTER_CONFIRM;
            }
            Stage::Entry(entry) => {
                if let Some(prompt) = entry.ahead.first() {
                    view.body = vec![
                        if *prompt == Prompt::Current {
                            strings::HINT_CURRENT
                        } else {
                            strings::HINT_RULES
                        }
                        .to_owned(),
                    ];
                    view.fields = vec![FieldView {
                        label: prompt.label(),
                        value: "*".repeat(entry.typed.chars().count()),
                        active: true,
                    }];
                }
                view.footer = strings::FOOTER_ENTRY;
            }
        }
        view
    }
}

impl Entry {
    fn new(purpose: Purpose, ahead: Vec<Prompt>) -> Self {
        Self {
            purpose,
            ahead,
            current: None,
            new: None,
            typed: Zeroizing::new(String::with_capacity(MAX_CHARS * 4)),
        }
    }
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

const fn policy_notice(policy: PolicyError) -> Notice {
    error(match policy {
        PolicyError::TooShort => strings::ERR_SHORT,
        PolicyError::TooLong => strings::ERR_LONG,
        PolicyError::ControlCharacter => strings::ERR_CONTROL,
    })
}
