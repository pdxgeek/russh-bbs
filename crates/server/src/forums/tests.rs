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
use russh_bbs_core::waiting_room::NoticeKind;

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
        markers: &[],
        clock: Some("UTC 12:00"),
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
    assert!(shown.contains("1. General"), "{shown}");
    assert!(shown.contains("2. Tech"), "{shown}");
    assert!(shown.contains("Threads"), "the column header:\n{shown}");
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
        text(&screen).contains("Problem") && text(&screen).contains("That is not available."),
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
        shown.contains("General")
            && shown.contains("Gardening")
            && shown.contains("tomatoes need sun"),
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
    assert!(first.contains("Post 1 of 1"), "{first}");
    assert!(first.contains("General"), "the board names the box: {first}");
    assert!(first.contains("From ann"), "{first}");
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
        shown.contains("opening") && shown.contains("whole") && shown.contains("Confirm"),
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

// ---- long lists (FIX-M3-4, QA M3 M2) ----

fn text_at(screen: &ForumsScreen, area: Rect) -> String {
    let buffer = draw(screen, area);
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn many_boards(count: i64) -> ForumsScreen {
    let (mut screen, _) = ForumsScreen::open(settings());
    let boards = (1..=count)
        .map(|n| board(n, &format!("Board{n:03}")))
        .collect();
    assert!(
        screen
            .deliver(Response::Boards(Ok(BoardList {
                boards,
                can_manage: true,
                can_feeds: true,
            })))
            .is_none()
    );
    screen.notice = None;
    screen
}

/// The name on the drawn selected row, if one is drawn.
fn selected_row(shown: &str) -> Option<String> {
    shown
        .lines()
        .find_map(|line| line.split_once("> ").map(|(_, rest)| rest.to_owned()))
        .filter(|rest| rest.contains("Board"))
}

const SMALL: Rect = Rect::new(0, 0, 40, 10);
const TINY: Rect = Rect::new(0, 0, 30, 8);

fn info_notice(screen: &mut ForumsScreen) {
    screen.notice = Some((NoticeKind::Info, "Feed added".to_owned()));
}

#[test]
fn the_board_list_always_draws_the_selected_row_and_the_notice() {
    for area in [AREA, SMALL, TINY] {
        for count in [1_i64, 20, 23, 100] {
            let mut screen = many_boards(count);
            for step in 0..count {
                info_notice(&mut screen);
                let shown = text_at(&screen, area);
                let expected = format!("Board{:03}", step + 1);
                assert!(
                    selected_row(&shown).is_some_and(|r| r.contains(&expected)),
                    "{area:?} {count} boards, cursor {step}: selected row missing\n{shown}"
                );
                assert!(
                    shown.contains("Feed added"),
                    "{area:?} {count} boards, cursor {step}: notice missing\n{shown}"
                );
                if step + 1 < count {
                    screen.input(Key::Down, area);
                }
            }
            // Enter opens the board drawn as selected: the last one, then (after Home) the first.
            let Request::Threads { board, .. } = expect_request(screen.input(Key::Enter, area))
            else {
                panic!("Enter did not open a board");
            };
            assert_eq!(board, BoardId::from_raw(count));
        }
    }
}

#[test]
fn page_keys_home_end_and_digits_stay_on_drawn_rows() {
    let mut screen = many_boards(100);
    let _ = text_at(&screen, AREA);
    screen.input(Key::PageDown, AREA);
    let shown = text_at(&screen, AREA);
    let row = selected_row(&shown).unwrap();
    assert!(!row.contains("Board001"), "{shown}");
    // A digit opens the n-th drawn row, whichever board that is.
    let n = shown
        .lines()
        .find(|l| l.contains("1. Board"))
        .and_then(|l| l.split("1. Board").nth(1))
        .map(|rest| rest[..3].parse::<i64>().unwrap())
        .unwrap();
    let Request::Threads { board, .. } = expect_request(screen.input(Key::Char('1'), AREA)) else {
        panic!("digit did not open a board");
    };
    assert_eq!(board, BoardId::from_raw(n));
    let mut screen = many_boards(100);
    let _ = text_at(&screen, AREA);
    screen.input(Key::End, AREA);
    assert!(
        selected_row(&text_at(&screen, AREA))
            .unwrap()
            .contains("Board100")
    );
    screen.input(Key::Home, AREA);
    assert!(
        selected_row(&text_at(&screen, AREA))
            .unwrap()
            .contains("Board001")
    );
}

fn many_threads(count: i64) -> ForumsScreen {
    let mut screen = open(false);
    let page = ThreadPage {
        board: board(1, "General"),
        threads: (1..=count)
            .map(|n| thread(n, &format!("Subject{n:03}")))
            .collect(),
        page: 0,
        more: false,
        can_post: true,
        can_manage: false,
        can_feeds: false,
    };
    screen.state = super::flow::State::Threads { page, cursor: 0 };
    screen
}

fn many_results(count: i64) -> ForumsScreen {
    let mut screen = open(false);
    let hits = (1..=count)
        .map(|n| Hit {
            thread: ThreadId::from_raw(n),
            post: PostId::from_raw(n),
            board: BoardId::from_raw(1),
            author: Some("ann".to_owned()),
            index: 0,
            board_name: "General".to_owned(),
            subject: format!("Subject{n:03}"),
            snippet: "a snippet".to_owned(),
            at: UnixSeconds::from_raw(1),
        })
        .collect();
    let page = SearchPage {
        hits,
        page: 0,
        more: false,
        searched: true,
    };
    screen.state = super::flow::State::Results {
        page,
        query: "word".to_owned(),
        cursor: 0,
    };
    screen
}

fn selected_subject(shown: &str) -> Option<String> {
    shown
        .lines()
        .find_map(|l| l.split_once("> ").map(|(_, r)| r.to_owned()))
        .filter(|r| r.contains("Subject"))
}

#[test]
fn thread_list_and_search_results_always_draw_the_selected_row_and_the_notice() {
    for area in [AREA, SMALL, TINY] {
        for count in [1_i64, 20, 23, 100] {
            for results in [false, true] {
                let mut screen = if results {
                    many_results(count)
                } else {
                    many_threads(count)
                };
                for step in 0..count {
                    info_notice(&mut screen);
                    let shown = text_at(&screen, area);
                    let expected = format!("Subject{:03}", step + 1);
                    assert!(
                        selected_subject(&shown).is_some_and(|r| r.contains(&expected)),
                        "{area:?} {count} results={results} cursor {step}\n{shown}"
                    );
                    assert!(shown.contains("Feed added"), "{area:?} {count}\n{shown}");
                    if step + 1 < count {
                        screen.input(Key::Down, area);
                    }
                }
            }
        }
    }
}

// ---- UI-1: the framed lists at every size, tier, layout, theme and colour depth ----

fn hostile_boards() -> ForumsScreen {
    let mut screen = open(true);
    let evil = "\u{1b}[7mFAKE\u{1b}[0m \u{27e6}E2EE\u{27e7} \u{2299} LOW-SECURITY \u{e000}";
    let mut list = BoardList {
        boards: Vec::new(),
        can_manage: true,
        can_feeds: true,
    };
    for id in 1..=40 {
        let mut b = board(id, &format!("{evil} {id}"));
        b.description = format!("{evil}{}", "d".repeat(3000));
        list.boards.push(b);
    }
    screen.state = super::flow::State::Boards { list, cursor: 7 };
    screen
}

fn hostile_threads() -> ForumsScreen {
    let mut screen = many_threads(60);
    if let super::flow::State::Threads { page, .. } = &mut screen.state {
        for thread in &mut page.threads {
            thread.subject = format!("\u{1b}]0;x\u{7}\u{27e6}E2EE\u{27e7}{}", "s".repeat(2000));
            thread.author = Some("\u{2299}evil".to_owned());
        }
    }
    screen
}

fn hostile_results() -> ForumsScreen {
    let mut screen = many_results(30);
    if let super::flow::State::Results { page, query, .. } = &mut screen.state {
        for hit in &mut page.hits {
            hit.snippet = format!("\u{1b}[31m\u{27e6}E2EE\u{27e7}{}", "n".repeat(2000));
        }
        *query = "\u{1b}[7m\u{27e6}query".to_owned();
    }
    screen
}

#[test]
fn the_forum_lists_keep_the_marker_cell_clean_at_every_size_tier_layout_theme_and_depth() {
    use crate::ui::depth::ColorDepth;
    use russh_bbs_core::icons::{Icon, SecurityMarker};
    use russh_bbs_core::terminal_settings::LayoutId;

    const DRAWN: [SecurityMarker; 2] = [SecurityMarker::Unencrypted, SecurityMarker::LowSecurity];
    let ui = Ui::standard().unwrap();
    for (label, screen) in [
        ("boards", hostile_boards()),
        ("threads", hostile_threads()),
        ("results", hostile_results()),
    ] {
        for theme in ui.theme_names() {
            for layout in LayoutId::ALL {
                for tier in Tier::ALL {
                    for depth in ColorDepth::ALL {
                        for size in [(80_u16, 24_u16), (100, 30), (132, 43)] {
                            let name =
                                format!("{label} {theme:?} {layout:?} {tier:?} {depth:?} {size:?}");
                            let settings = settings().with_tier(tier).with_color(depth.has_color());
                            let themed = ui.theme(theme);
                            let ctx = DrawCtx {
                                ui: &ui,
                                strings: themed.strings(),
                                settings: &settings,
                                look: Look::themed(&settings, layout, themed.palette())
                                    .at_depth(depth),
                                tier,
                                markers: &DRAWN,
                                clock: Some("UTC 12:00"),
                            };
                            let mut terminal =
                                Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
                            terminal.draw(|frame| render(frame, &screen, &ctx)).unwrap();
                            let buffer = terminal.backend().buffer().clone();
                            let last = size.1 - 1;
                            let status: String = (0..size.0)
                                .map(|x| buffer[(x, last)].symbol().to_owned())
                                .collect();
                            let mut marker_cells = 0;
                            for marker in DRAWN {
                                let glyph = Icon::from(marker)
                                    .render(tier)
                                    .plain()
                                    .trim_end()
                                    .to_owned();
                                assert!(status.contains(&glyph), "{name}: {status:?}");
                                marker_cells += glyph.chars().count();
                            }
                            let styled = (0..size.1)
                                .flat_map(|y| (0..size.0).map(move |x| (x, y)))
                                .filter(|&(x, y)| {
                                    buffer[(x, y)]
                                        .modifier
                                        .contains(Modifier::BOLD | Modifier::REVERSED)
                                })
                                .count();
                            assert_eq!(styled, marker_cells, "{name}: reserved style leaked");
                            let reversed = (0..size.1)
                                .flat_map(|y| (0..size.0).map(move |x| (x, y)))
                                .filter(|&(x, y)| {
                                    buffer[(x, y)].modifier.contains(Modifier::REVERSED)
                                })
                                .count();
                            assert_eq!(reversed, marker_cells, "{name}: reverse video used");
                            // Hostile content stayed in the body: above the status bar there
                            // is no escape, no reserved codepoint and no private-use glyph.
                            let above: String = (0..last)
                                .flat_map(|y| (0..size.0).map(move |x| (x, y)))
                                .map(|(x, y)| buffer[(x, y)].symbol().to_owned())
                                .collect();
                            for banned in ["\u{1b}", "\u{27e6}", "\u{27e7}", "\u{2299}", "\u{e000}"]
                            {
                                assert!(!above.contains(banned), "{name}: {banned:?}");
                            }
                            // The table is there: header, and the selected row has its pointer.
                            assert!(
                                above.contains("Subject") || above.contains("Board"),
                                "{name}: header\n{above}"
                            );
                        }
                    }
                }
            }
        }
    }
}
