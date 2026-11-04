//! The shared world: what heroes have changed for everyone this season. The dungeon's layout is
//! not here (it is a pure function of the seed); only the changes are: opened shortcuts, slain
//! guardians, the fallen awaiting rescue, the hall, and a short run of news.
//!
//! Every change is a small pure function on [`World`], used twice: on the session's own copy so
//! the hero sees it at once, and inside a `Ctx::edit_world` closure so it merges into whatever
//! the store holds now (another hero may have changed it meanwhile). All of them are idempotent.

use crate::codec::{Reader, Writer};
use crate::content::{Content, ShortcutKind};
use crate::content_town::ProjectEffect;
use crate::rules::{DEPTH_MAX, RECOVERY_DAYS};

/// The world blob's format version.
pub(crate) const WORLD_FORMAT: u32 = 2;

/// A rescue is remembered this long, so a hero who comes back late is still carried home.
const RESCUE_KEEP_DAYS: i64 = 14;
const NEWS_MAX: usize = 6;
const DEEPEST_MAX: usize = 8;
const FALLEN_MAX: usize = 16;
/// Most pledges the world holds (a hero's pledge to an unfinished project is one entry).
pub(crate) const PLEDGES_MAX: usize = 3000;

/// What a hero has given to one project, in all. The world keeps the *largest* total it has
/// heard from each hero, so a pledge repeated, replayed or applied twice credits nothing twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pledge {
    pub(crate) hero: String,
    pub(crate) project: String,
    pub(crate) coin: u32,
    pub(crate) salvage: u32,
}

/// What the finished projects have done to the town, for everyone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TownEffects {
    pub(crate) forge: u32,
    pub(crate) temple: u32,
    pub(crate) market: u32,
    pub(crate) guild: u32,
    pub(crate) library: bool,
    pub(crate) reliquary: bool,
    /// Levels a project lift reaches.
    pub(crate) lifts: Vec<u8>,
}

/// A hero who died below and may be carried home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fallen {
    pub(crate) name: String,
    pub(crate) depth: u8,
    pub(crate) room: u8,
    /// The calendar day they fell.
    pub(crate) day: i64,
    pub(crate) rescued_by: Option<String>,
}

/// The shared world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct World {
    pub(crate) season: u32,
    /// Bumped when the Cold Furnace falls; the dungeon is re-seeded.
    pub(crate) epoch: u16,
    /// The last calendar day the tick applied.
    pub(crate) day: i64,
    pub(crate) shortcuts: Vec<(u8, ShortcutKind)>,
    /// Guardians slain this epoch: band and slayer.
    pub(crate) slain: Vec<(u8, String)>,
    /// Who felled the Cold Furnace, until the next tick collapses the dungeon.
    pub(crate) finale: Option<String>,
    /// The hall: deepest heroes by name.
    pub(crate) deepest: Vec<(String, u8)>,
    pub(crate) fallen: Vec<Fallen>,
    pub(crate) news: Vec<String>,
    /// Gifts to the town's projects (see [`Pledge`]).
    pub(crate) pledges: Vec<Pledge>,
}

/// A change to the world, as a value, so the engine can hand it to the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Change {
    Shortcut(u8, ShortcutKind, String),
    Slain(u8, String),
    Depth(String, u8),
    Fall(Fallen),
    Rescue(String, String),
    /// A hero's running total to one project.
    Pledge(Pledge),
    /// A line for the tavern board.
    News(String),
}

impl World {
    pub(crate) fn new(season: u32) -> Self {
        Self {
            season,
            epoch: 0,
            day: 0,
            shortcuts: Vec::new(),
            slain: Vec::new(),
            finale: None,
            deepest: Vec::new(),
            fallen: Vec::new(),
            news: Vec::new(),
            pledges: Vec::new(),
        }
    }

    /// The stored world for `season`, or a fresh one (also when the stored one is from an older
    /// season: staff ended it, and nothing is deleted, so the new season starts clean).
    pub(crate) fn for_season(bytes: Option<&[u8]>, season: u32) -> Self {
        bytes
            .and_then(Self::decode)
            .filter(|w| w.season == season)
            .unwrap_or_else(|| Self::new(season))
    }

    #[cfg(test)]
    pub(crate) fn shortcut_at(&self, depth: u8) -> Option<ShortcutKind> {
        self.shortcuts
            .iter()
            .find(|(d, _)| *d == depth)
            .map(|(_, k)| *k)
    }

    pub(crate) fn is_slain(&self, band: u8) -> bool {
        self.slain.iter().any(|(b, _)| *b == band)
    }

    pub(crate) fn add_news(&mut self, line: String) {
        if self.news.last() != Some(&line) {
            self.news.push(line);
        }
        while self.news.len() > NEWS_MAX {
            self.news.remove(0);
        }
    }

    /// Apply one change. Idempotent.
    pub(crate) fn apply(&mut self, change: &Change) {
        match change {
            Change::Shortcut(depth, kind, by) => {
                if let Some(slot) = self.shortcuts.iter_mut().find(|(d, _)| d == depth) {
                    if kind.flat_cost() < slot.1.flat_cost() {
                        slot.1 = *kind;
                    }
                } else if *depth >= 1 && *depth <= DEPTH_MAX {
                    self.shortcuts.push((*depth, *kind));
                    self.shortcuts.sort_by_key(|(d, _)| *d);
                    self.add_news(format!("{by} opened a {} on level {depth}.", kind.label()));
                }
            }
            Change::Slain(band, by) => {
                if !self.is_slain(*band) {
                    self.slain.push((*band, by.clone()));
                    self.slain.sort_by_key(|(b, _)| *b);
                    self.add_news(format!("{by} slew the guardian of band {band}."));
                    if *band >= 5 {
                        self.finale = Some(by.clone());
                    }
                }
            }
            Change::Depth(name, depth) => {
                match self.deepest.iter_mut().find(|(n, _)| n == name) {
                    Some(slot) => slot.1 = slot.1.max(*depth),
                    None => self.deepest.push((name.clone(), *depth)),
                }
                self.deepest
                    .sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                self.deepest.truncate(DEEPEST_MAX);
            }
            Change::Fall(fallen) => {
                let open = self
                    .fallen
                    .iter()
                    .any(|f| f.name == fallen.name && f.day == fallen.day);
                if !open {
                    self.fallen.push(fallen.clone());
                    while self.fallen.len() > FALLEN_MAX {
                        self.fallen.remove(0);
                    }
                    self.add_news(format!(
                        "{} did not come back from level {}.",
                        fallen.name, fallen.depth
                    ));
                }
            }
            Change::Pledge(pledge) => {
                let room = self.pledges.len() < PLEDGES_MAX;
                match self
                    .pledges
                    .iter_mut()
                    .find(|p| p.hero == pledge.hero && p.project == pledge.project)
                {
                    Some(slot) => {
                        slot.coin = slot.coin.max(pledge.coin);
                        slot.salvage = slot.salvage.max(pledge.salvage);
                    }
                    None if room => self.pledges.push(pledge.clone()),
                    None => {}
                }
            }
            Change::News(line) => self.add_news(line.clone()),
            Change::Rescue(name, by) => {
                let mut rescued = false;
                for f in &mut self.fallen {
                    if f.name == *name && f.rescued_by.is_none() {
                        f.rescued_by = Some(by.clone());
                        rescued = true;
                    }
                }
                if rescued {
                    self.add_news(format!("{by} carried {name} home."));
                }
            }
        }
    }

    /// Coin and salvage given to `project` by everyone.
    pub(crate) fn progress(&self, project: &str) -> (u32, u32) {
        self.pledges
            .iter()
            .filter(|p| p.project == project)
            .fold((0u32, 0u32), |(c, s), p| {
                (c.saturating_add(p.coin), s.saturating_add(p.salvage))
            })
    }

    /// Whether a project is finished: fully paid for, and everything it needs is finished.
    pub(crate) fn is_done(&self, content: &Content, project: &str) -> bool {
        self.done_within(content, project, 0)
    }

    fn done_within(&self, content: &Content, project: &str, depth: u8) -> bool {
        let Some(def) = content.project(project) else {
            return false;
        };
        if depth > 6 {
            return false;
        }
        let (coin, salvage) = self.progress(project);
        coin >= def.coin
            && salvage >= def.salvage
            && def
                .requires
                .iter()
                .all(|r| self.done_within(content, r, depth + 1))
    }

    /// Whether the town can take gifts toward `project` now (not finished, requirements met).
    pub(crate) fn is_open(&self, content: &Content, project: &str) -> bool {
        content.project(project).is_some_and(|def| {
            !self.is_done(content, project) && def.requires.iter().all(|r| self.is_done(content, r))
        })
    }

    /// What the finished projects have done.
    pub(crate) fn town(&self, content: &Content) -> TownEffects {
        let mut out = TownEffects::default();
        for p in &content.projects.project {
            if !self.is_done(content, &p.id) {
                continue;
            }
            match p.effect {
                ProjectEffect::Forge { level } => out.forge = out.forge.max(level),
                ProjectEffect::Temple { level } => out.temple = out.temple.max(level),
                ProjectEffect::Market { level } => out.market = out.market.max(level),
                ProjectEffect::Guild { level } => out.guild = out.guild.max(level),
                ProjectEffect::Library => out.library = true,
                ProjectEffect::Reliquary => out.reliquary = true,
                ProjectEffect::Lift { depth } => out.lifts.push(depth),
            }
        }
        out.lifts.sort_unstable();
        out
    }

    /// Every way down from the town: shortcuts heroes opened and lifts the town built. A lift
    /// the town built does not collapse with the dungeon.
    pub(crate) fn ways_down(&self, content: &Content) -> Vec<(u8, ShortcutKind)> {
        let mut out = self.shortcuts.clone();
        for depth in self.town(content).lifts {
            match out.iter_mut().find(|(d, _)| *d == depth) {
                Some(slot) => slot.1 = ShortcutKind::Lift,
                None => out.push((depth, ShortcutKind::Lift)),
            }
        }
        out.sort_by_key(|(d, _)| *d);
        out
    }

    /// The fallen entry for `name` that is still open on `today`.
    pub(crate) fn fallen_of(&self, name: &str, today: i64) -> Option<&Fallen> {
        self.fallen
            .iter()
            .rev()
            .find(|f| f.name == name && today - f.day <= RESCUE_KEEP_DAYS)
    }

    /// One day of the world: expire the fallen, and collapse the dungeon if the Furnace fell.
    pub(crate) fn tick(&mut self, day: i64) {
        self.day = day;
        self.fallen.retain(|f| {
            let age = day - f.day;
            age <= RECOVERY_DAYS + 1 || (f.rescued_by.is_some() && age <= RESCUE_KEEP_DAYS)
        });
        if let Some(by) = self.finale.take() {
            self.epoch = self.epoch.saturating_add(1);
            self.shortcuts.clear();
            self.slain.clear();
            self.fallen.clear();
            self.add_news(format!(
                "{by} put out the Cold Furnace. The Emberdeep collapses and reshapes itself."
            ));
        }
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(self.season);
        w.u16(self.epoch);
        w.i64(self.day);
        w.count(self.shortcuts.len());
        for (depth, kind) in &self.shortcuts {
            w.u8(*depth);
            w.u8(kind_code(*kind));
        }
        w.count(self.slain.len());
        for (band, by) in &self.slain {
            w.u8(*band);
            w.str(by);
        }
        w.bool(self.finale.is_some());
        if let Some(by) = &self.finale {
            w.str(by);
        }
        w.count(self.deepest.len());
        for (name, depth) in &self.deepest {
            w.str(name);
            w.u8(*depth);
        }
        w.count(self.fallen.len());
        for f in &self.fallen {
            w.str(&f.name);
            w.u8(f.depth);
            w.u8(f.room);
            w.i64(f.day);
            w.bool(f.rescued_by.is_some());
            if let Some(by) = &f.rescued_by {
                w.str(by);
            }
        }
        w.count(self.news.len());
        for line in &self.news {
            w.str(line);
        }
        // Format 2: the town's pledges.
        w.u16(u16::try_from(self.pledges.len()).unwrap_or(u16::MAX));
        for p in &self.pledges {
            w.str(&p.hero);
            w.str(&p.project);
            w.u32(p.coin);
            w.u32(p.salvage);
        }
        w.0
    }

    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        let season = r.u32()?;
        let epoch = r.u16()?;
        let day = r.i64()?;
        let mut shortcuts = Vec::new();
        for _ in 0..r.count()? {
            let depth = r.u8()?;
            shortcuts.push((depth, kind_from(r.u8()?)?));
        }
        let mut slain = Vec::new();
        for _ in 0..r.count()? {
            let band = r.u8()?;
            slain.push((band, r.str()?));
        }
        let finale = if r.bool()? { Some(r.str()?) } else { None };
        let mut deepest = Vec::new();
        for _ in 0..r.count()? {
            let name = r.str()?;
            deepest.push((name, r.u8()?));
        }
        let mut fallen = Vec::new();
        for _ in 0..r.count()? {
            let name = r.str()?;
            let depth = r.u8()?;
            let room = r.u8()?;
            let day = r.i64()?;
            let rescued_by = if r.bool()? { Some(r.str()?) } else { None };
            fallen.push(Fallen {
                name,
                depth,
                room,
                day,
                rescued_by,
            });
        }
        let mut news = Vec::new();
        for _ in 0..r.count()? {
            news.push(r.str()?);
        }
        let mut pledges = Vec::new();
        for _ in 0..r.u16()? {
            pledges.push(Pledge {
                hero: r.str()?,
                project: r.str()?,
                coin: r.u32()?,
                salvage: r.u32()?,
            });
        }
        if pledges.len() > PLEDGES_MAX {
            return None;
        }
        r.done().then_some(Self {
            season,
            epoch,
            day,
            shortcuts,
            slain,
            finale,
            deepest,
            fallen,
            news,
            pledges,
        })
    }
}

/// The empty format-2 section appended to a format-1 world.
pub(crate) fn upgrade_v1(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = bytes.to_vec();
    out.extend_from_slice(&0u16.to_le_bytes());
    World::decode(&out).map(|_| out)
}

pub(crate) fn kind_code(kind: ShortcutKind) -> u8 {
    match kind {
        ShortcutKind::Lift => 0,
        ShortcutKind::Bridge => 1,
        ShortcutKind::Rope => 2,
    }
}

pub(crate) fn kind_from(code: u8) -> Option<ShortcutKind> {
    match code {
        0 => Some(ShortcutKind::Lift),
        1 => Some(ShortcutKind::Bridge),
        2 => Some(ShortcutKind::Rope),
        _ => None,
    }
}

/// The shared world in a few numbers, for tests, the balance harness and the staff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldSummary {
    /// The dungeon's epoch (bumped when the Cold Furnace falls).
    pub epoch: u16,
    /// The last day the tick applied.
    pub day: i64,
    /// Shortcuts open: level and kind name.
    pub shortcuts: Vec<(u8, String)>,
    /// Guardians slain: band.
    pub slain: Vec<u8>,
    /// The hall, deepest first.
    pub deepest: Vec<(String, u8)>,
    /// Heroes waiting to be found.
    pub fallen: usize,
    /// Heroes carried home.
    pub rescued: usize,
    /// The tavern board.
    pub news: Vec<String>,
    /// Coin and salvage given to each project so far (unfinished and finished alike).
    pub pledged: Vec<(String, u32, u32)>,
}

/// Read a world blob into a [`WorldSummary`].
#[must_use]
pub fn summarize_world(bytes: &[u8]) -> Option<WorldSummary> {
    let w = World::decode(bytes)?;
    Some(WorldSummary {
        epoch: w.epoch,
        day: w.day,
        shortcuts: w
            .shortcuts
            .iter()
            .map(|(d, k)| (*d, k.label().to_owned()))
            .collect(),
        slain: w.slain.iter().map(|(b, _)| *b).collect(),
        deepest: w.deepest.clone(),
        fallen: w.fallen.iter().filter(|f| f.rescued_by.is_none()).count(),
        rescued: w.fallen.iter().filter(|f| f.rescued_by.is_some()).count(),
        news: w.news.clone(),
        pledged: {
            let mut ids: Vec<&String> = w.pledges.iter().map(|p| &p.project).collect();
            ids.sort();
            ids.dedup();
            ids.into_iter()
                .map(|id| {
                    let (c, s) = w.progress(id);
                    (id.clone(), c, s)
                })
                .collect()
        },
    })
}

/// The town in a few numbers, for tests, the balance harness and the staff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TownSummary {
    /// Finished projects, by id.
    pub done: Vec<String>,
    /// Every way down from the town: level and kind name.
    pub ways_down: Vec<(u8, String)>,
    /// Forge, temple, market and guild levels.
    pub levels: [u32; 4],
    /// Whether the library is open.
    pub library: bool,
    /// Whether the reliquary stands.
    pub reliquary: bool,
}

/// Read a world blob into a [`TownSummary`].
#[must_use]
pub fn summarize_town(bytes: &[u8], content: &Content) -> Option<TownSummary> {
    let w = World::decode(bytes)?;
    let t = w.town(content);
    Some(TownSummary {
        done: content
            .projects
            .project
            .iter()
            .filter(|p| w.is_done(content, &p.id))
            .map(|p| p.id.clone())
            .collect(),
        ways_down: w
            .ways_down(content)
            .iter()
            .map(|(d, k)| (*d, k.label().to_owned()))
            .collect(),
        levels: [t.forge, t.temple, t.market, t.guild],
        library: t.library,
        reliquary: t.reliquary,
    })
}
