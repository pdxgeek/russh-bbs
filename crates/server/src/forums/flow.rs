//! The forum screens as a state machine: keys in, requests and effects out. No store, no
//! terminal; `draw` turns the state into lines and `exec` runs its requests.

use std::sync::{Mutex, PoisonError};

use ratatui::layout::Rect;
use russh_bbs_core::forums::{
    BoardId, SearchPage, ThreadPage, ThreadView, prepare_description, prepare_name, prepare_subject,
};

use russh_bbs_core::terminal_settings::{
    Pager, PagerEffect, PagerKey, TerminalSettings,
};
use russh_bbs_core::waiting_room::NoticeKind;

use super::exec::{BoardList, Fail, Purpose, Request, Response};
use super::strings;
use crate::ssh::terminal::Key;
use crate::ui::editor::{DraftSave, Editor, EditorEffect, Outcome};
use crate::ui::pager;

/// Longest text a one-line prompt accepts while typing.
const INPUT_MAX: usize = 200;

/// What the session does after a key.
#[derive(Debug)]
pub(crate) enum ForumEffect {
    /// Redraw.
    Stay,
    /// Leave the forums for the main menu.
    Exit,
    /// Run this against the store, then give the answer to [`ForumsScreen::deliver`].
    Request(Request),
    /// The member logged off (Ctrl-C, Ctrl-D). Store the draft first, if the editor had one.
    LogOff(Option<DraftSave>),
}

/// Where Esc takes a reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Back {
    Threads { board: BoardId, page: u32 },
    Results { query: String, page: u32 },
}

#[derive(Debug)]
pub(super) struct Reader {
    pub(super) view: ThreadView,
    pub(super) back: Back,
    pager: Pager,
    width: usize,
    confirm: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum InputKind {
    Search,
    Subject { board: BoardId, board_name: String },
    BoardName,
    BoardDescription { name: String },
    BoardRestricted { name: String, description: String },
    Allow { board: BoardId },
    FeedUrl,
    FeedName { url: String },
    FeedHttp { url: String, name: String },
    FeedRemove { board: BoardId },
}

#[derive(Debug)]
pub(super) struct Input {
    pub(super) kind: InputKind,
    pub(super) text: String,
    prev: Box<State>,
}

/// The editor behind a lock. The text area keeps its layout caches in `Cell`s, so it is not
/// `Sync`, and the session's shell task holds a reference to the screen across `.await`. Only
/// the shell touches the editor, one key batch at a time, so the lock is never contended.
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
    Boards {
        list: BoardList,
        cursor: usize,
    },
    Threads {
        page: ThreadPage,
        cursor: usize,
    },
    Reader(Box<Reader>),
    Results {
        page: SearchPage,
        query: String,
        cursor: usize,
    },
    Input(Box<Input>),
    Editor(Box<EditorState>),
}

/// What the screen is waiting for. Keys are ignored meanwhile (the session answers before it
/// applies the next key).
#[derive(Debug)]
enum Pending {
    Boards,
    Threads,
    Post(Back),
    Search,
    Compose,
    Draft,
    Submit,
    Remove,
    Create,
    Allow,
    Feed,
}

/// Which rows of the current list the last draw showed. Drawing is the only thing that knows
/// how many rows fit, so it records the window here and the keys read it back (FIX-M3-4):
/// a digit picks among the drawn rows, and Page Up / Page Down move by one drawn page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ListView {
    /// Index of the first drawn item.
    pub(super) top: usize,
    /// How many items were drawn (0 before the first draw).
    pub(super) shown: usize,
}

/// A [`ListView`] shared between drawing (`&self`) and keys. A mutex because the screen is held
/// across awaits and so must be `Sync`; the value is `Copy` and never held across a call.
#[derive(Debug, Default)]
pub(super) struct ViewCell(Mutex<ListView>);

impl ViewCell {
    pub(super) fn get(&self) -> ListView {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn set(&self, view: ListView) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = view;
    }
}

/// The forum screens.
#[derive(Debug)]
pub(crate) struct ForumsScreen {
    pub(super) state: State,
    pub(super) notice: Option<(NoticeKind, String)>,
    /// Set by drawing; see [`ListView`].
    pub(super) view: ViewCell,
    pending: Option<Pending>,
    settings: TerminalSettings,
}

impl ForumsScreen {
    /// Which screen of the forums this is, for the motion wipe: a change starts one (UI-4).
    pub(crate) fn screen_id(&self) -> u8 {
        match &self.state {
            State::Start => 0,
            State::Boards { .. } => 1,
            State::Threads { .. } => 2,
            State::Reader(_) => 3,
            State::Results { .. } => 4,
            State::Input(_) => 5,
            State::Editor(_) => 6,
        }
    }

    /// Whether a one-line input is open (the cursor blinks).
    pub(crate) fn has_input(&self) -> bool {
        matches!(self.state, State::Input(_))
    }
}

fn info(text: &str) -> (NoticeKind, String) {
    (NoticeKind::Info, text.to_owned())
}

fn problem(text: &str) -> (NoticeKind, String) {
    (NoticeKind::Error, text.to_owned())
}

/// The row a list key lands on, if it moves or picks one.
enum Pick {
    Move(usize),
    Open(usize),
    None,
}

/// What a list key does. `view` is what the last draw showed: digits count the drawn rows, so
/// `Enter` and a digit can only ever open a row that is on screen.
fn pick(key: Key, cursor: usize, len: usize, view: ListView) -> Pick {
    if len == 0 {
        return Pick::None;
    }
    // Before the first draw nothing is windowed.
    let view = if view.shown == 0 {
        ListView { top: 0, shown: len }
    } else {
        view
    };
    let page = view.shown.max(1);
    let cursor = cursor.min(len - 1);
    match key {
        Key::Up => Pick::Move(cursor.saturating_sub(1)),
        Key::Down => Pick::Move((cursor + 1).min(len - 1)),
        Key::PageUp => Pick::Move(cursor.saturating_sub(page)),
        Key::PageDown => Pick::Move((cursor + page).min(len - 1)),
        Key::Home => Pick::Move(0),
        Key::End => Pick::Move(len - 1),
        Key::Enter => Pick::Open(cursor),
        Key::Char(c @ '1'..='9') => c
            .to_digit(10)
            .and_then(|d| usize::try_from(d - 1).ok())
            .filter(|n| *n < view.shown)
            .map(|n| view.top + n)
            .filter(|at| *at < len)
            .map_or(Pick::None, Pick::Open),
        _ => Pick::None,
    }
}

impl ForumsScreen {
    /// Open the forums; the first request loads the board list.
    pub(crate) fn open(settings: TerminalSettings) -> (Self, Request) {
        (
            Self {
                state: State::Start,
                notice: None,
                view: ViewCell::default(),
                pending: Some(Pending::Boards),
                settings,
            },
            Request::Boards,
        )
    }

    /// The editor, while one is open.
    pub(super) fn editor(&self) -> Option<&SyncEditor> {
        match &self.state {
            State::Editor(state) => Some(&state.editor),
            _ => None,
        }
    }

    /// The reader's pager at `width`, if the reader is showing.
    pub(super) fn reader_pager(&self) -> Option<(&Reader, &Pager, usize)> {
        match &self.state {
            State::Reader(reader) => Some((reader, &reader.pager, reader.width)),
            _ => None,
        }
    }

    /// Rebuild a reader's pager for a new text width (a resize): the page position is lost.
    pub(super) fn pager_for(&self, reader: &Reader, width: usize) -> Pager {
        Pager::new(
            reader_lines(&reader.view, width, self.notice.as_ref()),
            &self.settings,
        )
    }

    fn request(&mut self, pending: Pending, request: Request) -> ForumEffect {
        self.pending = Some(pending);
        ForumEffect::Request(request)
    }

    /// Feed one key. `area` is the drawn rectangle, for the reader's page size.
    pub(crate) fn input(&mut self, key: Key, area: Rect) -> ForumEffect {
        if self.pending.is_some() {
            return if matches!(key, Key::Ctrl('c' | 'd')) {
                ForumEffect::LogOff(None)
            } else {
                ForumEffect::Stay
            };
        }
        if matches!(key, Key::Ctrl('c' | 'd')) && !matches!(self.state, State::Editor(_)) {
            return ForumEffect::LogOff(None);
        }
        self.notice = None;
        match std::mem::replace(&mut self.state, State::Start) {
            State::Start => ForumEffect::Exit,
            State::Boards { list, cursor } => self.boards_key(key, list, cursor),
            State::Threads { page, cursor } => self.threads_key(key, page, cursor),
            State::Reader(reader) => self.reader_key(key, *reader, area),
            State::Results {
                page,
                query,
                cursor,
            } => self.results_key(key, page, query, cursor),
            State::Input(input) => self.input_key(key, *input),
            State::Editor(state) => self.editor_key(key, *state),
        }
    }

    fn boards_key(&mut self, key: Key, list: BoardList, mut cursor: usize) -> ForumEffect {
        match pick(key, cursor, list.boards.len(), self.view.get()) {
            Pick::Move(to) => cursor = to,
            Pick::Open(at) => {
                let board = list.boards[at].id;
                self.state = State::Boards { list, cursor: at };
                return self.request(Pending::Threads, Request::Threads { board, page: 0 });
            }
            Pick::None => match key {
                Key::Escape => {
                    self.state = State::Boards { list, cursor };
                    return ForumEffect::Exit;
                }
                Key::Char('s' | 'S') => {
                    return self.start_input(InputKind::Search, State::Boards { list, cursor });
                }
                Key::Char('n' | 'N') if list.can_manage => {
                    return self.start_input(InputKind::BoardName, State::Boards { list, cursor });
                }
                Key::Char('f' | 'F') if list.can_feeds => {
                    return self.start_input(InputKind::FeedUrl, State::Boards { list, cursor });
                }
                _ => {}
            },
        }
        self.state = State::Boards { list, cursor };
        ForumEffect::Stay
    }

    fn start_input(&mut self, kind: InputKind, prev: State) -> ForumEffect {
        self.state = State::Input(Box::new(Input {
            kind,
            text: String::new(),
            prev: Box::new(prev),
        }));
        ForumEffect::Stay
    }

    fn threads_key(&mut self, key: Key, page: ThreadPage, mut cursor: usize) -> ForumEffect {
        let board = page.board.id;
        match pick(key, cursor, page.threads.len(), self.view.get()) {
            Pick::Move(to) => cursor = to,
            Pick::Open(at) => {
                let thread = page.threads[at].id;
                let back = Back::Threads {
                    board,
                    page: page.page,
                };
                self.state = State::Threads { page, cursor: at };
                return self.request(Pending::Post(back), Request::Post { thread, index: 0 });
            }
            Pick::None => match key {
                Key::Escape => {
                    self.state = State::Threads { page, cursor };
                    return self.request(Pending::Boards, Request::Boards);
                }
                Key::Char('n' | 'N') if page.more => {
                    let next = page.page + 1;
                    self.state = State::Threads { page, cursor };
                    return self.request(Pending::Threads, Request::Threads { board, page: next });
                }
                Key::Char('b' | 'B') if page.page > 0 => {
                    let previous = page.page - 1;
                    self.state = State::Threads { page, cursor };
                    return self.request(
                        Pending::Threads,
                        Request::Threads {
                            board,
                            page: previous,
                        },
                    );
                }
                Key::Char('w' | 'W') if page.can_post => {
                    let kind = InputKind::Subject {
                        board,
                        board_name: page.board.name.clone(),
                    };
                    return self.start_input(kind, State::Threads { page, cursor });
                }
                Key::Char('x' | 'X') if page.can_feeds && page.board.feed => {
                    return self.start_input(
                        InputKind::FeedRemove { board },
                        State::Threads { page, cursor },
                    );
                }
                Key::Char('p' | 'P') if page.can_feeds && !page.board.restricted => {
                    self.state = State::Threads { page, cursor };
                    return self.request(Pending::Feed, Request::TogglePublic(board));
                }
                Key::Char('a' | 'A') if page.can_manage && page.board.restricted => {
                    return self
                        .start_input(InputKind::Allow { board }, State::Threads { page, cursor });
                }
                Key::Char('s' | 'S') => {
                    return self.start_input(InputKind::Search, State::Threads { page, cursor });
                }
                _ => {}
            },
        }
        self.state = State::Threads { page, cursor };
        ForumEffect::Stay
    }

    fn results_key(
        &mut self,
        key: Key,
        page: SearchPage,
        query: String,
        mut cursor: usize,
    ) -> ForumEffect {
        match pick(key, cursor, page.hits.len(), self.view.get()) {
            Pick::Move(to) => cursor = to,
            Pick::Open(at) => {
                let hit = &page.hits[at];
                let (thread, index) = (hit.thread, hit.index);
                let back = Back::Results {
                    query: query.clone(),
                    page: page.page,
                };
                self.state = State::Results {
                    page,
                    query,
                    cursor: at,
                };
                return self.request(Pending::Post(back), Request::Post { thread, index });
            }
            Pick::None => match key {
                Key::Escape => {
                    self.state = State::Results {
                        page,
                        query,
                        cursor,
                    };
                    return self.request(Pending::Boards, Request::Boards);
                }
                Key::Char('n' | 'N') if page.more => {
                    let next = page.page + 1;
                    let request = Request::Search {
                        query: query.clone(),
                        page: next,
                    };
                    self.state = State::Results {
                        page,
                        query,
                        cursor,
                    };
                    return self.request(Pending::Search, request);
                }
                Key::Char('b' | 'B') if page.page > 0 => {
                    let previous = page.page - 1;
                    let request = Request::Search {
                        query: query.clone(),
                        page: previous,
                    };
                    self.state = State::Results {
                        page,
                        query,
                        cursor,
                    };
                    return self.request(Pending::Search, request);
                }
                Key::Char('s' | 'S') => {
                    return self.start_input(
                        InputKind::Search,
                        State::Results {
                            page,
                            query,
                            cursor,
                        },
                    );
                }
                _ => {}
            },
        }
        self.state = State::Results {
            page,
            query,
            cursor,
        };
        ForumEffect::Stay
    }

    fn reader_key(&mut self, key: Key, mut reader: Reader, area: Rect) -> ForumEffect {
        let rows = pager::text_rows(area);
        let width = pager::text_width(area);
        if reader.width != width {
            reader.pager = self.pager_for(&reader, width);
            reader.width = width;
        }
        let thread = reader.view.thread.id;
        let index = reader.view.index;
        if reader.confirm {
            reader.confirm = false;
            let post = reader.view.post.id;
            let confirmed = matches!(key, Key::Char('y' | 'Y'));
            self.state = State::Reader(Box::new(reader));
            return if confirmed {
                self.request(Pending::Remove, Request::Remove(post))
            } else {
                ForumEffect::Stay
            };
        }
        let step = |to: u64| Request::Post { thread, index: to };
        let (effect, follow) = match key {
            Key::Char(' ') | Key::Enter | Key::PageDown => {
                let done = reader.pager.input(PagerKey::Next, rows) == PagerEffect::Done;
                if done && index + 1 < reader.view.total {
                    (None, Some(step(index + 1)))
                } else {
                    (None, None)
                }
            }
            Key::Char('b' | 'B') | Key::PageUp => {
                reader.pager.input(PagerKey::Previous, rows);
                (None, None)
            }
            Key::Char('n' | 'N') if index + 1 < reader.view.total => (None, Some(step(index + 1))),
            Key::Char('p' | 'P') if index > 0 => (None, Some(step(index - 1))),
            Key::Char('r' | 'R') if reader.view.can_reply => {
                (Some(Self::compose(&reader, false)), None)
            }
            Key::Char('c' | 'C') if reader.view.can_reply => {
                (Some(Self::compose(&reader, true)), None)
            }
            Key::Char('x' | 'X') if reader.view.can_moderate => {
                reader.confirm = true;
                (None, None)
            }
            Key::Escape | Key::Char('q' | 'Q') => {
                let request = match &reader.back {
                    Back::Threads { board, page } => (
                        Pending::Threads,
                        Request::Threads {
                            board: *board,
                            page: *page,
                        },
                    ),
                    Back::Results { query, page } => (
                        Pending::Search,
                        Request::Search {
                            query: query.clone(),
                            page: *page,
                        },
                    ),
                };
                self.state = State::Reader(Box::new(reader));
                return self.request(request.0, request.1);
            }
            _ => (None, None),
        };
        let back = reader.back.clone();
        self.state = State::Reader(Box::new(reader));
        if let Some(request) = effect {
            self.pending = Some(Pending::Compose);
            return ForumEffect::Request(request);
        }
        match follow {
            Some(request) => self.request(Pending::Post(back), request),
            None => ForumEffect::Stay,
        }
    }

    fn compose(reader: &Reader, quoted: bool) -> Request {
        Request::Compose {
            purpose: Purpose::Reply {
                thread: reader.view.thread.id,
                subject: reader.view.thread.subject.clone(),
            },
            quote: quoted.then(|| reader.view.post.quoted()),
        }
    }

    fn input_key(&mut self, key: Key, mut input: Input) -> ForumEffect {
        let max = match input.kind {
            InputKind::Search => INPUT_MAX,
            InputKind::Subject { .. } => 120,
            InputKind::BoardName | InputKind::FeedName { .. } => 40,
            InputKind::BoardDescription { .. } => 160,
            InputKind::Allow { .. } => 64,
            InputKind::FeedUrl => crate::forums::feeds_url_max(),
            InputKind::BoardRestricted { .. }
            | InputKind::FeedHttp { .. }
            | InputKind::FeedRemove { .. } => 0,
        };
        let restricted_prompt = matches!(
            input.kind,
            InputKind::BoardRestricted { .. }
                | InputKind::FeedHttp { .. }
                | InputKind::FeedRemove { .. }
        );
        match key {
            Key::Escape => {
                self.state = *input.prev;
                return ForumEffect::Stay;
            }
            Key::Char(c) if !restricted_prompt && !c.is_control() => {
                if input.text.chars().count() < max {
                    input.text.push(c);
                }
            }
            Key::Backspace if !restricted_prompt => {
                input.text.pop();
            }
            Key::Char('y' | 'Y' | 'n' | 'N') if restricted_prompt => {
                let yes = matches!(key, Key::Char('y' | 'Y'));
                match input.kind.clone() {
                    InputKind::BoardRestricted { name, description } => {
                        self.state = State::Input(Box::new(input));
                        return self.request(
                            Pending::Create,
                            Request::CreateBoard {
                                name,
                                description,
                                restricted: yes,
                            },
                        );
                    }
                    InputKind::FeedHttp { url, name } => {
                        self.state = State::Input(Box::new(input));
                        return self.request(
                            Pending::Feed,
                            Request::AddFeed {
                                url,
                                name,
                                allow_http: yes,
                            },
                        );
                    }
                    InputKind::FeedRemove { board } => {
                        if yes {
                            self.state = State::Input(Box::new(input));
                            return self.request(Pending::Feed, Request::RemoveFeed(board));
                        }
                        self.state = *input.prev;
                        return ForumEffect::Stay;
                    }
                    _ => {}
                }
            }
            Key::Enter if !restricted_prompt => return self.input_enter(input),
            _ => {}
        }
        self.state = State::Input(Box::new(input));
        ForumEffect::Stay
    }

    fn input_enter(&mut self, mut input: Input) -> ForumEffect {
        let text = input.text.clone();
        match input.kind.clone() {
            InputKind::Search => {
                self.state = State::Input(Box::new(input));
                self.request(
                    Pending::Search,
                    Request::Search {
                        query: text,
                        page: 0,
                    },
                )
            }
            InputKind::Subject { board, board_name } => match prepare_subject(&text) {
                Ok(subject) => {
                    let purpose = Purpose::Thread {
                        board,
                        board_name,
                        subject: subject.into_string(),
                    };
                    self.state = State::Input(Box::new(input));
                    self.request(
                        Pending::Compose,
                        Request::Compose {
                            purpose,
                            quote: None,
                        },
                    )
                }
                Err(error) => self.refuse(input, &error.to_string()),
            },
            InputKind::BoardName => match prepare_name(&text) {
                Ok(name) => {
                    input.kind = InputKind::BoardDescription {
                        name: name.into_string(),
                    };
                    input.text.clear();
                    self.state = State::Input(Box::new(input));
                    ForumEffect::Stay
                }
                Err(error) => self.refuse(input, &error.to_string()),
            },
            InputKind::BoardDescription { name } => match prepare_description(&text) {
                Ok(description) => {
                    input.kind = InputKind::BoardRestricted {
                        name,
                        description: description.into_string(),
                    };
                    input.text.clear();
                    self.state = State::Input(Box::new(input));
                    ForumEffect::Stay
                }
                Err(error) => self.refuse(input, &error.to_string()),
            },
            InputKind::Allow { board } => {
                self.state = State::Input(Box::new(input));
                self.request(
                    Pending::Allow,
                    Request::Allow {
                        board,
                        handle: text,
                    },
                )
            }
            InputKind::FeedUrl => {
                // Check the address now, so a typo is caught before the second question.
                match russh_bbs_core::feeds::FeedUrl::parse(&text, true) {
                    Ok(url) => {
                        input.kind = InputKind::FeedName {
                            url: url.as_str().to_owned(),
                        };
                        input.text.clear();
                        self.state = State::Input(Box::new(input));
                        ForumEffect::Stay
                    }
                    Err(_) => self.refuse(input, strings::FEED_URL_BAD.trim_end_matches('.')),
                }
            }
            InputKind::FeedName { url } => match prepare_name(&text) {
                Ok(name) => {
                    input.kind = InputKind::FeedHttp {
                        url,
                        name: name.into_string(),
                    };
                    input.text.clear();
                    self.state = State::Input(Box::new(input));
                    ForumEffect::Stay
                }
                Err(error) => self.refuse(input, &error.to_string()),
            },
            InputKind::BoardRestricted { .. }
            | InputKind::FeedHttp { .. }
            | InputKind::FeedRemove { .. } => {
                self.state = State::Input(Box::new(input));
                ForumEffect::Stay
            }
        }
    }

    fn refuse(&mut self, input: Input, why: &str) -> ForumEffect {
        let mut chars = why.chars();
        let text = chars
            .next()
            .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect());
        self.notice = Some((NoticeKind::Error, format!("{text}.")));
        self.state = State::Input(Box::new(input));
        ForumEffect::Stay
    }

    fn editor_key(&mut self, key: Key, state: EditorState) -> ForumEffect {
        let effect = state.editor.with(|editor| editor.input(key));
        match effect {
            EditorEffect::Stay => {
                self.state = State::Editor(Box::new(state));
                ForumEffect::Stay
            }
            EditorEffect::Done(Outcome::Submitted(body)) => {
                let purpose = state.purpose.clone();
                self.state = State::Editor(Box::new(state));
                self.request(Pending::Submit, Request::Submit { purpose, body })
            }
            EditorEffect::Done(Outcome::Cancelled) => {
                self.state = *state.prev;
                ForumEffect::Stay
            }
            EditorEffect::Done(Outcome::SavedDraft) => {
                self.state = *state.prev;
                self.notice = Some(info(strings::OK_DRAFT));
                ForumEffect::Stay
            }
            EditorEffect::SaveDraft(save) => {
                self.state = State::Editor(Box::new(state));
                self.request(Pending::Draft, Request::SaveDraft(save))
            }
            EditorEffect::LogOff(save) => {
                self.state = State::Editor(Box::new(state));
                ForumEffect::LogOff(save)
            }
        }
    }

    /// Take the answer to the request the screen was waiting on. Returns a follow-up request
    /// when the answer calls for one (the list again after a removal).
    pub(crate) fn deliver(&mut self, response: Response) -> Option<Request> {
        let follow = self.deliver_inner(response);
        self.show_notice_in_reader();
        follow
    }

    /// A reader's text carries the note about the last action (posted, refused, removed) as
    /// its first line, so a reader that is shown again after one is rebuilt with it. This
    /// starts the post from its first page, which is where a new note is wanted anyway.
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
            (Pending::Boards, Response::Boards(result)) => self.got_boards(result),
            (Pending::Threads, Response::Threads(result)) => self.got_threads(result),
            (Pending::Post(back), Response::Post(result)) => self.got_post(result, back),
            (Pending::Search, Response::Search(result)) => self.got_search(result),
            (Pending::Compose, Response::Compose(result)) => self.got_editor(result),
            (Pending::Draft, Response::Draft(result)) => {
                self.got_draft(result);
                None
            }
            (Pending::Submit, Response::Posted(result)) => self.got_posted(result),
            (Pending::Remove, Response::Removed(result)) => self.got_removed(result),
            (Pending::Create, Response::Created(result)) => self.got_created(result),
            (Pending::Allow, Response::Allowed(result)) => {
                self.got_allowed(result);
                None
            }
            (Pending::Feed, Response::Feed(result)) => self.got_feed(result),
            (_, _) => {
                // An answer for something else: treat as a failure of what was asked.
                self.notice = Some(problem(strings::FAILED));
                None
            }
        }
    }

    fn fail(&mut self, failure: &Fail) -> Option<Request> {
        self.notice = Some((NoticeKind::Error, failure.text.clone()));
        if failure.gone {
            self.pending = Some(Pending::Boards);
            Some(Request::Boards)
        } else {
            None
        }
    }

    fn got_boards(&mut self, result: Result<BoardList, Fail>) -> Option<Request> {
        match result {
            Ok(list) => {
                self.state = State::Boards { list, cursor: 0 };
                None
            }
            Err(failure) => {
                if matches!(self.state, State::Start) {
                    self.state = State::Boards {
                        list: BoardList {
                            boards: Vec::new(),
                            can_manage: false,
                            can_feeds: false,
                        },
                        cursor: 0,
                    };
                }
                self.notice = Some((NoticeKind::Error, failure.text));
                None
            }
        }
    }

    fn got_threads(&mut self, result: Result<ThreadPage, Fail>) -> Option<Request> {
        match result {
            Ok(page) => {
                self.state = State::Threads { page, cursor: 0 };
                None
            }
            Err(failure) => self.fail(&failure),
        }
    }

    fn got_post(&mut self, result: Result<ThreadView, Fail>, back: Back) -> Option<Request> {
        match result {
            Ok(view) => {
                let notice = self.notice.take();
                let pager = Pager::new(reader_lines(&view, 70, notice.as_ref()), &self.settings);
                self.notice = notice;
                self.state = State::Reader(Box::new(Reader {
                    view,
                    back,
                    pager,
                    width: 70,
                    confirm: false,
                }));
                None
            }
            Err(failure) => self.fail(&failure),
        }
    }

    fn got_search(&mut self, result: Result<SearchPage, Fail>) -> Option<Request> {
        match result {
            Ok(page) => {
                let query = match &self.state {
                    State::Input(input) if input.kind == InputKind::Search => input.text.clone(),
                    State::Results { query, .. } => query.clone(),
                    State::Reader(reader) => match &reader.back {
                        Back::Results { query, .. } => query.clone(),
                        Back::Threads { .. } => String::new(),
                    },
                    _ => String::new(),
                };
                self.state = State::Results {
                    page,
                    query,
                    cursor: 0,
                };
                None
            }
            Err(failure) => self.fail(&failure),
        }
    }

    fn got_editor(&mut self, result: Result<Box<Editor>, Fail>) -> Option<Request> {
        match result {
            Ok(editor) => {
                // From the reader (a reply) the reader is the way back; from the subject prompt
                // it is the screen under the prompt.
                let (prev, purpose) = match std::mem::replace(&mut self.state, State::Start) {
                    State::Input(input) => {
                        let purpose = match &input.kind {
                            InputKind::Subject { board, board_name } => Purpose::Thread {
                                board: *board,
                                board_name: board_name.clone(),
                                subject: prepare_subject(&input.text)
                                    .map(russh_bbs_core::sanitize::Sanitized::into_string)
                                    .unwrap_or_default(),
                            },
                            _ => return None,
                        };
                        (*input.prev, purpose)
                    }
                    State::Reader(reader) => {
                        let purpose = Purpose::Reply {
                            thread: reader.view.thread.id,
                            subject: reader.view.thread.subject.clone(),
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
            Err(failure) => self.fail(&failure),
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

    fn got_posted(
        &mut self,
        result: Result<russh_bbs_core::forums::Posted, Fail>,
    ) -> Option<Request> {
        let State::Editor(state) = std::mem::replace(&mut self.state, State::Start) else {
            return None;
        };
        let state = *state;
        match result {
            Ok(posted) => {
                let back = match (&state.purpose, *state.prev) {
                    (Purpose::Thread { board, .. }, _) => Back::Threads {
                        board: *board,
                        page: 0,
                    },
                    (Purpose::Reply { .. }, State::Reader(reader)) => reader.back,
                    (Purpose::Reply { .. }, _) => return Some(self.restart_boards()),
                };
                self.notice = Some(info(strings::OK_POSTED));
                self.pending = Some(Pending::Post(back));
                Some(Request::Post {
                    thread: posted.thread,
                    index: u64::MAX,
                })
            }
            Err(failure) => {
                // Back to where the member was, with the reason; the text is a draft.
                self.state = *state.prev;
                let mut text = failure.text.clone();
                if failure.kept_draft {
                    text.push(' ');
                    text.push_str(strings::KEPT_DRAFT);
                }
                self.notice = Some((NoticeKind::Error, text));
                if failure.gone {
                    return Some(self.restart_boards());
                }
                None
            }
        }
    }

    fn restart_boards(&mut self) -> Request {
        self.pending = Some(Pending::Boards);
        Request::Boards
    }

    fn got_removed(
        &mut self,
        result: Result<russh_bbs_core::forums::Gone, Fail>,
    ) -> Option<Request> {
        match result {
            Ok(gone) => {
                let index = match &self.state {
                    State::Reader(reader) => reader.view.index,
                    _ => 0,
                };
                if gone.thread_removed {
                    self.notice = Some(info(strings::OK_REMOVED_THREAD));
                    self.pending = Some(Pending::Threads);
                    Some(Request::Threads {
                        board: gone.board,
                        page: 0,
                    })
                } else {
                    let back = match &self.state {
                        State::Reader(reader) => reader.back.clone(),
                        _ => Back::Threads {
                            board: gone.board,
                            page: 0,
                        },
                    };
                    self.notice = Some(info(strings::OK_REMOVED));
                    self.pending = Some(Pending::Post(back));
                    Some(Request::Post {
                        thread: gone.thread,
                        index: index.saturating_sub(1),
                    })
                }
            }
            Err(failure) => self.fail(&failure),
        }
    }

    fn got_created(&mut self, result: Result<BoardId, Fail>) -> Option<Request> {
        match result {
            Ok(_) => {
                if let State::Input(input) = std::mem::replace(&mut self.state, State::Start) {
                    self.state = *input.prev;
                }
                self.notice = Some(info(strings::OK_BOARD));
                Some(self.restart_boards())
            }
            Err(failure) => {
                // Name taken or similar: back to the name prompt so it can be changed.
                if let State::Input(input) = &mut self.state {
                    input.kind = InputKind::BoardName;
                    input.text.clear();
                }
                self.fail(&failure)
            }
        }
    }

    fn got_feed(&mut self, result: Result<super::exec::FeedDone, Fail>) -> Option<Request> {
        match result {
            Ok(done) => {
                // Out of a prompt: back to where it started. A toggle made straight from a
                // list has no prompt to leave, and the list stays as it is.
                self.state = match std::mem::replace(&mut self.state, State::Start) {
                    State::Input(input) => *input.prev,
                    other => other,
                };
                self.notice = Some(info(done.text));
                done.reload.then(|| self.restart_boards())
            }
            Err(failure) => self.fail(&failure),
        }
    }

    fn got_allowed(&mut self, result: Result<(), Fail>) {
        match result {
            Ok(()) => {
                if let State::Input(input) = std::mem::replace(&mut self.state, State::Start) {
                    self.state = *input.prev;
                }
                self.notice = Some(info(strings::OK_ALLOWED));
            }
            Err(failure) => {
                self.fail(&failure);
            }
        }
    }
}

/// The reader's text: the post wrapped to `width`, quote marks kept on every wrapped row. The
/// header block, the post's position and the keys are drawn by the reader around the text
/// (`ui::screens::reader`); the notice (a result of the last action) is the first line.
fn reader_lines(
    view: &ThreadView,
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
    let body: Vec<String> = view
        .post
        .body
        .as_str()
        .split('\n')
        .map(str::to_owned)
        .collect();
    out.extend(pager::wrap_quoted(&body, width));
    out
}

/// The title of the box around a reader.
pub(super) fn reader_title(reader: &Reader) -> String {
    reader.view.thread.subject.clone()
}

/// The confirmation text under a reader that is asking before it removes a post.
pub(super) fn confirm_text(reader: &Reader) -> Option<&'static str> {
    reader.confirm.then_some(if reader.view.index == 0 {
        strings::CONFIRM_REMOVE_FIRST
    } else {
        strings::CONFIRM_REMOVE
    })
}
