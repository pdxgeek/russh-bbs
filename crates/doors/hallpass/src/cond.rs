//! The condition language content uses to say who may do what.
//!
//! A condition is a short string, parsed when the content loads (so a typo fails the schema test,
//! not a live game):
//!
//! | Form | Holds when |
//! |---|---|
//! | `tag:X` | one of the player's traits grants the capability `X` |
//! | `trait:X` | the player has trait `X` |
//! | `item:X` | the player carries item `X` |
//! | `know:X` | the player has learned `X` (a route, a routine) |
//! | `world:X` | the big-scheme effect `X` is active in the shared world |
//! | `expelled` | the character has been expelled |
//! | `not:<cond>` | the inner condition does not hold |
//! | `stat>=N` (also `<=`, `>`, `<`, `=`) | a number compares: `legend`, `notoriety`, `heat`, `file`, `cool`, `nerve`, `cash`, `grades`, `pulled`, `caught`, `skill:ID`, `standing:CLIQUE`, `wary:NPC`, `grade:SUBJECT` |

use serde::Deserialize;

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `>=`
    Ge,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `<`
    Lt,
    /// `=`
    Eq,
}

/// A number the game keeps about a player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stat {
    /// Legend, the term's main score.
    Legend,
    /// How much the prefects and staff watch the player.
    Notoriety,
    /// Current heat with the staff.
    Heat,
    /// Points on the file.
    File,
    /// Cool.
    Cool,
    /// Nerve left today.
    Nerve,
    /// Cash.
    Cash,
    /// Average grade.
    Grades,
    /// Schemes pulled off this term.
    Pulled,
    /// Times caught in the act this term.
    Caught,
    /// A skill's level.
    Skill(String),
    /// Standing with a clique.
    Standing(String),
    /// A staff member's wariness of this player.
    Wary(String),
    /// One subject's grade.
    Grade(String),
}

/// One parsed condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cond {
    /// `tag:X`
    Tag(String),
    /// `trait:X`
    Trait(String),
    /// `item:X`
    Item(String),
    /// `know:X`
    Know(String),
    /// `world:X`
    World(String),
    /// `expelled`
    Expelled,
    /// `not:<cond>`
    Not(Box<Cond>),
    /// A comparison.
    Cmp {
        /// What is compared.
        stat: Stat,
        /// How.
        op: Op,
        /// Against what.
        value: i32,
    },
}

/// Why a condition string did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadCond(pub String);

impl std::fmt::Display for BadCond {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bad condition `{}`", self.0)
    }
}

fn stat_of(name: &str) -> Option<Stat> {
    Some(match name {
        "legend" => Stat::Legend,
        "notoriety" => Stat::Notoriety,
        "heat" => Stat::Heat,
        "file" => Stat::File,
        "cool" => Stat::Cool,
        "nerve" => Stat::Nerve,
        "cash" => Stat::Cash,
        "grades" => Stat::Grades,
        "pulled" => Stat::Pulled,
        "caught" => Stat::Caught,
        other => {
            let (kind, id) = other.split_once(':')?;
            if id.is_empty() {
                return None;
            }
            match kind {
                "skill" => Stat::Skill(id.to_owned()),
                "standing" => Stat::Standing(id.to_owned()),
                "wary" => Stat::Wary(id.to_owned()),
                "grade" => Stat::Grade(id.to_owned()),
                _ => return None,
            }
        }
    })
}

impl Cond {
    /// Parse one condition.
    pub fn parse(text: &str) -> Result<Self, BadCond> {
        let bad = || BadCond(text.to_owned());
        let text = text.trim();
        if text == "expelled" {
            return Ok(Self::Expelled);
        }
        if let Some(inner) = text.strip_prefix("not:") {
            return Ok(Self::Not(Box::new(Self::parse(inner)?)));
        }
        for (symbol, op) in [
            (">=", Op::Ge),
            ("<=", Op::Le),
            (">", Op::Gt),
            ("<", Op::Lt),
            ("=", Op::Eq),
        ] {
            if let Some((left, right)) = text.split_once(symbol) {
                let stat = stat_of(left.trim()).ok_or_else(bad)?;
                let value = right.trim().parse().map_err(|_| bad())?;
                return Ok(Self::Cmp { stat, op, value });
            }
        }
        let (kind, id) = text.split_once(':').ok_or_else(bad)?;
        if id.is_empty() || id.contains(char::is_whitespace) {
            return Err(bad());
        }
        let id = id.to_owned();
        match kind {
            "tag" => Ok(Self::Tag(id)),
            "trait" => Ok(Self::Trait(id)),
            "item" => Ok(Self::Item(id)),
            "know" => Ok(Self::Know(id)),
            "world" => Ok(Self::World(id)),
            _ => Err(bad()),
        }
    }
}

impl Cond {
    /// A few plain words for a condition, for a player who is being told what a lock wants.
    #[must_use]
    pub fn describe(&self) -> String {
        let words = |id: &str| id.replace('_', " ");
        match self {
            Self::Tag(t) => format!("a knack for {}", words(t)),
            Self::Trait(t) => format!("being {}", words(t)),
            Self::Item(i) => format!("carrying {}", words(i)),
            Self::Know(k) => format!("learning the {}", words(k)),
            Self::World(w) => format!("the school having changed ({})", words(w)),
            Self::Expelled => "being expelled".to_owned(),
            Self::Not(inner) => format!("not {}", inner.describe()),
            Self::Cmp { stat, op, value } => {
                let name = match stat {
                    Stat::Skill(id) | Stat::Standing(id) | Stat::Wary(id) | Stat::Grade(id) => {
                        words(id)
                    }
                    other => format!("{other:?}").to_lowercase(),
                };
                let symbol = match op {
                    Op::Ge => "at least",
                    Op::Le => "at most",
                    Op::Gt => "over",
                    Op::Lt => "under",
                    Op::Eq => "exactly",
                };
                format!("{name} {symbol} {value}")
            }
        }
    }
}

impl TryFrom<String> for Cond {
    type Error = BadCond;
    fn try_from(text: String) -> Result<Self, BadCond> {
        Self::parse(&text)
    }
}

impl<'de> Deserialize<'de> for Cond {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

impl Op {
    /// Compare `left` with `right`.
    #[must_use]
    pub const fn test(self, left: i32, right: i32) -> bool {
        match self {
            Self::Ge => left >= right,
            Self::Le => left <= right,
            Self::Gt => left > right,
            Self::Lt => left < right,
            Self::Eq => left == right,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn every_form_parses_and_typos_are_refused() {
        assert_eq!(Cond::parse("tag:forge").unwrap(), Cond::Tag("forge".into()));
        assert_eq!(
            Cond::parse("standing:ink_society>=15").unwrap(),
            Cond::Cmp {
                stat: Stat::Standing("ink_society".into()),
                op: Op::Ge,
                value: 15
            }
        );
        assert_eq!(
            Cond::parse("not:trait:teachers_pet").unwrap(),
            Cond::Not(Box::new(Cond::Trait("teachers_pet".into())))
        );
        assert_eq!(
            Cond::parse("file<=3").unwrap(),
            Cond::Cmp {
                stat: Stat::File,
                op: Op::Le,
                value: 3
            }
        );
        assert_eq!(Cond::parse("expelled").unwrap(), Cond::Expelled);
        for bad in [
            "",
            "tag:",
            "tag",
            "legend>=x",
            "mystery>=1",
            "skill:>=1",
            "weird:thing",
            "tag:a b",
        ] {
            assert!(Cond::parse(bad).is_err(), "{bad:?}");
        }
        assert!(
            Op::Ge.test(3, 3)
                && !Op::Gt.test(3, 3)
                && Op::Lt.test(2, 3)
                && Op::Eq.test(1, 1)
                && Op::Le.test(1, 2)
        );
    }
}
