#![allow(clippy::unwrap_used, clippy::panic)]

//! The queue screens as drawn: the list, a request and the confirmation inside the classic
//! layout, at 80x24 and other sizes, in every icon tier, with hostile requester text.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Modifier;
use russh_bbs_core::access::{Permission, Slug, Snapshot, Via};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::queue::{
    Message, PendingRequest, Queue, QueueKey, QueueMode, QueueScreen, QueueView, RequestId,
    RequestKind, Response, Voucher,
};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountId, UnixSeconds};
use russh_bbs_core::terminal_settings::TerminalSettings;

use super::lines;
use crate::settings::Look;
use crate::ui::Ui;
use crate::ui::classic::{ContentScreen, MENU_MARKERS, render_content};

fn draw(view: &QueueView, tier: Tier, size: (u16, u16)) -> Buffer {
    let ui = Ui::standard().unwrap();
    let look = Look::of(&TerminalSettings::default().with_tier(tier));
    let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
    terminal
        .draw(|frame| {
            let width =
                usize::from(frame.area().width.min(crate::settings::MAX_WIDTH)).saturating_sub(2);
            render_content(
                frame,
                ContentScreen {
                    strings: ui.strings(),
                    lines: lines(view, ui.strings(), look, width),
                    look,
                    tier,
                    markers: &MENU_MARKERS,
                },
            );
        })
        .unwrap();
    terminal.backend().buffer().clone()
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol().to_owned())
        .collect()
}

fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| row(buffer, y))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_chrome_cell(buffer: &Buffer, x: u16, y: u16) -> bool {
    buffer[(x, y)]
        .modifier
        .contains(Modifier::BOLD | Modifier::REVERSED)
}

fn access() -> Snapshot {
    Snapshot::from_role(
        Some(AccountId::from_raw(2)),
        &Slug::parse("cosysop").unwrap(),
        40,
        &Via::Account,
        [
            Permission::MembersApprove,
            Permission::MembersDeny,
            Permission::MembersVouch,
        ],
    )
}

fn request(id: i64, handle: &str, message: &str, vouchers: usize) -> PendingRequest {
    PendingRequest {
        id: RequestId::from_raw(id),
        handle: sanitize_username(handle).unwrap(),
        message: Message::new(message),
        created_at: UnixSeconds::from_raw(1_000),
        expires_at: UnixSeconds::from_raw(1_000 + 14 * 86_400),
        kind: RequestKind::Invite,
        vouchers: (0..vouchers)
            .map(|n| Voucher {
                account: AccountId::from_raw(100 + i64::try_from(n).unwrap()),
                handle: sanitize_username(&format!("voucher_{n}_{}", "v".repeat(12))).unwrap(),
            })
            .collect(),
    }
}

/// The fullest screens: four rows with the longest handles, messages and vouch names.
fn screen_with(rows: Vec<PendingRequest>) -> QueueScreen {
    let mut screen = QueueScreen::open(QueueMode::Sysop, Some(AccountId::from_raw(2)));
    screen.take_request();
    screen.deliver(Response::Queue(Queue {
        now: UnixSeconds::from_raw(1_000 + 3 * 86_400),
        requests: rows,
    }));
    screen
}

fn long_rows(message: &str) -> Vec<PendingRequest> {
    (1..=6)
        .map(|n| request(n, &format!("handle_{n}_{}", "h".repeat(15)), message, 5))
        .collect()
}

#[test]
fn the_fullest_screens_fit_80x24_with_their_footer_in_every_tier() {
    let access = access();
    for tier in Tier::ALL {
        for (name, message) in [("plain", "m".repeat(280)), ("wide", "漢".repeat(280))] {
            let mut screen = screen_with(long_rows(&message));
            // The list.
            let list = screen.view(&access);
            // A request, then the confirmation of each decision.
            let mut views = vec![("list", list, "1-4: open")];
            screen.press(&access, QueueKey::Char('1'));
            views.push(("detail", screen.view(&access), "Esc: back to the list"));
            for (key, label) in [('a', "approve"), ('d', "deny"), ('v', "vouch")] {
                screen.press(&access, QueueKey::Char(key));
                views.push((label, screen.view(&access), "any other key: cancel"));
                screen.press(&access, QueueKey::Char('n'));
            }
            // A refusal's notice on the request, the one time the detail grows by two lines.
            screen.press(&access, QueueKey::Char('a'));
            screen.press(&access, QueueKey::Char('y'));
            screen.take_request();
            screen.deliver(Response::HandleTaken);
            views.push(("refusal", screen.view(&access), "Esc: back to the list"));
            for (what, view, footer) in views {
                let buffer = draw(&view, tier, (80, 24));
                let out = text(&buffer);
                assert!(
                    out.contains(footer),
                    "{tier:?} {name} {what}: footer clipped\n{out}"
                );
                // The status row still shows the server-readable marker.
                assert!(
                    row(&buffer, 23).contains("The server can read"),
                    "{tier:?} {name} {what}: {:?}",
                    row(&buffer, 23)
                );
            }
        }
    }
}

#[test]
fn a_list_entry_is_two_lines_and_the_message_is_cut_to_one() {
    let access = access();
    let screen = screen_with(vec![request(1, "alice", &"x".repeat(280), 2)]);
    let out = text(&draw(&screen.view(&access), Tier::Ascii, (80, 24)));
    assert!(out.contains("[1] alice"), "{out}");
    assert!(out.contains("asked 3d ago   vouches: 2"), "{out}");
    let message_lines = out.lines().filter(|l| l.contains("xxxx")).count();
    assert_eq!(message_lines, 1, "the message stays on one line\n{out}");
}

#[test]
fn the_detail_wraps_the_whole_message_and_names_the_vouchers() {
    let access = access();
    let mut screen = screen_with(vec![request(7, "alice", &"word ".repeat(56), 2)]);
    screen.press(&access, QueueKey::Char('1'));
    let out = text(&draw(&screen.view(&access), Tier::Ascii, (80, 24)));
    assert!(out.contains("Request 7"), "{out}");
    assert!(out.contains("Request 7: alice"), "{out}");
    assert!(
        out.contains("voucher_0_vvvvvvvvvvvv, voucher_1_vvvvvvvvvvvv"),
        "{out}"
    );
    assert!(out.contains("Asked 3d ago. Kept until"), "{out}");
    assert!(
        out.matches("word word").count() >= 3,
        "the message wraps\n{out}"
    );
    assert!(out.contains("as typed by the requester"), "{out}");
    assert!(out.contains("[a] Approve   [d] Deny   [v] Vouch"), "{out}");
}

#[test]
fn requester_text_never_reaches_the_status_row_or_takes_the_marker_style() {
    let access = access();
    let hostile = format!(
        "\u{1b}[2J\u{1b}[31mFAKE\u{7} \u{301a}!\u{301b} \u{25c9} {}",
        "\u{e0b0}".repeat(10)
    );
    for tier in Tier::ALL {
        let screen = screen_with(vec![request(1, "mallory", &hostile, 1)]);
        let list = draw(&screen.view(&access), tier, (80, 24));
        let mut detail_screen = screen_with(vec![request(1, "mallory", &hostile, 1)]);
        detail_screen.press(&access, QueueKey::Char('1'));
        let detail = draw(&detail_screen.view(&access), tier, (80, 24));
        for buffer in [&list, &detail] {
            let all = text(buffer);
            assert!(!all.contains('\u{1b}') && !all.contains('\u{7}'), "{all:?}");
            assert!(
                !all.contains('\u{e0b0}'),
                "Nerd Font glyphs are neutralized"
            );
            // Only the status row (the last) may hold the reserved marker style.
            for y in 0..23 {
                for x in 0..buffer.area.width {
                    assert!(
                        !is_chrome_cell(buffer, x, y),
                        "{tier:?}: reserved style at ({x},{y})\n{all}"
                    );
                }
            }
            assert!(row(buffer, 23).contains("The server can read"));
            assert!(!row(buffer, 23).contains("FAKE"));
        }
    }
}

#[test]
fn it_still_draws_on_a_tiny_terminal_without_panicking() {
    let access = access();
    let screen = screen_with(long_rows("hello"));
    for size in [(1, 1), (20, 5), (40, 12), (132, 50)] {
        let _ = draw(&screen.view(&access), Tier::Ascii, size);
    }
}

#[tokio::test]
async fn running_a_request_checks_the_permission_and_never_hangs() {
    use russh_bbs_core::access::NoMembers;
    use russh_bbs_core::queue::Request;
    // The empty directory has no queue: a listing is empty, a decision finds nothing.
    let staff = access();
    let Response::Queue(queue) = super::run(&NoMembers, &staff, Request::List).await else {
        panic!("a listing");
    };
    assert_eq!(queue.requests.len(), 0);
    assert_eq!(
        super::run(&NoMembers, &staff, Request::Approve(RequestId::from_raw(1))).await,
        Response::Gone
    );
    let nobody = Snapshot::empty();
    assert_eq!(
        super::run(&NoMembers, &nobody, Request::List).await,
        Response::NotPermitted
    );
}

#[tokio::test]
async fn the_purge_runs_at_once_and_keeps_running() {
    use russh_bbs_core::access::NoMembers;
    use std::time::Duration;
    // The empty directory has nothing to purge, and says so.
    assert_eq!(
        super::purge_once(&NoMembers).await,
        Some(russh_bbs_core::queue::Purged::default())
    );
    // The loop never returns on its own: it is still running when the timeout ends.
    let ran = tokio::time::timeout(
        Duration::from_millis(150),
        super::purge_every(NoMembers, Duration::from_millis(20)),
    )
    .await;
    assert!(ran.is_err());
}
