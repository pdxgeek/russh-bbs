//! The hall's book: what each season came to, written when the next season's world is made,
//! and the line a clan carries in its own hall about the season it played.

use super::legacy::{Ending, Tally};
use super::{Wire, caps, legacy};
use crate::clan::Clan;
use crate::codec::{Reader, Writer};
use crate::world::World;

/// What a record is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Record {
    /// Most raids won.
    Raider,
    /// The biggest haul from raids.
    Haul,
    /// Most tallies traded on the exchange.
    Trader,
    /// The longest pact standing at the end.
    Pact,
    /// Most pacts broken.
    Oathbreaker,
}

impl Record {
    const ALL: [Self; 5] = [
        Self::Raider,
        Self::Haul,
        Self::Trader,
        Self::Pact,
        Self::Oathbreaker,
    ];

    /// The title on the hall's wall.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Raider => "Most raids won",
            Self::Haul => "Biggest raid haul",
            Self::Trader => "Busiest on the exchange",
            Self::Pact => "Longest pact kept (days)",
            Self::Oathbreaker => "Most oaths broken",
        }
    }

    const fn code(self) -> u8 {
        self as u8
    }

    fn from_code(c: u8) -> Option<Self> {
        Self::ALL.get(usize::from(c)).copied()
    }
}

/// One holder of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    /// What record.
    pub record: Record,
    /// Whose.
    pub clan: String,
    /// The figure.
    pub value: u32,
}

/// A season in the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonRecord {
    /// The door's season number.
    pub season: u32,
    /// Clans that played.
    pub clans: u8,
    /// The top of the league: name and renown.
    pub top: Vec<(String, u32)>,
    /// The records.
    pub records: Vec<Holder>,
    /// How the season is remembered.
    pub ending: String,
    /// Which ending it was ([`Ending::code`]); kept in the world's version 4 tail.
    pub code: u8,
}

impl Wire for SeasonRecord {
    fn write(&self, w: &mut Writer) {
        w.u32(self.season);
        w.u8(self.clans);
        w.count(self.top.len());
        for (n, s) in &self.top {
            w.str(n);
            w.u32(*s);
        }
        w.count(self.records.len());
        for h in &self.records {
            w.u8(h.record.code());
            w.str(&h.clan);
            w.u32(h.value);
        }
        w.str(&self.ending);
    }

    fn read(r: &mut Reader<'_>) -> Option<Self> {
        let season = r.u32()?;
        let clans = r.u8()?;
        let top = (0..r.u8()?)
            .map(|_| Some((r.str()?, r.u32()?)))
            .collect::<Option<Vec<_>>>()?;
        let records = (0..r.u8()?)
            .map(|_| {
                Some(Holder {
                    record: Record::from_code(r.u8()?)?,
                    clan: r.str()?,
                    value: r.u32()?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            season,
            clans,
            top,
            records,
            ending: r.str()?,
            code: 0,
        })
    }
}

/// The best (largest, ties by name) of a figure over the clans, if above zero.
fn best(w: &World, f: impl Fn(&crate::world::ClanEntry) -> u32) -> Option<(String, u32)> {
    w.clans
        .iter()
        .map(|c| (f(c), &c.name))
        .filter(|(v, _)| *v > 0)
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(a.1)))
        .map(|(v, n)| (n.clone(), v))
}

impl SeasonRecord {
    /// Write up a world that is ending. `day` is the day of the writing.
    #[must_use]
    pub fn of(w: &World, day: i64) -> Self {
        let top: Vec<(String, u32)> = w
            .standings()
            .into_iter()
            .take(5)
            .map(|c| (c.name.clone(), c.score))
            .collect();
        let mut records = Vec::new();
        let mut put = |record, found: Option<(String, u32)>| {
            if let Some((clan, value)) = found {
                records.push(Holder {
                    record,
                    clan,
                    value,
                });
            }
        };
        put(Record::Raider, best(w, |c| u32::from(c.record.raids_won)));
        put(Record::Haul, best(w, |c| c.record.haul));
        put(Record::Trader, best(w, |c| c.record.traded));
        let longest = w
            .social
            .pacts
            .iter()
            .filter(|p| p.state == super::pact::State::Active)
            .max_by_key(|p| (day - p.since, std::cmp::Reverse(p.id)))
            .map(|p| {
                let name = w
                    .clans
                    .get(usize::from(p.a))
                    .map_or_else(String::new, |c| c.name.clone());
                let other = w
                    .clans
                    .get(usize::from(p.b))
                    .map_or_else(String::new, |c| c.name.clone());
                (
                    format!("{name} and {other}"),
                    u32::try_from((day - p.since).max(0)).unwrap_or(0),
                )
            });
        put(Record::Pact, longest);
        put(
            Record::Oathbreaker,
            best(w, |c| u32::from(c.record.betrayals)),
        );
        let winner = top.first().map_or("nobody", |t| t.0.as_str());
        let which = which_ending(w);
        Self {
            season: w.season,
            clans: u8::try_from(w.clans.len()).unwrap_or(u8::MAX),
            ending: ending(w, winner),
            code: which.code(),
            top,
            records,
        }
    }
}

/// Which ending a world's season comes to.
#[must_use]
pub fn which_ending(w: &World) -> Ending {
    if w.clans.is_empty() {
        Ending::Quiet
    } else {
        Ending::of(&Tally::of(w))
    }
}

/// How the season is remembered, by what it was mostly about.
fn ending(w: &World, winner: &str) -> String {
    if w.clans.is_empty() {
        return "The marsh stood empty. The tide came and went and nobody was there to see it."
            .to_owned();
    }
    format!("{} {winner} held the highest ground.", which_ending(w).line())
}

impl World {
    /// The world a new season starts with: empty, but carrying the book of the seasons before,
    /// with the one just ended written into it.
    #[must_use]
    pub fn successor(old: Option<&Self>, season: u32, seed: u64, day: i64) -> Self {
        let mut w = Self::new(season, seed, day);
        if let Some(old) = old.filter(|o| o.season < season) {
            w.social.book.clone_from(&old.social.book);
            if !w.social.book.iter().any(|b| b.season == old.season) {
                w.social.book.push(SeasonRecord::of(old, day));
            }
            // What the season that ended leaves the one that begins.
            w.legacy = which_ending(old);
            legacy::adjust_prices(w.legacy, &mut w.prices);
            let extra = w.social.book.len().saturating_sub(caps::BOOK_SEASONS);
            w.social.book.drain(..extra);
        }
        w
    }
}

/// The line a clan writes in its own hall about the season it just finished: where it
/// stood, and what it did.
#[must_use]
pub fn season_line(clan: &Clan, old: Option<&World>) -> String {
    let standing = old.and_then(|w| {
        let table = w.standings();
        let rank = table.iter().position(|c| c.name == clan.name)? + 1;
        Some((rank, table.len(), table[rank - 1].score))
    });
    let deeds = format!(
        "{} raid{} won, {} beaten off",
        clan.social.raids_won,
        if clan.social.raids_won == 1 { "" } else { "s" },
        clan.social.repelled
    );
    match standing {
        Some((rank, of, score)) => format!(
            "Season {} ended: {} of {} with renown {score}; {deeds}.",
            clan.season,
            ordinal(rank),
            of
        ),
        None => format!("Season {} ended; {deeds}.", clan.season),
    }
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::assert_is_empty)]
    use super::*;

    fn world() -> World {
        let mut w = World::new(2, 5, 100);
        for (n, score) in [("Ash", 40), ("Brindle", 90), ("Cove", 10)] {
            w.register(n, 0);
            w.set_score(n, score);
        }
        w.clans[0].record.raids_won = 4;
        w.clans[0].record.haul = 120;
        w.clans[1].record.traded = 300;
        w
    }

    #[test]
    fn a_season_is_written_with_its_top_its_records_and_an_ending() {
        let w = world();
        let r = SeasonRecord::of(&w, 150);
        assert_eq!(r.season, 2);
        assert_eq!(r.top[0], ("Brindle".to_owned(), 90));
        let raider = r
            .records
            .iter()
            .find(|h| h.record == Record::Raider)
            .unwrap();
        assert_eq!((raider.clan.as_str(), raider.value), ("Ash", 4));
        assert!(
            r.records
                .iter()
                .any(|h| h.record == Record::Trader && h.clan == "Brindle")
        );
        assert!(!r.records.iter().any(|h| h.record == Record::Oathbreaker));
        assert!(r.ending.contains("Brindle"));
        let mut bytes = Writer::new();
        r.write(&mut bytes);
        let bytes = bytes.finish();
        assert_eq!(SeasonRecord::read(&mut Reader::new(&bytes)), Some(r));
    }

    #[test]
    fn the_ending_follows_what_the_season_was_about() {
        let mut w = world();
        w.clans[0].record.raids_won = 9;
        assert!(ending(&w, "Ash").contains("raids"));
        w.clans[0].record.betrayals = 3;
        assert!(ending(&w, "Ash").contains("oaths"));
        assert!(ending(&World::new(1, 1, 0), "x").contains("empty"));
    }

    #[test]
    fn the_next_world_carries_the_book_once_and_keeps_eight_seasons() {
        let old = world();
        let next = World::successor(Some(&old), 3, 9, 200);
        assert_eq!(next.social.book.len(), 1);
        assert_eq!(next.social.book[0].season, 2);
        assert!(next.clans.is_empty() && next.social.mail.is_empty());
        // A world of the same season is not written up again, and fresh boards have no book.
        assert!(
            World::successor(Some(&old), 2, 9, 200)
                .social
                .book
                .is_empty()
        );
        assert!(World::successor(None, 1, 9, 200).social.book.is_empty());
        let mut w = next;
        for s in 4..20 {
            w = World::successor(Some(&w), s, 9, 200 + i64::from(s));
        }
        assert_eq!(w.social.book.len(), caps::BOOK_SEASONS);
        assert_eq!(w.social.book.last().map(|b| b.season), Some(18));
    }

    #[test]
    fn a_clan_writes_where_it_stood() {
        let (_, _, mut clan) = crate::clan::tests::fixture();
        clan.name = "Brindle".into();
        clan.season = 2;
        clan.social.raids_won = 1;
        let line = season_line(&clan, Some(&world()));
        assert!(
            line.contains("1st of 3") && line.contains("renown 90"),
            "{line}"
        );
        assert!(line.contains("1 raid won"));
        assert!(season_line(&clan, None).contains("Season 2 ended"));
    }
}
