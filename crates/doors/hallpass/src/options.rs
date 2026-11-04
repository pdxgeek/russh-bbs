//! What the character can do right now, for the screen and for bots.
//!
//! The list is built by proposing every action that could make sense and keeping the ones the
//! engine accepts when tried on a copy of the game, so there is exactly one place where the rules
//! live and the list can never offer something `apply` would refuse.

use russh_bbs_door::DoorRng;

use crate::game::{Action, Game};
use crate::state::Status;

impl Game {
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
        } else {
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
        proposals.retain(|action| {
            let mut copy = self.clone();
            let mut rng = DoorRng::from_seed(0);
            copy.apply(action, &mut rng).is_ok()
        });
        proposals
    }
}
