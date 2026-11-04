//! The balance harness hook: run a door's engine headless for N simulated days.
//!
//! A [`Simulation`] is an in-memory board with one door: a [`MemoryDoorStore`], a manual clock
//! and any number of simulated players. Each player is a [`Bot`] that reads the door's text view
//! (the same lines a screen reader gets) and answers with a key. The simulation plays every
//! player's visit for a day through the real save path (open, keys, flush with revision
//! checks), then advances the clock a day and runs the daily tick exactly as the scheduler
//! would, with the same catch-up cap. A game's balance test builds bots for its strategies, runs
//! a few hundred simulated days and reads the saves and world back out of
//! [`Simulation::store`] to report its curves. The framework counts only what is game-neutral
//! ([`SimReport`]); the game owns every other number.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use russh_bbs_core::audit::AuditEvent;
use russh_bbs_core::doors::{
    DoorId, DoorStore, DoorStoreError, MAX_SAVE_BYTES, MAX_WORLD_BYTES, Revision, SaveWrite,
    StoredSave, StoredWorld, WorldWrite,
};
use russh_bbs_core::limits::{Clock, Day, ManualClock, SECONDS_PER_DAY};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

use crate::door::{Door, DoorEnv, Flow};
use crate::host::{FlushError, OpenError, Running};
use crate::key::DoorKey;
use crate::tick::{TickError, TickPolicy, TickReport, run_due};

/// An in-memory [`DoorStore`] with the same rules as the real ones: revisions, caps, bars,
/// seasons, tick days. For tests and the harness; a board never uses it.
#[derive(Debug, Default)]
pub struct MemoryDoorStore {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    saves: HashMap<(String, i64), StoredSave>,
    worlds: HashMap<String, StoredWorld>,
    seasons: HashMap<String, u32>,
    bars: HashMap<(String, i64), ()>,
    now: i64,
}

impl MemoryDoorStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Set the time the store stamps on writes.
    pub fn set_now(&self, at: UnixSeconds) {
        self.lock().now = at.as_raw();
    }
}

impl DoorStore for MemoryDoorStore {
    async fn load_save(
        &self,
        door: &DoorId,
        account: AccountId,
    ) -> Result<Option<StoredSave>, StoreError> {
        Ok(self
            .lock()
            .saves
            .get(&(door.as_str().to_owned(), account.as_raw()))
            .cloned())
    }

    async fn write_save(
        &self,
        door: &DoorId,
        account: AccountId,
        write: &SaveWrite<'_>,
    ) -> Result<Revision, DoorStoreError> {
        if write.bytes.len() > MAX_SAVE_BYTES {
            return Err(DoorStoreError::TooLarge {
                cap: MAX_SAVE_BYTES,
            });
        }
        let mut inner = self.lock();
        let key = (door.as_str().to_owned(), account.as_raw());
        let current = inner.saves.get(&key).map_or(Revision::NONE, |s| s.revision);
        if current != write.expected {
            return Err(DoorStoreError::Conflict);
        }
        let revision = Revision::from_raw(current.as_raw() + 1);
        let updated_at = UnixSeconds::from_raw(inner.now);
        inner.saves.insert(
            key,
            StoredSave {
                season: write.season,
                version: write.version,
                revision,
                bytes: write.bytes.to_vec(),
                updated_at,
            },
        );
        Ok(revision)
    }

    async fn load_world(&self, door: &DoorId) -> Result<Option<StoredWorld>, StoreError> {
        Ok(self.lock().worlds.get(door.as_str()).cloned())
    }

    async fn write_world(
        &self,
        door: &DoorId,
        write: &WorldWrite<'_>,
    ) -> Result<Revision, DoorStoreError> {
        if write.bytes.len() > MAX_WORLD_BYTES {
            return Err(DoorStoreError::TooLarge {
                cap: MAX_WORLD_BYTES,
            });
        }
        let mut inner = self.lock();
        let current = inner.worlds.get(door.as_str());
        let current_revision = current.map_or(Revision::NONE, |w| w.revision);
        if current_revision != write.expected {
            return Err(DoorStoreError::Conflict);
        }
        let last_tick = write.tick.or_else(|| current.and_then(|w| w.last_tick));
        let revision = Revision::from_raw(current_revision.as_raw() + 1);
        inner.worlds.insert(
            door.as_str().to_owned(),
            StoredWorld {
                version: write.version,
                revision,
                bytes: write.bytes.to_vec(),
                last_tick,
            },
        );
        Ok(revision)
    }

    async fn season(&self, door: &DoorId) -> Result<u32, StoreError> {
        Ok(self.lock().seasons.get(door.as_str()).copied().unwrap_or(1))
    }

    async fn is_barred(&self, door: &DoorId, account: AccountId) -> Result<bool, StoreError> {
        Ok(self
            .lock()
            .bars
            .contains_key(&(door.as_str().to_owned(), account.as_raw())))
    }

    async fn account_rank(&self, _account: AccountId) -> Result<Option<u8>, StoreError> {
        Ok(Some(10))
    }

    async fn reset_player(
        &self,
        door: &DoorId,
        account: AccountId,
        _audit: &AuditEvent,
    ) -> Result<bool, StoreError> {
        Ok(self
            .lock()
            .saves
            .remove(&(door.as_str().to_owned(), account.as_raw()))
            .is_some())
    }

    async fn end_season(&self, door: &DoorId, _audit: &AuditEvent) -> Result<u32, StoreError> {
        let mut inner = self.lock();
        let season = inner.seasons.entry(door.as_str().to_owned()).or_insert(1);
        *season += 1;
        Ok(*season)
    }

    async fn bar_player(
        &self,
        door: &DoorId,
        account: AccountId,
        _by: AccountId,
        _audit: &AuditEvent,
    ) -> Result<bool, StoreError> {
        Ok(self
            .lock()
            .bars
            .insert((door.as_str().to_owned(), account.as_raw()), ())
            .is_none())
    }

    async fn unbar_player(
        &self,
        door: &DoorId,
        account: AccountId,
        _audit: &AuditEvent,
    ) -> Result<bool, StoreError> {
        Ok(self
            .lock()
            .bars
            .remove(&(door.as_str().to_owned(), account.as_raw()))
            .is_some())
    }
}

/// A simulated player: it reads the door's text view and answers with a key.
pub trait Bot {
    /// The next key given what the door shows (`screen` is [`DoorSession::look_around`]
    /// (crate::DoorSession::look_around)), or `None` to end today's visit. `visit` is the
    /// 1-based count of this bot's visits so far; `keys_this_visit` how many keys it has sent in
    /// this visit.
    fn next_key(&mut self, screen: &[String], visit: u32, keys_this_visit: u32) -> Option<DoorKey>;
}

/// The most keys one simulated visit may send before the harness ends it (a bot that never stops
/// would otherwise hang a test).
pub const MAX_KEYS_PER_VISIT: u32 = 2_000;

/// What the framework counts about a run. Everything else is the game's to measure from the
/// store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SimReport {
    /// Days simulated.
    pub days: u32,
    /// Visits played.
    pub visits: u32,
    /// Keys sent.
    pub keys: u64,
    /// Visits that ended because the door asked to exit.
    pub exits: u32,
    /// Visits cut off at [`MAX_KEYS_PER_VISIT`].
    pub cut_off: u32,
    /// Saves written.
    pub saves: u32,
    /// Tick days applied.
    pub ticks: u32,
    /// The largest save written, in bytes.
    pub largest_save: usize,
}

/// Why a simulation step failed.
#[derive(Debug, thiserror::Error)]
pub enum SimError {
    /// A door would not open.
    #[error(transparent)]
    Open(#[from] OpenError),
    /// A write failed.
    #[error(transparent)]
    Flush(#[from] FlushError),
    /// The daily tick failed.
    #[error(transparent)]
    Tick(#[from] TickError),
}

/// A headless board with one door.
pub struct Simulation {
    door: Arc<dyn Door>,
    store: MemoryDoorStore,
    clock: Arc<ManualClock>,
    env: DoorEnv,
    seed: u64,
    policy: TickPolicy,
    players: Vec<(AccountId, Box<dyn Bot>, u32)>,
    report: SimReport,
}

impl Simulation {
    /// A board running `door` that starts at the beginning of `start`, with sessions seeded
    /// from `seed`.
    #[must_use]
    pub fn new(door: Arc<dyn Door>, start: Day, seed: u64) -> Self {
        let store = MemoryDoorStore::new();
        store.set_now(start.start());
        Self {
            door,
            store,
            clock: Arc::new(ManualClock::new(start.start())),
            env: DoorEnv::for_tests("sim"),
            seed,
            policy: TickPolicy::default(),
            players: Vec::new(),
            report: SimReport::default(),
        }
    }

    /// Set the catch-up policy the simulated scheduler uses.
    pub fn set_policy(&mut self, policy: TickPolicy) {
        self.policy = policy;
    }

    /// Add a player driven by `bot`. Returns their account id.
    pub fn add_player(&mut self, bot: Box<dyn Bot>) -> AccountId {
        let id = AccountId::from_raw(i64::try_from(self.players.len()).unwrap_or(0) + 1);
        self.players.push((id, bot, 0));
        id
    }

    /// The store, to read saves and the world back for the game's own numbers.
    #[must_use]
    pub fn store(&self) -> &MemoryDoorStore {
        &self.store
    }

    /// The board's clock.
    #[must_use]
    pub fn clock(&self) -> &Arc<ManualClock> {
        &self.clock
    }

    /// What was counted so far.
    #[must_use]
    pub const fn report(&self) -> SimReport {
        self.report
    }

    /// Every player visits once.
    ///
    /// # Errors
    /// The first failed open or write; the run stops there.
    pub async fn play_day(&mut self) -> Result<(), SimError> {
        let clock: Arc<dyn Clock> = self.clock.clone();
        for index in 0..self.players.len() {
            let account = self.players[index].0;
            self.players[index].2 += 1;
            let visit = self.players[index].2;
            let seed = self.seed ^ (u64::from(visit) << 32) ^ u64::try_from(account.as_raw()).unwrap_or(0);
            let mut running = Running::open(
                &self.store,
                Arc::clone(&self.door),
                account,
                &self.env,
                Arc::clone(&clock),
                seed,
            )
            .await?;
            self.report.visits += 1;
            let mut sent = 0u32;
            loop {
                if sent >= MAX_KEYS_PER_VISIT {
                    self.report.cut_off += 1;
                    break;
                }
                let screen = running.look_around(&self.env);
                let Some(key) = self.players[index].1.next_key(&screen, visit, sent) else {
                    break;
                };
                sent += 1;
                self.report.keys += 1;
                let flow = running.key(key);
                if running.has_pending() {
                    self.report.saves += 1;
                }
                running.flush(&self.store).await?;
                if flow == Flow::Exit {
                    self.report.exits += 1;
                    break;
                }
            }
        }
        Ok(())
    }

    /// Move the clock to the start of the next day and run the daily tick, as the scheduler
    /// would.
    ///
    /// # Errors
    /// A failed tick.
    pub async fn advance_day(&mut self) -> Result<TickReport, SimError> {
        self.advance_days(1).await
    }

    /// Move the clock forward `days` whole days at once (downtime) and run the tick once.
    ///
    /// # Errors
    /// A failed tick.
    pub async fn advance_days(&mut self, days: u32) -> Result<TickReport, SimError> {
        let today = Day::of(self.clock.now());
        let next = Day::from_raw(today.as_raw() + i64::from(days));
        self.clock.set(next.start());
        self.store.set_now(next.start());
        let report = run_due(&self.store, self.door.as_ref(), self.clock.as_ref(), self.policy).await?;
        self.report.days += days;
        self.report.ticks += u32::try_from(report.applied.len()).unwrap_or(u32::MAX);
        Ok(report)
    }

    /// Run the tick for the current day without moving the clock (the first run of a new
    /// board).
    ///
    /// # Errors
    /// A failed tick.
    pub async fn tick_now(&mut self) -> Result<TickReport, SimError> {
        let report = run_due(&self.store, self.door.as_ref(), self.clock.as_ref(), self.policy).await?;
        self.report.ticks += u32::try_from(report.applied.len()).unwrap_or(u32::MAX);
        Ok(report)
    }

    /// Simulate `days` days: tick, then everyone plays, then the next day. The usual balance
    /// run.
    ///
    /// # Errors
    /// The first failure; the run stops there.
    pub async fn run_days(&mut self, days: u32) -> Result<SimReport, SimError> {
        self.tick_now().await?;
        for _ in 0..days {
            self.play_day().await?;
            self.advance_day().await?;
        }
        Ok(self.report)
    }
}

/// Seconds in a simulated day, for bots that reason about time.
pub const DAY_SECONDS: i64 = SECONDS_PER_DAY;
