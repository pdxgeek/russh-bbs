//! The RPG rule: every element changes outcomes. The same scheme, played by different builds
//! (traits, skills, allies), offers different options and ends differently.
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

use common::{Sandbox, Setup, arrange, busted, monday, pulled, run_scheme};
use russh_bbs_door_hallpass::game::{Action, Phase};
use russh_bbs_door_hallpass::state::{Status, map_set};

const START_TRAITS: [&str; 4] = ["smooth_talker", "teachers_pet", "fast_runner", "tinkerer"];

fn draft_ids(sb: &Sandbox) -> Vec<(String, String)> {
    sb.game.drafts()
}

fn available(sb: &Sandbox) -> (Vec<(String, String)>, Vec<String>, Vec<String>) {
    let twists = sb
        .game
        .content
        .twists
        .iter()
        .filter(|t| sb.game.all_hold(&t.requires))
        .map(|t| t.id.clone())
        .collect();
    let getaways = sb
        .game
        .content
        .getaways
        .iter()
        .filter(|g| sb.game.all_hold(&g.requires))
        .map(|g| g.id.clone())
        .collect();
    (draft_ids(sb), twists, getaways)
}

#[test]
fn different_builds_see_different_choices_on_the_same_board() {
    let builds: Vec<_> = START_TRAITS
        .iter()
        .map(|t| available(&Sandbox::new(t, 1)))
        .collect();
    for (i, a) in builds.iter().enumerate() {
        for b in &builds[i + 1..] {
            assert_ne!(a, b, "two starting traits must not offer the same options");
        }
    }
    let has = |t: &str, method: &str| {
        draft_ids(&Sandbox::new(t, 1))
            .iter()
            .any(|(_, m)| m == method)
    };
    assert!(has("smooth_talker", "forged_note"), "a talker can forge");
    for t in ["teachers_pet", "fast_runner", "tinkerer"] {
        assert!(!has(t, "forged_note"), "{t} cannot");
    }
    let twist = |t: &str, id: &str| available(&Sandbox::new(t, 1)).1.iter().any(|x| x == id);
    assert!(twist("tinkerer", "timed") && !twist("smooth_talker", "timed"));
    let getaway = |t: &str, id: &str| available(&Sandbox::new(t, 1)).2.iter().any(|x| x == id);
    assert!(getaway("fast_runner", "sprint") && !getaway("tinkerer", "sprint"));
}

#[test]
fn who_will_help_you_depends_on_who_you_are() {
    let recruit = |trait_id: &str, bench: i32, npc_place: &str| {
        let mut sb = Sandbox::new(trait_id, 1);
        sb.game.player.add_standing("the_bench", bench);
        sb.game.player.add_standing("strays", 10);
        sb.ok(Action::Draft {
            target: "pell".into(),
            method: "salt_swap".into(),
        });
        sb.game.player.slot = 5;
        sb.game.player.place = npc_place.into();
        let scheme = sb.game.player.schemes[0].clone();
        sb.game
            .recruitable(&scheme)
            .into_iter()
            .map(|(n, r, _)| (n, r))
            .collect::<Vec<_>>()
    };
    let sol = ("sol".to_owned(), "lookout".to_owned());
    assert!(
        recruit("fast_runner", 13, "quad").contains(&sol),
        "the back row will watch a door for a runner"
    );
    assert!(
        !recruit("teachers_pet", 30, "quad").contains(&sol),
        "never for a teacher's pet, whatever their standing"
    );
    assert!(
        !recruit("fast_runner", 0, "quad").contains(&sol),
        "nor for someone they do not know"
    );
    let mags = ("mags".to_owned(), "lookout".to_owned());
    assert!(
        recruit("teachers_pet", 0, "library").contains(&mags),
        "the Strays help anyone who treats them decently"
    );
}

fn pull_rate(seeds: u64, build: impl Fn(u64) -> (Sandbox, u16), push: u8) -> (f64, f64) {
    let mut pulled_n = 0u32;
    let mut busted_n = 0u32;
    for seed in 0..seeds {
        let (mut sb, id) = build(seed);
        let events = run_scheme(&mut sb, id, push);
        pulled_n += u32::from(pulled(&events));
        busted_n += u32::from(busted(&events));
    }
    (
        f64::from(pulled_n) / seeds as f64,
        f64::from(busted_n) / seeds as f64,
    )
}

#[test]
fn the_same_scheme_ends_differently_for_a_tinkerer_and_a_runner() {
    // A stink bomb is a tinker act and a sprint away: the two builds are good at different halves.
    let build = |trait_id: &'static str, getaway: Option<&'static str>| {
        move |seed| {
            let mut sb = Sandbox::new(trait_id, seed);
            let id = arrange(
                &mut sb,
                "pell",
                "stink_bomb",
                &Setup {
                    getaway,
                    ..Setup::default()
                },
            );
            (sb, id)
        }
    };
    let tinkerer = build("tinkerer", None)(1);
    let runner = build("fast_runner", Some("sprint"))(1);
    let (ts, rs) = (
        tinkerer.0.game.player.schemes[0].clone(),
        runner.0.game.player.schemes[0].clone(),
    );
    let chance =
        |g: &russh_bbs_door_hallpass::game::Game, s: &_, p| g.chance(s, p, 0, 0).unwrap().total;
    assert!(
        chance(&tinkerer.0.game, &ts, Phase::Act) > chance(&runner.0.game, &rs, Phase::Act),
        "the tinkerer is better at the deed"
    );
    assert!(
        chance(&runner.0.game, &rs, Phase::Escape) > chance(&tinkerer.0.game, &ts, Phase::Escape),
        "the runner is better at the exit"
    );
    // And the difference shows in where the failures happen, over the same 400 dice.
    let fail_phase = |b: &dyn Fn(u64) -> (Sandbox, u16)| {
        let (mut act, mut escape) = (0, 0);
        for seed in 0..400 {
            let (mut sb, id) = b(seed);
            for e in run_scheme(&mut sb, id, 0) {
                if let russh_bbs_door_hallpass::game::Event::Busted { phase, .. } = e {
                    match phase {
                        Phase::Act => act += 1,
                        Phase::Escape => escape += 1,
                        Phase::Sneak => {}
                    }
                }
            }
        }
        (act, escape)
    };
    let (t_act, t_escape) = fail_phase(&build("tinkerer", None));
    let (r_act, r_escape) = fail_phase(&build("fast_runner", Some("sprint")));
    assert!(
        t_act < r_act,
        "tinkerer busts at the act less ({t_act} vs {r_act})"
    );
    assert!(
        r_escape < t_escape,
        "runner busts at the escape less ({r_escape} vs {t_escape})"
    );
}

#[test]
fn allies_and_gear_change_the_odds_and_the_result() {
    let lone = |seed| {
        let mut sb = Sandbox::new("tinkerer", seed);
        let id = arrange(&mut sb, "orrin", "stink_bomb", &Setup::default());
        (sb, id)
    };
    let backed = |seed| {
        let mut sb = Sandbox::new("tinkerer", seed);
        let id = arrange(
            &mut sb,
            "orrin",
            "stink_bomb",
            &Setup {
                twists: vec!["decoy", "gadget"],
                scout: 3,
                allies: vec![
                    ("sol", "lookout"),
                    ("sol", "distraction"),
                    ("pip", "gadget"),
                ],
                ..Setup::default()
            },
        );
        (sb, id)
    };
    let (mut a, ia) = lone(1);
    let (mut b, ib) = backed(1);
    let sa = a.game.player.scheme(ia).unwrap().clone();
    let sb_ = b.game.player.scheme(ib).unwrap().clone();
    let ca = a.game.chance(&sa, Phase::Sneak, 0, 0).unwrap();
    let cb = b.game.chance(&sb_, Phase::Sneak, 0, 0).unwrap();
    assert!(cb.total > ca.total + 20, "{} vs {}", cb.total, ca.total);
    assert!(cb.parts.iter().any(|(n, v)| n == "a lookout" && *v == 10));
    assert!(cb.parts.iter().any(|(n, v)| n == "scouting" && *v == 15));
    let (lone_rate, lone_bust) = pull_rate(300, lone, 0);
    let (backed_rate, backed_bust) = pull_rate(300, backed, 0);
    assert!(
        backed_rate > lone_rate + 0.15,
        "pulled off: {backed_rate:.2} vs {lone_rate:.2}"
    );
    assert!(
        backed_bust < lone_bust,
        "caught: {backed_bust:.2} vs {lone_bust:.2}"
    );
    // Nerve is a resource that buys odds for the moment.
    let (pushed_rate, _) = pull_rate(300, lone, 2);
    assert!(
        pushed_rate > lone_rate + 0.1,
        "{pushed_rate:.2} vs {lone_rate:.2}"
    );
    // Reading the two screens' numbers does not change them.
    let _ = (&mut a, &mut b);
}

#[test]
fn skills_improve_by_use_and_the_odds_follow() {
    let mut sb = Sandbox::new("smooth_talker", 4);
    let id = arrange(
        &mut sb,
        "pell",
        "salt_swap",
        &Setup {
            know: vec!["staff_rota"],
            ..Setup::default()
        },
    );
    assert_eq!(sb.game.skill_level("sneak"), 0);
    // Doing is how skills grow: each phase gained a point of experience in its skill.
    let events = run_scheme(&mut sb, id, 0);
    let xp = |sb: &Sandbox, skill: &str| {
        russh_bbs_door_hallpass::state::map_get(&sb.game.player.xp, skill)
            .copied()
            .unwrap_or(0)
    };
    assert!(xp(&sb, "sneak") >= 1, "{events:?}");
    // Enough practice and the level, and with it the chance, goes up.
    map_set(&mut sb.game.player.xp, "sneak", 3);
    assert_eq!(sb.game.skill_level("sneak"), 1);
    sb.give("salt", 1);
    sb.game.sync_day(sb.game.player.day + 14);
    sb.ok(Action::Draft {
        target: "alder".into(),
        method: "salt_swap".into(),
    });
    let s2 = sb.game.player.schemes.last().unwrap().clone();
    let mut fresh = Sandbox::new("smooth_talker", 4);
    fresh.ok(Action::Draft {
        target: "alder".into(),
        method: "salt_swap".into(),
    });
    let s3 = fresh.game.player.schemes[0].clone();
    // Not the same day, so compare like with like: the skill part only.
    let part = |sb: &Sandbox, s: &_| {
        sb.game
            .chance(s, Phase::Sneak, 0, 0)
            .unwrap()
            .parts
            .iter()
            .find(|(n, _)| n.starts_with("skill"))
            .unwrap()
            .1
    };
    assert_eq!(part(&sb, &s2), part(&fresh, &s3) + 6);
}

#[test]
fn choices_close_doors_for_the_rest_of_the_term() {
    // Becoming a known troublemaker closes the innocent getaway and the teacher's-pet trust.
    let mut sb = Sandbox::new("fast_runner", 1);
    let blend = |sb: &Sandbox| {
        sb.game
            .content
            .getaways
            .iter()
            .any(|g| g.id == "blend_in" && sb.game.all_hold(&g.requires))
    };
    assert!(blend(&sb));
    sb.game.player.notoriety = 60;
    sb.game.check_traits();
    assert!(!blend(&sb), "no one believes the picture of innocence now");
    // A teacher who has caught on closes the unscouted route.
    sb.ok(Action::Draft {
        target: "orrin".into(),
        method: "salt_swap".into(),
    });
    let s = sb.game.player.schemes[0].clone();
    sb.game.bump_wary("orrin", 3);
    assert!(!sb.game.readiness(&s).ready());
    assert_eq!(sb.game.readiness(&s).scout_need, 1);
    // A big scheme already done closes nothing but is remembered: an old trick halves its legend.
    let seed = (0..200)
        .find(|seed| {
            let mut s = Sandbox::new("smooth_talker", *seed);
            let id = arrange(
                &mut s,
                "pell",
                "salt_swap",
                &Setup {
                    know: vec!["staff_rota"],
                    ..Setup::default()
                },
            );
            pulled(&run_scheme(&mut s, id, 0))
        })
        .unwrap();
    let mut s = Sandbox::new("smooth_talker", seed);
    let id = arrange(
        &mut s,
        "pell",
        "salt_swap",
        &Setup {
            know: vec!["staff_rota"],
            ..Setup::default()
        },
    );
    run_scheme(&mut s, id, 0);
    assert_eq!(
        russh_bbs_door_hallpass::state::map_get(&s.game.player.done, "pell/salt_swap"),
        Some(&1)
    );
}

#[test]
fn consequences_persist_across_days_and_across_a_save() {
    let mut sb = Sandbox::new("smooth_talker", 1);
    sb.game.add_file("caught", 3);
    sb.game.bump_wary("vane", 2);
    sb.game.player.legend = 17;
    sb.game.player.heat = 40;
    let next = monday() + 1;
    sb.game.sync_day(next);
    let p = sb.game.player.clone();
    assert_eq!(p.file_points(), 3, "the file does not vanish overnight");
    assert_eq!(p.wary_of("vane"), 2);
    assert_eq!(p.legend, 17);
    assert!(p.heat > 0 && p.heat < 40, "heat cools but does not vanish");
    assert_eq!(p.due, 1, "the detention is owed tomorrow, not today");
    // Through the save format and back, and into the next visit, it is all still there.
    let bytes = russh_bbs_door_hallpass::save::encode_player(&p);
    let loaded = russh_bbs_door_hallpass::save::decode_player(&bytes).unwrap();
    assert_eq!(loaded, p);
    let (game, _) = russh_bbs_door_hallpass::game::Game::open(
        &common::content(),
        "ann",
        Some(loaded),
        None,
        next,
    );
    assert_eq!(game.player.file, p.file);
    assert_eq!(game.player.status, Status::Active);
}

#[test]
fn no_content_is_dead_every_option_is_open_to_some_build() {
    let content = common::content();
    let mut everything = Sandbox::new("smooth_talker", 1);
    for t in ["tinkerer", "fast_runner"] {
        everything.game.player.traits.push(t.to_owned());
    }
    everything.game.player.traits.push("ghost".into());
    everything.give("greasepaint", 1);
    everything.give("servo", 1);
    for club in ["robotics", "rowing"] {
        everything
            .game
            .player
            .life
            .clubs
            .push(russh_bbs_door_hallpass::state::Membership {
                club: club.into(),
                rank: 2,
                merit: 5,
                last_meet: -1,
            });
    }
    everything.game.player.legend = 100;
    everything.game.player.notoriety = 10;
    for k in [
        "staff_rota",
        "back_stairs",
        "organ_key_spot",
        "caretaker_rounds",
    ] {
        everything.game.player.learn(k);
    }
    for s in ["sneak", "bluff", "tinker", "fleet"] {
        map_set(&mut everything.game.player.xp, s, 40);
    }
    let (drafts, twists, getaways) = available(&everything);
    for m in &content.methods {
        if drafts.iter().any(|(_, id)| *id == m.id) {
            continue;
        }
        // Some methods are for one kind of pupil and not for another (a teacher's pet is never
        // offered the owl badges). Each must still be open to a pupil who meets exactly its own
        // requirements: build that pupil and ask.
        let mut build = Sandbox::new("smooth_talker", 1);
        build.game.world.term_start = build.game.player.day - 20;
        for cond in &m.requires {
            common::satisfy(&mut build.game, cond);
        }
        assert!(
            draft_ids(&build).iter().any(|(_, id)| *id == m.id),
            "method {} is unreachable for any build",
            m.id
        );
    }
    for t in &content.twists {
        assert!(twists.contains(&t.id), "twist {} is unreachable", t.id);
    }
    for g in &content.getaways {
        // `blend_in` is for those with a clean name: reachable by someone without the trait.
        let clean = Sandbox::new("teachers_pet", 1);
        assert!(
            getaways.contains(&g.id) || available(&clean).2.contains(&g.id),
            "getaway {} is unreachable",
            g.id
        );
    }
    for tr in &content.traits {
        assert!(
            tr.start || !tr.earn.is_empty(),
            "trait {} cannot be had",
            tr.id
        );
    }
}
