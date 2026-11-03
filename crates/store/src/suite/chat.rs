//! Chat room definitions against any backend (M3-F, D84, D78): a seeded lobby, audited
//! creation and closing, names unique among open rooms, an atomic cap, nothing about what is
//! said stored anywhere.
#![allow(clippy::unwrap_used, clippy::panic)]

use russh_bbs_core::access::{AccessStore, Permission, PermissionCheck, Slug};
use russh_bbs_core::chat::{CreateRoomError, Room, RoomId, RoomStore};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, StoreError,
};

use super::{Harness, TestStore};

async fn account<S: AccountStore>(store: &S, name: &str, role: Slug) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

fn slug(text: &str) -> Slug {
    Slug::parse(text).unwrap()
}

fn names(rooms: &[Room]) -> Vec<&str> {
    rooms.iter().map(|r| r.name.as_str()).collect()
}

async fn audit_rows<H: Harness>(h: &H) -> Vec<String> {
    h.texts(
        "SELECT action || '|' || actor_kind || '|' || CAST(actor_id AS TEXT) || '|' \
         || resource_kind || '|' || CAST(resource_id AS TEXT) || '|' || detail \
         FROM audit_log WHERE action LIKE 'chat.%' ORDER BY id",
    )
    .await
}

pub(crate) async fn a_new_board_has_a_lobby_and_the_roles_hold_the_chat_permissions<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let rooms = s.open_rooms().await.unwrap();
    assert_eq!(names(&rooms), ["lobby"]);
    assert_eq!(rooms[0].id, RoomId::from_raw(1));

    let member = account(s, "mia", Slug::MEMBER).await;
    let cosysop = account(s, "cory", Slug::COSYSOP).await;
    let sysop = account(s, "sam", Slug::SYSOP).await;
    let held = |snapshot: &russh_bbs_core::access::Snapshot, p| snapshot.allows(p);
    let mia = s.snapshot(Some(member.id)).await.unwrap();
    for p in [
        Permission::ChatJoin,
        Permission::ChatSpeak,
        Permission::ChatWhisper,
    ] {
        assert!(held(&mia, p), "member lacks {}", p.name());
    }
    assert!(!held(&mia, Permission::ChatManageRooms));
    assert!(!held(&mia, Permission::ChatModerate));
    for who in [cosysop.id, sysop.id] {
        let snapshot = s.snapshot(Some(who)).await.unwrap();
        assert!(held(&snapshot, Permission::ChatManageRooms));
        assert!(held(&snapshot, Permission::ChatJoin));
    }
    // The waiting room reaches no room.
    let waiting = s.snapshot(None).await.unwrap();
    for p in [
        Permission::ChatJoin,
        Permission::ChatSpeak,
        Permission::ChatWhisper,
    ] {
        assert!(!held(&waiting, p));
    }
}

pub(crate) async fn creating_and_closing_a_room_are_audited_with_the_name_and_nothing_else<
    H: Harness,
>() {
    let h = H::fresh().await;
    let s = h.store();
    let sam = account(s, "sam", Slug::SYSOP).await;
    let room = s.create_room(&slug("games"), sam.id, 32).await.unwrap();
    assert_eq!(room.name.as_str(), "games");
    assert_eq!(names(&s.open_rooms().await.unwrap()), ["games", "lobby"]);
    assert!(s.close_room(room.id, sam.id).await.unwrap());
    assert_eq!(names(&s.open_rooms().await.unwrap()), ["lobby"]);
    let id = room.id.as_raw();
    assert_eq!(
        audit_rows(&h).await,
        [
            format!("chat.room_created|account|{}|room|{id}|name games", sam.id),
            format!("chat.room_closed|account|{}|room|{id}|", sam.id),
        ]
    );
    // Closing again, or a room that never existed, changes nothing and writes no row.
    assert!(!s.close_room(room.id, sam.id).await.unwrap());
    assert!(!s.close_room(RoomId::from_raw(999), sam.id).await.unwrap());
    assert!(!s.close_room(RoomId::from_raw(0), sam.id).await.unwrap());
    assert_eq!(audit_rows(&h).await.len(), 2);
}

pub(crate) async fn a_name_is_unique_among_open_rooms_and_ids_are_never_reused<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let sam = account(s, "sam", Slug::SYSOP).await;
    assert!(matches!(
        s.create_room(&slug("lobby"), sam.id, 32).await,
        Err(CreateRoomError::NameTaken)
    ));
    let first = s.create_room(&slug("games"), sam.id, 32).await.unwrap();
    assert!(s.close_room(first.id, sam.id).await.unwrap());
    let second = s.create_room(&slug("games"), sam.id, 32).await.unwrap();
    assert_ne!(
        first.id, second.id,
        "a closed room's id is never handed out again"
    );
    assert!(second.id > first.id);
    // A failed creation leaves no audit row behind.
    let before = audit_rows(&h).await.len();
    assert!(s.create_room(&slug("games"), sam.id, 32).await.is_err());
    assert_eq!(audit_rows(&h).await.len(), before);
}

pub(crate) async fn the_room_cap_refuses_and_concurrent_creations_never_pass_it<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let sam = account(s, "sam", Slug::SYSOP).await;
    // One room (the lobby) is open; room for three more.
    let mut tasks = Vec::new();
    for n in 0..10 {
        let s = s.clone();
        let who = sam.id;
        tasks.push(tokio::spawn(async move {
            s.create_room(&slug(&format!("r{n}")), who, 4).await
        }));
    }
    let mut made = 0;
    let mut refused = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => made += 1,
            Err(CreateRoomError::TooMany) => refused += 1,
            Err(other) => panic!("unexpected {other}"),
        }
    }
    assert_eq!((made, refused), (3, 7));
    assert_eq!(s.open_rooms().await.unwrap().len(), 4);
    // Closing one frees a place.
    let some = s.open_rooms().await.unwrap().remove(0);
    assert!(s.close_room(some.id, sam.id).await.unwrap());
    s.create_room(&slug("again"), sam.id, 4).await.unwrap();
}

pub(crate) async fn an_unknown_actor_is_refused_and_nothing_is_written<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    assert!(matches!(
        s.create_room(&slug("ghosts"), AccountId::from_raw(4242), 32)
            .await,
        Err(CreateRoomError::Store(StoreError::AccountNotFound))
    ));
    assert_eq!(names(&s.open_rooms().await.unwrap()), ["lobby"]);
    assert_eq!(audit_rows(&h).await.len(), 0);
}

pub(crate) async fn room_writes_never_land_without_their_audit_row<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let sam = account(s, "sam", Slug::SYSOP).await;
    h.break_audit_inserts().await;
    assert!(s.create_room(&slug("games"), sam.id, 32).await.is_err());
    assert_eq!(names(&s.open_rooms().await.unwrap()), ["lobby"]);
    assert!(s.close_room(RoomId::from_raw(1), sam.id).await.is_err());
    assert_eq!(
        names(&s.open_rooms().await.unwrap()),
        ["lobby"],
        "the room is still open: the close rolled back"
    );
}

pub(crate) async fn the_database_refuses_names_the_code_would_never_write<H: Harness>() {
    let h = H::fresh().await;
    let long = "x".repeat(33);
    assert!(
        h.raw(&format!(
            "INSERT INTO chat_room (name, created_at) VALUES ('{long}', 1)"
        ))
        .await
        .is_err()
    );
    assert!(
        h.raw("INSERT INTO chat_room (name, created_at) VALUES ('', 1)")
            .await
            .is_err()
    );
    // Closing before creation is refused.
    assert!(
        h.raw("INSERT INTO chat_room (name, created_at, closed_at) VALUES ('t', 10, 5)")
            .await
            .is_err()
    );
}

pub(crate) async fn a_stored_name_the_code_could_not_have_written_is_corrupt_not_shown<
    H: Harness,
>() {
    let h = H::fresh().await;
    h.raw("INSERT INTO chat_room (name, created_at) VALUES ('Bad Name', 1)")
        .await
        .unwrap();
    assert!(matches!(
        h.store().open_rooms().await,
        Err(StoreError::Corrupt(_))
    ));
}

pub(crate) async fn nothing_a_member_says_has_a_table_to_land_in<H: Harness>() {
    let h = H::fresh().await;
    // The only chat table is the room definition. No table for lines or whispers exists under
    // any name the code could have meant (D78); `chat_room` has no text column besides the name.
    for table in [
        "chat_line",
        "chat_message",
        "chat_whisper",
        "chat_history",
        "whisper",
    ] {
        assert!(
            h.raw(&format!("SELECT 1 FROM {table}")).await.is_err(),
            "{table} exists"
        );
    }
    h.raw("SELECT id, name, created_at, created_by, closed_at FROM chat_room")
        .await
        .unwrap();
}

pub(crate) async fn rooms_survive_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let sam = account(h.store(), "sam", Slug::SYSOP).await;
    h.store()
        .create_room(&slug("games"), sam.id, 32)
        .await
        .unwrap();
    h.reopen().await;
    assert_eq!(
        names(&h.store().open_rooms().await.unwrap()),
        ["games", "lobby"]
    );
}

pub(crate) async fn a_broken_store_errors_instead_of_listing_no_rooms<H: Harness>() {
    let h = H::fresh().await;
    h.close().await;
    assert!(h.store().open_rooms().await.is_err());
}

#[allow(dead_code)]
fn _assert_test_store<S: TestStore>() {}
