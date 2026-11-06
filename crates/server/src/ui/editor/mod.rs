//! The full-screen message editor (M3-B, D48, D80): the widget the forum, mail and poll slices
//! call to compose text.
//!
//! # Using it
//!
//! ```text
//! let request = EditorRequest::new(title)          // title: Sanitized, shown in the box
//!     .context(ContextKey::parse("forum:reply:12")?) // optional: enables saved drafts
//!     .initial(quote(Some(&who), &post, QuoteLimits::default())) // optional starting text
//!     .limits(BodyLimits::new(4_000, 100));          // optional, never above the board-wide cap
//! let mut editor = Editor::open_with_store(&store, account, request).await?; // loads a draft
//! // every key batch:
//! match editor.input(key) {
//!     EditorEffect::Stay => {}
//!     EditorEffect::Done(Outcome::Submitted(body)) => { /* write the post, THEN discard_draft */ }
//!     EditorEffect::Done(Outcome::Cancelled) => { /* nothing was sent or saved */ }
//!     EditorEffect::SaveDraft(save) => {
//!         let result = save.persist(&store, account, now).await;
//!         match editor.draft_saved(result) { EditorEffect::Done(Outcome::SavedDraft) => {}, _ => {} }
//!     }
//!     EditorEffect::LogOff(save) => { /* persist `save` if Some, then end the session */ }
//! }
//! editor.render(frame, &EditorView { ui, settings, markers }); // every frame
//! ```
//!
//! The editor is a plain state machine (no store, no tasks): the host session feeds it keys,
//! draws it and does the store work its effects ask for, the same way the other screens work
//! (`ssh/screen.rs`). `docs/ui.md` documents this API and the keys.
//!
//! # What it guarantees
//!
//! * **Bounded.** Typing, pasting from the editor's own kill buffer, and undo/redo can never take
//!   the text past [`BodyLimits`]; a refused edit is undone and says so in words.
//! * **Sanitized.** Everything on screen is typed printable ASCII or text that already passed
//!   the shared sanitizer ([`Sanitized`], [`Body`]). What leaves the editor is a [`Body`] made by
//!   the shared multi-line sanitizer again; there is no other sanitizer here.
//! * **No chrome from content.** The text is drawn with a plain style (quote lines dimmed), the
//!   cursor with reverse+underline, never the reserved marker style (bold + reverse), and only
//!   inside the box. Security markers go in the reserved status row through
//!   `chrome::render_status`, which no content can reach (D39, D45).
//! * **Honest.** The editor says nothing about who can read a draft or a message. It does not call
//!   anything private or show a lock; callers add no such claim either (AGENTS.md, D78).

mod keys;
mod render;
mod strings;

use russh_bbs_core::drafts::{
    Body, BodyError, BodyLimits, ContextKey, Draft, DraftError, DraftStore, SaveOutcome,
    is_quote_line, save_draft,
};
use russh_bbs_core::icons::SecurityMarker;
use russh_bbs_core::sanitize::Sanitized;
use russh_bbs_core::store::{AccountId, UnixSeconds};
use russh_bbs_core::terminal_settings::TerminalSettings;
use russh_bbs_core::terminal_settings::strings::fill;
use russh_bbs_core::waiting_room::NoticeKind;
use tui_textarea::{CursorMove, Scrolling, TextArea, WrapMode};

pub use keys::EditorKey;

use crate::ssh::terminal::Key;
use crate::ui::Ui;

/// Spaces a tab inserts.
const TAB_LENGTH: u8 = 4;

/// Ask for an editor.
#[derive(Debug, Clone)]
pub struct EditorRequest {
    title: Sanitized,
    context: Option<ContextKey>,
    initial: Option<Sanitized>,
    limits: BodyLimits,
}

impl EditorRequest {
    /// A request with `title` in the box heading. The title is one line the caller sanitized
    /// (a board name is member-chosen text, so it must have been).
    #[must_use]
    pub fn new(title: Sanitized) -> Self {
        Self {
            title,
            context: None,
            initial: None,
            limits: BodyLimits::MAX,
        }
    }

    /// Give the editor a context key: it then offers to save a draft, and
    /// [`Editor::open_with_store`] resumes the member's draft for that key.
    #[must_use]
    pub fn context(mut self, context: ContextKey) -> Self {
        self.context = Some(context);
        self
    }

    /// Starting text, already sanitized (a [`quote`](russh_bbs_core::drafts::quote), or the
    /// text of a post being edited). A saved draft for the same context takes its place.
    #[must_use]
    pub fn initial(mut self, text: Sanitized) -> Self {
        self.initial = Some(text);
        self
    }

    /// Narrower bounds than the board-wide ones.
    #[must_use]
    pub fn limits(mut self, limits: BodyLimits) -> Self {
        self.limits = limits;
        self
    }
}

/// How an editor session ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The member sent the text. Write it, and only then call
    /// [`discard_draft`](russh_bbs_core::drafts::discard_draft) for the editor's
    /// [`context`](Editor::context), so a failed write loses nothing.
    Submitted(Body),
    /// The member left without sending or saving. Nothing stored changed.
    Cancelled,
    /// The member's draft was saved ([`Editor::draft_saved`]) and the editor is closed.
    SavedDraft,
}

/// A draft the host should store: the text as typed, and the bounds it must fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftSave {
    context: ContextKey,
    text: String,
    limits: BodyLimits,
}

impl DraftSave {
    /// The context the draft is for.
    #[must_use]
    pub fn context(&self) -> &ContextKey {
        &self.context
    }

    /// The text to store (sanitized again by [`save_draft`]).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Store it: [`save_draft`] with this draft's context, text and limits.
    ///
    /// # Errors
    /// What [`save_draft`] returns.
    pub async fn persist<S: DraftStore>(
        &self,
        store: &S,
        account: AccountId,
        now: UnixSeconds,
    ) -> Result<SaveOutcome, DraftError> {
        save_draft(store, account, &self.context, &self.text, self.limits, now).await
    }
}

/// What the host does after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorEffect {
    /// Redraw and keep going.
    Stay,
    /// The editor is finished.
    Done(Outcome),
    /// The member asked to keep a draft and leave. Store it with [`DraftSave::persist`], then
    /// give the result to [`Editor::draft_saved`], which finishes the editor or, if the draft
    /// was refused, returns to editing with the reason on screen.
    SaveDraft(DraftSave),
    /// The member logged off (Ctrl-C, Ctrl-D) from inside the editor. Store the draft if there
    /// is one (changed text and a context key), then end the session.
    LogOff(Option<DraftSave>),
}

/// What the editor needs to draw a frame, besides itself.
#[derive(Debug, Clone, Copy)]
pub struct EditorView<'a> {
    /// The string table and the framework's chrome strings.
    pub ui: &'a Ui,
    /// The member's settings: color, icon tier and border style.
    pub settings: &'a TerminalSettings,
    /// Markers for the reserved status row. The same ones the rest of the signed-in session
    /// shows; the editor adds none of its own.
    pub markers: &'a [SecurityMarker],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Editing,
    /// Esc with unsaved changes: save, discard or keep editing.
    Leaving,
    /// The key list.
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Message {
    kind: NoticeKind,
    text: String,
}

impl Message {
    fn info(text: impl Into<String>) -> Self {
        Self {
            kind: NoticeKind::Info,
            text: text.into(),
        }
    }

    fn problem(text: impl Into<String>) -> Self {
        Self {
            kind: NoticeKind::Error,
            text: text.into(),
        }
    }
}

/// One open editor.
#[derive(Debug, Clone)]
pub struct Editor {
    area: TextArea<'static>,
    title: Sanitized,
    context: Option<ContextKey>,
    limits: BodyLimits,
    /// The text when the editor opened (or the draft it resumed): unsaved changes are measured
    /// against it.
    baseline: String,
    mode: Mode,
    message: Option<Message>,
}

impl Editor {
    /// Open an editor. `saved` is the member's stored draft for the request's context, if the
    /// caller already loaded one; it replaces the request's starting text.
    ///
    /// # Errors
    /// [`BodyError`] when the starting text or the saved draft does not fit the request's
    /// limits. That is a bug in the caller (a quote larger than the post it is quoted into);
    /// the editor refuses rather than cut a member's text.
    pub fn open(request: EditorRequest, saved: Option<Draft>) -> Result<Self, BodyError> {
        let EditorRequest {
            title,
            context,
            initial,
            limits,
        } = request;
        let mut message = None;
        let start = match (saved, initial) {
            (Some(draft), initial) => {
                message = Some(Message::info(if initial.is_some() {
                    strings::RESUMED_INSTEAD
                } else {
                    strings::RESUMED
                }));
                Body::prepare(limits, draft.body.as_str())?
            }
            (None, Some(initial)) => Body::from_sanitized(limits, &initial)?,
            (None, None) => Body::prepare(limits, "")?,
        };
        let text = start.as_str().to_owned();
        let mut area = TextArea::new(text.split('\n').map(str::to_owned).collect());
        area.set_wrap_mode(WrapMode::Word);
        area.set_tab_length(TAB_LENGTH);
        // The default underlines the cursor's line; reverse alone is one bold away from the
        // reserved marker style, so the cursor cell is reverse + underline (D39).
        area.set_cursor_line_style(ratatui::style::Style::default());
        area.set_cursor_style(ratatui::style::Style::default().add_modifier(
            ratatui::style::Modifier::REVERSED | ratatui::style::Modifier::UNDERLINED,
        ));
        area.move_cursor(CursorMove::Bottom);
        area.move_cursor(CursorMove::End);
        let mut editor = Self {
            area,
            title,
            context,
            limits,
            baseline: text,
            mode: Mode::Editing,
            message,
        };
        editor.mark_quotes();
        Ok(editor)
    }

    /// Open an editor, resuming the member's saved draft for the request's context if there is
    /// one.
    ///
    /// # Errors
    /// [`DraftError::Store`] when the draft cannot be read; [`DraftError::Body`] as for
    /// [`Editor::open`].
    pub async fn open_with_store<S: DraftStore>(
        store: &S,
        account: AccountId,
        request: EditorRequest,
    ) -> Result<Self, DraftError> {
        let saved = match &request.context {
            Some(context) => russh_bbs_core::drafts::load_draft(store, account, context).await?,
            None => None,
        };
        Ok(Self::open(request, saved)?)
    }

    /// The context key, if the request had one.
    #[must_use]
    pub fn context(&self) -> Option<&ContextKey> {
        self.context.as_ref()
    }

    /// The text as typed so far.
    #[must_use]
    pub fn text(&self) -> String {
        self.area.lines().join("\n")
    }

    /// The cursor as (line, column), both counted in characters from zero.
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        self.area.cursor()
    }

    /// Whether the text differs from what the editor opened with.
    #[must_use]
    pub fn is_changed(&self) -> bool {
        self.text() != self.baseline
    }

    /// Feed one key from the terminal.
    pub fn input(&mut self, key: Key) -> EditorEffect {
        match EditorKey::from_terminal(key) {
            Some(key) => self.press(key),
            None => EditorEffect::Stay,
        }
    }

    /// Feed one editor key.
    pub fn press(&mut self, key: EditorKey) -> EditorEffect {
        if key == EditorKey::LogOff {
            return EditorEffect::LogOff(self.pending_draft());
        }
        match self.mode {
            Mode::Help => {
                self.mode = Mode::Editing;
                EditorEffect::Stay
            }
            Mode::Leaving => self.leaving(key),
            Mode::Editing => self.editing(key),
        }
    }

    /// Tell the editor what became of a [`EditorEffect::SaveDraft`]. A stored draft finishes
    /// the editor; a refused one (the member already keeps the most drafts allowed, or storage
    /// failed) returns to editing with the reason on screen, so nothing typed is lost.
    pub fn draft_saved(&mut self, result: Result<SaveOutcome, DraftError>) -> EditorEffect {
        match result {
            Ok(_) => EditorEffect::Done(Outcome::SavedDraft),
            Err(error) => {
                self.mode = Mode::Editing;
                self.message = Some(Message::problem(match error {
                    DraftError::Store(_) => strings::DRAFT_FAILED.to_owned(),
                    other => fill(strings::DRAFT_REFUSED, &[("why", &other.to_string())]),
                }));
                EditorEffect::Stay
            }
        }
    }

    /// Draw the editor into `frame`: a box with the text, its counters and the keys, and the
    /// reserved status row underneath.
    pub fn render(&self, frame: &mut ratatui::Frame<'_>, view: &EditorView<'_>) {
        render::render(self, frame, view);
    }

    fn editing(&mut self, key: EditorKey) -> EditorEffect {
        self.message = None;
        match key {
            EditorKey::Send => return self.send(),
            EditorKey::Leave => return self.leave(),
            EditorKey::Help => self.mode = Mode::Help,
            EditorKey::Char(c) => self.guarded(|area| area.insert_char(c)),
            EditorKey::Enter => self.guarded(TextArea::insert_newline),
            EditorKey::Tab => self.guarded(|area| {
                area.insert_tab();
            }),
            EditorKey::Backspace => self.guarded(|area| {
                area.delete_char();
            }),
            EditorKey::Delete => self.guarded(|area| {
                area.delete_next_char();
            }),
            EditorKey::DeleteWord => self.guarded(|area| {
                area.delete_word();
            }),
            EditorKey::KillLine => self.guarded(|area| {
                area.delete_line_by_end();
            }),
            EditorKey::Yank => self.guarded(|area| {
                area.paste();
            }),
            EditorKey::Undo => self.guarded(|area| {
                area.undo();
            }),
            EditorKey::Redo => self.guarded(|area| {
                area.redo();
            }),
            EditorKey::Left => self.area.move_cursor(CursorMove::Back),
            EditorKey::Right => self.area.move_cursor(CursorMove::Forward),
            EditorKey::Up => self.area.move_cursor(CursorMove::Up),
            EditorKey::Down => self.area.move_cursor(CursorMove::Down),
            EditorKey::Home => self.area.move_cursor(CursorMove::Head),
            EditorKey::End => self.area.move_cursor(CursorMove::End),
            EditorKey::PageUp => self.area.scroll(Scrolling::PageUp),
            EditorKey::PageDown => self.area.scroll(Scrolling::PageDown),
            // Handled before the mode: it never reaches here.
            EditorKey::LogOff => {}
        }
        EditorEffect::Stay
    }

    fn leaving(&mut self, key: EditorKey) -> EditorEffect {
        match key {
            EditorKey::Char('s' | 'S') => match self.draft() {
                Some(save) => EditorEffect::SaveDraft(save),
                // No context: there is nowhere to keep a draft, and the prompt did not offer it.
                None => EditorEffect::Stay,
            },
            EditorKey::Char('d' | 'D') => EditorEffect::Done(Outcome::Cancelled),
            // Anything else (c, Esc, a stray key) keeps the text: only an explicit choice
            // discards it.
            _ => {
                self.mode = Mode::Editing;
                EditorEffect::Stay
            }
        }
    }

    fn leave(&mut self) -> EditorEffect {
        if self.is_changed() {
            self.mode = Mode::Leaving;
            EditorEffect::Stay
        } else {
            EditorEffect::Done(Outcome::Cancelled)
        }
    }

    fn send(&mut self) -> EditorEffect {
        match Body::prepare(self.limits, &self.text()) {
            Ok(body) if body.is_blank() => {
                self.message = Some(Message::problem(strings::NOTHING_TO_SEND));
                EditorEffect::Stay
            }
            Ok(body) => EditorEffect::Done(Outcome::Submitted(body)),
            Err(error) => {
                self.message = Some(Message::problem(fill(
                    strings::CANNOT_SEND,
                    &[("why", &error.to_string())],
                )));
                EditorEffect::Stay
            }
        }
    }

    /// The draft to keep, when the editor has a context key.
    fn draft(&self) -> Option<DraftSave> {
        self.context.as_ref().map(|context| DraftSave {
            context: context.clone(),
            text: self.text(),
            limits: self.limits,
        })
    }

    /// The draft to save on logoff, on a lost connection and while editing (FIX-M3-7, D134):
    /// only if there is somewhere to keep it and something new.
    #[must_use]
    pub fn pending_draft(&self) -> Option<DraftSave> {
        if self.is_changed() {
            self.draft()
        } else {
            None
        }
    }

    /// Run an edit; if it took the text past the limits, undo it and say why.
    fn guarded(&mut self, edit: impl FnOnce(&mut TextArea<'static>)) {
        edit(&mut self.area);
        let (chars, lines) = self.measure();
        let refused = if chars > self.limits.max_chars() {
            Some(fill(
                strings::AT_LIMIT_CHARS,
                &[("max", &self.limits.max_chars().to_string())],
            ))
        } else if lines > self.limits.max_lines() {
            Some(fill(
                strings::AT_LIMIT_LINES,
                &[("max", &self.limits.max_lines().to_string())],
            ))
        } else {
            None
        };
        if let Some(text) = refused {
            self.area.undo();
            self.message = Some(Message::problem(text));
        }
        self.mark_quotes();
    }

    /// Characters and lines in the text, counted the way [`Body`] counts them.
    fn measure(&self) -> (usize, usize) {
        let lines = self.area.lines();
        let chars: usize = lines.iter().map(|line| line.chars().count()).sum();
        (chars + lines.len().saturating_sub(1), lines.len())
    }

    /// Dim the quoted lines. Content gets a plain style only, never the marker style.
    fn mark_quotes(&mut self) {
        self.area.clear_custom_highlight();
        let dim = ratatui::style::Style::default().add_modifier(ratatui::style::Modifier::DIM);
        let quoted: Vec<(usize, usize)> = self
            .area
            .lines()
            .iter()
            .enumerate()
            .filter(|(_, line)| is_quote_line(line))
            .map(|(row, line)| (row, line.chars().count()))
            .collect();
        for (row, len) in quoted {
            self.area.custom_highlight(((row, 0), (row, len)), dim, 0);
        }
    }
}

#[cfg(test)]
mod tests;
