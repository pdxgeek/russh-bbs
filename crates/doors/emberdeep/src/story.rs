//! The chain of questlines and the Furnace's verdict (M4-E4), on the hero. Goals read the hero's
//! own tallies (kills by monster, new rooms by kind, guardians, contracts, gifts, level, rescues,
//! duels), so a step can already be done when it is first read and the chain catches up in one go.
//! A step with a choice waits for it: the choice sets a flag for good, and flags open and close
//! room options and the verdicts the slayer of the Cold Furnace may give.

use crate::content::{Content, Gain, RoomKind};
use crate::content_story::{Goal, Quest, Step};
use crate::endings;
use crate::game::{Event, Game, Refused, Tone, say};
use crate::social::SocialChange;
use crate::world::Change;

impl Game {
    /// A monster of `id` was killed.
    pub(crate) fn note_kill(&mut self, id: &str) {
        if let Some(at) = self.hero.story.slain.iter().position(|(m, _)| m == id) {
            let slot = &mut self.hero.story.slain[at];
            slot.1 = slot.1.saturating_add(1);
        } else if self.hero.story.slain.len() < 60 {
            self.hero.story.slain.push((id.to_owned(), 1));
        }
    }

    /// The hero walked into a room never entered before this epoch.
    pub(crate) fn note_new_room(&mut self, kind: RoomKind) {
        let slot = &mut self.hero.story.visits[kind.index()];
        *slot = slot.saturating_add(1);
    }

    fn goal_met(&self, goal: &Goal) -> bool {
        let h = &self.hero;
        match goal {
            Goal::Reach { depth } => h.best_depth >= *depth,
            Goal::Slay { monster, count } => {
                h.story
                    .slain
                    .iter()
                    .find(|(m, _)| m == monster)
                    .map_or(0, |(_, n)| *n)
                    >= *count
            }
            Goal::Visit { room, count } => h.story.visits[room.index()] >= *count,
            Goal::Guardian { band } => h.story.guardians & (1 << (band.saturating_sub(1))) != 0,
            Goal::Contracts { count } => h.contracts_done >= *count,
            Goal::Give { coin } => h.given.iter().map(|(_, c, _)| *c).sum::<u32>() >= *coin,
            Goal::Level { level } => h.level >= *level,
            Goal::Rescues { count } => h.social.rescues >= *count,
            Goal::Duels { wins } => h.social.wins >= *wins,
        }
    }

    /// Steps done in `quest` (0 if the hero has not begun it).
    pub(crate) fn quest_done(&self, quest: &str) -> usize {
        self.hero
            .story
            .quests
            .iter()
            .find(|(q, _)| q == quest)
            .map_or(0, |(_, n)| usize::from(*n))
    }

    /// Whether the hero has begun the line (reached its level).
    pub(crate) fn quest_open(&self, q: &Quest) -> bool {
        self.hero.level >= q.level
    }

    /// The step `quest` is on, if the hero has begun it and it is not finished.
    pub(crate) fn quest_step<'a>(&self, q: &'a Quest) -> Option<&'a Step> {
        self.quest_open(q)
            .then(|| q.step.get(self.quest_done(&q.id)))
            .flatten()
    }

    /// Whether the step the hero is on has its goal met and waits on a choice.
    pub(crate) fn quest_waiting<'a>(&self, q: &'a Quest) -> Option<&'a Step> {
        self.quest_step(q)
            .filter(|s| !s.choose.is_empty() && self.goal_met(&s.goal))
    }

    fn set_done(&mut self, quest: &str, done: usize) {
        let done = u8::try_from(done).unwrap_or(u8::MAX);
        match self.hero.story.quests.iter_mut().find(|(q, _)| q == quest) {
            Some(slot) => slot.1 = done,
            None => self.hero.story.quests.push((quest.to_owned(), done)),
        }
    }

    fn pay(&mut self, content: &Content, reward: &[Gain], ev: &mut Vec<Event>) {
        for g in reward {
            match g {
                Gain::Coin { amount } => {
                    self.hero.coin = self.hero.coin.saturating_add(*amount);
                    say(ev, Tone::Good, format!("+{amount} coin"));
                }
                Gain::Salvage { amount } => {
                    self.hero.stash = self.hero.stash.saturating_add(*amount);
                    say(ev, Tone::Good, format!("+{amount} salvage (to the store)"));
                }
                Gain::Xp { amount } => self.gain_xp(*amount, ev),
                Gain::Relic => {
                    self.hero.relics = self.hero.relics.saturating_add(1);
                    say(ev, Tone::Good, "+1 relic");
                }
                Gain::Recipe { id } => self.find_recipe(content, id, ev),
                Gain::Core { id } => self.find_core(content, id, ev),
                _ => {}
            }
        }
    }

    /// Carry every line as far as its tallies allow. Called after every action.
    pub(crate) fn story_advance(&mut self, content: &Content, ev: &mut Vec<Event>) {
        for q in &content.quests.quest {
            if !self.quest_open(q) {
                continue;
            }
            let mut done = self.quest_done(&q.id);
            while let Some(step) = q.step.get(done) {
                if !step.choose.is_empty() || !self.goal_met(&step.goal) {
                    break;
                }
                say(ev, Tone::Good, format!("{}: {}", q.name, step.done_text));
                self.pay(content, &step.reward, ev);
                done += 1;
                self.set_done(&q.id, done);
            }
            if done == self.quest_done(&q.id)
                && !self.hero.story.quests.iter().any(|(id, _)| *id == q.id)
            {
                self.set_done(&q.id, 0);
            }
        }
    }

    /// Make the choice that closes a step.
    pub(crate) fn choose(
        &mut self,
        content: &Content,
        quest: &str,
        choice: &str,
        ev: &mut Vec<Event>,
    ) -> Result<(), Refused> {
        let q = content
            .quests
            .quest
            .iter()
            .find(|q| q.id == quest)
            .ok_or_else(|| Refused::new("There is no such questline."))?;
        let step = self
            .quest_waiting(q)
            .ok_or_else(|| Refused::new("Nothing waits on a choice there."))?;
        let c = step
            .choose
            .iter()
            .find(|c| c.id == choice)
            .ok_or_else(|| Refused::new("That is not one of the choices."))?;
        if !self.hero.story.flags.contains(&c.flag) && self.hero.story.flags.len() < 60 {
            self.hero.story.flags.push(c.flag.clone());
        }
        say(ev, Tone::Good, c.text.clone());
        say(ev, Tone::Good, format!("{}: {}", q.name, step.done_text));
        let reward = step.reward.clone();
        self.pay(content, &reward, ev);
        let done = self.quest_done(&q.id) + 1;
        self.set_done(&q.id, done);
        self.story_advance(content, ev);
        Ok(())
    }

    /// The verdicts open to this hero.
    pub(crate) fn verdicts(&self) -> Vec<&'static endings::Verdict> {
        endings::VERDICTS
            .iter()
            .filter(|v| {
                v.needs_flag
                    .is_none_or(|f| self.hero.story.flags.iter().any(|x| x == f))
            })
            .collect()
    }

    /// Give the verdict on the Cold Furnace (the slayer's, once).
    pub(crate) fn give_verdict(&mut self, id: &str, ev: &mut Vec<Event>) -> Result<(), Refused> {
        if !self.hero.story.verdict_due {
            return Err(Refused::new("No one asks you for a verdict."));
        }
        let v = self
            .verdicts()
            .into_iter()
            .find(|v| v.id == id)
            .ok_or_else(|| Refused::new("That is not open to you."))?;
        self.hero.story.verdict_due = false;
        say(ev, Tone::Good, v.text);
        let by = self.hero.name.clone();
        self.emit(Change::Social(SocialChange::Verdict {
            kind: v.id.to_owned(),
            by,
        }));
        Ok(())
    }
}
