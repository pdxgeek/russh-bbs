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
use crate::crew::{Crew, Member, Op, OpState};
use crate::player::{
    ARCS_MAX, ArcState, DONE_MAX, FIXERS_MAX, LEGEND_MAX, Legend, MODS, PARTS, Player, SENT_MAX,
    STATS, Sent, Stake, Ties,
};
use crate::social::{
    Bounty, Deed, Delivery, Effect, Order, Pair, Posted, SeasonRecord, Social, Tallies,
    BOUNTIES_MAX, DEEDS_MAX, DELIVERIES_MAX, HALL_MAX, HIST_MAX, ORDERS_MAX, PAIRS_MAX, POSTED_MAX,
};
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

    fn string(&mut self, max: usize) -> Option<String> {
        String::from_utf8(self.list(max)?).ok()
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

    fn string(&mut self, text: &str, max: usize) {
        let cut = (0..=text.len().min(max))
            .rev()
            .find(|i| text.is_char_boundary(*i))
            .unwrap_or(0);
        self.list(text.as_bytes().get(..cut).unwrap_or_default());
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
    encode_ties(&mut w, &p.ties);
    w.0
}

fn encode_ties(w: &mut Writer, t: &Ties) {
    // Version 3: the social layer and the story.
    w.i64(t.born);
    w.u8(u8::from(t.pvp));
    w.u8(u8::from(t.crews_open));
    w.i64(t.pvp_off_on);
    w.u8(t.honeypot);
    w.u32(t.credited_to);
    w.u8(u8::try_from(t.sent.len().min(SENT_MAX)).unwrap_or(0));
    for s in t.sent.iter().take(SENT_MAX) {
        w.u32(s.nonce);
        w.u8(s.kind);
        w.i64(s.day);
        w.u32(s.marks);
        w.u8(s.cycles);
    }
    match t.stake {
        Some(s) => {
            w.u8(1);
            w.u32(s.op);
            w.u32(s.cycles);
        }
        None => w.u8(0),
    }
    for a in t.arcs {
        w.u8(a.step);
        w.u8(u8::from(a.awaiting));
        w.i64(a.since);
        w.u8(a.ending);
    }
    w.u32(t.flags);
    w.u32(t.endings);
    w.u8(u8::try_from(t.legend.len().min(LEGEND_MAX)).unwrap_or(0));
    for l in t.legend.iter().take(LEGEND_MAX) {
        w.u32(l.season);
        w.u8(l.ending);
        w.u32(l.rep);
        w.u32(l.jobs);
    }
    for s in t.stats {
        w.u16(s);
    }
    w.u16(t.perks);
    for m in t.mods {
        w.u8(m);
    }
}

fn decode_ties(r: &mut Reader<'_>) -> Option<Ties> {
    let born = r.i64()?;
    let pvp = r.u8()? != 0;
    let crews_open = r.u8()? != 0;
    let pvp_off_on = r.i64()?;
    let honeypot = r.u8()?;
    if honeypot > crate::social::HONEYPOT_MAX {
        return None;
    }
    let credited_to = r.u32()?;
    let n = usize::from(r.u8()?);
    if n > SENT_MAX {
        return None;
    }
    let mut sent = Vec::with_capacity(n);
    for _ in 0..n {
        sent.push(Sent {
            nonce: r.u32()?,
            kind: r.u8()?,
            day: r.i64()?,
            marks: r.u32()?,
            cycles: r.u8()?,
        });
    }
    let stake = match r.u8()? {
        0 => None,
        1 => Some(Stake {
            op: r.u32()?,
            cycles: r.u32()?,
        }),
        _ => return None,
    };
    let mut arcs = [ArcState::default(); ARCS_MAX];
    for a in &mut arcs {
        *a = ArcState {
            step: r.u8()?,
            awaiting: r.u8()? != 0,
            since: r.i64()?,
            ending: r.u8()?,
        };
    }
    let flags = r.u32()?;
    let endings = r.u32()?;
    let n = usize::from(r.u8()?);
    if n > LEGEND_MAX {
        return None;
    }
    let mut legend = Vec::with_capacity(n);
    for _ in 0..n {
        legend.push(Legend {
            season: r.u32()?,
            ending: r.u8()?,
            rep: r.u32()?,
            jobs: r.u32()?,
        });
    }
    let mut stats = [0u16; STATS];
    for s in &mut stats {
        *s = r.u16()?;
    }
    let perks = r.u16()?;
    let mut mods = [0u8; MODS];
    for m in &mut mods {
        *m = r.u8()?;
    }
    Some(Ties {
        born,
        pvp,
        crews_open,
        pvp_off_on,
        honeypot,
        credited_to,
        sent,
        stake,
        arcs,
        flags,
        endings,
        legend,
        stats,
        perks,
        mods,
    })
}

/// Bring a version 1 save to version 2: no fragments, no coded scripts. `None` if the bytes are
/// not a version 1 save.
///
#[must_use]
pub fn migrate_save_v1(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = bytes.to_vec();
    out.push(0);
    out.extend_from_slice(&[0; CODED_MAX * 3]);
    decode_v2(&out).map(|_| out)
}

/// Bring a version 1 world to version 2: no season yet, nothing learned. `None` if the bytes are
/// not a version 1 world.
///
#[must_use]
pub fn migrate_world_v1(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut out = bytes.to_vec();
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(0);
    decode_world_v2(&out).map(|_| out)
}

/// Bring a version 2 save to version 3: the social layer and the story start empty, and the
/// protection week is over (the character is not new). `None` if the bytes are not a version 2
/// save.
#[must_use]
pub fn migrate_save_v2(bytes: &[u8]) -> Option<Vec<u8>> {
    decode_v2(bytes).map(|p| encode(&p))
}

/// Bring a version 2 world to version 3: no price history and an empty social state. `None` if
/// the bytes are not a version 2 world.
#[must_use]
pub fn migrate_world_v2(bytes: &[u8]) -> Option<Vec<u8>> {
    decode_world_v2(bytes).map(|w| encode_world(&w))
}

/// Decode a character, or `None` for bytes that are not one.
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<Player> {
    let mut r = Reader(bytes);
    let mut p = decode_base(&mut r)?;
    p.ties = decode_ties(&mut r)?;
    r.done()?;
    Some(p)
}

/// Decode a version 2 character (no social layer).
#[must_use]
pub fn decode_v2(bytes: &[u8]) -> Option<Player> {
    let mut r = Reader(bytes);
    let mut p = decode_base(&mut r)?;
    r.done()?;
    p.ties = Ties::grandfathered(p.day);
    Some(p)
}

fn decode_base(r: &mut Reader<'_>) -> Option<Player> {
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
        ties: Ties::new(day),
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
    // Version 3: the price history and the social state.
    w.list(&world.hist[world.hist.len().saturating_sub(HIST_MAX)..]);
    encode_social(&mut w, &world.social);
    w.0
}

fn encode_tallies(w: &mut Writer, t: &Tallies) {
    for v in [
        t.deep_breaches,
        t.wardens,
        t.crew_jobs,
        t.hacks_in,
        t.hacks_caught,
        t.bounties_paid,
        t.burns,
        t.jobs,
        t.finale,
    ] {
        w.u32(v);
    }
}

fn decode_tallies(r: &mut Reader<'_>) -> Option<Tallies> {
    Some(Tallies {
        deep_breaches: r.u32()?,
        wardens: r.u32()?,
        crew_jobs: r.u32()?,
        hacks_in: r.u32()?,
        hacks_caught: r.u32()?,
        bounties_paid: r.u32()?,
        burns: r.u32()?,
        jobs: r.u32()?,
        finale: r.u32()?,
    })
}

const HANDLE: usize = crate::social::HANDLE_MAX;
const TEXT: usize = 80;

fn encode_member(w: &mut Writer, m: &Member) {
    w.string(&m.handle, HANDLE);
    w.list(&m.deck);
    w.u8(u8::try_from(m.builds.len().min(6)).unwrap_or(0));
    for (num, b) in m.builds.iter().take(6) {
        w.u8(*num);
        w.u8(b.effect);
        w.u8(b.target);
        w.u8(b.modifier);
    }
    for k in m.skill {
        w.u8(k);
    }
    w.u8(m.shield);
    w.u8(m.iface);
    w.u32(m.cycles);
    w.u32(m.buffer);
}

fn decode_member(r: &mut Reader<'_>) -> Option<Member> {
    let handle = r.string(HANDLE)?;
    let deck = r.list(6)?;
    let n = usize::from(r.u8()?);
    if n > 6 {
        return None;
    }
    let mut builds = Vec::with_capacity(n);
    for _ in 0..n {
        let num = r.u8()?;
        builds.push((
            num,
            Build {
                effect: r.u8()?,
                target: r.u8()?,
                modifier: r.u8()?,
            },
        ));
    }
    let mut skill = [0u8; 5];
    for k in &mut skill {
        *k = r.u8()?;
    }
    Some(Member {
        handle,
        deck,
        builds,
        skill,
        shield: r.u8()?,
        iface: r.u8()?,
        cycles: r.u32()?,
        buffer: r.u32()?,
    })
}

fn encode_contract(w: &mut Writer, c: &Contract) {
    w.u32(c.id);
    w.u8(c.tmpl);
    w.u8(c.corp);
    w.u8(c.fixer);
    w.u8(c.tier);
    w.u32(c.reward);
    w.i64(c.expires);
}

fn decode_contract(r: &mut Reader<'_>) -> Option<Contract> {
    Some(Contract {
        id: r.u32()?,
        tmpl: r.u8()?,
        corp: r.u8()?,
        fixer: r.u8()?,
        tier: r.u8()?,
        reward: r.u32()?,
        expires: r.i64()?,
    })
}

fn encode_social(w: &mut Writer, s: &Social) {
    w.u32(s.next_id);
    w.u8(u8::try_from(s.posted.len().min(POSTED_MAX)).unwrap_or(0));
    for p in s.posted.iter().take(POSTED_MAX) {
        w.string(&p.handle, HANDLE);
        w.u8(p.tier);
        w.u8(p.heat);
        w.u8(p.honeypot);
        w.u8(p.shield);
        w.u8(p.edge);
        w.u16(p.cash);
        w.u32(p.rep);
        w.i64(p.born);
        w.i64(p.seen);
        w.i64(p.peace_until);
        w.u8(u8::from(p.pvp) | (u8::from(p.crews_open) << 1));
    }
    w.u8(u8::try_from(s.orders.len().min(ORDERS_MAX)).unwrap_or(0));
    for o in s.orders.iter().take(ORDERS_MAX) {
        w.u32(o.id);
        w.string(&o.from, HANDLE);
        w.string(&o.to, HANDLE);
        w.i64(o.day);
    }
    w.u8(u8::try_from(s.deliveries.len().min(DELIVERIES_MAX)).unwrap_or(0));
    for d in s.deliveries.iter().take(DELIVERIES_MAX) {
        w.u32(d.id);
        w.string(&d.to, HANDLE);
        w.i64(d.day);
        w.u8(d.effect.code());
        w.u32(u32::from_le_bytes(d.n.to_le_bytes()));
        w.string(&d.who, HANDLE);
        w.u8(d.line);
        w.u32(d.nonce);
    }
    w.u8(u8::try_from(s.bounties.len().min(BOUNTIES_MAX)).unwrap_or(0));
    for b in s.bounties.iter().take(BOUNTIES_MAX) {
        w.u32(b.id);
        w.string(&b.target, HANDLE);
        w.string(&b.poster, HANDLE);
        w.u8(b.fixer);
        w.u32(b.reward);
        w.u32(b.stake);
        w.u8(b.tier);
        w.i64(b.posted);
        w.i64(b.expires);
        w.string(&b.note, TEXT);
    }
    w.u8(u8::try_from(s.deeds.len().min(DEEDS_MAX)).unwrap_or(0));
    for d in s.deeds.iter().take(DEEDS_MAX) {
        w.i64(d.day);
        w.u8(d.line);
        w.string(&d.who, HANDLE);
        w.u32(d.n);
    }
    encode_tallies(w, &s.tallies);
    w.string(s.finale.as_deref().unwrap_or(""), HANDLE);
    w.u8(u8::from(s.finale.is_some()));
    w.u8(u8::try_from(s.hall.len().min(HALL_MAX)).unwrap_or(0));
    for h in s.hall.iter().take(HALL_MAX) {
        w.u32(h.season);
        w.u8(h.ending);
        w.u8(u8::try_from(h.top.len().min(5)).unwrap_or(0));
        for (name, rep) in h.top.iter().take(5) {
            w.string(name, HANDLE);
            w.u32(*rep);
        }
        encode_tallies(w, &h.tallies);
        w.string(&h.finale, HANDLE);
    }
    w.u8(u8::try_from(s.pairs.len().min(PAIRS_MAX)).unwrap_or(0));
    for p in s.pairs.iter().take(PAIRS_MAX) {
        w.string(&p.from, HANDLE);
        w.string(&p.to, HANDLE);
        w.i64(p.day);
    }
    w.u8(u8::try_from(s.crews.len().min(crate::crew::CREWS_MAX)).unwrap_or(0));
    for c in s.crews.iter().take(crate::crew::CREWS_MAX) {
        w.u32(c.id);
        w.string(&c.name, crate::crew::NAME_MAX * 4);
        w.string(&c.leader, HANDLE);
        w.u8(u8::try_from(c.members.len().min(crate::crew::CREW_MAX)).unwrap_or(0));
        for m in c.members.iter().take(crate::crew::CREW_MAX) {
            w.string(m, HANDLE);
        }
        w.u8(u8::try_from(c.invited.len().min(crate::crew::INVITES_MAX)).unwrap_or(0));
        for m in c.invited.iter().take(crate::crew::INVITES_MAX) {
            w.string(m, HANDLE);
        }
        w.i64(c.made);
        w.u16(c.jobs);
    }
    w.u8(u8::try_from(s.ops.len().min(crate::crew::OPS_MAX)).unwrap_or(0));
    for o in s.ops.iter().take(crate::crew::OPS_MAX) {
        w.u32(o.id);
        w.u32(o.crew);
        encode_contract(w, &o.contract);
        w.u32(o.season);
        w.u8(o.state.code());
        w.i64(o.made);
        w.i64(o.last);
        w.i64(o.ended);
        w.list(&o.holes);
        w.u8(o.alert);
        w.u8(u8::from(o.backdoor));
        w.u8(u8::try_from(o.learned.len().min(ROWS_MAX)).unwrap_or(0));
        for (sig, uses) in o.learned.iter().take(ROWS_MAX) {
            w.u16(*sig);
            w.u32(*uses);
        }
        w.u8(u8::try_from(o.members.len().min(crate::crew::CREW_MAX)).unwrap_or(0));
        for m in o.members.iter().take(crate::crew::CREW_MAX) {
            encode_member(w, m);
        }
        let log = &o.log[..o.log.len().min(crate::crew::ACTIONS_MAX + 1)];
        w.u16(u16::try_from(log.len()).unwrap_or(0));
        for a in log {
            w.u16(*a);
        }
        w.u8(u8::try_from(o.settled.len().min(crate::crew::CREW_MAX)).unwrap_or(0));
        for m in o.settled.iter().take(crate::crew::CREW_MAX) {
            w.string(m, HANDLE);
        }
    }
}

fn decode_social(r: &mut Reader<'_>) -> Option<Social> {
    let mut s = Social {
        next_id: r.u32()?,
        ..Social::default()
    };
    let n = usize::from(r.u8()?);
    if n > POSTED_MAX {
        return None;
    }
    for _ in 0..n {
        let handle = r.string(HANDLE)?;
        let (tier, heat, honeypot, shield, edge) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?, r.u8()?);
        let cash = r.u16()?;
        let rep = r.u32()?;
        let (born, seen, peace_until) = (r.i64()?, r.i64()?, r.i64()?);
        let bits = r.u8()?;
        s.posted.push(Posted {
            handle,
            tier,
            heat,
            honeypot,
            shield,
            edge,
            cash,
            rep,
            born,
            seen,
            peace_until,
            pvp: bits & 1 != 0,
            crews_open: bits & 2 != 0,
        });
    }
    let n = usize::from(r.u8()?);
    if n > ORDERS_MAX {
        return None;
    }
    for _ in 0..n {
        s.orders.push(Order {
            id: r.u32()?,
            from: r.string(HANDLE)?,
            to: r.string(HANDLE)?,
            day: r.i64()?,
        });
    }
    let n = usize::from(r.u8()?);
    if n > DELIVERIES_MAX {
        return None;
    }
    for _ in 0..n {
        s.deliveries.push(Delivery {
            id: r.u32()?,
            to: r.string(HANDLE)?,
            day: r.i64()?,
            effect: Effect::from_code(r.u8()?)?,
            n: i32::from_le_bytes(r.u32()?.to_le_bytes()),
            who: r.string(HANDLE)?,
            line: r.u8()?,
            nonce: r.u32()?,
        });
    }
    let n = usize::from(r.u8()?);
    if n > BOUNTIES_MAX {
        return None;
    }
    for _ in 0..n {
        s.bounties.push(Bounty {
            id: r.u32()?,
            target: r.string(HANDLE)?,
            poster: r.string(HANDLE)?,
            fixer: r.u8()?,
            reward: r.u32()?,
            stake: r.u32()?,
            tier: r.u8()?,
            posted: r.i64()?,
            expires: r.i64()?,
            note: r.string(TEXT)?,
        });
    }
    let n = usize::from(r.u8()?);
    if n > DEEDS_MAX {
        return None;
    }
    for _ in 0..n {
        s.deeds.push(Deed {
            day: r.i64()?,
            line: r.u8()?,
            who: r.string(HANDLE)?,
            n: r.u32()?,
        });
    }
    s.tallies = decode_tallies(r)?;
    let finale = r.string(HANDLE)?;
    s.finale = (r.u8()? != 0).then_some(finale);
    let n = usize::from(r.u8()?);
    if n > HALL_MAX {
        return None;
    }
    for _ in 0..n {
        let season = r.u32()?;
        let ending = r.u8()?;
        let k = usize::from(r.u8()?);
        if k > 5 {
            return None;
        }
        let mut top = Vec::with_capacity(k);
        for _ in 0..k {
            top.push((r.string(HANDLE)?, r.u32()?));
        }
        s.hall.push(SeasonRecord {
            season,
            ending,
            top,
            tallies: decode_tallies(r)?,
            finale: r.string(HANDLE)?,
        });
    }
    let n = usize::from(r.u8()?);
    if n > PAIRS_MAX {
        return None;
    }
    for _ in 0..n {
        s.pairs.push(Pair {
            from: r.string(HANDLE)?,
            to: r.string(HANDLE)?,
            day: r.i64()?,
        });
    }
    let n = usize::from(r.u8()?);
    if n > crate::crew::CREWS_MAX {
        return None;
    }
    for _ in 0..n {
        let id = r.u32()?;
        let name = r.string(crate::crew::NAME_MAX * 4)?;
        let leader = r.string(HANDLE)?;
        let k = usize::from(r.u8()?);
        if k > crate::crew::CREW_MAX {
            return None;
        }
        let mut members = Vec::with_capacity(k);
        for _ in 0..k {
            members.push(r.string(HANDLE)?);
        }
        let k = usize::from(r.u8()?);
        if k > crate::crew::INVITES_MAX {
            return None;
        }
        let mut invited = Vec::with_capacity(k);
        for _ in 0..k {
            invited.push(r.string(HANDLE)?);
        }
        s.crews.push(Crew {
            id,
            name,
            leader,
            members,
            invited,
            made: r.i64()?,
            jobs: r.u16()?,
        });
    }
    let n = usize::from(r.u8()?);
    if n > crate::crew::OPS_MAX {
        return None;
    }
    for _ in 0..n {
        let id = r.u32()?;
        let crew = r.u32()?;
        let contract = decode_contract(r)?;
        let season = r.u32()?;
        let state = OpState::from_code(r.u8()?)?;
        let (made, last, ended) = (r.i64()?, r.i64()?, r.i64()?);
        let holes = r.list(HOLES_MAX)?;
        let alert = r.u8()?;
        let backdoor = r.u8()? != 0;
        let k = usize::from(r.u8()?);
        if k > ROWS_MAX {
            return None;
        }
        let mut learned = Vec::with_capacity(k);
        for _ in 0..k {
            learned.push((r.u16()?, r.u32()?));
        }
        let k = usize::from(r.u8()?);
        if k > crate::crew::CREW_MAX {
            return None;
        }
        let mut members = Vec::with_capacity(k);
        for _ in 0..k {
            members.push(decode_member(r)?);
        }
        let k = usize::from(r.u16()?);
        if k > crate::crew::ACTIONS_MAX + 1 {
            return None;
        }
        let mut log = Vec::with_capacity(k);
        for _ in 0..k {
            log.push(r.u16()?);
        }
        let k = usize::from(r.u8()?);
        if k > crate::crew::CREW_MAX {
            return None;
        }
        let mut settled = Vec::with_capacity(k);
        for _ in 0..k {
            settled.push(r.string(HANDLE)?);
        }
        s.ops.push(Op {
            id,
            crew,
            contract,
            season,
            state,
            made,
            last,
            ended,
            holes,
            alert,
            backdoor,
            learned,
            members,
            log,
            settled,
        });
    }
    Some(s)
}

/// Decode the shared world, or `None` for bytes that are not one.
#[must_use]
pub fn decode_world(bytes: &[u8]) -> Option<World> {
    let mut r = Reader(bytes);
    let mut world = decode_world_base(&mut r)?;
    world.hist = r.list(HIST_MAX)?;
    world.social = decode_social(&mut r)?;
    r.done()?;
    Some(world)
}

/// Decode a version 2 world (no price history, no social state).
#[must_use]
pub fn decode_world_v2(bytes: &[u8]) -> Option<World> {
    let mut r = Reader(bytes);
    let world = decode_world_base(&mut r)?;
    r.done()?;
    Some(world)
}

fn decode_world_base(r: &mut Reader<'_>) -> Option<World> {
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
    Some(World {
        day,
        market_pct,
        nets,
        board,
        breaches,
        season,
        learned,
        hist: Vec::new(),
        social: Social::default(),
    })
}
