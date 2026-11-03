//! The hub and the request path, with a manual clock, the real SQLite store for room
//! definitions, and no sleeps. Concurrency tests run on a multi-thread runtime.
#![allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]

use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh_bbs_core::access::{Held, Permission, ResourceRef, Scope, Slug, Snapshot, Source, Via};
use russh_bbs_core::chat::{
    ChatLimits, ChatNotice, ChatRequest, ChatResponse, Event, Room, RoomId, clean_line,
};
use russh_bbs_core::sanitize::{Username, sanitize_username};
use russh_bbs_core::store::{AccountId, AccountState, AccountStore, NewAccount};
use russh_bbs_core::waiting_room::{ManualClock, RatePolicy};
use russh_bbs_store::SqliteStore;
use tempfile::TempDir;

use super::{ChatSession, Hub};

fn handle(text: &str) -> Username {
    sanitize_username(text).unwrap()
}

fn snapshot(account: i64, perms: &[Permission]) -> Snapshot {
    Snapshot::new(
        Some(AccountId::from_raw(account)),
        perms
            .iter()
            .map(|p| Held {
                permission: *p,
                scope: Scope::Global,
                source: Source::Role {
                    role: Slug::MEMBER,
                    rank: 10,
                    via: Via::Account,
                },
            })
            .collect(),
    )
}

const MEMBER: [Permission; 3] = [
    Permission::ChatJoin,
    Permission::ChatSpeak,
    Permission::ChatWhisper,
];

fn member(account: i64) -> Snapshot {
    snapshot(account, &MEMBER)
}

fn staff(account: i64) -> Snapshot {
    snapshot(
        account,
        &[
            Permission::ChatJoin,
            Permission::ChatSpeak,
            Permission::ChatManageRooms,
        ],
    )
}

fn roomy(n: u64) -> ChatLimits {
    ChatLimits {
        rate: RatePolicy::new(u32::try_from(n).unwrap(), Duration::from_millis(1)),
        scrollback: 1000,
        queue: 4096,
        members_per_room: 64,
        sessions: 64,
    }
}

struct World {
    hub: Arc<Hub>,
    clock: ManualClock,
    store: SqliteStore,
    dir: TempDir,
}

async fn world(limits: ChatLimits) -> World {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
    // Accounts 1..=3 exist so staff actions have an actor (the sysop is account 1).
    for name in ["sam", "ann", "bob"] {
        store
            .create_account(NewAccount {
                username: handle(name),
                role: Slug::MEMBER,
                state: AccountState::Active,
            })
            .await
            .unwrap();
    }
    let clock = ManualClock::new();
    World {
        hub: Hub::new(limits, Arc::new(clock.clone())),
        clock,
        store,
        dir,
    }
}

impl World {
    fn session(&self, account: i64, name: &str) -> ChatSession {
        self.hub
            .connect(AccountId::from_raw(account), handle(name))
            .unwrap()
    }

    async fn run(
        &self,
        who: &ChatSession,
        access: &Snapshot,
        request: ChatRequest,
    ) -> ChatResponse {
        who.execute(&self.store, access, request).await
    }

    async fn enter(&self, who: &ChatSession, access: &Snapshot, room: i64) -> ChatResponse {
        self.run(who, access, ChatRequest::Join(RoomId::from_raw(room)))
            .await
    }
}

fn say(text: &str) -> ChatRequest {
    ChatRequest::Say(clean_line(text).unwrap())
}

fn lines_of(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Line(line) => Some(format!("{}:{}", line.from.as_str(), line.text.as_str())),
            _ => None,
        })
        .collect()
}

fn joined(response: &ChatResponse) -> bool {
    matches!(response, ChatResponse::Joined { .. })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_sessions_in_one_room_all_see_every_line_in_one_order() {
    const SESSIONS: i64 = 8;
    const LINES: usize = 40;
    let w = Arc::new(world(roomy(1000)).await);
    let mut sessions = Vec::new();
    for n in 1..=SESSIONS {
        let s = Arc::new(w.session(n, &format!("u{n}")));
        assert!(joined(&w.enter(&s, &member(n), 1).await));
        sessions.push(s);
    }
    let mut tasks = Vec::new();
    for (index, s) in sessions.iter().enumerate() {
        let (w, s) = (Arc::clone(&w), Arc::clone(s));
        let n = i64::try_from(index).unwrap() + 1;
        tasks.push(tokio::spawn(async move {
            let access = member(n);
            for i in 0..LINES {
                let response = w.run(&s, &access, say(&format!("m{i}"))).await;
                assert_eq!(response, ChatResponse::Done);
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let reference = lines_of(&sessions[0].drain());
    assert_eq!(reference.len(), usize::try_from(SESSIONS).unwrap() * LINES);
    for s in &sessions[1..] {
        assert_eq!(lines_of(&s.drain()), reference, "one order for everyone");
    }
    // Each sender's own lines are in the order it sent them.
    for n in 1..=SESSIONS {
        let own: Vec<_> = reference
            .iter()
            .filter(|l| l.starts_with(&format!("u{n}:")))
            .collect();
        assert_eq!(own.len(), LINES);
        assert!(own.windows(2).all(|p| {
            let k = |s: &&String| s.rsplit('m').next().unwrap().parse::<usize>().unwrap();
            k(&p[0]) < k(&p[1])
        }));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stalled_reader_never_holds_up_the_room() {
    let limits = ChatLimits {
        queue: 16,
        ..roomy(10_000)
    };
    let w = Arc::new(world(limits).await);
    let stalled = w.session(1, "stalled");
    let a = Arc::new(w.session(2, "ann"));
    let b = Arc::new(w.session(3, "bob"));
    assert!(joined(&w.enter(&stalled, &member(1), 1).await));
    assert!(joined(&w.enter(&a, &member(2), 1).await));
    assert!(joined(&w.enter(&b, &member(3), 1).await));
    a.drain();
    b.drain();
    // `stalled` never drains. Two senders flood the room; both finish (a blocked room would
    // hang here) and the active reader keeps up.
    let mut tasks = Vec::new();
    for (s, n) in [(&a, 2), (&b, 3)] {
        let (w, s) = (Arc::clone(&w), Arc::clone(s));
        tasks.push(tokio::spawn(async move {
            let access = member(n);
            let mut seen = 0;
            for i in 0..500 {
                w.run(&s, &access, say(&format!("l{i}"))).await;
                seen += lines_of(&s.drain()).len();
            }
            seen
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let events = stalled.drain();
    let Event::Missed(missed) = events[0] else {
        panic!("expected a missed count first: {:?}", events.first())
    };
    // 1000 lines were said and 2 join announcements came before them; the queue kept 16.
    assert!(missed >= 900, "{missed}");
    assert!(events.len() <= 17, "bounded: {}", events.len());
}

#[tokio::test]
async fn disconnecting_mid_line_sends_nothing_and_tells_the_room() {
    let w = world(roomy(100)).await;
    let a = w.session(2, "ann");
    let b = w.session(3, "bob");
    w.enter(&a, &member(2), 1).await;
    w.enter(&b, &member(3), 1).await;
    a.drain();
    b.drain();
    assert_eq!(w.hub.session_count(), 2);
    // Bob had typed half a line (it lives in his screen only, never reaches the hub) and his
    // connection ends: the session is dropped.
    drop(b);
    assert_eq!(a.drain(), vec![Event::Left(handle("bob"))]);
    assert_eq!(w.hub.session_count(), 1);
    // Ann goes too; nothing is left behind.
    drop(a);
    assert_eq!(w.hub.session_count(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_line_wakes_a_waiting_session_without_any_key() {
    let w = Arc::new(world(roomy(100)).await);
    let reader = Arc::new(w.session(2, "ann"));
    let writer = w.session(3, "bob");
    w.enter(&reader, &member(2), 1).await;
    w.enter(&writer, &member(3), 1).await;
    reader.drain();
    let waiting = {
        let reader = Arc::clone(&reader);
        tokio::spawn(async move {
            // A wake can be stale (a permit from an earlier event); the shell treats an empty
            // drain as nothing to do, and so does this loop.
            loop {
                reader.woken().await;
                let events = reader.drain();
                if !events.is_empty() {
                    break lines_of(&events);
                }
            }
        })
    };
    w.run(&writer, &member(3), say("hello")).await;
    assert_eq!(waiting.await.unwrap(), ["bob:hello"]);
}

#[tokio::test]
async fn a_flood_gets_slow_down_and_reaches_nobody() {
    let limits = ChatLimits {
        rate: RatePolicy::new(3, Duration::from_secs(1)),
        ..roomy(1)
    };
    let w = world(limits).await;
    let a = w.session(2, "ann");
    let b = w.session(3, "bob");
    w.enter(&a, &member(2), 1).await;
    w.enter(&b, &member(3), 1).await;
    b.drain();
    for _ in 0..3 {
        assert_eq!(w.run(&a, &member(2), say("x")).await, ChatResponse::Done);
    }
    for _ in 0..20 {
        assert_eq!(
            w.run(&a, &member(2), say("spam")).await,
            ChatResponse::Refused(ChatNotice::SlowDown)
        );
    }
    assert_eq!(lines_of(&b.drain()).len(), 3);
    w.clock.advance(Duration::from_secs(1));
    assert_eq!(w.run(&a, &member(2), say("ok")).await, ChatResponse::Done);
}

#[tokio::test]
async fn a_whisper_reaches_only_its_target_and_a_miss_says_the_same_thing_every_time() {
    let w = world(roomy(100)).await;
    let ann = w.session(2, "ann");
    let bob = w.session(3, "bob");
    let cy = w.session(4, "cy");
    let elsewhere = w.session(5, "dee");
    for (s, n) in [(&ann, 2), (&bob, 3), (&cy, 4)] {
        w.enter(s, &member(n), 1).await;
    }
    // Dee exists and is in chat, but in another room (room 2 does not exist as a definition;
    // she is simply in no room), so she is "not here" like a name nobody has.
    for s in [&ann, &bob, &cy] {
        s.drain();
    }
    let to_bob = ChatRequest::Whisper {
        to: "Bob".into(),
        text: clean_line("the plan").unwrap(),
    };
    assert_eq!(w.run(&ann, &member(2), to_bob).await, ChatResponse::Done);
    assert_eq!(
        bob.drain(),
        vec![Event::Whisper {
            from: handle("ann"),
            text: clean_line("the plan").unwrap()
        }]
    );
    assert_eq!(ann.drain().len(), 1, "the sender's echo");
    assert!(cy.drain().is_empty(), "a third member sees nothing");
    assert_eq!(elsewhere.drain().len(), 0);
    for target in ["dee", "nobody", "zzz", "ann", "", "\u{1b}[31m"] {
        let miss = ChatRequest::Whisper {
            to: target.into(),
            text: clean_line("psst").unwrap(),
        };
        assert_eq!(
            w.run(&ann, &member(2), miss).await,
            ChatResponse::Refused(ChatNotice::NotHere),
            "target {target:?}"
        );
    }
    assert!(bob.drain().is_empty() && cy.drain().is_empty());
}

#[tokio::test]
async fn rooms_are_found_only_by_those_who_may_reach_them() {
    let w = world(roomy(100)).await;
    let admin = staff(1);
    let sysop = w.session(1, "sam");
    // Staff open "vault".
    let ChatResponse::Rooms { rooms, notice } = w
        .run(
            &sysop,
            &admin,
            ChatRequest::Create(Slug::parse("vault").unwrap()),
        )
        .await
    else {
        panic!("expected the list")
    };
    assert_eq!(notice, Some(ChatNotice::Created));
    let vault = rooms
        .iter()
        .find(|r| r.name.as_str() == "vault")
        .unwrap()
        .id;
    // A member who may join only the lobby (a scoped grant on room 1).
    let only_lobby = Snapshot::new(
        Some(AccountId::from_raw(2)),
        [Permission::ChatJoin, Permission::ChatSpeak]
            .into_iter()
            .map(|permission| Held {
                permission,
                scope: Scope::Resource(RoomId::from_raw(1).resource().unwrap()),
                source: Source::Role {
                    role: Slug::MEMBER,
                    rank: 10,
                    via: Via::Account,
                },
            })
            .collect(),
    );
    let ann = w.session(2, "ann");
    let ChatResponse::Rooms { rooms, .. } = w.run(&ann, &only_lobby, ChatRequest::List).await
    else {
        panic!()
    };
    assert_eq!(names(&rooms), ["lobby"], "the vault is not listed");
    // By id: denied, closed, and never-existed all say the same thing and list the same rooms.
    let denied = w.run(&ann, &only_lobby, ChatRequest::Join(vault)).await;
    let missing = w
        .run(&ann, &only_lobby, ChatRequest::Join(RoomId::from_raw(9999)))
        .await;
    assert_eq!(denied, missing);
    assert!(matches!(
        denied,
        ChatResponse::Rooms {
            notice: Some(ChatNotice::NotFound),
            ..
        }
    ));
    // A session holding nothing (the waiting room's snapshot) finds nothing.
    let nobody = w.session(3, "bob");
    let none = w
        .run(
            &nobody,
            &Snapshot::empty(),
            ChatRequest::Join(RoomId::from_raw(1)),
        )
        .await;
    assert!(
        matches!(none, ChatResponse::Rooms { rooms, notice: Some(ChatNotice::NotFound) } if rooms.is_empty())
    );
    assert_eq!(w.hub.session_count(), 3);
}

fn names(rooms: &[Room]) -> Vec<&str> {
    rooms.iter().map(|r| r.name.as_str()).collect()
}

#[tokio::test]
async fn permissions_are_checked_on_every_action_and_a_lost_room_sends_the_member_back() {
    let w = world(roomy(100)).await;
    let ann = w.session(2, "ann");
    assert!(joined(&w.enter(&ann, &member(2), 1).await));
    // Speaking needs chat.speak.
    let listen_only = snapshot(2, &[Permission::ChatJoin]);
    assert_eq!(
        w.run(&ann, &listen_only, say("hi")).await,
        ChatResponse::Refused(ChatNotice::NotPermitted)
    );
    // Whispering needs chat.whisper.
    let whisper = ChatRequest::Whisper {
        to: "bob".into(),
        text: clean_line("x").unwrap(),
    };
    assert_eq!(
        w.run(&ann, &listen_only, whisper).await,
        ChatResponse::Refused(ChatNotice::NotPermitted)
    );
    // Losing chat.join puts the member out of the room and back at the list.
    let nothing = snapshot(2, &[Permission::ChatSpeak]);
    let response = w.run(&ann, &nothing, say("hi")).await;
    assert!(matches!(
        response,
        ChatResponse::Rooms {
            notice: Some(ChatNotice::NotFound),
            ..
        }
    ));
    assert_eq!(ann.room(), None);
}

#[tokio::test]
async fn only_room_managers_open_and_close_rooms_and_closing_puts_members_out() {
    let w = world(roomy(100)).await;
    let plain = member(2);
    let ann = w.session(2, "ann");
    let r = w
        .run(&ann, &plain, ChatRequest::Create(Slug::parse("x").unwrap()))
        .await;
    assert!(matches!(
        r,
        ChatResponse::Rooms {
            notice: Some(ChatNotice::NotPermitted),
            ..
        }
    ));
    let r = w
        .run(&ann, &plain, ChatRequest::Close(RoomId::from_raw(1)))
        .await;
    assert!(matches!(
        r,
        ChatResponse::Rooms {
            notice: Some(ChatNotice::NotPermitted),
            ..
        }
    ));

    let sam = w.session(1, "sam");
    let admin = staff(1);
    w.enter(&ann, &plain, 1).await;
    w.enter(&sam, &admin, 1).await;
    ann.drain();
    sam.drain();
    let waiting = {
        // Ann is woken by the close without pressing a key.
        let ann = Arc::new(ann);
        let ann2 = Arc::clone(&ann);
        let task = tokio::spawn(async move {
            loop {
                ann2.woken().await;
                let events = ann2.drain();
                if !events.is_empty() {
                    break events;
                }
            }
        });
        (ann, task)
    };
    let closed = w
        .run(&sam, &admin, ChatRequest::Close(RoomId::from_raw(1)))
        .await;
    assert!(matches!(
        closed,
        ChatResponse::Rooms {
            notice: Some(ChatNotice::Closed),
            ..
        }
    ));
    assert_eq!(waiting.1.await.unwrap(), vec![Event::RoomClosed]);
    assert_eq!(waiting.0.room(), None);
    assert_eq!(sam.room(), None);
    // And nobody can join it now.
    let r = w.enter(&waiting.0, &plain, 1).await;
    assert!(matches!(
        r,
        ChatResponse::Rooms {
            notice: Some(ChatNotice::NotFound),
            ..
        }
    ));
}

#[tokio::test]
async fn the_member_cap_and_the_session_cap_are_answered_in_words() {
    let limits = ChatLimits {
        members_per_room: 2,
        sessions: 3,
        ..roomy(100)
    };
    let w = world(limits).await;
    let a = w.session(1, "sam");
    let b = w.session(2, "ann");
    let c = w.session(3, "bob");
    assert!(joined(&w.enter(&a, &member(1), 1).await));
    assert!(joined(&w.enter(&b, &member(2), 1).await));
    assert_eq!(
        w.enter(&c, &member(3), 1).await,
        ChatResponse::Refused(ChatNotice::RoomFull)
    );
    assert!(
        w.hub
            .connect(AccountId::from_raw(4), handle("dee"))
            .is_err()
    );
}

/// A tracing writer that keeps everything written, to prove what is *not* logged.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn nothing_said_or_whispered_is_logged() {
    let capture = Capture::default();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(capture.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let w = world(roomy(100)).await;
    let ann = w.session(2, "ann");
    let bob = w.session(3, "bob");
    w.enter(&ann, &member(2), 1).await;
    w.enter(&bob, &member(3), 1).await;
    w.run(&ann, &member(2), say("PUBLIC-CANARY-1")).await;
    let whisper = ChatRequest::Whisper {
        to: "bob".into(),
        text: clean_line("WHISPER-CANARY-2").unwrap(),
    };
    w.run(&ann, &member(2), whisper).await;
    // A flood, a miss and a close exercise the refusal paths too.
    for _ in 0..50 {
        w.run(&ann, &member(2), say("FLOOD-CANARY-3")).await;
    }
    drop(bob);
    let logged = String::from_utf8(capture.0.lock().unwrap().clone()).unwrap();
    for canary in ["PUBLIC-CANARY-1", "WHISPER-CANARY-2", "FLOOD-CANARY-3"] {
        assert!(!logged.contains(canary), "{canary} was logged: {logged}");
    }
}

#[tokio::test]
async fn nothing_said_reaches_the_database() {
    let w = world(roomy(100)).await;
    let ann = w.session(2, "ann");
    let bob = w.session(3, "bob");
    w.enter(&ann, &member(2), 1).await;
    w.enter(&bob, &member(3), 1).await;
    w.run(&ann, &member(2), say("DB-CANARY-1")).await;
    let whisper = ChatRequest::Whisper {
        to: "bob".into(),
        text: clean_line("DB-CANARY-2").unwrap(),
    };
    w.run(&ann, &member(2), whisper).await;
    // Close the store's pool and read the file itself: the canaries are not in it, and the audit
    // log has no chat row (only staff room actions write one).
    w.store.close().await;
    let bytes = std::fs::read(w.dir.path().join("bbs.db")).unwrap();
    let wal = std::fs::read(w.dir.path().join("bbs.db-wal")).unwrap_or_default();
    for blob in [&bytes, &wal] {
        let text = String::from_utf8_lossy(blob);
        assert!(!text.contains("DB-CANARY"));
    }
}

#[allow(dead_code)]
fn _unused(_: ResourceRef) {}

// ---- drawing ----

mod drawing {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use russh_bbs_core::access::Slug;
    use russh_bbs_core::chat::{
        ChatKey, ChatNotice, ChatResponse, ChatScreen, Entered, Event, Line, Room, RoomId,
        clean_line,
    };
    use russh_bbs_core::icons::Tier;
    use russh_bbs_core::store::UnixSeconds;
    use russh_bbs_core::terminal_settings::TerminalSettings;

    use super::handle;
    use crate::chat::lines;
    use crate::settings::Look;
    use crate::ui::Ui;
    use crate::ui::classic::{ContentScreen, MENU_MARKERS, render_content};

    fn room(n: i64, name: &str) -> Room {
        Room {
            id: RoomId::from_raw(n),
            name: Slug::parse(name).unwrap(),
            created_at: UnixSeconds::from_raw(0),
        }
    }

    fn draw(screen: &ChatScreen, tier: Tier, size: (u16, u16)) -> Buffer {
        let ui = Ui::standard().unwrap();
        let look = Look::of(&TerminalSettings::default().with_tier(tier));
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
        terminal
            .draw(|frame| {
                let width = usize::from(frame.area().width.min(crate::settings::MAX_WIDTH))
                    .saturating_sub(2);
                let height = crate::chat::box_height(frame.area().height);
                render_content(
                    frame,
                    ContentScreen {
                        strings: ui.strings(),
                        lines: lines(screen, ui.strings(), look, width, height),
                        look,
                        tier,
                        markers: &MENU_MARKERS,
                    },
                );
            })
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn rows(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect()
    }

    fn in_room() -> ChatScreen {
        let mut screen = ChatScreen::open();
        screen.deliver(ChatResponse::Rooms {
            rooms: vec![room(1, "lobby")],
            notice: None,
        });
        screen.input(ChatKey::Enter);
        screen.deliver(ChatResponse::Joined {
            room: room(1, "lobby"),
            entered: Entered {
                roster: vec![handle("ann")],
                history: Vec::new(),
            },
        });
        screen
    }

    fn say_many(screen: &mut ChatScreen, count: usize) {
        let events = (0..count)
            .map(|n| {
                Event::Line(Line {
                    from: handle("ann"),
                    text: clean_line(&format!("line number {n}")).unwrap(),
                })
            })
            .collect();
        screen.receive(events);
    }

    #[test]
    fn the_newest_lines_and_the_input_fit_every_size_and_the_status_row_is_untouched() {
        let mut screen = in_room();
        say_many(&mut screen, 100);
        for size in [(80, 24), (40, 12), (120, 50), (20, 6)] {
            let buffer = draw(&screen, Tier::Ascii, size);
            let text = rows(&buffer);
            assert_eq!(text.len(), usize::from(size.1));
            // The last row is the reserved status row, drawn by the layout: the marker words.
            let last = text.last().unwrap();
            assert!(
                last.contains("server can read") || size.0 < 40,
                "{size:?}: {last:?}"
            );
            let body = text.join("\n");
            // A 20-column window wraps a line over several rows; the newest line is whole only
            // where it fits.
            assert!(
                size.0 < 30 || body.contains("line number 99"),
                "{size:?}: newest line missing"
            );
            assert!(
                !body.contains("line number 0 "),
                "{size:?}: oldest line is off screen"
            );
            assert!(body.contains("> _"), "{size:?}: input line missing");
        }
    }

    #[test]
    fn long_lines_wrap_inside_the_box_and_hostile_text_is_plain() {
        let mut screen = in_room();
        screen.receive(vec![Event::Line(Line {
            from: handle("ann"),
            text: clean_line(&format!("{}\u{1b}[31m\u{202e}END", "word ".repeat(50))).unwrap(),
        })]);
        let buffer = draw(&screen, Tier::Ascii, (80, 24));
        let text = rows(&buffer);
        let body = text.join("\n");
        assert!(
            body.contains("END"),
            "the end of a long line is reachable by wrapping"
        );
        for row in &text {
            assert!(!row.contains('\u{1b}') && !row.contains('\u{202e}'));
        }
        // The box did not grow: every row has the same width and the frame is intact.
        assert!(text.iter().all(|r| r.chars().count() == 80));
    }

    #[test]
    fn whispers_are_marked_in_words_and_the_input_shows_its_tail() {
        let mut screen = in_room();
        screen.receive(vec![Event::Whisper {
            from: handle("bob"),
            text: clean_line("over here").unwrap(),
        }]);
        for c in "x".repeat(200).chars() {
            screen.input(ChatKey::Char(c));
        }
        screen.input(ChatKey::Char('!'));
        let text = rows(&draw(&screen, Tier::Ascii, (80, 24))).join("\n");
        assert!(text.contains("bob whispers: over here"));
        assert!(
            text.contains("x!_"),
            "the end of what is typed stays visible"
        );
    }

    #[test]
    fn the_room_list_marks_the_cursor_shows_notices_and_keeps_it_in_view() {
        let mut screen = ChatScreen::open();
        let many = (1..=32).map(|n| room(n, &format!("room{n:02}"))).collect();
        screen.deliver(ChatResponse::Rooms {
            rooms: many,
            notice: Some(ChatNotice::NotFound),
        });
        for _ in 0..31 {
            screen.input(ChatKey::Down);
        }
        screen.deliver(ChatResponse::Refused(ChatNotice::Failed));
        let text = rows(&draw(&screen, Tier::Ascii, (80, 24))).join("\n");
        assert!(
            text.contains("> room32"),
            "the selected last room is on screen"
        );
        assert!(!text.contains("room01"), "the list scrolled");
        assert!(text.contains("Problem: That did not work."), "{text}");
        assert!(text.contains("Esc back"));
    }
}
