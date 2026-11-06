//! Changes to the shared school, and the daily tick.
//!
//! A [`WorldDelta`] is a small pure edit of the stored [`World`]. The board may run a world edit
//! more than once (it retries on a conflict), so applying the same delta to the stored bytes
//! always gives a valid world, and a big scheme's effect is never applied twice.

use russh_bbs_door::DoorRng;

use crate::commons::{
    BACKING_CAP, CANDIDATE_CAP, Candidate, PLOT_CAP, PLOT_PER_TARGET, Plot, Pupil, ROSTER_CAP,
    RUMOUR_CAP, VERDICT_CAP, VOTE_CAP, Verdict, plot_state,
};
use crate::content::Content;
use crate::state::{Active, World, YEAR_CAP, YearEntry, map_get, map_set, to_u8};

/// One edit of the shared world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldDelta {
    /// A big scheme took effect.
    Effect {
        /// The effect id.
        id: String,
        /// The player's handle.
        by: String,
        /// The day.
        day: i64,
    },
    /// A place drew attention.
    Patrol {
        /// The place.
        place: String,
        /// Heat added.
        amount: u8,
    },
    /// A line for the paper.
    Headline {
        /// The day.
        day: i64,
        /// The words (template text with the name filled in).
        text: String,
    },
    /// A player's legend for the hall.
    Hall {
        /// The handle.
        name: String,
        /// Their legend.
        legend: u32,
    },
    /// What the school knows of a player (their roster entry, replaced whole).
    Roster(Pupil),
    /// A structured scheme posted from one pupil to another. The stored world decides whether it
    /// is accepted: neither may have opted out, and the limits hold against what is stored.
    Plot {
        /// The sender's handle.
        from: String,
        /// The target's handle.
        to: String,
        /// The template id.
        kind: String,
        /// The day.
        day: i64,
    },
    /// The target's verdict on a plot: landed, countered or turned away.
    PlotResolve {
        /// The plot number.
        id: u32,
        /// The state it takes.
        state: u8,
    },
    /// The sender has heard the outcome: the record goes.
    PlotCollect {
        /// The plot number.
        id: u32,
    },
    /// A rumour from a template.
    Rumour {
        /// The template id.
        id: String,
        /// A character id, a clique id, or `self`.
        about: String,
        /// The handle of whoever started it.
        by: String,
        /// The day.
        day: i64,
    },
    /// A pupil stands in the election.
    Nominate {
        /// The handle.
        who: String,
    },
    /// Canvassing for a pupil who stands.
    Backing {
        /// The handle.
        who: String,
        /// What it won.
        add: u16,
    },
    /// A vote.
    Vote {
        /// The voter's handle.
        voter: String,
        /// The ballot id.
        who: String,
    },
    /// Trust cast on how the east wing's secret comes out.
    Verdict {
        /// The handle.
        by: String,
        /// The verdict id.
        trust: String,
        /// The day.
        day: i64,
    },
    /// A clue was found somewhere in the school.
    Clue,
}

impl WorldDelta {
    /// Apply to a world. Idempotent for effects: one that is already in force is not stacked.
    pub fn apply(&self, world: &mut World, content: &Content) {
        match self {
            Self::Effect { id, by, day } => {
                let Some(effect) = content.effect(id) else {
                    return;
                };
                if world.has(id) {
                    return;
                }
                let until = (effect.days > 0).then_some(day + effect.days);
                world.active.push(Active {
                    id: id.clone(),
                    since: *day,
                    until,
                    by: by.clone(),
                });
                world.changes = world.changes.saturating_add(1);
            }
            Self::Patrol { place, amount } => {
                let now = i32::from(map_get(&world.patrol, place).copied().unwrap_or(0))
                    + i32::from(*amount);
                map_set(&mut world.patrol, place, to_u8(now.min(30)));
            }
            Self::Headline { day, text } => world.headline(*day, text.clone()),
            Self::Hall { name, legend } => world.hall_update(name, *legend),
            Self::Roster(pupil) => roster_upsert(world, pupil),
            Self::Plot {
                from,
                to,
                kind,
                day,
            } => post_plot(world, content, from, to, kind, *day),
            Self::PlotResolve { id, state } => {
                if let Some(p) = world
                    .commons
                    .plots
                    .iter_mut()
                    .find(|p| p.id == *id && p.state == plot_state::WAITING)
                {
                    p.state = *state;
                }
            }
            Self::PlotCollect { id } => world.commons.plots.retain(|p| p.id != *id),
            Self::Rumour { id, about, by, day } => {
                let c = &mut world.commons;
                c.rumours.retain(|r| !(r.id == *id && r.about == *about));
                c.rumours.push(crate::commons::Rumour {
                    id: id.clone(),
                    about: about.clone(),
                    by: by.clone(),
                    day: *day,
                });
                let over = c.rumours.len().saturating_sub(RUMOUR_CAP);
                c.rumours.drain(..over);
            }
            Self::Nominate { who } => {
                let c = &mut world.commons.election;
                if c.winner.is_none()
                    && c.candidates.len() < CANDIDATE_CAP
                    && !c.candidates.iter().any(|x| x.who == *who)
                {
                    c.candidates.push(Candidate {
                        who: who.clone(),
                        backing: 0,
                    });
                }
            }
            Self::Backing { who, add } => {
                let c = &mut world.commons.election;
                if c.winner.is_none()
                    && let Some(cand) = c.candidates.iter_mut().find(|x| x.who == *who)
                {
                    cand.backing = cand.backing.saturating_add(*add).min(BACKING_CAP);
                }
            }
            Self::Vote { voter, who } => cast(world, content, voter, who),
            Self::Verdict { by, trust, day } => {
                let c = &mut world.commons;
                if c.finale.is_none()
                    && c.verdicts.len() < VERDICT_CAP
                    && !c.verdicts.iter().any(|v| v.by == *by)
                    && content.verdict(trust).is_some()
                {
                    c.verdicts.push(Verdict {
                        by: by.clone(),
                        trust: trust.clone(),
                        day: *day,
                    });
                }
            }
            Self::Clue => {
                world.commons.clues = world.commons.clues.saturating_add(1);
            }
        }
    }

    /// Apply to stored bytes (or a new world started on `today`) and return the new bytes.
    ///
    /// Stored bytes that are not a world are left alone (`None`): an edit must never replace a
    /// damaged world with a fresh one.
    #[must_use]
    pub fn apply_to_bytes(
        &self,
        bytes: Option<&[u8]>,
        today: i64,
        content: &Content,
    ) -> Option<Vec<u8>> {
        let mut world = match bytes {
            Some(stored) => crate::wire::decode(stored)?,
            None => World::new(today),
        };
        self.apply(&mut world, content);
        Some(world.to_bytes())
    }
}

/// Put a pupil on the roster, or replace their entry. The roster is capped: the pupil who has
/// not played for longest makes room.
fn roster_upsert(world: &mut World, pupil: &Pupil) {
    let r = &mut world.commons.roster;
    if let Some(slot) = r.iter_mut().find(|p| p.name == pupil.name) {
        *slot = pupil.clone();
        return;
    }
    if r.len() >= ROSTER_CAP
        && let Some(oldest) = r
            .iter()
            .enumerate()
            .min_by_key(|(_, p)| p.last_day)
            .map(|(i, _)| i)
    {
        r.remove(oldest);
    }
    r.push(pupil.clone());
}

/// Accept a plot into the post, or quietly drop it. The checks are against the *stored* world, so
/// a session working from an old snapshot cannot get past an opt-out or a limit.
fn post_plot(world: &mut World, content: &Content, from: &str, to: &str, kind: &str, day: i64) {
    let rules = &content.social.rules;
    let c = &mut world.commons;
    if content.plot(kind).is_none() || from == to || c.opted_out(from) || c.opted_out(to) {
        return;
    }
    if c.pupil(to).is_none() {
        return;
    }
    if c.plots.len() >= PLOT_CAP || c.waiting_for(to) >= PLOT_PER_TARGET {
        return;
    }
    if c.plots
        .iter()
        .filter(|p| p.from == from && p.day == day)
        .count()
        >= rules.plots_per_day
    {
        return;
    }
    if c.plots
        .iter()
        .any(|p| p.from == from && p.to == to && day - p.day < rules.pair_days)
    {
        return;
    }
    c.next_plot = c.next_plot.saturating_add(1);
    c.plots.push(Plot {
        id: c.next_plot,
        kind: kind.to_owned(),
        from: from.to_owned(),
        to: to.to_owned(),
        day,
        state: plot_state::WAITING,
    });
}

/// Record a vote if the ballot has the name and the voter has not voted.
fn cast(world: &mut World, content: &Content, voter: &str, who: &str) {
    if world.commons.election.winner.is_some()
        || world.commons.election.votes.len() >= VOTE_CAP
        || world.commons.election.votes.iter().any(|(v, _)| v == voter)
    {
        return;
    }
    if crate::social::ballot_of(content, world)
        .iter()
        .any(|b| b.id == who)
    {
        world
            .commons
            .election
            .votes
            .push((voter.to_owned(), who.to_owned()));
    }
}

/// Apply one day to the world: the term clock, expiring effects, patrols easing, a new term when
/// the old one is over. A pure function of the world, the day and the tick's RNG.
pub fn tick_day(world: &mut World, content: &Content, day: i64, _rng: &mut DoorRng) {
    if day <= world.last_day {
        return;
    }
    world.last_day = day;
    world.active.retain(|a| a.until.is_none_or(|end| end > day));
    for (_, heat) in &mut world.patrol {
        *heat = heat.saturating_sub(1);
    }
    world.patrol.retain(|(_, heat)| *heat > 0);
    let rules = &content.social.rules;
    world
        .commons
        .plots
        .retain(|p| day - p.day <= rules.plot_days);
    world
        .commons
        .rumours
        .retain(|r| day - r.day <= rules.rumour_days);
    if world.term_day(day) >= rules.tally_day {
        crate::social::count_election(world, content, day);
    }
    if world.term_day(day) >= content.mystery.rules.finale_day {
        crate::social::finale(world, content, day);
    }
    if world.term_day(day) >= content.rules.term_days {
        crate::social::give_awards(world, content);
        if let Some(best) = world.hall.first() {
            world.yearbook.push(YearEntry {
                term: world.term,
                name: best.name.clone(),
                legend: best.legend,
                changes: world.changes,
            });
            if world.yearbook.len() > YEAR_CAP {
                world.yearbook.remove(0);
            }
        }
        world.term = world.term.saturating_add(1);
        world.term_start = day;
        world.active.clear();
        world.patrol.clear();
        world.hall.clear();
        world.changes = 0;
        crate::social::new_term(&mut world.commons);
        world.headline(day, "A new term begins at Greywater Academy.".to_owned());
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn an_edit_never_replaces_a_damaged_world() {
        let content = Content::embedded().unwrap();
        let delta = WorldDelta::Patrol {
            place: "quad".into(),
            amount: 1,
        };
        assert_eq!(delta.apply_to_bytes(Some(&[9, 9, 9]), 5, &content), None);
        let made = delta.apply_to_bytes(None, 5, &content).unwrap();
        assert_eq!(World::from_bytes(Some(&made), 0).patrol_heat("quad"), 1);
        let again = delta.apply_to_bytes(Some(&made), 5, &content).unwrap();
        assert_eq!(World::from_bytes(Some(&again), 0).patrol_heat("quad"), 2);
    }

    #[test]
    fn an_effect_is_applied_once_and_expires_and_a_term_rolls_over() {
        let content = Content::embedded().unwrap();
        let mut world = World::new(100);
        let delta = WorldDelta::Effect {
            id: "tuck_shop_closed".into(),
            by: "ann".into(),
            day: 103,
        };
        delta.apply(&mut world, &content);
        delta.apply(&mut world, &content);
        assert_eq!(world.active.len(), 1, "a retried edit does not stack");
        assert_eq!(world.changes, 1);
        let mut rng = DoorRng::from_seed(1);
        tick_day(&mut world, &content, 109, &mut rng);
        assert!(world.has("tuck_shop_closed"));
        tick_day(&mut world, &content, 110, &mut rng);
        assert!(
            !world.has("tuck_shop_closed"),
            "seven days, then it reopens"
        );
        world.hall_update("ann", 40);
        tick_day(
            &mut world,
            &content,
            100 + content.rules.term_days,
            &mut rng,
        );
        assert_eq!(
            (world.term, world.term_start),
            (2, 100 + content.rules.term_days)
        );
        assert_eq!(world.yearbook[0].name, "ann");
        assert_eq!(world.hall, Vec::new());
        // Ticking an old day changes nothing.
        let before = world.clone();
        tick_day(&mut world, &content, 50, &mut rng);
        assert_eq!(world, before);
    }
}
