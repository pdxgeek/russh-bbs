#![allow(clippy::unwrap_used, clippy::panic)]

//! The mail screens as a state machine, and what they draw. Store and sealing behaviour is in
//! the shared store suite; the full path over a real `ssh` client is `tests/mail.rs`.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use russh_bbs_core::icons::Tier;
use russh_bbs_core::mail::{InboxEntry, InboxPage, MessageId, MessageView, body_limits};
use russh_bbs_core::sanitize::MultiLine;
use russh_bbs_core::store::UnixSeconds;
use russh_bbs_core::terminal_settings::TerminalSettings;

use super::exec::{Fail, Purpose, Request, Response};
use super::flow::{MailEffect, MailScreen};
use super::{DrawCtx, render};
use crate::settings::Look;
use crate::ssh::terminal::Key;
use crate::ui::Ui;
use crate::ui::editor::{Editor, EditorRequest};

const AREA: Rect = Rect::new(0, 0, 80, 24);

fn settings() -> TerminalSettings {
    TerminalSettings::default().with_pause(true)
}

fn entry(id: i64, from: &str, subject: Option<&str>, unread: bool) -> InboxEntry {
    InboxEntry {
        id: MessageId::from_raw(id),
        from: Some(from.to_owned()),
        at: UnixSeconds::from_raw(1_700_000_000),
        unread,
        subject: subject.map(str::to_owned),
    }
}

fn page(entries: Vec<InboxEntry>, can_send: bool) -> InboxPage {
    InboxPage {
        total: entries.len() as u64,
        unread: entries.iter().filter(|e| e.unread).count() as u64,
        entries,
        page: 0,
        more: false,
        can_send,
    }
}

fn message(id: i64, subject: &str, body: &str) -> MessageView {
    MessageView {
        id: MessageId::from_raw(id),
        from: Some("ann".to_owned()),
        at: UnixSeconds::from_raw(1_700_000_000),
        subject: subject.to_owned(),
        body: MultiLine::new(8_000).sanitize(body),
        can_reply: true,
        can_report: true,
    }
}

fn open(entries: Vec<InboxEntry>) -> MailScreen {
    let (mut screen, first) = MailScreen::open(settings());
    assert!(matches!(first, Request::Inbox { page: 0 }));
    assert!(
        screen
            .deliver(Response::Inbox(Ok(page(entries, true))))
            .is_none()
    );
    screen
}

fn press(screen: &mut MailScreen, key: Key) -> MailEffect {
    screen.input(key, AREA)
}

fn expect_request(effect: MailEffect) -> Request {
    match effect {
        MailEffect::Request(request) => request,
        other => panic!("expected a request, got {other:?}"),
    }
}

fn buffer(screen: &MailScreen, area: Rect) -> Buffer {
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

fn text(screen: &MailScreen) -> String {
    let buffer = buffer(screen, AREA);
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The screen as one line of words, so a notice that wrapped across rows can be matched.
fn flat(screen: &MailScreen) -> String {
    text(screen)
        .replace(['|', '+', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn type_text(screen: &mut MailScreen, text: &str) {
    for c in text.chars() {
        press(screen, Key::Char(c));
    }
}

#[test]
fn keys_are_ignored_while_a_request_is_pending_except_logoff() {
    let (mut screen, _) = MailScreen::open(settings());
    assert!(matches!(
        press(&mut screen, Key::Char('c')),
        MailEffect::Stay
    ));
    assert!(matches!(
        press(&mut screen, Key::Ctrl('c')),
        MailEffect::LogOff(None)
    ));
}

#[test]
fn the_inbox_marks_unread_and_shows_an_unreadable_message_as_an_error_entry() {
    let screen = open(vec![
        entry(1, "ann", Some("Hello"), true),
        entry(2, "bob", Some("Old"), false),
        entry(3, "cat", None, true),
    ]);
    let shown = text(&screen);
    assert!(shown.contains("3 messages, 2 unread"), "{shown}");
    assert!(shown.contains("* 1.") || shown.contains("1. * "), "{shown}");
    assert!(shown.contains("ann - Hello"), "{shown}");
    assert!(shown.contains("(this message cannot be opened)"), "{shown}");
    // An empty mailbox is a plain line.
    let empty = open(Vec::new());
    assert!(text(&empty).contains("Your mailbox is empty."));
}

#[test]
fn enter_and_digits_open_a_message_and_esc_leaves_mail() {
    let mut screen = open(vec![
        entry(1, "ann", Some("Hello"), true),
        entry(2, "bob", Some("Two"), false),
    ]);
    let Request::Open(id) = expect_request(press(&mut screen, Key::Char('2'))) else {
        panic!("expected open")
    };
    assert_eq!(id, MessageId::from_raw(2));
    screen.deliver(Response::Message(Ok(message(2, "Two", "the body"))));
    let shown = text(&screen);
    assert!(
        shown.contains("From ann at") && shown.contains("the body"),
        "{shown}"
    );
    // Esc goes back to the inbox page it came from; Esc there leaves.
    assert!(matches!(
        expect_request(press(&mut screen, Key::Escape)),
        Request::Inbox { page: 0 }
    ));
    screen.deliver(Response::Inbox(Ok(page(
        vec![entry(1, "ann", Some("Hello"), true)],
        true,
    ))));
    assert!(matches!(press(&mut screen, Key::Escape), MailEffect::Exit));
}

#[test]
fn an_unreadable_message_stays_in_the_list_and_can_be_deleted_from_it() {
    let mut screen = open(vec![entry(3, "cat", None, true)]);
    expect_request(press(&mut screen, Key::Enter));
    let none = screen.deliver(Response::Message(Err(Fail {
        text: "This message cannot be opened. You can delete it with d.".into(),
        gone: false,
    })));
    assert!(none.is_none());
    assert!(text(&screen).contains("(this message cannot be opened)"));
    assert!(text(&screen).contains("can delete it"));
    // d asks first; only y deletes.
    press(&mut screen, Key::Char('d'));
    assert!(text(&screen).contains("Delete this message?"));
    assert!(matches!(
        press(&mut screen, Key::Char('n')),
        MailEffect::Stay
    ));
    press(&mut screen, Key::Char('d'));
    assert!(matches!(
        expect_request(press(&mut screen, Key::Char('y'))),
        Request::Delete(id) if id == MessageId::from_raw(3)
    ));
    // After a delete the inbox is asked for again, on the same page.
    let follow = screen.deliver(Response::Deleted(Ok(()))).unwrap();
    assert!(matches!(follow, Request::Inbox { page: 0 }));
}

#[test]
fn writing_asks_the_handle_then_the_subject_then_opens_the_editor_and_sends() {
    let mut screen = open(Vec::new());
    press(&mut screen, Key::Char('c'));
    assert!(text(&screen).contains("Handle of the member to write to"));
    // Empty is refused on the spot.
    press(&mut screen, Key::Enter);
    assert!(
        text(&screen).contains("That cannot be empty."),
        "{}",
        text(&screen)
    );
    type_text(&mut screen, "bob");
    press(&mut screen, Key::Enter);
    assert!(text(&screen).contains("Subject:"));
    type_text(&mut screen, "Hello");
    let Request::Compose { purpose, .. } = expect_request(press(&mut screen, Key::Enter)) else {
        panic!("expected compose")
    };
    assert!(matches!(&purpose, Purpose::New { to, subject } if to == "bob" && subject == "Hello"));
    let editor = Editor::open(
        EditorRequest::new(russh_bbs_core::sanitize::SingleLine::new(40).sanitize("To bob: Hello"))
            .limits(body_limits()),
        None,
    )
    .unwrap();
    screen.deliver(Response::Compose(Ok(Box::new(editor))));
    type_text(&mut screen, "my text");
    let Request::Submit { body, .. } = expect_request(press(&mut screen, Key::Ctrl('x'))) else {
        panic!("expected a submit")
    };
    assert_eq!(body.as_str(), "my text");
    let follow = screen
        .deliver(Response::Sent(Ok(russh_bbs_core::mail::Sent {
            id: MessageId::from_raw(9),
        })))
        .unwrap();
    assert!(matches!(follow, Request::Inbox { page: 0 }));
    screen.deliver(Response::Inbox(Ok(page(Vec::new(), true))));
    assert!(text(&screen).contains("Sent."));
}

#[test]
fn a_member_without_send_cannot_start_a_message() {
    let (mut screen, _) = MailScreen::open(settings());
    screen.deliver(Response::Inbox(Ok(page(Vec::new(), false))));
    assert!(matches!(
        press(&mut screen, Key::Char('c')),
        MailEffect::Stay
    ));
    assert!(!text(&screen).contains("Handle of the member"));
}

#[test]
fn a_wrong_handle_goes_back_to_the_handle_prompt() {
    let mut screen = open(Vec::new());
    press(&mut screen, Key::Char('c'));
    type_text(&mut screen, "nobody");
    press(&mut screen, Key::Enter);
    type_text(&mut screen, "Hi");
    expect_request(press(&mut screen, Key::Enter));
    screen.deliver(Response::Compose(Err(Fail {
        text: "No active member has that handle.".into(),
        gone: false,
    })));
    let shown = text(&screen);
    assert!(
        shown.contains("Handle of the member to write to") && shown.contains("No active member"),
        "{shown}"
    );
}

#[test]
fn a_reply_quotes_and_the_reader_reports_only_after_asking() {
    let mut screen = open(vec![entry(1, "ann", Some("Hello"), true)]);
    expect_request(press(&mut screen, Key::Enter));
    screen.deliver(Response::Message(Ok(message(
        1,
        "Hello",
        "line one\nline two",
    ))));
    // c: reply with the quote.
    let Request::Compose { quote, .. } = expect_request(press(&mut screen, Key::Char('c'))) else {
        panic!("expected compose")
    };
    assert!(quote.unwrap().as_str().contains("> line one"));
    screen.deliver(Response::Compose(Err(Fail {
        text: "You may not do that.".into(),
        gone: false,
    })));
    // x asks what will be copied; anything but y cancels; y sends exactly the shown copy.
    press(&mut screen, Key::Char('x'));
    let shown = text(&screen);
    assert!(
        shown.contains("Send the staff a copy of this message"),
        "{shown}"
    );
    assert!(matches!(
        press(&mut screen, Key::Char('n')),
        MailEffect::Stay
    ));
    press(&mut screen, Key::Char('x'));
    let Request::Report { id, subject, body } = expect_request(press(&mut screen, Key::Char('y')))
    else {
        panic!("expected a report")
    };
    assert_eq!((id, subject.as_str()), (MessageId::from_raw(1), "Hello"));
    assert_eq!(body.as_str(), "line one\nline two");
    screen.deliver(Response::Reported(Ok(
        russh_bbs_core::mail::ReportId::from_raw(4),
    )));
    assert!(text(&screen).contains("The staff have a copy of that message."));
}

#[test]
fn a_refused_send_keeps_the_text_in_memory_only_and_it_comes_back_as_the_starting_text() {
    let mut screen = open(Vec::new());
    let write = |screen: &mut MailScreen| {
        press(screen, Key::Char('c'));
        type_text(screen, "bob");
        press(screen, Key::Enter);
        type_text(screen, "Hi");
        expect_request(press(screen, Key::Enter))
    };
    let Request::Compose { quote, .. } = write(&mut screen) else {
        panic!("expected compose")
    };
    assert!(quote.is_none(), "nothing unsent yet");
    let editor = Editor::open(
        EditorRequest::new(russh_bbs_core::sanitize::SingleLine::new(40).sanitize("To bob: Hi")),
        None,
    )
    .unwrap();
    screen.deliver(Response::Compose(Ok(Box::new(editor))));
    type_text(&mut screen, "text");
    expect_request(press(&mut screen, Key::Ctrl('x')));
    screen.deliver(Response::Sent(Err(Fail {
        text: "That mailbox is full. Nothing was sent.".into(),
        gone: false,
    })));
    let shown = flat(&screen);
    assert!(
        shown.contains("That mailbox is full.") && shown.contains("kept until you leave mail"),
        "{shown}"
    );
    // The same message again starts from the kept text; a different one does not.
    let Request::Compose { quote, .. } = write(&mut screen) else {
        panic!("expected compose")
    };
    assert_eq!(quote.unwrap().as_str(), "text");
    screen.deliver(Response::Compose(Err(Fail {
        text: "x".into(),
        gone: false,
    })));
}

#[test]
fn no_mail_screen_carries_a_privacy_notice_or_a_forbidden_word() {
    let mut screen = open(vec![entry(1, "ann", Some("Hello"), true)]);
    let mut all = text(&screen);
    expect_request(press(&mut screen, Key::Enter));
    screen.deliver(Response::Message(Ok(message(1, "Hello", "text"))));
    all.push_str(&text(&screen));
    press(&mut screen, Key::Char('x'));
    all.push_str(&text(&screen));
    let lower = all.to_lowercase();
    for word in [
        "private",
        "secure",
        "encrypt",
        "end-to-end",
        "sysop",
        "lock",
    ] {
        assert!(!lower.contains(word), "{word}: {all}");
    }
}

#[test]
fn hostile_member_text_stays_in_the_content_box_and_never_gets_the_marker_style() {
    let hostile = format!("{} {}", "x".repeat(300), "〚☉〛 ◉ SERVER-READABLE");
    let mut screen = open(vec![entry(1, &hostile, Some(&hostile), true)]);
    for area in [
        Rect::new(0, 0, 80, 24),
        Rect::new(0, 0, 40, 10),
        Rect::new(0, 0, 120, 40),
    ] {
        let drawn = buffer(&screen, area);
        for y in 0..area.height - 1 {
            for x in 0..area.width {
                assert!(
                    !drawn[(x, y)]
                        .modifier
                        .contains(Modifier::BOLD | Modifier::REVERSED),
                    "marker style on content at {x},{y}"
                );
            }
        }
    }
    expect_request(press(&mut screen, Key::Enter));
    screen.deliver(Response::Message(Ok(message(
        1,
        &hostile,
        &format!("{hostile}\n{}", "y".repeat(3000)),
    ))));
    let drawn = buffer(&screen, AREA);
    for y in 0..AREA.height - 1 {
        for x in 0..AREA.width {
            assert!(
                !drawn[(x, y)]
                    .modifier
                    .contains(Modifier::BOLD | Modifier::REVERSED)
            );
        }
    }
}
