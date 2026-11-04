//! The claim screen: one field for the token, a few attempts, one generic failure.
//!
//! The screen is identical whether or not the board is claimed (D41): it never learns the
//! state. After a rejection it says the same words every time.

use super::token::TOKEN_TEXT_MAX_CHARS;
use crate::passwords::{MAX_CHARS as PASSWORD_MAX_CHARS, PolicyError, Secret, check_new};
use crate::waiting_room::{FieldView, MenuItemView, Notice, NoticeKind, View};
use zeroize::Zeroize as _;

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
    /// The password field's label, when the claim also sets the founder's password (D94 (6)).
    pub const LABEL_PASSWORD: &str = "Choose a password (12 to 128 characters)";
    /// The repeat field's label.
    pub const LABEL_PASSWORD_AGAIN: &str = "Type the password again";
    /// The two entries differ.
    pub const PASSWORDS_DIFFER: &str = "The two passwords did not match. Please type them again.";
    /// Key hints.
    pub const FOOTER: &str = "Enter: send   Esc: disconnect";
    /// The one failure message, for every kind of failure.
    pub const REJECTED: &str = "That token was not accepted.";
    /// Shown when the session has used its attempts.
    pub const TOO_MANY: &str = "Too many attempts. Goodbye.";
    /// Shown while a claim is being checked.
    pub const WORKING: &str = "Checking...";
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
    /// Check this token and, if it is right, make the founder with this password (D94 (6)).
    /// Then call [`ClaimForm::rejected`] or [`ClaimForm::accepted`].
    SubmitWithPassword {
        /// The token as typed.
        token: String,
        /// The password the founder chose.
        password: Secret,
    },
    /// Redraw once, then close the session.
    Disconnect,
}

/// One session's claim prompt.
#[derive(Debug)]
pub struct ClaimForm {
    value: String,
    /// The password and its repeat, when the form asks for them. `Some` also means stage 1 and
    /// 2 exist.
    secrets: Option<[String; 2]>,
    /// Which field has the keys: 0 token, 1 password, 2 repeat.
    stage: usize,
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
            secrets: None,
            stage: 0,
            notice: None,
            attempts: 0,
            max_attempts,
            done: false,
        }
    }

    /// Ask for the founder's password and its repeat after the token (D94 (6)): every account
    /// has a password, the founder's included.
    #[must_use]
    pub fn with_password(mut self) -> Self {
        self.secrets = Some([
            String::with_capacity(PASSWORD_MAX_CHARS),
            String::with_capacity(PASSWORD_MAX_CHARS),
        ]);
        self
    }

    fn wipe_secrets(&mut self) {
        if let Some(secrets) = &mut self.secrets {
            for secret in secrets {
                secret.zeroize();
            }
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
                if self.stage == 0 {
                    if (c.is_ascii_alphanumeric() || c == '-')
                        && self.value.chars().count() < TOKEN_TEXT_MAX_CHARS
                    {
                        self.value.push(c);
                    }
                } else if let Some(secret) = self
                    .secrets
                    .as_mut()
                    .and_then(|secrets| secrets.get_mut(self.stage - 1))
                {
                    // Printable ASCII, exactly as typed (what a telnet login can type back).
                    if (c.is_ascii_graphic() || c == ' ')
                        && secret.chars().count() < PASSWORD_MAX_CHARS
                    {
                        secret.push(c);
                    }
                }
                ClaimEffect::Stay
            }
            ClaimInput::Backspace => {
                if self.stage == 0 {
                    self.value.pop();
                } else if let Some(secret) = self
                    .secrets
                    .as_mut()
                    .and_then(|secrets| secrets.get_mut(self.stage - 1))
                {
                    secret.pop();
                }
                ClaimEffect::Stay
            }
            ClaimInput::Enter if self.secrets.is_some() && self.stage < 2 => {
                self.stage += 1;
                ClaimEffect::Stay
            }
            ClaimInput::Enter if self.secrets.is_some() => {
                self.attempts += 1;
                if self.attempts > self.max_attempts {
                    self.notice = Some(Notice {
                        kind: NoticeKind::Error,
                        text: strings::TOO_MANY,
                    });
                    return ClaimEffect::Disconnect;
                }
                let [chosen, again] = self.secrets.clone().unwrap_or_default();
                let local = if chosen != again {
                    Some(strings::PASSWORDS_DIFFER)
                } else {
                    check_new(&chosen).err().map(|policy| match policy {
                        PolicyError::TooShort => crate::passwords::strings::ERR_SHORT,
                        PolicyError::TooLong => crate::passwords::strings::ERR_LONG,
                        PolicyError::ControlCharacter => crate::passwords::strings::ERR_CONTROL,
                    })
                };
                if let Some(text) = local {
                    // A typing mistake, said as one: nothing was sent, the token stays.
                    self.wipe_secrets();
                    self.stage = 1;
                    self.notice = Some(Notice {
                        kind: NoticeKind::Error,
                        text,
                    });
                    return ClaimEffect::Stay;
                }
                let password = Secret::new(chosen);
                self.wipe_secrets();
                self.stage = 0;
                ClaimEffect::SubmitWithPassword {
                    token: std::mem::take(&mut self.value),
                    password,
                }
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
        // Start again from the token: nothing about the failure says which part was wrong.
        self.stage = 0;
        self.wipe_secrets();
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
        self.wipe_secrets();
    }

    fn fields(&self) -> Vec<FieldView> {
        let mut fields = vec![FieldView {
            label: strings::LABEL,
            value: "*".repeat(self.value.chars().count()),
            active: self.stage == 0,
        }];
        if let Some(secrets) = &self.secrets {
            for (index, (label, secret)) in [strings::LABEL_PASSWORD, strings::LABEL_PASSWORD_AGAIN]
                .into_iter()
                .zip(secrets)
                .enumerate()
            {
                fields.push(FieldView {
                    label,
                    value: "*".repeat(secret.chars().count()),
                    active: self.stage == index + 1,
                });
            }
        }
        fields
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
            fields: self.fields(),
            notice: self.notice,
            footer: strings::FOOTER,
        }
    }
}
