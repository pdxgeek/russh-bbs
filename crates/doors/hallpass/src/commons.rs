//! What the social layer keeps: a character's own side ([`Social`], appended to the save at
//! version 3) and the school's shared side ([`Commons`], appended to the world at version 2).
//!
//! Everything one player does to or for another goes through [`Commons`]: a plot or a rumour is
//! a small record in the shared world, chosen from a template and aimed at a name taken from the
//! roster, never typed. Every list here has a cap, so no player can fill the world.

use crate::state::{Map, map_get};
use crate::wire::wire_struct;

/// The most pupils on the roster.
pub const ROSTER_CAP: usize = 16;
/// The most rumours going round at once.
pub const RUMOUR_CAP: usize = 10;
/// The most plots waiting in the world at once.
pub const PLOT_CAP: usize = 16;
/// The most plots waiting for any one pupil.
pub const PLOT_PER_TARGET: usize = 3;
/// The most verdicts on the mystery kept.
pub const VERDICT_CAP: usize = 12;
/// The most award wins kept.
pub const AWARD_CAP: usize = 24;
/// The most votes kept (one a pupil a term).
pub const VOTE_CAP: usize = 16;
/// The most candidates on the ballot.
pub const CANDIDATE_CAP: usize = 8;
/// The most lines about what was done to a character that they keep.
pub const INBOX_CAP: usize = 6;
/// The most backing one candidate can hold.
pub const BACKING_CAP: u16 = 60;

/// A character's own social state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[allow(clippy::struct_excessive_bools)] // plain saved state: each flag is its own fact
pub struct Social {
    /// Whether they have opted out of player-versus-player interactions, both ways.
    pub opt_out: bool,
    /// The day until which no plot is delivered to them (set after one lands or is countered).
    pub protected_until: i64,
    /// The highest plot id already delivered to them.
    pub seen_plot: u32,
    /// Plots they sent: `(target, day)`, kept for the pair and daily limits.
    pub sent: Vec<(String, i64)>,
    /// The day they last spread a rumour, and how many that day.
    pub rumour_day: i64,
    /// Rumours spread on `rumour_day`.
    pub rumours_today: u8,
    /// The term in which they last voted.
    pub voted_term: u32,
    /// The term in which they stood for election.
    pub ran_term: u32,
    /// The day they last canvassed, and how many times that day.
    pub canvass_day: i64,
    /// Times canvassed on `canvass_day`.
    pub canvassed: u8,
    /// The trust they cast on the mystery this term, if any.
    pub verdict: Option<String>,
    /// The term whose election result they have already been told.
    pub told_term: u32,
    /// The term whose finale they have already been told.
    pub told_finale: u32,
    /// Awards won, as `term:award`.
    pub awards: Vec<String>,
    /// What was done to them, newest last.
    pub inbox: Vec<String>,
}
wire_struct!(Social {
    opt_out,
    protected_until,
    seen_plot,
    sent,
    rumour_day,
    rumours_today,
    voted_term,
    ran_term,
    canvass_day,
    canvassed,
    verdict,
    told_term,
    told_finale,
    awards,
    inbox
});

impl Social {
    /// Add a line to the inbox, dropping the oldest past the cap.
    pub fn note(&mut self, line: String) {
        self.inbox.push(line);
        if self.inbox.len() > INBOX_CAP {
            self.inbox.remove(0);
        }
    }
}

/// One pupil on the roster: what the school knows of a player. The handle is already public
/// (the hall of legend names it), and the numbers are what the awards are scored on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pupil {
    /// The handle.
    pub name: String,
    /// The last day they played.
    pub last_day: i64,
    /// Whether they have opted out of player-versus-player interactions.
    pub opt_out: bool,
    /// Legend this term.
    pub legend: u32,
    /// Average grade.
    pub grades: u8,
    /// File points.
    pub file: u8,
    /// Schemes pulled off this term.
    pub pulled: u16,
    /// Clues found in the mystery.
    pub clues: u8,
    /// Club merit this term.
    pub merit: u16,
    /// Friends made.
    pub friends: u8,
}
wire_struct!(Pupil {
    name,
    last_day,
    opt_out,
    legend,
    grades,
    file,
    pulled,
    clues,
    merit,
    friends
});

/// A rumour going round, from a template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rumour {
    /// The template id.
    pub id: String,
    /// Who or what it is about: a character id, a clique id, or `self`.
    pub about: String,
    /// The handle of whoever started it.
    pub by: String,
    /// The day it started.
    pub day: i64,
}
wire_struct!(Rumour { id, about, by, day });

/// A plot waiting for its target, or its outcome waiting for its sender.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plot {
    /// A number that only grows.
    pub id: u32,
    /// The template id.
    pub kind: String,
    /// The sender's handle.
    pub from: String,
    /// The target's handle.
    pub to: String,
    /// The day it was sent.
    pub day: i64,
    /// 0 waiting, 1 landed, 2 countered, 3 turned away (opted out or protected).
    pub state: u8,
}
wire_struct!(Plot {
    id,
    kind,
    from,
    to,
    day,
    state
});

/// Plot states.
pub mod plot_state {
    /// Not yet delivered.
    pub const WAITING: u8 = 0;
    /// It landed.
    pub const LANDED: u8 = 1;
    /// The target saw it coming.
    pub const COUNTERED: u8 = 2;
    /// The target was out of reach (opted out or protected).
    pub const TURNED: u8 = 3;
}

/// Someone on the ballot: a pupil's handle, or `npc:ID`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Who.
    pub who: String,
    /// Canvassing: what the campaign has won.
    pub backing: u16,
}
wire_struct!(Candidate { who, backing });

/// The school election of the term.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Election {
    /// The ballot: players who stood (the characters who run are on it from the start).
    pub candidates: Vec<Candidate>,
    /// Votes cast: `(voter, candidate)`, one a voter.
    pub votes: Vec<(String, String)>,
    /// The winner, once counted.
    pub winner: Option<String>,
}
wire_struct!(Election {
    candidates,
    votes,
    winner
});

/// Whom a player trusted with the east wing's secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// The handle.
    pub by: String,
    /// The verdict id.
    pub trust: String,
    /// The day.
    pub day: i64,
}
wire_struct!(Verdict { by, trust, day });

/// An award won.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwardWin {
    /// The term.
    pub term: u32,
    /// The award id.
    pub award: String,
    /// The winner's handle.
    pub name: String,
}
wire_struct!(AwardWin { term, award, name });

/// The social side of the shared world.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Commons {
    /// Who plays here.
    pub roster: Vec<Pupil>,
    /// Rumours going round.
    pub rumours: Vec<Rumour>,
    /// Plots in the post.
    pub plots: Vec<Plot>,
    /// The next plot number.
    pub next_plot: u32,
    /// The election.
    pub election: Election,
    /// Trust cast on the mystery this term, in the order cast.
    pub verdicts: Vec<Verdict>,
    /// How the mystery ended this term, once it has.
    pub finale: Option<String>,
    /// Clues the whole school has found this term.
    pub clues: u16,
    /// Awards, newest last.
    pub awards: Vec<AwardWin>,
    /// The day each office-holder's term of notice began: reserved for the next slice.
    pub marks: Map<i64>,
}
wire_struct!(Commons {
    roster,
    rumours,
    plots,
    next_plot,
    election,
    verdicts,
    finale,
    clues,
    awards,
    marks
});

impl Commons {
    /// A pupil's roster entry.
    #[must_use]
    pub fn pupil(&self, name: &str) -> Option<&Pupil> {
        self.roster.iter().find(|p| p.name == name)
    }

    /// Whether a pupil has opted out. Someone not on the roster has not.
    #[must_use]
    pub fn opted_out(&self, name: &str) -> bool {
        self.pupil(name).is_some_and(|p| p.opt_out)
    }

    /// Plots waiting for `name`.
    #[must_use]
    pub fn waiting_for(&self, name: &str) -> usize {
        self.plots
            .iter()
            .filter(|p| p.to == name && p.state == plot_state::WAITING)
            .count()
    }

    /// Votes for a candidate.
    #[must_use]
    pub fn votes_for(&self, who: &str) -> usize {
        self.election.votes.iter().filter(|(_, c)| c == who).count()
    }

    /// A candidate's backing.
    #[must_use]
    pub fn backing(&self, who: &str) -> u16 {
        self.election
            .candidates
            .iter()
            .find(|c| c.who == who)
            .map_or(0, |c| c.backing)
    }

    /// Rumours in force about `who` (the template ids).
    pub fn rumours_about<'a>(&'a self, who: &'a str) -> impl Iterator<Item = &'a Rumour> {
        self.rumours.iter().filter(move |r| r.about == who)
    }

    /// The verdict a player cast, if any.
    #[must_use]
    pub fn verdict_of(&self, name: &str) -> Option<&str> {
        self.verdicts
            .iter()
            .find(|v| v.by == name)
            .map(|v| v.trust.as_str())
    }

    /// A value from a map by id (a small helper for the tick).
    #[must_use]
    pub fn mark(&self, id: &str) -> Option<i64> {
        map_get(&self.marks, id).copied()
    }
}
