//! Cross-table checks for the school-life tables: lessons, clubs, conversations and storylines.
//!
//! Every id a condition or an effect names must exist, every conversation node must be reachable,
//! every storyline step must have something to do, every ending must be reachable, and the tone
//! rules hold (the Strays are never the cost of a choice).

use std::collections::HashSet;

use crate::cond::{Cond, Stat};
use crate::content::Content;
use crate::effect::Eff;

/// Items a lock asks for that no shop sells: the mystery (a later slice) hands them over.
const ARRIVES_LATER: &[&str] = &[];

impl Content {
    /// Problems with the conditions anywhere in the content.
    pub(crate) fn cond_problems(&self, what: &str, conds: &[Cond], out: &mut Vec<String>) {
        for c in conds {
            self.one_cond(what, c, out);
        }
    }

    fn one_cond(&self, what: &str, cond: &Cond, out: &mut Vec<String>) {
        let mut missing = |kind: &str, id: &str| out.push(format!("{what}: unknown {kind} `{id}`"));
        match cond {
            Cond::Tag(t) if !self.traits.iter().any(|x| x.tags.contains(t)) => {
                missing("capability", t);
            }
            Cond::Trait(t) if self.trait_spec(t).is_none() => missing("trait", t),
            Cond::Item(i) if self.item(i).is_none() && !ARRIVES_LATER.contains(&i.as_str()) => {
                missing("item", i);
            }
            Cond::World(w) if self.effect(w).is_none() => missing("world effect", w),
            Cond::Arc { arc, step } => match self.arc(arc) {
                None => missing("storyline", arc),
                Some(a) if !a.step.iter().any(|s| s.id == *step) => missing("step", step),
                Some(_) => {}
            },
            Cond::Ended { arc, ending } => match self.arc(arc) {
                None => missing("storyline", arc),
                Some(a) if !a.ending.iter().any(|s| s.id == *ending) => missing("ending", ending),
                Some(_) => {}
            },
            Cond::Not(inner) => self.one_cond(what, inner, out),
            Cond::Cmp { stat, .. } => match stat {
                Stat::Skill(id) if !self.skills.iter().any(|s| s.id == *id) => missing("skill", id),
                Stat::Standing(id) if self.clique(id).is_none() => missing("clique", id),
                Stat::Wary(id) | Stat::Bond(id) if self.npc(id).is_none() => {
                    missing("character", id);
                }
                Stat::Grade(id) if self.subject(id).is_none() => missing("subject", id),
                Stat::Rank(id) if self.club(id).is_none() => missing("club", id),
                _ => {}
            },
            _ => {}
        }
    }

    pub(crate) fn eff_problems(&self, what: &str, effs: &[Eff], out: &mut Vec<String>) {
        for e in effs {
            let mut missing = |kind: &str, id: &str| {
                out.push(format!("{what}: effect names unknown {kind} `{id}`"));
            };
            match e {
                Eff::Stand(c, delta) => {
                    if self.clique(c).is_none() {
                        missing("clique", c);
                    }
                    if c == "strays" && *delta < 0 {
                        out.push(format!(
                            "{what}: no choice costs the Strays standing; they are allies"
                        ));
                    }
                }
                Eff::Bond(n, _) | Eff::Wary(n, _) if self.npc(n).is_none() => {
                    missing("character", n);
                }
                Eff::Item(i) if self.item(i).is_none() => missing("item", i),
                Eff::Xp(s, _) if !self.skills.iter().any(|x| x.id == *s) => missing("skill", s),
                Eff::Grade(s, _) if self.subject(s).is_none() => missing("subject", s),
                Eff::Merit(c, _) if self.club(c).is_none() => missing("club", c),
                Eff::Trait(t) => match self.trait_spec(t) {
                    None => missing("trait", t),
                    Some(spec) if spec.start => {
                        out.push(format!(
                            "{what}: `{t}` is a starting trait, not an earned one"
                        ));
                    }
                    Some(_) => {}
                },
                Eff::File(k, _) if !self.file_kinds.iter().any(|x| x.id == *k) => {
                    missing("file kind", k);
                }
                Eff::World(w) if self.effect(w).is_none() => missing("world effect", w),
                Eff::Arc(a, step) => match self.arc(a) {
                    None => missing("storyline", a),
                    Some(spec) if !spec.step.iter().any(|s| s.id == *step) => missing("step", step),
                    Some(_) => {}
                },
                Eff::ArcEnd(a, ending) => match self.arc(a) {
                    None => missing("storyline", a),
                    Some(spec) if !spec.ending.iter().any(|s| s.id == *ending) => {
                        missing("ending", ending);
                    }
                    Some(_) => {}
                },
                Eff::Betray(n) if n != "last_ally" => match self.npc(n) {
                    None => missing("character", n),
                    Some(npc) if npc.clique.as_deref() == Some("strays") => {
                        out.push(format!("{what}: the Strays are never betrayed in a script"));
                    }
                    Some(_) => {}
                },
                _ => {}
            }
        }
    }

    /// The flags something in the content can grant.
    fn granted_flags(&self) -> HashSet<String> {
        let mut out: HashSet<String> = self.grants.iter().map(|g| g.know.clone()).collect();
        out.insert("won_election".to_owned());
        out.extend(self.mystery.clues.iter().map(|c| format!("clue_{}", c.id)));
        out.extend(
            self.mystery
                .clues
                .iter()
                .flat_map(|c| c.effects.iter())
                .filter_map(|e| match e {
                    Eff::Know(k) => Some(k.clone()),
                    _ => None,
                }),
        );
        for n in &self.npcs {
            out.extend(n.ally.iter().flat_map(|a| a.grants.iter().cloned()));
        }
        let effects = self
            .talks
            .iter()
            .flat_map(|t| t.node.iter())
            .flat_map(|n| n.choice.iter())
            .flat_map(|c| c.effects.iter())
            .chain(
                self.arcs
                    .iter()
                    .flat_map(|a| a.ending.iter())
                    .flat_map(|e| e.effects.iter()),
            );
        for e in effects {
            if let Eff::Know(k) = e {
                out.insert(k.clone());
            }
        }
        out
    }

    fn flags_tested(&self) -> Vec<(String, String)> {
        fn walk(what: &str, c: &Cond, out: &mut Vec<(String, String)>) {
            match c {
                Cond::Know(k) => out.push((what.to_owned(), k.clone())),
                Cond::Not(inner) => walk(what, inner, out),
                _ => {}
            }
        }
        let mut out = Vec::new();
        for p in &self.places {
            for c in &p.unlock {
                walk(&format!("place `{}`", p.id), c, &mut out);
            }
        }
        for n in &self.npcs {
            for a in &n.ally {
                for c in &a.needs {
                    walk(&format!("character `{}`", n.id), c, &mut out);
                }
            }
        }
        for t in &self.talks {
            for c in &t.when {
                walk(&format!("talk `{}`", t.id), c, &mut out);
            }
            for c in t
                .node
                .iter()
                .flat_map(|n| n.choice.iter())
                .flat_map(|c| &c.needs)
            {
                walk(&format!("talk `{}`", t.id), c, &mut out);
            }
        }
        for a in &self.arcs {
            for c in &a.start {
                walk(&format!("arc `{}`", a.id), c, &mut out);
            }
        }
        for c in &self.mystery.clues {
            for cond in &c.needs {
                walk(&format!("clue `{}`", c.id), cond, &mut out);
            }
        }
        out
    }

    /// Problems across the lessons, clubs, conversations and storylines, and the conditions in
    /// the older tables that now have more to name.
    pub(crate) fn life_problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let r = &self.rules;
        if r.lesson_rounds == 0
            || r.lesson_gains.len() != r.lesson_rounds + 1
            || r.lesson_xp.len() != r.lesson_rounds + 1
            || r.level_grades.len() != 2
            || r.level_grades[0] >= r.level_grades[1]
        {
            out.push("lesson rules: gains and xp need one entry per possible score, and level_grades two rising grades".into());
        }
        if r.betray_bond >= 0
            || r.betray_to > r.betray_bond
            || r.rival_bond >= 0
            || r.friend_bond <= 0
        {
            out.push("bond rules: friend positive, rival and betrayal negative, betrayal leaves a bond at or under its limit".into());
        }
        for s in &self.subjects {
            match self.lessons.iter().find(|l| l.subject == s.id) {
                None => out.push(format!("subject `{}` has no lesson", s.id)),
                Some(l) if !self.skills.iter().any(|k| k.id == l.skill) => {
                    out.push(format!(
                        "lesson `{}` trains unknown skill `{}`",
                        l.subject, l.skill
                    ));
                }
                Some(_) => {}
            }
        }
        for l in &self.lessons {
            if self.subject(&l.subject).is_none() {
                out.push(format!("lesson names unknown subject `{}`", l.subject));
            }
        }
        // Conditions in the older tables.
        for p in &self.places {
            self.cond_problems(&format!("place `{}`", p.id), &p.unlock, &mut out);
        }
        for n in &self.npcs {
            for a in &n.ally {
                self.cond_problems(
                    &format!("character `{}` as {}", n.id, a.role),
                    &a.needs,
                    &mut out,
                );
            }
        }
        for t in &self.traits {
            self.cond_problems(&format!("trait `{}`", t.id), &t.earn, &mut out);
        }
        for i in &self.items {
            self.cond_problems(&format!("item `{}`", i.id), &i.needs, &mut out);
        }
        for m in &self.methods {
            self.cond_problems(&format!("method `{}`", m.id), &m.requires, &mut out);
        }
        for t in &self.twists {
            self.cond_problems(&format!("twist `{}`", t.id), &t.requires, &mut out);
        }
        for g in &self.getaways {
            self.cond_problems(&format!("getaway `{}`", g.id), &g.requires, &mut out);
        }
        for e in &self.endings {
            self.cond_problems(&format!("ending `{}`", e.id), &e.requires, &mut out);
        }
        // Clubs.
        for c in &self.clubs {
            let what = format!("club `{}`", c.id);
            let place = |id: &str| self.place(id).is_some();
            if !place(&c.place) || c.join_place.as_deref().is_some_and(|p| !place(p)) {
                out.push(format!("{what} has an unknown place"));
            }
            if self.npc(&c.advisor).is_none_or(|n| n.role != "staff") {
                out.push(format!("{what}: the advisor must be a member of staff"));
            }
            for m in &c.members {
                if self.npc(m).is_none() {
                    out.push(format!("{what} has unknown member `{m}`"));
                }
            }
            if !self.skills.iter().any(|s| s.id == c.skill) {
                out.push(format!("{what} trains unknown skill `{}`", c.skill));
            }
            for rival in &c.rivals {
                match self.club(rival) {
                    None => out.push(format!("{what} rivals unknown club `{rival}`")),
                    Some(other) if !other.rivals.contains(&c.id) => {
                        out.push(format!("{what} and `{rival}` must rival each other"));
                    }
                    Some(_) => {}
                }
            }
            self.cond_problems(&what, &c.join, &mut out);
            // The meeting must be possible: the place is open in that kind of slot.
            if self
                .place(&c.place)
                .is_some_and(|p| !p.hours.contains(&c.kind))
            {
                out.push(format!("{what} meets at a time its place is shut"));
            }
        }
        // The calendar.
        for e in &self.calendar {
            let what = format!("event `{}`", e.id);
            match self.place(&e.place) {
                None => out.push(format!("{what} is held at unknown place `{}`", e.place)),
                Some(p) if !e.kinds.iter().all(|k| p.hours.contains(k)) => {
                    out.push(format!("{what} runs in a period its place is shut"));
                }
                Some(_) => {}
            }
            if e.to >= self.rules.term_days {
                out.push(format!("{what} falls after the term ends"));
            }
            match self.npc(&e.host) {
                None => out.push(format!("{what} is run by unknown character `{}`", e.host)),
                Some(n) if n.role == "student" && n.clique.as_deref() == Some("strays") => {
                    out.push(format!("{what}: a Stray is never the host of an ordeal"));
                }
                Some(_) => {}
            }
            let talks = self
                .talks
                .iter()
                .filter(|t| t.event.as_deref() == Some(e.id.as_str()))
                .count();
            if talks < 3 {
                out.push(format!("{what} needs at least three scenes, it has {talks}"));
            }
        }
        // Conversations.
        for t in &self.talks {
            let what = format!("talk `{}`", t.id);
            if let Some(ev) = &t.event {
                match self.calendar.iter().find(|e| e.id == *ev) {
                    None => out.push(format!("{what} belongs to unknown event `{ev}`")),
                    Some(e) if e.host != t.npc => {
                        out.push(format!("{what} is with `{}`, not the event's host", t.npc));
                    }
                    Some(_) => {}
                }
            }
            if self.npc(&t.npc).is_none() {
                out.push(format!("{what} is with unknown character `{}`", t.npc));
            }
            self.cond_problems(&what, &t.when, &mut out);
            if let (Some(a), Some(step)) = (&t.arc, &t.step) {
                match self.arc(a) {
                    None => out.push(format!("{what} belongs to unknown storyline `{a}`")),
                    Some(spec) if !spec.step.iter().any(|s| s.id == *step) => {
                        out.push(format!("{what} is open at unknown step `{step}`"));
                    }
                    Some(_) => {}
                }
            }
            for n in &t.node {
                for c in &n.choice {
                    self.cond_problems(&what, &c.needs, &mut out);
                    self.eff_problems(&what, &c.effects, &mut out);
                }
            }
        }
        // Storylines.
        let mut ended: HashSet<(String, String)> = HashSet::new();
        let mut entered: HashSet<(String, String)> = HashSet::new();
        for t in &self.talks {
            for e in t
                .node
                .iter()
                .flat_map(|n| n.choice.iter())
                .flat_map(|c| c.effects.iter())
            {
                match e {
                    Eff::ArcEnd(a, end) => {
                        ended.insert((a.clone(), end.clone()));
                    }
                    Eff::Arc(a, step) => {
                        entered.insert((a.clone(), step.clone()));
                    }
                    _ => {}
                }
            }
        }
        for e in self.mystery.clues.iter().flat_map(|c| c.effects.iter()) {
            if let Eff::Arc(a, step) = e {
                entered.insert((a.clone(), step.clone()));
            }
        }
        for a in &self.arcs {
            let what = format!("storyline `{}`", a.id);
            self.cond_problems(&what, &a.start, &mut out);
            for e in &a.ending {
                self.eff_problems(&what, &e.effects, &mut out);
                if !ended.contains(&(a.id.clone(), e.id.clone())) {
                    out.push(format!("{what}: ending `{}` can never be reached", e.id));
                }
            }
            for s in &a.step {
                if s.id != a.first && !entered.contains(&(a.id.clone(), s.id.clone())) {
                    out.push(format!("{what}: step `{}` can never be entered", s.id));
                }
                let a_clue_waits_here = self.mystery.clues.iter().any(|c| {
                    c.needs.iter().any(
                        |n| matches!(n, Cond::Arc { arc, step } if *arc == a.id && *step == s.id),
                    )
                });
                if !a_clue_waits_here
                    && !self.talks.iter().any(|t| {
                        t.arc.as_deref() == Some(a.id.as_str())
                            && t.step.as_deref() == Some(s.id.as_str())
                    })
                {
                    out.push(format!(
                        "{what}: step `{}` has no conversation to move it on",
                        s.id
                    ));
                }
            }
        }
        // A flag a condition tests must be something the game can grant.
        let granted = self.granted_flags();
        for (what, flag) in self.flags_tested() {
            if !granted.contains(&flag) && !flag.starts_with("betrayed_") {
                out.push(format!("{what} tests `know:{flag}`, which nothing grants"));
            }
        }
        out
    }

    /// A club by id.
    #[must_use]
    pub fn club(&self, id: &str) -> Option<&crate::content::ClubSpec> {
        self.clubs.iter().find(|c| c.id == id)
    }

    /// A storyline by id.
    #[must_use]
    pub fn arc(&self, id: &str) -> Option<&crate::content::ArcSpec> {
        self.arcs.iter().find(|a| a.id == id)
    }

    /// A conversation by id.
    #[must_use]
    pub fn talk(&self, id: &str) -> Option<&crate::content::Talk> {
        self.talks.iter().find(|t| t.id == id)
    }

    /// The lesson a subject uses.
    #[must_use]
    pub fn lesson_for(&self, subject: &str) -> Option<&crate::content::LessonSpec> {
        self.lessons.iter().find(|l| l.subject == subject)
    }
}
