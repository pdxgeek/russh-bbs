//! Saltmarch's social layer (M4-S4) through the framework: two or three accounts sharing one
//! board, real saves and one shared world, the daily tick, and a replay of each visit.
//!
//! A framework replay log carries no world, and every social visit depends on one, so these
//! tests keep their own log: the save and the world as the visit found them, the seed and the
//! keys. [`replay`] reopens a fresh store in that state and runs the same keys, and the result
//! must be the same save and the same world, byte for byte.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::assert_is_empty
)]

use std::sync::Arc;

use russh_bbs_core::audit::{Action, Actor, AuditEvent, Target};
use russh_bbs_core::doors::{DoorId, DoorStore, Revision, SaveWrite, WorldWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey};
use russh_bbs_door_saltmarch::clan::Clan;
use russh_bbs_door_saltmarch::content::Catalog;
use russh_bbs_door_saltmarch::crew::Crew;
use russh_bbs_door_saltmarch::social::{caps, renown};
use russh_bbs_door_saltmarch::types::{Resource, Trade};
use russh_bbs_door_saltmarch::world::World;
use russh_bbs_door_saltmarch::{SAVE_VERSION, Saltmarch, WORLD_VERSION};

const DAY: i64 = 20_000;

struct Board {
    store: MemoryDoorStore,
    door: Arc<dyn Door>,
    clock: Arc<ManualClock>,
    id: DoorId,
}

/// A visit that can be replayed: what it found, what it did, what it left.
struct Log {
    who: u8,
    handle: String,
    seed: u64,
    day: i64,
    save: Option<Vec<u8>>,
    world: Option<Vec<u8>>,
    keys: Vec<DoorKey>,
    after_save: Option<Vec<u8>>,
    after_world: Option<Vec<u8>>,
}

struct Seat<'a> {
    board: &'a Board,
    run: Running,
    env: DoorEnv,
    log: Log,
}

fn account(who: u8) -> AccountId {
    AccountId::from_raw(i64::from(who))
}

impl Board {
    fn new(day: i64) -> Self {
        let door: Arc<dyn Door> = Saltmarch::shared().unwrap();
        let id = door.info().id.clone();
        let board = Self {
            store: MemoryDoorStore::new(),
            door,
            clock: Arc::new(ManualClock::new(Day::from_raw(day).start())),
            id,
        };
        board.store.set_now(Day::from_raw(day).start());
        board
    }

    fn set_day(&self, day: i64) {
        self.clock.set(Day::from_raw(day).start());
        self.store.set_now(Day::from_raw(day).start());
    }

    fn day(&self) -> i64 {
        Day::of(self.clock.now()).as_raw()
    }

    async fn tick(&self) {
        run_due(
            &self.store,
            self.door.as_ref(),
            self.clock.as_ref(),
            TickPolicy::default(),
        )
        .await
        .unwrap();
    }

    async fn save_of(&self, who: u8) -> Option<Vec<u8>> {
        self.store
            .load_save(&self.id, account(who))
            .await
            .unwrap()
            .map(|s| s.bytes)
    }

    async fn world_bytes(&self) -> Option<Vec<u8>> {
        self.store
            .load_world(&self.id)
            .await
            .unwrap()
            .map(|w| w.bytes)
    }

    async fn clan(&self, who: u8) -> Clan {
        Clan::decode(&self.save_of(who).await.unwrap()).unwrap()
    }

    async fn world(&self) -> World {
        World::decode(&self.world_bytes().await.unwrap()).unwrap()
    }

    /// Change a stored clan the way a long game would have, between visits.
    async fn edit(&self, who: u8, f: impl FnOnce(&mut Clan)) {
        let stored = self
            .store
            .load_save(&self.id, account(who))
            .await
            .unwrap()
            .unwrap();
        let mut clan = Clan::decode(&stored.bytes).unwrap();
        f(&mut clan);
        self.store
            .write_save(
                &self.id,
                account(who),
                &SaveWrite {
                    expected: stored.revision,
                    season: stored.season,
                    version: SAVE_VERSION,
                    bytes: &clan.encode(),
                },
            )
            .await
            .unwrap();
    }

    async fn visit(&self, who: u8, handle: &str, seed: u64) -> Seat<'_> {
        let env = DoorEnv::for_tests(handle);
        let clock: Arc<dyn Clock> = self.clock.clone();
        let run = Running::open(
            &self.store,
            self.door.clone(),
            account(who),
            &env,
            clock,
            seed,
        )
        .await
        .unwrap();
        Seat {
            board: self,
            run,
            env,
            log: Log {
                who,
                handle: handle.to_owned(),
                seed,
                day: self.day(),
                save: self.save_of(who).await,
                world: self.world_bytes().await,
                keys: Vec::new(),
                after_save: None,
                after_world: None,
            },
        }
    }
}

impl Seat<'_> {
    async fn key(&mut self, key: DoorKey) {
        self.log.keys.push(key);
        self.run.key(key);
        self.run.flush(&self.board.store).await.unwrap();
    }

    fn screen(&self) -> String {
        self.run.look_around(&self.env).join("\n")
    }

    fn said(&self, needle: &str) -> bool {
        self.screen().contains(needle)
    }

    /// Press the entry whose tag starts with `tag`. `false` if the screen offers none.
    async fn tag(&mut self, tag: &str) -> bool {
        let found = self.run.look_around(&self.env).iter().find_map(|line| {
            let rest = line.strip_prefix('[')?;
            let (key, text) = rest.split_once("] ")?;
            text.starts_with(tag).then(|| key.chars().next())?
        });
        match found {
            Some(c) => {
                self.key(DoorKey::Char(c)).await;
                true
            }
            None => false,
        }
    }

    async fn go(&mut self, tags: &[&str]) {
        for t in tags {
            assert!(self.tag(t).await, "no `{t}` on\n{}", self.screen());
        }
    }

    /// Any key: past the morning report.
    async fn past_report(&mut self) {
        if self.said("While you were away") || self.said("News from the Tidecourt") {
            self.key(DoorKey::Char(' ')).await;
        }
    }

    /// Leave the door, and keep the log of the visit with what it left behind.
    async fn finish(mut self) -> Log {
        self.run.flush(&self.board.store).await.unwrap();
        self.log.after_save = self.board.save_of(self.log.who).await;
        self.log.after_world = self.board.world_bytes().await;
        self.log
    }
}

/// Run a log again on a fresh store in the state the visit found, and compare what it leaves.
async fn replay(board: &Board, log: &Log) {
    let fresh = Board::new(log.day);
    let id = fresh.id.clone();
    if let Some(world) = &log.world {
        fresh
            .store
            .write_world(
                &id,
                &WorldWrite {
                    expected: Revision::NONE,
                    version: WORLD_VERSION,
                    bytes: world,
                    tick: None,
                },
            )
            .await
            .unwrap();
    }
    if let Some(save) = &log.save {
        fresh
            .store
            .write_save(
                &id,
                account(log.who),
                &SaveWrite {
                    expected: Revision::NONE,
                    season: 1,
                    version: SAVE_VERSION,
                    bytes: save,
                },
            )
            .await
            .unwrap();
    }
    let env = DoorEnv::for_tests(&log.handle);
    let clock: Arc<dyn Clock> = fresh.clock.clone();
    let mut run = Running::open(
        &fresh.store,
        fresh.door.clone(),
        account(log.who),
        &env,
        clock,
        log.seed,
    )
    .await
    .unwrap();
    for key in &log.keys {
        run.key(*key);
        run.flush(&fresh.store).await.unwrap();
    }
    assert_eq!(
        fresh.save_of(log.who).await,
        log.after_save,
        "the save replays"
    );
    assert_eq!(
        fresh.world_bytes().await,
        log.after_world,
        "the world replays"
    );
    let _ = board;
}

async fn found(board: &Board, who: u8, handle: &str) {
    let mut s = board.visit(who, handle, u64::from(who)).await;
    s.key(DoorKey::Enter).await;
    s.finish().await;
}

/// Give a stored clan a thicker skin: older than the new-clan week and with real fighters.
fn grown(clan: &mut Clan, fighters: usize) {
    let cat = Catalog::embedded().unwrap();
    let mut rng = russh_bbs_door::DoorRng::from_seed(11);
    clan.founded = DAY - 30;
    for _ in 0..fighters {
        let mut c = Crew::generate(&cat, &mut rng, 5, Some(Trade::Fighter));
        c.attrs = [9, 6, 6];
        c.xp[Trade::Fighter.index()] = 200;
        c.id = clan.take_id();
        clan.crew.push(c);
    }
    clan.stock.add(Resource::Arms, 6);
    clan.stock.add(Resource::Food, 40);
    clan.stock.add(Resource::Peat, 40);
}

/// Open the door, do something that saves (so the clan's post is current), leave.
async fn show_the_post(board: &Board, who: u8, handle: &str, seed: u64) {
    let mut s = board.visit(who, handle, seed).await;
    s.past_report().await;
    s.go(&["survey"]).await;
    s.finish().await;
}

fn total(a: &Clan, b: &Clan, what: Resource) -> u32 {
    a.stock.get(what) + b.stock.get(what)
}

// ------------------------------------------------------------------------------------------
// The market.

#[tokio::test]
async fn a_market_trade_between_two_clans_moves_goods_and_tallies_and_loses_only_fees() {
    let board = Board::new(DAY);
    found(&board, 1, "ann").await;
    found(&board, 2, "bob").await;
    board.edit(1, |c| c.stock.add(Resource::Salt, 50)).await;
    board.edit(2, |c| c.tallies = 400).await;
    let (a0, b0) = (board.clan(1).await, board.clan(2).await);

    // Ann posts an ask; the exchange answers with the post.
    let mut a = board.visit(1, "ann", 3).await;
    a.go(&["go:court", "go:exchange", "ask:salt:5@4"]).await;
    assert!(a.said("Sent to the exchange"));
    let ann_posts = a.finish().await;
    let pending = board.clan(1).await.social.pending.len();
    assert_eq!(
        pending, 1,
        "the goods wait in the book, and the clan remembers it"
    );

    // Bob sees the ask and buys it.
    let mut b = board.visit(2, "bob", 4).await;
    b.go(&["go:court", "go:exchange"]).await;
    assert!(b.said("Best ask: 5 at 4 each"), "{}", b.screen());
    b.go(&["xbuy:salt:5"]).await;
    let bob_buys = b.finish().await;

    // Both read the answer at their next visit.
    let mut a = board.visit(1, "ann", 5).await;
    assert!(a.said("The exchange paid"), "{}", a.screen());
    a.past_report().await;
    let ann_reads = a.finish().await;
    let mut b = board.visit(2, "bob", 6).await;
    assert!(b.said("The exchange sent 5 salt"), "{}", b.screen());
    b.past_report().await;
    let bob_reads = b.finish().await;

    let (a1, b1) = (board.clan(1).await, board.clan(2).await);
    assert_eq!(total(&a0, &b0, Resource::Salt), 50);
    assert_eq!(total(&a1, &b1, Resource::Salt), 50, "no goods made or lost");
    assert_eq!(
        (a1.stock.get(Resource::Salt), b1.stock.get(Resource::Salt)),
        (45, 5)
    );
    // 5 salt at 4 = 20 tallies; each paid a 1-tallie listing fee; the levy on 20 is nothing.
    assert_eq!(
        a1.tallies + b1.tallies + 2,
        a0.tallies + b0.tallies,
        "only fees are lost"
    );
    assert_eq!(b1.tallies, b0.tallies - 21);
    assert_eq!(a1.tallies, a0.tallies + 19);
    assert!(a1.social.pending.is_empty() && b1.social.pending.is_empty());
    let w = board.world().await;
    assert!(
        w.social.mail.is_empty() && w.social.receipts.is_empty(),
        "the world forgot"
    );
    assert_eq!(
        w.social.exchange.today[Resource::Salt.index()],
        (5, 20),
        "the trade is on the record"
    );
    for log in [&ann_posts, &bob_buys, &ann_reads, &bob_reads] {
        replay(&board, log).await;
    }
}

#[tokio::test]
async fn an_edit_the_board_dropped_gives_the_goods_back_once() {
    use russh_bbs_core::limits::Day as D;
    use russh_bbs_door::host::Driver;
    use russh_bbs_door::{Save, Start};
    let board = Board::new(DAY);
    found(&board, 1, "ann").await;
    board.edit(1, |c| c.stock.add(Resource::Salt, 50)).await;
    let before = board.clan(1).await;
    // A visit whose save is written but whose world edits never run (a crash between the two).
    let stored = board
        .store
        .load_save(&board.id, account(1))
        .await
        .unwrap()
        .unwrap();
    let world = board.world_bytes().await;
    let env = DoorEnv::for_tests("ann");
    let mut driver = Driver::open(
        board.door.as_ref(),
        Start {
            save: Save::Loaded {
                bytes: &stored.bytes,
                season: 1,
            },
            world: world.as_deref(),
            env: &env,
            today: D::from_raw(DAY),
            season: 1,
        },
        9,
        board.clock.clone(),
    );
    let press = |driver: &mut Driver, tag: &str| {
        let line = driver
            .look_around(&env)
            .into_iter()
            .find(|l| {
                l.strip_prefix('[')
                    .is_some_and(|r| r.split_once("] ").is_some_and(|(_, t)| t.starts_with(tag)))
            })
            .unwrap_or_else(|| panic!("no {tag}"));
        driver.key(DoorKey::Char(line.chars().nth(1).unwrap()));
    };
    press(&mut driver, "go:court");
    press(&mut driver, "go:exchange");
    press(&mut driver, "ask:salt:5@4");
    let effects = driver.take_effects();
    let bytes = effects.save().unwrap().to_vec();
    let mid = Clan::decode(&bytes).unwrap();
    assert_eq!(
        mid.stock.get(Resource::Salt),
        45,
        "the salt left the stores"
    );
    assert_eq!(mid.social.pending.len(), 1);
    board
        .store
        .write_save(
            &board.id,
            account(1),
            &SaveWrite {
                expected: stored.revision,
                season: 1,
                version: SAVE_VERSION,
                bytes: &bytes,
            },
        )
        .await
        .unwrap();
    assert!(
        board.world().await.social.exchange.asks.is_empty(),
        "the world never heard"
    );
    // The next visit finds no receipt, and takes the goods back.
    let mut a = board.visit(1, "ann", 10).await;
    assert!(a.said("never reached the Tidecourt"), "{}", a.screen());
    a.past_report().await;
    a.finish().await;
    let after = board.clan(1).await;
    assert_eq!(
        after.stock.get(Resource::Salt),
        before.stock.get(Resource::Salt)
    );
    assert_eq!(after.tallies, before.tallies, "the fee comes back too");
    assert!(after.social.pending.is_empty());
    // And it does not come back twice.
    let mut a = board.visit(1, "ann", 11).await;
    a.past_report().await;
    a.go(&["survey"]).await;
    a.finish().await;
    assert_eq!(board.clan(1).await.stock.get(Resource::Salt), 50);
}

// ------------------------------------------------------------------------------------------
// Raids.

/// Turn the gate's window to a turn of the water the party can walk (no boat needed).
async fn walkable_window(s: &mut Seat<'_>) {
    for _ in 0..5 {
        let landing = s
            .screen()
            .lines()
            .find(|l| l.starts_with("Landing at"))
            .map(str::to_owned);
        match landing {
            Some(l) if l.contains("(needs a boat)") => s.go(&["window:next"]).await,
            _ => break,
        }
    }
}

/// Walk a raid through the screens on whoever the gate is showing.
async fn send_raid(s: &mut Seat<'_>) {
    s.go(&["go:court", "go:raid"]).await;
    walkable_window(s).await;
    s.go(&["raid:send:"]).await;
}

#[tokio::test]
async fn a_raid_is_settled_overnight_against_the_posted_guard_and_both_clans_read_it() {
    let board = Board::new(DAY);
    found(&board, 1, "ann").await;
    found(&board, 2, "bob").await;
    board.edit(1, |c| grown(c, 3)).await;
    board
        .edit(2, |c| {
            c.founded = DAY - 30;
            c.crew.truncate(1);
            c.stock.add(Resource::Salt, 60);
            c.stock.add(Resource::Iron, 30);
            c.tallies = 500;
        })
        .await;
    show_the_post(&board, 1, "ann", 3).await;
    show_the_post(&board, 2, "bob", 4).await;
    let w = board.world().await;
    assert!(
        !w.clans[1].posted.guard.is_empty()
            && w.clans[1].posted.stock[Resource::Salt.index()] >= 55
    );

    // Before any raid is won, ann cannot gloat: no taunt among the words she may use.
    let mut a = board.visit(1, "ann", 5).await;
    a.go(&["go:court", "go:letters", "compose"]).await;
    for _ in 0..16 {
        assert!(!a.said("the sea keeps what we carried"), "{}", a.screen());
        a.go(&["template:next"]).await;
    }
    // Send the raid.
    let mut a = board.visit(1, "ann", 6).await;
    let (stock_before, crew_before) = {
        let c = board.clan(1).await;
        (c.stock.clone(), c.crew.len())
    };
    send_raid(&mut a).await;
    assert!(a.said("slip out toward bob"), "{}", a.screen());
    let sent = a.finish().await;
    let ann = board.clan(1).await;
    assert!(
        !ann.social.away.is_empty(),
        "the raiders are away from the hold"
    );
    assert_eq!(
        ann.stock.get(Resource::Salt),
        stock_before.get(Resource::Salt)
    );
    // Away crew cannot be sent anywhere else.
    let w = board.world().await;
    assert_eq!(w.social.orders.len(), 1);
    assert_eq!(w.social.orders[0].raiders.len(), ann.social.away.len());

    // The night.
    board.set_day(DAY + 1);
    board.tick().await;
    let w = board.world().await;
    assert!(w.social.orders.is_empty(), "settled");
    assert_eq!(w.social.mail.len(), 2, "a report for each side");

    // Ann reads her side.
    let mut a = board.visit(1, "ann", 7).await;
    assert!(a.said("broke the gate"), "{}", a.screen());
    a.past_report().await;
    let ann_reads = a.finish().await;
    let ann = board.clan(1).await;
    assert!(ann.social.away.is_empty(), "the party is home");
    assert_eq!(ann.social.raids_won, 1);
    assert!(ann.social.plundered > 0);
    assert!(ann.crew.len() >= crew_before - 1);
    // Bob reads his.
    let mut b = board.visit(2, "bob", 8).await;
    assert!(b.said("raided the hold in the night"), "{}", b.screen());
    b.past_report().await;
    let bob_reads = b.finish().await;
    let bob = board.clan(2).await;
    assert_eq!(bob.social.raided, 1);
    assert_eq!(
        bob.social.robbed, ann.social.plundered,
        "what one lost the other took"
    );
    assert!(bob.morale < 60 + 10);
    let w = board.world().await;
    assert!(
        w.social.mail.is_empty(),
        "both reports were folded and forgotten"
    );
    assert_eq!(w.clans[0].record.raids_won, 1);
    assert_eq!(w.clans[1].record.raided, 1);
    // The hold is left alone for two days; the same pair may not raid again for three.
    let mut a = board.visit(1, "ann", 9).await;
    a.past_report().await;
    a.go(&["go:court", "go:raid"]).await;
    assert!(
        a.said("raided each other too recently") || a.said("raided lately"),
        "{}",
        a.screen()
    );
    assert!(!a.tag("raid:send:").await);
    // Having won, ann may now use a taunt: the history opens the words.
    a.key(DoorKey::Escape).await;
    a.go(&["go:letters", "compose"]).await;
    let mut found_taunt = false;
    for _ in 0..16 {
        if a.said("the sea keeps what we carried") || a.said("your gate was lightly held") {
            found_taunt = true;
            break;
        }
        a.go(&["template:next"]).await;
    }
    assert!(found_taunt, "{}", a.screen());
    for log in [&sent, &ann_reads, &bob_reads] {
        replay(&board, log).await;
    }
}

#[tokio::test]
async fn a_raid_the_guard_beats_off_costs_the_raider_and_the_defender_keeps_everything() {
    let board = Board::new(DAY);
    found(&board, 1, "ann").await;
    found(&board, 2, "bob").await;
    // Ann has only her starting three; bob has built a proper guard and a palisade.
    board.edit(1, |c| c.founded = DAY - 30).await;
    board
        .edit(2, |c| {
            grown(c, 4);
            c.hold.plots[2] = Some("palisade".into());
            c.hold.plots[3] = Some("watch-tower".into());
            c.tallies = 400;
        })
        .await;
    show_the_post(&board, 1, "ann", 3).await;
    show_the_post(&board, 2, "bob", 4).await;
    let bob0 = board.clan(2).await;
    let mut a = board.visit(1, "ann", 6).await;
    send_raid(&mut a).await;
    a.finish().await;
    board.set_day(DAY + 1);
    board.tick().await;
    let mut a = board.visit(1, "ann", 7).await;
    assert!(a.said("beaten off the gate"), "{}", a.screen());
    a.past_report().await;
    a.finish().await;
    let ann = board.clan(1).await;
    assert_eq!((ann.social.raids_lost, ann.social.raids_won), (1, 0));
    let mut b = board.visit(2, "bob", 8).await;
    assert!(b.said("was beaten off"), "{}", b.screen());
    b.past_report().await;
    b.finish().await;
    let bob = board.clan(2).await;
    assert_eq!((bob.social.repelled, bob.social.raided), (1, 0));
    // A night costs what its crew cost (a hand who drinks is a tally a night), so the proof that
    // nothing was taken is the clan's own ledger of what it was robbed of.
    assert_eq!(bob.social.robbed, 0, "nothing was taken");
    assert!(bob.tallies + 8 >= bob0.tallies);
    assert!(bob.morale >= bob0.morale);
}

#[tokio::test]
async fn caps_and_protections_stop_a_player_griefing_another() {
    let board = Board::new(DAY);
    found(&board, 1, "ann").await;
    found(&board, 2, "bob").await;
    found(&board, 3, "cat").await;
    board.edit(1, |c| grown(c, 2)).await;
    board.edit(3, |c| grown(c, 2)).await;
    show_the_post(&board, 1, "ann", 4).await;
    show_the_post(&board, 3, "cat", 5).await;
    // Bob is new: protected for a week. The gate offers no raid on him.
    let mut a = board.visit(1, "ann", 6).await;
    a.go(&["go:court", "go:raid"]).await;
    let on_bob = a.said("Target: bob") || a.said("raid on bob");
    if on_bob {
        assert!(a.said("too new to be raided"), "{}", a.screen());
        assert!(!a.tag("raid:send:").await);
        a.go(&["target:next"]).await;
    }
    // Cat is old and fair game; ann may send two raids a day and no more.
    assert!(a.said("The way is open"), "{}", a.screen());
    walkable_window(&mut a).await;
    a.go(&["raid:send:"]).await;
    a.finish().await;
    let w = board.world().await;
    assert_eq!(w.social.orders.len(), 1);
    // The same pair cannot raid again for three days, and a raided clan's own raids are not
    // blocked by being a target.
    let mut a = board.visit(1, "ann", 7).await;
    a.go(&["go:court", "go:raid"]).await;
    assert!(!a.tag("raid:send:").await, "{}", a.screen());
    // A clan nobody has seen for ten days cannot be raided at all.
    board.set_day(DAY + 11);
    board.tick().await;
    show_the_post(&board, 1, "ann", 8).await;
    let mut a = board.visit(1, "ann", 9).await;
    a.past_report().await;
    a.go(&["go:court", "go:raid"]).await;
    assert!(
        a.said("Nobody has seen that clan")
            || a.said("not on the Tidecourt's rolls")
            || !a.said("The way is open"),
        "{}",
        a.screen()
    );
    assert!(!a.tag("raid:send:").await);
}

// ------------------------------------------------------------------------------------------
// Pacts, letters, reputation.

#[tokio::test]
async fn a_pact_is_made_and_broken_and_the_clans_remember() {
    let board = Board::new(DAY);
    found(&board, 1, "ann").await;
    found(&board, 2, "bob").await;
    found(&board, 3, "cat").await;
    for who in [1, 2, 3] {
        board.edit(who, |c| c.founded = DAY - 30).await;
    }
    for (who, h) in [(1, "ann"), (2, "bob"), (3, "cat")] {
        show_the_post(&board, who, h, 20 + u64::from(who)).await;
    }
    // Ann offers bob a truce.
    let mut a = board.visit(1, "ann", 3).await;
    a.go(&["go:court", "go:pacts"]).await;
    // Choose bob among the clans on the rolls.
    for _ in 0..3 {
        if a.said("Offer bob a pact") {
            break;
        }
        a.go(&["ptarget:next"]).await;
    }
    a.go(&["offer:"]).await;
    let offered = a.finish().await;
    // Bob finds the offer as a letter, and accepts it.
    let mut b = board.visit(2, "bob", 4).await;
    assert!(b.said("offers a pact"), "{}", b.screen());
    b.past_report().await;
    b.go(&["go:court", "go:letters"]).await;
    assert!(b.said("accept:"), "{}", b.screen());
    b.go(&["accept:"]).await;
    let accepted = b.finish().await;
    // Ann learns it was signed; a pact now stands, and neither may raid the other.
    let mut a = board.visit(1, "ann", 5).await;
    assert!(a.said("signed the pact"), "{}", a.screen());
    a.past_report().await;
    a.go(&["go:court", "go:raid"]).await;
    for _ in 0..3 {
        if a.said("Target: bob") {
            break;
        }
        a.go(&["target:next"]).await;
    }
    assert!(a.said("A pact stands between you"), "{}", a.screen());
    assert!(!a.tag("raid:send:").await);
    a.finish().await;
    assert!(board.world().await.pact_between(0, 1).is_some());
    // Ann breaks it: two presses, because it is not small.
    let mut a = board.visit(1, "ann", 6).await;
    a.go(&["go:court", "go:pacts"]).await;
    a.go(&["break:"]).await;
    assert!(a.said("Press again to break faith"), "{}", a.screen());
    a.go(&["break:"]).await;
    let broken = a.finish().await;
    let w = board.world().await;
    assert!(w.pact_between(0, 1).is_none());
    assert_eq!(
        (w.clans[0].record.betrayals, w.clans[0].record.rep),
        (1, -25)
    );
    // Bob is told, and remembers; the whole marsh can see it on the deed feed.
    let mut b = board.visit(2, "bob", 7).await;
    assert!(b.said("broke the pact"), "{}", b.screen());
    b.past_report().await;
    b.finish().await;
    assert!(
        board
            .clan(2)
            .await
            .social
            .grudges
            .contains(&"ann".to_owned())
    );
    // The betrayal closes options: ann may offer nothing for a few days, trade is closed to her
    // for the season, and her renown pays for it.
    let mut a = board.visit(1, "ann", 8).await;
    a.past_report().await;
    a.go(&["go:court", "go:pacts"]).await;
    assert!(a.said("Cooling off"), "{}", a.screen());
    assert!(!a.tag("offer:").await, "no offers while cooling off");
    assert!(a.said("Betrayals on record: 1"), "{}", a.screen());
    while !a.said("Terms with cat") {
        a.go(&["ptarget:next"]).await;
    }
    assert!(
        a.said("trade is closed: a pact was broken"),
        "{}",
        a.screen()
    );
    let w = board.world().await;
    assert!(
        w.closed_reason(0, 2, 2, DAY + 10)
            .is_some_and(|r| r.contains("broken"))
    );
    for log in [&offered, &accepted, &broken] {
        replay(&board, log).await;
    }
}

#[tokio::test]
async fn a_question_mark_typed_into_a_letter_line_is_text_not_the_look_around_key() {
    let board = Board::new(DAY);
    found(&board, 1, "ann").await;
    found(&board, 2, "bob").await;
    let mut a = board.visit(1, "ann", 3).await;
    a.go(&["go:court", "go:letters", "compose"]).await;
    a.go(&["line"]).await;
    for c in "who? me?".chars() {
        a.key(DoorKey::Char(c)).await;
    }
    a.key(DoorKey::Enter).await;
    assert!(a.said("Your line: who? me?"), "{}", a.screen());
}

#[tokio::test]
async fn letters_are_templates_with_a_clean_limited_line_and_a_clan_can_block_a_sender() {
    let board = Board::new(DAY);
    found(&board, 1, "ann").await;
    found(&board, 2, "bob").await;
    let mut a = board.visit(1, "ann", 3).await;
    a.go(&["go:court", "go:letters", "compose"]).await;
    a.go(&["line"]).await;
    // Typing a line with an escape sequence in it: it is cleaned on the way in.
    for c in "hi \u{1b}[31mthere\u{7}".chars() {
        a.key(DoorKey::Char(c)).await;
    }
    a.key(DoorKey::Enter).await;
    assert!(a.said("Your line: hi"), "{}", a.screen());
    assert!(!a.said('\u{1b}'.to_string().as_str()));
    a.go(&["send:"]).await;
    let sent = a.finish().await;
    let w = board.world().await;
    let line = w.social.mail.iter().find_map(|d| match &d.body {
        russh_bbs_door_saltmarch::social::Body::Letter(l) => l.line.clone(),
        _ => None,
    });
    let line = line.unwrap();
    assert!(
        !line.contains('\u{1b}') && !line.contains('\u{7}'),
        "{line:?}"
    );
    // Bob reads it.
    let mut b = board.visit(2, "bob", 4).await;
    assert!(b.said("A letter from ann"), "{}", b.screen());
    b.past_report().await;
    b.go(&["go:court", "go:letters"]).await;
    assert!(b.said("ann says: hi"), "{}", b.screen());
    // Leaving is a key, and a letter on the highlight is a letter read: it is saved as read.
    b.key(DoorKey::Escape).await;
    let read = b.finish().await;
    assert_eq!(board.clan(2).await.social.unread(), 0);
    assert_eq!(board.clan(2).await.social.inbox.len(), 1);
    // The post is limited: seven lines in a minute are too many for the session...
    let mut a = board.visit(1, "ann", 5).await;
    a.go(&["go:court", "go:letters", "compose"]).await;
    let mut refused = false;
    for _ in 0..8 {
        a.go(&["line"]).await;
        for c in "again".chars() {
            a.key(DoorKey::Char(c)).await;
        }
        a.key(DoorKey::Enter).await;
        if a.said("Slow down") {
            refused = true;
            break;
        }
    }
    assert!(refused, "the board's limit of six lines a minute applies");
    // ...and a recipient already holding two of her letters takes no more.
    a.go(&["send:"]).await;
    a.finish().await;
    // (Bob read the first, so two of hers wait now; a third in a row is turned away.)
    for seed in [6, 7] {
        let mut a = board.visit(1, "ann", seed).await;
        a.past_report().await;
        a.go(&["go:court", "go:letters", "compose", "send:"]).await;
        a.finish().await;
    }
    let mut a = board.visit(1, "ann", 8).await;
    assert!(a.said("already has letters from you"), "{}", a.screen());
    a.past_report().await;
    a.finish().await;
    // Bob blocks her: nothing more reaches him, and ann is not told.
    let mut b = board.visit(2, "bob", 9).await;
    b.past_report().await;
    b.go(&["go:court", "go:letters", "block"]).await;
    b.finish().await;
    let mail_before = board.world().await.social.mail.len();
    let mut a = board.visit(1, "ann", 10).await;
    a.past_report().await;
    a.go(&["go:court", "go:letters", "compose", "send:"]).await;
    a.finish().await;
    assert_eq!(
        board.world().await.social.mail.len(),
        mail_before,
        "blocked, quietly"
    );
    for log in [&sent, &read] {
        replay(&board, log).await;
    }
}

// ------------------------------------------------------------------------------------------
// The league and the hall.

#[tokio::test]
async fn the_league_ranks_renown_and_the_world_has_what_each_clan_last_saved() {
    let board = Board::new(DAY);
    found(&board, 1, "ann").await;
    found(&board, 2, "bob").await;
    board.edit(1, |c| grown(c, 3)).await;
    show_the_post(&board, 1, "ann", 3).await;
    show_the_post(&board, 2, "bob", 4).await;
    let (w, ann, bob) = (
        board.world().await,
        board.clan(1).await,
        board.clan(2).await,
    );
    let cat = Catalog::embedded().unwrap();
    for (ix, clan) in [(0, &ann), (1, &bob)] {
        assert_eq!(
            w.clans[ix].score,
            renown::of(&cat, clan, &w, board.day()).total(),
            "the league number is renown"
        );
    }
    assert!(w.clans[0].score > w.clans[1].score);
    let mut a = board.visit(1, "ann", 5).await;
    a.past_report().await;
    a.go(&["go:court", "go:league"]).await;
    assert!(a.said("ranked by renown"), "{}", a.screen());
    assert!(a.said("you are number 1 of 2"), "{}", a.screen());
}

#[tokio::test]
async fn a_season_ends_in_the_book_and_each_clan_writes_where_it_stood() {
    let board = Board::new(DAY);
    found(&board, 1, "ann").await;
    found(&board, 2, "bob").await;
    board.edit(1, |c| grown(c, 2)).await;
    show_the_post(&board, 1, "ann", 3).await;
    show_the_post(&board, 2, "bob", 4).await;
    let before = board.world().await;
    assert!(before.social.book.is_empty());
    board
        .store
        .end_season(
            &board.id,
            &AuditEvent::new(Actor::System, Action::DoorSeasonEnded, Target::None),
        )
        .await
        .unwrap();
    // The next season's first visit makes its world, carrying the book.
    let mut a = board.visit(1, "ann", 5).await;
    a.past_report().await;
    a.go(&["go:court", "go:hall"]).await;
    a.finish().await;
    let w = board.world().await;
    assert_eq!(w.season, 2);
    assert_eq!(w.social.book.len(), 1);
    assert_eq!(w.social.book[0].season, 1);
    assert_eq!(
        w.social.book[0].top[0].0, "ann",
        "the strongest hold topped the league"
    );
    assert!(w.social.book[0].ending.contains("ann"));
    let clan = board.clan(1).await;
    assert_eq!(clan.season, 2);
    let line = clan.hall.last().unwrap();
    assert!(
        line.season == 1 && line.text.contains("1st of 2") && line.text.contains("renown"),
        "{line:?}"
    );
    // Bob's turn: he reads where he stood, from the book the new world carries.
    let mut b = board.visit(2, "bob", 6).await;
    b.past_report().await;
    b.go(&["go:court", "go:hall"]).await;
    assert!(b.said("in the book"), "{}", b.screen());
    b.finish().await;
    let clan = board.clan(2).await;
    assert_eq!(clan.hall.last().unwrap().season, 1);
    assert!(clan.social.pending.is_empty() && clan.social.inbox.is_empty());
    assert!(w.social.orders.len() <= caps::ORDERS_CAP);
}
