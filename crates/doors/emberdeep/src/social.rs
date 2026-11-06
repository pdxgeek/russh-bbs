//! The social layer (M4-E4): companies (parties of up to three), the notes that carry a rescue's
//! thanks and a duel's bruise from one hero's save to another's, the duel ring and its ladder,
//! epitaphs, and the Furnace's verdict. The world holds all of it ([`Commons`]); a hero's save
//! holds only what that hero chose and did (`hero::Social`).
//!
//! **Nothing here moves one player's coin or hit points from another player's session.** A
//! hero's save is written only by that hero's own session, so anything owed *to* another hero is a
//! [`Note`] in the world which its addressee collects (once, guarded by the ids in their save)
//! next time they open the door. Every change is idempotent, so the session's own copy and the
//! board's stored world agree whatever order two heroes' edits merge in.
//!
//! **Text.** The only text one player shows another is an epitaph (through `Ctx::player_text`:
//! sanitized, one line, rate limited) and names (handles the board already sanitized). Everything
//! else is built here from fixed templates; a challenge or an invitation carries no words.

use russh_bbs_door::DoorRng;

use crate::codec::{Reader, Writer};

/// The most characters of an epitaph.
pub(crate) const EPITAPH_MAX: usize = 80;
/// A company's size.
pub(crate) const PARTY_MAX: usize = 3;
/// How many heroes the roster remembers.
pub(crate) const ROSTER_MAX: usize = 48;
const PARTIES_MAX: usize = 24;
const INVITES_MAX: usize = 24;
const NOTES_MAX: usize = 60;
const LADDER_MAX: usize = 20;
const BOUTS_MAX: usize = 60;
const CLEARED_MAX: usize = 24;
const EPITAPHS_MAX: usize = 10;
const CHRONICLE_MAX: usize = 8;
/// Invitations a hero may have outstanding.
pub(crate) const INVITES_PER_HERO: usize = 2;
/// Days an invitation stays good.
pub(crate) const INVITE_DAYS: i64 = 3;
/// Days a note waits to be collected.
const NOTE_DAYS: i64 = 14;
/// Days a hero stays on the roster without opening the door.
const ROSTER_DAYS: i64 = 21;
/// Duels a hero may call for in a day.
pub(crate) const BOUTS_PER_DAY: u8 = 2;
/// Duels one hero may be called out to in a day.
pub(crate) const CALLS_PER_TARGET: usize = 3;
/// The widest gap in hero level a duel allows.
pub(crate) const DUEL_LEVEL_GAP: u8 = 4;
/// The stakes the ring takes.
pub(crate) const WAGERS: [u32; 4] = [0, 5, 20, 50];
/// Rounds a duel lasts before the judges call it.
const DUEL_ROUNDS: u32 = 12;

/// What a duellist brings to the ring: a snapshot of the hero when they last opened the door.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Ghost {
    pub(crate) hp: i32,
    pub(crate) armor: i32,
    pub(crate) damage: i32,
    pub(crate) crit: i32,
    pub(crate) evade: i32,
}

/// A hero the town knows of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub(crate) name: String,
    pub(crate) level: u8,
    pub(crate) calling: String,
    /// The last day the hero opened the door.
    pub(crate) day: i64,
    /// Open to invitations.
    pub(crate) open: bool,
    /// In the ring.
    pub(crate) ring: bool,
    pub(crate) ghost: Ghost,
}

/// A company of heroes who delve the same dungeon and share its doors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Party {
    pub(crate) leader: String,
    pub(crate) members: Vec<String>,
    pub(crate) formed: i64,
    /// The day `cleared` belongs to.
    pub(crate) day: i64,
    /// Rooms where a member has dealt with the pack today (depth, room): for every member.
    pub(crate) cleared: Vec<(u8, u8)>,
    /// Companions carried home by their own company.
    pub(crate) rescues: u16,
    /// The deepest level any member has reached.
    pub(crate) deepest: u8,
}

/// An invitation to a company.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invite {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) day: i64,
}

/// What a note carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoteKind {
    /// A rescued hero's thanks, in coin.
    Thanks,
    /// A bruise from the ring.
    Bruise,
}

/// Something owed to a hero, collected by that hero's own session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Note {
    pub(crate) id: String,
    pub(crate) to: String,
    pub(crate) from: String,
    pub(crate) kind: NoteKind,
    pub(crate) amount: u32,
    pub(crate) day: i64,
}

/// A duellist's standing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rank {
    pub(crate) name: String,
    pub(crate) wins: u16,
    pub(crate) losses: u16,
    pub(crate) points: u32,
}

/// A duel, as it enters the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Bout {
    pub(crate) id: String,
    pub(crate) challenger: String,
    pub(crate) target: String,
    pub(crate) challenger_won: bool,
    pub(crate) wager: u32,
    pub(crate) day: i64,
}

/// A line for the hall about a hero who was not found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Epitaph {
    pub(crate) name: String,
    pub(crate) text: String,
    pub(crate) depth: u8,
    pub(crate) day: i64,
}

/// Everything the heroes share beyond the dungeon and the works.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Commons {
    pub(crate) roster: Vec<Member>,
    pub(crate) parties: Vec<Party>,
    pub(crate) invites: Vec<Invite>,
    pub(crate) notes: Vec<Note>,
    pub(crate) ladder: Vec<Rank>,
    /// Recent duels (ids kept so one is never counted twice).
    pub(crate) bouts: Vec<Bout>,
    /// Coin staked and lost in the ring, until the night's tick gives it to the works.
    pub(crate) ring_fund: u32,
    /// Duels fought and rescues made, this season.
    pub(crate) duels: u16,
    pub(crate) rescues: u16,
    pub(crate) epitaphs: Vec<Epitaph>,
    /// How each fall of the Cold Furnace ended: epoch, ending code.
    pub(crate) chronicle: Vec<(u16, u8)>,
    /// The slayer's verdict on the Furnace, until the tick collapses the dungeon.
    pub(crate) verdict: Option<(String, String)>,
}

/// A change to the commons, as a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SocialChange {
    Roster(Member),
    Invite(Invite),
    Join {
        leader: String,
        name: String,
    },
    Leave(String),
    PartyClear {
        who: String,
        day: i64,
        depth: u8,
        room: u8,
    },
    PartyDepth {
        who: String,
        depth: u8,
    },
    Note(Note),
    Collect {
        id: String,
        to: String,
    },
    Duel(Bout),
    Epitaph(Epitaph),
    Verdict {
        kind: String,
        by: String,
    },
}

impl Commons {
    pub(crate) fn party_of(&self, name: &str) -> Option<&Party> {
        self.parties
            .iter()
            .find(|p| p.members.iter().any(|m| m == name))
    }

    /// Whether two heroes are in the same company.
    pub(crate) fn companions(&self, a: &str, b: &str) -> bool {
        a != b
            && self
                .party_of(a)
                .is_some_and(|p| p.members.iter().any(|m| m == b))
    }

    pub(crate) fn member(&self, name: &str) -> Option<&Member> {
        self.roster.iter().find(|m| m.name == name)
    }

    pub(crate) fn rank_of(&self, name: &str) -> Option<&Rank> {
        self.ladder.iter().find(|r| r.name == name)
    }

    /// Invitations waiting for `name`.
    pub(crate) fn invites_for<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Invite> {
        self.invites.iter().filter(move |i| i.to == name)
    }

    pub(crate) fn invites_from(&self, name: &str) -> usize {
        self.invites.iter().filter(|i| i.from == name).count()
    }

    /// Rooms a member of `name`'s company has dealt with on `day`.
    pub(crate) fn party_cleared(&self, name: &str, day: i64, depth: u8, room: u8) -> bool {
        self.party_of(name)
            .is_some_and(|p| p.day == day && p.cleared.contains(&(depth, room)))
    }

    /// Duels `name` has been called out to on `day`.
    pub(crate) fn calls_on(&self, name: &str, day: i64) -> usize {
        self.bouts
            .iter()
            .filter(|b| b.target == name && b.day == day)
            .count()
    }

    pub(crate) fn thanks_note(from: &str, to: &str, day: i64, amount: u32) -> Note {
        Note {
            id: format!("t:{from}:{day}"),
            to: to.to_owned(),
            from: from.to_owned(),
            kind: NoteKind::Thanks,
            amount,
            day,
        }
    }

    /// Apply a change. Idempotent. Returns a line for the board's news, if it is news.
    #[allow(clippy::too_many_lines, reason = "one arm per kind of change")]
    pub(crate) fn apply(&mut self, change: &SocialChange) -> Option<String> {
        match change {
            SocialChange::Roster(m) => {
                match self.roster.iter().position(|x| x.name == m.name) {
                    Some(at) => self.roster[at] = m.clone(),
                    None if self.roster.len() < ROSTER_MAX => self.roster.push(m.clone()),
                    None => {
                        // A full roster forgets the hero who has been away longest.
                        if let Some(old) = self
                            .roster
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, x)| x.day)
                            .map(|(i, _)| i)
                            && self.roster[old].day < m.day
                        {
                            self.roster[old] = m.clone();
                        }
                    }
                }
                if !m.ring {
                    self.ladder
                        .retain(|r| r.name != m.name || r.wins + r.losses > 0);
                }
                None
            }
            SocialChange::Invite(i) => {
                let fresh = i.from != i.to
                    && self.party_of(&i.to).is_none()
                    && !self
                        .invites
                        .iter()
                        .any(|x| x.from == i.from && x.to == i.to)
                    && self.invites_from(&i.from) < INVITES_PER_HERO
                    && self.invites.len() < INVITES_MAX;
                if fresh {
                    self.invites.push(i.clone());
                }
                None
            }
            SocialChange::Join { leader, name } => self.join(leader, name),
            SocialChange::Leave(name) => self.leave(name),
            SocialChange::PartyClear {
                who,
                day,
                depth,
                room,
            } => {
                if let Some(p) = self
                    .parties
                    .iter_mut()
                    .find(|p| p.members.iter().any(|m| m == who))
                {
                    if p.day != *day {
                        p.day = *day;
                        p.cleared.clear();
                    }
                    if !p.cleared.contains(&(*depth, *room)) && p.cleared.len() < CLEARED_MAX {
                        p.cleared.push((*depth, *room));
                    }
                }
                None
            }
            SocialChange::PartyDepth { who, depth } => {
                if let Some(p) = self
                    .parties
                    .iter_mut()
                    .find(|p| p.members.iter().any(|m| m == who))
                {
                    p.deepest = p.deepest.max(*depth);
                }
                None
            }
            SocialChange::Note(n) => {
                if !self.notes.iter().any(|x| x.id == n.id) && self.notes.len() < NOTES_MAX {
                    self.notes.push(n.clone());
                }
                None
            }
            SocialChange::Collect { id, to } => {
                self.notes.retain(|n| !(n.id == *id && n.to == *to));
                None
            }
            SocialChange::Duel(b) => self.duel(b),
            SocialChange::Epitaph(e) => {
                if !self
                    .epitaphs
                    .iter()
                    .any(|x| x.name == e.name && x.day == e.day)
                {
                    self.epitaphs.push(e.clone());
                    while self.epitaphs.len() > EPITAPHS_MAX {
                        self.epitaphs.remove(0);
                    }
                }
                None
            }
            SocialChange::Verdict { kind, by } => {
                if self.verdict.is_none() {
                    self.verdict = Some((kind.clone(), by.clone()));
                }
                None
            }
        }
    }

    fn join(&mut self, leader: &str, name: &str) -> Option<String> {
        if leader == name || self.party_of(name).is_some() {
            return None;
        }
        // Only an invitation can bring a hero into a company.
        if !self
            .invites
            .iter()
            .any(|i| i.from == leader && i.to == name)
        {
            return None;
        }
        let day = self.member(name).map_or(0, |m| m.day);
        let at = self
            .parties
            .iter()
            .position(|p| p.members.iter().any(|m| m == leader));
        let ok = match at {
            Some(at) if self.parties[at].members.len() < PARTY_MAX => {
                self.parties[at].members.push(name.to_owned());
                true
            }
            None if self.parties.len() < PARTIES_MAX => {
                self.parties.push(Party {
                    leader: leader.to_owned(),
                    members: vec![leader.to_owned(), name.to_owned()],
                    formed: day,
                    day: 0,
                    cleared: Vec::new(),
                    rescues: 0,
                    deepest: 0,
                });
                true
            }
            Some(_) | None => false,
        };
        self.invites.retain(|i| i.to != name);
        ok.then(|| format!("{name} joined {leader}'s company."))
    }

    fn leave(&mut self, name: &str) -> Option<String> {
        let at = self
            .parties
            .iter()
            .position(|p| p.members.iter().any(|m| m == name))?;
        let party = &mut self.parties[at];
        party.members.retain(|m| m != name);
        let line = if party.members.len() < 2 {
            self.parties.remove(at);
            format!("{name} left; the company is no more.")
        } else {
            if party.leader == name {
                party.leader = party.members[0].clone();
            }
            format!("{name} left {}'s company.", party.leader)
        };
        Some(line)
    }

    fn duel(&mut self, b: &Bout) -> Option<String> {
        if self.bouts.iter().any(|x| x.id == b.id) {
            return None;
        }
        if self.calls_on(&b.target, b.day) >= CALLS_PER_TARGET * 2 {
            return None;
        }
        self.bouts.push(b.clone());
        while self.bouts.len() > BOUTS_MAX {
            self.bouts.remove(0);
        }
        self.duels = self.duels.saturating_add(1);
        let (winner, loser) = if b.challenger_won {
            (&b.challenger, &b.target)
        } else {
            (&b.target, &b.challenger)
        };
        self.bump(winner, true);
        self.bump(loser, false);
        if !b.challenger_won {
            self.ring_fund = self.ring_fund.saturating_add(b.wager);
        }
        // The loser who is not the one making the call collects a bruise from their own session.
        if loser == &b.target {
            self.apply(&SocialChange::Note(Note {
                id: format!("b:{}", b.id),
                to: b.target.clone(),
                from: b.challenger.clone(),
                kind: NoteKind::Bruise,
                amount: 1,
                day: b.day,
            }));
        }
        Some(format!("{winner} bested {loser} in the ring."))
    }

    fn bump(&mut self, name: &str, won: bool) {
        let at = if let Some(i) = self.ladder.iter().position(|r| r.name == name) {
            i
        } else {
            {
                if self.ladder.len() >= LADDER_MAX {
                    // The bottom of the ladder is dropped to make room.
                    if let Some(low) = self
                        .ladder
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, r)| r.points)
                        .map(|(i, _)| i)
                    {
                        self.ladder.remove(low);
                    }
                }
                self.ladder.push(Rank {
                    name: name.to_owned(),
                    wins: 0,
                    losses: 0,
                    points: 5,
                });
                self.ladder.len() - 1
            }
        };
        let r = &mut self.ladder[at];
        if won {
            r.wins = r.wins.saturating_add(1);
            r.points = r.points.saturating_add(3);
        } else {
            r.losses = r.losses.saturating_add(1);
            r.points = r.points.saturating_sub(2);
        }
        self.ladder
            .sort_by(|a, b| b.points.cmp(&a.points).then_with(|| a.name.cmp(&b.name)));
    }

    /// A rescue between companions counts for the company.
    pub(crate) fn on_rescue(&mut self, rescued: &str, by: &str) {
        self.rescues = self.rescues.saturating_add(1);
        if self.companions(rescued, by)
            && let Some(p) = self
                .parties
                .iter_mut()
                .find(|p| p.members.iter().any(|m| m == by))
        {
            p.rescues = p.rescues.saturating_add(1);
        }
    }

    /// One day: old invitations, notes and strangers are forgotten.
    pub(crate) fn tick(&mut self, day: i64) -> Vec<String> {
        let mut news = Vec::new();
        self.invites.retain(|i| day - i.day <= INVITE_DAYS);
        self.notes.retain(|n| day - n.day <= NOTE_DAYS);
        self.bouts.retain(|b| day - b.day <= 3);
        let gone: Vec<String> = self
            .roster
            .iter()
            .filter(|m| day - m.day > ROSTER_DAYS)
            .map(|m| m.name.clone())
            .collect();
        self.roster.retain(|m| day - m.day <= ROSTER_DAYS);
        for name in gone {
            news.extend(self.leave(&name));
        }
        news
    }

    // ---- bytes ----

    pub(crate) fn encode(&self, w: &mut Writer) {
        w.count(self.roster.len());
        for m in &self.roster {
            w.str(&m.name);
            w.u8(m.level);
            w.str(&m.calling);
            w.i64(m.day);
            w.bool(m.open);
            w.bool(m.ring);
            for v in [
                m.ghost.hp,
                m.ghost.armor,
                m.ghost.damage,
                m.ghost.crit,
                m.ghost.evade,
            ] {
                w.i32(v);
            }
        }
        w.count(self.parties.len());
        for p in &self.parties {
            w.str(&p.leader);
            w.count(p.members.len());
            for m in &p.members {
                w.str(m);
            }
            w.i64(p.formed);
            w.i64(p.day);
            w.count(p.cleared.len());
            for (d, r) in &p.cleared {
                w.u8(*d);
                w.u8(*r);
            }
            w.u16(p.rescues);
            w.u8(p.deepest);
        }
        w.count(self.invites.len());
        for i in &self.invites {
            w.str(&i.from);
            w.str(&i.to);
            w.i64(i.day);
        }
        w.count(self.notes.len());
        for n in &self.notes {
            w.str(&n.id);
            w.str(&n.to);
            w.str(&n.from);
            w.u8(match n.kind {
                NoteKind::Thanks => 0,
                NoteKind::Bruise => 1,
            });
            w.u32(n.amount);
            w.i64(n.day);
        }
        w.count(self.ladder.len());
        for r in &self.ladder {
            w.str(&r.name);
            w.u16(r.wins);
            w.u16(r.losses);
            w.u32(r.points);
        }
        w.count(self.bouts.len());
        for b in &self.bouts {
            w.str(&b.id);
            w.str(&b.challenger);
            w.str(&b.target);
            w.bool(b.challenger_won);
            w.u32(b.wager);
            w.i64(b.day);
        }
        w.u32(self.ring_fund);
        w.u16(self.duels);
        w.u16(self.rescues);
        w.count(self.epitaphs.len());
        for e in &self.epitaphs {
            w.str(&e.name);
            w.str(&e.text);
            w.u8(e.depth);
            w.i64(e.day);
        }
        w.count(self.chronicle.len());
        for (epoch, code) in &self.chronicle {
            w.u16(*epoch);
            w.u8(*code);
        }
        w.bool(self.verdict.is_some());
        if let Some((kind, by)) = &self.verdict {
            w.str(kind);
            w.str(by);
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one block per list, read in the order written"
    )]
    pub(crate) fn decode(r: &mut Reader<'_>) -> Option<Self> {
        let mut c = Self::default();
        let n = r.count()?;
        if n > ROSTER_MAX {
            return None;
        }
        for _ in 0..n {
            c.roster.push(Member {
                name: r.str()?,
                level: r.u8()?,
                calling: r.str()?,
                day: r.i64()?,
                open: r.bool()?,
                ring: r.bool()?,
                ghost: Ghost {
                    hp: r.i32()?,
                    armor: r.i32()?,
                    damage: r.i32()?,
                    crit: r.i32()?,
                    evade: r.i32()?,
                },
            });
        }
        let n = r.count()?;
        if n > PARTIES_MAX {
            return None;
        }
        for _ in 0..n {
            let leader = r.str()?;
            let k = r.count()?;
            if k > PARTY_MAX {
                return None;
            }
            let members = (0..k).map(|_| r.str()).collect::<Option<Vec<_>>>()?;
            let formed = r.i64()?;
            let day = r.i64()?;
            let k = r.count()?;
            if k > CLEARED_MAX {
                return None;
            }
            let cleared = (0..k)
                .map(|_| Some((r.u8()?, r.u8()?)))
                .collect::<Option<Vec<_>>>()?;
            c.parties.push(Party {
                leader,
                members,
                formed,
                day,
                cleared,
                rescues: r.u16()?,
                deepest: r.u8()?,
            });
        }
        let n = r.count()?;
        if n > INVITES_MAX {
            return None;
        }
        for _ in 0..n {
            c.invites.push(Invite {
                from: r.str()?,
                to: r.str()?,
                day: r.i64()?,
            });
        }
        let n = r.count()?;
        if n > NOTES_MAX {
            return None;
        }
        for _ in 0..n {
            let id = r.str()?;
            let to = r.str()?;
            let from = r.str()?;
            let kind = match r.u8()? {
                0 => NoteKind::Thanks,
                1 => NoteKind::Bruise,
                _ => return None,
            };
            c.notes.push(Note {
                id,
                to,
                from,
                kind,
                amount: r.u32()?,
                day: r.i64()?,
            });
        }
        let n = r.count()?;
        if n > LADDER_MAX {
            return None;
        }
        for _ in 0..n {
            c.ladder.push(Rank {
                name: r.str()?,
                wins: r.u16()?,
                losses: r.u16()?,
                points: r.u32()?,
            });
        }
        let n = r.count()?;
        if n > BOUTS_MAX {
            return None;
        }
        for _ in 0..n {
            c.bouts.push(Bout {
                id: r.str()?,
                challenger: r.str()?,
                target: r.str()?,
                challenger_won: r.bool()?,
                wager: r.u32()?,
                day: r.i64()?,
            });
        }
        c.ring_fund = r.u32()?;
        c.duels = r.u16()?;
        c.rescues = r.u16()?;
        let n = r.count()?;
        if n > EPITAPHS_MAX {
            return None;
        }
        for _ in 0..n {
            c.epitaphs.push(Epitaph {
                name: r.str()?,
                text: r.str()?,
                depth: r.u8()?,
                day: r.i64()?,
            });
        }
        let n = r.count()?;
        if n > CHRONICLE_MAX {
            return None;
        }
        for _ in 0..n {
            c.chronicle.push((r.u16()?, r.u8()?));
        }
        if r.bool()? {
            c.verdict = Some((r.str()?, r.str()?));
        }
        Some(c)
    }

    /// The bytes of an empty commons (what a format-2 world gains).
    pub(crate) fn empty_bytes() -> Vec<u8> {
        let mut w = Writer::default();
        Self::default().encode(&mut w);
        w.0
    }
}

/// How a duel went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Duel {
    pub(crate) challenger_won: bool,
    pub(crate) rounds: u32,
    /// Hit points left, challenger then target.
    pub(crate) left: (i32, i32),
}

/// Fight a duel between two snapshots. Pure given the generator: the challenger's own generator
/// is used, and it is saved, so a replayed run reproduces the same duel.
pub(crate) fn resolve(rng: &mut DoorRng, a: &Ghost, b: &Ghost) -> Duel {
    let mut hp = (a.hp.max(1), b.hp.max(1));
    let start = hp;
    let mut rounds = 0;
    while rounds < DUEL_ROUNDS && hp.0 > 0 && hp.1 > 0 {
        rounds += 1;
        hp.1 -= swing(rng, a, b);
        if hp.1 > 0 {
            hp.0 -= swing(rng, b, a);
        }
    }
    let challenger_won = if hp.0 <= 0 {
        false
    } else if hp.1 <= 0 {
        true
    } else {
        // The judges give it to whoever has more of their strength left; a tie goes to the
        // one who was called out.
        i64::from(hp.0) * i64::from(start.1) > i64::from(hp.1) * i64::from(start.0)
    };
    Duel {
        challenger_won,
        rounds,
        left: (hp.0.max(0), hp.1.max(0)),
    }
}

fn swing(rng: &mut DoorRng, from: &Ghost, to: &Ghost) -> i32 {
    let dodge = to.evade.clamp(0, 60);
    if i32::try_from(rng.below(100)).unwrap_or(99) < dodge {
        return 0;
    }
    let spread = u32::try_from((from.damage / 3).max(1)).unwrap_or(1);
    let swing = from.damage + i32::try_from(rng.below(spread * 2 + 1)).unwrap_or(0)
        - i32::try_from(spread).unwrap_or(0);
    let crit = i32::try_from(rng.below(100)).unwrap_or(99) < from.crit.clamp(0, 50);
    let raw = if crit { swing * 3 / 2 } else { swing };
    (raw - to.armor).max(1)
}

#[cfg(test)]
mod unit {
    use super::*;

    fn ghost(hp: i32, damage: i32) -> Ghost {
        Ghost {
            hp,
            armor: 1,
            damage,
            crit: 5,
            evade: 10,
        }
    }

    #[test]
    fn a_stronger_ghost_wins_most_duels_and_a_duel_always_ends() {
        let mut rng = DoorRng::from_seed(5);
        let mut wins = 0;
        for _ in 0..200 {
            let d = resolve(&mut rng, &ghost(60, 14), &ghost(30, 6));
            assert!(d.rounds <= DUEL_ROUNDS);
            if d.challenger_won {
                wins += 1;
            }
        }
        assert!(wins > 170, "{wins}");
    }
}
