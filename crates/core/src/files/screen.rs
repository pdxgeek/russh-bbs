//! The Files screens as a pure state machine (M4-F2): no store, no drawing, no clock. It emits
//! string-table keys and already-sanitized content as a [`ScreenView`] (the members screens'
//! view type, so the server draws it with the same code), and asks for work as [`Request`]s that
//! [`execute`] answers against the store, the blob store and the session's access.
//!
//! Members browse areas and files, see a file's details and the SFTP path to fetch it by, tag
//! and search, and keep a download list; staff with `files.approve` get an approval queue and
//! approve, reject or remove. Downloads and uploads themselves are SFTP's (ssh only): over telnet
//! these screens browse and say so.
//!
//! Keys that arrive while a request is outstanding are ignored except Esc, so a key never acts
//! on a list that is about to be replaced.

use crate::access::{AccessStore, Permission, PermissionCheck, Snapshot};
use crate::blobs::BlobStore;
use crate::profiles::{Line, ScreenKey, ScreenView, Step};
use crate::store::UnixSeconds;
use crate::terminal_settings::format_utc;

use super::ids::{AreaId, FileId};
use super::keys;
use super::service::{self, AreaSummary, FileError, FilePage, FileSummary, FileView};
use super::store::{FileState, FileStore, ScanState};
use super::text::TAG_MAX_CHARS;

/// How this session can fetch files, which decides what the screens say about downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilesMode {
    /// The board has no file store wired (a build without one): nothing works.
    #[default]
    Unavailable,
    /// ssh with SFTP on: uploads and downloads happen over `sftp`.
    Sftp,
    /// ssh, but the sysop turned SFTP off: browse only.
    NoSftp,
    /// telnet: browse only; downloads need ssh.
    Telnet,
}

/// What a note on the screen says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    /// Whether it is a problem.
    pub error: bool,
    /// The string-table key.
    pub key: &'static str,
}

const fn done(key: &'static str) -> Note {
    Note { error: false, key }
}

const fn problem(key: &'static str) -> Note {
    Note { error: true, key }
}

/// Work the screen asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// The areas the member may list.
    Areas,
    /// A page of an area's approved files.
    Area {
        /// The area.
        area: AreaId,
        /// The page, from zero.
        page: u32,
    },
    /// One file in full.
    File(FileId),
    /// A page of the approval queue.
    Queue {
        /// The page, from zero.
        page: u32,
    },
    /// The download list.
    List,
    /// A search.
    Search {
        /// What was typed.
        query: String,
        /// The page, from zero.
        page: u32,
    },
    /// Approve a quarantined file.
    Approve(FileId),
    /// Reject a quarantined file.
    Reject(FileId),
    /// Remove a file.
    Remove(FileId),
    /// Add a tag.
    Tag(FileId, String),
    /// Remove a tag.
    Untag(FileId, String),
    /// Replace the description.
    Describe(FileId, String),
    /// Put the file on the download list, or take it off.
    ToggleList(FileId),
    /// Empty the download list.
    ClearList,
    /// Make an area.
    CreateArea {
        /// The name.
        name: String,
        /// Whether clean uploads skip approval.
        auto_approve: bool,
        /// Whether it needs a scoped grant.
        restricted: bool,
    },
}

/// The answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Response {
    /// The areas, and what else the member may reach.
    Areas {
        /// The areas.
        areas: Vec<AreaSummary>,
        /// Whether the approval queue is open to them.
        queue: bool,
        /// Whether they may make areas.
        manage: bool,
    },
    /// A page of an area.
    Area(FilePage),
    /// One file.
    File(Box<FileView>),
    /// A page of the queue.
    Queue(FilePage),
    /// A page of search results.
    Search {
        /// The query that was run.
        query: String,
        /// The page.
        page: FilePage,
    },
    /// The download list.
    List(Vec<FileSummary>),
    /// A change was made; ask for `next` to show the result.
    Changed {
        /// What to say.
        note: Note,
        /// What to load next.
        next: Option<Request>,
    },
    /// Nothing changed.
    Refused(Note),
    /// What was asked for is gone or was never visible: back to the areas.
    Gone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Typing {
    Idle,
    Search(String),
    Tag(String),
    Untag(String),
    Describe(String),
    AreaName(String),
    /// Yes or no for an action that cannot be undone.
    Confirm(Box<Request>, &'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NewArea {
    name: String,
    auto: bool,
    restricted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Loading,
    Areas {
        areas: Vec<AreaSummary>,
        queue: bool,
        manage: bool,
    },
    Area(FilePage),
    File(Box<FileView>),
    Queue(FilePage),
    Search {
        query: String,
        page: FilePage,
    },
    List(Vec<FileSummary>),
}

/// One open Files screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesScreen {
    mode: Mode,
    mode_stack: Vec<Mode>,
    pending: Option<Request>,
    waiting: bool,
    note: Option<Note>,
    typing: Typing,
    area_form: Option<NewArea>,
    files: FilesMode,
}

fn digit_index(c: char) -> Option<usize> {
    c.to_digit(10)
        .filter(|d| (1..=9).contains(d))
        .and_then(|d| usize::try_from(d - 1).ok())
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes;
    let mut rest = 0_u64;
    let mut unit = 0;
    while value >= 1024 && unit < UNITS.len() - 1 {
        rest = value % 1024;
        value /= 1024;
        unit += 1;
    }
    if unit == 0 {
        format!("{value} B")
    } else {
        format!("{value}.{} {}", rest * 10 / 1024, UNITS[unit])
    }
}

impl FilesScreen {
    /// Open the screens at the area list; the first request is waiting to be taken.
    #[must_use]
    pub fn open(files: FilesMode) -> Self {
        let mut screen = Self {
            mode: Mode::Loading,
            mode_stack: Vec::new(),
            pending: None,
            waiting: false,
            note: None,
            typing: Typing::Idle,
            area_form: None,
            files,
        };
        if files == FilesMode::Unavailable {
            screen.mode = Mode::Areas {
                areas: Vec::new(),
                queue: false,
                manage: false,
            };
            screen.note = Some(problem(keys::NOTE_UNAVAILABLE));
        } else {
            screen.ask(Request::Areas);
        }
        screen
    }

    fn ask(&mut self, request: Request) {
        self.pending = Some(request);
        self.waiting = true;
    }

    /// Whether the session still holds what the screens need.
    #[must_use]
    pub fn still_permitted(&self, access: &dyn PermissionCheck) -> bool {
        access.allows(Permission::FilesList)
    }

    /// The request the screen is waiting on, once. Run it with [`execute`] and give the answer to
    /// [`FilesScreen::deliver`].
    pub fn take_request(&mut self) -> Option<Request> {
        self.pending.take()
    }

    /// An answer arrived.
    pub fn deliver(&mut self, response: Response) -> Step {
        self.waiting = false;
        match response {
            Response::Areas {
                areas,
                queue,
                manage,
            } => {
                self.mode_stack.clear();
                self.mode = Mode::Areas {
                    areas,
                    queue,
                    manage,
                };
            }
            Response::Area(page) => self.show(Mode::Area(page)),
            Response::File(view) => self.show(Mode::File(view)),
            Response::Queue(page) => self.show(Mode::Queue(page)),
            Response::Search { query, page } => self.show(Mode::Search { query, page }),
            Response::List(rows) => self.show(Mode::List(rows)),
            Response::Changed { note, next } => {
                self.note = Some(note);
                self.typing = Typing::Idle;
                if let Some(next) = next {
                    self.ask(next);
                }
            }
            Response::Refused(note) => {
                self.note = Some(note);
                self.typing = Typing::Idle;
            }
            Response::Gone => {
                self.note = Some(problem(keys::NOTE_NOT_FOUND));
                self.typing = Typing::Idle;
                self.mode_stack.clear();
                self.mode = Mode::Loading;
                self.ask(Request::Areas);
            }
        }
        Step::Stay
    }

    /// Replace the current screen with `next`, remembering where it came from unless it is the
    /// same kind of screen (a reload after a change).
    fn show(&mut self, next: Mode) {
        let same = std::mem::discriminant(&self.mode) == std::mem::discriminant(&next);
        if !same && !matches!(self.mode, Mode::Loading) {
            let old = std::mem::replace(&mut self.mode, Mode::Loading);
            self.mode_stack.push(old);
        }
        self.mode = next;
    }

    /// Go back one screen, or leave from the first.
    fn back(&mut self) -> Step {
        match self.mode_stack.pop() {
            Some(previous) => {
                self.mode = previous;
                Step::Stay
            }
            None => Step::Leave,
        }
    }

    /// Handle one key.
    pub fn press(&mut self, access: &dyn PermissionCheck, key: ScreenKey) -> Step {
        if !access.allows(Permission::FilesList) {
            return Step::Denied;
        }
        if self.waiting {
            return if key == ScreenKey::Escape {
                Step::Leave
            } else {
                Step::Stay
            };
        }
        if self.typing != Typing::Idle {
            self.type_key(key);
            return Step::Stay;
        }
        if self.area_form.is_some() {
            self.form_key(key);
            return Step::Stay;
        }
        self.note = None;
        match key {
            ScreenKey::Escape => return self.back(),
            ScreenKey::Char('/') if self.files != FilesMode::Unavailable => {
                self.typing = Typing::Search(String::new());
                return Step::Stay;
            }
            ScreenKey::Char('l' | 'L') if self.files != FilesMode::Unavailable => {
                self.ask(Request::List);
                return Step::Stay;
            }
            _ => {}
        }
        match &self.mode {
            Mode::Loading => {}
            Mode::Areas { .. } => self.areas_key(access, key),
            Mode::Area(_) => self.area_key(key),
            Mode::File(_) => self.file_key(access, key),
            Mode::Queue(_) => self.queue_key(key),
            Mode::Search { .. } => self.search_key(key),
            Mode::List(_) => self.list_key(key),
        }
        Step::Stay
    }

    fn areas_key(&mut self, access: &dyn PermissionCheck, key: ScreenKey) {
        let Mode::Areas {
            areas,
            queue,
            manage,
        } = &self.mode
        else {
            return;
        };
        let (queue, manage) = (*queue, *manage);
        match key {
            ScreenKey::Char(c) => {
                if let Some(area) = digit_index(c).and_then(|i| areas.get(i)) {
                    let area = area.id;
                    self.ask(Request::Area { area, page: 0 });
                } else if matches!(c, 'q' | 'Q') && queue {
                    self.ask(Request::Queue { page: 0 });
                } else if matches!(c, 'n' | 'N')
                    && manage
                    && access.allows(Permission::FilesManageAreas)
                {
                    self.area_form = Some(NewArea {
                        name: String::new(),
                        auto: false,
                        restricted: false,
                    });
                    self.typing = Typing::AreaName(String::new());
                }
            }
            _ => {}
        }
    }

    fn area_key(&mut self, key: ScreenKey) {
        let Mode::Area(page) = &self.mode else {
            return;
        };
        let Some(area) = page.area.as_ref().map(|a| a.id) else {
            return;
        };
        let current = page.page;
        match key {
            ScreenKey::Char(c) => {
                if let Some(file) = digit_index(c).and_then(|i| page.files.get(i)) {
                    let id = file.id;
                    self.ask(Request::File(id));
                } else if matches!(c, 'n' | 'N') && page.more {
                    self.ask(Request::Area {
                        area,
                        page: current + 1,
                    });
                } else if matches!(c, 'p' | 'P') && current > 0 {
                    self.ask(Request::Area {
                        area,
                        page: current - 1,
                    });
                }
            }
            _ => {}
        }
    }

    fn queue_key(&mut self, key: ScreenKey) {
        let Mode::Queue(page) = &self.mode else {
            return;
        };
        let current = page.page;
        if let ScreenKey::Char(c) = key {
            if let Some(file) = digit_index(c).and_then(|i| page.files.get(i)) {
                let id = file.id;
                self.ask(Request::File(id));
            } else if matches!(c, 'n' | 'N') && page.more {
                self.ask(Request::Queue { page: current + 1 });
            } else if matches!(c, 'p' | 'P') && current > 0 {
                self.ask(Request::Queue { page: current - 1 });
            }
        }
    }

    fn search_key(&mut self, key: ScreenKey) {
        let Mode::Search { query, page } = &self.mode else {
            return;
        };
        let current = page.page;
        if let ScreenKey::Char(c) = key {
            if let Some(file) = digit_index(c).and_then(|i| page.files.get(i)) {
                let id = file.id;
                self.ask(Request::File(id));
            } else if matches!(c, 'n' | 'N') && page.more {
                let query = query.clone();
                self.ask(Request::Search {
                    query,
                    page: current + 1,
                });
            } else if matches!(c, 'p' | 'P') && current > 0 {
                let query = query.clone();
                self.ask(Request::Search {
                    query,
                    page: current - 1,
                });
            }
        }
    }

    fn list_key(&mut self, key: ScreenKey) {
        let Mode::List(rows) = &self.mode else {
            return;
        };
        if let ScreenKey::Char(c) = key {
            if let Some(file) = digit_index(c).and_then(|i| rows.get(i)) {
                let id = file.id;
                self.ask(Request::File(id));
            } else if matches!(c, 'c' | 'C') && !rows.is_empty() {
                self.ask(Request::ClearList);
            }
        }
    }

    fn file_key(&mut self, access: &dyn PermissionCheck, key: ScreenKey) {
        let Mode::File(view) = &self.mode else {
            return;
        };
        let id = view.file.id;
        let approved = view.file.state == FileState::Approved;
        let ScreenKey::Char(c) = key else {
            return;
        };
        match c.to_ascii_lowercase() {
            't' if view.can_tag && access.allows(Permission::FilesTag) => {
                self.typing = Typing::Tag(String::new());
            }
            'u' if view.can_tag && !view.file.tags.is_empty() => {
                self.typing = Typing::Untag(String::new());
            }
            'd' if view.can_edit => self.typing = Typing::Describe(String::new()),
            'b' if approved && view.download_path.is_some() => {
                self.ask(Request::ToggleList(id));
            }
            'a' if view.can_decide && !approved => self.ask(Request::Approve(id)),
            'r' if view.can_decide && !approved => {
                self.typing = Typing::Confirm(Box::new(Request::Reject(id)), keys::CONFIRM_REJECT);
            }
            'x' if view.can_decide || (view.can_edit && !approved) => {
                self.typing = Typing::Confirm(Box::new(Request::Remove(id)), keys::CONFIRM_REMOVE);
            }
            _ => {}
        }
    }

    fn type_key(&mut self, key: ScreenKey) {
        let file = match &self.mode {
            Mode::File(view) => Some(view.file.id),
            _ => None,
        };
        let typing = std::mem::replace(&mut self.typing, Typing::Idle);
        let mut request = None;
        let next = match typing {
            Typing::Idle => Typing::Idle,
            Typing::Confirm(request_to_run, text) => match key {
                ScreenKey::Char('y' | 'Y') => {
                    request = Some(*request_to_run);
                    Typing::Idle
                }
                ScreenKey::Char(_) | ScreenKey::Backspace | ScreenKey::Enter => {
                    Typing::Confirm(request_to_run, text)
                }
                ScreenKey::Escape => Typing::Idle,
            },
            Typing::Search(mut text) => match self.edit(&mut text, key, 100) {
                Edit::Submit if !text.trim().is_empty() => {
                    request = Some(Request::Search {
                        query: text.clone(),
                        page: 0,
                    });
                    Typing::Idle
                }
                Edit::Submit | Edit::Cancel => Typing::Idle,
                Edit::Keep => Typing::Search(text),
            },
            Typing::Tag(mut text) => match self.edit(&mut text, key, TAG_MAX_CHARS + 8) {
                Edit::Submit => {
                    request = file.map(|id| Request::Tag(id, text.clone()));
                    Typing::Idle
                }
                Edit::Cancel => Typing::Idle,
                Edit::Keep => Typing::Tag(text),
            },
            Typing::Untag(mut text) => match self.edit(&mut text, key, TAG_MAX_CHARS + 8) {
                Edit::Submit => {
                    request = file.map(|id| Request::Untag(id, text.clone()));
                    Typing::Idle
                }
                Edit::Cancel => Typing::Idle,
                Edit::Keep => Typing::Untag(text),
            },
            Typing::Describe(mut text) => {
                match self.edit(&mut text, key, super::text::FILE_DESCRIPTION_MAX_CHARS) {
                    Edit::Submit => {
                        request = file.map(|id| Request::Describe(id, text.clone()));
                        Typing::Idle
                    }
                    Edit::Cancel => Typing::Idle,
                    Edit::Keep => Typing::Describe(text),
                }
            }
            Typing::AreaName(mut text) => {
                match self.edit(&mut text, key, super::text::AREA_NAME_MAX_CHARS) {
                    Edit::Submit if !text.trim().is_empty() => {
                        if let Some(form) = &mut self.area_form {
                            form.name = text;
                        }
                        Typing::Idle
                    }
                    Edit::Cancel => {
                        self.area_form = None;
                        Typing::Idle
                    }
                    Edit::Submit | Edit::Keep => Typing::AreaName(text),
                }
            }
        };
        self.typing = next;
        if let Some(request) = request {
            self.ask(request);
        }
    }

    /// The new-area form after the name: `a` toggles auto-approve, `r` restricted, Enter makes
    /// it, Esc drops it.
    fn form_key(&mut self, key: ScreenKey) {
        let Some(form) = &mut self.area_form else {
            return;
        };
        match key {
            ScreenKey::Escape => self.area_form = None,
            ScreenKey::Char('a' | 'A') => form.auto = !form.auto,
            ScreenKey::Char('r' | 'R') => form.restricted = !form.restricted,
            ScreenKey::Enter => {
                let request = Request::CreateArea {
                    name: form.name.clone(),
                    auto_approve: form.auto,
                    restricted: form.restricted,
                };
                self.area_form = None;
                self.ask(request);
            }
            ScreenKey::Char(_) | ScreenKey::Backspace => {}
        }
    }

    #[allow(clippy::unused_self)]
    fn edit(&self, text: &mut String, key: ScreenKey, max: usize) -> Edit {
        match key {
            ScreenKey::Escape => Edit::Cancel,
            ScreenKey::Enter => Edit::Submit,
            ScreenKey::Backspace => {
                text.pop();
                Edit::Keep
            }
            ScreenKey::Char(c) => {
                if !c.is_control() && text.chars().count() < max {
                    text.push(c);
                }
                Edit::Keep
            }
        }
    }

    /// What a layout draws.
    #[must_use]
    pub fn view(&self, access: &dyn PermissionCheck) -> ScreenView {
        let _ = access;
        let mut lines = Vec::new();
        let mut footer = keys::FOOTER_BACK;
        match &self.mode {
            Mode::Loading => {}
            Mode::Areas {
                areas,
                queue,
                manage,
            } => {
                footer = keys::FOOTER_AREAS;
                lines.push(Line::Heading(keys::AREAS_TITLE));
                self.note_line(&mut lines);
                if areas.is_empty() {
                    lines.push(Line::Text(keys::AREAS_EMPTY));
                }
                for (index, area) in areas.iter().enumerate() {
                    lines.push(Line::Filled {
                        key: keys::AREA_ROW,
                        values: vec![
                            ("key", number(index + 1)),
                            ("name", area.name.clone()),
                            ("files", area.files.to_string()),
                        ],
                    });
                    if area.restricted {
                        lines.push(Line::Text(keys::AREA_FLAG_RESTRICTED));
                    }
                    if area.auto_approve {
                        lines.push(Line::Text(keys::AREA_FLAG_AUTO));
                    }
                }
                lines.push(Line::Blank);
                if !areas.is_empty() {
                    lines.push(Line::Text(keys::HINT_OPEN));
                }
                lines.push(Line::Text(keys::HINT_SEARCH));
                lines.push(Line::Text(keys::HINT_LIST));
                if *queue {
                    lines.push(Line::Text(keys::HINT_QUEUE));
                }
                if *manage {
                    lines.push(Line::Text(keys::HINT_NEW_AREA));
                }
                lines.push(Line::Blank);
                lines.push(Line::Text(self.mode_note()));
                lines.push(Line::Text(keys::NOTE_READABLE));
            }
            Mode::Area(page) => {
                footer = keys::FOOTER_AREA;
                if let Some(area) = &page.area {
                    lines.push(Line::Filled {
                        key: keys::AREA_TITLE,
                        values: vec![("name", area.name.clone())],
                    });
                }
                self.note_line(&mut lines);
                if page.files.is_empty() {
                    lines.push(Line::Text(keys::AREA_EMPTY));
                }
                file_rows(&mut lines, &page.files, keys::FILE_ROW);
                page_lines(&mut lines, page);
                if page.can_upload && self.files == FilesMode::Sftp {
                    if let Some(area) = &page.area {
                        lines.push(Line::Blank);
                        lines.push(Line::Filled {
                            key: keys::HINT_UPLOAD,
                            values: vec![("path", format!("/{}/", area.dir))],
                        });
                    }
                }
                lines.push(Line::Blank);
                lines.push(Line::Text(self.mode_note()));
            }
            Mode::File(view) => {
                footer = keys::FOOTER_FILE;
                self.file_lines(&mut lines, view);
            }
            Mode::Queue(page) => {
                footer = keys::FOOTER_AREA;
                lines.push(Line::Heading(keys::QUEUE_TITLE));
                self.note_line(&mut lines);
                if page.files.is_empty() {
                    lines.push(Line::Text(keys::QUEUE_EMPTY));
                }
                file_rows(&mut lines, &page.files, keys::QUEUE_ROW);
                page_lines(&mut lines, page);
            }
            Mode::Search { query, page } => {
                footer = keys::FOOTER_AREA;
                lines.push(Line::Filled {
                    key: keys::SEARCH_TITLE,
                    values: vec![("query", query.clone())],
                });
                self.note_line(&mut lines);
                if page.files.is_empty() {
                    lines.push(Line::Text(keys::SEARCH_EMPTY));
                }
                file_rows(&mut lines, &page.files, keys::FILE_ROW);
                page_lines(&mut lines, page);
            }
            Mode::List(rows) => {
                lines.push(Line::Heading(keys::LIST_TITLE));
                self.note_line(&mut lines);
                if rows.is_empty() {
                    lines.push(Line::Text(keys::LIST_EMPTY));
                }
                file_rows(&mut lines, rows, keys::LIST_ROW);
                lines.push(Line::Blank);
                if self.files == FilesMode::Sftp {
                    lines.push(Line::Text(keys::LIST_HOWTO));
                } else {
                    lines.push(Line::Text(self.mode_note()));
                }
                if !rows.is_empty() {
                    lines.push(Line::Text(keys::HINT_LIST_CLEAR));
                }
            }
        }
        self.typing_lines(&mut lines, &mut footer);
        ScreenView { lines, footer }
    }

    fn mode_note(&self) -> &'static str {
        match self.files {
            FilesMode::Sftp => keys::NOTE_SFTP,
            FilesMode::NoSftp => keys::NOTE_NO_SFTP,
            FilesMode::Telnet => keys::NOTE_TELNET,
            FilesMode::Unavailable => keys::NOTE_UNAVAILABLE,
        }
    }

    fn note_line(&self, lines: &mut Vec<Line>) {
        if let Some(note) = self.note {
            lines.push(Line::Notice {
                error: note.error,
                text: note.key,
            });
        }
    }

    fn file_lines(&self, lines: &mut Vec<Line>, view: &FileView) {
        let file = &view.file;
        lines.push(Line::Filled {
            key: keys::FILE_TITLE,
            values: vec![("name", file.name.clone())],
        });
        self.note_line(lines);
        let pair =
            |lines: &mut Vec<Line>, key: &'static str, values: Vec<(&'static str, String)>| {
                lines.push(Line::Filled { key, values });
            };
        pair(
            lines,
            keys::FILE_AREA,
            vec![("area", file.area_name.clone())],
        );
        pair(
            lines,
            keys::FILE_SIZE,
            vec![
                ("size", human_size(file.size)),
                ("bytes", file.size.to_string()),
            ],
        );
        pair(
            lines,
            keys::FILE_UPLOADER,
            vec![
                ("handle", file.uploader.clone().unwrap_or_default()),
                ("when", format_utc(file.at)),
            ],
        );
        pair(lines, keys::FILE_SHA, vec![("hash", view.sha256.clone())]);
        lines.push(Line::Text(match view.scan {
            ScanState::Unscanned => keys::FILE_SCAN_UNSCANNED,
            ScanState::Clean => keys::FILE_SCAN_CLEAN,
            ScanState::Failed | ScanState::Infected => keys::FILE_SCAN_FAILED,
        }));
        if file.tags.is_empty() {
            lines.push(Line::Text(keys::FILE_TAGS_NONE));
        } else {
            pair(lines, keys::FILE_TAGS, vec![("tags", file.tags.join(", "))]);
        }
        if !view.description.as_str().is_empty() {
            pair(
                lines,
                keys::FILE_DESCRIPTION,
                vec![("text", view.description.as_str().to_owned())],
            );
        }
        if file.state == FileState::Approved {
            pair(
                lines,
                keys::FILE_DOWNLOADS,
                vec![("count", file.downloads.to_string())],
            );
        } else {
            lines.push(Line::Text(keys::FILE_PENDING));
        }
        if view.on_list {
            lines.push(Line::Text(keys::FILE_ON_LIST));
        }
        lines.push(Line::Blank);
        match (&view.download_path, self.files) {
            (Some(path), FilesMode::Sftp) => {
                pair(lines, keys::DOWNLOAD_PATH, vec![("path", path.clone())]);
            }
            (Some(_) | None, FilesMode::Telnet) => lines.push(Line::Text(keys::NOTE_TELNET)),
            (Some(_) | None, FilesMode::NoSftp) => lines.push(Line::Text(keys::NOTE_NO_SFTP)),
            (None, FilesMode::Sftp | FilesMode::Unavailable)
            | (Some(_), FilesMode::Unavailable) => {
                if file.state == FileState::Approved {
                    lines.push(Line::Text(keys::DOWNLOAD_NO_ACCESS));
                }
            }
        }
        lines.push(Line::Text(keys::NOTE_READABLE));
        lines.push(Line::Blank);
        if view.can_tag {
            lines.push(Line::Text(keys::HINT_TAG));
        }
        if view.can_edit {
            lines.push(Line::Text(keys::HINT_DESCRIBE));
        }
        if file.state == FileState::Approved && view.download_path.is_some() {
            lines.push(Line::Text(keys::HINT_LIST_TOGGLE));
        }
        if view.can_decide && file.state != FileState::Approved {
            lines.push(Line::Text(keys::HINT_DECIDE));
        }
        if view.can_decide || (view.can_edit && file.state != FileState::Approved) {
            lines.push(Line::Text(keys::HINT_REMOVE));
        }
    }

    fn typing_lines(&self, lines: &mut Vec<Line>, footer: &mut &'static str) {
        let field = |lines: &mut Vec<Line>, label, value: &String| {
            lines.push(Line::Field {
                label,
                value: value.clone(),
                active: true,
            });
        };
        match &self.typing {
            Typing::Idle => {
                if let Some(form) = &self.area_form {
                    lines.push(Line::Pair {
                        label: keys::FIELD_AREA_NAME,
                        value: Some(form.name.clone()),
                    });
                    lines.push(Line::Pair {
                        label: keys::FIELD_AREA_AUTO,
                        value: Some(yes_no(form.auto)),
                    });
                    lines.push(Line::Pair {
                        label: keys::FIELD_AREA_RESTRICTED,
                        value: Some(yes_no(form.restricted)),
                    });
                    *footer = keys::FOOTER_TYPING;
                }
            }
            Typing::Search(text) => {
                field(lines, keys::FIELD_SEARCH, text);
                *footer = keys::FOOTER_TYPING;
            }
            Typing::Tag(text) => {
                field(lines, keys::FIELD_TAG, text);
                *footer = keys::FOOTER_TYPING;
            }
            Typing::Untag(text) => {
                field(lines, keys::FIELD_UNTAG, text);
                *footer = keys::FOOTER_TYPING;
            }
            Typing::Describe(text) => {
                field(lines, keys::FIELD_DESCRIPTION, text);
                *footer = keys::FOOTER_TYPING;
            }
            Typing::AreaName(text) => {
                field(lines, keys::FIELD_AREA_NAME, text);
                *footer = keys::FOOTER_TYPING;
            }
            Typing::Confirm(_, text) => {
                lines.push(Line::Notice { error: true, text });
                *footer = keys::FOOTER_CONFIRM;
            }
        }
    }
}

enum Edit {
    Keep,
    Submit,
    Cancel,
}

fn yes_no(on: bool) -> String {
    if on { "yes" } else { "no" }.to_owned()
}

fn number(n: usize) -> String {
    n.to_string()
}

fn file_rows(lines: &mut Vec<Line>, files: &[FileSummary], key: &'static str) {
    for (index, file) in files.iter().enumerate() {
        lines.push(Line::Filled {
            key,
            values: vec![
                ("key", number(index + 1)),
                ("name", file.name.clone()),
                ("size", human_size(file.size)),
                ("area", file.area_name.clone()),
                ("handle", file.uploader.clone().unwrap_or_default()),
                ("tags", file.tags.join(" ")),
            ],
        });
    }
}

fn page_lines(lines: &mut Vec<Line>, page: &FilePage) {
    if page.page > 0 || page.more {
        lines.push(Line::Filled {
            key: keys::PAGE,
            values: vec![("page", (page.page + 1).to_string())],
        });
    }
    if !page.files.is_empty() {
        lines.push(Line::Text(keys::HINT_OPEN));
    }
    if page.more {
        lines.push(Line::Text(keys::HINT_NEXT));
    }
    if page.page > 0 {
        lines.push(Line::Text(keys::HINT_PREVIOUS));
    }
}

fn refusal(error: &FileError) -> Note {
    match error {
        FileError::NotFound => problem(keys::NOTE_NOT_FOUND),
        FileError::NotPermitted(_) | FileError::NotSignedIn => problem(keys::NOTE_NOT_PERMITTED),
        FileError::Text(_) => problem(keys::NOTE_TEXT),
        FileError::TooManyTags => problem(keys::NOTE_TOO_MANY_TAGS),
        FileError::ListFull => problem(keys::NOTE_LIST_FULL),
        FileError::NotPending => problem(keys::NOTE_DECIDED),
        FileError::NameTaken => problem(keys::NOTE_NAME_TAKEN),
        FileError::Limit(_)
        | FileError::UnknownMember
        | FileError::Blob(_)
        | FileError::Store(_) => problem(keys::NOTE_FAILED),
    }
}

fn failed(error: &FileError) -> Response {
    match error {
        FileError::NotFound => Response::Gone,
        other => Response::Refused(refusal(other)),
    }
}

/// Run a request against the store and the blob store with the access loaded for this action.
/// `blobs` is `None` on a board built without a file store: reads that need none still work,
/// changes that move bytes are refused.
pub async fn execute<S: FileStore + AccessStore, B: BlobStore>(
    store: &S,
    blobs: Option<&B>,
    snapshot: &Snapshot,
    now: UnixSeconds,
    request: Request,
) -> Response {
    match run(store, blobs, snapshot, now, request).await {
        Ok(response) => response,
        Err(error) => failed(&error),
    }
}

async fn run<S: FileStore + AccessStore, B: BlobStore>(
    store: &S,
    blobs: Option<&B>,
    snapshot: &Snapshot,
    now: UnixSeconds,
    request: Request,
) -> Result<Response, FileError> {
    let changed = |note: Note, next: Request| Response::Changed {
        note,
        next: Some(next),
    };
    Ok(match request {
        Request::Areas => Response::Areas {
            areas: service::areas(store, snapshot).await?,
            queue: service::can_approve_anywhere(snapshot),
            manage: snapshot
                .resolve(Permission::FilesManageAreas, None)
                .is_granted(),
        },
        Request::Area { area, page } => {
            Response::Area(service::area_page(store, snapshot, area, page).await?)
        }
        Request::File(id) => {
            Response::File(Box::new(service::file_view(store, snapshot, id).await?))
        }
        Request::Queue { page } => Response::Queue(service::queue(store, snapshot, page).await?),
        Request::List => Response::List(
            service::list_files(store, snapshot)
                .await?
                .iter()
                .map(FileSummary::of)
                .collect(),
        ),
        Request::Search { query, page } => Response::Search {
            page: service::search(store, snapshot, &query, page).await?,
            query,
        },
        Request::Approve(id) => {
            let Some(blobs) = blobs else {
                return Ok(Response::Refused(problem(keys::NOTE_UNAVAILABLE)));
            };
            service::approve(store, blobs, snapshot, id, now).await?;
            changed(done(keys::NOTE_APPROVED), Request::Queue { page: 0 })
        }
        Request::Reject(id) => {
            let Some(blobs) = blobs else {
                return Ok(Response::Refused(problem(keys::NOTE_UNAVAILABLE)));
            };
            service::reject(store, blobs, snapshot, id, now).await?;
            changed(done(keys::NOTE_REJECTED), Request::Queue { page: 0 })
        }
        Request::Remove(id) => {
            let Some(blobs) = blobs else {
                return Ok(Response::Refused(problem(keys::NOTE_UNAVAILABLE)));
            };
            service::remove(store, blobs, snapshot, id).await?;
            changed(done(keys::NOTE_REMOVED), Request::Areas)
        }
        Request::Tag(id, text) => {
            service::tag(store, snapshot, id, &text).await?;
            changed(done(keys::NOTE_TAGGED), Request::File(id))
        }
        Request::Untag(id, text) => {
            service::untag(store, snapshot, id, &text).await?;
            changed(done(keys::NOTE_UNTAGGED), Request::File(id))
        }
        Request::Describe(id, text) => {
            service::describe(store, snapshot, id, &text).await?;
            changed(done(keys::NOTE_DESCRIBED), Request::File(id))
        }
        Request::ToggleList(id) => {
            let view = service::file_view(store, snapshot, id).await?;
            if view.on_list {
                service::list_remove(store, snapshot, id).await?;
                changed(done(keys::NOTE_UNLISTED), Request::File(id))
            } else {
                service::list_add(store, snapshot, id, now).await?;
                changed(done(keys::NOTE_LISTED), Request::File(id))
            }
        }
        Request::ClearList => {
            service::list_clear(store, snapshot).await?;
            changed(done(keys::NOTE_LIST_CLEARED), Request::List)
        }
        Request::CreateArea {
            name,
            auto_approve,
            restricted,
        } => {
            service::create_area(store, snapshot, now, &name, "", restricted, auto_approve).await?;
            changed(done(keys::NOTE_AREA_CREATED), Request::Areas)
        }
    })
}
