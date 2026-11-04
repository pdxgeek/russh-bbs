//! Class minigames.
//!
//! Each subject uses one short puzzle kind (`lessons.toml`), a few rounds per lesson. A round is a
//! pure function of the lesson's stored seed, the round number and the level, so a lesson can be
//! left and resumed from the save and a replay log reproduces it. How hard it is depends on the
//! subject grade; how many wrong answers are struck out for the pupil depends on the skill the
//! subject trains, so two builds in the same classroom see different options. The result feeds the
//! grade, the skill and what the teacher thinks of the pupil.

use russh_bbs_door::DoorRng;

use crate::content::Content;
use crate::game::{Event, Game, Refused, Spent};
use crate::state::{Lesson, map_get, map_set, to_u8};

/// One puzzle: what is asked, the answers on offer, and which one is right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Round {
    /// The lines of the question.
    pub prompt: Vec<String>,
    /// The answers, in the order shown.
    pub options: Vec<String>,
    /// The index of the right one.
    pub answer: usize,
}

/// A round as the pupil sees it: struck-out answers cannot be chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundView {
    /// The subject.
    pub subject: String,
    /// The round now up, from 0.
    pub number: u8,
    /// How many rounds the lesson has.
    pub rounds: usize,
    /// The question.
    pub round: Round,
    /// For each answer, whether it can still be chosen.
    pub open: Vec<bool>,
}

fn mix(seed: u64, round: u8) -> DoorRng {
    DoorRng::from_seed(seed ^ (u64::from(round) + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

fn pick(rng: &mut DoorRng, n: usize) -> usize {
    usize::try_from(rng.below(u32::try_from(n.max(1)).unwrap_or(1))).unwrap_or(0)
}

fn between(rng: &mut DoorRng, low: i32, high: i32) -> i32 {
    let span = u32::try_from((high - low + 1).max(1)).unwrap_or(1);
    low + i32::try_from(rng.below(span)).unwrap_or(0)
}

/// Shuffle `options` and return where `right` landed.
fn shuffled(rng: &mut DoorRng, mut options: Vec<String>, right: usize) -> (Vec<String>, usize) {
    let mut at = right;
    for i in (1..options.len()).rev() {
        let j = pick(rng, i + 1);
        options.swap(i, j);
        if at == i {
            at = j;
        } else if at == j {
            at = i;
        }
    }
    (options, at)
}

fn sequence(rng: &mut DoorRng, level: u8) -> Round {
    let (terms, next) = match level {
        0 => {
            let (a, d) = (between(rng, 1, 20), between(rng, 2, 9));
            ((0..4).map(|i| a + d * i).collect::<Vec<_>>(), a + d * 4)
        }
        1 if rng.chance(1, 2) => {
            let (a, m) = (between(rng, 1, 4), between(rng, 2, 3));
            let mut v = vec![a];
            for _ in 0..4 {
                v.push(v[v.len() - 1] * m);
            }
            let last = v.pop().unwrap_or(0);
            (v, last)
        }
        1 => {
            let (a, x, y) = (between(rng, 1, 15), between(rng, 2, 6), between(rng, 7, 12));
            let mut v = vec![a];
            for i in 0..4 {
                v.push(v[v.len() - 1] + if i % 2 == 0 { x } else { y });
            }
            let last = v.pop().unwrap_or(0);
            (v, last)
        }
        _ => {
            let (a, d, k) = (between(rng, 1, 10), between(rng, 1, 5), between(rng, 1, 3));
            let mut v = vec![a];
            for i in 0..5 {
                v.push(v[v.len() - 1] + d + k * i);
            }
            let last = v.pop().unwrap_or(0);
            (v, last)
        }
    };
    let shown = terms
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let want = 3 + usize::from(level == 2);
    let mut options = vec![next.to_string()];
    let mut spread = 1;
    while options.len() < want {
        for delta in [1, -1, 2, -2] {
            let wrong = (next + delta * spread).to_string();
            if !options.contains(&wrong) && options.len() < want && pick(rng, 3) != 0 {
                options.push(wrong);
            }
        }
        spread += 1;
    }
    let (options, answer) = shuffled(rng, options, 0);
    Round {
        prompt: vec!["Continue the sequence:".into(), format!("{shown}, ?")],
        options,
        answer,
    }
}

fn balance(rng: &mut DoorRng, level: u8) -> Round {
    let flasks = if level == 2 { 4 } else { 3 };
    let top = [9, 15, 24][usize::from(level.min(2))];
    loop {
        let w: Vec<i32> = (0..flasks).map(|_| between(rng, 2, top)).collect();
        let mut pairs = Vec::new();
        for i in 0..w.len() {
            for j in (i + 1)..w.len() {
                pairs.push((i, j, w[i] + w[j]));
            }
        }
        let mut sums: Vec<i32> = pairs.iter().map(|p| p.2).collect();
        sums.sort_unstable();
        if sums.windows(2).any(|s| s[0] == s[1]) {
            continue;
        }
        let right = pick(rng, pairs.len());
        let target = pairs[right].2;
        let labels = ["A", "B", "C", "D"];
        let weights = w
            .iter()
            .enumerate()
            .map(|(i, x)| format!("{} {x}g", labels[i]))
            .collect::<Vec<_>>()
            .join(", ");
        let mut options: Vec<String> = pairs
            .iter()
            .map(|(i, j, _)| format!("{} and {}", labels[*i], labels[*j]))
            .collect();
        // Four answers at most: keep the right pair among them.
        let right_label = options[right].clone();
        options.truncate(4);
        if !options.contains(&right_label) {
            options[3].clone_from(&right_label);
        }
        let idx = options.iter().position(|o| *o == right_label).unwrap_or(0);
        let (options, answer) = shuffled(rng, options, idx);
        return Round {
            prompt: vec![
                format!("Flasks: {weights}."),
                format!("Which two on the scale read exactly {target}g?"),
            ],
            options,
            answer,
        };
    }
}

fn vocab(content: &Content, rng: &mut DoorRng, level: u8) -> Round {
    let words = &content.words;
    let w = pick(rng, words.len());
    let reverse = level >= 1 && rng.chance(1, 2);
    let count = 3 + usize::from(level == 2);
    let mut chosen = vec![w];
    while chosen.len() < count {
        let k = pick(rng, words.len());
        if !chosen.contains(&k)
            && words[k].meaning != words[w].meaning
            && words[k].term != words[w].term
        {
            chosen.push(k);
        }
    }
    let (prompt, options): (String, Vec<String>) = if reverse {
        (
            format!("Which word means \"{}\"?", words[w].meaning),
            chosen.iter().map(|k| words[*k].term.clone()).collect(),
        )
    } else {
        (
            format!("What does \"{}\" mean?", words[w].term),
            chosen.iter().map(|k| words[*k].meaning.clone()).collect(),
        )
    };
    let (options, answer) = shuffled(rng, options, 0);
    Round {
        prompt: vec!["Greywater Latin:".into(), prompt],
        options,
        answer,
    }
}

fn timeline(content: &Content, rng: &mut DoorRng, level: u8) -> Round {
    let events = &content.events;
    let gap = [40, 15, 3][usize::from(level.min(2))];
    let mut chosen: Vec<usize> = Vec::new();
    for _ in 0..40 {
        chosen.clear();
        while chosen.len() < 3 {
            let k = pick(rng, events.len());
            if !chosen.contains(&k) {
                chosen.push(k);
            }
        }
        let mut years: Vec<i32> = chosen.iter().map(|k| events[*k].year).collect();
        years.sort_unstable();
        if years.windows(2).all(|y| y[1] - y[0] >= gap) {
            break;
        }
    }
    let earliest = rng.chance(1, 2);
    let right = chosen
        .iter()
        .enumerate()
        .min_by_key(|(_, k)| {
            if earliest {
                events[**k].year
            } else {
                -events[**k].year
            }
        })
        .map_or(0, |(i, _)| i);
    let options: Vec<String> = chosen.iter().map(|k| events[*k].name.clone()).collect();
    let (options, answer) = shuffled(rng, options, right);
    Round {
        prompt: vec![format!(
            "Which of these happened {}?",
            if earliest { "earliest" } else { "latest" }
        )],
        options,
        answer,
    }
}

fn pattern(content: &Content, rng: &mut DoorRng, level: u8) -> Round {
    let motif = &content.motifs[pick(rng, content.motifs.len())];
    let period = 2 + usize::from(level.min(2));
    let mut glyphs: Vec<&String> = Vec::new();
    while glyphs.len() < period {
        let g = &motif.glyphs[pick(rng, motif.glyphs.len())];
        if !glyphs.contains(&g) {
            glyphs.push(g);
        }
    }
    let shown = period * 2 + 1;
    let line = (0..shown)
        .map(|i| glyphs[i % period].as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let right = glyphs[shown % period].clone();
    let mut options = vec![right];
    for g in &motif.glyphs {
        if !options.contains(g) && options.len() < 4 {
            options.push(g.clone());
        }
    }
    let (options, answer) = shuffled(rng, options, 0);
    Round {
        prompt: vec![
            "The border repeats. What comes next?".into(),
            format!("{line} ?"),
        ],
        options,
        answer,
    }
}

fn relay(rng: &mut DoorRng, level: u8) -> Round {
    let laps = 3 + i32::from(level);
    let names = ["Ash", "Birch", "Cedar"];
    loop {
        let runners: Vec<(i32, i32)> = (0..3)
            .map(|_| {
                (
                    between(rng, 18, 34),
                    if level == 0 {
                        0
                    } else {
                        between(rng, 2, 4 + 4 * i32::from(level))
                    },
                )
            })
            .collect();
        let totals: Vec<i32> = runners
            .iter()
            .map(|(t, r)| laps * t + (laps - 1) * r)
            .collect();
        let best = totals.iter().min().copied().unwrap_or(0);
        if totals.iter().filter(|t| **t == best).count() != 1 {
            continue;
        }
        let right = totals.iter().position(|t| *t == best).unwrap_or(0);
        let options: Vec<String> = runners
            .iter()
            .enumerate()
            .map(|(i, (t, r))| {
                if *r == 0 {
                    format!("{}: {t}s a lap", names[i])
                } else {
                    format!("{}: {t}s a lap, {r}s rest between", names[i])
                }
            })
            .collect();
        return Round {
            prompt: vec![format!("Who finishes {laps} laps first?")],
            options,
            answer: right,
        };
    }
}

impl Content {
    /// Build round `round` of a lesson from its seed.
    #[must_use]
    pub fn lesson_round(&self, kind: &str, seed: u64, round: u8, level: u8) -> Round {
        let mut rng = mix(seed, round);
        match kind {
            "sequence" => sequence(&mut rng, level),
            "balance" => balance(&mut rng, level),
            "vocab" => vocab(self, &mut rng, level),
            "timeline" => timeline(self, &mut rng, level),
            "relay" => relay(&mut rng, level),
            _ => pattern(self, &mut rng, level),
        }
    }
}

impl Game {
    /// The round now up, as the pupil sees it.
    #[must_use]
    pub fn lesson_view(&self) -> Option<RoundView> {
        let lesson = self.player.life.lesson.as_ref()?;
        let spec = self.content.lesson_for(&lesson.subject)?;
        let round = self
            .content
            .lesson_round(&spec.kind, lesson.seed, lesson.round, lesson.level);
        let mut open = vec![true; round.options.len()];
        let mut struck = 0;
        for (i, slot) in open.iter_mut().enumerate() {
            if i != round.answer && struck < lesson.hints {
                *slot = false;
                struck += 1;
            }
        }
        Some(RoundView {
            subject: lesson.subject.clone(),
            number: lesson.round,
            rounds: self.content.rules.lesson_rounds,
            round,
            open,
        })
    }

    /// Sit down in the classroom: the lesson's puzzles are fixed now.
    pub(crate) fn begin_lesson(
        &mut self,
        subject: &str,
        rng: &mut DoorRng,
    ) -> Result<Vec<Event>, Refused> {
        let spec = self
            .content
            .lesson_for(subject)
            .cloned()
            .ok_or_else(|| Refused::new("There is no such lesson."))?;
        let grade = i32::from(map_get(&self.player.grades, subject).copied().unwrap_or(0));
        let levels = &self.content.rules.level_grades;
        let level = if grade < levels[0] {
            0
        } else if grade < levels[1] {
            1
        } else {
            2
        };
        let hints = to_u8((self.skill_level(&spec.skill) / 2).clamp(0, 2));
        self.player.life.lesson = Some(Lesson {
            subject: subject.to_owned(),
            seed: rng.next_u64(),
            round: 0,
            right: 0,
            level,
            hints,
        });
        let mut events = vec![Event::Say(spec.intro.clone())];
        if hints > 0 {
            events.push(Event::Say(format!(
                "Your {} lets you rule out {} wrong answer{} in each puzzle.",
                self.content
                    .skills
                    .iter()
                    .find(|s| s.id == spec.skill)
                    .map_or(spec.skill.as_str(), |s| s.name.as_str())
                    .to_lowercase(),
                hints,
                if hints == 1 { "" } else { "s" }
            )));
        }
        Ok(events)
    }

    /// Answer the puzzle now up with option `choice`.
    pub(crate) fn answer(&mut self, choice: u8, rng: &mut DoorRng) -> Result<Vec<Event>, Refused> {
        let view = self
            .lesson_view()
            .ok_or_else(|| Refused::new("You are not in a lesson."))?;
        let index = usize::from(choice);
        if !view.open.get(index).copied().unwrap_or(false) {
            return Err(Refused::new("That answer is not on offer."));
        }
        let right = index == view.round.answer;
        let mut events = vec![Event::Say(if right {
            "Right.".into()
        } else {
            format!(
                "Not quite. It was {}.",
                view.round.options[view.round.answer]
            )
        })];
        let Some(lesson) = self.player.life.lesson.as_mut() else {
            return Ok(events);
        };
        lesson.right += u8::from(right);
        lesson.round += 1;
        if usize::from(lesson.round) < self.content.rules.lesson_rounds {
            return Ok(events);
        }
        let done = lesson.clone();
        self.player.life.lesson = None;
        events.extend(self.finish_lesson(&done, rng));
        Ok(events)
    }

    fn finish_lesson(&mut self, done: &Lesson, rng: &mut DoorRng) -> Vec<Event> {
        let content = self.content.clone();
        let rules = &content.rules;
        let score = usize::from(done.right).min(rules.lesson_rounds);
        let bonus: i32 = if score == 0 {
            0
        } else {
            self.player
                .traits
                .iter()
                .filter_map(|t| content.trait_spec(t))
                .map(|t| t.grade_bonus)
                .sum()
        };
        let gain = rules.lesson_gains[score] + bonus;
        let grade = i32::from(
            map_get(&self.player.grades, &done.subject)
                .copied()
                .unwrap_or(0),
        );
        map_set(
            &mut self.player.grades,
            &done.subject,
            to_u8((grade + gain).min(100)),
        );
        let subject = content.subject(&done.subject);
        let name = subject.map_or(done.subject.as_str(), |s| s.name.as_str());
        let mut events = vec![Event::Say(format!(
            "{name}: {} of {} right, grade {}{gain}.",
            done.right,
            rules.lesson_rounds,
            if gain >= 0 { "+" } else { "" }
        ))];
        if let Some(spec) = content.lesson_for(&done.subject) {
            let xp = rules.lesson_xp[score];
            if xp > 0
                && let Some(skill) = self.gain_xp(&spec.skill, xp)
            {
                events.push(Event::Say(format!("Your {skill} has improved.")));
            }
        }
        // The teacher notices the work.
        if let Some(teacher) = subject.map(|s| s.teacher.clone()) {
            let delta = if score == rules.lesson_rounds {
                self.player.life.perfect = self.player.life.perfect.saturating_add(1);
                2
            } else if score == 0 {
                -1
            } else {
                0
            };
            if delta != 0 {
                self.player.life.add_bond(&teacher, delta);
                events.push(Event::Say(
                    if delta > 0 {
                        "A rare smile from the front of the room."
                    } else {
                        "A sigh from the front of the room."
                    }
                    .into(),
                ));
            }
        }
        self.player.life.lessons = self.player.life.lessons.saturating_add(1);
        self.player.attended = true;
        events.extend(self.end_slot(Spent::Attended, rng));
        events
    }

    /// Walk out of the lesson in progress: it counts as a skipped class.
    pub(crate) fn walk_out(&mut self, rng: &mut DoorRng) -> Vec<Event> {
        self.player.life.lesson = None;
        let mut events = vec![Event::Say("You gather your books and walk out.".into())];
        events.extend(self.end_slot(Spent::Other, rng));
        events
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn every_kind_makes_sound_puzzles_at_every_level() {
        let content = Content::embedded().unwrap();
        for kind in [
            "sequence", "balance", "vocab", "timeline", "pattern", "relay",
        ] {
            for level in 0..3u8 {
                for seed in 0..60u64 {
                    for round in 0..3u8 {
                        let r = content.lesson_round(kind, seed, round, level);
                        assert!(r.answer < r.options.len(), "{kind}");
                        assert!((3..=4).contains(&r.options.len()), "{kind} {:?}", r.options);
                        let mut seen = r.options.clone();
                        seen.sort();
                        seen.dedup();
                        assert_eq!(
                            seen.len(),
                            r.options.len(),
                            "{kind}: repeated answers {:?}",
                            r.options
                        );
                        assert_eq!(r, content.lesson_round(kind, seed, round, level), "pure");
                    }
                }
            }
        }
    }
}
