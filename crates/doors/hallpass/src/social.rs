//! The social layer (M4-H4): structured player schemes, template rumours, the school election,
//! the end-of-term awards, the opt-out and the east wing's search.
//!
//! **Players never type at each other.** A plot is a template from `social.toml` aimed at a name
//! taken from the roster; a rumour is a template aimed at a character, a clique or yourself. What
//! one player does to another is a record in the shared world ([`Commons`]); the target's own
//! session delivers it the next time they open the door, decides whether it lands (an opt-out, a
//! protection window and a counterplay all stop it), and the sender hears the outcome the next
//! time they open. Every list is capped, so no player can fill the world or pile on one target.
//!
//! The world is read once when a visit opens (the framework has no read of the current world
//! for a session; `docs/coordination.md`), so delivery happens at the start of a visit.

use russh_bbs_door::DoorRng;

use crate::commons::{AwardWin, Commons, PLOT_CAP, PLOT_PER_TARGET, Plot, Pupil, plot_state};
use crate::content::Content;
use crate::effect::Eff;
use crate::game::{Action, Event, Game, Refused, Spent};
use crate::state::{World, to_i16, to_u8};
use crate::world::WorldDelta;

/// One line of the ballot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BallotLine {
    /// The id to vote for: `npc:ID` or a pupil's handle.
    pub id: String,
    /// The name to show.
    pub name: String,
    /// What they stand for.
    pub platform: String,
    /// Canvassing, rumours and the swing of the school, before votes.
    pub backing: i32,
    /// Votes cast for them so far.
    pub votes: usize,
    /// Whether the character running is a player.
    pub player: bool,
}

/// What a roster pupil can be sent, and whether they can be reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classmate {
    /// The handle.
    pub name: String,
    /// Whether they opted out (and so cannot be reached, and cannot reach anyone).
    pub opted_out: bool,
    /// Legend this term.
    pub legend: u32,
    /// The last day they played.
    pub last_day: i64,
}

impl Game {
    // ------------------------------------------------------------ the roster

    /// What the school knows of this player, for the roster and the awards.
    #[must_use]
    pub fn pupil_entry(&self) -> Pupil {
        let p = &self.player;
        let friend = self.content.rules.friend_bond;
        Pupil {
            name: p.name.clone(),
            last_day: p.day,
            opt_out: p.social.opt_out,
            legend: p.legend,
            grades: to_u8(self.average_grade()),
            file: to_u8(p.file_points()),
            pulled: p.pulled,
            clues: to_u8(self.stat(&crate::cond::Stat::Clues)),
            merit: p.life.clubs.iter().map(|m| m.merit).sum(),
            friends: to_u8(
                i32::try_from(
                    p.life
                        .bonds
                        .iter()
                        .filter(|(_, b)| i32::from(*b) >= friend)
                        .count(),
                )
                .unwrap_or(0),
            ),
        }
    }

    /// The other pupils, as this session sees the roster.
    #[must_use]
    pub fn classmates(&self) -> Vec<Classmate> {
        let today = self.player.day;
        self.world
            .commons
            .roster
            .iter()
            .filter(|p| p.name != self.player.name && today - p.last_day <= 28)
            .map(|p| Classmate {
                name: p.name.clone(),
                opted_out: p.opt_out,
                legend: p.legend,
                last_day: p.last_day,
            })
            .collect()
    }

    // ------------------------------------------------------------ the opt-out

    pub(crate) fn set_opt_out(&mut self, out: bool) -> Vec<Event> {
        if self.player.social.opt_out == out {
            return vec![Event::Say(if out {
                "You are already opted out of player-versus-player play.".into()
            } else {
                "You are already open to player-versus-player play.".into()
            })];
        }
        self.player.social.opt_out = out;
        vec![Event::Say(if out {
            "You have opted out. Nobody can plot against you, and you cannot plot against anyone; rumours, the election and the awards carry on.".into()
        } else {
            "You are open to plots again, from the fixed list, with a counterplay and a rest between.".into()
        })]
    }

    // ------------------------------------------------------------ plots

    /// Why `kind` cannot be sent to `to` right now, or `None` if it can.
    #[must_use]
    pub fn plot_block(&self, kind: &str, to: &str) -> Option<String> {
        let spec = self.content.plot(kind)?;
        let p = &self.player;
        let rules = &self.content.social.rules;
        let today = p.day;
        if p.social.opt_out {
            return Some("You have opted out of player-versus-player play.".into());
        }
        if to == p.name {
            return Some("Not on yourself.".into());
        }
        let Some(target) = self.world.commons.pupil(to) else {
            return Some("There is nobody of that name at school.".into());
        };
        if target.opt_out {
            return Some(format!("{to} has opted out of this."));
        }
        if today - target.last_day > 28 {
            return Some(format!("{to} has not been about for weeks."));
        }
        if !self.all_hold(&spec.needs) {
            return Some("You are not the sort who can pull that off yet.".into());
        }
        if i32::from(p.nerve) < spec.nerve {
            return Some("You do not have the nerve.".into());
        }
        if p.cash < spec.cash {
            return Some("You cannot afford it.".into());
        }
        let sent_today = p.social.sent.iter().filter(|(_, d)| *d == today).count();
        if sent_today >= rules.plots_per_day {
            return Some("You have done enough scheming for one day.".into());
        }
        if p.social
            .sent
            .iter()
            .any(|(t, d)| t == to && today - d < rules.pair_days)
        {
            return Some(format!(
                "You have just had a go at {to}; give it a few days."
            ));
        }
        let c = &self.world.commons;
        if c.waiting_for(to) >= PLOT_PER_TARGET || c.plots.len() >= PLOT_CAP {
            return Some(format!("The post is full: {to} has enough on their plate."));
        }
        None
    }

    pub(crate) fn send_plot(
        &mut self,
        kind: &str,
        to: &str,
        rng: &mut DoorRng,
    ) -> Result<Vec<Event>, Refused> {
        self.current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        let spec = self
            .content
            .plot(kind)
            .cloned()
            .ok_or_else(|| Refused::new("There is no such scheme to play."))?;
        if let Some(why) = self.plot_block(kind, to) {
            return Err(Refused::new(why));
        }
        let day = self.player.day;
        self.player.nerve = to_u8(i32::from(self.player.nerve) - spec.nerve);
        self.player.cash -= spec.cash;
        self.player.social.sent.push((to.to_owned(), day));
        let keep = self.content.social.rules.pair_days.max(7);
        self.player.social.sent.retain(|(_, d)| day - d < keep);
        self.push_world(WorldDelta::Plot {
            from: self.player.name.clone(),
            to: to.to_owned(),
            kind: kind.to_owned(),
            day,
        });
        let mut events = vec![Event::Say(format!(
            "You set {} in motion for {to}. You will hear how it went the next time you are at school.",
            spec.name.to_lowercase()
        ))];
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    /// Deliver the plots waiting for this player and collect the outcome of the ones they sent.
    pub(crate) fn deliver_plots(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        let day = self.player.day;
        let name = self.player.name.clone();
        let rules = self.content.social.rules.clone();
        let waiting: Vec<Plot> = self
            .world
            .commons
            .plots
            .iter()
            .filter(|p| p.to == name && p.state == plot_state::WAITING)
            .filter(|p| p.id > self.player.social.seen_plot)
            .cloned()
            .collect();
        for plot in waiting {
            self.player.social.seen_plot = self.player.social.seen_plot.max(plot.id);
            let Some(spec) = self.content.plot(&plot.kind).cloned() else {
                continue;
            };
            let state = if self.player.social.opt_out || self.player.social.protected_until > day {
                plot_state::TURNED
            } else if spec.kind == "mischief" && self.all_hold_any(&spec.counter) {
                events.push(Event::Say(spec.countered.replace("{from}", &plot.from)));
                self.player
                    .social
                    .note(spec.countered.replace("{from}", &plot.from));
                self.player.social.protected_until = day + rules.protect_days;
                plot_state::COUNTERED
            } else {
                for e in &spec.hit {
                    if let Eff::Stat(g, n) = e {
                        events.extend(self.apply_effects(&[Eff::Stat(*g, *n)], 3));
                    }
                }
                let line = spec.seen.replace("{from}", &plot.from);
                events.push(Event::Say(line.clone()));
                self.player.social.note(line);
                if spec.kind == "mischief" {
                    self.player.social.protected_until = day + rules.protect_days;
                }
                plot_state::LANDED
            };
            self.push_world(WorldDelta::PlotResolve { id: plot.id, state });
        }
        let done: Vec<Plot> = self
            .world
            .commons
            .plots
            .iter()
            .filter(|p| p.from == name && p.state != plot_state::WAITING)
            .cloned()
            .collect();
        for plot in done {
            if let Some(spec) = self.content.plot(&plot.kind).cloned() {
                match plot.state {
                    plot_state::LANDED => {
                        events.push(Event::Say(format!(
                            "News of your {} on {}: it landed.",
                            spec.name.to_lowercase(),
                            plot.to
                        )));
                        events.extend(self.apply_effects(&spec.reward, 3));
                    }
                    plot_state::COUNTERED => {
                        events.push(Event::Say(format!(
                            "News of your {} on {}: they saw it coming.",
                            spec.name.to_lowercase(),
                            plot.to
                        )));
                        events.extend(self.apply_effects(&spec.fizzle, 3));
                    }
                    _ => events.push(Event::Say(format!(
                        "Your {} on {} never reached them.",
                        spec.name.to_lowercase(),
                        plot.to
                    ))),
                }
            }
            self.push_world(WorldDelta::PlotCollect { id: plot.id });
        }
        events
    }

    /// Whether any one of the conditions holds (a counterplay is any one way out).
    fn all_hold_any(&self, conds: &[crate::cond::Cond]) -> bool {
        conds.iter().any(|c| self.holds(c))
    }

    // ------------------------------------------------------------ rumours

    /// Why `rumour` about `about` cannot be started now, or `None`.
    #[must_use]
    pub fn rumour_block(&self, rumour: &str, about: &str) -> Option<String> {
        let spec = self.content.rumour(rumour)?;
        let p = &self.player;
        if !self
            .content
            .rumour_subjects(spec)
            .iter()
            .any(|s| s == about)
        {
            return Some("That is not something that can be said about them.".into());
        }
        if !self.all_hold(&spec.needs) {
            return Some("Nobody would believe it from you yet.".into());
        }
        let rules = &self.content.social.rules;
        if p.social.rumour_day == p.day && p.social.rumours_today >= rules.rumours_per_day {
            return Some("You have had your say for today.".into());
        }
        let Some(slot) = self.current_slot() else {
            return Some("The day is over.".into());
        };
        let audience = self
            .npcs_at(&p.place, p.day, slot)
            .iter()
            .any(|n| !n.is_authority());
        if !audience {
            return Some("There is nobody here to tell.".into());
        }
        None
    }

    pub(crate) fn spread(
        &mut self,
        rumour: &str,
        about: &str,
        rng: &mut DoorRng,
    ) -> Result<Vec<Event>, Refused> {
        let spec = self
            .content
            .rumour(rumour)
            .cloned()
            .ok_or_else(|| Refused::new("There is no such rumour."))?;
        if let Some(why) = self.rumour_block(rumour, about) {
            return Err(Refused::new(why));
        }
        let day = self.player.day;
        if self.player.social.rumour_day != day {
            self.player.social.rumour_day = day;
            self.player.social.rumours_today = 0;
        }
        self.player.social.rumours_today += 1;
        let npc = self.content.npc(about).cloned();
        let clique = match (&npc, spec.about.as_str()) {
            (Some(n), _) => n.clique.clone(),
            (None, "clique") => Some(about.to_owned()),
            _ => None,
        };
        if let Some(c) = clique.as_deref()
            && spec.stand != 0
        {
            self.player.add_standing(c, spec.stand);
        }
        if let Some(n) = &npc
            && spec.bond != 0
        {
            let _ = self.change_bond(&n.id, spec.bond);
        }
        let p = &mut self.player;
        p.cool = to_i16(i32::from(p.cool) + spec.cool);
        p.heat = to_u8(i32::from(p.heat) + spec.heat);
        if spec.legend != 0 {
            let _ = self.legend_gain(i64::from(spec.legend));
        }
        if let Some(n) = &npc
            && spec.patrol > 0
        {
            self.push_world(WorldDelta::Patrol {
                place: n.home.clone(),
                amount: to_u8(spec.patrol),
            });
        }
        self.push_world(WorldDelta::Rumour {
            id: rumour.to_owned(),
            about: about.to_owned(),
            by: self.player.name.clone(),
            day,
        });
        let subject = match (&npc, spec.about.as_str()) {
            (Some(n), _) => n.name.clone(),
            (None, "clique") => self
                .content
                .clique(about)
                .map_or_else(|| about.to_owned(), |c| c.name.clone()),
            _ => self.player.name.clone(),
        };
        let mut events = vec![Event::Say(format!(
            "You let it drop where it will be heard: \"{}\"",
            spec.text.replace("{about}", &subject)
        ))];
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    /// The rumours going round, as lines for the paper and the morning bell.
    #[must_use]
    pub fn rumour_lines(&self) -> Vec<String> {
        let today = self.player.day;
        let days = self.content.social.rules.rumour_days;
        self.world
            .commons
            .rumours
            .iter()
            .rev()
            .filter(|r| today - r.day < days)
            .filter_map(|r| {
                let spec = self.content.rumour(&r.id)?;
                let subject = self
                    .content
                    .npc(&r.about)
                    .map(|n| n.name.clone())
                    .or_else(|| self.content.clique(&r.about).map(|c| c.name.clone()))
                    .unwrap_or_else(|| r.by.clone());
                Some(spec.text.replace("{about}", &subject))
            })
            .collect()
    }

    // ------------------------------------------------------------ the election

    /// Whether the election is open to stand in now.
    #[must_use]
    pub fn election_open(&self) -> bool {
        let r = &self.content.social.rules;
        let d = self.term_day();
        d >= r.nominate_from && d < r.tally_day && self.world.commons.election.winner.is_none()
    }

    /// Whether votes may be cast now.
    #[must_use]
    pub fn voting_open(&self) -> bool {
        let r = &self.content.social.rules;
        let d = self.term_day();
        d >= r.vote_from && d < r.tally_day && self.world.commons.election.winner.is_none()
    }

    /// The ballot as this session sees the world: characters who run, then pupils who stood.
    #[must_use]
    pub fn ballot(&self) -> Vec<BallotLine> {
        ballot_of(&self.content, &self.world)
    }

    pub(crate) fn nominate(&mut self) -> Result<Vec<Event>, Refused> {
        if !self.election_open() {
            return Err(Refused::new("There is no election to stand in now."));
        }
        let term = self.player.term;
        if self.player.social.ran_term == term {
            return Err(Refused::new("You are already standing."));
        }
        if !self.all_hold(&self.content.social.rules.nominate_needs.clone()) {
            return Err(Refused::new(
                "The prefects will not take your name: you need a better record to stand.",
            ));
        }
        if self.world.commons.election.candidates.len() >= crate::commons::CANDIDATE_CAP {
            return Err(Refused::new("The ballot is full."));
        }
        self.player.social.ran_term = term;
        self.push_world(WorldDelta::Nominate {
            who: self.player.name.clone(),
        });
        Ok(vec![Event::Say(
            "Your name goes on the ballot. Now win them round.".into(),
        )])
    }

    pub(crate) fn canvass(&mut self, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let rules = self.content.social.rules.clone();
        if !self.election_open() || self.player.social.ran_term != self.player.term {
            return Err(Refused::new("You are not standing in an election now."));
        }
        let day = self.player.day;
        if self.player.social.canvass_day != day {
            self.player.social.canvass_day = day;
            self.player.social.canvassed = 0;
        }
        if self.player.social.canvassed >= rules.canvass_per_day {
            return Err(Refused::new("You have worked the corridors enough today."));
        }
        if self.player.nerve == 0 {
            return Err(Refused::new("You do not have the nerve to knock on doors."));
        }
        self.current_slot()
            .ok_or_else(|| Refused::new("The day is over."))?;
        self.player.nerve -= 1;
        self.player.social.canvassed += 1;
        let best = self
            .player
            .standing
            .iter()
            .map(|(_, v)| i32::from(*v))
            .max()
            .unwrap_or(0);
        let base = (1 + best.max(0) / 16 + i32::from(self.player.cool).max(0) / 20).clamp(1, 4);
        // Each door knocked on wins fewer as the campaign reaches the people who would listen.
        let cap = i32::from(crate::commons::BACKING_CAP);
        let have = i32::from(self.world.commons.backing(&self.player.name));
        let add = ((base * (cap - have) + cap / 2) / cap).clamp(1, 4);
        self.push_world(WorldDelta::Backing {
            who: self.player.name.clone(),
            add: u16::try_from(add).unwrap_or(1),
        });
        self.gain_xp("bluff", 1);
        let mut events = vec![Event::Say(format!(
            "You work the corridors for your cause (+{add} backing)."
        ))];
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    pub(crate) fn cast_vote(&mut self, who: &str) -> Result<Vec<Event>, Refused> {
        if !self.voting_open() {
            return Err(Refused::new("The polls are not open."));
        }
        if self.player.social.voted_term == self.player.term {
            return Err(Refused::new("You have already voted this term."));
        }
        let Some(line) = self.ballot().into_iter().find(|b| b.id == who) else {
            return Err(Refused::new("That name is not on the ballot."));
        };
        self.player.social.voted_term = self.player.term;
        self.push_world(WorldDelta::Vote {
            voter: self.player.name.clone(),
            who: who.to_owned(),
        });
        Ok(vec![Event::Say(format!(
            "You mark your paper for {}.",
            line.name
        ))])
    }

    // ------------------------------------------------------------ the mystery

    /// The clue a search here and now would turn up, if any.
    #[must_use]
    pub fn next_clue(&self) -> Option<&crate::social_spec::ClueSpec> {
        let slot = self.current_slot()?;
        let kind = self.kind(self.player.day, slot);
        self.content.mystery.clues.iter().find(|c| {
            c.place == self.player.place
                && c.kinds.iter().any(|k| k == kind)
                && !self.player.knows(&format!("clue_{}", c.id))
                && c.after
                    .iter()
                    .all(|a| self.player.knows(&format!("clue_{a}")))
                && self.all_hold(&c.needs)
        })
    }

    pub(crate) fn search(&mut self, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let Some(clue) = self.next_clue().cloned() else {
            return Err(Refused::new(
                "You search, and find nothing worth the trouble.",
            ));
        };
        let day = self.player.day;
        let slot = self.current_slot().unwrap_or(0);
        let mut events = Vec::new();
        let rules = self.content.mystery.rules.clone();
        let caught = (i32::from(clue.risk) * rules.risk_pct
            + 4 * self.patrol_level(&self.player.place)
            - 5 * self.skill_level("sneak")
            - i32::from(self.player.cool).max(0) / 10)
            .clamp(0, 90);
        let _ = (day, slot);
        if clue.risk > 0 && i32::try_from(rng.below(100)).unwrap_or(0) < caught {
            events.push(Event::Say(
                "A torch swings round the door. You duck out, but not unseen: a note for your housemaster.".into(),
            ));
            events.extend(self.add_file("out_of_bounds", 1));
            self.gain_xp("sneak", 1);
            events.extend(self.end_slot(Spent::Other, rng));
            return Ok(events);
        }
        self.player.learn(&format!("clue_{}", clue.id));
        events.push(Event::Say(clue.text.clone()));
        events.extend(self.apply_effects(&clue.effects, 0));
        self.gain_xp("sneak", 1);
        self.push_world(WorldDelta::Clue);
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    /// Cast trust on how the east wing's secret comes out (the mystery's endings do this).
    pub(crate) fn cast_verdict(&mut self, id: &str) -> Vec<Event> {
        if self.player.social.verdict.is_some() || self.content.verdict(id).is_none() {
            return Vec::new();
        }
        self.player.social.verdict = Some(id.to_owned());
        self.push_world(WorldDelta::Verdict {
            by: self.player.name.clone(),
            trust: id.to_owned(),
            day: self.player.day,
        });
        vec![Event::Say(
            "You have made up your mind about the East Wing. Whatever comes of it comes to everyone.".into(),
        )]
    }

    // ------------------------------------------------------------ the days

    /// Plots delivered, the election result and the finale, for a character in school.
    fn social_news(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        events.extend(self.deliver_plots());
        let term = self.player.term;
        let winner = self.world.commons.election.winner.clone();
        if let Some(winner) = winner
            && self.player.social.told_term != term
        {
            self.player.social.told_term = term;
            let rules = self.content.social.rules.clone();
            if winner == self.player.name {
                events.push(Event::Say(
                    "You won the school election. The prefects shake your hand through gritted teeth.".into(),
                ));
                let _ = self.legend_gain(i64::from(rules.winner_legend));
                events.extend(self.apply_effects(&[Eff::Trait(rules.winner_trait.clone())], 3));
            } else {
                events.push(Event::Say(format!(
                    "The school has chosen {} as its head.",
                    self.winner_name(&winner)
                )));
            }
        }
        if let Some(end) = self.world.commons.finale.clone()
            && self.player.social.told_finale != term
        {
            self.player.social.told_finale = term;
            let line = self.content.verdict(&end).map_or_else(
                || self.content.mystery.rules.unsolved.clone(),
                |v| format!("The East Wing: {}.", v.blurb),
            );
            events.push(Event::Say(line));
        }
        events
    }

    /// Take the board's latest view of the school (`Ctx::world`) in the middle of a visit: what
    /// other pupils posted since the visit opened is delivered now (a scheme aimed at this pupil,
    /// the result of the election, the finale), and the roster is fresh. A view of another term,
    /// or one that is damaged or no different, changes nothing.
    pub fn refresh_world(&mut self, bytes: &[u8]) -> Vec<Event> {
        let Some(fresh) = crate::save::decode_world(bytes) else {
            return Vec::new();
        };
        if fresh == self.world || fresh.term != self.world.term {
            return Vec::new();
        }
        self.world = fresh;
        if self.player.status == crate::state::Status::Active && !self.term_over() {
            self.social_day()
        } else {
            Vec::new()
        }
    }

    /// What the social layer owes this player at the start of a visit: plots delivered, the
    /// election result, the finale, awards won.
    pub fn social_day(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if self.player.status == crate::state::Status::Active {
            events.extend(self.social_news());
        }
        let wins: Vec<AwardWin> = self
            .world
            .commons
            .awards
            .iter()
            .filter(|a| a.name == self.player.name)
            .filter(|a| {
                !self
                    .player
                    .social
                    .awards
                    .contains(&format!("{}:{}", a.term, a.award))
            })
            .cloned()
            .collect();
        for win in wins {
            if self.player.social.awards.len() >= 24 {
                break;
            }
            let Some(spec) = self
                .content
                .social
                .awards
                .iter()
                .find(|a| a.id == win.award)
            else {
                continue;
            };
            let line = format!("Award, term {}: {}. {}", win.term, spec.name, spec.text);
            if self.player.status == crate::state::Status::Active {
                self.player.cash = self
                    .player
                    .cash
                    .saturating_add(self.content.social.rules.award_cash);
            } else {
                self.player.social.unpaid = self.player.social.unpaid.saturating_add(1);
            }
            self.player
                .social
                .awards
                .push(format!("{}:{}", win.term, win.award));
            self.player.social.note(line.clone());
            events.push(Event::Say(line));
        }
        events
    }

    /// The name of a rumour's subject: a character, a clique, or yourself.
    #[must_use]
    pub fn subject_name(&self, about: &str) -> String {
        self.content
            .npc(about)
            .map(|n| n.name.clone())
            .or_else(|| self.content.clique(about).map(|c| c.name.clone()))
            .unwrap_or_else(|| "Yourself".to_owned())
    }

    /// The name of an election winner id.
    #[must_use]
    pub fn winner_name(&self, id: &str) -> String {
        id.strip_prefix("npc:")
            .and_then(|n| self.content.npc(n))
            .map_or_else(|| id.to_owned(), |n| n.name.clone())
    }

    /// An award's title for a stored `term:award` entry.
    #[must_use]
    pub fn award_title(&self, entry: &str) -> String {
        let (term, id) = entry.split_once(':').unwrap_or(("?", entry));
        let name = self
            .content
            .social
            .awards
            .iter()
            .find(|a| a.id == id)
            .map_or(id, |a| a.name.as_str());
        format!("{name} (term {term})")
    }
}

// ---------------------------------------------------------------- world side

/// The ballot of a world: characters who run (unless withdrawn), then pupils who stood.
#[must_use]
pub fn ballot_of(content: &Content, world: &World) -> Vec<BallotLine> {
    let c = &world.commons;
    let mut out = Vec::new();
    for r in &content.social.runners {
        if r.withdrawn_by.iter().any(|e| world.has(e)) {
            continue;
        }
        let id = format!("npc:{}", r.npc);
        let swing: i32 = r
            .swing
            .iter()
            .filter(|(e, _)| world.has(e))
            .map(|(_, n)| *n)
            .sum();
        let talk: i32 = c
            .rumours_about(&r.npc)
            .filter_map(|u| content.rumour(&u.id))
            .map(|u| u.backing)
            .sum();
        let campaigning = (world.last_day - world.term_start - content.social.rules.nominate_from)
            .clamp(
                0,
                content.social.rules.tally_day - content.social.rules.nominate_from,
            );
        let grown = r.grow * i32::try_from(campaigning).unwrap_or(0);
        // The school's mood: each term it favours someone a little, in a fixed rotation, so the
        // same character is not the favourite every term.
        let slot = i32::try_from(
            content
                .social
                .runners
                .iter()
                .position(|x| x.npc == r.npc)
                .unwrap_or(0),
        )
        .unwrap_or(0);
        let mood = (i32::try_from(world.term % 3).unwrap_or(0) + 2 * slot) % 3 * 2 - 2;
        out.push(BallotLine {
            name: content
                .npc(&r.npc)
                .map_or_else(|| r.npc.clone(), |n| n.name.clone()),
            platform: r.platform.clone(),
            backing: r.base + grown + mood + swing + talk,
            votes: c.votes_for(&id),
            player: false,
            id,
        });
    }
    for cand in &c.election.candidates {
        out.push(BallotLine {
            id: cand.who.clone(),
            name: cand.who.clone(),
            platform: "A pupil who thinks they could do better.".to_owned(),
            backing: i32::from(cand.backing),
            votes: c.votes_for(&cand.who),
            player: true,
        });
    }
    out
}

/// Count the votes: the highest total wins; a tie goes to the character who runs, then by name.
#[must_use]
pub fn tally(content: &Content, world: &World) -> Option<BallotLine> {
    let weight = content.social.rules.vote_weight;
    let mut lines = ballot_of(content, world);
    lines.sort_by(|a, b| {
        let sa = a.backing + weight * i32::try_from(a.votes).unwrap_or(0);
        let sb = b.backing + weight * i32::try_from(b.votes).unwrap_or(0);
        sb.cmp(&sa)
            .then_with(|| a.player.cmp(&b.player))
            .then_with(|| a.id.cmp(&b.id))
    });
    lines.into_iter().next()
}

/// Score a pupil for an award.
fn award_score(stat: &str, p: &Pupil) -> i32 {
    match stat {
        "legend" => i32::try_from(p.legend).unwrap_or(i32::MAX),
        "grades" => i32::from(p.grades),
        "pulled" => i32::from(p.pulled),
        "clues" => i32::from(p.clues),
        "merit" => i32::from(p.merit),
        "friends" => i32::from(p.friends),
        // A clean getaway: schemes pulled off, with the file against them.
        "clean" => i32::from(p.pulled) * 10 - i32::from(p.file) * 6,
        _ => 0,
    }
}

/// Hand out the term's awards from the roster.
pub fn give_awards(world: &mut World, content: &Content) {
    let term = world.term;
    let mut wins = Vec::new();
    for spec in &content.social.awards {
        let best = world
            .commons
            .roster
            .iter()
            .map(|p| (award_score(&spec.stat, p), p))
            .filter(|(s, _)| *s >= spec.min)
            .min_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        if let Some((_, p)) = best {
            wins.push(AwardWin {
                term,
                award: spec.id.clone(),
                name: p.name.clone(),
            });
        }
    }
    for w in wins {
        let day = world.last_day;
        if let Some(spec) = content.social.awards.iter().find(|a| a.id == w.award) {
            world.headline(day, format!("{}: {}. {}", spec.name, w.name, spec.text));
        }
        world.commons.awards.push(w);
    }
    let over = world
        .commons
        .awards
        .len()
        .saturating_sub(crate::commons::AWARD_CAP);
    world.commons.awards.drain(..over);
}

/// Count the trust cast on the east wing and bring about the winning ending for everyone.
pub fn finale(world: &mut World, content: &Content, day: i64) {
    if world.commons.finale.is_some() {
        return;
    }
    let mut tally: Vec<(&str, usize)> = Vec::new();
    for v in &world.commons.verdicts {
        match tally.iter_mut().find(|(id, _)| *id == v.trust) {
            Some(t) => t.1 += 1,
            None => tally.push((v.trust.as_str(), 1)),
        }
    }
    let order = |id: &str| content.verdict(id).map_or(i32::MAX, |v| v.order);
    tally.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| order(a.0).cmp(&order(b.0))));
    let Some((win, _)) = tally.first().copied() else {
        world.commons.finale = Some(String::new());
        world.headline(day, content.mystery.rules.unsolved.clone());
        return;
    };
    let by = world
        .commons
        .verdicts
        .iter()
        .find(|v| v.trust == win)
        .map_or_else(String::new, |v| v.by.clone());
    let win = win.to_owned();
    if let Some(spec) = content.verdict(&win) {
        WorldDelta::Effect {
            id: spec.effect.clone(),
            by: by.clone(),
            day,
        }
        .apply(world, content);
        if let Some(effect) = content.effect(&spec.effect) {
            world.headline(day, effect.headline.replace("{player}", &by));
        }
    }
    world.commons.finale = Some(win);
}

/// Count the election and bring about the winner's change to the school.
pub fn count_election(world: &mut World, content: &Content, day: i64) {
    if world.commons.election.winner.is_some() {
        return;
    }
    let Some(win) = tally(content, world) else {
        return;
    };
    let (effect, by) = if let Some(npc) = win.id.strip_prefix("npc:") {
        (
            content
                .social
                .runners
                .iter()
                .find(|r| r.npc == npc)
                .map(|r| r.win.clone()),
            win.name.clone(),
        )
    } else {
        (
            Some(content.social.rules.player_win_effect.clone()),
            win.id.clone(),
        )
    };
    if let Some(effect) = effect {
        WorldDelta::Effect {
            id: effect.clone(),
            by: by.clone(),
            day,
        }
        .apply(world, content);
        if let Some(e) = content.effect(&effect) {
            world.headline(day, e.headline.replace("{player}", &by));
        }
    }
    world.commons.election.winner = Some(win.id);
}

/// Clear what belongs to one term, keeping who plays and what was won.
pub fn new_term(c: &mut Commons) {
    c.rumours.clear();
    c.plots.clear();
    c.election = crate::commons::Election::default();
    c.verdicts.clear();
    c.finale = None;
    c.clues = 0;
    for p in &mut c.roster {
        p.legend = 0;
        p.pulled = 0;
        p.clues = 0;
        p.merit = 0;
        p.file = 0;
        p.grades = 0;
        p.friends = 0;
    }
}

impl Game {
    /// Whether an action names a choice that depends on the world's roster (for the screen).
    #[must_use]
    pub fn is_social(action: &Action) -> bool {
        matches!(
            action,
            Action::Plot { .. }
                | Action::Spread { .. }
                | Action::Nominate
                | Action::Canvass
                | Action::Vote(_)
                | Action::OptOut(_)
                | Action::Search
        )
    }
}

/// A short, honest word on one's own opt-out for the screens.
#[must_use]
pub fn opt_word(out: bool) -> &'static str {
    if out { "opted out" } else { "open to plots" }
}
