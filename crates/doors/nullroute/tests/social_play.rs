//! The social screens with two accounts on one board, through the framework's store: a hack
//! filed from the rivals screen is settled by the night and arrives at the next visit; a bounty
//! is posted with a note that went through the framework's text rules; the opt-out; a honeypot.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    clippy::expect_used
)]

mod common;

use std::sync::Arc;

use common::*;
use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey};
use russh_bbs_door_nullroute::Nullroute;
use russh_bbs_door_nullroute::player::Player;
use russh_bbs_door_nullroute::save::{decode, decode_world, encode};
use russh_bbs_door_nullroute::world::World;

const DAY: i64 = 20_300;

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

async fn open(
    store: &MemoryDoorStore,
    door: &Arc<dyn Door>,
    account: AccountId,
    name: &str,
    day: i64,
    seed: u64,
) -> Running {
    let e = DoorEnv::for_tests(name);
    Running::open(
        store,
        Arc::clone(door),
        account,
        &e,
        clock(day) as Arc<dyn Clock>,
        seed,
    )
    .await
    .unwrap()
}

async fn keys(r: &mut Running, store: &MemoryDoorStore, keys: &str) {
    for ch in keys.chars() {
        let key = match ch {
            '\n' => DoorKey::Enter,
            '\u{1b}' => DoorKey::Escape,
            '^' => DoorKey::Up,
            '|' => DoorKey::Down,
            c => DoorKey::Char(c),
        };
        r.key(key);
        r.flush(store).await.unwrap();
    }
}

async fn world(store: &MemoryDoorStore, door: &Arc<dyn Door>) -> World {
    let w = store.load_world(&door.info().id).await.unwrap().unwrap();
    decode_world(&w.bytes).unwrap()
}

async fn player(store: &MemoryDoorStore, door: &Arc<dyn Door>, a: AccountId) -> Player {
    let s = store.load_save(&door.info().id, a).await.unwrap().unwrap();
    decode(&s.bytes).unwrap()
}

async fn board() -> (MemoryDoorStore, Arc<dyn Door>, AccountId, AccountId) {
    let store = MemoryDoorStore::new();
    store.set_now(Day::from_raw(DAY).start());
    let door: Arc<dyn Door> = Nullroute::shared().unwrap();
    let c = content();
    let (ann, bob) = (AccountId::from_raw(1), AccountId::from_raw(2));
    run_due(
        &store,
        door.as_ref(),
        clock(DAY).as_ref(),
        TickPolicy::default(),
    )
    .await
    .unwrap();
    for (account, name, seed, edge_rig) in [
        (ann, "ann", 21u64, [3u8, 0, 0, 3, 0]),
        (bob, "bob", 22, [0; 5]),
    ] {
        let mut p = Player::new(&c, seed, name, 1, DAY - 30);
        p.rep = 12;
        p.marks = 500;
        p.rig = edge_rig;
        p.xp = [40; 5];
        let id = door.info().id.clone();
        store
            .write_save(
                &id,
                account,
                &SaveWrite {
                    expected: Revision::NONE,
                    season: 1,
                    version: 3,
                    bytes: &encode(&p),
                },
            )
            .await
            .unwrap();
    }
    (store, door, ann, bob)
}

#[tokio::test]
async fn a_hack_is_filed_from_the_screen_settled_by_the_night_and_told_at_the_next_visit() {
    let (store, door, ann, bob) = board().await;
    // bob looks in once, so he is on the board.
    let mut r = open(&store, &door, bob, "bob", DAY, 1).await;
    keys(&mut r, &store, "h\u{1b}").await;
    // ann opens the rivals screen: bob is listed, with what a scout sees and nothing hidden.
    let e = DoorEnv::for_tests("ann");
    let mut r = open(&store, &door, ann, "ann", DAY, 2).await;
    keys(&mut r, &store, "v").await;
    let screen = r.look_around(&e).join("\n");
    assert!(screen.contains("RIVALS"), "{screen}");
    assert!(screen.contains("bob"), "{screen}");
    assert!(
        !screen.to_lowercase().contains("honeypot level"),
        "a honeypot is not shown"
    );
    assert!(screen.contains("board can read every post"), "{screen}");
    let cycles = player(&store, &door, ann).await.cycles;
    // bob is a new character in the first week: refused up front, nothing spent.
    // (born DAY - 30 here, so he is not new: the hack is allowed.)
    keys(&mut r, &store, "h").await;
    let after = player(&store, &door, ann).await;
    assert_eq!(after.cycles, cycles - 6);
    let w = world(&store, &door).await;
    assert_eq!(w.social.orders.len(), 1);
    assert_eq!(
        (
            w.social.orders[0].from.as_str(),
            w.social.orders[0].to.as_str()
        ),
        ("ann", "bob")
    );
    assert!(r.look_around(&e).join("\n").contains("filed"));
    // A second hack on the same person is refused by the pair rule, and spends nothing.
    keys(&mut r, &store, "h").await;
    assert_eq!(player(&store, &door, ann).await.cycles, cycles - 6);
    // The night passes: the order is settled against what bob posted.
    store.set_now(Day::from_raw(DAY + 1).start());
    run_due(
        &store,
        door.as_ref(),
        clock(DAY + 1).as_ref(),
        TickPolicy::default(),
    )
    .await
    .unwrap();
    let w = world(&store, &door).await;
    assert!(w.social.orders.is_empty());
    let s = &w.social.tallies;
    assert_eq!(
        s.hacks_in + s.hacks_caught + u32::from(w.social.deliveries.iter().any(|d| d.line == 5)),
        1,
        "{s:?}"
    );
    // Both are told at their next visit, in the board's own template words.
    let mut r = open(&store, &door, bob, "bob", DAY + 1, 3).await;
    let eb = DoorEnv::for_tests("bob");
    let text = r.look_around(&eb).join("\n");
    assert!(
        text.contains("ann got into your hideout") || text.contains("ann probed your hideout"),
        "{text}"
    );
    keys(&mut r, &store, "h\u{1b}").await;
    let mut r = open(&store, &door, ann, "ann", DAY + 1, 4).await;
    let text = r.look_around(&e).join("\n");
    assert!(text.contains("hideout") && text.contains("bob"), "{text}");
    keys(&mut r, &store, "h\u{1b}").await;
    // Told once: the next visit has nothing to fold in.
    let r = open(&store, &door, ann, "ann", DAY + 1, 5).await;
    assert!(
        !r.look_around(&e).join("\n").contains("hideout of"),
        "told once"
    );
}

#[tokio::test]
async fn a_bounty_with_a_note_is_posted_and_the_note_went_through_the_text_rules() {
    let (store, door, ann, bob) = board().await;
    let mut r = open(&store, &door, bob, "bob", DAY, 1).await;
    keys(&mut r, &store, "h\u{1b}").await;
    let e = DoorEnv::for_tests("ann");
    let mut r = open(&store, &door, ann, "ann", DAY, 2).await;
    let marks = player(&store, &door, ann).await.marks;
    keys(&mut r, &store, "vb").await;
    // The note prompt: a hostile line is made plain, one line, within the limit.
    assert!(r.look_around(&e).join("\n").contains("bounty on bob"));
    keys(&mut r, &store, "pay \u{7}\u{202e}up?\n").await;
    let p = player(&store, &door, ann).await;
    assert_eq!(p.marks, marks - 100, "the stake is taken");
    let w = world(&store, &door).await;
    assert_eq!(w.social.bounties.len(), 1);
    let b = &w.social.bounties[0];
    assert_eq!(
        (b.target.as_str(), b.poster.as_str(), b.reward),
        ("bob", "ann", 80)
    );
    assert!(
        !b.note.chars().any(|c| c.is_control() || c == '\u{202e}'),
        "{:?}",
        b.note
    );
    assert!(b.note.contains("pay"));
    // A typed `?` goes into the text: the prompt takes text, so the look-around key is not special.
    assert!(b.note.contains('?'), "{:?}", b.note);
    // bob is told that a price is on him, and sees it on the street.
    let eb = DoorEnv::for_tests("bob");
    let mut r = open(&store, &door, bob, "bob", DAY, 3).await;
    assert!(r.look_around(&eb).join("\n").contains("price on your head"));
    keys(&mut r, &store, "w2").await;
    let text = r.look_around(&eb).join("\n");
    assert!(text.contains("bob") && text.contains("80 marks"), "{text}");
    // A second bounty by the same poster on the same head is refused up front.
    let mut r = open(&store, &door, ann, "ann", DAY, 4).await;
    keys(&mut r, &store, "vb").await;
    assert!(
        r.look_around(&e).join("\n").contains("Not now"),
        "{}",
        r.look_around(&e).join("\n")
    );
    assert_eq!(player(&store, &door, ann).await.marks, marks - 100);
}

#[tokio::test]
async fn opting_out_takes_a_player_off_the_rivals_board_for_everyone() {
    let (store, door, ann, bob) = board().await;
    let mut r = open(&store, &door, bob, "bob", DAY, 1).await;
    keys(&mut r, &store, "vt").await;
    assert!(!player(&store, &door, bob).await.ties.pvp);
    let e = DoorEnv::for_tests("ann");
    let mut r = open(&store, &door, ann, "ann", DAY, 2).await;
    keys(&mut r, &store, "v").await;
    assert!(
        r.look_around(&e)
            .join("\n")
            .contains("Nobody else is on the board"),
        "{}",
        r.look_around(&e).join("\n")
    );
    keys(&mut r, &store, "h").await;
    assert!(world(&store, &door).await.social.orders.is_empty());
    // Bob may not come back at once, and may after two days.
    let eb = DoorEnv::for_tests("bob");
    let mut r = open(&store, &door, bob, "bob", DAY, 3).await;
    keys(&mut r, &store, "vt").await;
    assert!(r.look_around(&eb).join("\n").contains("recently"));
    assert!(!player(&store, &door, bob).await.ties.pvp);
    let mut r = open(&store, &door, bob, "bob", DAY + 2, 4).await;
    keys(&mut r, &store, "vt").await;
    assert!(player(&store, &door, bob).await.ties.pvp);
}

#[tokio::test]
async fn a_honeypot_is_set_from_the_hideout_and_costs_what_it_says() {
    let (store, door, ann, _bob) = board().await;
    let e = DoorEnv::for_tests("ann");
    let mut r = open(&store, &door, ann, "ann", DAY, 1).await;
    let marks = player(&store, &door, ann).await.marks;
    keys(&mut r, &store, "hk").await;
    let p = player(&store, &door, ann).await;
    assert_eq!(p.ties.honeypot, 1);
    assert_eq!(p.marks, marks - 60);
    assert!(
        r.look_around(&e)
            .join("\n")
            .contains("Honeypots: level 1 of 3")
    );
    // The scouts see nothing of it: only the owner's own screen says so, and the posted record
    // carries it for the night's roll alone.
    let w = world(&store, &door).await;
    assert_eq!(w.social.posted_of("ann").unwrap().honeypot, 1);
}
