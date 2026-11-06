//! The look-around: what each screen shows, in sentences for a screen reader. It comes after the
//! plain `[key] label` menu the text screen has always had, under a `Looking around:` line, and it
//! reads the same engine calls the drawing does. Every screen, every tier and every size gives
//! the same text: it describes the screen, not the picture.

use russh_bbs_door::DoorEnv;

use super::map::selected_place;
use super::{Screen, day::capitalise};
use crate::game::{Game, Phase};
use crate::session::{Mode, Session};
use crate::state::map_get;

fn names_at(g: &Game, place: &str) -> Vec<String> {
    let Some(slot) = g.current_slot() else {
        return Vec::new();
    };
    g.npcs_at(place, g.player.day, slot)
        .iter()
        .map(|n| {
            let word = g.regard(&n.id).word();
            if word.is_empty() {
                format!("{} ({})", n.name, n.title)
            } else {
                format!("{} ({}, {word})", n.name, n.title)
            }
        })
        .collect()
}

fn list(items: &[String], none: &str) -> String {
    if items.is_empty() {
        none.to_owned()
    } else {
        items.join("; ")
    }
}

/// The timetable as a sentence: each period, the one now marked.
fn timetable(g: &Game) -> String {
    let now = g.current_slot();
    (0..g.periods())
        .map(|slot| {
            let label = super::day::slot_label(g, g.player.day, slot);
            if now == Some(slot) {
                format!("{label} (now)")
            } else {
                label
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn vitals(g: &Game) -> String {
    let p = &g.player;
    format!(
        "Heat {}. File {} of {}. Nerve {} of {}. Legend {}. Cash {}.",
        p.heat,
        p.file_points(),
        g.content.rules.file_full,
        p.nerve,
        p.nerve_max,
        p.legend,
        p.cash
    )
}

fn day_text(g: &Game) -> Vec<String> {
    let p = &g.player;
    let place = g.content.place(&p.place);
    let mut out = vec![format!(
        "The day. You are in the {}, in the {} wing. {}",
        place.map_or("?", |x| x.name.as_str()),
        place.map_or("?", |x| x.wing.as_str()),
        g.describe_place(&p.place)
    )];
    out.push(format!(
        "Today's timetable, period by period: {}.",
        timetable(g)
    ));
    if g.current_slot().is_some() {
        out.push(format!(
            "Here: {}.",
            list(&names_at(g, &p.place), "nobody else")
        ));
        let mut near = Vec::new();
        for (place, hops) in g.routes() {
            if hops > 1 {
                continue;
            }
            let who = names_at(g, &place);
            if !who.is_empty() {
                near.push(format!(
                    "{}: {}",
                    g.content.place(&place).map_or("?", |x| x.name.as_str()),
                    who.join(", ")
                ));
            }
        }
        if !near.is_empty() {
            out.push(format!(
                "Nearby, as far as you can see: {}.",
                near.join("; ")
            ));
        }
    } else {
        out.push("Lights are out. The day is done.".to_owned());
    }
    out.push(vitals(g));
    if p.detention > 0 {
        out.push(format!("Detention owed: {}.", p.detention));
    }
    if p.summons {
        out.push("You are summoned to the headmistress tomorrow.".to_owned());
    }
    out
}

fn map_text(s: &Session) -> Vec<String> {
    let g = &s.game;
    let here = &g.player.place;
    let cursor = selected_place(s);
    let name = |id: &str| {
        g.content
            .place(id)
            .map_or(id.to_owned(), |p| p.name.clone())
    };
    let mut out = vec![format!(
        "The school map: {} places in {} wings, drawn as corridors. You are in the {}. The cursor is on the {}.",
        g.content.places.len(),
        {
            let mut wings: Vec<&str> = g.content.places.iter().map(|p| p.wing.as_str()).collect();
            wings.sort_unstable();
            wings.dedup();
            wings.len()
        },
        name(here),
        name(&cursor),
    )];
    let routes = g.routes();
    if routes.is_empty() {
        out.push("You cannot walk anywhere this period.".to_owned());
    } else {
        let walk: Vec<String> = routes
            .iter()
            .map(|(p, hops)| {
                format!(
                    "{} ({} {})",
                    name(p),
                    hops,
                    if *hops == 1 { "corridor" } else { "corridors" }
                )
            })
            .collect();
        out.push(format!("You can walk to: {}.", walk.join(", ")));
    }
    let slot = g.current_slot();
    let mut locked = Vec::new();
    let mut shut = Vec::new();
    for spec in &g.content.places {
        if *here == spec.id {
            continue;
        }
        let keyed = spec.lock.is_some() && !spec.unlock.iter().any(|c| g.holds(c));
        if keyed {
            locked.push(spec.name.clone());
        } else if slot.is_some_and(|sl| !g.place_open(&spec.id, g.player.day, sl)) {
            shut.push(spec.name.clone());
        }
    }
    if !locked.is_empty() {
        out.push(format!("Locked to you: {}.", locked.join(", ")));
    }
    if !shut.is_empty() {
        out.push(format!("Shut at this hour: {}.", shut.join(", ")));
    }
    let patrols: Vec<String> = g
        .content
        .places
        .iter()
        .filter(|p| {
            g.patrol_level(&p.id) > 0 && (routes.iter().any(|(r, _)| *r == p.id) || p.id == *here)
        })
        .map(|p| format!("{} (level {})", p.name, g.patrol_level(&p.id)))
        .collect();
    if !patrols.is_empty() {
        out.push(format!("Patrols you know of: {}.", patrols.join(", ")));
    }
    if let Some(spec) = g.content.place(&cursor) {
        let who = if routes.iter().any(|(r, _)| *r == cursor) || cursor == *here {
            list(&names_at(g, &cursor), "nobody you can see")
        } else {
            "out of sight".to_owned()
        };
        let exits: Vec<String> = spec.exits.iter().map(|e| name(e)).collect();
        out.push(format!(
            "{}: {} wing, open in {}. Exits: {}. People there: {}.",
            spec.name,
            spec.wing,
            spec.hours.join(", "),
            exits.join(", "),
            who
        ));
    }
    out.push(
        "Arrow keys move the cursor from place to place; Enter walks to it if you can reach it."
            .to_owned(),
    );
    out
}

fn scheme_text(g: &Game, id: u16) -> Vec<String> {
    let Some(s) = g.player.scheme(id) else {
        return Vec::new();
    };
    let r = g.readiness(s);
    let m = g.content.method(&s.method).map_or("?", |m| m.name.as_str());
    let t = g.content.target(&s.target).map_or("?", |t| t.name.as_str());
    let mut out = vec![format!("The planner for scheme {id}: {m} against {t}.")];
    let odds: Vec<String> = [Phase::Sneak, Phase::Act, Phase::Escape]
        .iter()
        .filter_map(|p| {
            g.chance(s, *p, 0, 0)
                .map(|c| format!("{} {} percent", p.name(), c.total))
        })
        .collect();
    out.push(format!(
        "Odds: {}. Traced back if it goes clean: {} percent.",
        odds.join(", "),
        g.trace_chance(s, 0)
    ));
    for p in [Phase::Sneak, Phase::Act, Phase::Escape] {
        if let Some(c) = g.chance(s, p, 0, 0) {
            let why: Vec<String> = c
                .parts
                .iter()
                .filter(|(_, v)| *v != 0)
                .map(|(n, v)| format!("{n} {v:+}"))
                .collect();
            out.push(format!(
                "Why {} is {} percent: {}.",
                p.name(),
                c.total,
                why.join(", ")
            ));
        }
    }
    out.push(if r.ready() {
        "Ready to run when the time comes.".to_owned()
    } else {
        let mut need = Vec::new();
        if !r.missing_items.is_empty() {
            need.push("items");
        }
        if !r.missing_roles.is_empty() {
            need.push("helpers");
        }
        if r.scout_have < r.scout_need {
            need.push("scouting");
        }
        if !r.scheduled {
            need.push("a time");
        }
        format!("Not ready: it still needs {}.", need.join(", "))
    });
    out
}

fn run_text(g: &Game) -> Vec<String> {
    let Some(run) = g.player.run.as_ref() else {
        return Vec::new();
    };
    let phase = Phase::from_index(run.phase).unwrap_or(Phase::Sneak);
    let mut out = vec![format!(
        "A scheme is running, stage {} of 3: {}. Nerve {} of {}. Slips so far: {}.",
        run.phase + 1,
        phase.name(),
        g.player.nerve,
        g.player.nerve_max,
        run.flubs
    )];
    if let Some(s) = g.player.scheme(run.scheme) {
        let spend: Vec<String> = (0..=3u8)
            .filter(|p| *p <= g.player.nerve)
            .filter_map(|p| {
                g.chance(s, phase, p, run.flubs)
                    .map(|c| format!("spending {p} gives {} percent", c.total))
            })
            .collect();
        out.push(format!("Choices: {}.", spend.join("; ")));
    }
    out
}

fn file_text(g: &Game) -> Vec<String> {
    let p = &g.player;
    let mut out = vec![format!(
        "Your file in the headmistress's office: {} of {} points. Detention owed {}. Pulled off {}, caught {}, traced {}.",
        p.file_points(),
        g.content.rules.file_full,
        p.detention,
        p.pulled,
        p.caught,
        p.pinned
    )];
    let entries = g.file_text();
    if entries.is_empty() {
        out.push("The file is empty.".to_owned());
    } else {
        out.push(format!("Entries: {}.", entries.join("; ")));
    }
    let grades: Vec<String> = g
        .content
        .subjects
        .iter()
        .map(|s| {
            format!(
                "{} {}",
                s.name,
                map_get(&p.grades, &s.id).copied().unwrap_or(0)
            )
        })
        .collect();
    out.push(format!("Grades: {}.", grades.join(", ")));
    out
}

fn lesson_text(g: &Game) -> Vec<String> {
    let Some(view) = g.lesson_view() else {
        return Vec::new();
    };
    let subject = g
        .content
        .subject(&view.subject)
        .map_or(view.subject.as_str(), |s| s.name.as_str());
    let mut out = vec![format!(
        "A {subject} lesson, puzzle {} of {}. {}",
        u32::from(view.number) + 1,
        view.rounds,
        view.round.prompt.join(" ")
    )];
    for (i, o) in view.round.options.iter().enumerate() {
        let open = view.open.get(i).copied().unwrap_or(false);
        out.push(format!(
            "Answer {}: {o}{}.",
            i + 1,
            if open { "" } else { " (ruled out)" }
        ));
    }
    out
}

fn scene_text(g: &Game) -> Vec<String> {
    let Some(view) = g.scene_view() else {
        return Vec::new();
    };
    let mut out = vec![format!(
        "A conversation with {}: {}. {}",
        view.who, view.title, view.text
    )];
    for (i, c) in view.choices.iter().enumerate() {
        out.push(if c.open {
            format!("Choice {}: {}.", i + 1, c.label)
        } else {
            format!(
                "Choice {} is shut, it needs {}: {}.",
                i + 1,
                c.wants,
                c.label
            )
        });
    }
    out
}

fn clubs_text(g: &Game) -> Vec<String> {
    let mut out = vec![format!(
        "Clubs: you can belong to {}.",
        g.content.rules.max_clubs
    )];
    for club in &g.content.clubs {
        let rank = g.player.life.rank_in(&club.id);
        let days: Vec<&str> = club
            .days
            .iter()
            .filter_map(|d| {
                [
                    "Monday",
                    "Tuesday",
                    "Wednesday",
                    "Thursday",
                    "Friday",
                    "Saturday",
                    "Sunday",
                ]
                .get(usize::from(*d))
                .copied()
            })
            .collect();
        out.push(if rank > 0 {
            format!(
                "{}: you are rank {rank} of {}. Meets {}.",
                club.name,
                club.ranks.len(),
                days.join(", ")
            )
        } else if let Some(why) = g.join_block(club) {
            format!("{}: closed to you. {why}", club.name)
        } else {
            format!("{}: open to join. Meets {}.", club.name, days.join(", "))
        });
    }
    out
}

fn paper_text(g: &Game) -> Vec<String> {
    let mut out = vec!["The school paper, the Greywater Gazette.".to_owned()];
    if g.world.paper.is_empty() {
        out.push("No headlines.".to_owned());
    }
    for h in g.world.paper.iter().rev().take(6) {
        out.push(format!("Headline: {}", h.text));
    }
    for a in &g.world.active {
        if let Some(e) = g.content.effect(&a.id) {
            out.push(format!("In force: {}.", e.name));
        }
    }
    out
}

fn hall_text(g: &Game) -> Vec<String> {
    let mut out = vec!["The hall of legend: the best of the school's pranksters.".to_owned()];
    if g.world.hall.is_empty() {
        out.push("Nobody has made the hall yet.".to_owned());
    }
    for (i, e) in g.world.hall.iter().take(8).enumerate() {
        out.push(format!("{}. {}, legend {}.", i + 1, e.name, e.legend));
    }
    out
}

fn yearbook_text(g: &Game) -> Vec<String> {
    let mut out = vec!["The yearbook: past terms.".to_owned()];
    if g.world.yearbook.is_empty() {
        out.push("No term has ended yet.".to_owned());
    }
    for y in g.world.yearbook.iter().rev().take(6) {
        out.push(format!(
            "Term {}: {}, legend {}, {} changes to the school.",
            y.term, y.name, y.legend, y.changes
        ));
    }
    out
}

fn journal_text(g: &Game) -> Vec<String> {
    let mut out = vec!["The journal: your storylines and your friends.".to_owned()];
    for l in g.journal() {
        out.push(format!("Story: {l}."));
    }
    let mut friends = Vec::new();
    let mut rivals = Vec::new();
    for (id, _) in &g.player.life.bonds {
        if let Some(n) = g.content.npc(id) {
            match g.regard(id) {
                crate::talk::Regard::Friend => friends.push(n.name.clone()),
                crate::talk::Regard::Rival | crate::talk::Regard::Betrayed => {
                    rivals.push(n.name.clone());
                }
                crate::talk::Regard::Neutral => {}
            }
        }
    }
    out.push(format!("Friends: {}.", list(&friends, "none yet")));
    out.push(format!("Rivals: {}.", list(&rivals, "none")));
    out
}

/// The `Looking around:` section for the screen `s` is showing.
pub(in crate::session) fn describe(s: &Session, _env: &DoorEnv) -> Vec<String> {
    let g = &s.game;
    let screen = Screen::of(s);
    let (body, _) = s.view();
    let mut out = vec![format!("Looking around: the {} screen.", screen.name())];
    match screen {
        Screen::Enrol => out.push(
            "Greywater Academy, a boarding school on a fog-bound lake. Choose who you want to be known as.".to_owned(),
        ),
        Screen::Over => {
            out.push("The term is over.".to_owned());
            out.extend(hall_text(g));
        }
        Screen::Main => out.extend(day_text(g)),
        Screen::Go => out.extend(map_text(s)),
        Screen::Run => out.extend(run_text(g)),
        Screen::Lesson => out.extend(lesson_text(g)),
        Screen::Scene => out.extend(scene_text(g)),
        Screen::Scheme => {
            if let Mode::Scheme(id) = s.mode {
                out.extend(scheme_text(g, id));
            }
        }
        Screen::Stats => out.extend(file_text(g)),
        Screen::Clubs => out.extend(clubs_text(g)),
        Screen::Journal => out.extend(journal_text(g)),
        Screen::News => out.extend(paper_text(g)),
        Screen::Hall => out.extend(hall_text(g)),
        Screen::Yearbook => out.extend(yearbook_text(g)),
        Screen::Study => {
            out.push("Choose a subject to study. Your grades:".to_owned());
            for sub in &g.content.subjects {
                out.push(format!(
                    "{} {}.",
                    capitalise(&sub.name),
                    map_get(&g.player.grades, &sub.id).copied().unwrap_or(0)
                ));
            }
        }
        Screen::Shop => out.push(format!("A shop. You have {} cash.", g.player.cash)),
        Screen::Plans
        | Screen::DraftTarget
        | Screen::DraftMethod
        | Screen::Twists
        | Screen::Getaway
        | Screen::Recruit
        | Screen::Schedule
        | Screen::Social
        | Screen::Classmates
        | Screen::PlotKinds
        | Screen::Rumours
        | Screen::RumourAbout
        | Screen::Election
        | Screen::Talk => {
            out.extend(body.iter().take(2).cloned());
            let (_, items) = s.view();
            let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
            out.push(format!(
                "A list of {} choices, the highlighted one explained beside it: {}.",
                labels.len(),
                labels.join("; ")
            ));
        }
    }
    if out.iter().map(String::len).sum::<usize>() < 140 {
        let (_, items) = s.view();
        let labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
        out.push(format!("You can: {}.", labels.join("; ")));
    }
    out
}
