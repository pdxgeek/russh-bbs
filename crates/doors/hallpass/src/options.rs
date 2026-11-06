//! What the character can do right now, for the screen and for bots.
//!
//! The list is built by proposing every action that could make sense and keeping the ones the
//! engine accepts when tried on a copy of the game, so there is exactly one place where the rules
//! live and the list can never offer something `apply` would refuse.

use russh_bbs_door::DoorRng;

use crate::game::{Action, Game};
use crate::state::Status;

impl Game {
    /// Whether `apply` would accept `action` now (tried on a copy; nothing changes).
    #[must_use]
    pub fn can(&self, action: &Action) -> bool {
        let mut copy = self.clone();
        copy.apply(action, &mut DoorRng::from_seed(0)).is_ok()
    }

    /// Every action the engine would accept now.
    #[must_use]
    pub fn legal_actions(&self) -> Vec<Action> {
        let mut proposals: Vec<Action> = Vec::new();
        if self.player.status == Status::New {
            proposals.extend(
                self.content
                    .traits
                    .iter()
                    .filter(|t| t.start)
                    .map(|t| Action::Enrol(t.id.clone())),
            );
        }
        if self.player.run.is_some() {
            proposals.extend((0..=3).map(|push| Action::RunPhase { push }));
        } else if self.player.life.lesson.is_some() {
            proposals.extend((0..4).map(Action::Answer));
            proposals.push(Action::Abandon);
        } else if self.player.life.scene.is_some() {
            proposals.extend((0..8).map(Action::Choose));
            proposals.push(Action::Abandon);
        } else {
            proposals.extend(
                self.talks_here()
                    .into_iter()
                    .map(|t| Action::Talk(t.id.clone())),
            );
            for club in &self.content.clubs {
                proposals.push(Action::Join(club.id.clone()));
                proposals.push(Action::Quit(club.id.clone()));
            }
            proposals.push(Action::Meet);
            proposals.push(Action::Search);
            proposals.push(Action::Nominate);
            proposals.push(Action::Canvass);
            proposals.push(Action::OptOut(!self.player.social.opt_out));
            for line in self.ballot() {
                proposals.push(Action::Vote(line.id));
            }
            for spec in &self.content.social.plots {
                for mate in self.classmates() {
                    if self.plot_block(&spec.id, &mate.name).is_none() {
                        proposals.push(Action::Plot {
                            kind: spec.id.clone(),
                            to: mate.name,
                        });
                    }
                }
            }
            for spec in &self.content.social.rumours {
                for about in self.content.rumour_subjects(spec) {
                    if self.rumour_block(&spec.id, &about).is_none() {
                        proposals.push(Action::Spread {
                            rumour: spec.id.clone(),
                            about,
                        });
                    }
                }
            }
            proposals.extend(self.reachable().into_iter().map(Action::Go));
            proposals.push(Action::Attend);
            proposals.extend(
                self.content
                    .subjects
                    .iter()
                    .map(|s| Action::Study(s.id.clone())),
            );
            proposals.push(Action::Loiter);
            proposals.push(Action::Rest);
            proposals.push(Action::Sleep);
            proposals.extend(self.content.items.iter().map(|i| Action::Buy(i.id.clone())));
            for (target, method) in self.drafts() {
                proposals.push(Action::Draft { target, method });
            }
            for scheme in &self.player.schemes {
                let id = scheme.id;
                proposals.push(Action::Drop(id));
                proposals.push(Action::Scout(id));
                proposals.push(Action::BeginRun(id));
                proposals.extend(
                    self.content
                        .twists
                        .iter()
                        .map(|t| Action::AddTwist(id, t.id.clone())),
                );
                proposals.extend(
                    self.content
                        .getaways
                        .iter()
                        .map(|g| Action::SetGetaway(id, g.id.clone())),
                );
                proposals.extend(self.candidate_times(scheme).into_iter().map(|(day, slot)| {
                    Action::Schedule {
                        scheme: id,
                        day,
                        slot,
                    }
                }));
                proposals.extend(self.recruitable(scheme).into_iter().map(|(npc, role, _)| {
                    Action::Recruit {
                        scheme: id,
                        npc,
                        role,
                    }
                }));
            }
        }
        proposals.retain(|action| self.can(action));
        proposals
    }
}
