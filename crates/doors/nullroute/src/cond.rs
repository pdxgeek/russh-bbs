//! A small condition language for content: what a choice, a story step or an ending needs.
//!
//! A condition is terms joined by `&`, each optionally negated with `!`:
//!
//! | Term | Meaning |
//! |---|---|
//! | `flag:name` | the story flag is set (names are listed in `arcs.toml`) |
//! | `own:N` | the character owns script number N |
//! | `rep>=10`, `heat<40`, `marks>=200`, `jobs`, `burns`, `bots`, `arcs_done` | a number against a value |
//! | `skill.break>=3` | a skill level (`probe`, `break`, `hide`, `fight`, `utility`) |
//! | `standing.2>=2` | standing with fixer N |
//! | `rig.shield>=2` | a rig level (`cpu`, `memory`, `cooling`, `iface`, `shield`) |
//! | `stat.hacks_in>=1` | a lifetime counter (see [`crate::player::stat`]) |
//! | `tally.wardens>=6` | what the whole board did this season |
//!
//! Operators are `>=`, `<=`, `==`, `>` and `<`. An empty condition is true. Conditions are parsed
//! when content loads, so a typo is a failed schema test, not a dead branch at run time.

use crate::player::{PARTS, Player, Ties};
use crate::social::Tallies;

/// A number a condition can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Var {
    /// Reputation.
    Rep,
    /// Heat.
    Heat,
    /// Marks on hand.
    Marks,
    /// Jobs done.
    Jobs,
    /// Times burned.
    Burns,
    /// Bots at the hideout.
    Bots,
    /// Arcs that have ended.
    ArcsDone,
    /// A skill level (index in `Family::ALL`).
    Skill(usize),
    /// Standing with a fixer.
    Standing(usize),
    /// A rig level.
    Rig(usize),
    /// A lifetime counter.
    Stat(usize),
    /// A season tally.
    Tally(usize),
}

/// A comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `>=`
    Ge,
    /// `<=`
    Le,
    /// `==`
    Eq,
    /// `>`
    Gt,
    /// `<`
    Lt,
}

/// One term.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Term {
    /// A story flag (its bit).
    Flag(u8),
    /// A script owned.
    Own(u8),
    /// A number against a value.
    Cmp(Var, Op, i64),
}

/// A parsed condition.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cond(Vec<(bool, Term)>);

const SKILLS: [&str; 5] = ["probe", "break", "hide", "fight", "utility"];
const RIGS: [&str; PARTS] = ["cpu", "memory", "cooling", "iface", "shield"];
const STATS: [&str; 8] = [
    "hacks",
    "hacks_in",
    "caught",
    "bounties",
    "crew_runs",
    "crew_jobs",
    "wardens",
    "finale",
];
const TALLIES: [&str; 9] = [
    "deep",
    "wardens",
    "crew_jobs",
    "hacks_in",
    "hacks_caught",
    "bounties",
    "burns",
    "jobs",
    "finale",
];

fn index_of(names: &[&str], name: &str) -> Option<usize> {
    names.iter().position(|n| *n == name)
}

fn var(name: &str) -> Result<Var, String> {
    Ok(match name {
        "rep" => Var::Rep,
        "heat" => Var::Heat,
        "marks" => Var::Marks,
        "jobs" => Var::Jobs,
        "burns" => Var::Burns,
        "bots" => Var::Bots,
        "arcs_done" => Var::ArcsDone,
        _ => {
            let (head, tail) = name
                .split_once('.')
                .ok_or(format!("unknown name `{name}`"))?;
            match head {
                "skill" => Var::Skill(index_of(&SKILLS, tail).ok_or(format!("no skill `{tail}`"))?),
                "rig" => Var::Rig(index_of(&RIGS, tail).ok_or(format!("no rig part `{tail}`"))?),
                "stat" => Var::Stat(index_of(&STATS, tail).ok_or(format!("no stat `{tail}`"))?),
                "tally" => {
                    Var::Tally(index_of(&TALLIES, tail).ok_or(format!("no tally `{tail}`"))?)
                }
                "standing" => {
                    let n: usize = tail.parse().map_err(|_| format!("no fixer `{tail}`"))?;
                    if n >= crate::player::FIXERS_MAX {
                        return Err(format!("no fixer `{tail}`"));
                    }
                    Var::Standing(n)
                }
                _ => return Err(format!("unknown name `{name}`")),
            }
        }
    })
}

impl Cond {
    /// Parse a condition. `flags` are the story flag names, in bit order.
    ///
    /// # Errors
    /// What is wrong, in a few words.
    pub fn parse(text: &str, flags: &[String]) -> Result<Self, String> {
        let mut terms = Vec::new();
        for raw in text.split('&') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            let (not, raw) = raw
                .strip_prefix('!')
                .map_or((false, raw), |r| (true, r.trim()));
            if let Some(name) = raw.strip_prefix("flag:") {
                let bit = flags
                    .iter()
                    .position(|f| f == name.trim())
                    .ok_or(format!("no flag `{}`", name.trim()))?;
                terms.push((
                    not,
                    Term::Flag(u8::try_from(bit).map_err(|_| "too many flags")?),
                ));
                continue;
            }
            if let Some(num) = raw.strip_prefix("own:") {
                let n: u8 = num
                    .trim()
                    .parse()
                    .map_err(|_| format!("bad script `{num}`"))?;
                terms.push((not, Term::Own(n)));
                continue;
            }
            let (at, op, len) = [
                (">=", Op::Ge),
                ("<=", Op::Le),
                ("==", Op::Eq),
                (">", Op::Gt),
                ("<", Op::Lt),
            ]
            .iter()
            .filter_map(|(s, op)| raw.find(s).map(|i| (i, *op, s.len())))
            .min_by_key(|(i, _, _)| *i)
            .ok_or(format!("no comparison in `{raw}`"))?;
            let name = raw[..at].trim();
            let value: i64 = raw[at + len..]
                .trim()
                .parse()
                .map_err(|_| format!("`{}` is not a number", raw[at + len..].trim()))?;
            terms.push((not, Term::Cmp(var(name)?, op, value)));
        }
        Ok(Self(terms))
    }

    /// Whether the condition holds.
    #[must_use]
    pub fn eval(&self, p: Option<&Player>, t: &Tallies) -> bool {
        self.0.iter().all(|(not, term)| {
            let v = match *term {
                Term::Flag(b) => p.is_some_and(|p| p.ties.flags & (1 << b) != 0),
                Term::Own(n) => p.is_some_and(|p| p.owned.contains(&n)),
                Term::Cmp(var, op, value) => {
                    let have = read(var, p, t);
                    match op {
                        Op::Ge => have >= value,
                        Op::Le => have <= value,
                        Op::Eq => have == value,
                        Op::Gt => have > value,
                        Op::Lt => have < value,
                    }
                }
            };
            v != *not
        })
    }

    /// The terms (for a test that wants to know what a condition asks for).
    #[must_use]
    pub fn terms(&self) -> &[(bool, Term)] {
        &self.0
    }
}

fn read(var: Var, p: Option<&Player>, t: &Tallies) -> i64 {
    let tally = |i: usize| -> i64 {
        i64::from(match i {
            0 => t.deep_breaches,
            1 => t.wardens,
            2 => t.crew_jobs,
            3 => t.hacks_in,
            4 => t.hacks_caught,
            5 => t.bounties_paid,
            6 => t.burns,
            7 => t.jobs,
            _ => t.finale,
        })
    };
    if let Var::Tally(i) = var {
        return tally(i);
    }
    let Some(p) = p else { return 0 };
    let ties: &Ties = &p.ties;
    match var {
        Var::Rep => i64::from(p.rep),
        Var::Heat => i64::from(p.heat),
        Var::Marks => i64::from(p.marks),
        Var::Jobs => i64::from(p.jobs),
        Var::Burns => i64::from(p.burns),
        Var::Bots => i64::from(p.bots),
        Var::ArcsDone => {
            i64::try_from(ties.arcs.iter().filter(|a| a.ending != 0).count()).unwrap_or(0)
        }
        Var::Skill(i) => i64::from(p.skills().get(i).copied().unwrap_or(0)),
        Var::Standing(i) => i64::from(p.standing.get(i).copied().unwrap_or(0)),
        Var::Rig(i) => i64::from(p.level(i)),
        Var::Stat(i) => i64::from(ties.stats.get(i).copied().unwrap_or(0)),
        Var::Tally(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::content::Content;

    fn player() -> Player {
        Player::new(&Content::embedded(), 1, "ann", 1, 100)
    }

    #[test]
    fn terms_parse_and_read_the_character() {
        let flags = vec!["a".to_owned(), "b".to_owned()];
        let mut p = player();
        p.rep = 12;
        p.ties.flags = 0b10;
        let t = Tallies::default();
        let ok = |s: &str| Cond::parse(s, &flags).unwrap().eval(Some(&p), &t);
        assert!(ok(""));
        assert!(ok("rep>=10"));
        assert!(!ok("rep>12"));
        assert!(ok("rep>=10 & !flag:a & flag:b"));
        assert!(!ok("flag:a"));
        assert!(ok("skill.break<=0 & standing.0==0 & rig.cpu<1"));
        assert_eq!(ok("own:1"), p.owned.contains(&1));
        assert!(ok("tally.wardens>=0"));
    }

    #[test]
    fn typos_are_refused_when_the_content_loads() {
        let flags = vec!["a".to_owned()];
        for bad in [
            "reputation>=3",
            "flag:zzz",
            "rep>=ten",
            "rep",
            "skill.sing>=1",
            "standing.9>=1",
            "own:x",
        ] {
            assert!(Cond::parse(bad, &flags).is_err(), "{bad}");
        }
    }

    #[test]
    fn tallies_are_the_boards_not_the_players() {
        let t = Tallies {
            wardens: 7,
            ..Tallies::default()
        };
        let c = Cond::parse("tally.wardens>=6", &[]).unwrap();
        assert!(c.eval(None, &t));
        assert!(!c.eval(None, &Tallies::default()));
    }
}
