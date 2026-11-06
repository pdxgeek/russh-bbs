//! The social layer through the framework: two accounts, the real store, the real nightly tick.

#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::sync::Arc;

use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite, WorldWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{DoorEnv, DoorKey};
use russh_bbs_door_emberdeep::showcase::{self, RescuePair};
use russh_bbs_door_emberdeep::{
    DOOR_ID, Emberdeep, SAVE_VERSION, WORLD_VERSION, summarize, summarize_world,
};

const DAY: i64 = showcase::DAY;

struct Board {
    door: Arc<dyn russh_bbs_door::Door>,
    store: MemoryDoorStore,
    clock: Arc<ManualClock>,
    id: russh_bbs_door::DoorId,
}

impl Board {
    fn new() -> Self {
        let door = Emberdeep::shared().unwrap();
        let id = door.info().id.clone();
        let store = MemoryDoorStore::new();
        let clock = Arc::new(ManualClock::new(Day::from_raw(DAY).start()));
        store.set_now(clock.now());
        Self {
            door,
            store,
            clock,
            id,
        }
    }

    async fn seed(&self, pair: &RescuePair) {
        for (n, bytes) in [(1, &pair.fallen), (2, &pair.rescuer)] {
            self.store
                .write_save(
                    &self.id,
                    AccountId::from_raw(n),
                    &SaveWrite {
                        expected: Revision::NONE,
                        season: 1,
                        version: SAVE_VERSION,
                        bytes,
                    },
                )
                .await
                .unwrap();
        }
        self.store
            .write_world(
                &self.id,
                &WorldWrite {
                    expected: Revision::NONE,
                    version: WORLD_VERSION,
                    bytes: &pair.world,
                    tick: Some(Day::from_raw(DAY)),
                },
            )
            .await
            .unwrap();
    }

    async fn open(&self, n: i64, handle: &str) -> (Running, DoorEnv) {
        let env = DoorEnv::for_tests(handle);
        let clock: Arc<dyn Clock> = self.clock.clone();
        let r = Running::open(
            &self.store,
            Arc::clone(&self.door),
            AccountId::from_raw(n),
            &env,
            clock,
            9,
        )
        .await
        .unwrap();
        (r, env)
    }

    async fn press(&self, r: &mut Running, keys: &str) {
        for t in keys.split_whitespace() {
            let _ = r.key(DoorKey::from_token(t).unwrap());
            r.flush(&self.store).await.unwrap();
        }
    }

    async fn next_day(&self) {
        self.clock.set(Day::from_raw(DAY + 1).start());
        self.store.set_now(self.clock.now());
        run_due(
            &self.store,
            self.door.as_ref(),
            self.clock.as_ref(),
            TickPolicy::default(),
        )
        .await
        .unwrap();
    }

    async fn hero(&self, n: i64) -> russh_bbs_door_emberdeep::Summary {
        let content = Emberdeep::new().unwrap();
        let s = self
            .store
            .load_save(&self.id, AccountId::from_raw(n))
            .await
            .unwrap()
            .unwrap();
        summarize(&s.bytes, content.content().unwrap()).unwrap()
    }

    async fn world(&self) -> russh_bbs_door_emberdeep::WorldSummary {
        summarize_world(
            &self
                .store
                .load_world(&self.id)
                .await
                .unwrap()
                .unwrap()
                .bytes,
        )
        .unwrap()
    }
}

/// The key a screen offers for the entry whose label starts with `label`.
fn key_for(screen: &[String], label: &str) -> String {
    for line in screen {
        let mut rest = line.as_str();
        while let Some(at) = rest.find('[') {
            let tail = &rest[at + 1..];
            let Some(close) = tail.find(']') else { break };
            let after = tail[close + 1..].trim_start();
            if after.starts_with(label) && tail[..close].chars().count() == 1 {
                return tail[..close].to_owned();
            }
            rest = &tail[close + 1..];
        }
    }
    panic!("no `{label}` on {screen:#?}");
}

#[tokio::test]
async fn two_players_form_a_company_by_invitation_through_the_store() {
    let b = Board::new();
    // Two new heroes, made with real keys, appear on each other's roster.
    let (mut ann, env_a) = b.open(1, "ann").await;
    b.press(&mut ann, "1 a b enter 1 1 2 3 enter").await;
    let (mut bo, env_b) = b.open(2, "bo").await;
    b.press(&mut bo, "1 a b enter 1 1 2 3 enter").await;
    // Ann opens the company, invites Bo.
    let (mut ann, _) = b.open(1, "ann").await;
    b.press(&mut ann, "c").await;
    let key = key_for(&ann.look_around(&env_a), "Invite bo");
    b.press(&mut ann, &key).await;
    // Bo sees the invitation, and accepts.
    let (mut bo, _) = b.open(2, "bo").await;
    b.press(&mut bo, "c").await;
    let screen = bo.look_around(&env_b);
    assert!(
        screen.iter().any(|l| l.contains("ann invites you")),
        "{screen:?}"
    );
    let key = key_for(&screen, "Accept ann");
    b.press(&mut bo, &key).await;
    let w = b.world().await;
    assert_eq!(w.companies.len(), 1);
    assert_eq!(w.companies[0].1, vec!["ann".to_owned(), "bo".to_owned()]);
    // Ann, reopening, sees the company in the hub.
    let (ann, env) = b.open(1, "ann").await;
    assert!(ann.look_around(&env).iter().any(|l| l.contains("Company")));
}

#[tokio::test]
async fn a_stranger_carries_a_fallen_hero_home_and_is_thanked_once() {
    let b = Board::new();
    b.seed(&showcase::rescue_pair(false)).await;
    let coin_before = b.hero(2).await.coin;
    // Bo, standing where Ann lies, carries her home.
    let (mut bo, env) = b.open(2, "bo").await;
    let screen = bo.look_around(&env);
    let key = key_for(&screen, "Carry ann home");
    b.press(&mut bo, &key).await;
    assert_eq!(b.hero(2).await.rescues.0, 1);
    assert!(b.world().await.carried_home == 1);
    // The night passes. Ann wakes at home and pays her thanks out of her own purse.
    b.next_day().await;
    let (mut ann, env) = b.open(1, "ann").await;
    let screen = ann.look_around(&env);
    assert!(
        screen.iter().any(|l| l.contains("carried you home")),
        "{screen:?}"
    );
    b.press(&mut ann, "esc").await;
    let hero = b.hero(1).await;
    assert!(
        hero.coin < 100,
        "thanks come out of her purse: {}",
        hero.coin
    );
    let thanks = 100 - hero.coin;
    // Bo, opening the door the next day, collects it; opening again pays nothing more.
    let (mut bo, env) = b.open(2, "bo").await;
    let screen = bo.look_around(&env);
    assert!(
        screen.iter().any(|l| l.contains("sends their thanks")),
        "{screen:?}"
    );
    b.press(&mut bo, "x").await;
    let after = b.hero(2).await.coin;
    assert!(
        after >= coin_before + thanks,
        "{after} vs {coin_before} + {thanks}"
    );
    let (mut bo, _) = b.open(2, "bo").await;
    b.press(&mut bo, "x").await;
    assert_eq!(b.hero(2).await.coin, after, "a note is paid once");
}

#[tokio::test]
async fn a_companion_rescue_costs_a_turn_less_and_owes_no_thanks() {
    let b = Board::new();
    b.seed(&showcase::rescue_pair(true)).await;
    let (mut bo, env) = b.open(2, "bo").await;
    let turns = b.hero(2).await.turns;
    let key = key_for(&bo.look_around(&env), "Carry ann home");
    b.press(&mut bo, &key).await;
    assert_eq!(
        turns - b.hero(2).await.turns,
        2,
        "two turns for a companion, three for a stranger"
    );
    let w = b.world().await;
    assert_eq!(w.companies[0].2, 1, "the company counts it");
    b.next_day().await;
    let (mut ann, env) = b.open(1, "ann").await;
    assert!(
        ann.look_around(&env)
            .iter()
            .any(|l| l.contains("of your company"))
    );
    b.press(&mut ann, "esc").await;
    assert_eq!(b.hero(1).await.coin, 100, "no thanks between companions");
}

#[tokio::test]
async fn nobody_comes_and_the_epitaph_reaches_the_hall() {
    let b = Board::new();
    b.seed(&showcase::rescue_pair(false)).await;
    // Four days pass with no rescue.
    for d in 1..=4 {
        b.clock.set(Day::from_raw(DAY + d).start());
        b.store.set_now(b.clock.now());
        run_due(
            &b.store,
            b.door.as_ref(),
            b.clock.as_ref(),
            TickPolicy::default(),
        )
        .await
        .unwrap();
    }
    let (mut ann, env) = b.open(1, "ann").await;
    let screen = ann.look_around(&env);
    assert!(
        screen.iter().any(|l| l.contains("wake in the temple")),
        "{screen:?}"
    );
    b.press(&mut ann, "esc").await;
    assert_eq!(b.world().await.epitaphs, 1);
    let _ = DOOR_ID;
}

#[test]
fn a_social_branch_replays_exactly_and_saves_what_the_hero_chose() {
    use russh_bbs_door::replay::{self, ReplayLog};
    let door = Emberdeep::new().unwrap();
    let env = DoorEnv::for_tests("ann");
    // Make a warden, step into the ring, close the door to invitations, write an epitaph.
    let text = "1 a b enter 1 1 2 3 enter u 1 esc c 1 esc j e h i enter esc";
    let log = ReplayLog {
        door: DOOR_ID.to_owned(),
        seed: 3,
        start: Day::from_raw(DAY).start().as_raw(),
        season: 1,
        save: None,
        keys: text
            .split_whitespace()
            .map(|t| DoorKey::from_token(t).unwrap())
            .collect(),
    };
    let run = replay::run_checked(&door, &log, &env).unwrap();
    assert_eq!(run, replay::run_checked(&door, &log, &env).unwrap());
    let bytes = run.save.as_deref().unwrap();
    let s = summarize(bytes, door.content().unwrap()).unwrap();
    assert!(s.ring, "the hero chose the ring");
}
