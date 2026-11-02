//! The claim screen: one field for the token, a few attempts, one generic failure.
//!
//! The screen is identical whether or not the board is claimed (D38): it never learns the
//! state. After a rejection it says the same words every time.

use super::token::TOKEN_TEXT_MAX_CHARS;
use crate::waiting_room::{FieldView, MenuItemView, Notice, NoticeKind, View};

/// Token attempts one session gets before it is disconnected.
pub const MAX_ATTEMPTS_PER_SESSION: u32 = 3;

/// Fixed strings of the claim screen. All are chrome.
pub mod strings {
    /// Window title.
    pub const TITLE: &str = "russh-bbs";
    /// Heading of the prompt.
    pub const HEADING: &str = "Setup token";
    /// Explains what to type, without saying anything about the board's state.
    pub const PROMPT: &str = "Enter the setup token printed on the server console.";
    /// The field label.
    pub const LABEL: &str = "Token";
    /// Key hints.
    pub const FOOTER: &str = "Enter: send   Esc: disconnect";
    /// The one failure message, for every kind of failure.
    pub const REJECTED: &str = "That token was not accepted.";
    /// Shown when the session has used its attempts.
    pub const TOO_MANY: &str = "Too many attempts. Goodbye.";
    /// Shown while a claim is being checked.
    pub const WORKING: &str = "Checking...";
    /// Heading of the signed-in placeholder.
    pub const SIGNED_IN_HEADING: &str = "Signed in";
    /// Placeholder body for a sysop. The member and sysop experience are later work.
    pub const SIGNED_IN_SYSOP: &str = "You are signed in as sysop.";
    /// Placeholder body for a member.
    pub const SIGNED_IN_MEMBER: &str = "You are signed in as a member.";
    /// Honest note that the real screens do not exist yet.
    pub const PLACEHOLDER_NOTE: &str = "This is a placeholder. The board itself is not built yet.";
    /// Key hint on the placeholder.
    pub const SIGNED_IN_FOOTER: &str = "Ctrl-C: disconnect";
}

/// What the person did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimInput {
    /// A printable character.
    Char(char),
    /// Enter.
    Enter,
    /// Backspace.
    Backspace,
    /// Escape: leave.
    Escape,
    /// Ctrl-C / Ctrl-D: leave.
    Quit,
}

/// What the front end must do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimEffect {
    /// Redraw.
    Stay,
    /// Check this token, then call [`ClaimForm::rejected`] or [`ClaimForm::accepted`].
    Submit(String),
    /// Redraw once, then close the session.
    Disconnect,
}

/// One session's claim prompt.
#[derive(Debug)]
pub struct ClaimForm {
    value: String,
    notice: Option<Notice>,
    attempts: u32,
    max_attempts: u32,
    done: bool,
}

impl ClaimForm {
    /// A fresh prompt allowing `max_attempts` tries.
    #[must_use]
    pub fn new(max_attempts: u32) -> Self {
        Self {
            value: String::new(),
            notice: None,
            attempts: 0,
            max_attempts,
            done: false,
        }
    }

    /// Handle one input. Typed text is kept as ASCII letters, digits and dashes only, which is
    /// all a token contains; the rest is dropped, so nothing else can reach the screen.
    pub fn input(&mut self, input: ClaimInput) -> ClaimEffect {
        if self.done {
            return match input {
                ClaimInput::Quit | ClaimInput::Escape => ClaimEffect::Disconnect,
                _ => ClaimEffect::Stay,
            };
        }
        match input {
            ClaimInput::Quit | ClaimInput::Escape => ClaimEffect::Disconnect,
            ClaimInput::Char(c) => {
                if (c.is_ascii_alphanumeric() || c == '-')
                    && self.value.chars().count() < TOKEN_TEXT_MAX_CHARS
                {
                    self.value.push(c);
                }
                ClaimEffect::Stay
            }
            ClaimInput::Backspace => {
                self.value.pop();
                ClaimEffect::Stay
            }
            ClaimInput::Enter => {
                self.attempts += 1;
                if self.attempts > self.max_attempts {
                    self.notice = Some(Notice {
                        kind: NoticeKind::Error,
                        text: strings::TOO_MANY,
                    });
                    return ClaimEffect::Disconnect;
                }
                ClaimEffect::Submit(std::mem::take(&mut self.value))
            }
        }
    }

    /// The submitted token failed. Always the same message.
    pub fn rejected(&mut self) -> ClaimEffect {
        self.notice = Some(Notice {
            kind: NoticeKind::Error,
            text: strings::REJECTED,
        });
        if self.attempts >= self.max_attempts {
            self.notice = Some(Notice {
                kind: NoticeKind::Error,
                text: strings::TOO_MANY,
            });
            return ClaimEffect::Disconnect;
        }
        ClaimEffect::Stay
    }

    /// Mark the prompt finished (the claim succeeded and the screen changes).
    pub fn accepted(&mut self) {
        self.done = true;
        self.value.clear();
    }

    /// The prompt as a [`View`]. The typed token is shown as dots, never echoed.
    #[must_use]
    pub fn view(&self) -> View {
        View {
            title: strings::TITLE,
            heading: strings::HEADING,
            banner: Vec::new(),
            body: vec![strings::PROMPT.to_owned()],
            note: None,
            menu: Vec::<MenuItemView>::new(),
            fields: vec![FieldView {
                label: strings::LABEL,
                value: "*".repeat(self.value.chars().count()),
                active: true,
            }],
            notice: self.notice,
            footer: strings::FOOTER,
        }
    }
}

/// The signed-in placeholder for an account with `role`.
#[must_use]
pub fn signed_in_view(role: crate::store::Role) -> View {
    let body = match role {
        crate::store::Role::Sysop => strings::SIGNED_IN_SYSOP,
        crate::store::Role::Member => strings::SIGNED_IN_MEMBER,
    };
    View {
        title: strings::TITLE,
        heading: strings::SIGNED_IN_HEADING,
        banner: Vec::new(),
        body: vec![body.to_owned()],
        note: Some(strings::PLACEHOLDER_NOTE),
        menu: Vec::new(),
        fields: Vec::new(),
        notice: None,
        footer: strings::SIGNED_IN_FOOTER,
    }
}
