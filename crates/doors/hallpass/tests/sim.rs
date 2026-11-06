//! A headless simulation of whole 6-week terms: heat, file and legend curves, endings, and which
//! branches of the content actually get used. Run with `--nocapture` to see the tables.
#![allow(
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::type_complexity,
    clippy::collapsible_if,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::print_stderr,
    clippy::panic
)]
mod common;

use std::collections::BTreeMap;
use std::fmt::Write;

use common::bot::{
    Policy, athlete, balanced, cautious, play_day, politician, reckless, recluse, rival, scholar,
    sleuth, take_coverage, variety,
};
use common::{content, monday};
use russh_bbs_door::DoorRng;
use russh_bbs_door_hallpass::game::{Action, Game};
use russh_bbs_door_hallpass::state::{Status, World};
use russh_bbs_door_hallpass::world::tick_day;

#[derive(Default, Debug, Clone)]
struct Curve {
    heat: Vec<u8>,
    file: Vec<i32>,
    legend: Vec<u32>,
}

#[derive(Debug, Clone)]
struct Outcome {
    policy: &'static str,
    ending: String,
    legend: u32,
    file: i32,
    pulled: u16,
    caught: u16,
    pinned: u16,
    big: u16,
    curve: Curve,
    methods: Vec<String>,
    /// Average grade at the end.
    grades: i32,
    /// Lessons taken and lessons with every puzzle right.
    lessons: (u16, u16),
    /// Club and rank at the end.
    clubs: Vec<(String, u8)>,
    /// Each storyline begun and how it stands: its ending, or `open`.
    arcs: Vec<(String, String)>,
    /// Friends and rivals made.
    friends: usize,
    rivals: usize,
    /// Clues found in the east wing's mystery.
    clues: i32,
    /// The pupil's own ending of the mystery, `open` if they did not reach one, `none` if it never began.
    wing: String,
    /// Schemes sent to other pupils, how they ended for the sender, and rumours started.
    sent: u32,
    landed: u32,
    seen: u32,
    turned: u32,
    rumours: u32,
    /// Whether they stood for election and how many votes and backing they got.
    ran: bool,
    opted_out: bool,
    awards: Vec<String>,
    /// Lines in the inbox that are not awards: what other pupils did to this one.
    hit: usize,
}

/// One whole term on one shared school with one player per policy.
fn term(policies: &[Policy], traits: &[&str], seed: u64) -> (Vec<Outcome>, World) {
    let content = content();
    let start = monday();
    let mut world = World::new(start);
    // The school's mood rotates with the term, so the seeds stand for terms one, two and three.
    world.term = 1 + u32::try_from(seed % 3).unwrap();
    let mut games: Vec<Game> = Vec::new();
    let mut rngs: Vec<DoorRng> = Vec::new();
    for (i, policy) in policies.iter().enumerate() {
        let name = format!("{}{}", policy.name, i);
        let (mut g, _) = Game::open(&content, &name, None, Some(&world.to_bytes()), start);
        let mut rng = DoorRng::from_seed(seed * 1000 + i as u64);
        g.apply(
            &Action::Enrol(traits[(i + usize::try_from(seed).unwrap()) % traits.len()].to_owned()),
            &mut rng,
        )
        .unwrap();
        games.push(g);
        rngs.push(rng);
    }
    let mut curves = vec![Curve::default(); policies.len()];
    let mut methods: Vec<Vec<String>> = vec![Vec::new(); policies.len()];
    // sent, landed, seen, turned, rumours
    let mut social: Vec<[u32; 5]> = vec![[0; 5]; policies.len()];
    let mut tick_rng = DoorRng::from_label("hallpass", seed as i64);
    for day in 0..content.rules.term_days {
        let today = start + day;
        for (i, g) in games.iter_mut().enumerate() {
            g.world = world.clone();
            let mut events = g.sync_day(today);
            events.extend(g.social_day());
            events.extend(play_day(g, &policies[i], &mut rngs[i]));
            for e in &events {
                let text = e.describe();
                let n = &mut social[i];
                if text.contains("in motion for") {
                    n[0] += 1;
                } else if text.contains("it landed") {
                    n[1] += 1;
                } else if text.contains("saw it coming") {
                    n[2] += 1;
                } else if text.contains("never reached") {
                    n[3] += 1;
                } else if text.contains("You let it drop") {
                    n[4] += 1;
                }
            }
            for d in g.take_world_changes() {
                d.apply(&mut world, &content);
            }
            let p = &g.player;
            curves[i].heat.push(p.heat);
            curves[i].file.push(p.file_points());
            curves[i].legend.push(p.legend);
            if let Some(m) = p.done.last() {
                if !methods[i].contains(&m.0) {
                    methods[i].push(m.0.clone());
                }
            }
        }
        if day + 1 < content.rules.term_days {
            tick_day(&mut world, &content, today + 1, &mut tick_rng);
        }
    }
    let end = start + content.rules.term_days;
    let social = social;
    let outcomes = games
        .into_iter()
        .enumerate()
        .map(|(i, mut g)| {
            let (grades, lessons, clubs, arcs, friends, rivals) = {
                let p = &g.player;
                (
                    g.average_grade(),
                    (p.life.lessons, p.life.perfect),
                    p.life
                        .clubs
                        .iter()
                        .map(|m| (m.club.clone(), m.rank))
                        .collect(),
                    p.life
                        .arcs
                        .iter()
                        .map(|a| {
                            (
                                a.id.clone(),
                                a.ended.clone().unwrap_or_else(|| "open".into()),
                            )
                        })
                        .collect::<Vec<_>>(),
                    p.life
                        .bonds
                        .iter()
                        .filter(|(n, _)| {
                            g.regard(n) == russh_bbs_door_hallpass::talk::Regard::Friend
                        })
                        .count(),
                    p.life
                        .bonds
                        .iter()
                        .filter(|(n, _)| {
                            matches!(
                                g.regard(n),
                                russh_bbs_door_hallpass::talk::Regard::Rival
                                    | russh_bbs_door_hallpass::talk::Regard::Betrayed
                            )
                        })
                        .count(),
                )
            };
            g.world = world.clone();
            g.sync_day(end);
            let ending = g
                .player
                .history
                .last()
                .map_or_else(|| "unfinished".to_owned(), |r| r.ending.clone());
            let wing = g.player.life.arc("the_wing").map_or_else(
                || "none".to_owned(),
                |a| a.ended.clone().unwrap_or_else(|| "open".to_owned()),
            );
            let clues = g.stat(&russh_bbs_door_hallpass::cond::Stat::Clues);
            Outcome {
                clues,
                wing,
                sent: social[i][0],
                landed: social[i][1],
                seen: social[i][2],
                turned: social[i][3],
                rumours: social[i][4],
                ran: g.player.social.ran_term == g.player.term,
                hit: g
                    .player
                    .social
                    .inbox
                    .iter()
                    .filter(|l| !l.starts_with("Award"))
                    .count(),
                opted_out: g.player.social.opt_out,
                awards: g.player.social.awards.clone(),
                policy: policies[i].name,
                ending,
                legend: g.player.legend,
                file: g.player.file_points(),
                pulled: g.player.pulled,
                caught: g.player.caught,
                pinned: g.player.pinned,
                big: g.player.big,
                curve: curves[i].clone(),
                methods: g.player.done.iter().map(|(k, _)| k.clone()).collect(),
                grades,
                lessons,
                clubs,
                arcs,
                friends,
                rivals,
            }
        })
        .collect();
    (outcomes, world)
}

fn all_policies() -> Vec<Policy> {
    vec![cautious(), balanced(), reckless(), variety()]
}

#[test]
fn a_whole_term_plays_out_and_the_curves_are_sane() {
    let (outcomes, world) = term(
        &all_policies(),
        &["smooth_talker", "fast_runner", "tinkerer"],
        1,
    );
    eprintln!("\n=== one 6-week term, three players on one school (seed 1) ===");
    eprintln!(
        "{:<9} {:>7} {:>5} {:>6} {:>6} {:>6} {:>4}  ending",
        "policy", "legend", "file", "pulled", "caught", "traced", "big"
    );
    for o in &outcomes {
        eprintln!(
            "{:<9} {:>7} {:>5} {:>6} {:>6} {:>6} {:>4}  {}",
            o.policy, o.legend, o.file, o.pulled, o.caught, o.pinned, o.big, o.ending
        );
    }
    eprintln!("\nday   | heat c/b/r   | file c/b/r  | legend c/b/r");
    for day in (0..42).step_by(3) {
        let at =
            |f: &dyn Fn(&Outcome) -> String| outcomes.iter().map(f).collect::<Vec<_>>().join("/");
        eprintln!(
            "{:>4}  | {:<12} | {:<11} | {}",
            day + 1,
            at(&|o| o.curve.heat[day].to_string()),
            at(&|o| o.curve.file[day].to_string()),
            at(&|o| o.curve.legend[day].to_string()),
        );
    }
    eprintln!(
        "school changes this term: {}, paper: {} headlines",
        world.changes,
        world.paper.len()
    );
    for o in &outcomes {
        assert_eq!(o.curve.heat.len(), 42);
        assert!(
            o.curve.heat.iter().all(|h| *h <= 100),
            "{} heat {:?}",
            o.policy,
            o.curve.heat
        );
        assert!(
            o.curve.legend.windows(2).all(|w| w[0] <= w[1]),
            "legend never falls"
        );
        assert!(o.curve.file.iter().all(|f| *f <= 14), "{:?}", o.curve.file);
        assert_ne!(o.ending, "unfinished");
    }
}

#[test]
fn over_many_terms_the_playstyles_pay_differently_and_every_ending_and_branch_is_reachable() {
    let seeds = 40;
    let mut stats: BTreeMap<&'static str, Vec<Outcome>> = BTreeMap::new();
    let mut endings: BTreeMap<String, u32> = BTreeMap::new();
    let mut methods: BTreeMap<String, u32> = BTreeMap::new();
    for seed in 0..seeds {
        let (outcomes, _) = term(
            &all_policies(),
            &["smooth_talker", "fast_runner", "tinkerer", "teachers_pet"],
            seed,
        );
        for o in outcomes {
            *endings.entry(o.ending.clone()).or_default() += 1;
            for m in &o.methods {
                *methods
                    .entry(m.split('/').nth(1).unwrap_or("?").to_owned())
                    .or_default() += 1;
            }
            stats.entry(o.policy).or_default().push(o);
        }
    }
    eprintln!("\n=== {seeds} terms per policy ===");
    eprintln!(
        "{:<9} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9}",
        "policy", "legend", "file", "pulled", "caught", "traced", "expelled"
    );
    let mean =
        |v: &[Outcome], f: &dyn Fn(&Outcome) -> f64| v.iter().map(f).sum::<f64>() / v.len() as f64;
    for (name, v) in &stats {
        eprintln!(
            "{:<9} {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>8.0}%",
            name,
            mean(v, &|o| f64::from(o.legend)),
            mean(v, &|o| f64::from(o.file)),
            mean(v, &|o| f64::from(o.pulled)),
            mean(v, &|o| f64::from(o.caught)),
            mean(v, &|o| f64::from(o.pinned)),
            100.0 * mean(v, &|o| f64::from(u8::from(o.ending == "expelled"))),
        );
    }
    eprintln!("endings: {endings:?}");
    eprintln!("methods used: {methods:?}");
    let legend = |n: &str| mean(&stats[n], &|o| f64::from(o.legend));
    let expelled = |n: &str| mean(&stats[n], &|o| f64::from(u8::from(o.ending == "expelled")));
    assert!(
        legend("reckless") > legend("cautious"),
        "boldness pays in legend"
    );
    assert!(
        expelled("reckless") > expelled("cautious"),
        "and costs in expulsions"
    );
    assert!(legend("cautious") > 0.0, "even the careful make a name");
    assert!(endings.len() >= 3, "several endings occur: {endings:?}");
    assert!(
        methods.len() >= 5,
        "many different tricks get used: {methods:?}"
    );
    let _ = Status::Active;
}

fn life_policies() -> Vec<Policy> {
    vec![scholar(), cautious(), balanced(), reckless(), variety()]
}

#[test]
fn school_life_in_numbers_grades_club_ranks_and_storyline_endings() {
    let seeds = 40u64;
    let mut stats: BTreeMap<&'static str, Vec<Outcome>> = BTreeMap::new();
    for seed in 0..seeds {
        let (outcomes, _) = term(
            &life_policies(),
            &["smooth_talker", "fast_runner", "tinkerer", "teachers_pet"],
            seed,
        );
        for o in outcomes {
            stats.entry(o.policy).or_default().push(o);
        }
    }
    let mean =
        |v: &[Outcome], f: &dyn Fn(&Outcome) -> f64| v.iter().map(f).sum::<f64>() / v.len() as f64;
    eprintln!("\n=== school life: {seeds} terms per policy ===");
    eprintln!(
        "{:<9} {:>7} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9}",
        "policy", "grades", "lessons", "perfect", "clubs", "friends", "rivals", "honour%"
    );
    for (name, v) in &stats {
        eprintln!(
            "{:<9} {:>7.1} {:>8.1} {:>8.1} {:>8.2} {:>8.1} {:>8.1} {:>8.0}%",
            name,
            mean(v, &|o| f64::from(o.grades)),
            mean(v, &|o| f64::from(o.lessons.0)),
            mean(v, &|o| f64::from(o.lessons.1)),
            mean(v, &|o| o.clubs.len() as f64),
            mean(v, &|o| o.friends as f64),
            mean(v, &|o| o.rivals as f64),
            100.0 * mean(v, &|o| f64::from(u8::from(o.grades >= 75))),
        );
    }
    // Club ranks reached, over every player.
    let mut ranks: BTreeMap<(String, u8), u32> = BTreeMap::new();
    let mut by_arc: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for o in stats.values().flatten() {
        for (club, rank) in &o.clubs {
            *ranks.entry((club.clone(), *rank)).or_default() += 1;
        }
        for (arc, end) in &o.arcs {
            *by_arc
                .entry(arc.clone())
                .or_default()
                .entry(end.clone())
                .or_default() += 1;
        }
    }
    eprintln!("\nclub ranks reached (club, rank: players):");
    for ((club, rank), n) in &ranks {
        eprintln!("  {club:<9} rank {rank}: {n}");
    }
    eprintln!("storyline endings (how each story stood at term end, players):");
    for (arc, ends) in &by_arc {
        eprintln!("  {arc}: {ends:?}");
    }
    let grades = |n: &str| mean(&stats[n], &|o| f64::from(o.grades));
    assert!(
        grades("scholar") > grades("reckless"),
        "a pupil who works has better marks than one who skips: {} vs {}",
        grades("scholar"),
        grades("reckless")
    );
    assert!(
        ranks.keys().any(|(_, r)| *r >= 2),
        "clubs are climbed: {ranks:?}"
    );
    assert!(
        by_arc.len() == 3,
        "every storyline begins for somebody: {by_arc:?}"
    );
    let finished: usize = by_arc
        .values()
        .map(|e| e.keys().filter(|k| *k != "open").count())
        .sum();
    assert!(finished >= 3, "several storyline endings occur: {by_arc:?}");
}

// ---------------------------------------------------------------- the full report (H4)

/// How the east wing's secret is settled by the investigators of a term: which of the six endings
/// they go for rotates with the seed, so every ending is tried.
const ENDINGS: [&[&str]; 6] = [
    &["fenn"],
    &["soames"],
    &["wren"],
    &["vane"],
    &["bonfire"],
    &["walk_away"],
];

fn mixed_policies(seed: u64) -> Vec<Policy> {
    let which = usize::try_from(seed % 6).unwrap();
    let mut investigator = sleuth();
    investigator.verdicts = ENDINGS[which];
    let mut quiet = recluse();
    quiet.verdicts = ENDINGS[(which + 3) % 6];
    vec![
        scholar(),
        cautious(),
        balanced(),
        reckless(),
        variety(),
        investigator,
        politician(),
        rival(),
        quiet,
        athlete(),
    ]
}

/// The policy a player name belongs to (`politician6` is a politician).
fn policy_of(name: &str) -> &str {
    name.trim_end_matches(|c: char| c.is_ascii_digit())
}

/// Everything the balance pass measures, over `seeds` whole terms of nine mixed styles on one school.
struct Report {
    stats: BTreeMap<&'static str, Vec<Outcome>>,
    winners: BTreeMap<String, u32>,
    finales: BTreeMap<String, u32>,
    awards: BTreeMap<String, u32>,
    effects: BTreeMap<String, u32>,
    coverage: BTreeMap<String, u32>,
    terms: u32,
}

fn report(seeds: u64) -> Report {
    let content = content();
    let mut r = Report {
        stats: BTreeMap::new(),
        winners: BTreeMap::new(),
        finales: BTreeMap::new(),
        awards: BTreeMap::new(),
        effects: BTreeMap::new(),
        coverage: BTreeMap::new(),
        terms: 0,
    };
    let _ = take_coverage();
    for seed in 0..seeds {
        let (outcomes, world) = term(
            &mixed_policies(seed),
            &["smooth_talker", "fast_runner", "tinkerer", "teachers_pet"],
            seed,
        );
        r.terms += 1;
        let winner = world
            .commons
            .election
            .winner
            .clone()
            .map_or_else(|| "nobody".to_owned(), |w| policy_of(&w).to_owned());
        *r.winners.entry(winner).or_default() += 1;
        *r.finales
            .entry(world.commons.finale.clone().map_or_else(
                || "not yet".to_owned(),
                |f| {
                    if f.is_empty() {
                        "unsolved".to_owned()
                    } else {
                        f
                    }
                },
            ))
            .or_default() += 1;
        for a in &world.active {
            *r.effects.entry(a.id.clone()).or_default() += 1;
        }
        let mut rolled = world.clone();
        tick_day(
            &mut rolled,
            &content,
            world.term_start + content.rules.term_days,
            &mut DoorRng::from_seed(1),
        );
        for a in &rolled.commons.awards {
            *r.awards
                .entry(format!("{}:{}", a.award, policy_of(&a.name)))
                .or_default() += 1;
        }
        for o in outcomes {
            r.stats.entry(o.policy).or_default().push(o);
        }
    }
    r.coverage = take_coverage();
    r
}

fn print_report(r: &Report) {
    let mean =
        |v: &[Outcome], f: &dyn Fn(&Outcome) -> f64| v.iter().map(f).sum::<f64>() / v.len() as f64;
    eprintln!("\n=== {} terms, nine styles on one school ===", r.terms);
    eprintln!(
        "{:<11} {:>6} {:>6} {:>6} {:>5} {:>8} {:>6} {:>7} {:>6} {:>7} {:>6} {:>6} {:>5} {:>7}",
        "style",
        "legend",
        "pulled",
        "caught",
        "file",
        "expelled",
        "grade",
        "honour%",
        "clues",
        "wing%",
        "sent",
        "landed",
        "seen",
        "rumours"
    );
    for (name, v) in &r.stats {
        eprintln!(
            "{:<11} {:>6.1} {:>6.1} {:>6.1} {:>5.1} {:>7.0}% {:>6.1} {:>6.0}% {:>6.1} {:>6.0}% {:>6.1} {:>6.1} {:>5.1} {:>7.1}",
            name,
            mean(v, &|o| f64::from(o.legend)),
            mean(v, &|o| f64::from(o.pulled)),
            mean(v, &|o| f64::from(o.caught)),
            mean(v, &|o| f64::from(o.file)),
            100.0 * mean(v, &|o| f64::from(u8::from(o.ending == "expelled"))),
            mean(v, &|o| f64::from(o.grades)),
            100.0 * mean(v, &|o| f64::from(u8::from(o.grades >= 75))),
            mean(v, &|o| f64::from(o.clues)),
            100.0
                * mean(v, &|o| f64::from(u8::from(
                    !["none", "open"].contains(&o.wing.as_str())
                ))),
            mean(v, &|o| f64::from(o.sent)),
            mean(v, &|o| f64::from(o.landed)),
            mean(v, &|o| f64::from(o.seen)),
            mean(v, &|o| f64::from(o.rumours)),
        );
    }
    eprintln!(
        "\nother pupils and the polls (per style: schemes turned away, share who stood, awards won in total):"
    );
    for (name, v) in &r.stats {
        eprintln!(
            "  {name:<11} turned away {:>4.1}  stood {:>3.0}%  awards held {:>3}",
            mean(v, &|o| f64::from(o.turned)),
            100.0 * mean(v, &|o| f64::from(u8::from(o.ran))),
            v.iter().map(|o| o.awards.len()).sum::<usize>(),
        );
    }
    let mut endings: BTreeMap<String, u32> = BTreeMap::new();
    let mut wing: BTreeMap<String, u32> = BTreeMap::new();
    let mut ranks: BTreeMap<(String, u8), u32> = BTreeMap::new();
    let mut by_arc: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    let mut deepest: BTreeMap<i32, u32> = BTreeMap::new();
    for o in r.stats.values().flatten() {
        *endings.entry(o.ending.clone()).or_default() += 1;
        *wing.entry(o.wing.clone()).or_default() += 1;
        *deepest.entry(o.clues).or_default() += 1;
        for (club, rank) in &o.clubs {
            *ranks.entry((club.clone(), *rank)).or_default() += 1;
        }
        for (arc, end) in &o.arcs {
            *by_arc
                .entry(arc.clone())
                .or_default()
                .entry(end.clone())
                .or_default() += 1;
        }
    }
    eprintln!("\nterm endings (players): {endings:?}");
    eprintln!("club ranks (club rank: players):");
    let mut line = String::new();
    for ((club, rank), n) in &ranks {
        let _ = write!(line, "  {club} {rank}: {n};");
    }
    eprintln!("{line}");
    eprintln!("storylines at term end (players):");
    for (arc, ends) in &by_arc {
        eprintln!("  {arc}: {ends:?}");
    }
    eprintln!("the east wing: clues found by players {deepest:?}");
    eprintln!("the east wing: each pupil's own ending {wing:?}");
    eprintln!("the east wing: how the school ended it {:?}", r.finales);
    eprintln!("election winners (who won the term): {:?}", r.winners);
    let mut by_award: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (k, n) in &r.awards {
        let (award, who) = k.split_once(':').unwrap_or((k, "?"));
        by_award
            .entry(award)
            .or_default()
            .push(format!("{who} {n}"));
    }
    eprintln!("awards won over the terms (winning style and times): {by_award:?}");
    eprintln!("world effects in force at term end: {:?}", r.effects);
    let c = content();
    let total_choices: usize = c
        .talks
        .iter()
        .flat_map(|t| t.node.iter().map(move |n| (t, n)))
        .map(|(_, n)| n.choice.len())
        .sum();
    let taken_choices = r
        .coverage
        .keys()
        .filter(|k| k.starts_with("choice:"))
        .count();
    let talks_taken: std::collections::BTreeSet<&str> = r
        .coverage
        .keys()
        .filter_map(|k| k.strip_prefix("choice:"))
        .filter_map(|k| k.split('/').next())
        .collect();
    let dead_talks: Vec<&str> = c
        .talks
        .iter()
        .map(|t| t.id.as_str())
        .filter(|t| !talks_taken.contains(t))
        .collect();
    eprintln!(
        "branch coverage: {taken_choices} of {total_choices} conversation choices taken; talks never had: {dead_talks:?}"
    );
    if std::env::var("HALLPASS_LIST_UNTAKEN").is_ok() {
        for t in &c.talks {
            for n in &t.node {
                for (i, ch) in n.choice.iter().enumerate() {
                    if !r
                        .coverage
                        .contains_key(&format!("choice:{}/{}/{i}", t.id, n.id))
                    {
                        eprintln!("untaken: {}/{}/{i} {}", t.id, n.id, ch.label);
                    }
                }
            }
        }
    }
    let missed = |prefix: &str, all: Vec<String>| -> Vec<String> {
        all.into_iter()
            .filter(|id| !r.coverage.contains_key(&format!("{prefix}:{id}")))
            .collect()
    };
    eprintln!(
        "methods never drafted: {:?}",
        missed("method", c.methods.iter().map(|m| m.id.clone()).collect())
    );
    eprintln!(
        "plots never played: {:?}; rumours never told: {:?}; clues never found: {:?}",
        missed(
            "plot",
            c.social.plots.iter().map(|p| p.id.clone()).collect()
        ),
        missed(
            "rumour",
            c.social.rumours.iter().map(|p| p.id.clone()).collect()
        ),
        missed(
            "clue",
            c.mystery.clues.iter().map(|p| p.id.clone()).collect()
        ),
    );
}

#[test]
fn the_full_report_nine_styles_whole_terms() {
    let seeds: u64 = std::env::var("HALLPASS_SEEDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);
    let r = report(seeds);
    print_report(&r);
    // Sanity only here; the no-dead-content gates are in the next test.
    assert_eq!(r.stats.len(), 10);
    for v in r.stats.values() {
        assert_eq!(v.len() as u64, seeds);
    }
}

#[test]
fn no_ending_and_no_branch_of_the_social_layer_or_the_mystery_is_dead_content() {
    let r = report(40);
    let c = content();
    let all = |f: &dyn Fn(&Outcome) -> bool| r.stats.values().flatten().any(f);
    // The clubs the H2 simulation never saw joined are joined, and climbed.
    for club in ["paper", "candle"] {
        assert!(
            all(&|o| o.clubs.iter().any(|(c, rank)| c == club && *rank >= 1)),
            "{club} was never joined"
        );
    }
    assert!(
        all(&|o| o.clubs.iter().any(|(c, rank)| c == "candle" && *rank >= 3)),
        "the Candle Society is climbed"
    );
    // Every ending of the prefects' ledger occurs, including the headline H2 never saw.
    let arc_end = |arc: &str, end: &str| all(&|o| o.arcs.iter().any(|(a, e)| a == arc && e == end));
    for ledger in ["shrugged", "returned", "owed", "purged", "headline"] {
        assert!(
            arc_end("ledger", ledger),
            "ledger ending {ledger} never occurs"
        );
    }
    for light in ["ignored", "quiet", "informer", "secret", "shared"] {
        assert!(
            arc_end("east_light", light),
            "east light ending {light} never occurs"
        );
    }
    // Every ending of the east wing, for a pupil and for the school.
    for ending in ["fenn", "soames", "vane", "paper", "bonfire", "walk_away"] {
        assert!(
            arc_end("the_wing", ending),
            "the_wing ending {ending} never occurs"
        );
    }
    for verdict in &c.mystery.verdicts {
        assert!(
            r.finales.get(&verdict.id).copied().unwrap_or(0) > 0,
            "the school never settled on {}: {:?}",
            verdict.id,
            r.finales
        );
    }
    assert!(r.finales.contains_key("unsolved"), "{:?}", r.finales);
    // The election: each character who runs wins sometimes, and so do pupils.
    for runner in &c.social.runners {
        assert!(
            r.winners.contains_key(&format!("npc:{}", runner.npc)),
            "{} never wins: {:?}",
            runner.npc,
            r.winners
        );
    }
    assert!(
        r.winners.keys().any(|w| !w.starts_with("npc:")),
        "{:?}",
        r.winners
    );
    // Every award, plot, rumour and clue gets used.
    for a in &c.social.awards {
        assert!(
            r.awards
                .keys()
                .any(|k| k.starts_with(&format!("{}:", a.id))),
            "award {} is never won",
            a.id
        );
    }
    for p in &c.social.plots {
        assert!(
            r.coverage.contains_key(&format!("plot:{}", p.id)),
            "plot {}",
            p.id
        );
    }
    for u in &c.social.rumours {
        assert!(
            r.coverage.contains_key(&format!("rumour:{}", u.id)),
            "rumour {}",
            u.id
        );
    }
    for k in &c.mystery.clues {
        assert!(
            r.coverage.contains_key(&format!("clue:{}", k.id)),
            "clue {}",
            k.id
        );
    }
    // Every scheme is tried by somebody: there are sixty, and none sits unused.
    for m in &c.methods {
        assert!(
            r.coverage.contains_key(&format!("method:{}", m.id)),
            "scheme {} was never drafted",
            m.id
        );
    }
    // Every term event is attended, and each of its scenes is had by somebody.
    for t in c.talks.iter().filter(|t| t.event.is_some()) {
        assert!(
            r.coverage
                .keys()
                .any(|k| k.starts_with(&format!("choice:{}/", t.id))),
            "event scene {} was never had",
            t.id
        );
    }
    // Conversations: nearly every choice is taken by somebody. The ones that need a betrayal
    // (a made-up quarrel) are the exception the bots do not play.
    let total: usize = c
        .talks
        .iter()
        .flat_map(|t| t.node.iter())
        .map(|n| n.choice.len())
        .sum();
    let taken = r
        .coverage
        .keys()
        .filter(|k| k.starts_with("choice:"))
        .count();
    assert!(
        taken * 100 >= total * 90,
        "{taken} of {total} conversation choices taken"
    );
    // The opt-out holds in play: the pupil who opted out sent nothing and was sent nothing,
    // while the others were hit.
    let hits = |style: &str| r.stats[style].iter().map(|o| o.hit).sum::<usize>();
    let sent = |style: &str| r.stats[style].iter().map(|o| o.sent).sum::<u32>();
    assert!(r.stats["recluse"].iter().all(|o| o.opted_out));
    assert_eq!(
        hits("recluse"),
        0,
        "nothing was done to a pupil who opted out"
    );
    assert_eq!(sent("recluse"), 0);
    assert!(hits("balanced") + hits("reckless") + hits("rival") + hits("cautious") > 0);
    // The strong are not unbeatable: expulsions happen only to the reckless, and the careful are never expelled.
    let expelled = |style: &str| {
        r.stats[style]
            .iter()
            .filter(|o| o.ending == "expelled")
            .count()
    };
    assert_eq!(
        expelled("scholar") + expelled("cautious") + expelled("recluse"),
        0
    );
    assert!(expelled("reckless") > 0);
}

#[test]
fn the_school_mood_rotates_so_a_different_character_is_the_favourite_each_term() {
    // With nobody standing, the count over terms one to six gives different winners.
    let c = content();
    let mut winners = std::collections::BTreeSet::new();
    for term in 1..=6u32 {
        let mut w = World::new(0);
        w.term = term;
        w.last_day = c.social.rules.tally_day;
        winners.insert(russh_bbs_door_hallpass::social::tally(&c, &w).unwrap().id);
    }
    assert!(winners.len() >= 3, "{winners:?}");
}
