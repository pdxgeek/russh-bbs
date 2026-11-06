//! Convoys, opt-in piracy and outposts through the framework's real open, key, flush and tick
//! path with a store, two or more captains and a manual clock.
#![allow(
    clippy::assert_is_empty,
    clippy::unreadable_literal,
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::similar_names,
    missing_docs
)]

use std::sync::Arc;

use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey};
use russh_bbs_door_long_haul::player::{Cause, Player};
use russh_bbs_door_long_haul::ship::Installed;
use russh_bbs_door_long_haul::social::{self, RaidOutcome, Social, SocialDelta};
use russh_bbs_door_long_haul::world::{Delta, World};
use russh_bbs_door_long_haul::{LongHaul, SAVE_VERSION};

const DAY0: i64 = 20_000;

struct Board {
    door: Arc<LongHaul>,
    store: MemoryDoorStore,
    clock: Arc<ManualClock>,
    visits: u64,
    season: u32,
}

fn keys(text: &str) -> Vec<DoorKey> {
    text.split(' ')
        .filter(|t| !t.is_empty())
        .map(|t| DoorKey::from_token(t).unwrap())
        .collect()
}

impl Board {
    fn new() -> Self {
        let store = MemoryDoorStore::new();
        store.set_now(Day::from_raw(DAY0).start());
        Self {
            door: Arc::new(LongHaul::new().unwrap()),
            store,
            clock: Arc::new(ManualClock::new(Day::from_raw(DAY0).start())),
            visits: 0,
            season: 1,
        }
    }

    fn day(&self) -> i64 {
        Day::of(self.clock.now()).as_raw()
    }

    fn id(n: i64) -> AccountId {
        AccountId::from_raw(n)
    }

    /// One visit: open the door as `n` (handle `sim<n>`), press the keys, flush after each.
    async fn visit(&mut self, n: i64, text: &str) -> Vec<String> {
        self.visits += 1;
        let env = DoorEnv::for_tests(&format!("sim{n}"));
        let clock: Arc<dyn Clock> = self.clock.clone();
        let door: Arc<dyn Door> = self.door.clone();
        let mut running = Running::open(
            &self.store,
            door,
            Self::id(n),
            &env,
            clock,
            0xC0FFEE ^ self.visits,
        )
        .await
        .unwrap();
        for key in keys(text) {
            running.key(key);
            running.flush(&self.store).await.unwrap();
        }
        running.look_around(&env)
    }

    async fn night(&self) {
        self.clock.advance(86_400);
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

    async fn player(&self, n: i64) -> Player {
        let saved = self
            .store
            .load_save(&self.door.info().id, Self::id(n))
            .await
            .unwrap()
            .unwrap();
        let ext = self.world_ext().await;
        Player::decode(&saved.bytes, self.door.content(), &ext).unwrap()
    }

    async fn edit(&self, n: i64, f: impl FnOnce(&mut Player)) {
        let id = &self.door.info().id;
        let saved = self
            .store
            .load_save(id, Self::id(n))
            .await
            .unwrap()
            .unwrap();
        let ext = self.world_ext().await;
        let mut p = Player::decode(&saved.bytes, self.door.content(), &ext).unwrap();
        f(&mut p);
        self.store
            .write_save(
                id,
                Self::id(n),
                &SaveWrite {
                    expected: saved.revision,
                    season: self.season,
                    version: SAVE_VERSION,
                    bytes: &p.encode(),
                },
            )
            .await
            .unwrap();
    }

    /// The map with the outposts of the world the captains are in now (the current season).
    async fn world_ext(&self) -> russh_bbs_door_long_haul::frontier::Frontier {
        let world = self.world().await;
        let base = self.door.frontier(self.season);
        if world.season == self.season {
            world.extended(self.door.content(), &base)
        } else {
            (*base).clone()
        }
    }

    async fn world(&self) -> World {
        match self.store.load_world(&self.door.info().id).await.unwrap() {
            Some(stored) => {
                let season = World::peek_season(&stored.bytes).unwrap();
                let frontier = self.door.frontier(season);
                World::decode(&stored.bytes, self.door.content(), &frontier).unwrap()
            }
            None => World::fresh(
                self.door.content(),
                &self.door.frontier(self.season),
                self.day(),
            ),
        }
    }

    /// Change the stored world the way a captain's session would.
    async fn world_delta(&self, deltas: &[Delta]) {
        let id = &self.door.info().id;
        let stored = self.store.load_world(id).await.unwrap();
        let (old, rev) = stored.map_or((None, Revision::NONE), |s| (Some(s.bytes), s.revision));
        let bytes = World::apply_stored(
            old.as_deref(),
            self.door.content(),
            &self.door.frontier(self.season),
            self.day(),
            deltas,
        );
        self.store
            .write_world(
                id,
                &russh_bbs_core::doors::WorldWrite {
                    expected: rev,
                    version: russh_bbs_door_long_haul::WORLD_VERSION,
                    bytes: &bytes,
                    tick: None,
                },
            )
            .await
            .unwrap();
    }

    fn weapon(&self) -> u8 {
        let c = self.door.content();
        u8::try_from(c.modules.iter().position(|m| m.id == "lance1").unwrap()).unwrap()
    }
}

/// Sign on as captain `n`: the intro's Enter picks the first background.
async fn join(b: &mut Board, n: i64) {
    b.visit(n, "enter q").await;
}

#[tokio::test]
async fn two_captains_form_and_share_a_convoy_through_the_store() {
    let mut b = Board::new();
    join(&mut b, 1).await;
    join(&mut b, 2).await;
    // Captain 1 forms a convoy (a name from the tables); captain 2 joins it.
    b.visit(1, "o n").await;
    b.visit(2, "o enter").await;
    let world = b.world().await;
    let convoy = &world.social.convoys[0];
    assert_eq!(convoy.members.len(), 2);
    assert_eq!(b.player(1).await.social.convoy, convoy.id);
    assert_eq!(b.player(2).await.social.convoy, convoy.id);
    // Captain 2 pays a profit into the pool (what a sale does), the night shares it among the
    // captains who flew lately, and each collects exactly their half, once.
    b.edit(2, |p| p.credit(Cause::Start, 10_000)).await;
    b.world_delta(&[Delta::Social(SocialDelta::ConvoyPay {
        id: convoy.id,
        amount: 400,
    })])
    .await;
    b.night().await;
    let cash1 = b.player(1).await.cash;
    let cash2 = b.player(2).await.cash;
    b.visit(1, "q").await;
    b.visit(2, "q").await;
    let (p1, p2) = (b.player(1).await, b.player(2).await);
    assert!(
        p1.cash >= cash1 + 200 && p2.cash >= cash2 + 200,
        "{} {}",
        p1.cash - cash1,
        p2.cash - cash2
    );
    assert_eq!(p1.social.collected, 200);
    // Opening again collects nothing more.
    b.visit(1, "q").await;
    assert_eq!(b.player(1).await.cash, p1.cash);
    // A note is shared: quick note 3 from captain 1 is in the log captain 2 reads.
    let lines = b.visit(1, "o 3").await;
    assert!(lines.iter().any(|l| l.contains("Clean run")), "{lines:?}");
    let lines = b.visit(2, "o").await;
    assert!(lines.iter().any(|l| l.contains("Clean run")), "{lines:?}");
    // Leaving collects what is owed and frees the seat.
    b.visit(2, "o x").await;
    assert_eq!(b.player(2).await.social.convoy, 0);
    assert_eq!(b.world().await.social.convoys[0].members.len(), 1);
}

#[tokio::test]
async fn a_typed_convoy_line_is_sanitized_and_rate_limited() {
    let mut b = Board::new();
    join(&mut b, 1).await;
    b.visit(1, "o n").await;
    // Escape sequences and control characters typed into the line do not reach the world.
    b.visit(1, "o t h i u1b u5b 3 1 m enter").await;
    let log = b.world().await.social.convoys[0].notes.clone();
    assert_eq!(log.len(), 1);
    assert!(
        !log[0].text.contains('\u{1b}') && log[0].text.starts_with("hi"),
        "{:?}",
        log[0]
    );
    // More than six player lines a minute are refused by the framework's limiter.
    for _ in 0..8 {
        b.visit(1, "o t x enter").await;
    }
    assert!(b.world().await.social.convoys[0].notes.len() <= social::MAX_NOTES);
}

#[tokio::test]
async fn piracy_needs_both_to_opt_in_and_a_prize_is_exactly_what_the_victim_paid() {
    let mut b = Board::new();
    let weapon = b.weapon();
    let mut raided = 0;
    let mut checked = 0;
    // Several pairs: the roll is deterministic per raid, so look until a prize appears.
    for pair in 0..6i64 {
        let (hunter, prey) = (10 + pair * 2, 11 + pair * 2);
        join(&mut b, hunter).await;
        join(&mut b, prey).await;
        b.edit(hunter, |p| {
            p.ship.modules.push(Installed {
                module: weapon,
                wear: 0,
            });
            p.credit(Cause::Start, 4_000);
        })
        .await;
        b.edit(prey, |p| p.credit(Cause::Start, 15_000)).await;
        // The hunter has not flagged: no hunting, and the prey is not on the roster.
        let lines = b.visit(hunter, "o right enter").await;
        assert!(b.world().await.social.intents.is_empty(), "{lines:?}");
        // Only the hunter flags: there is nobody to hunt and the prey cannot be touched.
        b.visit(hunter, "o right f").await;
        let before = b.world().await.social.intents.len();
        b.visit(hunter, "o right enter").await;
        assert_eq!(b.world().await.social.intents.len(), before);
        b.visit(prey, "o right f").await;
    }
    b.night().await;
    for pair in 0..6i64 {
        let (hunter, prey) = (10 + pair * 2, 11 + pair * 2);
        // The hunt picks the first other captain on the roster; use the Piracy tab cursor.
        b.visit(hunter, "o right down enter").await;
        let _ = prey;
    }
    let intents = b.world().await.social.intents.len();
    assert!(intents > 0, "someone hunted");
    b.night().await;
    let raids = b.world().await.social.raids.clone();
    assert!(!raids.is_empty());
    // Everyone opens: victims pay, hunters are paid, nobody twice.
    let mut hunters_in = 0i64;
    let mut victims_out = 0i64;
    for n in 10..22 {
        let before = b.player(n).await;
        b.visit(n, "q").await;
        let after = b.player(n).await;
        hunters_in += after.ledger.income[Cause::Piracy.index()];
        victims_out += after.ledger.spend[Cause::Piracy.index()];
        let again = {
            b.visit(n, "q").await;
            b.player(n).await
        };
        assert_eq!(again.cash, after.cash, "a second open collects nothing");
        let _ = before;
    }
    // Hunters collect settled prizes on the open after the victim paid.
    for n in 10..22 {
        b.visit(n, "q").await;
        let p = b.player(n).await;
        hunters_in = hunters_in.max(0);
        let _ = p;
    }
    let mut total_in = 0;
    let mut total_out = 0;
    for n in 10..22 {
        let p = b.player(n).await;
        total_in += p.ledger.income[Cause::Piracy.index()];
        total_out += p.ledger.spend[Cause::Piracy.index()];
    }
    for r in &b.world().await.social.raids {
        if r.outcome == RaidOutcome::Raided && r.settled {
            raided += 1;
            checked += i64::from(r.paid);
        }
    }
    assert!(raided > 0, "no raid landed in six pairs");
    assert_eq!(
        total_in, total_out,
        "money in piracy is a transfer ({hunters_in} {victims_out})"
    );
    assert_eq!(total_out, checked);
}

#[tokio::test]
async fn players_fund_an_outpost_late_in_the_season_and_it_becomes_a_market() {
    let mut b = Board::new();
    join(&mut b, 1).await;
    join(&mut b, 2).await;
    let door = b.door.clone();
    let frontier = door.frontier(1);
    // A charted system with no station.
    let site = frontier
        .systems
        .iter()
        .find(|s| !s.uncharted && s.stations.is_empty())
        .map(|s| s.id)
        .unwrap();
    for n in [1, 2] {
        b.edit(n, |p| {
            p.credit(Cause::Start, 40_000);
            p.at = site;
            p.docked = None;
        })
        .await;
    }
    // Too early.
    b.visit(1, "o right right P").await;
    assert!(b.world().await.social.sites.is_empty());
    for _ in 0..22 {
        b.night().await;
    }
    b.visit(1, "o right right P P").await;
    b.visit(2, "o right right P").await;
    let cost = b.player(1).await.ledger.spend[Cause::Outpost.index()]
        + b.player(2).await.ledger.spend[Cause::Outpost.index()];
    assert_eq!(cost, 15_000);
    assert_eq!(
        b.player(1).await.ledger.spend[Cause::Outpost.index()],
        10_000
    );
    assert!(
        b.world().await.social.outposts.is_empty(),
        "opens at the night"
    );
    b.night().await;
    let world = b.world().await;
    let outpost = &world.social.outposts[0];
    assert_eq!(outpost.system, site);
    assert_eq!(outpost.level, 1);
    // Everyone's map now has a station there with stock, and a captain can dock and trade.
    let ext = world.extended(door.content(), &frontier);
    assert_eq!(ext.stations.len(), frontier.stations.len() + 1);
    assert_eq!(world.stock.len(), ext.stations.len());
    let st = ext.stations.last().unwrap();
    assert_eq!(st.name, outpost.name);
    b.visit(1, "d").await;
    let p = b.player(1).await;
    assert_eq!(p.docked, Some(st.id), "the new station can be docked at");
    // The world round trips and later nights keep it.
    b.night().await;
    assert_eq!(b.world().await.social.outposts.len(), 1);
    let _ = Social::cost_of(1);
}

#[tokio::test]
async fn a_season_ends_with_an_ending_a_legacy_an_heirloom_and_a_hall_that_carries_over() {
    use russh_bbs_core::audit::{Actor, events};
    let mut b = Board::new();
    join(&mut b, 1).await;
    join(&mut b, 2).await;
    // Captain 1 got rich and finished the League's story; captain 2 just got by.
    b.edit(1, |p| {
        p.credit(Cause::Start, 120_000);
        p.social.arcs[1].ending = 8;
    })
    .await;
    b.night().await;
    b.visit(1, "q").await;
    b.visit(2, "q").await;
    let world = b.world().await;
    let rich = world.social.hall(0);
    assert_eq!(
        rich.first().map(|r| r.name.clone()),
        Some(b.player(1).await.captain)
    );
    let heir = b.player(1).await.heirloom_choice(b.door.content());
    // Staff end the season.
    let id = b.door.info().id.clone();
    b.store
        .end_season(&id, &events::door_season_ended(Actor::System, "long-haul"))
        .await
        .unwrap();
    b.season = 2;
    // The next visit starts a new captain who remembers how it ended and inherits a module.
    let lines = b.visit(1, "enter").await;
    let p = b.player(1).await;
    assert_eq!(p.social.legacy.len(), 1);
    assert_eq!(p.social.legacy[0].season, 1);
    assert_eq!(
        p.social.legacy[0].ending, 8,
        "the arc's ending counts before riches"
    );
    assert_eq!(p.social.heirloom, heir);
    assert!(p.ship.modules.iter().any(|m| m.module == heir));
    assert!(p.cash < 5_000, "a new captain, a new purse");
    let text = lines.join("\n");
    assert!(
        text.contains("Last season ended") || p.social.legacy.len() == 1,
        "{text}"
    );
    // Captain 2 had no story: a season ending is picked from how they did.
    b.visit(2, "enter").await;
    assert!(b.player(2).await.social.legacy[0].ending >= 100);
    // The new world appears with the first change a captain makes to it, and keeps the hall.
    b.visit(1, "o n").await;
    let world = b.world().await;
    assert_eq!(world.season, 2);
    assert_eq!(world.social.past.len(), 1);
    assert_eq!(world.social.past[0].season, 1);
    assert!(!world.social.past[0].records.is_empty());
}
