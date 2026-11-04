//! The shared world: a big scheme pulled off by one player changes the school for every player on
//! the board, through the framework's real world edit and daily tick.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]
mod common;

use std::sync::Arc;

use common::{Sandbox, Setup, arrange, busted, monday, pulled, run_scheme};
use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey};
use russh_bbs_door_hallpass::game::{Action, Game};
use russh_bbs_door_hallpass::save::{decode_world, encode_player};
use russh_bbs_door_hallpass::state::{Player, World, map_set};
use russh_bbs_door_hallpass::{Hallpass, SAVE_VERSION};

/// A strong character with a big scheme set up and timed, standing at the place at the moment.
fn big_scheme_player(
    method: &str,
    target: &str,
    allies: Vec<(&'static str, &'static str)>,
) -> Player {
    let mut sb = Sandbox::new("smooth_talker", 1);
    for s in ["sneak", "bluff", "tinker", "fleet"] {
        map_set(&mut sb.game.player.xp, s, 40);
    }
    sb.game.player.legend = 40;
    sb.game.player.cool = 90;
    let id = arrange(
        &mut sb,
        target,
        method,
        &Setup {
            scout: 3,
            allies,
            ..Setup::default()
        },
    );
    let scheme = sb.game.player.scheme(id).unwrap().clone();
    let (day, slot) = scheme.when.unwrap();
    sb.game.sync_day(day);
    sb.game.player.slot = slot;
    sb.game.player.place = sb.game.scheme_place(&scheme).unwrap();
    sb.game.player.name = "ann".into();
    sb.game.player.clone()
}

fn keys(text: &str) -> Vec<DoorKey> {
    text.chars().map(DoorKey::Char).collect()
}

async fn store_save(store: &MemoryDoorStore, door: &dyn Door, account: i64, player: &Player) {
    store
        .write_save(
            &door.info().id,
            AccountId::from_raw(account),
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: SAVE_VERSION,
                bytes: &encode_player(player),
            },
        )
        .await
        .unwrap();
}

/// Run the player's single scheme through the screen, trying seeds until it is pulled off.
async fn pull_it_off(
    store: &MemoryDoorStore,
    door: &Arc<dyn Door>,
    player: &Player,
    account: i64,
    clock: &Arc<ManualClock>,
) -> bool {
    let env = DoorEnv::for_tests("ann");
    for seed in 0..60 {
        let fresh = MemoryDoorStore::new();
        // Each try starts from the same stored world and save.
        if let Some(world) = store.load_world(&door.info().id).await.unwrap() {
            fresh
                .write_world(
                    &door.info().id,
                    &russh_bbs_core::doors::WorldWrite {
                        expected: Revision::NONE,
                        version: 1,
                        bytes: &world.bytes,
                        tick: None,
                    },
                )
                .await
                .unwrap();
        }
        store_save(&fresh, door.as_ref(), account, player).await;
        let c: Arc<dyn Clock> = clock.clone();
        let mut run = Running::open(
            &fresh,
            door.clone(),
            AccountId::from_raw(account),
            &env,
            c,
            seed,
        )
        .await
        .unwrap();
        for key in keys("p1x000") {
            run.key(key);
            run.flush(&fresh).await.unwrap();
        }
        let text = run.look_around(&env).join("\n");
        if !text.contains("Caught")
            && fresh
                .load_world(&door.info().id)
                .await
                .unwrap()
                .is_some_and(|w| decode_world(&w.bytes).is_some_and(|w| !w.active.is_empty()))
        {
            // Replay the winning seed against the real store.
            let c: Arc<dyn Clock> = clock.clone();
            store_save(store, door.as_ref(), account, player).await;
            let mut run = Running::open(
                store,
                door.clone(),
                AccountId::from_raw(account),
                &env,
                c,
                seed,
            )
            .await
            .unwrap();
            for key in keys("p1x000") {
                run.key(key);
                run.flush(store).await.unwrap();
            }
            return true;
        }
    }
    false
}

#[tokio::test]
async fn a_big_scheme_by_one_player_changes_the_school_for_everyone() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Hallpass::shared().unwrap();
    let player = big_scheme_player("move_the_deputy", "marsh", vec![("wren", "cover")]);
    let day = player.day;
    let clock = Arc::new(ManualClock::new(Day::from_raw(day).start()));
    store.set_now(Day::from_raw(day).start());
    run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();

    let before = decode_world(
        &store
            .load_world(&door.info().id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert!(before.active.is_empty() && before.paper.is_empty());
    assert!(
        pull_it_off(&store, &door, &player, 1, &clock).await,
        "some seed pulls it off"
    );

    let after = decode_world(
        &store
            .load_world(&door.info().id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert!(after.has("deputy_moved"), "the world remembers");
    assert_eq!(after.changes, 1);
    assert!(
        after
            .paper
            .iter()
            .any(|h| h.text.contains("Marsh") && h.text.contains("ann")),
        "the paper names the culprit: {:?}",
        after.paper
    );
    assert_eq!(after.hall.first().map(|e| e.name.as_str()), Some("ann"));

    // Another player, who has never met ann, opens the door the next morning and finds a different school.
    let content = common::content();
    let (bea, _) = Game::open(&content, "bea", None, Some(&after.to_bytes()), day);
    let marsh = content.npc("marsh").unwrap();
    let lunch = 2;
    assert_eq!(
        bea.npc_place(marsh, day, lunch),
        "old_wing",
        "the deputy has been moved"
    );
    assert!(
        bea.place_open("staffroom", day, lunch),
        "the staffroom is open to everyone: the deputy is no longer patrolling"
    );
    let (bob, _) = Game::open(&content, "bob", None, Some(&before.to_bytes()), day);
    assert!(
        !bob.place_open("staffroom", day, lunch),
        "while the old world would not have been"
    );

    // And it shows on her screen, through the real framework.
    let env = DoorEnv::for_tests("bea");
    let c: Arc<dyn Clock> = clock.clone();
    let mut run = Running::open(&store, door.clone(), AccountId::from_raw(2), &env, c, 3)
        .await
        .unwrap();
    run.key(DoorKey::Char('1'));
    run.flush(&store).await.unwrap();
    run.key(DoorKey::Char('n'));
    let screen = run.look_around(&env).join("\n");
    assert!(screen.contains("reassigned to the Old Wing"), "{screen}");
    assert!(
        screen.contains("In force: The deputy head has been moved"),
        "{screen}"
    );
    assert!(screen.contains("1. ann"), "the hall of legend: {screen}");

    // The change lasts the term: ticking days on does not undo it.
    for i in 1..=5 {
        clock.set(Day::from_raw(day + i).start());
        store.set_now(Day::from_raw(day + i).start());
        run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
            .await
            .unwrap();
    }
    let later = decode_world(
        &store
            .load_world(&door.info().id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert!(later.has("deputy_moved"));
}

#[tokio::test]
async fn a_temporary_change_closes_a_place_for_everyone_and_the_tick_reopens_it() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Hallpass::shared().unwrap();
    let player = big_scheme_player("close_tuck_shop", "tarn", vec![]);
    let day = player.day;
    let clock = Arc::new(ManualClock::new(Day::from_raw(day).start()));
    store.set_now(Day::from_raw(day).start());
    run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert!(pull_it_off(&store, &door, &player, 1, &clock).await);
    let world = decode_world(
        &store
            .load_world(&door.info().id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert!(world.has("tuck_shop_closed"));
    let content = common::content();
    let lunch = 2;
    let (shut, _) = Game::open(&content, "bea", None, Some(&world.to_bytes()), day);
    assert!(
        !shut.place_open("tuck_shop", day, lunch),
        "the hatch is closed to everyone for a week"
    );
    // The shop sells nothing meanwhile.
    let mut bea = Sandbox::on("fast_runner", 1, day);
    bea.game.world = world.clone();
    bea.game.player.slot = 2;
    bea.game.player.place = "tuck_shop".into();
    assert!(bea.try_(Action::Buy("salt".into())).is_err());
    // Six days on it is still shut; the seventh reopens it.
    for i in 1..=8 {
        clock.set(Day::from_raw(day + i).start());
        store.set_now(Day::from_raw(day + i).start());
        run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
            .await
            .unwrap();
        let w = decode_world(
            &store
                .load_world(&door.info().id)
                .await
                .unwrap()
                .unwrap()
                .bytes,
        )
        .unwrap();
        assert_eq!(w.has("tuck_shop_closed"), i < 7, "day +{i}");
    }
    let (open, _) = Game::open(
        &content,
        "bea",
        None,
        Some(
            &store
                .load_world(&door.info().id)
                .await
                .unwrap()
                .unwrap()
                .bytes,
        ),
        day + 8,
    );
    assert!(open.place_open("tuck_shop", day + 8, lunch));
}

#[tokio::test]
async fn a_failed_big_scheme_changes_nothing_for_anyone() {
    let player = big_scheme_player("fake_fire_drill", "the_alarm", vec![("sol", "lookout")]);
    let _ = player;
    // A weak character with the same plan is caught on every seed we try; the world stays as it was.
    let mut sb = Sandbox::new("teachers_pet", 1);
    sb.game.player.legend = 40;
    sb.game.player.cool = 0;
    sb.game.player.heat = 80;
    sb.game.player.notoriety = 60;
    let id = arrange(
        &mut sb,
        "the_alarm",
        "fake_fire_drill",
        &Setup {
            allies: vec![("pip", "gadget"), ("sol", "lookout")],
            scout: 2,
            ..Setup::default()
        },
    );
    let mut caught = 0;
    for seed in 0..40 {
        let mut s = Sandbox::new("teachers_pet", seed);
        s.game.player.legend = 40;
        s.game.player.heat = 80;
        s.game.player.notoriety = 60;
        let id = arrange(
            &mut s,
            "the_alarm",
            "fake_fire_drill",
            &Setup {
                allies: vec![("pip", "gadget"), ("sol", "lookout")],
                scout: 2,
                ..Setup::default()
            },
        );
        let events = run_scheme(&mut s, id, 0);
        if busted(&events) {
            caught += 1;
            assert!(!s.game.world.has("drill_chaos"));
            assert!(
                s.game.take_world_changes().iter().all(|d| !matches!(
                    d,
                    russh_bbs_door_hallpass::world::WorldDelta::Effect { .. }
                ))
            );
        } else if pulled(&events) {
            assert!(s.game.world.has("drill_chaos"));
        }
    }
    assert!(caught > 0);
    let _ = (id, monday());
}

#[tokio::test]
async fn the_tick_is_idempotent_catches_up_with_a_cap_and_rolls_the_term() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Hallpass::shared().unwrap();
    let start = 20_000;
    let clock = Arc::new(ManualClock::new(Day::from_raw(start).start()));
    let r = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!(r.applied.len(), 1);
    let again = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert!(
        again.applied.is_empty(),
        "the same day twice changes nothing"
    );
    let w1 = decode_world(
        &store
            .load_world(&door.info().id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert_eq!((w1.term, w1.term_start), (1, start));
    // Down for 50 days: the tick catches up at most its cap, and still lands on the right day.
    clock.set(Day::from_raw(start + 50).start());
    let r = run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    assert_eq!(r.applied.len(), 7);
    let w2 = decode_world(
        &store
            .load_world(&door.info().id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert_eq!(w2.term, 2, "the term turned over while the board was down");
    assert_eq!(w2.last_day, start + 50);
}
