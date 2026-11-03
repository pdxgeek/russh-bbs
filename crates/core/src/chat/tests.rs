//! Hub rules, the screen and the room service, with a manual clock and an in-memory store.
//! No sleeps, no runtime for the hub.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::unused_async_trait_impl,
    clippy::assert_is_empty
)]

use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::access::{
    Held, Permission, ResourceId, ResourceKind, ResourceRef, Scope, Slug, Snapshot, Source, Via,
};
use crate::sanitize::{Username, sanitize_username};
use crate::store::{AccountId, StoreError, UnixSeconds};
use crate::waiting_room::{ManualClock, RatePolicy};

fn name(text: &str) -> Username {
    sanitize_username(text).unwrap()
}

fn room_id(n: i64) -> RoomId {
    RoomId::from_raw(n)
}

fn snapshot(account: i64, held: &[(Permission, Scope)]) -> Snapshot {
    Snapshot::new(
        Some(AccountId::from_raw(account)),
        held.iter()
            .map(|(permission, scope)| Held {
                permission: *permission,
                scope: *scope,
                source: Source::Role {
                    role: Slug::MEMBER,
                    rank: 10,
                    via: Via::Account,
                },
            })
            .collect(),
    )
}

fn member(account: i64) -> Snapshot {
    snapshot(
        account,
        &[
            (Permission::ChatJoin, Scope::Global),
            (Permission::ChatSpeak, Scope::Global),
            (Permission::ChatWhisper, Scope::Global),
        ],
    )
}

fn proof(snap: &Snapshot, permission: Permission, room: RoomId) -> crate::access::Granted {
    snap.require(permission, room.resource()).unwrap()
}

fn text(s: &str) -> crate::sanitize::Sanitized {
    clean_line(s).unwrap()
}

fn limits() -> ChatLimits {
    ChatLimits {
        rate: RatePolicy::new(3, Duration::from_secs(1)),
        scrollback: 4,
        queue: 4,
        members_per_room: 3,
        sessions: 6,
    }
}

struct Fixture {
    chat: Chat<ManualClock>,
    clock: ManualClock,
}

fn fixture() -> Fixture {
    let clock = ManualClock::new();
    Fixture {
        chat: Chat::new(limits(), clock.clone()),
        clock,
    }
}

/// A session for `account` named `handle`, joined to `room`.
fn seat(f: &mut Fixture, account: i64, handle: &str, room: RoomId) -> (SessionId, Snapshot) {
    let snap = member(account);
    let id = f
        .chat
        .connect(AccountId::from_raw(account), name(handle))
        .unwrap();
    f.chat
        .join(&proof(&snap, Permission::ChatJoin, room), id)
        .unwrap();
    (id, snap)
}

#[test]
fn a_line_reaches_everyone_in_the_room_and_only_them() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (ann, ann_snap) = seat(&mut f, 1, "ann", lobby);
    let (bob, _) = seat(&mut f, 2, "bob", lobby);
    let (cy, _) = seat(&mut f, 3, "cy", room_id(2));
    f.chat.drain(ann);
    f.chat.drain(bob);
    f.chat.drain(cy);

    let touched = f
        .chat
        .say(
            &proof(&ann_snap, Permission::ChatSpeak, lobby),
            ann,
            text("hello"),
        )
        .unwrap();
    assert!(touched.contains(&ann) && touched.contains(&bob) && !touched.contains(&cy));
    for who in [ann, bob] {
        let events = f.chat.drain(who);
        assert_eq!(events.len(), 1);
        let Event::Line(line) = &events[0] else {
            panic!("not a line: {events:?}")
        };
        assert_eq!((line.from.as_str(), line.text.as_str()), ("ann", "hello"));
    }
    assert!(f.chat.drain(cy).is_empty());
}

#[test]
fn control_characters_and_markers_never_reach_a_line() {
    let line = clean_line("hi\u{1b}[31m red \u{9b}2J\u{e000}\u{301a}x").unwrap();
    assert!(!line.as_str().contains('\u{1b}'));
    assert!(!line.as_str().contains('\u{9b}'));
    assert!(!line.as_str().contains('\u{e000}'));
    assert!(clean_line("   \u{1b}\u{7}  ").is_none());
    let long = "a".repeat(5000);
    assert_eq!(
        clean_line(&long).unwrap().as_str().chars().count(),
        MAX_LINE_CHARS
    );
}

#[test]
fn joining_and_leaving_is_announced_and_the_roster_follows() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (ann, _) = seat(&mut f, 1, "ann", lobby);
    f.chat.drain(ann);
    let (bob, _) = seat(&mut f, 2, "bob", lobby);
    assert_eq!(f.chat.drain(ann), vec![Event::Joined(name("bob"))]);
    let names: Vec<_> = f
        .chat
        .roster(ann)
        .iter()
        .map(|n| n.as_str().to_owned())
        .collect();
    assert_eq!(names, ["ann", "bob"]);
    let touched = f.chat.disconnect(bob);
    assert_eq!(touched, vec![ann]);
    assert_eq!(f.chat.drain(ann), vec![Event::Left(name("bob"))]);
    assert_eq!(f.chat.session_count(), 1);
    // Disconnecting twice is harmless.
    assert!(f.chat.disconnect(bob).is_empty());
}

#[test]
fn one_account_in_two_sessions_is_listed_once() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (a1, _) = seat(&mut f, 1, "ann", lobby);
    seat(&mut f, 1, "ann", lobby);
    assert_eq!(f.chat.roster(a1).len(), 1);
}

#[test]
fn a_late_joiner_gets_the_scrollback_capped_at_the_limit() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (ann, snap) = seat(&mut f, 1, "ann", lobby);
    for n in 0..4 {
        f.chat
            .say(
                &proof(&snap, Permission::ChatSpeak, lobby),
                ann,
                text(&format!("l{n}")),
            )
            .unwrap();
        f.clock.advance(Duration::from_secs(1));
    }
    f.chat
        .say(&proof(&snap, Permission::ChatSpeak, lobby), ann, text("l4"))
        .unwrap();
    assert_eq!(f.chat.scrollback_len(lobby), 4);
    let bob = f.chat.connect(AccountId::from_raw(2), name("bob")).unwrap();
    let (entered, _) = f
        .chat
        .join(&proof(&member(2), Permission::ChatJoin, lobby), bob)
        .unwrap();
    let said: Vec<_> = entered
        .history
        .iter()
        .map(|l| l.text.as_str().to_owned())
        .collect();
    assert_eq!(said, ["l1", "l2", "l3", "l4"]);
}

#[test]
fn a_flood_is_refused_and_nothing_is_sent() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (ann, snap) = seat(&mut f, 1, "ann", lobby);
    let (bob, _) = seat(&mut f, 2, "bob", lobby);
    f.chat.drain(ann);
    f.chat.drain(bob);
    let speak = proof(&snap, Permission::ChatSpeak, lobby);
    for n in 0..3 {
        f.chat.say(&speak, ann, text(&format!("m{n}"))).unwrap();
    }
    let refused = f.chat.say(&speak, ann, text("m3")).unwrap_err();
    assert!(matches!(refused, SayError::RateLimited { .. }));
    assert_eq!(
        f.chat.drain(bob).len(),
        3,
        "the refused line reached nobody"
    );
    // A whisper spends the same allowance.
    assert!(matches!(
        f.chat.whisper(
            &proof(&snap, Permission::ChatWhisper, lobby),
            ann,
            "bob",
            text("psst")
        ),
        Err(WhisperError::RateLimited { .. })
    ));
    // The allowance comes back with time.
    f.clock.advance(Duration::from_secs(1));
    f.chat.say(&speak, ann, text("again")).unwrap();
}

#[test]
fn the_limit_is_per_session() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (ann, a) = seat(&mut f, 1, "ann", lobby);
    let (bob, b) = seat(&mut f, 2, "bob", lobby);
    for _ in 0..3 {
        f.chat
            .say(&proof(&a, Permission::ChatSpeak, lobby), ann, text("x"))
            .unwrap();
    }
    f.chat
        .say(&proof(&b, Permission::ChatSpeak, lobby), bob, text("y"))
        .unwrap();
}

#[test]
fn a_whisper_reaches_its_target_and_the_sender_only() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (ann, a) = seat(&mut f, 1, "ann", lobby);
    let (bob, _) = seat(&mut f, 2, "bob", lobby);
    let (cy, _) = seat(&mut f, 3, "cy", lobby);
    for s in [ann, bob, cy] {
        f.chat.drain(s);
    }
    let touched = f
        .chat
        .whisper(
            &proof(&a, Permission::ChatWhisper, lobby),
            ann,
            "BOB",
            text("secret"),
        )
        .unwrap();
    assert!(touched.contains(&bob) && touched.contains(&ann) && !touched.contains(&cy));
    assert_eq!(
        f.chat.drain(bob),
        vec![Event::Whisper {
            from: name("ann"),
            text: text("secret")
        }]
    );
    assert_eq!(
        f.chat.drain(ann),
        vec![Event::WhisperSent {
            to: name("bob"),
            text: text("secret")
        }]
    );
    assert!(f.chat.drain(cy).is_empty(), "a third member sees nothing");
    // And it is not in the scrollback a newcomer gets.
    assert_eq!(f.chat.scrollback_len(lobby), 0);
}

#[test]
fn a_whisper_to_anyone_not_here_gets_one_answer() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (ann, a) = seat(&mut f, 1, "ann", lobby);
    seat(&mut f, 2, "bob", room_id(2)); // exists, but in another room
    let w = proof(&a, Permission::ChatWhisper, lobby);
    for target in ["bob", "nobody", "x", "", "\u{1b}[2J", "ann"] {
        assert_eq!(
            f.chat.whisper(&w, ann, target, text("hi")).unwrap_err(),
            WhisperError::NotHere,
            "target {target:?}"
        );
    }
}

#[test]
fn a_slow_session_loses_its_oldest_events_and_is_told_how_many() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (ann, a) = seat(&mut f, 1, "ann", lobby);
    let (bob, _) = seat(&mut f, 2, "bob", lobby);
    f.chat.drain(bob);
    // bob never drains. ann says more than bob's queue holds (rate allows 3 per second).
    for n in 0..10 {
        f.chat
            .say(
                &proof(&a, Permission::ChatSpeak, lobby),
                ann,
                text(&format!("m{n}")),
            )
            .unwrap();
        f.clock.advance(Duration::from_secs(1));
    }
    let events = f.chat.drain(bob);
    assert_eq!(events[0], Event::Missed(6));
    assert_eq!(
        events.len(),
        5,
        "the missed marker and the queue's four newest"
    );
    let Event::Line(last) = events.last().unwrap() else {
        panic!()
    };
    assert_eq!(last.text.as_str(), "m9");
    // The room was never blocked: ann's own queue is a separate one and was drained as needed.
    assert!(f.chat.has_events(ann));
}

#[test]
fn the_caps_on_members_sessions_and_rooms_hold() {
    let mut f = fixture();
    let lobby = room_id(1);
    for n in 1..=3 {
        seat(&mut f, n, &format!("u{n}"), lobby);
    }
    let full = f.chat.connect(AccountId::from_raw(4), name("u4")).unwrap();
    assert_eq!(
        f.chat
            .join(&proof(&member(4), Permission::ChatJoin, lobby), full)
            .unwrap_err(),
        JoinError::Full
    );
    // The refused session can still join another room.
    f.chat
        .join(&proof(&member(4), Permission::ChatJoin, room_id(2)), full)
        .unwrap();
    f.chat.connect(AccountId::from_raw(5), name("u5")).unwrap();
    f.chat.connect(AccountId::from_raw(6), name("u6")).unwrap();
    assert_eq!(
        f.chat
            .connect(AccountId::from_raw(7), name("u7"))
            .unwrap_err(),
        JoinError::Busy
    );
    // Room runtime state is bounded by MAX_ROOMS.
    let mut big = Chat::new(
        ChatLimits {
            sessions: 100,
            ..limits()
        },
        ManualClock::new(),
    );
    for n in 1..=i64::try_from(MAX_ROOMS).unwrap() {
        let id = big
            .connect(AccountId::from_raw(n), name(&format!("m{n}")))
            .unwrap();
        big.join(&proof(&member(n), Permission::ChatJoin, room_id(n)), id)
            .unwrap();
    }
    let extra = big.connect(AccountId::from_raw(99), name("extra")).unwrap();
    assert_eq!(
        big.join(
            &proof(&member(99), Permission::ChatJoin, room_id(1000)),
            extra
        )
        .unwrap_err(),
        JoinError::Busy
    );
    assert_eq!(big.room_count(), MAX_ROOMS);
}

#[test]
fn a_proof_for_another_room_account_or_permission_does_nothing() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (ann, a) = seat(&mut f, 1, "ann", lobby);
    let (_bob, b) = seat(&mut f, 2, "bob", lobby);
    // Speak proof for another room.
    assert_eq!(
        f.chat
            .say(
                &proof(&a, Permission::ChatSpeak, room_id(2)),
                ann,
                text("x")
            )
            .unwrap_err(),
        SayError::NotInRoom
    );
    // Someone else's proof.
    assert_eq!(
        f.chat
            .say(&proof(&b, Permission::ChatSpeak, lobby), ann, text("x"))
            .unwrap_err(),
        SayError::NotInRoom
    );
    // A join proof is not a speak proof.
    assert_eq!(
        f.chat
            .say(&proof(&a, Permission::ChatJoin, lobby), ann, text("x"))
            .unwrap_err(),
        SayError::NotInRoom
    );
    // A speak proof is not a join proof.
    let id = f.chat.connect(AccountId::from_raw(1), name("ann")).unwrap();
    assert_eq!(
        f.chat
            .join(&proof(&a, Permission::ChatSpeak, lobby), id)
            .unwrap_err(),
        JoinError::NotFound
    );
    // A global proof (no resource) names no room.
    let global = a.require(Permission::ChatJoin, None).unwrap();
    assert_eq!(f.chat.join(&global, id).unwrap_err(), JoinError::NotFound);
}

#[test]
fn closing_a_room_puts_everyone_out_and_forgets_it() {
    let mut f = fixture();
    let lobby = room_id(1);
    let (ann, a) = seat(&mut f, 1, "ann", lobby);
    f.chat
        .say(&proof(&a, Permission::ChatSpeak, lobby), ann, text("x"))
        .unwrap();
    f.chat.drain(ann);
    assert_eq!(f.chat.close_room(lobby), vec![ann]);
    assert_eq!(f.chat.drain(ann), vec![Event::RoomClosed]);
    assert_eq!(f.chat.scrollback_len(lobby), 0);
    assert_eq!(
        f.chat
            .say(&proof(&a, Permission::ChatSpeak, lobby), ann, text("x"))
            .unwrap_err(),
        SayError::NotInRoom
    );
}

#[test]
fn switching_rooms_leaves_the_first() {
    let mut f = fixture();
    let (ann, a) = seat(&mut f, 1, "ann", room_id(1));
    let (bob, _) = seat(&mut f, 2, "bob", room_id(1));
    f.chat.drain(bob);
    f.chat
        .join(&proof(&a, Permission::ChatJoin, room_id(2)), ann)
        .unwrap();
    assert_eq!(f.chat.drain(bob), vec![Event::Left(name("ann"))]);
    assert_eq!(f.chat.member_count(room_id(1)), 1);
    assert_eq!(f.chat.member_count(room_id(2)), 1);
}

// ---- the room list and staff actions ----

#[derive(Default)]
struct FakeRooms(Mutex<Vec<(i64, String, bool)>>);

impl RoomStore for FakeRooms {
    async fn create_room(
        &self,
        name: &Slug,
        _actor: AccountId,
        max_rooms: usize,
    ) -> Result<Room, CreateRoomError> {
        let mut rooms = self.0.lock().unwrap();
        let open = rooms.iter().filter(|r| r.2).count();
        if rooms.iter().any(|r| r.2 && r.1 == name.as_str()) {
            return Err(CreateRoomError::NameTaken);
        }
        if open >= max_rooms {
            return Err(CreateRoomError::TooMany);
        }
        let id = i64::try_from(rooms.len()).unwrap() + 1;
        rooms.push((id, name.as_str().to_owned(), true));
        Ok(Room {
            id: RoomId::from_raw(id),
            name: name.clone(),
            created_at: UnixSeconds::from_raw(0),
        })
    }

    async fn close_room(&self, id: RoomId, _actor: AccountId) -> Result<bool, StoreError> {
        let mut rooms = self.0.lock().unwrap();
        Ok(rooms
            .iter_mut()
            .find(|r| r.0 == id.as_raw() && r.2)
            .map(|r| r.2 = false)
            .is_some())
    }

    async fn open_rooms(&self) -> Result<Vec<Room>, StoreError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r.2)
            .map(|r| Room {
                id: RoomId::from_raw(r.0),
                name: Slug::parse(&r.1).unwrap(),
                created_at: UnixSeconds::from_raw(0),
            })
            .collect())
    }
}

fn slug(s: &str) -> Slug {
    Slug::parse(s).unwrap()
}

fn staff() -> Snapshot {
    snapshot(
        9,
        &[
            (Permission::ChatJoin, Scope::Global),
            (Permission::ChatManageRooms, Scope::Global),
        ],
    )
}

#[tokio::test]
async fn the_list_and_join_by_id_follow_the_session_s_reach() {
    let store = FakeRooms::default();
    let admin = staff();
    for n in ["lobby", "staff", "games"] {
        create_room(&store, &admin, &slug(n)).await.unwrap();
    }
    // This member may join room 1 and, by an allow entry, room 3 only.
    let scoped = |n| {
        (
            Permission::ChatJoin,
            Scope::Resource(ResourceRef {
                kind: ResourceKind::Room,
                id: ResourceId::new(n).unwrap(),
            }),
        )
    };
    let limited = snapshot(5, &[scoped(1), scoped(3)]);
    let names: Vec<_> = joinable_rooms(&store, &limited)
        .await
        .unwrap()
        .iter()
        .map(|r| r.name.as_str().to_owned())
        .collect();
    assert_eq!(names, ["lobby", "games"]);
    require_join(&store, &limited, room_id(1)).await.unwrap();
    // Denied by id, closed, and never-existed are the same answer.
    for id in [2, 99, 0, -1] {
        assert!(matches!(
            require_join(&store, &limited, room_id(id)).await,
            Err(RoomError::NotFound)
        ));
    }
    close_room(&store, &admin, room_id(1)).await.unwrap();
    assert!(matches!(
        require_join(&store, &limited, room_id(1)).await,
        Err(RoomError::NotFound)
    ));
    // A session holding nothing sees an empty list.
    assert!(
        joinable_rooms(&store, &Snapshot::empty())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn only_room_managers_open_and_close_rooms() {
    let store = FakeRooms::default();
    let plain = member(1);
    assert!(matches!(
        create_room(&store, &plain, &slug("x")).await,
        Err(RoomError::Denied)
    ));
    assert!(matches!(
        close_room(&store, &plain, room_id(1)).await,
        Err(RoomError::Denied)
    ));
    assert!(matches!(
        create_room(&store, &Snapshot::empty(), &slug("x")).await,
        Err(RoomError::Denied)
    ));
    let admin = staff();
    create_room(&store, &admin, &slug("x")).await.unwrap();
    assert!(matches!(
        create_room(&store, &admin, &slug("x")).await,
        Err(RoomError::NameTaken)
    ));
    assert!(close_room(&store, &admin, room_id(1)).await.unwrap());
    assert!(!close_room(&store, &admin, room_id(1)).await.unwrap());
}

#[tokio::test]
async fn the_room_cap_is_passed_to_the_store() {
    let store = FakeRooms::default();
    let admin = staff();
    for n in 0..MAX_ROOMS {
        create_room(&store, &admin, &slug(&format!("r{n}")))
            .await
            .unwrap();
    }
    assert!(matches!(
        create_room(&store, &admin, &slug("one-more")).await,
        Err(RoomError::TooMany)
    ));
}

// ---- the screen ----

fn room(n: i64, name: &str) -> Room {
    Room {
        id: room_id(n),
        name: slug(name),
        created_at: UnixSeconds::from_raw(0),
    }
}

fn type_str(screen: &mut ChatScreen, s: &str) {
    for c in s.chars() {
        screen.input(ChatKey::Char(c));
    }
}

fn in_a_room() -> ChatScreen {
    let mut screen = ChatScreen::open();
    screen.deliver(ChatResponse::Rooms {
        rooms: vec![room(1, "lobby"), room(2, "games")],
        notice: None,
    });
    screen.input(ChatKey::Down);
    assert_eq!(
        screen.input(ChatKey::Enter),
        ChatEffect::Request(ChatRequest::Join(room_id(2)))
    );
    screen.deliver(ChatResponse::Joined {
        room: room(2, "games"),
        entered: Entered {
            roster: vec![name("ann")],
            history: Vec::new(),
        },
    });
    screen
}

#[test]
fn the_list_moves_joins_and_backs_out() {
    let mut screen = ChatScreen::open();
    assert_eq!(
        screen.input(ChatKey::Enter),
        ChatEffect::Stay,
        "an empty list joins nothing"
    );
    screen.deliver(ChatResponse::Rooms {
        rooms: vec![room(1, "lobby"), room(2, "games")],
        notice: None,
    });
    screen.input(ChatKey::Down);
    screen.input(ChatKey::Down);
    assert_eq!(
        screen.input(ChatKey::Enter),
        ChatEffect::Request(ChatRequest::Join(room_id(2)))
    );
    screen.input(ChatKey::Up);
    assert_eq!(
        screen.input(ChatKey::Enter),
        ChatEffect::Request(ChatRequest::Join(room_id(1)))
    );
    assert_eq!(screen.input(ChatKey::Escape), ChatEffect::Exit);
}

#[test]
fn room_management_keys_exist_only_for_managers() {
    let mut screen = ChatScreen::open();
    screen.deliver(ChatResponse::Rooms {
        rooms: vec![room(1, "lobby")],
        notice: None,
    });
    type_str(&mut screen, "nx");
    let ChatPage::List(list) = screen.page() else {
        panic!()
    };
    assert!(list.naming.is_none() && list.closing.is_none());

    screen.set_manage(true);
    screen.input(ChatKey::Char('n'));
    type_str(&mut screen, "Games-2");
    assert_eq!(
        screen.input(ChatKey::Enter),
        ChatEffect::Request(ChatRequest::Create(slug("games-2")))
    );
    screen.input(ChatKey::Char('n'));
    type_str(&mut screen, "bad name!");
    assert_eq!(
        screen.input(ChatKey::Enter),
        ChatEffect::Request(ChatRequest::List)
    );
    // Close asks first; only y confirms.
    screen.input(ChatKey::Char('x'));
    assert_eq!(screen.input(ChatKey::Char('n')), ChatEffect::Stay);
    screen.input(ChatKey::Char('x'));
    assert_eq!(
        screen.input(ChatKey::Char('y')),
        ChatEffect::Request(ChatRequest::Close(room_id(1)))
    );
    // Losing the permission closes the prompts.
    screen.input(ChatKey::Char('n'));
    screen.set_manage(false);
    let ChatPage::List(list) = screen.page() else {
        panic!()
    };
    assert!(list.naming.is_none());
}

#[test]
fn typing_sends_a_cleaned_line_and_commands_are_not_lines() {
    let mut screen = in_a_room();
    type_str(&mut screen, "hello \u{1b}[31mthere");
    let ChatEffect::Request(ChatRequest::Say(line)) = screen.input(ChatKey::Enter) else {
        panic!()
    };
    assert!(!line.as_str().contains('\u{1b}'));
    // A blank line sends nothing.
    type_str(&mut screen, "   ");
    assert_eq!(screen.input(ChatKey::Enter), ChatEffect::Stay);

    type_str(&mut screen, "/w Bob   psst hello");
    let ChatEffect::Request(ChatRequest::Whisper { to, text }) = screen.input(ChatKey::Enter)
    else {
        panic!()
    };
    assert_eq!((to.as_str(), text.as_str()), ("Bob", "psst hello"));
    type_str(&mut screen, "/w bob");
    assert_eq!(
        screen.input(ChatKey::Enter),
        ChatEffect::Stay,
        "usage hint, nothing sent"
    );
    type_str(&mut screen, "/who");
    assert_eq!(
        screen.input(ChatKey::Enter),
        ChatEffect::Request(ChatRequest::Who)
    );
    type_str(&mut screen, "/nonsense");
    assert_eq!(screen.input(ChatKey::Enter), ChatEffect::Stay);
    type_str(&mut screen, "/leave");
    assert_eq!(
        screen.input(ChatKey::Enter),
        ChatEffect::Request(ChatRequest::Leave)
    );
    assert_eq!(
        screen.input(ChatKey::Escape),
        ChatEffect::Request(ChatRequest::Leave)
    );
}

#[test]
fn an_unsent_line_is_gone_with_the_screen_and_the_input_is_bounded() {
    let mut screen = in_a_room();
    type_str(&mut screen, &"x".repeat(5000));
    let ChatPage::Room(page) = screen.page() else {
        panic!()
    };
    assert_eq!(page.input.chars().count(), INPUT_MAX_CHARS);
    drop(screen); // a connection that ends mid-line: nothing was ever requested
}

#[test]
fn events_become_rows_the_transcript_is_bounded_and_a_closed_room_asks_for_the_list() {
    let mut screen = in_a_room();
    let events = (0..TRANSCRIPT_ROWS + 50)
        .map(|n| {
            Event::Line(Line {
                from: name("ann"),
                text: text(&format!("m{n}")),
            })
        })
        .collect();
    assert_eq!(screen.receive(events), None);
    let ChatPage::Room(page) = screen.page() else {
        panic!()
    };
    assert_eq!(page.rows.len(), TRANSCRIPT_ROWS);
    assert_eq!(
        screen.receive(vec![Event::RoomClosed]),
        Some(ChatRequest::List)
    );
}

#[test]
fn a_refusal_is_a_notice_that_the_next_key_clears() {
    let mut screen = in_a_room();
    screen.deliver(ChatResponse::Refused(ChatNotice::NotHere));
    assert_eq!(screen.notice(), Some(ChatNotice::NotHere));
    screen.input(ChatKey::Char('a'));
    assert_eq!(screen.notice(), None);
}

#[test]
fn nothing_in_the_chat_texts_calls_anything_private() {
    let mut all = vec![
        strings::HEADING_ROOMS,
        strings::NO_ROOMS,
        strings::FOOTER_ROOMS,
        strings::FOOTER_STAFF,
        strings::FOOTER_ROOM,
        strings::PROMPT_NEW,
        strings::CONFIRM_CLOSE,
        strings::HERE,
        strings::YOU_ARE_IN,
        strings::WHISPER_FROM,
        strings::WHISPER_TO,
    ];
    all.extend(strings::HELP);
    let notices = [
        ChatNotice::NotFound,
        ChatNotice::NotPermitted,
        ChatNotice::RoomFull,
        ChatNotice::ChatFull,
        ChatNotice::SlowDown,
        ChatNotice::NotHere,
        ChatNotice::Failed,
        ChatNotice::BadName,
        ChatNotice::NameTaken,
        ChatNotice::TooManyRooms,
        ChatNotice::Created,
        ChatNotice::Closed,
    ];
    all.extend(notices.iter().map(|n| n.text().1));
    for text in all {
        let lower = text.to_lowercase();
        for word in ["private", "secure", "encrypt", "confidential"] {
            assert!(!lower.contains(word), "{text:?} says {word}");
        }
    }
}
