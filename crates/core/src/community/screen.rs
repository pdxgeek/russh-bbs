//! The community screens as a pure state machine: no store, no drawing, no clock. It emits
//! string-table keys and already-sanitized content as a [`ScreenView`] (the members screens'
//! view type, so the server draws it with the same code), and asks for work as [`Request`]s.
//!
//! Keys that arrive while a request is outstanding are ignored except Esc, so a key never acts
//! on a list that is about to be replaced. The poll description is typed in the full-screen
//! editor, which is the server's widget: the screen only asks for it ([`EditorAsk`]) and takes
//! back the text.

use crate::access::{Permission, PermissionCheck};
use crate::profiles::{Line, ScreenKey, ScreenView, Step};
use crate::terminal_settings::format_utc;

use super::keys;
use super::service::{CallerRow, Entry, Note, PollDetail, PollRow, Request, Response};
use super::types::{
    LineKind, OPTION_MAX, OPTIONS_MAX, OPTIONS_MIN, POLL_PAGE, QUESTION_MAX, ResultsVisibility,
};

/// Which feature the menu item opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// The one-liners.
    Oneliners,
    /// The graffiti wall.
    Graffiti,
    /// Last callers.
    LastCallers,
    /// The poll list.
    Polls,
}

/// A request to open the full-screen editor for a poll's description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorAsk {
    /// The text so far (sanitized), empty for the first time.
    pub initial: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Typing {
    Idle,
    Line(String),
    Remove(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormStep {
    Question,
    Option(usize),
    Review,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Form {
    step: FormStep,
    question: String,
    options: Vec<String>,
    description: String,
    visibility: ResultsVisibility,
}

impl Form {
    fn new() -> Self {
        Self {
            step: FormStep::Question,
            question: String::new(),
            options: vec![String::new(); OPTIONS_MAX],
            description: String::new(),
            visibility: ResultsVisibility::default(),
        }
    }

    fn filled(&self) -> usize {
        self.options.iter().filter(|o| !o.trim().is_empty()).count()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Loading,
    Lines {
        kind: LineKind,
        entries: Vec<Entry>,
        typing: Typing,
    },
    Callers(Vec<CallerRow>),
    Polls {
        rows: Vec<PollRow>,
        total: usize,
    },
    Poll(Box<PollDetail>),
    Form(Box<Form>),
}

/// One open community screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommunityScreen {
    mode: Mode,
    /// A request not yet taken by the front end.
    pending: Option<Request>,
    /// Whether an answer is owed (a request was set and its response has not arrived).
    waiting: bool,
    note: Option<Note>,
    editor: Option<EditorAsk>,
    /// Where the poll list starts, so leaving a poll returns to the same page.
    offset: usize,
}

const OPTION_KEYS: [&str; OPTIONS_MAX] = [
    keys::FORM_OPTION_1,
    keys::FORM_OPTION_2,
    keys::FORM_OPTION_3,
    keys::FORM_OPTION_4,
    keys::FORM_OPTION_5,
    keys::FORM_OPTION_6,
];

impl CommunityScreen {
    /// Open `section`; its first request is waiting to be taken.
    #[must_use]
    pub fn open(section: Section) -> Self {
        let request = match section {
            Section::Oneliners => Request::Lines(LineKind::Oneliner),
            Section::Graffiti => Request::Lines(LineKind::Graffiti),
            Section::LastCallers => Request::Callers,
            Section::Polls => Request::Polls { offset: 0 },
        };
        let mut screen = Self {
            mode: Mode::Loading,
            pending: None,
            waiting: false,
            note: None,
            editor: None,
            offset: 0,
        };
        screen.ask(request);
        screen
    }

    fn ask(&mut self, request: Request) {
        self.pending = Some(request);
        self.waiting = true;
    }

    /// Whether the session still holds what the screens need. The flow closes the screen when
    /// this turns false (a change applies on the next action).
    #[must_use]
    pub fn still_permitted(&self, access: &dyn PermissionCheck) -> bool {
        access.allows(Permission::CommunityView)
    }

    /// The request the screen is waiting on, once. Run it with [`super::execute`] and give the
    /// answer to [`CommunityScreen::deliver`].
    pub fn take_request(&mut self) -> Option<Request> {
        self.pending.take()
    }

    /// The editor the screen wants opened, once. Give the result to
    /// [`CommunityScreen::deliver_description`].
    pub fn take_editor(&mut self) -> Option<EditorAsk> {
        self.editor.take()
    }

    /// The editor finished. `Some(text)` is the description to keep (already sanitized by the
    /// editor); `None` keeps what was there.
    pub fn deliver_description(&mut self, text: Option<String>) {
        if let (Mode::Form(form), Some(text)) = (&mut self.mode, text) {
            text.trim().clone_into(&mut form.description);
        }
    }

    /// Whether the screen is showing the poll form (the editor can be opened from it).
    #[must_use]
    pub fn in_form(&self) -> bool {
        matches!(self.mode, Mode::Form(_))
    }

    /// Handle one key.
    pub fn press(&mut self, access: &dyn PermissionCheck, key: ScreenKey) -> Step {
        if !access.allows(Permission::CommunityView) {
            return Step::Denied;
        }
        if self.waiting || self.editor.is_some() {
            // The answer to the last request has not come; acting now would act on a list that
            // is about to change. Only leaving is always possible.
            return if key == ScreenKey::Escape {
                Step::Leave
            } else {
                Step::Stay
            };
        }
        self.note = None;
        match &mut self.mode {
            Mode::Loading | Mode::Callers(_) => leave_on_escape(key),
            Mode::Lines { .. } => self.lines_key(access, key),
            Mode::Polls { .. } => self.polls_key(access, key),
            Mode::Poll(_) => self.poll_key(access, key),
            Mode::Form(_) => self.form_key(key),
        }
    }

    fn lines_key(&mut self, access: &dyn PermissionCheck, key: ScreenKey) -> Step {
        let Mode::Lines {
            kind,
            entries,
            typing,
        } = &mut self.mode
        else {
            return Step::Stay;
        };
        let kind = *kind;
        let mut request = None;
        match typing {
            Typing::Idle => match key {
                ScreenKey::Escape => return Step::Leave,
                ScreenKey::Char('p' | 'P') if access.allows(kind.post_permission()) => {
                    *typing = Typing::Line(String::new());
                }
                ScreenKey::Char('d' | 'D')
                    if access.allows(Permission::CommunityModerate) && !entries.is_empty() =>
                {
                    *typing = Typing::Remove(String::new());
                }
                _ => {}
            },
            Typing::Line(text) => match key {
                ScreenKey::Escape => *typing = Typing::Idle,
                ScreenKey::Char(c) if !c.is_control() => {
                    if text.chars().count() < kind.max_chars() {
                        text.push(c);
                    }
                }
                ScreenKey::Char(_) => {}
                ScreenKey::Backspace => {
                    text.pop();
                }
                ScreenKey::Enter => request = Some(Request::Post(kind, text.clone())),
            },
            Typing::Remove(number) => match key {
                ScreenKey::Escape => *typing = Typing::Idle,
                ScreenKey::Char(c) if c.is_ascii_digit() && number.len() < 2 => number.push(c),
                ScreenKey::Char(_) => {}
                ScreenKey::Backspace => {
                    number.pop();
                }
                ScreenKey::Enter => {
                    let picked = number
                        .parse::<usize>()
                        .ok()
                        .and_then(|n| n.checked_sub(1))
                        .and_then(|n| entries.get(n))
                        .map(|entry| entry.id);
                    if let Some(id) = picked {
                        request = Some(Request::Remove(kind, id));
                    } else {
                        *typing = Typing::Idle;
                        self.note = Some(Note {
                            error: true,
                            key: keys::NOTE_NOT_FOUND,
                        });
                    }
                }
            },
        }
        if let Some(request) = request {
            self.ask(request);
        }
        Step::Stay
    }

    fn polls_key(&mut self, access: &dyn PermissionCheck, key: ScreenKey) -> Step {
        let Mode::Polls { rows, total } = &self.mode else {
            return Step::Stay;
        };
        let offset = self.offset;
        match key {
            ScreenKey::Escape => return Step::Leave,
            ScreenKey::Char(c @ '1'..='9') => {
                let picked = usize::from(c as u8 - b'1');
                if let Some(row) = rows.get(picked) {
                    let id = row.id;
                    self.ask(Request::Poll(id));
                }
            }
            ScreenKey::Char('n' | 'N') if offset + POLL_PAGE < *total => {
                self.ask(Request::Polls {
                    offset: offset + POLL_PAGE,
                });
            }
            ScreenKey::Char('p' | 'P') if offset > 0 => {
                self.ask(Request::Polls {
                    offset: offset.saturating_sub(POLL_PAGE),
                });
            }
            ScreenKey::Char('c' | 'C') if access.allows(Permission::PollsCreate) => {
                self.mode = Mode::Form(Box::new(Form::new()));
            }
            _ => {}
        }
        Step::Stay
    }

    fn poll_key(&mut self, access: &dyn PermissionCheck, key: ScreenKey) -> Step {
        let Mode::Poll(detail) = &self.mode else {
            return Step::Stay;
        };
        let id = detail.id;
        let offset = self.offset;
        let request = match key {
            ScreenKey::Escape => Some(Request::Polls { offset }),
            // Offered only while a vote is possible (the hint), but a key still asks: a poll
            // closed or voted on in another session answers in words, not silence.
            ScreenKey::Char(c @ '1'..='6') if access.allows(Permission::PollsVote) => {
                let option = usize::from(c as u8 - b'1');
                (option < detail.options.len()).then_some(Request::Vote(id, option))
            }
            ScreenKey::Char('c' | 'C') if detail.can.close => Some(Request::Close(id)),
            ScreenKey::Char('x' | 'X')
                if detail.can.remove && access.allows(Permission::CommunityModerate) =>
            {
                Some(Request::RemovePoll(id))
            }
            _ => None,
        };
        if let Some(request) = request {
            self.ask(request);
        }
        Step::Stay
    }

    fn form_key(&mut self, key: ScreenKey) -> Step {
        let offset = self.offset;
        let Mode::Form(form) = &mut self.mode else {
            return Step::Stay;
        };
        if key == ScreenKey::Escape {
            self.ask(Request::Polls { offset });
            return Step::Stay;
        }
        let mut note = None;
        match form.step {
            FormStep::Question => match key {
                ScreenKey::Char(c) if !c.is_control() => {
                    if form.question.chars().count() < QUESTION_MAX {
                        form.question.push(c);
                    }
                }
                ScreenKey::Backspace => {
                    form.question.pop();
                }
                ScreenKey::Enter if form.question.trim().is_empty() => {
                    note = Some(keys::NOTE_POLL_QUESTION_BLANK);
                }
                ScreenKey::Enter => form.step = FormStep::Option(0),
                _ => {}
            },
            FormStep::Option(i) => match key {
                ScreenKey::Char(c) if !c.is_control() => {
                    if form.options[i].chars().count() < OPTION_MAX {
                        form.options[i].push(c);
                    }
                }
                ScreenKey::Backspace => {
                    if form.options[i].pop().is_none() {
                        form.step = if i == 0 {
                            FormStep::Question
                        } else {
                            FormStep::Option(i - 1)
                        };
                    }
                }
                ScreenKey::Enter if form.options[i].trim().is_empty() => {
                    if form.filled() >= OPTIONS_MIN {
                        form.step = FormStep::Review;
                    } else {
                        note = Some(keys::NOTE_POLL_TOO_FEW);
                    }
                }
                ScreenKey::Enter => {
                    form.step = if i + 1 >= OPTIONS_MAX {
                        FormStep::Review
                    } else {
                        FormStep::Option(i + 1)
                    };
                }
                _ => {}
            },
            FormStep::Review => match key {
                ScreenKey::Char('d' | 'D') => {
                    self.editor = Some(EditorAsk {
                        initial: form.description.clone(),
                    });
                }
                ScreenKey::Char('r' | 'R') => {
                    let all = ResultsVisibility::ALL;
                    let at = all.iter().position(|v| *v == form.visibility).unwrap_or(0);
                    form.visibility = all[(at + 1) % all.len()];
                }
                ScreenKey::Backspace => {
                    let last = form
                        .options
                        .iter()
                        .rposition(|o| !o.trim().is_empty())
                        .unwrap_or(0);
                    form.step = FormStep::Option(last);
                }
                ScreenKey::Enter => {
                    let request = Request::Create {
                        question: form.question.clone(),
                        options: form.options.clone(),
                        description: form.description.clone(),
                        visibility: form.visibility,
                    };
                    self.ask(request);
                }
                _ => {}
            },
        }
        if let Some(key) = note {
            self.note = Some(Note { error: true, key });
        }
        Step::Stay
    }

    /// Hand the answer of a request to the screen.
    pub fn deliver(&mut self, response: Response) -> Step {
        self.waiting = false;
        match response {
            Response::NotPermitted => return Step::Denied,
            Response::Failed => {
                self.note = Some(Note {
                    error: true,
                    key: keys::NOTE_FAILED,
                });
                // A list that could not be loaded has nothing to show; a form keeps its text.
                if let Mode::Lines { typing, .. } = &mut self.mode {
                    *typing = Typing::Idle;
                }
            }
            Response::Lines {
                kind,
                entries,
                note,
            } => {
                self.mode = Mode::Lines {
                    kind,
                    entries,
                    typing: Typing::Idle,
                };
                self.note = note;
            }
            Response::Callers(rows) => self.mode = Mode::Callers(rows),
            Response::Polls {
                offset,
                rows,
                total,
                note,
            } => {
                if rows.is_empty() && offset > 0 {
                    // The last page emptied (a poll was removed): show the first one.
                    self.offset = 0;
                    self.ask(Request::Polls { offset: 0 });
                    return Step::Stay;
                }
                self.offset = offset;
                self.mode = Mode::Polls { rows, total };
                self.note = note;
            }
            Response::Poll { detail, note } => {
                self.mode = Mode::Poll(detail);
                self.note = note;
            }
            Response::Refused(note) => {
                self.note = Some(note);
                if matches!(self.mode, Mode::Poll(_)) {
                    let offset = self.offset;
                    self.ask(Request::Polls { offset });
                }
            }
        }
        Step::Stay
    }

    /// The screen as a layout draws it.
    #[must_use]
    pub fn view(&self, access: &dyn PermissionCheck) -> ScreenView {
        let mut lines = Vec::new();
        let mut footer = keys::FOOTER_BACK;
        match &self.mode {
            Mode::Loading => {}
            Mode::Lines {
                kind,
                entries,
                typing,
            } => {
                footer = lines_view(&mut lines, access, *kind, entries, typing);
            }
            Mode::Callers(rows) => callers_view(&mut lines, rows),
            Mode::Polls { rows, total } => {
                polls_view(&mut lines, access, rows, *total, self.offset);
            }
            Mode::Poll(detail) => poll_view(&mut lines, access, detail),
            Mode::Form(form) => {
                form_view(&mut lines, form);
                footer = keys::FOOTER_FORM;
            }
        }
        if let Some(note) = self.note {
            lines.push(Line::Notice {
                error: note.error,
                text: note.key,
            });
        }
        ScreenView { lines, footer }
    }
}

fn leave_on_escape(key: ScreenKey) -> Step {
    if key == ScreenKey::Escape {
        Step::Leave
    } else {
        Step::Stay
    }
}

fn number(n: usize) -> String {
    n.to_string()
}

fn lines_view(
    out: &mut Vec<Line>,
    access: &dyn PermissionCheck,
    kind: LineKind,
    entries: &[Entry],
    typing: &Typing,
) -> &'static str {
    let (title, empty) = match kind {
        LineKind::Oneliner => (keys::ONELINERS_TITLE, keys::ONELINERS_EMPTY),
        LineKind::Graffiti => (keys::GRAFFITI_TITLE, keys::GRAFFITI_EMPTY),
    };
    out.push(Line::Heading(title));
    if kind == LineKind::Graffiti {
        out.push(Line::Filled {
            key: keys::GRAFFITI_INTRO,
            values: vec![("keep", number(kind.keep()))],
        });
    }
    if entries.is_empty() {
        out.push(Line::Text(empty));
    }
    for (index, entry) in entries.iter().enumerate() {
        out.push(Line::Filled {
            key: keys::ENTRY,
            values: vec![
                ("n", number(index + 1)),
                ("handle", entry.author.clone()),
                ("text", entry.text.clone()),
            ],
        });
    }
    match typing {
        Typing::Line(text) => {
            out.push(Line::Field {
                label: keys::FIELD_LINE,
                value: text.clone(),
                active: true,
            });
            keys::FOOTER_TYPING
        }
        Typing::Remove(text) => {
            out.push(Line::Field {
                label: keys::FIELD_REMOVE,
                value: text.clone(),
                active: true,
            });
            keys::FOOTER_REMOVE
        }
        Typing::Idle => {
            if access.allows(kind.post_permission()) {
                out.push(Line::Text(keys::HINT_POST));
            }
            if access.allows(Permission::CommunityModerate) && !entries.is_empty() {
                out.push(Line::Text(keys::HINT_REMOVE));
            }
            keys::FOOTER_BACK
        }
    }
}

fn callers_view(out: &mut Vec<Line>, rows: &[CallerRow]) {
    out.push(Line::Heading(keys::CALLERS_TITLE));
    if rows.is_empty() {
        out.push(Line::Text(keys::CALLERS_EMPTY));
    }
    for row in rows {
        out.push(Line::Filled {
            key: keys::CALLER,
            values: vec![("handle", row.handle.clone()), ("when", format_utc(row.at))],
        });
    }
}

fn polls_view(
    out: &mut Vec<Line>,
    access: &dyn PermissionCheck,
    rows: &[PollRow],
    total: usize,
    offset: usize,
) {
    out.push(Line::Heading(keys::POLLS_TITLE));
    if rows.is_empty() {
        out.push(Line::Text(keys::POLLS_EMPTY));
    }
    for (index, row) in rows.iter().enumerate() {
        out.push(Line::Filled {
            key: if row.closed {
                keys::POLL_ROW_CLOSED
            } else {
                keys::POLL_ROW_OPEN
            },
            values: vec![
                ("key", number(index + 1)),
                ("question", row.question.clone()),
                ("votes", row.votes.to_string()),
            ],
        });
    }
    if total > POLL_PAGE {
        out.push(Line::Filled {
            key: keys::POLLS_PAGE,
            values: vec![
                ("page", number(offset / POLL_PAGE + 1)),
                ("pages", number(total.div_ceil(POLL_PAGE))),
            ],
        });
    }
    if !rows.is_empty() {
        out.push(Line::Text(keys::HINT_OPEN));
    }
    if offset + POLL_PAGE < total {
        out.push(Line::Text(keys::HINT_NEXT));
    }
    if offset > 0 {
        out.push(Line::Text(keys::HINT_PREVIOUS));
    }
    if access.allows(Permission::PollsCreate) {
        out.push(Line::Text(keys::HINT_CREATE));
    }
}

fn poll_view(out: &mut Vec<Line>, access: &dyn PermissionCheck, poll: &PollDetail) {
    out.push(Line::Pair {
        label: keys::POLL_TITLE,
        value: Some(poll.question.clone()),
    });
    out.push(Line::Filled {
        key: keys::POLL_BY,
        values: vec![("handle", poll.creator.clone())],
    });
    if poll.closed {
        out.push(Line::Text(keys::POLL_CLOSED));
    }
    for text in poll.description.lines().take(5) {
        out.push(Line::Filled {
            key: keys::POLL_DESCRIPTION_LINE,
            values: vec![("text", text.to_owned())],
        });
    }
    let total: u64 = poll.counts.as_ref().map_or(0, |c| c.iter().sum());
    for (index, text) in poll.options.iter().enumerate() {
        let mine = poll.my_vote == Some(index);
        let mut values = vec![("key", number(index + 1)), ("text", text.clone())];
        let key = if let Some(counts) = &poll.counts {
            let count = counts.get(index).copied().unwrap_or(0);
            let percent = (count * 100).checked_div(total).unwrap_or(0);
            values.push(("count", count.to_string()));
            values.push(("percent", percent.to_string()));
            if mine {
                keys::POLL_OPTION_COUNT_MINE
            } else {
                keys::POLL_OPTION_COUNT
            }
        } else if mine {
            keys::POLL_OPTION_MINE
        } else {
            keys::POLL_OPTION
        };
        out.push(Line::Filled { key, values });
    }
    if poll.counts.is_some() {
        out.push(Line::Filled {
            key: keys::POLL_TOTAL,
            values: vec![("total", total.to_string())],
        });
    } else {
        out.push(Line::Text(match poll.visibility {
            ResultsVisibility::AfterVote => keys::POLL_HIDDEN_VOTE,
            ResultsVisibility::Always | ResultsVisibility::AfterClose => keys::POLL_HIDDEN_CLOSE,
        }));
    }
    if poll.can.vote && access.allows(Permission::PollsVote) {
        out.push(Line::Text(keys::HINT_VOTE));
    }
    if poll.can.close {
        out.push(Line::Text(keys::HINT_CLOSE));
    }
    if poll.can.remove && access.allows(Permission::CommunityModerate) {
        out.push(Line::Text(keys::HINT_REMOVE_POLL));
    }
}

fn form_view(out: &mut Vec<Line>, form: &Form) {
    out.push(Line::Heading(keys::FORM_TITLE));
    out.push(Line::Field {
        label: keys::FORM_QUESTION,
        value: form.question.clone(),
        active: form.step == FormStep::Question,
    });
    let shown = match form.step {
        FormStep::Question => 0,
        FormStep::Option(i) => i + 1,
        FormStep::Review => OPTIONS_MAX,
    };
    for (index, text) in form.options.iter().enumerate().take(shown) {
        if form.step == FormStep::Review && text.trim().is_empty() {
            continue;
        }
        out.push(Line::Field {
            label: OPTION_KEYS[index],
            value: text.clone(),
            active: form.step == FormStep::Option(index),
        });
    }
    if form.step == FormStep::Review {
        out.push(match form.description.lines().next() {
            Some(first) => Line::Pair {
                label: keys::FORM_DESCRIPTION,
                value: Some(first.to_owned()),
            },
            None => Line::Text(keys::FORM_DESCRIPTION_NONE),
        });
        out.push(Line::Text(match form.visibility {
            ResultsVisibility::Always => keys::POLL_VISIBILITY_ALWAYS,
            ResultsVisibility::AfterVote => keys::POLL_VISIBILITY_AFTER_VOTE,
            ResultsVisibility::AfterClose => keys::POLL_VISIBILITY_AFTER_CLOSE,
        }));
        out.push(Line::Text(keys::HINT_REVIEW));
    }
}
