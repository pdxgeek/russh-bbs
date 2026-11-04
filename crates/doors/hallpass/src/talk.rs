//! Conversations, effects, friendships and storylines.
//!
//! A conversation is a small tree in `talks.toml`: a character, conditions on the pupil (stats,
//! skills, traits, standing, the file, bonds, club rank, what is going on in the world), and
//! choices that are themselves open or shut by conditions. A choice does its effects: standing,
//! friendship, grades, the file, a flag, a step of a storyline, a change to the school everyone
//! shares. Friendships grow by doing (talking, helping, working together) and a betrayal closes
//! the door on a helper for the term. Storylines are chains of conversations gated by the step
//! the story is at, so a choice that moves the story on shuts the doors of the other branch.

use russh_bbs_door::DoorRng;

use crate::content::{Choice, Talk};
use crate::effect::{Eff, Gauge};
use crate::game::{Event, Game, Refused, Spent};
use crate::state::{ArcProgress, Scene, map_get, map_set, to_i16, to_u8};
use crate::world::WorldDelta;

/// One answer in a conversation, as the pupil sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceView {
    /// What it says.
    pub label: String,
    /// Whether the pupil can pick it.
    pub open: bool,
    /// For a shut choice, what it wants.
    pub wants: String,
}

/// A conversation now under way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneView {
    /// The menu line of the conversation.
    pub title: String,
    /// Who it is with.
    pub who: String,
    /// What they say.
    pub text: String,
    /// The answers.
    pub choices: Vec<ChoiceView>,
}

/// How a character stands with the pupil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regard {
    /// Closed to the pupil after a betrayal.
    Betrayed,
    /// A rival.
    Rival,
    /// Neither one thing nor the other.
    Neutral,
    /// A friend.
    Friend,
}

impl Regard {
    /// A word for it.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Betrayed => "will not speak to you",
            Self::Rival => "rival",
            Self::Neutral => "",
            Self::Friend => "friend",
        }
    }
}

impl Game {
    /// Friendship with a character.
    #[must_use]
    pub fn bond(&self, npc: &str) -> i32 {
        self.player.life.bond_with(npc)
    }

    /// How a character regards the pupil.
    #[must_use]
    pub fn regard(&self, npc: &str) -> Regard {
        let rules = &self.content.rules;
        let bond = self.bond(npc);
        if bond <= rules.betray_bond {
            Regard::Betrayed
        } else if bond <= rules.rival_bond {
            Regard::Rival
        } else if bond >= rules.friend_bond {
            Regard::Friend
        } else {
            Regard::Neutral
        }
    }

    /// Change a friendship, and say so if it crossed a line.
    pub(crate) fn change_bond(&mut self, npc: &str, delta: i32) -> Vec<Event> {
        let before = self.regard(npc);
        self.player.life.add_bond(npc, delta);
        let after = self.regard(npc);
        let Some(who) = self.content.npc(npc).map(|n| n.name.clone()) else {
            return Vec::new();
        };
        match (before, after) {
            (a, b) if a == b => Vec::new(),
            (_, Regard::Friend) => vec![Event::Say(format!("{who} counts you a friend now."))],
            (_, Regard::Rival) => vec![Event::Say(format!("{who} has made you a rival."))],
            _ => Vec::new(),
        }
    }

    /// Whether a conversation can be started now.
    #[must_use]
    pub fn talk_open(&self, talk: &Talk) -> bool {
        let Some(slot) = self.current_slot() else {
            return false;
        };
        let p = &self.player;
        if p.run.is_some() || p.life.lesson.is_some() || p.life.scene.is_some() {
            return false;
        }
        if !self
            .npcs_at(&p.place, p.day, slot)
            .iter()
            .any(|n| n.id == talk.npc)
        {
            return false;
        }
        if !self.all_hold(&talk.when) {
            return false;
        }
        // Someone you betrayed will only hear you out about making it up.
        if self.regard(&talk.npc) == Regard::Betrayed && !talk.when.iter().any(is_amends) {
            return false;
        }
        match map_get(&p.life.talked, &talk.id) {
            Some(_) if talk.once => return false,
            Some(day) if p.day - day < talk.cooldown => return false,
            _ => {}
        }
        if let (Some(arc), Some(step)) = (&talk.arc, &talk.step) {
            match p.life.arc(arc) {
                Some(a) if a.ended.is_none() && a.step == *step && p.day - a.since >= talk.wait => {}
                _ => return false,
            }
        }
        true
    }

    /// Conversations that could be started here and now.
    #[must_use]
    pub fn talks_here(&self) -> Vec<&Talk> {
        self.content
            .talks
            .iter()
            .filter(|t| self.talk_open(t))
            .collect()
    }

    fn choice_open(&self, choice: &Choice) -> bool {
        self.all_hold(&choice.needs)
    }

    /// The conversation now under way.
    #[must_use]
    pub fn scene_view(&self) -> Option<SceneView> {
        let scene = self.player.life.scene.as_ref()?;
        let talk = self.content.talk(&scene.talk)?;
        let node = talk.node.iter().find(|n| n.id == scene.node)?;
        let who = self
            .content
            .npc(&talk.npc)
            .map_or_else(|| talk.npc.clone(), |n| n.name.clone());
        Some(SceneView {
            title: talk.title.clone(),
            who,
            text: node.text.clone(),
            choices: node
                .choice
                .iter()
                .map(|c| ChoiceView {
                    label: c.label.clone(),
                    open: self.choice_open(c),
                    wants: c
                        .needs
                        .iter()
                        .filter(|n| !self.holds(n))
                        .map(crate::cond::Cond::describe)
                        .collect::<Vec<_>>()
                        .join(", "),
                })
                .collect(),
        })
    }

    pub(crate) fn start_talk(&mut self, id: &str) -> Result<Vec<Event>, Refused> {
        let talk = self
            .content
            .talk(id)
            .cloned()
            .ok_or_else(|| Refused::new("There is no such conversation."))?;
        if !self.talk_open(&talk) {
            return Err(Refused::new("That is not something to bring up now."));
        }
        self.player.life.scene = Some(Scene {
            talk: talk.id.clone(),
            node: talk.first.clone(),
            chosen: 0,
        });
        Ok(Vec::new())
    }

    pub(crate) fn choose(&mut self, index: u8, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let scene = self
            .player
            .life
            .scene
            .clone()
            .ok_or_else(|| Refused::new("You are not in a conversation."))?;
        let talk = self
            .content
            .talk(&scene.talk)
            .cloned()
            .ok_or_else(|| Refused::new("That conversation is gone."))?;
        let node = talk
            .node
            .iter()
            .find(|n| n.id == scene.node)
            .ok_or_else(|| Refused::new("That conversation is gone."))?;
        let choice = node
            .choice
            .get(usize::from(index))
            .cloned()
            .ok_or_else(|| Refused::new("No such answer."))?;
        if !self.choice_open(&choice) {
            return Err(Refused::new("You cannot say that to them yet."));
        }
        let day = self.player.day;
        map_set(&mut self.player.life.talked, &talk.id, day);
        let mut events = vec![Event::Say(choice.say.clone())];
        events.extend(self.apply_effects(&choice.effects, 0));
        let next = (!choice.next.is_empty()).then(|| choice.next.clone());
        match next {
            Some(node) if self.player.status == crate::state::Status::Active => {
                self.player.life.scene = Some(Scene {
                    talk: talk.id.clone(),
                    node,
                    chosen: scene.chosen.saturating_add(1),
                });
            }
            _ => {
                self.player.life.scene = None;
                if self.player.status == crate::state::Status::Active {
                    events.extend(self.end_slot(Spent::Other, rng));
                }
            }
        }
        Ok(events)
    }

    /// Step out of the lesson or conversation in progress.
    pub(crate) fn abandon(&mut self, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        if self.player.life.lesson.is_some() {
            return Ok(self.walk_out(rng));
        }
        let Some(scene) = self.player.life.scene.take() else {
            return Err(Refused::new("There is nothing to leave."));
        };
        if scene.chosen == 0 {
            return Ok(vec![Event::Say("You think better of it.".into())]);
        }
        let mut events = vec![Event::Say("You excuse yourself and go.".into())];
        events.extend(self.end_slot(Spent::Other, rng));
        Ok(events)
    }

    // ------------------------------------------------------------ effects

    /// Carry out effects. `depth` stops an ending from ending another story forever.
    pub(crate) fn apply_effects(&mut self, effects: &[Eff], depth: u8) -> Vec<Event> {
        let mut events = Vec::new();
        for effect in effects {
            events.extend(self.apply_effect(effect, depth));
        }
        events
    }

    fn apply_effect(&mut self, effect: &Eff, depth: u8) -> Vec<Event> {
        let day = self.player.day;
        match effect {
            Eff::Stand(clique, n) => {
                self.player.add_standing(clique, *n);
                Vec::new()
            }
            Eff::Bond(npc, n) => self.change_bond(npc, *n),
            Eff::Stat(gauge, n) => {
                let p = &mut self.player;
                match gauge {
                    Gauge::Cool => p.cool = to_i16(i32::from(p.cool) + n),
                    Gauge::Nerve => {
                        p.nerve = to_u8(i32::from(p.nerve) + n).min(p.nerve_max);
                    }
                    Gauge::Cash => p.cash = p.cash.saturating_add(*n),
                    Gauge::Notoriety => p.notoriety = to_u8(i32::from(p.notoriety) + n),
                    Gauge::Heat => p.heat = to_u8(i32::from(p.heat) + n),
                    Gauge::Legend => {
                        let _ = self.legend_gain(i64::from(*n));
                    }
                }
                Vec::new()
            }
            Eff::Know(k) => {
                self.player.learn(k);
                Vec::new()
            }
            Eff::Forget(k) => {
                self.player.knowledge.retain(|x| x != k);
                Vec::new()
            }
            Eff::File(kind, sev) => self.add_file(kind, *sev),
            Eff::Wary(npc, n) => {
                self.bump_wary(npc, *n);
                Vec::new()
            }
            Eff::Item(item) => {
                let have = self.player.count(item);
                map_set(&mut self.player.inventory, item, have.saturating_add(1).min(3));
                Vec::new()
            }
            Eff::Xp(skill, n) => self
                .gain_xp(skill, *n)
                .map(|s| vec![Event::Say(format!("Your {s} has improved."))])
                .unwrap_or_default(),
            Eff::Grade(subject, n) => {
                let now = i32::from(map_get(&self.player.grades, subject).copied().unwrap_or(0));
                map_set(&mut self.player.grades, subject, to_u8((now + n).min(100)));
                Vec::new()
            }
            Eff::Merit(club, n) => self.add_merit(club, *n),
            Eff::Trait(id) => {
                if self.player.traits.contains(id) {
                    Vec::new()
                } else {
                    self.player.traits.push(id.clone());
                    vec![Event::TraitEarned(id.clone())]
                }
            }
            Eff::Arc(arc, step) => {
                if let Some(a) = self.player.life.arcs.iter_mut().find(|a| a.id == *arc) {
                    if a.ended.is_none() {
                        a.step.clone_from(step);
                        a.since = day;
                    }
                }
                Vec::new()
            }
            Eff::ArcEnd(arc, ending) => self.end_arc(arc, ending, depth),
            Eff::World(effect) => self.world_change(effect),
            Eff::Betray(who) => self.betray(who),
        }
    }

    fn end_arc(&mut self, arc: &str, ending: &str, depth: u8) -> Vec<Event> {
        let Some(spec) = self.content.arc(arc).cloned() else {
            return Vec::new();
        };
        let Some(end) = spec.ending.iter().find(|e| e.id == ending).cloned() else {
            return Vec::new();
        };
        let Some(progress) = self.player.life.arcs.iter_mut().find(|a| a.id == arc) else {
            return Vec::new();
        };
        if progress.ended.is_some() {
            return Vec::new();
        }
        progress.ended = Some(ending.to_owned());
        let mut events = vec![Event::Say(format!("{}: {}", end.name, end.text))];
        if depth < 3 {
            events.extend(self.apply_effects(&end.effects, depth + 1));
        }
        events
    }

    fn world_change(&mut self, effect_id: &str) -> Vec<Event> {
        let Some(effect) = self.content.effect(effect_id).cloned() else {
            return Vec::new();
        };
        let day = self.player.day;
        let name = self.player.name.clone();
        self.player.big = self.player.big.saturating_add(1);
        self.push_world(WorldDelta::Effect {
            id: effect_id.to_owned(),
            by: name.clone(),
            day,
        });
        self.push_world(WorldDelta::Headline {
            day,
            text: effect.headline.replace("{player}", &name),
        });
        let legend = self.player.legend;
        self.push_world(WorldDelta::Hall { name, legend });
        vec![Event::WorldChange(effect_id.to_owned())]
    }

    fn betray(&mut self, who: &str) -> Vec<Event> {
        let npc_id = if who == "last_ally" {
            self.player
                .life
                .last_allies
                .iter()
                .find(|id| {
                    self.content
                        .npc(id)
                        .is_some_and(|n| n.clique.as_deref() != Some("strays"))
                })
                .cloned()
        } else {
            Some(who.to_owned())
        };
        let Some(npc) = npc_id.and_then(|id| self.content.npc(&id).cloned()) else {
            return vec![Event::Say("There is nobody to give up.".into())];
        };
        let to = self.content.rules.betray_to;
        let now = self.bond(&npc.id);
        self.player.life.add_bond(&npc.id, to.min(now) - now);
        self.player.learn(&format!("betrayed_{}", npc.id));
        if let Some(c) = &npc.clique {
            self.player.add_standing(c, -8);
        }
        self.player.cool = to_i16(i32::from(self.player.cool) - 3);
        vec![Event::Say(format!(
            "You give {} up. They will not forget it, and nor will their friends.",
            npc.name
        ))]
    }

    // ------------------------------------------------------------ helpers remember

    /// After a scheme: the helpers who were in it think better or worse of you.
    pub(crate) fn allies_remember(&mut self, allies: &[(String, String)], busted: bool) -> Vec<Event> {
        let mut events = Vec::new();
        self.player.life.last_allies = allies.iter().map(|(n, _)| n.clone()).collect();
        for (npc, _) in allies {
            events.extend(self.change_bond(npc, if busted { -3 } else { 2 }));
        }
        events
    }

    /// Characters near enough to the deed to see it who have made you a rival.
    #[must_use]
    pub fn rivals_at(&self, place: &str, day: i64, slot: usize) -> i32 {
        let rival = self.content.rules.rival_bond;
        i32::try_from(
            self.npcs_at(place, day, slot)
                .iter()
                .filter(|n| !n.is_authority() && self.bond(&n.id) <= rival)
                .count(),
        )
        .unwrap_or(0)
    }

    // ------------------------------------------------------------ storylines

    /// Start any storyline whose time has come.
    pub(crate) fn check_arcs(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        if self.player.status != crate::state::Status::Active {
            return events;
        }
        let starting: Vec<(String, String, String, String)> = self
            .content
            .arcs
            .iter()
            .filter(|a| self.player.life.arc(&a.id).is_none() && self.all_hold(&a.start))
            .map(|a| (a.id.clone(), a.first.clone(), a.name.clone(), a.blurb.clone()))
            .collect();
        for (id, first, name, blurb) in starting {
            if self.player.life.arcs.len() >= 12 {
                break;
            }
            let since = self.player.day;
            self.player.life.arcs.push(ArcProgress {
                id,
                step: first,
                since,
                ended: None,
            });
            events.push(Event::Say(format!("A story begins - {name}: {blurb}")));
        }
        events
    }

    /// The journal lines for the storylines begun: name and where each stands.
    #[must_use]
    pub fn journal(&self) -> Vec<String> {
        self.player
            .life
            .arcs
            .iter()
            .filter_map(|p| {
                let spec = self.content.arc(&p.id)?;
                let line = match &p.ended {
                    Some(end) => spec
                        .ending
                        .iter()
                        .find(|e| e.id == *end)
                        .map_or_else(String::new, |e| format!("over: {}", e.name)),
                    None => spec
                        .step
                        .iter()
                        .find(|s| s.id == p.step)
                        .map_or_else(String::new, |s| s.text.clone()),
                };
                Some(format!("{} - {line}", spec.name))
            })
            .collect()
    }
}

fn is_amends(cond: &crate::cond::Cond) -> bool {
    use crate::cond::{Cond, Op, Stat};
    matches!(
        cond,
        Cond::Cmp {
            stat: Stat::Bond(_),
            op: Op::Le | Op::Lt,
            ..
        }
    )
}
