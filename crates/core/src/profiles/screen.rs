//! The members screens as a pure state machine: the paged list with a handle search, a
//! profile, and the edit form. It does no I/O and draws nothing. A key either changes the state
//! or asks for a [`Request`] (which the front end runs with [`execute`](super::execute) and
//! answers through [`MembersScreen::deliver`]); what to show comes out as a [`ScreenView`] of
//! string-table keys and sanitized content, so any layout can draw it and a theme can change
//! every word.
//!
//! While a request is outstanding keys are ignored, so a screen never acts on a list it is
//! about to replace. Which keys are offered depends on the permissions the session holds *now*
//! (`e` appears only if an edit would be allowed), but the real gate is [`execute`], which
//! checks again with the snapshot of the action.

use crate::access::{Permission, PermissionCheck};
use crate::sanitize::{SingleLine, USERNAME_MAX_CHARS};
use crate::store::AccountId;

use super::fields::{Field, RawProfile};
use super::keys;
use super::service::{MemberPage, Request, Response};
use super::store::{Cursor, HandlePrefix, MemberQuery, MemberRecord, MemberSummary};

/// A key as the members screens see it. Quit (Ctrl-C) is the session's rule, not a screen's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenKey {
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
    /// Leave the screens, back to the menu.
    Leave,
    /// Leave, and say the action was not permitted.
    Denied,
}

/// One line of a screen, for a layout to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// An empty line.
    Blank,
    /// A heading (string-table key).
    Heading(&'static str),
    /// Fixed text (string-table key).
    Text(&'static str),
    /// Fixed text with `{name}` placeholders filled with sanitized values.
    Filled {
        /// The string-table key.
        key: &'static str,
        /// Placeholder names and their sanitized values.
        values: Vec<(&'static str, String)>,
    },
    /// A label (key) and a sanitized value; `None` when the field is empty.
    Pair {
        /// The label's string-table key.
        label: &'static str,
        /// The value, or `None` for "not set".
        value: Option<String>,
    },
    /// A list row picked by `key`: the handle and the sanitized display name.
    Row {
        /// The key that opens it.
        key: char,
        /// The handle.
        handle: String,
        /// The display name, empty if none.
        name: String,
    },
    /// A form field: label (key), sanitized text so far, and whether it has the cursor.
    Field {
        /// The label's string-table key.
        label: &'static str,
        /// The text so far.
        value: String,
        /// Whether typing goes here.
        active: bool,
    },
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
pub struct ScreenView {
    /// The lines, top to bottom.
    pub lines: Vec<Line>,
    /// The string-table key of the footer (the keys on offer).
    pub footer: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ListState {
    /// The search in force.
    prefix: HandlePrefix,
    /// The search being typed, if the person pressed `/`.
    typing: Option<String>,
    rows: Vec<MemberSummary>,
    next: Option<Cursor>,
    /// The `after` of every page shown so far; the last is the page on screen.
    trail: Vec<Option<Cursor>>,
    loaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditState {
    record: MemberRecord,
    raw: RawProfile,
    active: usize,
    /// Whether Esc returns to the profile (it was opened from one) or leaves.
    from_view: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    /// Waiting for the first answer.
    Loading,
    List(ListState),
    View(MemberRecord),
    Edit(Box<EditState>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Pending {
    request: Request,
    sent: bool,
}

/// The members list, a profile and the edit form, for one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembersScreen {
    own: Option<AccountId>,
    mode: Mode,
    /// The list to return to from a profile opened out of it.
    back: Option<ListState>,
    pending: Option<Pending>,
    notice: Option<(bool, &'static str)>,
}

impl MembersScreen {
    /// Open the member list (first page).
    #[must_use]
    pub fn list(own: Option<AccountId>) -> Self {
        let mut screen = Self::blank(own, Mode::List(ListState::default()));
        screen.ask(Request::List(MemberQuery::default()));
        screen
    }

    /// Open the session's own profile.
    #[must_use]
    pub fn own_profile(own: AccountId) -> Self {
        let mut screen = Self::blank(Some(own), Mode::Loading);
        screen.ask(Request::View(own));
        screen
    }

    /// Open the form to edit the session's own profile.
    #[must_use]
    pub fn edit_own(own: AccountId) -> Self {
        let mut screen = Self::blank(Some(own), Mode::Loading);
        screen.ask(Request::Edit(own));
        screen
    }

    const fn blank(own: Option<AccountId>, mode: Mode) -> Self {
        Self {
            own,
            mode,
            back: None,
            pending: None,
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
    /// [`MembersScreen::deliver`].
    pub fn take_request(&mut self) -> Option<Request> {
        let pending = self.pending.as_mut().filter(|p| !p.sent)?;
        pending.sent = true;
        Some(pending.request.clone())
    }

    /// Whether the session may edit this member's profile now (the key is offered; the real
    /// check is in `execute`).
    fn can_edit(&self, access: &dyn PermissionCheck, record: &MemberRecord) -> bool {
        if self.own == Some(record.id) {
            access.allows(Permission::AccountEditProfile)
        } else {
            access.allows(Permission::MembersEdit)
        }
    }

    /// Whether the session still holds what the open screen needs: `members.view` for the list
    /// and a profile, the edit permission that covers the member for the form. The front end
    /// closes the screen when this turns false, as it does for the settings page (a change
    /// applies on the next action).
    #[must_use]
    pub fn still_permitted(&self, access: &dyn PermissionCheck) -> bool {
        match &self.mode {
            Mode::Loading => true,
            Mode::List(_) | Mode::View(_) => access.allows(Permission::MembersView),
            Mode::Edit(edit) => self.can_edit(access, &edit.record),
        }
    }

    /// Handle one key.
    pub fn press(&mut self, access: &dyn PermissionCheck, key: ScreenKey) -> Step {
        if self.pending.is_some() {
            // The answer to the last request has not come; acting on this screen now would act
            // on a list that is about to change. Only leaving is always possible.
            return if key == ScreenKey::Escape && matches!(self.mode, Mode::Loading) {
                Step::Leave
            } else {
                Step::Stay
            };
        }
        self.notice = None;
        match &mut self.mode {
            Mode::Loading => Self::loading_key(key),
            Mode::List(_) => self.list_key(key),
            Mode::View(_) => self.view_key(access, key),
            Mode::Edit(_) => self.edit_key(key),
        }
    }

    const fn loading_key(key: ScreenKey) -> Step {
        match key {
            ScreenKey::Escape => Step::Leave,
            _ => Step::Stay,
        }
    }

    fn list_key(&mut self, key: ScreenKey) -> Step {
        let Mode::List(list) = &mut self.mode else {
            return Step::Stay;
        };
        match list_action(list, key) {
            ListAct::Nothing => Step::Stay,
            ListAct::Leave => Step::Leave,
            ListAct::Ask(request) => {
                self.ask(request);
                Step::Stay
            }
        }
    }

    fn view_key(&mut self, access: &dyn PermissionCheck, key: ScreenKey) -> Step {
        let Mode::View(record) = &self.mode else {
            return Step::Stay;
        };
        match key {
            ScreenKey::Escape => match self.back.take() {
                Some(list) => {
                    self.mode = Mode::List(list);
                    Step::Stay
                }
                None => Step::Leave,
            },
            ScreenKey::Char('e' | 'E') if self.can_edit(access, record) => {
                let id = record.id;
                self.ask(Request::Edit(id));
                Step::Stay
            }
            ScreenKey::Char(_) | ScreenKey::Enter | ScreenKey::Backspace => Step::Stay,
        }
    }

    fn edit_key(&mut self, key: ScreenKey) -> Step {
        let Mode::Edit(edit) = &mut self.mode else {
            return Step::Stay;
        };
        let field = Field::ALL.get(edit.active).copied().unwrap_or(Field::About);
        match key {
            ScreenKey::Char(c) if !c.is_control() => {
                let mut text = edit.raw.get(field).to_owned();
                if text.chars().count() < field.max_chars() {
                    text.push(c);
                    edit.raw.set(field, text);
                }
            }
            ScreenKey::Backspace => {
                let mut text = edit.raw.get(field).to_owned();
                text.pop();
                edit.raw.set(field, text);
            }
            ScreenKey::Enter => {
                if edit.active + 1 < Field::ALL.len() {
                    edit.active += 1;
                } else {
                    let request = Request::Save(edit.record.id, edit.raw.clone());
                    self.ask(request);
                }
            }
            ScreenKey::Escape => {
                let (record, from_view) = (edit.record.clone(), edit.from_view);
                if from_view {
                    self.mode = Mode::View(record);
                } else {
                    return Step::Leave;
                }
            }
            ScreenKey::Char(_) => {}
        }
        Step::Stay
    }

    /// Give back the answer to the outstanding request.
    pub fn deliver(&mut self, response: Response) -> Step {
        self.pending = None;
        self.notice = None;
        match response {
            Response::Page(query, page) => self.show_page(&query, page),
            Response::Profile(record) => {
                if let Mode::List(list) = std::mem::replace(&mut self.mode, Mode::Loading) {
                    self.back = Some(list);
                }
                self.mode = Mode::View(record);
            }
            Response::EditForm(record) => {
                let from_view = matches!(self.mode, Mode::View(_));
                self.mode = Mode::Edit(Box::new(EditState {
                    raw: record.profile.to_raw(),
                    record,
                    active: 0,
                    from_view,
                }));
            }
            Response::Saved(saved) => {
                if let Mode::Edit(edit) = &self.mode {
                    let record = MemberRecord {
                        id: saved.target,
                        handle: edit.record.handle.clone(),
                        profile: saved.profile,
                    };
                    self.mode = Mode::View(record);
                    self.notice = Some((
                        false,
                        if saved.changed.is_empty() {
                            keys::NOTICE_UNCHANGED
                        } else {
                            keys::NOTICE_SAVED
                        },
                    ));
                }
            }
            Response::NotFound => {
                self.notice = Some((true, keys::NOTICE_NOT_FOUND));
                if let Some(list) = self.back.take() {
                    self.mode = Mode::List(list);
                } else if matches!(self.mode, Mode::Loading | Mode::View(_) | Mode::Edit(_)) {
                    return Step::Leave;
                }
            }
            Response::NotPermitted => return Step::Denied,
            Response::Failed => self.notice = Some((true, keys::NOTICE_FAILED)),
        }
        Step::Stay
    }

    fn show_page(&mut self, query: &MemberQuery, page: MemberPage) {
        let mut list = match std::mem::replace(&mut self.mode, Mode::Loading) {
            Mode::List(list) => list,
            _ => ListState::default(),
        };
        match &query.after {
            None => list.trail = vec![None],
            Some(after) => {
                let at = list
                    .trail
                    .iter()
                    .position(|seen| seen.as_ref() == Some(after));
                match at {
                    Some(index) => list.trail.truncate(index + 1),
                    None => list.trail.push(Some(after.clone())),
                }
            }
        }
        list.prefix = query.prefix.clone();
        list.rows = page.members;
        list.next = page.next;
        list.loaded = true;
        self.mode = Mode::List(list);
    }

    /// What to draw, for a session holding `access`.
    #[must_use]
    pub fn view(&self, access: &dyn PermissionCheck) -> ScreenView {
        let mut view = match &self.mode {
            Mode::Loading => ScreenView {
                lines: Vec::new(),
                footer: keys::VIEW_FOOTER,
            },
            Mode::List(list) => list_view(list),
            Mode::View(record) => self.record_view(access, record),
            Mode::Edit(edit) => self.edit_view(edit),
        };
        if let Some((error, text)) = self.notice {
            view.lines.push(Line::Blank);
            view.lines.push(Line::Notice { error, text });
        }
        view
    }

    fn record_view(&self, access: &dyn PermissionCheck, record: &MemberRecord) -> ScreenView {
        let mut lines = vec![
            Line::Heading(keys::VIEW_TITLE),
            Line::Pair {
                label: keys::LABEL_HANDLE,
                value: Some(record.handle.as_str().to_owned()),
            },
        ];
        lines.extend(Field::ALL.iter().map(|field| Line::Pair {
            label: field.label_key(),
            value: Some(record.profile.get(*field).to_owned()).filter(|text| !text.is_empty()),
        }));
        ScreenView {
            lines,
            footer: if self.can_edit(access, record) {
                keys::VIEW_FOOTER_EDIT
            } else {
                keys::VIEW_FOOTER
            },
        }
    }

    fn edit_view(&self, edit: &EditState) -> ScreenView {
        let others = self.own != Some(edit.record.id);
        let handle = edit.record.handle.as_str().to_owned();
        let mut lines = vec![
            if others {
                Line::Filled {
                    key: keys::EDIT_TITLE_OTHER,
                    values: vec![("handle", handle)],
                }
            } else {
                Line::Heading(keys::EDIT_TITLE)
            },
            // The honest statement comes first and is always shown: the server can read the
            // profile and so can every member. The text says so and never calls anything private.
            Line::Text(keys::EDIT_READABLE),
            Line::Text(keys::EDIT_PLAIN_TEXT),
            Line::Blank,
        ];
        for (index, field) in Field::ALL.iter().enumerate() {
            lines.push(Line::Field {
                label: field.label_key(),
                value: SingleLine::new(field.max_chars())
                    .sanitize(edit.raw.get(*field))
                    .into_string(),
                active: index == edit.active,
            });
        }
        ScreenView {
            lines,
            footer: keys::EDIT_FOOTER,
        }
    }
}

/// What a key does on the list.
enum ListAct {
    Nothing,
    Leave,
    Ask(Request),
}

fn list_action(list: &mut ListState, key: ScreenKey) -> ListAct {
    if let Some(typed) = &mut list.typing {
        match key {
            ScreenKey::Char(c) => {
                if (c.is_ascii_alphanumeric() || c == '_' || c == '-')
                    && typed.chars().count() < USERNAME_MAX_CHARS
                {
                    typed.push(c);
                }
            }
            ScreenKey::Backspace => {
                typed.pop();
            }
            ScreenKey::Escape => list.typing = None,
            ScreenKey::Enter => {
                let query = MemberQuery {
                    prefix: HandlePrefix::parse(typed),
                    after: None,
                };
                list.typing = None;
                return ListAct::Ask(Request::List(query));
            }
        }
        return ListAct::Nothing;
    }
    let prefix = list.prefix.clone();
    match key {
        ScreenKey::Escape => return ListAct::Leave,
        ScreenKey::Char('/' | 's' | 'S') => list.typing = Some(String::new()),
        ScreenKey::Char('c' | 'C') if !prefix.is_empty() => {
            return ListAct::Ask(Request::List(MemberQuery::default()));
        }
        ScreenKey::Char('n' | 'N' | '>') => {
            if let Some(next) = list.next.clone() {
                return ListAct::Ask(Request::List(MemberQuery {
                    prefix,
                    after: Some(next),
                }));
            }
        }
        ScreenKey::Char('p' | 'P' | '<') => {
            let before = list
                .trail
                .len()
                .checked_sub(2)
                .and_then(|index| list.trail.get(index));
            if let Some(before) = before {
                return ListAct::Ask(Request::List(MemberQuery {
                    prefix,
                    after: before.clone(),
                }));
            }
        }
        ScreenKey::Char(digit) => {
            let row = digit
                .to_digit(10)
                .and_then(|n| usize::try_from(n).ok())
                .and_then(|n| n.checked_sub(1))
                .and_then(|index| list.rows.get(index));
            if let Some(row) = row {
                return ListAct::Ask(Request::View(row.id));
            }
        }
        ScreenKey::Enter | ScreenKey::Backspace => {}
    }
    ListAct::Nothing
}

fn list_view(list: &ListState) -> ScreenView {
    let mut lines = vec![Line::Heading(keys::LIST_TITLE)];
    lines.push(match &list.typing {
        Some(typed) => Line::Filled {
            key: keys::LIST_SEARCH,
            values: vec![("prefix", format!("{typed}_"))],
        },
        None if list.prefix.is_empty() => Line::Text(keys::LIST_SEARCH_NONE),
        None => Line::Filled {
            key: keys::LIST_SEARCH,
            values: vec![("prefix", list.prefix.as_str().to_owned())],
        },
    });
    lines.push(Line::Blank);
    if list.rows.is_empty() {
        if list.loaded {
            lines.push(Line::Text(if list.prefix.is_empty() {
                keys::LIST_EMPTY
            } else {
                keys::LIST_NO_MATCH
            }));
        }
    } else {
        for (index, row) in list.rows.iter().enumerate() {
            let digit = u32::try_from(index + 1)
                .ok()
                .and_then(|n| char::from_digit(n, 10))
                .unwrap_or('?');
            lines.push(Line::Row {
                key: digit,
                handle: row.handle.as_str().to_owned(),
                name: row.display_name.clone(),
            });
        }
    }
    lines.push(Line::Blank);
    lines.push(Line::Filled {
        key: keys::LIST_PAGE,
        values: vec![("page", list.trail.len().max(1).to_string())],
    });
    ScreenView {
        lines,
        footer: if list.typing.is_some() {
            keys::LIST_FOOTER_SEARCH
        } else {
            keys::LIST_FOOTER
        },
    }
}
