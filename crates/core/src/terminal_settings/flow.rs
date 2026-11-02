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
use super::settings::{SettingsError, TerminalSettings};
use super::strings::{self, fill};
use super::{format_utc, tier_name};
use crate::access::{Permission, PermissionCheck, Slug, Snapshot};
use crate::icons::Tier;
use crate::menu::{Action, MenuKey, MenuOutcome, MenuPage, MenuSession, main_tree};
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
        }
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

    /// Whether a page that changes the account's terminal settings is showing. The first-run
    /// glyph test is not one: it runs before the menu, and what it picks is kept for the session
    /// (the store refuses to save it without the permission).
    const fn settings_page_open(&self) -> bool {
        match &self.page {
            Page::Settings | Page::EditWidth(_) | Page::EditHeight(_) => true,
            Page::GlyphTest(test) => !test.is_first_run(),
            Page::Login | Page::Home | Page::Logoff => false,
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
            MemberInput::Enter | MemberInput::Backspace | MemberInput::Quit => {
                return MemberEffect::Stay;
            }
        };
        match self.menu.press(&self.access, key) {
            MenuOutcome::Stay => MemberEffect::Stay,
            MenuOutcome::Action(Action::TerminalSettings) => {
                self.page = Page::Settings;
                MemberEffect::Stay
            }
            MenuOutcome::Action(Action::LogOff) => self.log_off(),
        }
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

    /// The content part of the home page: who is signed in. The menu itself is
    /// [`MemberFlow::menu_page`].
    fn home_view(&self, view: &mut View) {
        view.body
            .push(fill(strings::SIGNED_IN_AS, &[("handle", &self.handle)]));
    }

    fn settings_view(&self, view: &mut View) {
        let s = &self.settings;
        let number = |value: Option<u16>| {
            value.map_or_else(|| strings::AUTOMATIC.to_owned(), |n| n.to_string())
        };
        view.heading = strings::SETTINGS_HEADING;
        let (cut_width, cut_height) = self.terminal.map_or((false, false), |(cols, rows)| {
            s.overrides_clamped(cols, rows)
        });
        let (cols, rows) = self.terminal.unwrap_or_default();
        let mut width = fill(strings::SETTING_WIDTH, &[("value", &number(s.width()))]);
        if cut_width {
            width.push_str(&fill(
                strings::CLAMPED_WIDTH,
                &[("terminal", &cols.to_string())],
            ));
        }
        let mut height = fill(strings::SETTING_HEIGHT, &[("value", &number(s.height()))]);
        if cut_height {
            height.push_str(&fill(
                strings::CLAMPED_HEIGHT,
                &[("terminal", &rows.to_string())],
            ));
        }
        view.body = vec![
            width,
            height,
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
