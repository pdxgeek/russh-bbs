//! The save and world formats, hand-written little-endian binary (the door may use no
//! serialization crate beyond `serde` and `toml`, and the byte layout is pinned by a test).
//!
//! Both formats start at version 1. A decoder checks every length and range and refuses trailing
//! bytes: bytes it cannot read are never guessed at (the door then starts a fresh character, and
//! the board keeps the old bytes until the next save overwrites them).
//!
//! To raise a version, add a `migrate_vN` here, raise `SAVE_VERSION` or `WORLD_VERSION` in
//! `lib.rs` and add the arm to `Door::migrate`.

use crate::code::{Build, CODED_MAX, FRAGMENTS_MAX};
use crate::learn::ROWS_MAX;
use crate::player::{DONE_MAX, FIXERS_MAX, PARTS, Player};
use crate::world::{BOARD_MAX, Contract, HOLES_MAX, Learned, NetState, World};

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|[b]| b)
    }

    fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_le_bytes)
    }

    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }

    fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }

    fn i64(&mut self) -> Option<i64> {
        self.array().map(i64::from_le_bytes)
    }

    fn list(&mut self, max: usize) -> Option<Vec<u8>> {
        let n = usize::from(self.u8()?);
        (n <= max).then(|| self.take(n).map(<[u8]>::to_vec))?
    }

    fn done(self) -> Option<()> {
        self.0.is_empty().then_some(())
    }
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    fn list(&mut self, bytes: &[u8]) {
        let n = bytes.len().min(255);
        self.u8(u8::try_from(n).unwrap_or(u8::MAX));
        self.0.extend_from_slice(bytes.get(..n).unwrap_or_default());
    }
}

/// The longest name a save holds, in bytes.
pub const NAME_BYTES_MAX: usize = 40;

/// Encode a character.
#[must_use]
pub fn encode(p: &Player) -> Vec<u8> {
    let mut w = Writer::default();
    w.u64(p.seed);
    w.u32(p.season);
    w.list(
        p.name
            .as_bytes()
            .get(..p.name.len().min(NAME_BYTES_MAX))
            .unwrap_or_default(),
    );
    w.i64(p.day);
    w.u32(p.cycles);
    w.u32(p.marks);
    w.u32(p.stash);
    w.u8(u8::try_from(p.heat.min(100)).unwrap_or(100));
    w.u32(p.rep);
    for level in p.rig {
        w.u8(level);
    }
    w.u8(p.fried);
    w.list(&p.owned);
    w.list(&p.deck);
    for xp in p.xp {
        w.u32(xp);
    }
    w.u8(p.bots);
    w.u8(u8::from(p.workbench));
    w.u8(p.fence);
    for s in p.standing {
        w.u8(s.to_le_bytes()[0]);
    }
    w.u8(u8::try_from(p.done.len().min(DONE_MAX)).unwrap_or(0));
    for id in p.done.iter().take(DONE_MAX) {
        w.u32(*id);
    }
    match p.pending {
        Some(id) => {
            w.u8(1);
            w.u32(id);
        }
        None => w.u8(0),
    }
    w.u8(p.proxies);
    w.u32(p.runs);
    w.u32(p.jobs);
    w.u32(p.burns);
    w.u64(p.earned);
    // Version 2: fragments and the coded-script slots (an empty slot is three zero bytes).
    w.list(&p.fragments);
    for slot in p.coded {
        let b = slot.unwrap_or(Build {
            effect: 0,
            target: 0,
            modifier: 0,
        });
        w.u8(b.effect);
        w.u8(b.target);
        w.u8(b.modifier);
    }
    w.0
}

/// Bring a version 1 save to version 2: no fragments, no coded scripts. `None` if the bytes are
/// not a version 1 save.
///
#[must_use]
pub fn migrate_save_v1(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = bytes.to_vec();
    out.push(0);
    out.extend_from_slice(&[0; CODED_MAX * 3]);
    decode(&out).map(|_| out)
}

/// Bring a version 1 world to version 2: no season yet, nothing learned. `None` if the bytes are
/// not a version 1 world.
///
#[must_use]
pub fn migrate_world_v1(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = bytes.to_vec();
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(0);
    decode_world(&out).map(|_| out)
}

/// Decode a character, or `None` for bytes that are not one.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<Player> {
    let mut r = Reader(bytes);
    let seed = r.u64()?;
    let season = r.u32()?;
    let name = String::from_utf8(r.list(NAME_BYTES_MAX)?).ok()?;
    let day = r.i64()?;
    let cycles = r.u32()?;
    let marks = r.u32()?;
    let stash = r.u32()?;
    let heat = u32::from(r.u8()?);
    let rep = r.u32()?;
    let mut rig = [0u8; PARTS];
    for level in &mut rig {
        *level = r.u8()?;
        if *level > 3 {
            return None;
        }
    }
    let fried = r.u8()?;
    let owned = r.list(128)?;
    let deck = r.list(6)?;
    let mut xp = [0u32; 5];
    for x in &mut xp {
        *x = r.u32()?;
    }
    let bots = r.u8()?;
    let workbench = r.u8()? != 0;
    let fence = r.u8()?;
    let mut standing = [0i8; FIXERS_MAX];
    for s in &mut standing {
        *s = i8::from_le_bytes([r.u8()?]);
    }
    let n_done = usize::from(r.u8()?);
    if n_done > DONE_MAX {
        return None;
    }
    let mut done = Vec::with_capacity(n_done);
    for _ in 0..n_done {
        done.push(r.u32()?);
    }
    let pending = match r.u8()? {
        0 => None,
        1 => Some(r.u32()?),
        _ => return None,
    };
    let proxies = r.u8()?;
    let runs = r.u32()?;
    let jobs = r.u32()?;
    let burns = r.u32()?;
    let earned = r.u64()?;
    let fragments = r.list(FRAGMENTS_MAX)?;
    let mut coded = [None; CODED_MAX];
    for slot in &mut coded {
        let (effect, target, modifier) = (r.u8()?, r.u8()?, r.u8()?);
        if effect != 0 {
            *slot = Some(Build {
                effect,
                target,
                modifier,
            });
        }
    }
    r.done()?;
    Some(Player {
        seed,
        season,
        name,
        day,
        cycles,
        marks,
        stash,
        heat,
        rep,
        rig,
        fried,
        owned,
        deck,
        xp,
        bots,
        workbench,
        fence,
        standing,
        done,
        pending,
        proxies,
        runs,
        jobs,
        burns,
        earned,
        fragments,
        coded,
    })
}

/// Encode the shared world.
#[must_use]
pub fn encode_world(world: &World) -> Vec<u8> {
    let mut w = Writer::default();
    w.i64(world.day);
    w.u8(u8::try_from(world.market_pct.min(255)).unwrap_or(100));
    w.u32(world.breaches);
    w.u8(u8::try_from(world.nets.len().min(255)).unwrap_or(0));
    for net in world.nets.iter().take(255) {
        w.u8(net.corp);
        w.u8(net.tier);
        w.u8(net.alert);
        w.u8(u8::from(net.backdoor));
        w.u16(net.breaches);
        w.list(&net.holes);
    }
    w.u8(u8::try_from(world.board.len().min(BOARD_MAX)).unwrap_or(0));
    for c in world.board.iter().take(BOARD_MAX) {
        w.u32(c.id);
        w.u8(c.tmpl);
        w.u8(c.corp);
        w.u8(c.fixer);
        w.u8(c.tier);
        w.u32(c.reward);
        w.i64(c.expires);
    }
    // Version 2: the season, and what the corporations have learned.
    w.u32(world.season);
    w.u8(u8::try_from(world.learned.len().min(ROWS_MAX)).unwrap_or(0));
    for l in world.learned.iter().take(ROWS_MAX) {
        w.u8(l.corp);
        w.u16(l.sig);
        w.u8(l.uses);
    }
    w.0
}

/// Decode the shared world, or `None` for bytes that are not one.
#[must_use]
pub fn decode_world(bytes: &[u8]) -> Option<World> {
    let mut r = Reader(bytes);
    let day = r.i64()?;
    let market_pct = u32::from(r.u8()?);
    let breaches = r.u32()?;
    let n = usize::from(r.u8()?);
    let mut nets = Vec::with_capacity(n);
    for _ in 0..n {
        let corp = r.u8()?;
        let tier = r.u8()?;
        let alert = r.u8()?;
        let backdoor = r.u8()? != 0;
        let breaches = r.u16()?;
        let holes = r.list(HOLES_MAX)?;
        nets.push(NetState {
            corp,
            tier,
            alert,
            holes,
            backdoor,
            breaches,
        });
    }
    let n = usize::from(r.u8()?);
    if n > BOARD_MAX {
        return None;
    }
    let mut board = Vec::with_capacity(n);
    for _ in 0..n {
        board.push(Contract {
            id: r.u32()?,
            tmpl: r.u8()?,
            corp: r.u8()?,
            fixer: r.u8()?,
            tier: r.u8()?,
            reward: r.u32()?,
            expires: r.i64()?,
        });
    }
    let season = r.u32()?;
    let n = usize::from(r.u8()?);
    if n > ROWS_MAX {
        return None;
    }
    let mut learned = Vec::with_capacity(n);
    for _ in 0..n {
        learned.push(Learned {
            corp: r.u8()?,
            sig: r.u16()?,
            uses: r.u8()?,
        });
    }
    r.done()?;
    Some(World {
        day,
        market_pct,
        nets,
        board,
        breaches,
        season,
        learned,
    })
}
