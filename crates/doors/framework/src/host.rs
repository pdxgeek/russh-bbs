//! Running a door: the synchronous half ([`Driver`]) and the half that talks to the store
//! ([`Running`]).
//!
//! `Driver` holds a session with its RNG, clock, text limiter and queued effects, and needs no
//! store, so replays and the balance harness use it directly. `Running` adds the board's side
//! of the save API: open (check the bar, load and migrate the save and the world), and `flush`
//! (write the queued save and world edits with a compare-and-swap).

use std::sync::{Arc, Mutex, PoisonError};

use russh_bbs_core::doors::{
    DoorStore, DoorStoreError, MAX_SAVE_BYTES, Revision, SaveWrite, StoredSave, StoredWorld,
    WorldWrite,
};
use russh_bbs_core::limits::{Clock, Day};
use russh_bbs_core::store::{AccountId, StoreError};
use tracing::{error, warn};

use crate::canvas::{Canvas, DoorStyle};
use crate::ctx::{Ctx, Effects, TextBudget, WorldEdit};
use crate::door::{
    BlobKind, Door, DoorEnv, DoorSession, Flow, LOOK_AROUND_KEY, MigrateError, Save, Start,
};
use crate::key::DoorKey;
use crate::rng::DoorRng;

/// How many times a world edit is retried when another writer got there first.
const WORLD_RETRIES: usize = 4;

/// The synchronous half of a running door.
pub struct Driver {
    // Behind mutexes only so a `Driver` is `Sync` (the board's session future holds it across
    // an await) without asking every door author to avoid `Cell`; they are never contended.
    session: Mutex<Box<dyn DoorSession>>,
    rng: DoorRng,
    clock: Arc<dyn Clock>,
    season: u32,
    text: TextBudget,
    effects: Mutex<Effects>,
    /// The latest world the door may read through [`Ctx::world`].
    world: Option<Vec<u8>>,
    /// Whether the look-around view (D98) is showing instead of the door's screen.
    looking: bool,
}

impl Driver {
    /// Open a session from `start`'s data, seeding its RNG with `seed`. `clock` is the injected
    /// clock.
    pub fn open(door: &dyn Door, start: Start<'_>, seed: u64, clock: Arc<dyn Clock>) -> Self {
        // One RNG for the whole visit: `open` draws from it first, then every key does.
        let mut rng = DoorRng::from_seed(seed);
        let season = start.season;
        let world = start.world.map(<[u8]>::to_vec);
        let session = door.open(start, &mut rng);
        Self {
            session: Mutex::new(session),
            rng,
            clock,
            season,
            text: TextBudget::default(),
            effects: Mutex::new(Effects::default()),
            world,
            looking: false,
        }
    }

    /// Replace the world the door reads through [`Ctx::world`] (the board does this when it
    /// re-reads the store; a replay does it from the log).
    pub fn set_world(&mut self, world: Option<Vec<u8>>) {
        self.world = world;
    }

    /// The world the door would read now.
    #[must_use]
    pub fn world(&self) -> Option<&[u8]> {
        self.world.as_deref()
    }

    /// Whether the look-around view is showing.
    #[must_use]
    pub const fn is_looking(&self) -> bool {
        self.looking
    }

    /// Give the door one key. Effects queue up until [`Driver::take_effects`].
    ///
    /// [`LOOK_AROUND_KEY`] opens the look-around view instead of reaching the door (unless the
    /// door is taking typed text, see [`DoorSession::captures_text`]); while the view shows,
    /// any key closes it and is not passed on.
    pub fn key(&mut self, key: DoorKey) -> Flow {
        if self.looking {
            self.looking = false;
            return Flow::Stay;
        }
        let session = self
            .session
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        if key == LOOK_AROUND_KEY && !session.captures_text() {
            self.looking = true;
            return Flow::Stay;
        }
        let mut ctx = Ctx {
            now: self.clock.now(),
            season: self.season,
            rng: &mut self.rng,
            effects: self
                .effects
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner),
            text: &mut self.text,
            world: self.world.as_deref(),
        };
        session.key(key, &mut ctx)
    }

    /// Draw the door (or the look-around view), telling the canvas the clock's time.
    pub fn render(&self, canvas: &mut Canvas<'_>, env: &DoorEnv) {
        canvas.now = self.clock.now();
        let session = self.session.lock().unwrap_or_else(PoisonError::into_inner);
        if self.looking {
            canvas.clear();
            canvas.text(0, 0, "Look around (any key returns)", DoorStyle::Accent);
            for (row, line) in (1u16..).zip(session.look_around(env)) {
                canvas.text(0, row, &line, DoorStyle::Plain);
            }
        } else {
            session.render(canvas, env);
        }
    }

    /// The door's text view.
    #[must_use]
    pub fn look_around(&self, env: &DoorEnv) -> Vec<String> {
        self.session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .look_around(env)
    }

    /// What the keys so far asked to write; clears it.
    pub fn take_effects(&mut self) -> Effects {
        std::mem::take(
            self.effects
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }

    /// The size of the save waiting to be written, if the keys asked for one.
    #[must_use]
    pub fn pending_save_len(&self) -> Option<usize> {
        self.effects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .save
            .as_ref()
            .map(Vec::len)
    }

    /// Whether anything is waiting to be written.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        !self
            .effects
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    }
}

/// Why a door could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// Staff barred this player from the door.
    #[error("barred from this door")]
    Barred,
    /// The save was written by a newer version of the door than this board runs. It is left
    /// alone, never loaded and never overwritten.
    #[error("the saved game is from a newer version (found {found}, this board has {supported})")]
    TooNew {
        /// The version in the store.
        found: u32,
        /// The version this code writes.
        supported: u32,
    },
    /// A migration failed.
    #[error(transparent)]
    Migrate(#[from] MigrateError),
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// Why queued effects were not written. The door is closed after any of these.
#[derive(Debug, thiserror::Error)]
pub enum FlushError {
    /// The save changed since this session read it (another session of the same player). The
    /// stored save is untouched.
    #[error("the save changed in another session")]
    Conflict,
    /// The door tried to save more than the cap. Nothing was written.
    #[error("the door tried to save more than {MAX_SAVE_BYTES} bytes")]
    TooLarge,
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// A door running for one player, with the store side attached.
pub struct Running {
    driver: Driver,
    door: Arc<dyn Door>,
    account: AccountId,
    revision: Revision,
    /// The clock second the door's world was last read.
    world_read_at: i64,
}

/// How old (in clock seconds) the world a door reads may get before the board re-reads it.
pub const WORLD_REFRESH_SECS: i64 = 1;

/// Bring a stored blob to `current`, one migration step at a time.
fn migrated(
    door: &dyn Door,
    kind: BlobKind,
    mut version: u32,
    current: u32,
    mut bytes: Vec<u8>,
) -> Result<Vec<u8>, OpenError> {
    if version > current {
        return Err(OpenError::TooNew {
            found: version,
            supported: current,
        });
    }
    while version < current {
        bytes = door.migrate(kind, version, &bytes)?;
        version += 1;
    }
    Ok(bytes)
}

/// Load the door's world, migrated to the current world version.
///
/// # Errors
/// [`OpenError::TooNew`] for a world from a newer door, [`OpenError::Migrate`], [`OpenError::Store`].
pub async fn load_world<S: DoorStore>(
    store: &S,
    door: &dyn Door,
) -> Result<Option<(Vec<u8>, Revision, Option<Day>)>, OpenError> {
    let info = door.info();
    let Some(StoredWorld {
        version,
        revision,
        bytes,
        last_tick,
    }) = store.load_world(&info.id).await?
    else {
        return Ok(None);
    };
    let bytes = migrated(door, BlobKind::World, version, info.world_version, bytes)?;
    Ok(Some((bytes, revision, last_tick)))
}

impl Running {
    /// Open `door` for `account`: refuse a barred player, load and migrate the save and the
    /// world, and build the session. `seed` seeds the session's RNG (the server passes
    /// operating-system randomness; tests pass a fixed number).
    ///
    /// # Errors
    /// See [`OpenError`].
    pub async fn open<S: DoorStore>(
        store: &S,
        door: Arc<dyn Door>,
        account: AccountId,
        env: &DoorEnv,
        clock: Arc<dyn Clock>,
        seed: u64,
    ) -> Result<Self, OpenError> {
        let info = door.info();
        if store.is_barred(&info.id, account).await? {
            return Err(OpenError::Barred);
        }
        let season = store.season(&info.id).await?;
        let stored = store.load_save(&info.id, account).await?;
        let (save_bytes, saved_season, revision) = match stored {
            Some(StoredSave {
                season: saved_season,
                version,
                revision,
                bytes,
                ..
            }) => (
                Some(migrated(
                    door.as_ref(),
                    BlobKind::Save,
                    version,
                    info.save_version,
                    bytes,
                )?),
                saved_season,
                revision,
            ),
            None => (None, season, Revision::NONE),
        };
        let world = load_world(store, door.as_ref()).await?.map(|w| w.0);
        let start = Start {
            save: match &save_bytes {
                Some(bytes) => Save::Loaded {
                    bytes,
                    season: saved_season,
                },
                None => Save::Fresh,
            },
            world: world.as_deref(),
            env,
            today: Day::of(clock.now()),
            season,
        };
        let world_read_at = clock.now().as_raw();
        let driver = Driver::open(door.as_ref(), start, seed, clock);
        Ok(Self {
            driver,
            door,
            account,
            revision,
            world_read_at,
        })
    }

    /// Whether the world the door reads is at least [`WORLD_REFRESH_SECS`] old by the clock.
    #[must_use]
    pub fn world_is_stale(&self) -> bool {
        self.driver
            .clock
            .now()
            .as_raw()
            .saturating_sub(self.world_read_at)
            >= WORLD_REFRESH_SECS
    }

    /// Re-read the door's world from the store (one read), so the next key sees other players'
    /// edits. Read-only: it changes nothing in the store.
    ///
    /// # Errors
    /// [`OpenError::Store`], [`OpenError::Migrate`] or [`OpenError::TooNew`] as for opening.
    pub async fn refresh_world<S: DoorStore>(&mut self, store: &S) -> Result<(), OpenError> {
        let world = load_world(store, self.door.as_ref()).await?.map(|w| w.0);
        self.driver.set_world(world);
        self.world_read_at = self.driver.clock.now().as_raw();
        Ok(())
    }

    /// [`Running::refresh_world`] when [`Running::world_is_stale`]; the board calls it before a
    /// key.
    ///
    /// # Errors
    /// See [`Running::refresh_world`].
    pub async fn refresh_world_if_stale<S: DoorStore>(
        &mut self,
        store: &S,
    ) -> Result<(), OpenError> {
        if self.world_is_stale() {
            self.refresh_world(store).await?;
        }
        Ok(())
    }

    /// Give the door one key.
    pub fn key(&mut self, key: DoorKey) -> Flow {
        self.driver.key(key)
    }

    /// Draw the door.
    pub fn render(&self, canvas: &mut Canvas<'_>, env: &DoorEnv) {
        self.driver.render(canvas, env);
    }

    /// The door's text view.
    #[must_use]
    pub fn look_around(&self, env: &DoorEnv) -> Vec<String> {
        self.driver.look_around(env)
    }

    /// Whether a save or a world change is waiting to be written.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        self.driver.has_pending()
    }

    /// The size of the save waiting to be written, if any.
    #[must_use]
    pub fn pending_save_len(&self) -> Option<usize> {
        self.driver.pending_save_len()
    }

    /// The door being run.
    #[must_use]
    pub fn door(&self) -> &Arc<dyn Door> {
        &self.door
    }

    /// Write what the keys asked for: the save (compare-and-swap on the revision this session
    /// read, so a second session of the same player cannot be overwritten silently) and the
    /// world edits (each retried on a conflict). On an error nothing further is written and the
    /// caller closes the door.
    ///
    /// # Errors
    /// See [`FlushError`].
    pub async fn flush<S: DoorStore>(&mut self, store: &S) -> Result<(), FlushError> {
        let effects = self.driver.take_effects();
        let info = self.door.info();
        if let Some(bytes) = effects.save {
            if bytes.len() > MAX_SAVE_BYTES {
                error!(door = %info.id, bytes = bytes.len(), "a door tried to save over the cap");
                return Err(FlushError::TooLarge);
            }
            let season = self.driver.season;
            let write = SaveWrite {
                expected: self.revision,
                season,
                version: info.save_version,
                bytes: &bytes,
            };
            match store.write_save(&info.id, self.account, &write).await {
                Ok(revision) => self.revision = revision,
                Err(DoorStoreError::Conflict) => return Err(FlushError::Conflict),
                Err(DoorStoreError::TooLarge { .. }) => return Err(FlushError::TooLarge),
                Err(DoorStoreError::Store(error)) => return Err(FlushError::Store(error)),
            }
        }
        let edited = !effects.world.is_empty();
        for edit in effects.world {
            self.apply_world_edit(store, edit).await?;
        }
        if edited {
            // The door sees its own edit (and anyone else's before it) on the next key.
            self.refresh_world(store)
                .await
                .map_err(|error| match error {
                    OpenError::Store(error) => FlushError::Store(error),
                    other => FlushError::Store(StoreError::Corrupt(other.to_string())),
                })?;
        }
        Ok(())
    }

    async fn apply_world_edit<S: DoorStore>(
        &self,
        store: &S,
        mut edit: WorldEdit,
    ) -> Result<(), FlushError> {
        let info = self.door.info();
        for _ in 0..WORLD_RETRIES {
            let current =
                load_world(store, self.door.as_ref())
                    .await
                    .map_err(|error| match error {
                        OpenError::Store(error) => FlushError::Store(error),
                        other => FlushError::Store(StoreError::Corrupt(other.to_string())),
                    })?;
            let (bytes, revision) = match &current {
                Some((bytes, revision, _)) => (Some(bytes.as_slice()), *revision),
                None => (None, Revision::NONE),
            };
            let Some(new) = edit(bytes) else {
                return Ok(());
            };
            let write = WorldWrite {
                expected: revision,
                version: info.world_version,
                bytes: &new,
                tick: None,
            };
            match store.write_world(&info.id, &write).await {
                Ok(_) => return Ok(()),
                Err(DoorStoreError::Conflict) => {}
                Err(DoorStoreError::TooLarge { .. }) => return Err(FlushError::TooLarge),
                Err(DoorStoreError::Store(error)) => return Err(FlushError::Store(error)),
            }
        }
        warn!(door = %info.id, "a world edit kept conflicting and was dropped");
        Ok(())
    }
}
