//! Effects: the small, data-driven changes a conversation choice or a storyline ending makes.
//!
//! An effect is a short string parsed when the content loads, like a condition:
//!
//! | Form | Does |
//! |---|---|
//! | `stand:CLIQUE:+N` | standing with a clique |
//! | `bond:NPC:+N` | friendship with a character (negative for a rivalry) |
//! | `stat:GAUGE:+N` | `cool`, `nerve`, `cash`, `notoriety`, `heat` or `legend` |
//! | `know:X` / `forget:X` | learn or lose a flag the conditions can test |
//! | `file:KIND:N` | a line on the file |
//! | `wary:NPC:+N` | a staff member's wariness |
//! | `item:ID` | an item is handed over |
//! | `xp:SKILL:N` | skill experience |
//! | `grade:SUBJECT:+N` | a grade |
//! | `merit:CLUB:+N` | club merit (only for a member) |
//! | `trait:ID` | an earned trait |
//! | `arc:ID:STEP` | a storyline moves to a step |
//! | `arc_end:ID:ENDING` | a storyline ends, and its ending's own effects follow |
//! | `world:EFFECT` | a big change to the school everyone shares |
//! | `verdict:ID` | cast trust on how the east wing's secret comes out (the mystery's endings) |
//! | `betray:NPC` | the player sells a helper out; `betray:last_ally` names the latest scheme's first helper |

use serde::Deserialize;

/// A number the player keeps that an effect can move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gauge {
    /// Cool.
    Cool,
    /// Nerve left today.
    Nerve,
    /// Cash.
    Cash,
    /// Notoriety.
    Notoriety,
    /// Heat.
    Heat,
    /// Legend.
    Legend,
}

/// One parsed effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eff {
    /// Standing with a clique.
    Stand(String, i32),
    /// Friendship with a character.
    Bond(String, i32),
    /// A gauge.
    Stat(Gauge, i32),
    /// Learn a flag.
    Know(String),
    /// Lose a flag.
    Forget(String),
    /// A file entry of a kind, with a severity.
    File(String, u8),
    /// Wariness of a staff member.
    Wary(String, i32),
    /// An item.
    Item(String),
    /// Skill experience.
    Xp(String, i32),
    /// A grade.
    Grade(String, i32),
    /// Club merit.
    Merit(String, i32),
    /// A trait.
    Trait(String),
    /// A storyline moves.
    Arc(String, String),
    /// A storyline ends.
    ArcEnd(String, String),
    /// A world effect.
    World(String),
    /// A betrayal.
    Betray(String),
    /// Trust cast on how the east wing's secret comes out (the mystery's endings).
    Verdict(String),
}

/// Why an effect string did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadEff(pub String);

impl std::fmt::Display for BadEff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bad effect `{}`", self.0)
    }
}

fn gauge(name: &str) -> Option<Gauge> {
    Some(match name {
        "cool" => Gauge::Cool,
        "nerve" => Gauge::Nerve,
        "cash" => Gauge::Cash,
        "notoriety" => Gauge::Notoriety,
        "heat" => Gauge::Heat,
        "legend" => Gauge::Legend,
        _ => return None,
    })
}

impl Eff {
    /// Parse one effect.
    pub fn parse(text: &str) -> Result<Self, BadEff> {
        let bad = || BadEff(text.to_owned());
        let parts: Vec<&str> = text.trim().split(':').collect();
        if parts
            .iter()
            .any(|p| p.is_empty() || p.contains(char::is_whitespace))
        {
            return Err(bad());
        }
        let num = |s: &str| s.trim_start_matches('+').parse::<i32>().map_err(|_| bad());
        let id = |s: &str| s.to_owned();
        Ok(match parts.as_slice() {
            ["stand", clique, n] => Self::Stand(id(clique), num(n)?),
            ["bond", npc, n] => Self::Bond(id(npc), num(n)?),
            ["stat", g, n] => Self::Stat(gauge(g).ok_or_else(bad)?, num(n)?),
            ["know", x] => Self::Know(id(x)),
            ["forget", x] => Self::Forget(id(x)),
            ["file", kind, n] => Self::File(id(kind), u8::try_from(num(n)?).map_err(|_| bad())?),
            ["wary", npc, n] => Self::Wary(id(npc), num(n)?),
            ["item", x] => Self::Item(id(x)),
            ["xp", skill, n] => Self::Xp(id(skill), num(n)?),
            ["grade", subject, n] => Self::Grade(id(subject), num(n)?),
            ["merit", club, n] => Self::Merit(id(club), num(n)?),
            ["trait", x] => Self::Trait(id(x)),
            ["arc", a, step] => Self::Arc(id(a), id(step)),
            ["arc_end", a, ending] => Self::ArcEnd(id(a), id(ending)),
            ["world", x] => Self::World(id(x)),
            ["betray", x] => Self::Betray(id(x)),
            ["verdict", x] => Self::Verdict(id(x)),
            _ => return Err(bad()),
        })
    }
}

impl<'de> Deserialize<'de> for Eff {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn every_form_parses_and_typos_are_refused() {
        assert_eq!(
            Eff::parse("stand:the_bench:+5").unwrap(),
            Eff::Stand("the_bench".into(), 5)
        );
        assert_eq!(
            Eff::parse("bond:sol:-4").unwrap(),
            Eff::Bond("sol".into(), -4)
        );
        assert_eq!(
            Eff::parse("stat:cash:3").unwrap(),
            Eff::Stat(Gauge::Cash, 3)
        );
        assert_eq!(
            Eff::parse("arc_end:bell:good").unwrap(),
            Eff::ArcEnd("bell".into(), "good".into())
        );
        assert_eq!(
            Eff::parse("betray:last_ally").unwrap(),
            Eff::Betray("last_ally".into())
        );
        for bad in [
            "",
            "stand:x",
            "stat:luck:1",
            "bond:sol:lots",
            "know:",
            "know:a b",
            "file:late:-1",
            "weird:1",
            "arc:only",
        ] {
            assert!(Eff::parse(bad).is_err(), "{bad:?}");
        }
    }
}
