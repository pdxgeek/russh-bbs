//! The fixers' arcs, the endings and the content budget: schema, reachability of every step,
//! choice and ending, branch replays (a build and a history open different choices; a betrayal
//! closes a door), the season's endings and the characters' epilogues.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names
)]

use std::collections::{BTreeSet, VecDeque};

use russh_bbs_door::DoorKey;
use russh_bbs_door::content::{self, check};
use russh_bbs_door_nullroute::cond::{Cond, Term};
use russh_bbs_door_nullroute::content::{CmKind, Content, Role};
use russh_bbs_door_nullroute::player::Player;
use russh_bbs_door_nullroute::social::Tallies;
use russh_bbs_door_nullroute::story::{
    ARC_TMPL, ARCS_TOML, LinesTable, SEASONS_TOML, SOCIAL_TOML, SeasonsTable, StoryTable, arc_id,
};
use russh_bbs_door_nullroute::world::Contract;

#[test]
fn the_story_tables_pass_their_schema_tests_and_a_broken_edit_is_refused() {
    assert_eq!(check::<StoryTable>("arcs", ARCS_TOML), Ok(()));
    assert_eq!(check::<SeasonsTable>("seasons", SEASONS_TOML), Ok(()));
    assert_eq!(check::<LinesTable>("social", SOCIAL_TOML), Ok(()));
    // A choice that points at a step that is not there.
    let broken = ARCS_TOML.replacen("next = 2", "next = 99", 1);
    assert!(matches!(
        check::<StoryTable>("arcs", &broken),
        Err(content::ContentError::Invalid { .. })
    ));
    // A condition with a typo.
    let broken = ARCS_TOML.replacen("rep>=10", "reputation>=10", 1);
    assert!(check::<StoryTable>("arcs", &broken).is_err());
    // A choice that goes backwards would make the arc loop.
    let mut text = ARCS_TOML.to_owned();
    text = text.replacen("next = 3", "next = 1", 1);
    assert!(check::<StoryTable>("arcs", &text).is_err());
    // A line that names a placeholder the game does not fill.
    let lines = SOCIAL_TOML.replacen("Your hack on {a} is filed", "Your hack on {b} is filed", 1);
    assert!(check::<LinesTable>("social", &lines).is_err());
    // A step objective the engine could not do (copy at a console).
    let impossible = ARCS_TOML.replacen(
        "target = \"VT\"\neffect = \"copy\"",
        "target = \"AC\"\neffect = \"copy\"",
        1,
    );
    assert!(check::<StoryTable>("arcs", &impossible).is_err());
}

#[test]
fn the_launch_content_budget_is_met_except_where_it_says_it_is_not() {
    let c = Content::try_embedded().unwrap();
    assert_eq!(c.corps.corp.len(), 12, "corporations");
    assert_eq!(c.nodes.node.len(), 30, "node types");
    assert_eq!(
        c.cms.cm.iter().filter(|m| m.kind != CmKind::Warden).count(),
        20,
        "countermeasures"
    );
    assert_eq!(
        c.cms.cm.iter().filter(|m| m.kind == CmKind::Warden).count(),
        3
    );
    assert_eq!(c.scripts.script.len(), 60, "scripts");
    assert_eq!(c.fragments.fragment.len(), 30, "fragments");
    assert_eq!(c.contracts.contract.len(), 80, "contract templates");
    assert_eq!(c.story.arc.len(), 5, "fixer arcs");
    assert_eq!(c.corps.fixer.len(), 5, "one fixer for each arc");
    assert_eq!(c.story.step.len(), 35, "arc contracts: about 35");
    assert!(c.story.ending.len() >= 15, "three endings an arc");
    assert!(c.seasons.season_ending.len() >= 6 && c.seasons.epilogue.len() >= 8);
    // Every corporation has its own home template of node types (three each), and every node type
    // that names a corporation names one that exists.
    for corp in &c.corps.corp {
        let own = c
            .nodes
            .node
            .iter()
            .filter(|n| n.corp == Some(corp.num))
            .count();
        assert!(
            own == 3 || corp.num < 5,
            "{} has {own} home node types",
            corp.name
        );
    }
    assert!(
        c.nodes
            .node
            .iter()
            .filter_map(|n| n.corp)
            .all(|n| c.corp(n).is_some())
    );
    assert!(c.nodes.node.iter().filter(|n| n.role == Role::Mid).count() >= 20);
    // Falling short, said plainly: the design's 40 rig parts and 15 hideout upgrades are not
    // built (five parts of three levels, and the hideout's four buildings). This test pins the
    // number so the day it is built, the test says so.
    assert_eq!(
        c.rig.part.len() * 3,
        15,
        "rig parts: 15 levels of 5 parts, not 40"
    );
}

/// Every (step, flags) state an arc can be in, assuming each non-flag condition is one a
/// character can meet. The walk is the proof that no step, choice or ending is dead content.
fn walk(c: &Content, arc: u8) -> (BTreeSet<u8>, BTreeSet<(u8, usize)>, BTreeSet<u8>) {
    let flags = &c.story.flags;
    let def = c.arc(arc).unwrap();
    let mut steps = BTreeSet::new();
    let mut choices = BTreeSet::new();
    let mut endings = BTreeSet::new();
    let mut seen: BTreeSet<(u8, u32)> = BTreeSet::new();
    let mut queue = VecDeque::from([(def.first, 0u32)]);
    while let Some((step, bits)) = queue.pop_front() {
        if !seen.insert((step, bits)) {
            continue;
        }
        steps.insert(step);
        let s = c.step(arc, step).unwrap();
        for (k, ch) in s.choice.iter().enumerate() {
            // The flag terms of the gate must hold; the rest are about the build.
            let cond = Cond::parse(&ch.gate, flags).unwrap();
            let ok = cond.terms().iter().all(|(not, term)| match term {
                Term::Flag(b) => (bits & (1 << b) != 0) != *not,
                _ => true,
            });
            if !ok {
                continue;
            }
            choices.insert((step, k));
            let mut next_bits = bits;
            for f in &ch.set {
                next_bits |= 1 << flags.iter().position(|x| x == f).unwrap();
            }
            for f in &ch.clear {
                next_bits &= !(1 << flags.iter().position(|x| x == f).unwrap());
            }
            if ch.ending != 0 {
                endings.insert(ch.ending);
            } else {
                queue.push_back((ch.next, next_bits));
            }
        }
    }
    (steps, choices, endings)
}

#[test]
fn every_step_every_choice_and_every_ending_of_every_arc_can_be_reached() {
    let c = Content::try_embedded().unwrap();
    for arc in &c.story.arc {
        let (steps, choices, endings) = walk(&c, arc.num);
        let all_steps: BTreeSet<u8> = c
            .story
            .step
            .iter()
            .filter(|s| s.arc == arc.num)
            .map(|s| s.id)
            .collect();
        assert_eq!(steps, all_steps, "{}: a step nobody can reach", arc.title);
        let mut all_choices = BTreeSet::new();
        for s in c.story.step.iter().filter(|s| s.arc == arc.num) {
            for k in 0..s.choice.len() {
                all_choices.insert((s.id, k));
            }
        }
        assert_eq!(
            choices, all_choices,
            "{}: a choice nobody can make",
            arc.title
        );
        let all_endings: BTreeSet<u8> = c
            .story
            .ending
            .iter()
            .filter(|e| e.arc == arc.num)
            .map(|e| e.num)
            .collect();
        assert_eq!(
            endings, all_endings,
            "{}: an ending nobody can reach",
            arc.title
        );
        // Several endings, a path of five to eight contracts, and a choice before the end.
        assert!(all_endings.len() >= 3, "{}", arc.title);
        let longest = longest_path(&c, arc.num, arc.first);
        assert!(
            (5..=8).contains(&longest),
            "{}: {longest} contracts",
            arc.title
        );
    }
}

fn longest_path(c: &Content, arc: u8, step: u8) -> usize {
    let s = c.step(arc, step).unwrap();
    1 + s
        .choice
        .iter()
        .filter(|ch| ch.next != 0)
        .map(|ch| longest_path(c, arc, ch.next))
        .max()
        .unwrap_or(0)
}

/// A character who meets every build condition in the story.
fn maxed(c: &Content) -> Player {
    let mut p = Player::new(c, 1, "ann", 1, 20_000);
    p.rep = 200;
    p.rig = [3; 5];
    p.xp = [200; 5];
    p.marks = 5_000;
    p.ties.stats = [9; 8];
    p
}

#[test]
fn a_gated_choice_is_shut_to_a_fresh_character_and_open_to_a_built_one() {
    let c = Content::try_embedded().unwrap();
    // The first arc's fifth step offers "follow the trail yourself" to a character good at probing.
    let mut fresh = Player::new(&c, 1, "ann", 1, 20_000);
    fresh.rep = 30;
    fresh.ties.arcs[0].step = 5;
    fresh.ties.arcs[0].awaiting = true;
    let choices = fresh.arc_choices(&c, 0);
    let (label, why) = &choices[0];
    assert!(label.label.contains("Follow the trail"));
    assert!(why.as_ref().unwrap().contains("skill.probe>=2"), "{why:?}");
    assert!(choices[1].1.is_none(), "the plain choice is always open");
    assert!(fresh.arc_choose(&c, 0, 0).is_err());
    let mut skilled = fresh.clone();
    skilled.xp[0] = 20; // Probe 2
    assert!(skilled.arc_choices(&c, 0)[0].1.is_none());
    assert!(skilled.arc_choose(&c, 0, 0).is_ok());
    assert_eq!(skilled.ties.arcs[0].step, 6);
    // And the flags a character's history set open or shut the endings on the next step.
    let mut trusted = skilled.clone();
    trusted.ties.arcs[0].awaiting = true;
    let flag = c
        .story
        .flags
        .iter()
        .position(|f| f == "marrow_trusted")
        .unwrap();
    trusted.ties.flags |= 1 << flag;
    let open_trusted: Vec<bool> = trusted
        .arc_choices(&c, 0)
        .iter()
        .map(|x| x.1.is_none())
        .collect();
    let mut cheat = skilled.clone();
    cheat.ties.arcs[0].awaiting = true;
    let open_cheat: Vec<bool> = cheat
        .arc_choices(&c, 0)
        .iter()
        .map(|x| x.1.is_none())
        .collect();
    assert_ne!(
        open_trusted, open_cheat,
        "different histories open different endings"
    );
}

#[test]
fn every_ending_is_reached_by_a_replay_of_choices_and_it_changes_the_character() {
    let c = Content::try_embedded().unwrap();
    let mut reached = BTreeSet::new();
    for arc in &c.story.arc {
        // Depth first through the choices a maxed character can make, keeping the flags: each
        // path that ends is a replay of one playthrough.
        fn go(c: &Content, arc: u8, p: Player, depth: usize, reached: &mut BTreeSet<u8>) {
            let st = p.ties.arcs[usize::from(arc)];
            assert!(depth < 12, "an arc loops");
            if st.ending != 0 {
                reached.insert(st.ending);
                // The ending is remembered for good and the story says so.
                assert_ne!(p.ties.endings & (1 << st.ending), 0);
                assert!(
                    p.arc_contract(c, arc, 20_100).is_none(),
                    "an ended arc offers nothing"
                );
                return;
            }
            let step = if st.step == 0 {
                c.arc(arc).unwrap().first
            } else {
                st.step
            };
            let mut p = p;
            // The job for the step is offered (or the step is gated by the build); complete it.
            p.ties.arcs[usize::from(arc)].step = step;
            p.arc_done(c, arc, step);
            for k in 0..p.arc_choices(c, arc).len() {
                let mut q = p.clone();
                q.day = 20_001;
                if q.arc_choices(c, arc)[k].1.is_some() {
                    continue;
                }
                let before = (q.rep, q.marks, q.standing);
                q.arc_choose(c, arc, k).unwrap();
                let after = (q.rep, q.marks, q.standing);
                let ch = &c.step(arc, step).unwrap().choice[k];
                if ch.rep != 0 || ch.marks != 0 || !ch.standing.is_empty() {
                    assert_ne!(before, after, "a choice with consequences has them");
                }
                go(c, arc, q, depth + 1, reached);
            }
        }
        go(&c, arc.num, maxed(&c), 0, &mut reached);
    }
    let all: BTreeSet<u8> = c.story.ending.iter().map(|e| e.num).collect();
    assert_eq!(
        reached, all,
        "every ending is reachable by some playthrough"
    );
}

#[test]
fn a_betrayal_closes_a_fixers_door_for_the_season_and_a_new_season_opens_it_again() {
    let c = Content::try_embedded().unwrap();
    let mut p = maxed(&c);
    // Marrow's arc: burn the ledger, the second ending.
    p.ties.arcs[0].step = 6;
    p.ties.arcs[0].awaiting = true;
    let at = c
        .step(0, 6)
        .unwrap()
        .choice
        .iter()
        .position(|ch| ch.ending == 2)
        .unwrap();
    p.arc_choose(&c, 0, at).unwrap();
    assert_eq!(p.ties.arcs[0].ending, 2);
    assert_eq!(p.standing[0], -5);
    let job = Contract {
        id: 1,
        tmpl: 0,
        corp: 5,
        fixer: 0,
        tier: 1,
        reward: 50,
        expires: 20_100,
    };
    assert_eq!(
        p.closed(&c, &job).unwrap(),
        "Marrow will not deal with you."
    );
    // The board's jobs from the other fixers are unaffected.
    let other = Contract { fixer: 2, ..job };
    assert!(p.closed(&c, &other).is_none());
    // A new season starts the standings and the story over, and the character remembers it.
    let before = p.ties.endings;
    p.new_season(&c, 2, 20_500);
    assert_eq!(p.standing[0], 0);
    assert_eq!(p.ties.arcs[0].ending, 0);
    assert_eq!(
        p.ties.endings, before,
        "the endings once reached are not forgotten"
    );
    assert_eq!(p.ties.legend.len(), 1);
    assert!(p.closed(&c, &job).is_none());
}

#[test]
fn an_arc_job_is_offered_a_day_after_the_last_choice_and_only_once_open() {
    let c = Content::try_embedded().unwrap();
    let mut p = Player::new(&c, 1, "ann", 1, 20_000);
    // The first step is open at once; the step after waits for the next day and its own gate.
    let job = p.arc_contract(&c, 0, 20_000).unwrap();
    assert_eq!((job.tmpl, job.id), (ARC_TMPL, arc_id(0, 1)));
    p.day = 20_000;
    p.arc_done(&c, 0, 1);
    assert!(
        p.arc_contract(&c, 0, 20_000).is_none(),
        "a choice is waiting"
    );
    p.arc_choose(&c, 0, 0).unwrap();
    assert!(
        p.arc_contract(&c, 0, 20_000).is_none(),
        "the fixer is quiet until tomorrow"
    );
    assert_eq!(p.ties.arcs[0].step, 2);
    assert!(p.arc_contract(&c, 0, 20_001).is_some());
    // A step with a reputation gate stays shut until the character has it.
    p.ties.arcs[0].step = 3;
    p.ties.arcs[0].since = 20_001;
    assert!(p.arc_contract(&c, 0, 20_002).is_none());
    p.rep = 10;
    assert!(p.arc_contract(&c, 0, 20_002).is_some());
    // A hot character is turned away from a deep step, as from the board.
    p.ties.arcs[0].step = 6;
    p.rep = 100;
    p.heat = 70;
    let deep = p.arc_contract(&c, 0, 20_010).unwrap();
    assert_eq!(
        p.closed(&c, &deep).unwrap(),
        "Too hot: the fixer is staying away from you."
    );
}

#[test]
fn the_fixer_hands_over_code_and_a_story_job_runs_like_any_other() {
    let c = Content::try_embedded().unwrap();
    // The fifth step of the first arc hands over a warden lens.
    let mut p = Player::new(&c, 1, "ann", 1, 20_000);
    let given = p.arc_done(&c, 0, 5);
    assert_eq!(given, vec![14]);
    assert!(p.fragments.contains(&14));
    // A story job begins a run through the engine like a board job.
    let mut q = maxed(&c);
    q.owned.extend([6u8, 9, 12, 24, 25]);
    q.deck.clear();
    for n in [23u8, 1, 6, 9] {
        let _ = q.load(&c, n);
    }
    let job = q.arc_contract(&c, 0, 20_000).unwrap();
    let world = russh_bbs_door_nullroute::world::World::default();
    let run = q.begin_run(&c, 1, &world, &job);
    assert!(run.is_ok(), "{:?}", run.err());
}

#[test]
fn every_season_ending_and_every_epilogue_can_be_the_one_chosen() {
    let c = Content::try_embedded().unwrap();
    // Season endings: for each rule, tallies that satisfy it and fail every rule above it.
    let tallies = |f: &dyn Fn(&mut Tallies)| {
        let mut t = Tallies::default();
        f(&mut t);
        t
    };
    let cases: Vec<(u8, Tallies)> = vec![
        (1, tallies(&|t| t.finale = 1)),
        (2, tallies(&|t| t.wardens = 3)),
        (3, tallies(&|t| t.hacks_in = 25)),
        (4, tallies(&|t| t.burns = 50)),
        (5, tallies(&|t| t.crew_jobs = 35)),
        (6, tallies(&|t| t.jobs = 450)),
        (7, Tallies::default()),
    ];
    for (want, t) in &cases {
        let got = c
            .seasons
            .season_ending
            .iter()
            .find(|e| Cond::parse(&e.rule, &[]).unwrap().eval(None, t))
            .unwrap()
            .num;
        assert_eq!(got, *want, "{t:?}");
    }
    assert_eq!(
        cases.len(),
        c.seasons.season_ending.len(),
        "a case for every ending"
    );
    // Epilogues: a character for each.
    let mk = |f: &dyn Fn(&mut Player)| {
        let mut p = Player::new(&c, 1, "ann", 1, 20_000);
        f(&mut p);
        p.epilogue(&c)
    };
    assert_eq!(mk(&|p| p.ties.stats[7] = 1), 1);
    assert_eq!(mk(&|p| p.ties.stats[6] = 2), 2);
    assert_eq!(mk(&|p| p.ties.stats[5] = 3), 3);
    assert_eq!(mk(&|p| p.ties.stats[3] = 2), 4);
    assert_eq!(
        mk(&|p| {
            for a in 0..3 {
                p.ties.arcs[a].ending = 1;
            }
        }),
        5
    );
    assert_eq!(
        mk(&|p| {
            p.burns = 12;
            p.jobs = 3;
        }),
        6
    );
    assert_eq!(mk(&|p| p.rep = 120), 7);
    assert_eq!(mk(&|p| p.jobs = 20), 8);
    assert_eq!(mk(&|_| {}), 9);
    assert_eq!(c.seasons.epilogue.len(), 9);
}

#[test]
fn the_arc_choice_is_a_replay_through_the_screen_and_is_the_same_every_time() {
    use russh_bbs_door::replay::{self, ReplayLog};
    use russh_bbs_door_nullroute::save::{decode, encode};
    let c = Content::try_embedded().unwrap();
    let door = russh_bbs_door_nullroute::Nullroute::new().unwrap();
    let mut p = Player::new(&c, 5, "ann", 1, 20_000);
    p.ties.arcs[0].step = 1;
    p.ties.arcs[0].awaiting = true;
    let keys = vec![
        DoorKey::Char('a'),
        DoorKey::Enter,
        DoorKey::Char('1'),
        DoorKey::Escape,
        DoorKey::Char('q'),
    ];
    let log = ReplayLog {
        door: "nullroute".into(),
        seed: 1,
        start: russh_bbs_core::limits::Day::from_raw(20_000)
            .start()
            .as_raw(),
        season: 1,
        save: Some(encode(&p)),
        keys,
    };
    let env = russh_bbs_door::DoorEnv::for_tests("ann");
    let a = replay::run(&door, &log, &env);
    let b = replay::run(&door, &log, &env);
    assert_eq!(a.save, b.save);
    let q = decode(a.save.as_deref().unwrap()).unwrap();
    assert_eq!(q.ties.arcs[0].step, 2, "choice 1 led on to step 2");
    assert!(!q.ties.arcs[0].awaiting);
    assert!(q.ties.flags != 0, "the choice set a flag");
    assert!(a.exited);
}
