#![allow(clippy::unwrap_used, clippy::panic)]

//! The editor: its keys, limits, leave and draft flow, and what it draws.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;
use russh_bbs_core::access::Slug;
use russh_bbs_core::drafts::{
    BodyLimits, ContextKey, DraftError, DraftStore, MAX_DRAFTS_PER_MEMBER, QuoteLimits,
    SaveOutcome, discard_draft, load_draft, quote,
};
use russh_bbs_core::icons::{SecurityMarker, Tier};
use russh_bbs_core::sanitize::{
    MultiLine, RESERVED_CHROME, Sanitized, SingleLine, sanitize_username,
};
use russh_bbs_core::store::{Account, AccountState, AccountStore, NewAccount, UnixSeconds};
use russh_bbs_core::terminal_settings::TerminalSettings;
use russh_bbs_store::SqliteStore;

use super::*;

fn title() -> Sanitized {
    SingleLine::new(60).sanitize("New post")
}

fn clean(text: &str) -> Sanitized {
    MultiLine::new(20_000).sanitize(text)
}

fn key(text: &str) -> ContextKey {
    ContextKey::parse(text).unwrap()
}

fn open() -> Editor {
    Editor::open(EditorRequest::new(title()), None).unwrap()
}

fn with_context() -> Editor {
    Editor::open(
        EditorRequest::new(title()).context(key("forum:reply:1")),
        None,
    )
    .unwrap()
}

fn type_text(editor: &mut Editor, text: &str) {
    for c in text.chars() {
        let effect = if c == '\n' {
            editor.press(EditorKey::Enter)
        } else {
            editor.press(EditorKey::Char(c))
        };
        assert_eq!(effect, EditorEffect::Stay);
    }
}

// ---- keys ----

#[test]
fn terminal_keys_map_to_editor_keys() {
    let map = EditorKey::from_terminal;
    assert_eq!(map(Key::Char('a')), Some(EditorKey::Char('a')));
    assert_eq!(
        map(Key::Char('?')),
        Some(EditorKey::Char('?')),
        "? is text here"
    );
    assert_eq!(map(Key::Enter), Some(EditorKey::Enter));
    assert_eq!(map(Key::Backspace), Some(EditorKey::Backspace));
    assert_eq!(map(Key::Escape), Some(EditorKey::Leave));
    assert_eq!(map(Key::Ctrl('i')), Some(EditorKey::Tab));
    assert_eq!(map(Key::Ctrl('c')), Some(EditorKey::LogOff));
    assert_eq!(map(Key::Ctrl('d')), Some(EditorKey::LogOff));
    assert_eq!(map(Key::Ctrl('x')), Some(EditorKey::Send));
    assert_eq!(map(Key::Ctrl('g')), Some(EditorKey::Help));
    assert_eq!(map(Key::Other), None);
    assert_eq!(map(Key::Ctrl('q')), None);
    assert_eq!(map(Key::Ctrl('s')), None, "XOFF is never a command");
}

#[test]
fn every_ctrl_chord_in_the_help_text_is_a_real_key() {
    for (chord, _) in strings::HELP_KEYS {
        for part in chord.split(", ").filter_map(|p| p.strip_prefix("Ctrl-")) {
            let letter = part.to_ascii_lowercase().chars().next().unwrap();
            assert!(
                EditorKey::from_terminal(Key::Ctrl(letter)).is_some(),
                "{chord} is listed but does nothing"
            );
        }
    }
    // And no mapped chord is missing from the list.
    let listed: String = strings::HELP_KEYS.iter().map(|(c, _)| *c).collect();
    for letter in 'a'..='z' {
        if EditorKey::from_terminal(Key::Ctrl(letter)).is_some() {
            let name = format!("Ctrl-{}", letter.to_ascii_uppercase());
            let tab = letter == 'i';
            assert!(listed.contains(&name) || tab, "{name} is not documented");
        }
    }
}

// ---- editing ----

#[test]
fn typing_enter_and_backspace_edit_the_text() {
    let mut editor = open();
    type_text(&mut editor, "hello\nworld");
    assert_eq!(editor.text(), "hello\nworld");
    assert_eq!(editor.cursor(), (1, 5));
    editor.press(EditorKey::Backspace);
    assert_eq!(editor.text(), "hello\nworl");
    editor.press(EditorKey::Home);
    editor.press(EditorKey::Backspace);
    assert_eq!(
        editor.text(),
        "helloworl",
        "backspace at a line start joins lines"
    );
    editor.press(EditorKey::Undo);
    assert_eq!(editor.text(), "hello\nworl");
    editor.press(EditorKey::Redo);
    assert_eq!(editor.text(), "helloworl");
}

#[test]
fn movement_kill_and_yank_work() {
    let mut editor = open();
    type_text(&mut editor, "one two\nthree");
    editor.press(EditorKey::Up);
    editor.press(EditorKey::Home);
    editor.press(EditorKey::Right);
    editor.press(EditorKey::Right);
    editor.press(EditorKey::Right);
    editor.press(EditorKey::KillLine);
    assert_eq!(editor.text(), "one\nthree");
    editor.press(EditorKey::Down);
    editor.press(EditorKey::End);
    editor.press(EditorKey::Yank);
    assert_eq!(editor.text(), "one\nthree two");
    editor.press(EditorKey::DeleteWord);
    assert_eq!(editor.text(), "one\nthree ");
    editor.press(EditorKey::Left);
    editor.press(EditorKey::Delete);
    assert_eq!(editor.text(), "one\nthree");
    editor.press(EditorKey::Tab);
    assert_eq!(
        editor.text(),
        "one\nthree   ",
        "to the next stop at column 8"
    );
}

#[test]
fn keys_the_editor_does_not_use_do_nothing() {
    let mut editor = open();
    for key in [Key::Other, Key::Ctrl('q'), Key::Ctrl('s'), Key::Ctrl('o')] {
        assert_eq!(editor.input(key), EditorEffect::Stay);
    }
    assert_eq!(editor.text(), "");
    assert!(!editor.is_changed());
}

// ---- limits ----

#[test]
fn typing_stops_at_the_character_limit_and_says_so() {
    let limits = BodyLimits::new(10, 5);
    let mut editor = Editor::open(EditorRequest::new(title()).limits(limits), None).unwrap();
    type_text(&mut editor, "0123456789");
    editor.press(EditorKey::Char('x'));
    assert_eq!(editor.text(), "0123456789");
    assert!(
        editor
            .message
            .as_ref()
            .unwrap()
            .text
            .contains("at most 10 characters"),
        "{:?}",
        editor.message
    );
    // Deleting makes room again.
    editor.press(EditorKey::Backspace);
    editor.press(EditorKey::Char('x'));
    assert_eq!(editor.text(), "012345678x");
    assert_eq!(editor.message, None);
}

#[test]
fn enter_stops_at_the_line_limit_and_a_newline_counts_as_a_character() {
    let mut editor = Editor::open(
        EditorRequest::new(title()).limits(BodyLimits::new(100, 3)),
        None,
    )
    .unwrap();
    type_text(&mut editor, "a\nb\nc");
    editor.press(EditorKey::Enter);
    assert_eq!(editor.text(), "a\nb\nc");
    assert!(
        editor
            .message
            .as_ref()
            .unwrap()
            .text
            .contains("at most 3 lines")
    );
    let mut tight = Editor::open(
        EditorRequest::new(title()).limits(BodyLimits::new(3, 10)),
        None,
    )
    .unwrap();
    type_text(&mut tight, "a\nb");
    tight.press(EditorKey::Enter);
    assert_eq!(tight.text(), "a\nb", "the newline is the fourth character");
}

#[test]
fn yank_and_redo_cannot_pass_the_limit_either() {
    let mut editor = Editor::open(
        EditorRequest::new(title()).limits(BodyLimits::new(10, 5)),
        None,
    )
    .unwrap();
    type_text(&mut editor, "abcdefghij");
    editor.press(EditorKey::Home);
    editor.press(EditorKey::KillLine);
    assert_eq!(editor.text(), "");
    type_text(&mut editor, "xyz");
    editor.press(EditorKey::Yank);
    assert_eq!(editor.text(), "xyz", "13 characters would not fit");
    assert!(editor.message.is_some());
    // Undo twice walks back; redo cannot push past the limit either.
    for _ in 0..8 {
        editor.press(EditorKey::Redo);
        let (chars, _) = editor.measure();
        assert!(chars <= 10, "{chars}");
    }
}

#[test]
fn a_starting_text_over_the_limit_is_refused_not_cut() {
    let request = EditorRequest::new(title())
        .initial(clean("0123456789"))
        .limits(BodyLimits::new(5, 5));
    assert_eq!(
        Editor::open(request, None).unwrap_err(),
        BodyError::TooLong { max: 5 }
    );
}

// ---- send, leave, drafts ----

#[test]
fn send_returns_the_sanitized_body_and_refuses_blank_text() {
    let mut editor = open();
    assert_eq!(editor.press(EditorKey::Send), EditorEffect::Stay);
    assert!(
        editor
            .message
            .as_ref()
            .unwrap()
            .text
            .contains("nothing to send")
    );
    type_text(&mut editor, "  \n ");
    assert_eq!(
        editor.press(EditorKey::Send),
        EditorEffect::Stay,
        "whitespace only"
    );
    type_text(&mut editor, "hello  \n\n");
    match editor.press(EditorKey::Send) {
        EditorEffect::Done(Outcome::Submitted(body)) => {
            assert!(body.as_str().contains("hello"));
            assert_eq!(body.trimmed().trim(), "hello");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn leaving_an_untouched_editor_cancels_at_once() {
    let mut editor = open();
    assert_eq!(
        editor.input(Key::Escape),
        EditorEffect::Done(Outcome::Cancelled)
    );
    let mut quoted =
        Editor::open(EditorRequest::new(title()).initial(clean("> hi\n\n")), None).unwrap();
    assert_eq!(
        quoted.input(Key::Escape),
        EditorEffect::Done(Outcome::Cancelled)
    );
}

#[test]
fn leaving_with_changes_asks_and_only_an_explicit_choice_discards() {
    let mut editor = with_context();
    type_text(&mut editor, "draft text");
    assert_eq!(editor.input(Key::Escape), EditorEffect::Stay);
    assert_eq!(editor.mode, Mode::Leaving);
    // A key the editor ignores (an arrow, today) leaves the prompt up.
    assert_eq!(editor.input(Key::Other), EditorEffect::Stay);
    assert_eq!(editor.mode, Mode::Leaving);
    // Esc again, a stray key, or c: back to editing with the text intact.
    for stray in [Key::Escape, Key::Char('z'), Key::Char('c'), Key::Enter] {
        assert_eq!(editor.input(stray), EditorEffect::Stay);
        assert_eq!(editor.text(), "draft text");
        assert_eq!(editor.mode, Mode::Editing, "{stray:?}");
        editor.input(Key::Escape);
    }
    match editor.input(Key::Char('s')) {
        EditorEffect::SaveDraft(save) => {
            assert_eq!(save.context(), &key("forum:reply:1"));
            assert_eq!(save.text(), "draft text");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        editor.input(Key::Char('d')),
        EditorEffect::Done(Outcome::Cancelled)
    );
}

#[test]
fn without_a_context_there_is_nowhere_to_save_a_draft() {
    let mut editor = open();
    type_text(&mut editor, "text");
    editor.input(Key::Escape);
    assert_eq!(editor.input(Key::Char('s')), EditorEffect::Stay);
    assert_eq!(editor.mode, Mode::Leaving, "the prompt stays up");
    assert_eq!(editor.input(Key::Ctrl('c')), EditorEffect::LogOff(None));
    assert_eq!(
        editor.input(Key::Char('d')),
        EditorEffect::Done(Outcome::Cancelled)
    );
}

#[test]
fn logging_off_hands_over_the_changed_draft_only() {
    let mut editor = with_context();
    assert_eq!(
        editor.input(Key::Ctrl('d')),
        EditorEffect::LogOff(None),
        "unchanged"
    );
    type_text(&mut editor, "unsent");
    match editor.input(Key::Ctrl('c')) {
        EditorEffect::LogOff(Some(save)) => assert_eq!(save.text(), "unsent"),
        other => panic!("{other:?}"),
    }
    // From the leave prompt and the key list too.
    editor.input(Key::Escape);
    assert!(matches!(
        editor.input(Key::Ctrl('d')),
        EditorEffect::LogOff(Some(_))
    ));
    let mut helping = with_context();
    type_text(&mut helping, "x");
    helping.input(Key::Ctrl('g'));
    assert!(matches!(
        helping.input(Key::Ctrl('c')),
        EditorEffect::LogOff(Some(_))
    ));
}

#[test]
fn a_refused_draft_returns_to_editing_with_the_reason() {
    let mut editor = with_context();
    type_text(&mut editor, "keep me");
    editor.input(Key::Escape);
    editor.input(Key::Char('s'));
    assert_eq!(
        editor.draft_saved(Err(DraftError::TooMany)),
        EditorEffect::Stay
    );
    assert_eq!(editor.mode, Mode::Editing);
    assert_eq!(editor.text(), "keep me");
    let shown = &editor.message.as_ref().unwrap().text;
    assert!(
        shown.contains("not saved") && shown.contains("discard one first"),
        "{shown}"
    );
    assert_eq!(
        editor.draft_saved(Ok(SaveOutcome::Created)),
        EditorEffect::Done(Outcome::SavedDraft)
    );
}

#[test]
fn help_lists_the_keys_and_any_key_returns() {
    let mut editor = open();
    type_text(&mut editor, "text");
    assert_eq!(editor.input(Key::Ctrl('g')), EditorEffect::Stay);
    assert_eq!(editor.mode, Mode::Help);
    assert_eq!(editor.input(Key::Char('z')), EditorEffect::Stay);
    assert_eq!(editor.mode, Mode::Editing);
    assert_eq!(
        editor.text(),
        "text",
        "the key that closed help typed nothing"
    );
}

// ---- drafts through a real store ----

async fn store_with_member() -> (tempfile::TempDir, SqliteStore, Account) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
    let account = store
        .create_account(NewAccount {
            username: sanitize_username("ann").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    (dir, store, account)
}

const NOW: UnixSeconds = UnixSeconds::from_raw(5_000);

#[tokio::test]
async fn a_draft_saved_from_the_editor_is_resumed_by_the_next_one() {
    let (_dir, store, ann) = store_with_member().await;
    let request = || EditorRequest::new(title()).context(key("forum:reply:7"));
    let mut editor = Editor::open_with_store(&store, ann.id, request())
        .await
        .unwrap();
    assert!(editor.message.is_none(), "no draft yet");
    type_text(&mut editor, "half a thought\nand more");
    editor.input(Key::Escape);
    let EditorEffect::SaveDraft(save) = editor.input(Key::Char('s')) else {
        panic!("expected a draft to save");
    };
    let saved = save.persist(&store, ann.id, NOW).await;
    assert_eq!(
        editor.draft_saved(saved),
        EditorEffect::Done(Outcome::SavedDraft)
    );

    let again = Editor::open_with_store(&store, ann.id, request())
        .await
        .unwrap();
    assert_eq!(again.text(), "half a thought\nand more");
    assert_eq!(again.cursor(), (1, 8), "the cursor starts at the end");
    assert!(again.message.as_ref().unwrap().text.contains("Resumed"));
    assert!(
        !again.is_changed(),
        "leaving an untouched resumed draft asks nothing"
    );
    // The draft is still stored until the caller's own write succeeds.
    assert!(
        load_draft(&store, ann.id, &key("forum:reply:7"))
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        discard_draft(&store, ann.id, &key("forum:reply:7"))
            .await
            .unwrap()
    );
    let fresh = Editor::open_with_store(&store, ann.id, request())
        .await
        .unwrap();
    assert_eq!(fresh.text(), "");
}

#[tokio::test]
async fn a_saved_draft_wins_over_the_starting_text_and_the_editor_says_so() {
    let (_dir, store, ann) = store_with_member().await;
    let who = SingleLine::new(40).sanitize("bob wrote:");
    let quoted = quote(Some(&who), &clean("original post"), QuoteLimits::default());
    let request = || {
        EditorRequest::new(title())
            .context(key("forum:reply:8"))
            .initial(quoted.clone())
    };
    let fresh = Editor::open_with_store(&store, ann.id, request())
        .await
        .unwrap();
    assert_eq!(fresh.text(), "bob wrote:\n> original post\n\n");
    assert_eq!(fresh.cursor(), (3, 0), "the cursor waits below the quote");
    let mut mine = fresh;
    type_text(&mut mine, "my reply");
    let EditorEffect::LogOff(Some(save)) = mine.input(Key::Ctrl('c')) else {
        panic!("a changed editor with a context hands over its draft on logoff");
    };
    save.persist(&store, ann.id, NOW).await.unwrap();
    let resumed = Editor::open_with_store(&store, ann.id, request())
        .await
        .unwrap();
    assert_eq!(resumed.text(), "bob wrote:\n> original post\n\nmy reply");
    assert!(
        resumed
            .message
            .as_ref()
            .unwrap()
            .text
            .contains("instead of the starting text")
    );
}

#[tokio::test]
async fn the_draft_cap_shows_up_as_a_problem_and_loses_nothing() {
    let (_dir, store, ann) = store_with_member().await;
    for n in 0..MAX_DRAFTS_PER_MEMBER {
        store
            .put_draft(
                ann.id,
                &key(&format!("full:{n}")),
                &russh_bbs_core::drafts::Body::prepare(BodyLimits::MAX, "x").unwrap(),
                NOW,
                MAX_DRAFTS_PER_MEMBER,
            )
            .await
            .unwrap();
    }
    let request = EditorRequest::new(title()).context(key("one:too:many"));
    let mut editor = Editor::open_with_store(&store, ann.id, request)
        .await
        .unwrap();
    type_text(&mut editor, "important");
    editor.input(Key::Escape);
    let EditorEffect::SaveDraft(save) = editor.input(Key::Char('s')) else {
        panic!("expected a draft to save");
    };
    let result = save.persist(&store, ann.id, NOW).await;
    assert!(matches!(result, Err(DraftError::TooMany)));
    assert_eq!(editor.draft_saved(result), EditorEffect::Stay);
    assert_eq!(editor.text(), "important");
    assert!(
        editor
            .message
            .as_ref()
            .unwrap()
            .text
            .contains("discard one first")
    );
}

#[tokio::test]
async fn clearing_a_resumed_draft_and_saving_removes_it() {
    let (_dir, store, ann) = store_with_member().await;
    let request = || EditorRequest::new(title()).context(key("forum:reply:9"));
    let mut editor = Editor::open_with_store(&store, ann.id, request())
        .await
        .unwrap();
    type_text(&mut editor, "going away");
    let EditorEffect::LogOff(Some(save)) = editor.input(Key::Ctrl('d')) else {
        panic!("draft expected");
    };
    save.persist(&store, ann.id, NOW).await.unwrap();
    let mut editor = Editor::open_with_store(&store, ann.id, request())
        .await
        .unwrap();
    for _ in 0.."going away".len() {
        editor.press(EditorKey::Backspace);
    }
    editor.input(Key::Escape);
    let EditorEffect::SaveDraft(save) = editor.input(Key::Char('s')) else {
        panic!("draft expected");
    };
    assert_eq!(
        save.persist(&store, ann.id, NOW).await.unwrap(),
        SaveOutcome::Removed
    );
    assert!(
        load_draft(&store, ann.id, &key("forum:reply:9"))
            .await
            .unwrap()
            .is_none()
    );
}

// ---- drawing ----

fn settings(tier: Option<Tier>, color: bool) -> TerminalSettings {
    let base = TerminalSettings::default().with_color(color);
    match tier {
        Some(tier) => base.with_tier(tier),
        None => base,
    }
}

fn draw(editor: &Editor, size: (u16, u16), settings: &TerminalSettings) -> Buffer {
    let ui = Ui::standard().unwrap();
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            editor.render(
                frame,
                &EditorView {
                    ui: &ui,
                    settings,
                    markers: &[SecurityMarker::ServerReadable],
                },
            );
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect::<String>()
}

/// The words on a screen with the box drawing removed, so text a row break split reads whole.
fn words(shown: &str) -> String {
    shown
        .replace(['|', '+'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn screen(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| row(buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_editor_draws_title_text_counters_and_keys() {
    let mut editor = open();
    type_text(&mut editor, "first line\nsecond line");
    let buffer = draw(&editor, (80, 24), &settings(None, false));
    let shown = screen(&buffer);
    assert!(shown.contains("New post"), "{shown}");
    assert!(shown.contains("first line") && shown.contains("second line"));
    assert!(shown.contains("Lines 2/400"), "{shown}");
    assert!(shown.contains("Characters 22/16000"));
    assert!(
        shown.contains("Ctrl-X send")
            && shown.contains("Esc leave")
            && shown.contains("Ctrl-G keys")
    );
    assert!(
        row(&buffer, 0).starts_with('+'),
        "ASCII border before the glyph test"
    );
    let unicode = draw(&editor, (80, 24), &settings(Some(Tier::Unicode), true));
    assert!(!row(&unicode, 0).starts_with('+'));
}

#[test]
fn the_leave_prompt_and_the_key_list_are_drawn_in_words() {
    let mut editor = with_context();
    type_text(&mut editor, "x");
    editor.input(Key::Escape);
    let shown = screen(&draw(&editor, (100, 24), &settings(None, false)));
    assert!(
        shown.contains("[s] save draft")
            && shown.contains("[d] discard changes")
            && shown.contains("[c] keep editing")
    );
    let mut plain = open();
    type_text(&mut plain, "x");
    plain.input(Key::Escape);
    let shown = screen(&draw(&plain, (100, 24), &settings(None, false)));
    assert!(!shown.contains("save draft"), "no draft without a context");
    assert!(shown.contains("[d] discard changes"));
    plain.input(Key::Char('c'));
    plain.input(Key::Ctrl('g'));
    let shown = screen(&draw(&plain, (100, 40), &settings(None, false)));
    for (chord, _) in strings::HELP_KEYS {
        assert!(shown.contains(chord), "{chord} missing from the key list");
    }
}

#[test]
fn notices_carry_a_word_not_only_a_color() {
    let mut editor = Editor::open(
        EditorRequest::new(title()).limits(BodyLimits::new(3, 3)),
        None,
    )
    .unwrap();
    type_text(&mut editor, "abc");
    editor.press(EditorKey::Char('d'));
    let shown = screen(&draw(&editor, (80, 24), &settings(None, false)));
    assert!(shown.contains("Problem: Limit reached"), "{shown}");
}

#[test]
fn long_text_scrolls_to_keep_the_cursor_on_screen() {
    let mut editor = open();
    for n in 1..=150 {
        type_text(&mut editor, &format!("line number {n}\n"));
    }
    type_text(&mut editor, "the very end");
    let shown = screen(&draw(&editor, (80, 24), &settings(None, false)));
    assert!(shown.contains("the very end"), "{shown}");
    assert!(
        !shown.contains("line number 1 "),
        "the top has scrolled away"
    );
    // Going back up scrolls back.
    for _ in 0..3 {
        editor.press(EditorKey::PageUp);
        draw(&editor, (80, 24), &settings(None, false));
    }
    for _ in 0..200 {
        editor.press(EditorKey::Up);
        draw(&editor, (80, 24), &settings(None, false));
    }
    let shown = screen(&draw(&editor, (80, 24), &settings(None, false)));
    assert!(
        shown.contains("line number 1\n") || shown.contains("line number 1 "),
        "{shown}"
    );
    assert!(!shown.contains("the very end"));
}

#[test]
fn a_long_line_wraps_and_a_resize_keeps_the_text_and_the_cursor() {
    let mut editor = open();
    let words = "the quick brown fox jumps over the lazy dog ".repeat(6);
    type_text(&mut editor, &words);
    type_text(&mut editor, "\nsecond");
    editor.press(EditorKey::Up);
    editor.press(EditorKey::Home);
    let (before_text, before_cursor) = (editor.text(), editor.cursor());
    for size in [(80, 24), (30, 10), (200, 60), (80, 24)] {
        let buffer = draw(&editor, size, &settings(None, false));
        assert_eq!(editor.text(), before_text, "{size:?}");
        assert_eq!(editor.cursor(), before_cursor, "{size:?}");
        let shown = screen(&buffer);
        assert!(shown.contains("quick"), "{size:?}\n{shown}");
    }
    // Typing after the resize lands where the cursor was.
    editor.press(EditorKey::Char('!'));
    assert!(editor.text().contains("!the quick"));
}

#[test]
fn wide_characters_wrap_without_overflowing_a_row() {
    // Wide (CJK, full-width) and emoji text can arrive as a starting text or a draft; it must
    // wrap on display width, never on character count (D48).
    let wide = "全角文字のテスト。".repeat(12);
    let emoji = "😀🎉👍🏽 ok ".repeat(12);
    let request = EditorRequest::new(title()).initial(clean(&format!("{wide}\n{emoji}")));
    let editor = Editor::open(request, None).unwrap();
    for width in [28_u16, 31, 40, 79, 80] {
        let buffer = draw(&editor, (width, 24), &settings(None, false));
        // Inside the box every row ends at the border: nothing ran past it.
        for y in 1..buffer.area.height.saturating_sub(3) {
            assert_eq!(
                buffer[(width - 1, y)].symbol(),
                "|",
                "row {y} at {width}: {:?}",
                row(&buffer, y)
            );
        }
        // Every wide character is on screen somewhere (nothing clipped away by the wrap).
        let shown: String = screen(&buffer)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let want: Vec<char> = wide.chars().collect();
        let visible = want.iter().filter(|c| shown.contains(**c)).count();
        assert_eq!(visible, want.len(), "width {width}");
    }
}

#[test]
fn a_window_too_small_for_the_editor_says_so_and_keeps_the_text() {
    let mut editor = open();
    type_text(&mut editor, "precious");
    for size in [(1, 1), (5, 3), (20, 5), (23, 24), (80, 6)] {
        let buffer = draw(&editor, size, &settings(None, false));
        assert_eq!(editor.text(), "precious");
        if size == (23, 24) || size == (80, 6) {
            let shown = screen(&buffer);
            assert!(
                words(&shown).contains("too small") || shown.contains("precious"),
                "{size:?}\n{shown}"
            );
        }
    }
    let shown = screen(&draw(&editor, (30, 5), &settings(None, false)));
    assert!(words(&shown).contains("too small"), "{shown}");
}

#[test]
fn quote_lines_are_dim_and_nothing_uses_the_marker_style() {
    let who = SingleLine::new(40).sanitize("bob wrote:");
    let quoted = quote(Some(&who), &clean("said this"), QuoteLimits::default());
    let mut editor = Editor::open(EditorRequest::new(title()).initial(quoted), None).unwrap();
    type_text(&mut editor, "reply");
    let buffer = draw(&editor, (80, 24), &settings(Some(Tier::Unicode), true));
    let quote_row = (0..24)
        .find(|&y| row(&buffer, y).contains("> said this"))
        .unwrap();
    let quote_x = row(&buffer, quote_row).find("> said").unwrap();
    assert!(
        buffer[(u16::try_from(quote_x).unwrap() + 3, quote_row)]
            .modifier
            .contains(Modifier::DIM)
    );
    let reply_row = (0..24)
        .find(|&y| row(&buffer, y).contains("reply"))
        .unwrap();
    let reply_x = row(&buffer, reply_row).find("reply").unwrap();
    assert!(
        !buffer[(u16::try_from(reply_x).unwrap(), reply_row)]
            .modifier
            .contains(Modifier::DIM)
    );
    // The reserved style is bold + reverse. It appears only in the status row.
    let height = buffer.area.height;
    for y in 0..height - 1 {
        for x in 0..buffer.area.width {
            let m = buffer[(x, y)].modifier;
            assert!(
                !(m.contains(Modifier::BOLD) && m.contains(Modifier::REVERSED)),
                "reserved style on content at ({x},{y})"
            );
        }
    }
}

#[test]
fn hostile_text_cannot_reach_the_status_row_or_wear_the_marker_style() {
    // Text that tries to look like chrome: the lookalikes the sanitizer allows, ASCII words
    // that name a marker, a huge line and many lines. The reserved characters themselves were
    // replaced before the editor ever saw them.
    let attempt = format!(
        "〚⊙〛 ◉ SERVER-READABLE E2EE SIGNED KEY-CHANGED \u{27E6}\u{2299}\u{27E7}\n{}\n{}",
        "x".repeat(2_000),
        "[unavailable]\n".repeat(150)
    );
    let initial = clean(&attempt);
    assert!(
        !initial
            .as_str()
            .chars()
            .any(|c| RESERVED_CHROME.contains(&c))
    );
    let mut editor = Editor::open(EditorRequest::new(title()).initial(initial), None).unwrap();
    type_text(&mut editor, "typed");
    for tier in [None, Some(Tier::Unicode), Some(Tier::NerdFont)] {
        for color in [false, true] {
            for size in [(80, 24), (24, 6), (200, 60), (40, 8), (3, 2), (1, 1)] {
                let s = settings(tier, color);
                let buffer = draw(&editor, size, &s);
                let height = buffer.area.height;
                // The empty editor's status row is the reference.
                let empty = draw(&open(), size, &s);
                if height >= 2 {
                    assert_eq!(
                        row(&buffer, height - 1),
                        row(&empty, height - 1),
                        "{size:?}"
                    );
                    for x in 0..buffer.area.width {
                        assert_eq!(
                            buffer[(x, height - 1)].modifier,
                            empty[(x, height - 1)].modifier,
                            "status row style at {x}, {size:?}"
                        );
                    }
                }
                for y in 0..height.saturating_sub(1) {
                    for x in 0..buffer.area.width {
                        let m = buffer[(x, y)].modifier;
                        assert!(
                            !(m.contains(Modifier::BOLD) && m.contains(Modifier::REVERSED)),
                            "marker style on content at ({x},{y}) {size:?} {tier:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn the_server_readable_marker_is_drawn_only_in_the_reserved_row() {
    let editor = open();
    let buffer = draw(&editor, (80, 24), &settings(None, false));
    let last = row(&buffer, 23);
    assert!(last.contains("SERVER-READABLE"), "{last:?}");
    for y in 0..23 {
        assert!(!row(&buffer, y).contains("SERVER-READABLE"), "row {y}");
    }
    let m = buffer[(0, 23)].modifier;
    assert!(m.contains(Modifier::BOLD) && m.contains(Modifier::REVERSED));
    // With no markers given there is none: the editor adds none of its own.
    let ui = Ui::standard().unwrap();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let s = settings(None, false);
    terminal
        .draw(|frame| {
            editor.render(
                frame,
                &EditorView {
                    ui: &ui,
                    settings: &s,
                    markers: &[],
                },
            );
        })
        .unwrap();
    assert!(!row(terminal.backend().buffer(), 23).contains("SERVER-READABLE"));
}

#[test]
fn nothing_on_the_editor_screen_calls_anything_private() {
    let mut editor = with_context();
    type_text(&mut editor, "x");
    editor.input(Key::Escape);
    let mut all = screen(&draw(
        &editor,
        (120, 40),
        &settings(Some(Tier::Unicode), true),
    ));
    editor.input(Key::Char('c'));
    editor.input(Key::Ctrl('g'));
    all.push_str(&screen(&draw(&editor, (120, 60), &settings(None, false))));
    for text in strings::HELP_KEYS.iter().flat_map(|(a, b)| [*a, *b]) {
        all.push_str(text);
    }
    let lower = all.to_lowercase();
    for word in ["private", "secure", "encrypt", "lock", "confidential"] {
        assert!(!lower.contains(word), "{word}");
    }
}

#[test]
fn a_message_at_the_board_wide_cap_can_be_typed_drawn_and_sent() {
    // 399 lines of 39 characters, one of 40, and 399 newlines: exactly 16,000 characters.
    let mut editor = open();
    for n in 0..400 {
        type_text(&mut editor, &"x".repeat(if n == 399 { 40 } else { 39 }));
        if n < 399 {
            editor.press(EditorKey::Enter);
        }
    }
    assert_eq!(editor.measure(), (16_000, 400));
    // One more of either is refused, and the text is unchanged.
    editor.press(EditorKey::Char('y'));
    editor.press(EditorKey::Enter);
    assert_eq!(editor.measure(), (16_000, 400));
    let shown = screen(&draw(&editor, (120, 40), &settings(None, false)));
    assert!(shown.contains("Lines 400/400") && shown.contains("Characters 16000/16000"));
    match editor.press(EditorKey::Send) {
        EditorEffect::Done(Outcome::Submitted(body)) => assert_eq!(body.chars(), 16_000),
        other => panic!("{other:?}"),
    }
}

#[test]
fn up_and_down_move_by_screen_row_inside_a_wrapped_line() {
    let mut editor = open();
    let long = format!("{} end", "word ".repeat(40));
    type_text(&mut editor, &long);
    type_text(&mut editor, "\nnext line");
    // The text area learns its width when it is drawn.
    draw(&editor, (80, 24), &settings(None, false));
    editor.press(EditorKey::Up);
    let (line, column) = editor.cursor();
    assert_eq!(
        line, 0,
        "still inside the long line, one screen row up from its last row"
    );
    assert!(column > 0);
    editor.press(EditorKey::Up);
    let (line_again, column_again) = editor.cursor();
    assert_eq!(line_again, 0);
    assert!(
        column_again < column,
        "moved up another screen row: {column_again} < {column}"
    );
}
