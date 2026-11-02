//! The signed-in session as a state machine: login screen, the glyph test on first login, a
//! placeholder home, the terminal settings screen and logoff (M1-E). M2 replaces the
//! placeholder home with the real menu tree; the screens around it stay.

use super::glyph::{GlyphAnswer, GlyphProgress, GlyphTest};
use super::settings::{SettingsError, TerminalSettings};
use super::strings::{self, fill};
use super::{format_utc, tier_name};
use crate::icons::Tier;
use crate::sanitize::{SingleLine, USERNAME_MAX_CHARS, Username};
use crate::store::{Role, UnixSeconds};
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
    /// Redraw (the logoff screen is showing) and close the session.
    Disconnect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Page {
    Login,
    GlyphTest(GlyphTest),
    Home,
    Settings,
    EditWidth(String),
    EditHeight(String),
    Logoff,
}

/// One signed-in session.
#[derive(Debug)]
pub struct MemberFlow {
    handle: String,
    role: Role,
    last_login: Option<UnixSeconds>,
    settings: TerminalSettings,
    page: Page,
    notice: Option<Notice>,
}

impl MemberFlow {
    /// Start at the login screen. The handle is sanitized again here, so the screen never
    /// relies on the caller having done it.
    #[must_use]
    pub fn new(
        handle: &Username,
        role: Role,
        last_login: Option<UnixSeconds>,
        settings: TerminalSettings,
    ) -> Self {
        Self {
            handle: SingleLine::new(USERNAME_MAX_CHARS)
                .sanitize(handle.as_str())
                .into_string(),
            role,
            last_login,
            settings,
            page: Page::Login,
            notice: None,
        }
    }

    /// The settings in force, for the renderer (tier, color, size overrides).
    #[must_use]
    pub const fn settings(&self) -> &TerminalSettings {
        &self.settings
    }

    /// The account's role. Shown nowhere yet; the permission model is M2's (D33).
    #[must_use]
    pub const fn role(&self) -> Role {
        self.role
    }

    /// Whether the logoff screen is showing.
    #[must_use]
    pub const fn is_logged_off(&self) -> bool {
        matches!(self.page, Page::Logoff)
    }

    /// Handle one input.
    pub fn input(&mut self, input: MemberInput) -> MemberEffect {
        if input == MemberInput::Quit {
            return self.log_off();
        }
        self.notice = None;
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
            Page::Settings => self.settings_input(input),
            Page::EditWidth(_) | Page::EditHeight(_) => self.edit_input(input),
            Page::Logoff => MemberEffect::Disconnect,
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
        match input {
            MemberInput::Char('s' | 'S') => {
                self.page = Page::Settings;
                MemberEffect::Stay
            }
            MemberInput::Char('l' | 'L' | 'q' | 'Q') => self.log_off(),
            _ => MemberEffect::Stay,
        }
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
            MemberInput::Char('i' | 'I') => {
                self.page = Page::GlyphTest(GlyphTest::new(self.settings.needs_glyph_test()));
                MemberEffect::Stay
            }
            MemberInput::Char('b' | 'B') | MemberInput::Escape => {
                self.page = Page::Home;
                MemberEffect::Stay
            }
            _ => MemberEffect::Stay,
        }
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
            MemberInput::Char(_) | MemberInput::Quit => MemberEffect::Stay,
        }
    }

    /// The lines left on the client's normal screen after it leaves the alternate one, so the
    /// logoff text stays in the scrollback. `None` until the person has logged off.
    #[must_use]
    pub fn farewell(&self) -> Option<Vec<String>> {
        self.is_logged_off().then(|| self.view().body)
    }

    /// The current page as a [`View`]. Every string is chrome or the sanitized handle.
    #[must_use]
    pub fn view(&self) -> View {
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
            Page::Login => self.login_view(&mut view),
            Page::GlyphTest(test) => glyph_view(&mut view, *test),
            Page::Home => self.home_view(&mut view),
            Page::Settings => self.settings_view(&mut view),
            Page::EditWidth(typed) => edit_view(&mut view, strings::FIELD_WIDTH, typed),
            Page::EditHeight(typed) => edit_view(&mut view, strings::FIELD_HEIGHT, typed),
            Page::Logoff => self.logoff_view(&mut view),
        }
        view
    }

    fn login_view(&self, view: &mut View) {
        view.heading = strings::LOGIN_HEADING;
        view.body
            .push(fill(strings::SIGNED_IN_AS, &[("handle", &self.handle)]));
        view.body.push(match self.last_login {
            Some(at) => fill(strings::LAST_LOGIN, &[("when", &format_utc(at))]),
            None => strings::FIRST_LOGIN.to_owned(),
        });
        view.body.push(String::new());
        view.body.push(self.terminal_line());
        view.footer = strings::LOGIN_FOOTER;
    }

    fn terminal_line(&self) -> String {
        match self.settings.chosen_tier() {
            None => strings::TERMINAL_LINE_UNSET.to_owned(),
            Some(tier) => fill(
                strings::TERMINAL_LINE,
                &[
                    ("icons", tier_name(tier)),
                    ("color", on_off(self.settings.use_color())),
                    ("pause", on_off(self.settings.pause())),
                ],
            ),
        }
    }

    fn home_view(&self, view: &mut View) {
        view.heading = strings::HOME_HEADING;
        view.body
            .push(fill(strings::SIGNED_IN_AS, &[("handle", &self.handle)]));
        view.note = Some(strings::PLACEHOLDER_NOTE);
        view.menu = vec![
            MenuItemView {
                key: 's',
                label: strings::MENU_SETTINGS,
            },
            MenuItemView {
                key: 'l',
                label: strings::MENU_LOGOFF,
            },
        ];
        view.footer = strings::HOME_FOOTER;
    }

    fn settings_view(&self, view: &mut View) {
        let s = &self.settings;
        let number = |value: Option<u16>| {
            value.map_or_else(|| strings::AUTOMATIC.to_owned(), |n| n.to_string())
        };
        view.heading = strings::SETTINGS_HEADING;
        view.body = vec![
            fill(strings::SETTING_WIDTH, &[("value", &number(s.width()))]),
            fill(strings::SETTING_HEIGHT, &[("value", &number(s.height()))]),
            fill(strings::SETTING_COLOR, &[("value", on_off(s.color()))]),
            fill(strings::SETTING_PAUSE, &[("value", on_off(s.pause()))]),
            fill(
                strings::SETTING_ICONS,
                &[(
                    "value",
                    s.chosen_tier().map_or(strings::NOT_CHOSEN, tier_name),
                )],
            ),
        ];
        view.menu = vec![
            MenuItemView {
                key: 'w',
                label: strings::MENU_WIDTH,
            },
            MenuItemView {
                key: 'h',
                label: strings::MENU_HEIGHT,
            },
            MenuItemView {
                key: 'c',
                label: strings::MENU_COLOR,
            },
            MenuItemView {
                key: 'p',
                label: strings::MENU_PAUSE,
            },
            MenuItemView {
                key: 'i',
                label: strings::MENU_ICONS,
            },
            MenuItemView {
                key: 'b',
                label: strings::MENU_BACK,
            },
        ];
        view.footer = strings::SETTINGS_FOOTER;
    }

    fn logoff_view(&self, view: &mut View) {
        view.heading = strings::LOGOFF_HEADING;
        view.body = vec![
            fill(strings::GOODBYE, &[("handle", &self.handle)]),
            strings::SETTINGS_SAVED_NOTE.to_owned(),
        ];
    }
}

fn glyph_view(view: &mut View, test: GlyphTest) {
    view.heading = strings::GLYPH_HEADING;
    view.body = vec![
        strings::GLYPH_WHY.to_owned(),
        String::new(),
        fill(
            strings::GLYPH_STYLE,
            &[("style", tier_name(test.step().tier()))],
        ),
        // Registry glyphs for this tier; the legend names them for a screen reader.
        test.samples(),
        strings::GLYPH_LEGEND.to_owned(),
        String::new(),
        strings::GLYPH_QUESTION.to_owned(),
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
