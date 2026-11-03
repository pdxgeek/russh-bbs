#![allow(clippy::unwrap_used, clippy::panic)]

//! The forum screens as a state machine, and what they draw. Store behaviour is in core's
//! shared suite; the full path over a real `ssh` client is `tests/forums.rs`.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use russh_bbs_core::drafts::{Body, DraftError};
use russh_bbs_core::forums::{
    BoardId, BoardSummary, Hit, PostId, PostView, SearchPage, ThreadId, ThreadPage, ThreadSummary,
    ThreadView, body_limits,
};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::MultiLine;
use russh_bbs_core::store::UnixSeconds;
use russh_bbs_core::terminal_settings::TerminalSettings;

use super::exec::{BoardList, Fail, Purpose, Request, Response};
use super::flow::{ForumEffect, ForumsScreen};
use super::{DrawCtx, render};
use crate::settings::Look;
use crate::ssh::terminal::Key;
use crate::ui::Ui;
use crate::ui::editor::{Editor, EditorRequest};

const AREA: Rect = Rect::new(0, 0, 80, 24);

fn settings() -> TerminalSettings {
    TerminalSettings::default().with_pause(true)
}

fn board(id: i64, name: &str) -> BoardSummary {
    BoardSummary {
        id: BoardId::from_raw(id),
        name: name.to_owned(),
        description: "About".to_owned(),
        restricted: false,
        feed: false,
        threads: 2,
    }
}

fn thread(id: i64, subject: &str) -> ThreadSummary {
    ThreadSummary {
        id: ThreadId::from_raw(id),
        board_name: "General".to_owned(),
        subject: subject.to_owned(),
        author: Some("ann".to_owned()),
        posts: 3,
        last_post_at: UnixSeconds::from_raw(1_700_000_000),
    }
}

fn view(index: u64, total: u64, body: &str, moderate: bool) -> ThreadView {
    ThreadView {
        thread: thread(7, "Subject line"),
        board: BoardId::from_raw(1),
        index,
        total,
        post: PostView {
            id: PostId::from_raw(40 + i64::try_from(index).unwrap()),
            author: Some("ann".to_owned()),
            at: UnixSeconds::from_raw(1_700_000_000),
            body: MultiLine::new(8_000).sanitize(body),
        },
        can_reply: true,
        can_moderate: moderate,
    }
}

fn boards(manage: bool) -> Response {
    Response::Boards(Ok(BoardList {
        boards: vec![board(1, "General"), board(2, "Tech")],
        can_manage: manage,
        can_feeds: false,
    }))
}

fn open(manage: bool) -> ForumsScreen {
    let (mut screen, first) = ForumsScreen::open(settings());
    assert!(matches!(first, Request::Boards));
    assert!(screen.deliver(boards(manage)).is_none());
    screen
}

fn press(screen: &mut ForumsScreen, key: Key) -> ForumEffect {
    screen.input(key, AREA)
}

fn text(screen: &ForumsScreen) -> String {
    let buffer = draw(screen, AREA);
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn draw(screen: &ForumsScreen, area: Rect) -> Buffer {
    let ui = Ui::standard().unwrap();
    let settings = settings();
    let ctx = DrawCtx {
        ui: &ui,
        strings: ui.strings(),
        settings: &settings,
        look: Look::of(&settings),
        tier: Tier::Ascii,
    };
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal.draw(|frame| render(frame, screen, &ctx)).unwrap();
    terminal.backend().buffer().clone()
}

fn expect_request(effect: ForumEffect) -> Request {
    match effect {
        ForumEffect::Request(request) => request,
        other => panic!("expected a request, got {other:?}"),
    }
}

fn into_reader(screen: &mut ForumsScreen, view: ThreadView) {
    screen.deliver(Response::Threads(Ok(ThreadPage {
        board: board(1, "General"),
        threads: vec![thread(7, "Subject line")],
        page: 0,
        more: false,
        can_post: true,
        can_manage: false,
        can_feeds: false,
    })));
    // Boards -> open General -> a threads page -> open the thread.
    let request = expect_request(press(screen, Key::Enter));
    assert!(matches!(request, Request::Threads { .. }));
    screen.deliver(Response::Threads(Ok(ThreadPage {
        board: board(1, "General"),
        threads: vec![thread(7, "Subject line")],
        page: 0,
        more: false,
        can_post: true,
        can_manage: false,
        can_feeds: false,
    })));
    let request = expect_request(press(screen, Key::Enter));
    assert!(matches!(request, Request::Post { index: 0, .. }));
    screen.deliver(Response::Post(Ok(view)));
}

#[test]
fn keys_are_ignored_while_a_request_is_pending_except_logoff() {
    let (mut screen, _) = ForumsScreen::open(settings());
    assert!(matches!(press(&mut screen, Key::Enter), ForumEffect::Stay));
    assert!(matches!(
        press(&mut screen, Key::Ctrl('c')),
        ForumEffect::LogOff(None)
    ));
}

#[test]
fn the_board_list_opens_a_board_and_esc_leaves() {
    let mut screen = open(false);
    let shown = text(&screen);
    assert!(shown.contains("1. General (2 threads)"), "{shown}");
    assert!(shown.contains("2. Tech"), "{shown}");
    assert!(!shown.contains("n new board"), "{shown}");
    press(&mut screen, Key::Down);
    let request = expect_request(press(&mut screen, Key::Enter));
    assert!(
        matches!(request, Request::Threads { board, page: 0 } if board == BoardId::from_raw(2))
    );
    screen
        .deliver(Response::Threads(Err(Fail {
            text: "That is not available.".into(),
            gone: true,
            kept_draft: false,
        })))
        .map(|next| assert!(matches!(next, Request::Boards)))
        .unwrap();
    assert!(
        text(&screen).contains("Problem: That is not available."),
        "{}",
        text(&screen)
    );
    screen.deliver(boards(false));
    assert!(matches!(press(&mut screen, Key::Escape), ForumEffect::Exit));
}

#[test]
fn digits_open_a_board_and_new_board_needs_the_manage_flag() {
    let mut screen = open(false);
    assert!(matches!(
        expect_request(press(&mut screen, Key::Char('2'))),
        Request::Threads { board, .. } if board == BoardId::from_raw(2)
    ));
    let mut staff = open(true);
    assert!(text(&staff).contains("n new board"));
    press(&mut staff, Key::Char('n'));
    assert!(text(&staff).contains("Name of the new board:"));
    for c in "Rota".chars() {
        press(&mut staff, Key::Char(c));
    }
    press(&mut staff, Key::Enter);
    assert!(text(&staff).contains("What is it for?"));
    press(&mut staff, Key::Enter);
    assert!(text(&staff).contains("y = restricted"));
    let request = expect_request(press(&mut staff, Key::Char('y')));
    assert!(matches!(
        request,
        Request::CreateBoard { ref name, restricted: true, .. } if name == "Rota"
    ));
    // Name taken: back at the name prompt with the reason.
    staff.deliver(Response::Created(Err(Fail {
        text: "A board with that name already exists.".into(),
        gone: false,
        kept_draft: false,
    })));
    let shown = text(&staff);
    assert!(
        shown.contains("Name of the new board:") && shown.contains("already exists"),
        "{shown}"
    );
}

#[test]
fn an_empty_name_is_refused_on_the_spot() {
    let mut staff = open(true);
    press(&mut staff, Key::Char('n'));
    assert!(matches!(press(&mut staff, Key::Enter), ForumEffect::Stay));
    assert!(
        text(&staff).contains("That cannot be empty."),
        "{}",
        text(&staff)
    );
}

#[test]
fn search_sends_what_was_typed_and_shows_results_that_open_at_their_post() {
    let mut screen = open(false);
    press(&mut screen, Key::Char('s'));
    for c in "tomato \"x*".chars() {
        press(&mut screen, Key::Char(c));
    }
    press(&mut screen, Key::Ctrl('a'));
    let request = expect_request(press(&mut screen, Key::Enter));
    let Request::Search { query, page } = request else {
        panic!("expected a search")
    };
    assert_eq!((query.as_str(), page), ("tomato \"x*", 0));
    screen.deliver(Response::Search(Ok(SearchPage {
        hits: vec![Hit {
            post: PostId::from_raw(9),
            thread: ThreadId::from_raw(7),
            board: BoardId::from_raw(1),
            board_name: "General".into(),
            subject: "Gardening".into(),
            author: Some("ann".into()),
            index: 3,
            snippet: "tomatoes need sun".into(),
            at: UnixSeconds::from_raw(1),
        }],
        page: 0,
        more: false,
        searched: true,
    })));
    let shown = text(&screen);
    assert!(
        shown.contains("General / Gardening") && shown.contains("tomatoes need sun"),
        "{shown}"
    );
    assert!(matches!(
        expect_request(press(&mut screen, Key::Enter)),
        Request::Post { index: 3, .. }
    ));
}

#[test]
fn nothing_found_is_a_plain_line() {
    let mut screen = open(false);
    press(&mut screen, Key::Char('s'));
    press(&mut screen, Key::Char('z'));
    expect_request(press(&mut screen, Key::Enter));
    screen.deliver(Response::Search(Ok(SearchPage {
        hits: Vec::new(),
        page: 0,
        more: false,
        searched: true,
    })));
    assert!(text(&screen).contains("Nothing found."));
}

#[test]
fn the_reader_pages_a_long_post_and_every_line_is_reachable() {
    let mut screen = open(false);
    let body: String = (0..70)
        .map(|i| ["line ", &i.to_string(), "\n"].concat())
        .collect();
    into_reader(&mut screen, view(0, 1, &body, false));
    let first = text(&screen);
    assert!(first.contains("More (1 of "), "{first}");
    assert!(first.contains("Post 1 of 1 in General"), "{first}");
    assert!(first.contains("line 0"), "{first}");
    let mut seen = first;
    for _ in 0..12 {
        press(&mut screen, Key::Char(' '));
        seen.push_str(&text(&screen));
    }
    assert!(seen.contains("line 69"), "the last line is reachable");
    // Back a page and out.
    press(&mut screen, Key::Char('b'));
    let request = expect_request(press(&mut screen, Key::Escape));
    assert!(matches!(request, Request::Threads { page: 0, .. }));
}

#[test]
fn the_reader_moves_between_posts_and_offers_reply_and_quote() {
    let mut screen = open(false);
    into_reader(&mut screen, view(1, 3, "hello\nthere", false));
    assert!(matches!(
        expect_request(press(&mut screen, Key::Char('n'))),
        Request::Post { index: 2, .. }
    ));
    screen.deliver(Response::Post(Ok(view(2, 3, "last", false))));
    assert!(
        matches!(press(&mut screen, Key::Char('n')), ForumEffect::Stay),
        "no post after the last"
    );
    assert!(matches!(
        expect_request(press(&mut screen, Key::Char('p'))),
        Request::Post { index: 1, .. }
    ));
    screen.deliver(Response::Post(Ok(view(1, 3, "hello\nthere", false))));
    let Request::Compose { purpose, quote } = expect_request(press(&mut screen, Key::Char('c')))
    else {
        panic!("expected compose")
    };
    assert!(matches!(purpose, Purpose::Reply { .. }));
    let quote = quote.unwrap();
    assert!(
        quote.as_str().contains("> hello") && quote.as_str().contains("> there"),
        "{quote:?}"
    );
    screen.deliver(Response::Compose(Err(Fail {
        text: "You may not do that.".into(),
        gone: false,
        kept_draft: false,
    })));
    let Request::Compose { quote, .. } = expect_request(press(&mut screen, Key::Char('r'))) else {
        panic!("expected compose")
    };
    assert!(quote.is_none());
}

#[test]
fn removing_a_post_asks_first_and_only_y_removes() {
    let mut screen = open(false);
    into_reader(&mut screen, view(0, 2, "spam", true));
    press(&mut screen, Key::Char('x'));
    let shown = text(&screen);
    assert!(
        shown.contains("opening post") && shown.contains("whole thread"),
        "{shown}"
    );
    assert!(matches!(
        press(&mut screen, Key::Char('n')),
        ForumEffect::Stay
    ));
    assert!(!text(&screen).contains("whole thread"));
    press(&mut screen, Key::Char('x'));
    assert!(matches!(
        expect_request(press(&mut screen, Key::Char('y'))),
        Request::Remove(post) if post == PostId::from_raw(40)
    ));
    // A member without the permission is not offered the key.
    let mut member = open(false);
    into_reader(&mut member, view(0, 2, "spam", false));
    assert!(matches!(
        press(&mut member, Key::Char('x')),
        ForumEffect::Stay
    ));
}

#[test]
fn a_new_thread_asks_the_subject_then_opens_the_editor_and_posts() {
    let mut screen = open(false);
    expect_request(press(&mut screen, Key::Enter));
    screen.deliver(Response::Threads(Ok(ThreadPage {
        board: board(1, "General"),
        threads: Vec::new(),
        page: 0,
        more: false,
        can_post: true,
        can_manage: false,
        can_feeds: false,
    })));
    assert!(text(&screen).contains("No threads yet"));
    press(&mut screen, Key::Char('w'));
    for c in "Hello".chars() {
        press(&mut screen, Key::Char(c));
    }
    let Request::Compose { purpose, .. } = expect_request(press(&mut screen, Key::Enter)) else {
        panic!("expected compose")
    };
    assert_eq!(purpose.context().unwrap().as_str(), "forum:new:1");
    let editor = Editor::open(
        EditorRequest::new(
            russh_bbs_core::sanitize::SingleLine::new(40).sanitize("General: Hello"),
        ),
        None,
    )
    .unwrap();
    screen.deliver(Response::Compose(Ok(Box::new(editor))));
    for c in "my text".chars() {
        press(&mut screen, Key::Char(c));
    }
    let Request::Submit { purpose, body } = expect_request(press(&mut screen, Key::Ctrl('x')))
    else {
        panic!("expected a submit")
    };
    assert_eq!(body.as_str(), "my text");
    assert!(matches!(purpose, Purpose::Thread { ref subject, .. } if subject == "Hello"));
    // The post worked: the thread opens at its last post with a note.
    let follow = screen
        .deliver(Response::Posted(Ok(russh_bbs_core::forums::Posted {
            thread: ThreadId::from_raw(7),
            post: PostId::from_raw(40),
        })))
        .unwrap();
    assert!(matches!(
        follow,
        Request::Post {
            index: u64::MAX,
            ..
        }
    ));
    screen.deliver(Response::Post(Ok(view(0, 1, "my text", false))));
    assert!(text(&screen).contains("Note: Posted."), "{}", text(&screen));
}

#[test]
fn a_refused_post_returns_to_the_screen_with_the_reason_and_the_draft_note() {
    let mut screen = open(false);
    into_reader(&mut screen, view(0, 1, "x", false));
    expect_request(press(&mut screen, Key::Char('r')));
    let editor = Editor::open(
        EditorRequest::new(russh_bbs_core::sanitize::SingleLine::new(40).sanitize("Reply")),
        None,
    )
    .unwrap();
    screen.deliver(Response::Compose(Ok(Box::new(editor))));
    press(&mut screen, Key::Char('a'));
    expect_request(press(&mut screen, Key::Ctrl('x')));
    assert!(screen.deliver(Response::Posted(Err(Fail {
        text: "You have reached your limit of 2 posts (posts_per_hour). Posts are counted over a rolling hour.".into(),
        gone: false,
        kept_draft: true,
    }))).is_none());
    let shown = text(&screen);
    assert!(shown.contains("reached your limit of 2 posts"), "{shown}");
    assert!(shown.contains("kept as a draft"), "{shown}");
    assert!(shown.contains("Post 1 of 1"), "back at the reader: {shown}");
}

#[test]
fn the_editor_asks_before_leaving_and_logoff_hands_over_the_draft() {
    let mut screen = open(false);
    into_reader(&mut screen, view(0, 1, "x", false));
    expect_request(press(&mut screen, Key::Char('r')));
    let editor = Editor::open(
        EditorRequest::new(russh_bbs_core::sanitize::SingleLine::new(40).sanitize("Reply"))
            .context(
                Purpose::Reply {
                    thread: ThreadId::from_raw(7),
                    subject: "s".into(),
                }
                .context()
                .unwrap(),
            ),
        None,
    )
    .unwrap();
    screen.deliver(Response::Compose(Ok(Box::new(editor))));
    press(&mut screen, Key::Char('z'));
    assert!(matches!(press(&mut screen, Key::Escape), ForumEffect::Stay));
    assert!(
        text(&screen).to_lowercase().contains("draft"),
        "{}",
        text(&screen)
    );
    match press(&mut screen, Key::Ctrl('d')) {
        ForumEffect::LogOff(Some(save)) => assert_eq!(save.text(), "z"),
        other => panic!("expected a draft to store, got {other:?}"),
    }
    let _ = DraftError::TooMany;
    let _ = Body::prepare(body_limits(), "").unwrap();
}

#[test]
fn hostile_member_text_stays_in_the_content_box_and_never_gets_the_marker_style() {
    let mut screen = ForumsScreen::open(settings()).0;
    let hostile = format!("{} {}", "x".repeat(300), "〚☉〛 ◉ SERVER-READABLE");
    screen.deliver(Response::Boards(Ok(BoardList {
        boards: vec![board(1, &hostile)],
        can_manage: false,
        can_feeds: false,
    })));
    expect_request(press(&mut screen, Key::Enter));
    screen.deliver(Response::Threads(Ok(ThreadPage {
        board: board(1, &hostile),
        threads: vec![thread(7, &hostile)],
        page: 0,
        more: false,
        can_post: true,
        can_manage: false,
        can_feeds: false,
    })));
    for area in [
        Rect::new(0, 0, 80, 24),
        Rect::new(0, 0, 40, 10),
        Rect::new(0, 0, 120, 40),
    ] {
        let buffer = draw(&screen, area);
        for y in 0..area.height - 1 {
            for x in 0..area.width {
                let modifier = buffer[(x, y)].modifier;
                assert!(
                    !modifier.contains(Modifier::BOLD | Modifier::REVERSED),
                    "marker style on content at {x},{y}"
                );
            }
        }
    }
    expect_request(press(&mut screen, Key::Enter));
    screen.deliver(Response::Post(Ok(view(
        0,
        1,
        &format!("{hostile}\n{}", "y".repeat(3000)),
        false,
    ))));
    let buffer = draw(&screen, AREA);
    for y in 0..AREA.height - 1 {
        for x in 0..AREA.width {
            assert!(
                !buffer[(x, y)]
                    .modifier
                    .contains(Modifier::BOLD | Modifier::REVERSED)
            );
        }
    }
}

/// FIX-M3-1: a pasted line break is not Enter in a single-line forum input, and Left, Right,
/// Home, End, Delete and the paging keys do nothing there.
#[test]
fn a_pasted_line_break_in_the_search_box_does_not_send_it_and_cursor_keys_do_nothing() {
    let mut screen = open(false);
    press(&mut screen, Key::Char('s'));
    for key in crate::ssh::terminal::parse_keys(b"\x1b[200~tom\nato\r\nes\x1b[201~") {
        assert!(
            matches!(press(&mut screen, key), ForumEffect::Stay),
            "{key:?} must not send the search"
        );
    }
    for key in [
        Key::Left,
        Key::Right,
        Key::Home,
        Key::End,
        Key::Delete,
        Key::PageUp,
        Key::PageDown,
        Key::PasteBreak,
    ] {
        assert!(matches!(press(&mut screen, key), ForumEffect::Stay));
    }
    let Request::Search { query, .. } = expect_request(press(&mut screen, Key::Enter)) else {
        panic!("expected a search")
    };
    assert_eq!(
        query, "tomatoes",
        "the breaks were dropped, nothing else typed"
    );
}

/// Inside the forum's editor screen a paste keeps its line breaks and the arrows move.
#[test]
fn the_forum_editor_keeps_pasted_line_breaks_and_takes_the_arrows() {
    let mut screen = open(false);
    expect_request(press(&mut screen, Key::Enter));
    screen.deliver(Response::Threads(Ok(ThreadPage {
        board: board(1, "General"),
        threads: Vec::new(),
        page: 0,
        more: false,
        can_post: true,
        can_manage: false,
        can_feeds: false,
    })));
    press(&mut screen, Key::Char('w'));
    for c in "Hello".chars() {
        press(&mut screen, Key::Char(c));
    }
    expect_request(press(&mut screen, Key::Enter));
    let editor = Editor::open(
        EditorRequest::new(russh_bbs_core::sanitize::SingleLine::new(40).sanitize("Reply")),
        None,
    )
    .unwrap();
    screen.deliver(Response::Compose(Ok(Box::new(editor))));
    for key in crate::ssh::terminal::parse_keys(b"\x1b[200~one\r\ntwo\x1b[201~\x1b[A\x1b[H>") {
        assert!(matches!(press(&mut screen, key), ForumEffect::Stay));
    }
    let Request::Submit { body, .. } = expect_request(press(&mut screen, Key::Ctrl('x'))) else {
        panic!("expected a submit")
    };
    assert_eq!(body.as_str(), ">one\ntwo");
}

/// FIX-M3-1: `PageDown` and `PageUp` turn the reader's pages; Home, End, the horizontal arrows
/// and a pasted line break do nothing there.
#[test]
fn page_keys_turn_the_readers_pages_and_other_editing_keys_do_nothing() {
    let mut screen = open(false);
    let body: String = (0..70)
        .map(|i| ["line ", &i.to_string(), "\n"].concat())
        .collect();
    into_reader(&mut screen, view(0, 1, &body, false));
    let first = text(&screen);
    assert!(first.contains("line 0"), "{first}");
    press(&mut screen, Key::PageDown);
    let second = text(&screen);
    assert!(!second.contains("line 0\n") && second != first, "{second}");
    let before = text(&screen);
    for key in [
        Key::Home,
        Key::End,
        Key::Left,
        Key::Right,
        Key::Delete,
        Key::PasteBreak,
    ] {
        assert!(matches!(press(&mut screen, key), ForumEffect::Stay));
    }
    assert_eq!(text(&screen), before, "those keys changed nothing");
    press(&mut screen, Key::PageUp);
    assert_eq!(text(&screen), first, "back on the first page");
}
