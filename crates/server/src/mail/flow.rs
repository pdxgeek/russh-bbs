//! The mail screens as a state machine: keys in, requests and effects out. No store, no key, no
//! terminal; `draw` turns the state into lines and `exec` runs its requests.

use std::sync::{Mutex, PoisonError};

use ratatui::layout::Rect;
use russh_bbs_core::drafts::Body;
use russh_bbs_core::mail::{InboxPage, MessageId, MessageView, body_limits, prepare_subject};
use russh_bbs_core::terminal_settings::{
    Pager, PagerEffect, PagerKey, TerminalSettings,
};
use russh_bbs_core::waiting_room::NoticeKind;

use super::exec::{Fail, Purpose, Request, Response};
use super::strings;
use crate::ssh::terminal::Key;
use crate::ui::editor::{DraftSave, Editor, EditorEffect, Outcome};
use crate::ui::pager;

/// What the session does after a key.
#[derive(Debug)]
pub(crate) enum MailEffect {
    /// Redraw.
    Stay,
    /// Leave mail for the main menu.
    Exit,
    /// Run this against the store, then give the answer to [`MailScreen::deliver`].
    Request(Request),
    /// The member logged off (Ctrl-C, Ctrl-D). Store the draft first, if the editor had one.
    LogOff(Option<DraftSave>),
}

/// What a reader is asking before it acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Confirm {
    None,
    Delete,
    Report,
}

#[derive(Debug)]
pub(super) struct Reader {
    pub(super) view: MessageView,
    /// The inbox page to go back to.
    page: u32,
    pager: Pager,
    width: usize,
    pub(super) confirm: Confirm,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InputKind {
    To,
    Subject { to: String },
}

#[derive(Debug)]
pub(super) struct Input {
    pub(super) kind: InputKind,
    pub(super) text: String,
    prev: Box<State>,
}

/// The editor behind a lock: the text area is not `Sync` and the shell task holds the screen
/// across `.await`. Only the shell touches it, one key batch at a time.
#[derive(Debug)]
pub(super) struct SyncEditor(Mutex<Editor>);

impl SyncEditor {
    fn new(editor: Editor) -> Self {
        Self(Mutex::new(editor))
    }

    pub(super) fn with<R>(&self, f: impl FnOnce(&mut Editor) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

#[derive(Debug)]
pub(super) struct EditorState {
    pub(super) editor: SyncEditor,
    purpose: Purpose,
    prev: Box<State>,
}

#[derive(Debug)]
pub(super) enum State {
    /// Nothing loaded yet.
    Start,
    Inbox {
        page: InboxPage,
        cursor: usize,
        /// The message a delete from the list is waiting to confirm.
        confirm: Option<MessageId>,
    },
    Reader(Box<Reader>),
    Input(Box<Input>),
    Editor(Box<EditorState>),
}

/// What the screen is waiting for. Keys are ignored meanwhile.
impl MailScreen {
    /// Which screen of mail this is, for the motion wipe: a change starts one (UI-4).
    pub(crate) fn screen_id(&self) -> u8 {
        match &self.state {
            State::Start => 0,
            State::Inbox { .. } => 1,
            State::Reader(_) => 2,
            State::Input(_) => 3,
            State::Editor(_) => 4,
        }
    }

    /// Whether a one-line input is open (the cursor blinks).
    pub(crate) fn has_input(&self) -> bool {
        matches!(self.state, State::Input(_))
    }
}

#[derive(Debug)]
enum Pending {
    Inbox,
    Open { page: u32 },
    Compose,
    Draft,
    Submit,
    Delete,
    Report,
}

/// The mail screens.
#[derive(Debug)]
pub(crate) struct MailScreen {
    pub(super) state: State,
    pub(super) notice: Option<(NoticeKind, String)>,
    pending: Option<Pending>,
    settings: TerminalSettings,
    /// A message the member wrote that was not sent (a limit, a full mailbox). Kept **in memory
    /// only** until they leave mail: a mail draft is never stored (D90), because the `draft`
    /// table is readable. It comes back as the starting text of the same message.
    unsent: Option<(Purpose, String)>,
    /// What the editor last submitted, until the answer says whether it was sent.
    submitted: Option<(Purpose, String)>,
}

fn info(text: &str) -> (NoticeKind, String) {
    (NoticeKind::Info, text.to_owned())
}

fn digit(key: Key) -> Option<usize> {
    match key {
        Key::Char(c @ '1'..='9') => c.to_digit(10).map(|d| d as usize - 1),
        _ => None,
    }
}

/// The row a list key lands on, if it moves or picks one.
enum Pick {
    Move(usize),
    Open(usize),
    None,
}

fn pick(key: Key, cursor: usize, len: usize) -> Pick {
    if len == 0 {
        return Pick::None;
    }
    match key {
        Key::Up => Pick::Move(cursor.saturating_sub(1)),
        Key::Down => Pick::Move((cursor + 1).min(len - 1)),
        Key::Enter => Pick::Open(cursor.min(len - 1)),
        key => digit(key)
            .filter(|n| *n < len)
            .map_or(Pick::None, Pick::Open),
    }
}

impl MailScreen {
    /// Open mail; the first request loads the first page of the inbox.
    pub(crate) fn open(settings: TerminalSettings) -> (Self, Request) {
        (
            Self {
                state: State::Start,
                notice: None,
                pending: Some(Pending::Inbox),
                settings,
                unsent: None,
                submitted: None,
            },
            Request::Inbox { page: 0 },
        )
    }

    /// The editor, while one is open.
    pub(super) fn editor(&self) -> Option<&SyncEditor> {
        match &self.state {
            State::Editor(state) => Some(&state.editor),
            _ => None,
        }
    }

    /// The reader's pager at its width, if the reader is showing.
    pub(super) fn reader_pager(&self) -> Option<(&Reader, &Pager, usize)> {
        match &self.state {
            State::Reader(reader) => Some((reader, &reader.pager, reader.width)),
            _ => None,
        }
    }

    /// Rebuild a reader's pager for a new text width (a resize).
    pub(super) fn pager_for(&self, reader: &Reader, width: usize) -> Pager {
        Pager::new(
            reader_lines(&reader.view, width, self.notice.as_ref()),
            &self.settings,
        )
    }

    fn request(&mut self, pending: Pending, request: Request) -> MailEffect {
        self.pending = Some(pending);
        MailEffect::Request(request)
    }

    /// Feed one key. `area` is the drawn rectangle, for the reader's page size.
    pub(crate) fn input(&mut self, key: Key, area: Rect) -> MailEffect {
        if self.pending.is_some() {
            return if matches!(key, Key::Ctrl('c' | 'd')) {
                MailEffect::LogOff(None)
            } else {
                MailEffect::Stay
            };
        }
        if matches!(key, Key::Ctrl('c' | 'd')) && !matches!(self.state, State::Editor(_)) {
            return MailEffect::LogOff(None);
        }
        self.notice = None;
        match std::mem::replace(&mut self.state, State::Start) {
            State::Start => MailEffect::Exit,
            State::Inbox {
                page,
                cursor,
                confirm,
            } => self.inbox_key(key, page, cursor, confirm),
            State::Reader(reader) => self.reader_key(key, *reader, area),
            State::Input(input) => self.input_key(key, *input),
            State::Editor(state) => self.editor_key(key, *state),
        }
    }

    fn inbox_key(
        &mut self,
        key: Key,
        page: InboxPage,
        mut cursor: usize,
        confirm: Option<MessageId>,
    ) -> MailEffect {
        if let Some(id) = confirm {
            self.state = State::Inbox {
                page,
                cursor,
                confirm: None,
            };
            return if matches!(key, Key::Char('y' | 'Y')) {
                self.request(Pending::Delete, Request::Delete(id))
            } else {
                MailEffect::Stay
            };
        }
        let here = page.page;
        match pick(key, cursor, page.entries.len()) {
            Pick::Move(to) => cursor = to,
            Pick::Open(at) => {
                let id = page.entries[at].id;
                self.state = State::Inbox {
                    page,
                    cursor: at,
                    confirm: None,
                };
                return self.request(Pending::Open { page: here }, Request::Open(id));
            }
            Pick::None => match key {
                Key::Escape => {
                    self.state = State::Inbox {
                        page,
                        cursor,
                        confirm: None,
                    };
                    return MailEffect::Exit;
                }
                Key::Char('c' | 'C') if page.can_send => {
                    return self.start_input(
                        InputKind::To,
                        State::Inbox {
                            page,
                            cursor,
                            confirm: None,
                        },
                    );
                }
                Key::Char('d' | 'D') if !page.entries.is_empty() => {
                    let id = page.entries[cursor.min(page.entries.len() - 1)].id;
                    self.state = State::Inbox {
                        page,
                        cursor,
                        confirm: Some(id),
                    };
                    return MailEffect::Stay;
                }
                Key::Char('n' | 'N') if page.more => {
                    self.state = State::Inbox {
                        page,
                        cursor,
                        confirm: None,
                    };
                    return self.request(Pending::Inbox, Request::Inbox { page: here + 1 });
                }
                Key::Char('b' | 'B') if here > 0 => {
                    self.state = State::Inbox {
                        page,
                        cursor,
                        confirm: None,
                    };
                    return self.request(Pending::Inbox, Request::Inbox { page: here - 1 });
                }
                _ => {}
            },
        }
        self.state = State::Inbox {
            page,
            cursor,
            confirm: None,
        };
        MailEffect::Stay
    }

    fn start_input(&mut self, kind: InputKind, prev: State) -> MailEffect {
        self.state = State::Input(Box::new(Input {
            kind,
            text: String::new(),
            prev: Box::new(prev),
        }));
        MailEffect::Stay
    }

    fn reader_key(&mut self, key: Key, mut reader: Reader, area: Rect) -> MailEffect {
        let rows = pager::text_rows(area);
        let width = pager::text_width(area);
        if reader.width != width {
            reader.pager = self.pager_for(&reader, width);
            reader.width = width;
        }
        let id = reader.view.id;
        if reader.confirm != Confirm::None {
            let confirm = std::mem::replace(&mut reader.confirm, Confirm::None);
            let confirmed = matches!(key, Key::Char('y' | 'Y'));
            if !confirmed {
                self.state = State::Reader(Box::new(reader));
                return MailEffect::Stay;
            }
            return match confirm {
                Confirm::Delete => {
                    self.state = State::Reader(Box::new(reader));
                    self.request(Pending::Delete, Request::Delete(id))
                }
                Confirm::Report => {
                    let copy = Body::from_sanitized(body_limits(), &reader.view.body);
                    let subject = reader.view.subject.clone();
                    self.state = State::Reader(Box::new(reader));
                    if let Ok(body) = copy {
                        self.request(Pending::Report, Request::Report { id, subject, body })
                    } else {
                        self.notice = Some((NoticeKind::Error, strings::FAILED.to_owned()));
                        MailEffect::Stay
                    }
                }
                Confirm::None => MailEffect::Stay,
            };
        }
        match key {
            Key::Char(' ') | Key::Enter => {
                let _ = reader.pager.input(PagerKey::Next, rows) == PagerEffect::Done;
            }
            Key::Char('b' | 'B') => {
                reader.pager.input(PagerKey::Previous, rows);
            }
            Key::Char('r' | 'R') if reader.view.can_reply => {
                let request = self.compose(&reader, false);
                self.state = State::Reader(Box::new(reader));
                return self.request(Pending::Compose, request);
            }
            Key::Char('c' | 'C') if reader.view.can_reply => {
                let request = self.compose(&reader, true);
                self.state = State::Reader(Box::new(reader));
                return self.request(Pending::Compose, request);
            }
            Key::Char('d' | 'D') => reader.confirm = Confirm::Delete,
            Key::Char('x' | 'X') if reader.view.can_report => reader.confirm = Confirm::Report,
            Key::Escape | Key::Char('q' | 'Q') => {
                let page = reader.page;
                self.state = State::Reader(Box::new(reader));
                return self.request(Pending::Inbox, Request::Inbox { page });
            }
            _ => {}
        }
        self.state = State::Reader(Box::new(reader));
        MailEffect::Stay
    }

    /// The text an unsent message of this purpose left behind, once.
    fn take_unsent(&mut self, purpose: &Purpose) -> Option<russh_bbs_core::sanitize::Sanitized> {
        let (kept, text) = self.unsent.take()?;
        if &kept == purpose {
            Some(russh_bbs_core::sanitize::MultiLine::new(8_000).sanitize(&text))
        } else {
            self.unsent = Some((kept, text));
            None
        }
    }

    fn compose(&mut self, reader: &Reader, quoted: bool) -> Request {
        let purpose = Purpose::Reply {
            id: reader.view.id,
            subject: reader.view.reply_subject(),
        };
        // The member's own unsent text wins over a fresh quote.
        let quote = self
            .take_unsent(&purpose)
            .or_else(|| quoted.then(|| reader.view.quoted()));
        Request::Compose { purpose, quote }
    }

    fn input_key(&mut self, key: Key, mut input: Input) -> MailEffect {
        let max = match input.kind {
            InputKind::To => 64,
            InputKind::Subject { .. } => 120,
        };
        match key {
            Key::Escape => {
                self.state = *input.prev;
                return MailEffect::Stay;
            }
            Key::Char(c) if !c.is_control() => {
                if input.text.chars().count() < max {
                    input.text.push(c);
                }
            }
            Key::Backspace => {
                input.text.pop();
            }
            Key::Enter => return self.input_enter(input),
            _ => {}
        }
        self.state = State::Input(Box::new(input));
        MailEffect::Stay
    }

    fn input_enter(&mut self, mut input: Input) -> MailEffect {
        let text = input.text.clone();
        match input.kind.clone() {
            InputKind::To => {
                let to = text.trim().to_owned();
                if to.is_empty() {
                    return self.refuse(input, "that cannot be empty");
                }
                input.kind = InputKind::Subject { to };
                input.text.clear();
                self.state = State::Input(Box::new(input));
                MailEffect::Stay
            }
            InputKind::Subject { to } => match prepare_subject(&text) {
                Ok(subject) => {
                    let purpose = Purpose::New {
                        to,
                        subject: subject.into_string(),
                    };
                    self.state = State::Input(Box::new(input));
                    let quote = self.take_unsent(&purpose);
                    self.request(Pending::Compose, Request::Compose { purpose, quote })
                }
                Err(error) => self.refuse(input, &error.to_string()),
            },
        }
    }

    fn refuse(&mut self, input: Input, why: &str) -> MailEffect {
        let mut chars = why.chars();
        let text = chars
            .next()
            .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect());
        self.notice = Some((NoticeKind::Error, format!("{text}.")));
        self.state = State::Input(Box::new(input));
        MailEffect::Stay
    }

    fn editor_key(&mut self, key: Key, state: EditorState) -> MailEffect {
        let effect = state.editor.with(|editor| editor.input(key));
        match effect {
            EditorEffect::Stay => {
                self.state = State::Editor(Box::new(state));
                MailEffect::Stay
            }
            EditorEffect::Done(Outcome::Submitted(body)) => {
                let purpose = state.purpose.clone();
                self.submitted = Some((purpose.clone(), body.as_str().to_owned()));
                self.state = State::Editor(Box::new(state));
                self.request(Pending::Submit, Request::Submit { purpose, body })
            }
            EditorEffect::Done(Outcome::Cancelled) => {
                self.state = *state.prev;
                MailEffect::Stay
            }
            EditorEffect::Done(Outcome::SavedDraft) => {
                self.state = *state.prev;
                self.notice = Some(info(strings::OK_DRAFT));
                MailEffect::Stay
            }
            EditorEffect::SaveDraft(save) => {
                self.state = State::Editor(Box::new(state));
                self.request(Pending::Draft, Request::SaveDraft(save))
            }
            EditorEffect::LogOff(save) => {
                self.state = State::Editor(Box::new(state));
                MailEffect::LogOff(save)
            }
        }
    }

    /// Take the answer to the request the screen was waiting on. Returns a follow-up request
    /// when the answer calls for one (the inbox again after a send or a delete).
    pub(crate) fn deliver(&mut self, response: Response) -> Option<Request> {
        let follow = self.deliver_inner(response);
        self.show_notice_in_reader();
        follow
    }

    /// A reader's text carries the note about the last action as its first line.
    fn show_notice_in_reader(&mut self) {
        let notice = self.notice.as_ref();
        if let (State::Reader(reader), Some(_)) = (&mut self.state, notice) {
            reader.pager = Pager::new(
                reader_lines(&reader.view, reader.width, notice),
                &self.settings,
            );
        }
    }

    fn deliver_inner(&mut self, response: Response) -> Option<Request> {
        let pending = self.pending.take()?;
        match (pending, response) {
            (Pending::Inbox, Response::Inbox(result)) => self.got_inbox(result),
            (Pending::Open { page }, Response::Message(result)) => self.got_message(result, page),
            (Pending::Compose, Response::Compose(result)) => self.got_editor(result),
            (Pending::Draft, Response::Draft(result)) => {
                self.got_draft(result);
                None
            }
            (Pending::Submit, Response::Sent(result)) => self.got_sent(result),
            (Pending::Delete, Response::Deleted(result)) => self.got_deleted(result),
            (Pending::Report, Response::Reported(result)) => {
                self.got_reported(result);
                None
            }
            (_, _) => {
                self.notice = Some((NoticeKind::Error, strings::FAILED.to_owned()));
                None
            }
        }
    }

    fn restart_inbox(&mut self, page: u32) -> Request {
        self.pending = Some(Pending::Inbox);
        Request::Inbox { page }
    }

    fn fail(&mut self, failure: &Fail, page: u32) -> Option<Request> {
        self.notice = Some((NoticeKind::Error, failure.text.clone()));
        if failure.gone {
            Some(self.restart_inbox(page))
        } else {
            None
        }
    }

    fn current_page(&self) -> u32 {
        match &self.state {
            State::Inbox { page, .. } => page.page,
            State::Reader(reader) => reader.page,
            _ => 0,
        }
    }

    fn got_inbox(&mut self, result: Result<InboxPage, Fail>) -> Option<Request> {
        match result {
            Ok(page) => {
                // A page past the end (after deletes) goes back one.
                if page.entries.is_empty() && page.page > 0 {
                    return Some(self.restart_inbox(page.page - 1));
                }
                self.state = State::Inbox {
                    page,
                    cursor: 0,
                    confirm: None,
                };
                None
            }
            Err(failure) => {
                if matches!(self.state, State::Start) {
                    self.state = State::Inbox {
                        page: InboxPage {
                            entries: Vec::new(),
                            page: 0,
                            more: false,
                            total: 0,
                            unread: 0,
                            can_send: false,
                        },
                        cursor: 0,
                        confirm: None,
                    };
                }
                self.notice = Some((NoticeKind::Error, failure.text));
                None
            }
        }
    }

    fn got_message(&mut self, result: Result<MessageView, Fail>, page: u32) -> Option<Request> {
        match result {
            Ok(view) => {
                let notice = self.notice.take();
                let pager = Pager::new(reader_lines(&view, 70, notice.as_ref()), &self.settings);
                self.notice = notice;
                self.state = State::Reader(Box::new(Reader {
                    view,
                    page,
                    pager,
                    width: 70,
                    confirm: Confirm::None,
                }));
                None
            }
            // An unreadable message stays in the list, where `d` deletes it.
            Err(failure) => self.fail(&failure, page),
        }
    }

    fn got_editor(&mut self, result: Result<Box<Editor>, Fail>) -> Option<Request> {
        match result {
            Ok(editor) => {
                let (prev, purpose) = match std::mem::replace(&mut self.state, State::Start) {
                    State::Input(input) => {
                        let purpose = match &input.kind {
                            InputKind::Subject { to } => Purpose::New {
                                to: to.clone(),
                                subject: prepare_subject(&input.text)
                                    .map(russh_bbs_core::sanitize::Sanitized::into_string)
                                    .unwrap_or_default(),
                            },
                            InputKind::To => return None,
                        };
                        (*input.prev, purpose)
                    }
                    State::Reader(reader) => {
                        let purpose = Purpose::Reply {
                            id: reader.view.id,
                            subject: reader.view.reply_subject(),
                        };
                        (State::Reader(reader), purpose)
                    }
                    other => {
                        self.state = other;
                        return None;
                    }
                };
                self.state = State::Editor(Box::new(EditorState {
                    editor: SyncEditor::new(*editor),
                    purpose,
                    prev: Box::new(prev),
                }));
                None
            }
            Err(failure) => {
                // A wrong handle goes back to the "to" prompt so it can be changed.
                if let State::Input(input) = &mut self.state
                    && matches!(input.kind, InputKind::Subject { .. })
                    && failure.text == strings::UNKNOWN_MEMBER
                {
                    input.kind = InputKind::To;
                    input.text.clear();
                }
                let page = self.current_page();
                self.fail(&failure, page)
            }
        }
    }

    fn got_draft(
        &mut self,
        result: Result<russh_bbs_core::drafts::SaveOutcome, russh_bbs_core::drafts::DraftError>,
    ) {
        if let State::Editor(state) = &mut self.state
            && state.editor.with(|editor| editor.draft_saved(result))
                == EditorEffect::Done(Outcome::SavedDraft)
            && let State::Editor(state) = std::mem::replace(&mut self.state, State::Start)
        {
            self.state = *state.prev;
            self.notice = Some(info(strings::OK_DRAFT));
        }
    }

    fn got_sent(&mut self, result: Result<russh_bbs_core::mail::Sent, Fail>) -> Option<Request> {
        let State::Editor(state) = std::mem::replace(&mut self.state, State::Start) else {
            return None;
        };
        let state = *state;
        match result {
            Ok(_) => {
                self.submitted = None;
                self.notice = Some(info(strings::OK_SENT));
                Some(self.restart_inbox(0))
            }
            Err(failure) => {
                // Back to where the member was, with the reason; the text is a draft.
                self.state = *state.prev;
                let mut text = failure.text.clone();
                if let Some(unsent) = self.submitted.take() {
                    self.unsent = Some(unsent);
                    text.push(' ');
                    text.push_str(strings::KEPT_UNSENT);
                }
                self.notice = Some((NoticeKind::Error, text));
                if failure.gone {
                    return Some(self.restart_inbox(0));
                }
                None
            }
        }
    }

    fn got_deleted(&mut self, result: Result<(), Fail>) -> Option<Request> {
        let page = self.current_page();
        match result {
            Ok(()) => {
                self.notice = Some(info(strings::OK_DELETED));
                Some(self.restart_inbox(page))
            }
            Err(failure) => self.fail(&failure, page),
        }
    }

    fn got_reported(&mut self, result: Result<russh_bbs_core::mail::ReportId, Fail>) {
        self.notice = Some(match result {
            Ok(_) => info(strings::OK_REPORTED),
            Err(failure) => (NoticeKind::Error, failure.text),
        });
    }
}

/// The reader's text: the message wrapped to `width`, quote marks kept on every wrapped row.
/// The header block and the keys are drawn by the reader (`ui::screens::reader`); the notice (a
/// result of the last action) is the first line.
fn reader_lines(
    view: &MessageView,
    width: usize,
    notice: Option<&(NoticeKind, String)>,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some((kind, text)) = notice {
        let label = match kind {
            NoticeKind::Info => "Note",
            NoticeKind::Error => "Problem",
        };
        out.extend(pager::wrap(&[format!("{label}: {text}")], width));
        out.push(String::new());
    }
    let body: Vec<String> = view.body.as_str().split('\n').map(str::to_owned).collect();
    out.extend(pager::wrap_quoted(&body, width));
    out
}

/// The title of the box around a reader: the subject.
pub(super) fn reader_title(reader: &Reader) -> String {
    reader.view.subject.clone()
}

/// The confirmation text under a reader that is asking before it acts.
pub(super) fn confirm_text(reader: &Reader) -> Option<&'static str> {
    match reader.confirm {
        Confirm::None => None,
        Confirm::Delete => Some(strings::CONFIRM_DELETE),
        Confirm::Report => Some(strings::CONFIRM_REPORT),
    }
}
