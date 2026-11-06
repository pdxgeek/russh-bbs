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

use common::bot::{Policy, balanced, cautious, play_day, reckless, scholar, variety};
use common::{content, monday};
use russh_bbs_door::DoorRng;
use russh_bbs_door_hallpass::game::{Action, Event, Game};
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
}

/// One whole term on one shared school with one player per policy.
fn term(policies: &[Policy], traits: &[&str], seed: u64) -> (Vec<Outcome>, World) {
    let content = content();
    let start = monday();
    let mut world = World::new(start);
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
    let mut tick_rng = DoorRng::from_label("hallpass", seed as i64);
    for day in 0..content.rules.term_days {
        let today = start + day;
        for (i, g) in games.iter_mut().enumerate() {
            g.world = world.clone();
            g.sync_day(today);
            let events = play_day(g, &policies[i], &mut rngs[i]);
            for e in &events {
                if let Event::Pulled { scheme, .. } = e {
                    let _ = scheme;
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
            Outcome {
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
        assert!(o.curve.heat.iter().all(|h| *h <= 100));
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
