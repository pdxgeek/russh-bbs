//! Schemes: the plan, the setup, the run, heat, the file, legend, and staff who learn.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]
mod common;

use common::{Sandbox, Setup, arrange, busted, legend_of, pulled, run_scheme};
use russh_bbs_door_hallpass::game::{Action, Event, Phase};

fn salt_setup() -> Setup {
    Setup {
        know: vec!["staff_rota"],
        ..Setup::default()
    }
}

/// A seed for which the scenario ends the way the test needs, found by trying seeds.
fn seed_where(want: impl Fn(&[Event]) -> bool, build: impl Fn(u64) -> (Sandbox, u16)) -> u64 {
    (0..400)
        .find(|seed| {
            let (mut sb, id) = build(*seed);
            want(&run_scheme(&mut sb, id, 0))
        })
        .expect("some seed gives this outcome")
}

fn salt_scenario(trait_id: &str, seed: u64) -> (Sandbox, u16) {
    let mut sb = Sandbox::new(trait_id, seed);
    let id = arrange(&mut sb, "pell", "salt_swap", &salt_setup());
    (sb, id)
}

#[test]
fn a_scheme_runs_from_plan_to_aftermath_and_everything_it_touches_changes() {
    let seed = seed_where(|e| pulled(e), |s| salt_scenario("smooth_talker", s));
    let (mut sb, id) = salt_scenario("smooth_talker", seed);
    let before = sb.game.player.clone();
    let events = run_scheme(&mut sb, id, 0);
    let p = &sb.game.player;
    assert!(pulled(&events));
    assert!(p.legend > before.legend, "legend is the score");
    assert!(p.heat > before.heat, "every scheme adds heat");
    assert_eq!(p.pulled, 1);
    assert!(p.schemes.is_empty() && p.run.is_none(), "the plan is spent");
    assert_eq!(p.wary_of("pell"), 1, "the target has started to learn");
    assert_eq!(p.count("salt"), 0, "the items are used up");
    assert!(
        p.campaign
            .as_ref()
            .is_some_and(|c| c.links == 1 && c.theme == "pell")
    );
    assert!(p.slot > before.slot, "the run took the period");
    let drained = sb.game.take_world_changes();
    assert!(
        drained
            .iter()
            .any(|d| matches!(d, russh_bbs_door_hallpass::world::WorldDelta::Hall { .. }))
    );
}

#[test]
fn big_schemes_need_legend_and_setup_before_they_can_run() {
    let mut sb = Sandbox::new("smooth_talker", 1);
    assert!(
        sb.try_(Action::Draft {
            target: "marsh".into(),
            method: "move_the_deputy".into()
        })
        .is_err()
    );
    sb.game.player.legend = 25;
    sb.ok(Action::Draft {
        target: "marsh".into(),
        method: "move_the_deputy".into(),
    });
    let id = sb.game.player.schemes[0].id;
    let scheme = sb.game.player.scheme(id).unwrap().clone();
    let r = sb.game.readiness(&scheme);
    assert_eq!(r.missing_items, ["memo_set", "forgery_kit"]);
    assert_eq!(r.missing_roles, ["cover"]);
    assert_eq!((r.scout_have, r.scout_need), (0, 2));
    assert!(!r.scheduled && !r.ready());
    sb.game.player.slot = 5;
    sb.game.player.place = "admin_corridor".into();
    assert!(sb.try_(Action::BeginRun(id)).is_err(), "not ready");
}

#[test]
fn times_come_from_the_target_schedule() {
    let sb = Sandbox::new("smooth_talker", 1);
    let mut sb = sb;
    sb.ok(Action::Draft {
        target: "pell".into(),
        method: "salt_swap".into(),
    });
    let scheme = sb.game.player.schemes[0].clone();
    let times = sb.game.candidate_times(&scheme);
    assert!(!times.is_empty());
    let pell = sb.game.content.npc("pell").unwrap().clone();
    for (day, slot) in &times {
        assert_ne!(
            sb.game.npc_place(&pell, *day, usize::from(*slot)),
            "staffroom",
            "the salt goes in while the maths master is elsewhere"
        );
        assert!(
            sb.game
                .content
                .place("staffroom")
                .unwrap()
                .hours
                .iter()
                .any(|h| h == sb.game.kind(*day, usize::from(*slot)))
        );
    }
    // Once Mr. Marsh has been moved to the Old Wing, a trick in the admin corridor can be timed
    // for the periods he used to patrol it.
    sb.game.player.legend = 25;
    sb.ok(Action::Draft {
        target: "marsh".into(),
        method: "move_the_deputy".into(),
    });
    let deputy = sb.game.player.schemes.last().unwrap().clone();
    let before = sb.game.candidate_times(&deputy);
    sb.game
        .world
        .active
        .push(russh_bbs_door_hallpass::state::Active {
            id: "deputy_moved".into(),
            since: 0,
            until: None,
            by: "bea".into(),
        });
    let after = sb.game.candidate_times(&deputy);
    assert!(before.iter().all(|t| after.contains(t)));
    assert!(
        after.len() > before.len(),
        "his old free-period patrol no longer rules those periods out"
    );
}

#[test]
fn staff_who_are_pranked_learn_and_the_same_trick_lands_softer() {
    let seed = seed_where(|e| pulled(e), |s| salt_scenario("smooth_talker", s));
    let (mut sb, id) = salt_scenario("smooth_talker", seed);
    let first = run_scheme(&mut sb, id, 0);
    assert!(pulled(&first));
    // The same trick on the same teacher, the next week.
    let day = sb.game.player.day + 7;
    sb.game.sync_day(day);
    sb.give("salt", 1);
    sb.game.player.nerve = sb.game.player.nerve_max;
    sb.ok(Action::Draft {
        target: "pell".into(),
        method: "salt_swap".into(),
    });
    let id2 = sb.game.player.schemes.last().unwrap().id;
    let s2 = sb.game.player.scheme(id2).unwrap().clone();
    let fresh = Sandbox::new("smooth_talker", 1);
    let mut fresh = fresh;
    fresh.game.player.learn("staff_rota");
    fresh.ok(Action::Draft {
        target: "pell".into(),
        method: "salt_swap".into(),
    });
    let s_fresh = fresh.game.player.schemes[0].clone();
    // Pell was wary at level 1 and decays after a week, but the old trick still counts against it.
    let now = sb.game.chance(&s2, Phase::Act, 0, 0).unwrap();
    let then = fresh.game.chance(&s_fresh, Phase::Act, 0, 0).unwrap();
    assert!(now.total < then.total, "{now:?} vs {then:?}");
    assert!(now.parts.iter().any(|(n, v)| n == "an old trick" && *v < 0));
}

#[test]
fn wariness_makes_a_target_harder_and_demands_more_scouting_and_then_fades() {
    let mut sb = Sandbox::new("smooth_talker", 1);
    sb.ok(Action::Draft {
        target: "orrin".into(),
        method: "salt_swap".into(),
    });
    let s = sb.game.player.schemes[0].clone();
    let calm = sb.game.chance(&s, Phase::Act, 0, 0).unwrap().total;
    assert_eq!(sb.game.required_scout(&s), 0);
    sb.game.bump_wary("orrin", 3);
    let wary = sb.game.chance(&s, Phase::Act, 0, 0).unwrap().total;
    assert_eq!(calm - wary, 15, "five points of chance per level");
    assert_eq!(
        sb.game.required_scout(&s),
        1,
        "at level 3 they check the sugar jar"
    );
    sb.game.bump_wary("orrin", 2);
    assert_eq!(sb.game.required_scout(&s), 2);
    // Without further mischief the staff relax, one level a week.
    let day = sb.game.player.day + 7 * 3;
    sb.game.sync_day(day);
    assert!(sb.game.player.wary_of("orrin") < 5);
    sb.game.sync_day(day + 7 * 7);
    assert_eq!(sb.game.player.wary_of("orrin"), 0);
}

#[test]
fn heat_builds_with_mischief_fades_with_time_and_makes_everything_worse() {
    let seed = seed_where(|e| pulled(e), |s| salt_scenario("smooth_talker", s));
    let (mut sb, id) = salt_scenario("smooth_talker", seed);
    let s_before = {
        let mut other = Sandbox::new("smooth_talker", 1);
        other.ok(Action::Draft {
            target: "alder".into(),
            method: "salt_swap".into(),
        });
        other
    };
    run_scheme(&mut sb, id, 0);
    let heat = sb.game.player.heat;
    assert!(heat >= 6);
    // With heat on, a fresh scheme against someone else is harder and likelier to be traced.
    sb.give("salt", 1);
    sb.ok(Action::Draft {
        target: "alder".into(),
        method: "salt_swap".into(),
    });
    let hot = sb.game.player.schemes.last().unwrap().clone();
    let cold = s_before.game.player.schemes[0].clone();
    assert!(
        sb.game.chance(&hot, Phase::Act, 0, 0).unwrap().total
            <= s_before.game.chance(&cold, Phase::Act, 0, 0).unwrap().total
    );
    assert!(sb.game.trace_chance(&hot, 0) > s_before.game.trace_chance(&cold, 0));
    // Time heals: heat decays by the rules' amount each day.
    let day = sb.game.player.day + 1;
    sb.game.sync_day(day);
    assert_eq!(
        i32::from(sb.game.player.heat),
        (i32::from(heat) - sb.game.content.rules.heat_decay).max(0)
    );
    sb.game.sync_day(day + 10);
    assert_eq!(sb.game.player.heat, 0);
}

#[test]
fn getting_caught_costs_the_file_a_detention_and_the_campaign() {
    let seed = seed_where(|e| busted(e), |s| salt_scenario("smooth_talker", s));
    let (mut sb, id) = salt_scenario("smooth_talker", seed);
    sb.game.player.campaign = Some(russh_bbs_door_hallpass::state::Campaign {
        theme: "pell".into(),
        links: 2,
        last_day: sb.game.player.day,
    });
    let events = run_scheme(&mut sb, id, 0);
    let p = &sb.game.player;
    assert!(busted(&events) && !pulled(&events));
    assert_eq!(p.caught, 1);
    assert!(p.campaign.is_none(), "a bust ends the campaign");
    assert!(p.detention >= 1);
    assert_eq!(
        p.file.last().map(|e| (e.kind.as_str(), e.sev)),
        Some(("caught", 2))
    );
    assert_eq!(p.legend, 0, "no legend for a failure");
    assert!(
        p.wary_of("pell") >= 2,
        "a bust teaches the target twice as much"
    );
    assert!(sb.game.take_world_changes().iter().any(|d| matches!(
        d,
        russh_bbs_door_hallpass::world::WorldDelta::Patrol { amount: 3, .. }
    )));
}

#[test]
fn chained_pranks_are_worth_more_than_one_offs() {
    let seed = seed_where(|e| pulled(e), |s| salt_scenario("smooth_talker", s));
    let (mut one_off, id) = salt_scenario("smooth_talker", seed);
    let single = legend_of(&run_scheme(&mut one_off, id, 0));
    // The same trick, but as the second link of a campaign against the same target.
    let (mut chained, id) = salt_scenario("smooth_talker", seed);
    chained.game.player.campaign = Some(russh_bbs_door_hallpass::state::Campaign {
        theme: "pell".into(),
        links: 2,
        last_day: chained.game.player.day,
    });
    let linked = run_scheme(&mut chained, id, 0);
    assert!(pulled(&linked));
    assert!(
        legend_of(&linked) > single,
        "{} vs {single}",
        legend_of(&linked)
    );
    // A different target does not extend the chain.
    let (mut other, id) = salt_scenario("smooth_talker", seed);
    other.game.player.campaign = Some(russh_bbs_door_hallpass::state::Campaign {
        theme: "orrin".into(),
        links: 2,
        last_day: other.game.player.day,
    });
    assert_eq!(legend_of(&run_scheme(&mut other, id, 0)), single);
    assert!(
        other
            .game
            .player
            .campaign
            .as_ref()
            .is_some_and(|c| c.theme == "pell" && c.links == 1)
    );
}

#[test]
fn nerve_buys_a_better_chance_but_only_what_you_have() {
    let mut sb = Sandbox::new("smooth_talker", 1);
    let id = arrange(&mut sb, "pell", "salt_swap", &salt_setup());
    let s = sb.game.player.scheme(id).unwrap().clone();
    let base = sb.game.chance(&s, Phase::Act, 0, 0).unwrap().total;
    let pushed = sb.game.chance(&s, Phase::Act, 3, 0).unwrap().total;
    assert_eq!(pushed - base, 3 * sb.game.content.rules.nerve_push_bonus);
    // Earlier slips in the same run make the rest harder.
    assert!(sb.game.chance(&s, Phase::Act, 0, 2).unwrap().total < base);
    let (day, slot) = s.when.unwrap();
    sb.game.sync_day(day);
    sb.game.player.slot = slot;
    sb.game.player.place = sb.game.scheme_place(&s).unwrap();
    sb.game.player.nerve = 1;
    sb.ok(Action::BeginRun(id));
    assert!(
        sb.try_(Action::RunPhase { push: 3 }).is_err(),
        "you cannot spend nerve you do not have"
    );
    assert!(
        sb.try_(Action::Go("quad".into())).is_err(),
        "nothing else while a scheme is under way"
    );
    sb.ok(Action::RunPhase { push: 1 });
    assert_eq!(sb.game.player.nerve, 0);
}

#[test]
fn a_cover_story_turns_a_bust_into_a_slip_once() {
    let cover = Setup {
        allies: vec![("wren", "cover")],
        ..salt_setup()
    };
    let with_cover = |seed| {
        let mut sb = Sandbox::new("smooth_talker", seed);
        let id = arrange(&mut sb, "pell", "salt_swap", &cover);
        (sb, id)
    };
    // A seed where the plain run is caught in some phase...
    let seed = seed_where(|e| busted(e), |s| salt_scenario("smooth_talker", s));
    let (mut plain, id) = salt_scenario("smooth_talker", seed);
    let plain_events = run_scheme(&mut plain, id, 0);
    assert!(busted(&plain_events));
    // ...is saved by the cover ally on the same dice (the cover also changes nothing else in the odds).
    let (mut covered, id) = with_cover(seed);
    let events = run_scheme(&mut covered, id, 0);
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Phase {
                result: russh_bbs_door_hallpass::game::PhaseResult::Covered,
                ..
            }
        )),
        "{events:?}"
    );
    // The cover is used up by the first near-miss; the run goes on and is judged on what is left.
    assert!(covered.game.player.caught <= plain.game.player.caught);
    assert!(covered.game.player.file_points() <= plain.game.player.file_points());
}
