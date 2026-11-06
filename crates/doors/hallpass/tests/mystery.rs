//! The east wing's mystery: the clues and their gates, the three ways to the key, the six endings
//! of a pupil's story (each replayed twice through the framework's real store and keys), and the
//! finale the whole school shares: the pupils' verdicts are counted and the winning ending
//! changes the school for everyone through the world edit.
//!
//! The framework's replay log has no stored world yet (FW-1 adds one), and the mystery's talks
//! open late in the term, so the branch replays here run a `Running` session against a store
//! seeded with a world and a save, twice with the same seed, and require the same result.
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

use std::sync::Arc;

use common::{Sandbox, content, meet, monday, satisfy};
use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite, WorldWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey, DoorRng};
use russh_bbs_door_hallpass::game::{Action, Game};
use russh_bbs_door_hallpass::save::{decode_player, decode_world, encode_player};
use russh_bbs_door_hallpass::state::{ArcProgress, Player, World, map_set};
use russh_bbs_door_hallpass::world::{WorldDelta, tick_day};
use russh_bbs_door_hallpass::{Hallpass, SAVE_VERSION, WORLD_VERSION};

/// The first slot of `day` of a given kind.
fn slot_of(g: &Game, kind: &str) -> usize {
    (0..9)
        .find(|s| g.kind(g.player.day, *s) == kind)
        .unwrap_or_else(|| panic!("no {kind} slot"))
}

/// A pupil `term_day` days into a term, in the story at `step`, as skilled as the test needs.
fn sleuth(step: &str, term_day: i64) -> Sandbox {
    let start = monday();
    let day = start + term_day;
    let mut sb = Sandbox::on("fast_runner", 3, day);
    sb.game.world.term_start = start;
    sb.game.player.life.arcs.push(ArcProgress {
        id: "the_wing".into(),
        step: step.into(),
        since: day - 1,
        ended: None,
    });
    map_set(&mut sb.game.player.xp, "sneak", 500);
    sb.game.player.cool = 90;
    sb.game.player.nerve_max = 12;
    sb.game.player.nerve = 12;
    sb
}

/// Put the pupil at `place` in a slot of `kind`, then search until the clue is found (a search
/// can be caught, which is a real outcome; this retries it on a copy with other dice).
fn find(sb: &mut Sandbox, place: &str, kind: &str, clue: &str) {
    sb.game.player.place = place.to_owned();
    sb.game.player.slot = u8::try_from(slot_of(&sb.game, kind)).unwrap();
    sb.game.player.nerve = sb.game.player.nerve_max;
    for seed in 0..60 {
        let mut g = sb.game.clone();
        let _ = g.apply(&Action::Search, &mut DoorRng::from_seed(seed));
        if g.player.knows(&format!("clue_{clue}")) {
            sb.game = g;
            return;
        }
    }
    panic!("never found {clue}");
}

fn flag(sb: &Sandbox, clue: &str) -> bool {
    sb.game.player.knows(&format!("clue_{clue}"))
}

fn step_of(sb: &Sandbox) -> String {
    sb.game.player.life.arc("the_wing").unwrap().step.clone()
}

#[test]
fn the_mystery_has_four_acts_over_weeks_and_every_clue_is_gated() {
    let c = content();
    // Four acts, a dozen clues; the first can be found in the first fortnight and the last not
    // before day 26, so the arc spans weeks, and the finale is after the last.
    assert_eq!(c.mystery.clues.len(), 12);
    for act in 1..=4 {
        assert!(c.mystery.clues.iter().any(|x| x.act == act));
    }
    assert!(c.mystery.rules.finale_day > 26 && c.mystery.rules.finale_day < c.rules.term_days);
    // A pupil who has not got to it cannot search: no arc step, no day, no earlier clue.
    let mut sb = sleuth("whispers", 6);
    sb.game.player.place = "library".into();
    sb.game.player.slot = u8::try_from(slot_of(&sb.game, "after")).unwrap();
    assert!(sb.game.next_clue().is_some());
    // c3 (old wing) wants c1 first; c12 wants day 26 and the truth step.
    sb.game.player.place = "old_wing".into();
    sb.game.player.slot = u8::try_from(slot_of(&sb.game, "evening")).unwrap();
    assert!(sb.game.next_clue().is_none(), "c3 needs c1 first (and day 9)");
    // Nothing to find where nothing is hidden.
    sb.game.player.place = "quad".into();
    assert!(sb.try_(Action::Search).is_err());
    assert!(!sb.game.can(&Action::Search));
}

#[test]
fn a_whole_investigation_by_search_reaches_the_choice_and_each_gate_holds_on_the_way() {
    let mut sb = sleuth("whispers", 30);
    // Act one.
    find(&mut sb, "library", "after", "c1_register");
    find(&mut sb, "chapel", "evening", "c2_hymn");
    assert_eq!(step_of(&sb), "whispers");
    // The key to the loft is not there until the arrows have led the way to the chapel.
    sb.game.player.place = "chapel".into();
    sb.game.player.slot = u8::try_from(slot_of(&sb.game, "evening")).unwrap();
    assert!(sb.game.next_clue().is_none(), "c4 waits for c3 and the step");
    find(&mut sb, "old_wing", "evening", "c3_arrows");
    assert_eq!(step_of(&sb), "chalk");
    // Act two: the loft is locked until the key behind the pipes is found.
    sb.game.player.place = "organ_loft".into();
    let loft_locked = !sb.game.place_open("organ_loft", sb.game.player.day, slot_of(&sb.game, "evening"));
    assert!(loft_locked);
    find(&mut sb, "chapel", "evening", "c4_pipes");
    assert!(sb.game.place_open("organ_loft", sb.game.player.day, slot_of(&sb.game, "evening")));
    find(&mut sb, "organ_loft", "evening", "c5_loft");
    assert_eq!(step_of(&sb), "loft");
    assert!(sb.game.player.count("tunnel_map") >= 1, "the map opens the tunnels and the cellars");
    find(&mut sb, "tunnels", "night", "c6_tunnel");
    find(&mut sb, "cellars", "night", "c7_trunk");
    assert_eq!(step_of(&sb), "key");
    assert!(sb.game.player.knows("ines_locket"));
    // Act three: the wing is shut and has no key yet.
    assert!(!sb.game.place_open("east_wing", sb.game.player.day, slot_of(&sb.game, "night")));
    find(&mut sb, "caretaker_shed", "after", "c8_shed");
    assert_eq!(step_of(&sb), "wing");
    assert!(sb.game.player.count("east_wing_key") >= 1);
    assert!(sb.game.place_open("east_wing", sb.game.player.day, slot_of(&sb.game, "night")));
    find(&mut sb, "east_wing", "night", "c9_hall");
    find(&mut sb, "east_wing", "night", "c10_diary");
    assert_eq!(step_of(&sb), "truth");
    assert!(sb.game.player.knows("diary_page"));
    // Act four.
    find(&mut sb, "east_wing", "night", "c11_bolt");
    find(&mut sb, "admin_corridor", "after", "c12_report");
    assert_eq!(step_of(&sb), "choice");
    assert!(sb.game.player.knows("fire_report") && sb.game.player.knows("bolt_outside"));
    assert_eq!(sb.game.stat(&russh_bbs_door_hallpass::cond::Stat::Clues), 12);
    // A clue is found once.
    sb.game.player.place = "admin_corridor".into();
    sb.game.player.slot = u8::try_from(slot_of(&sb.game, "after")).unwrap();
    assert!(sb.game.next_clue().is_none());
    // The journal says where the story stands.
    assert!(sb.game.journal().iter().any(|l| l.contains("What happened in the East Wing")
        && l.contains("trusted with it")));
}

#[test]
fn a_clue_cannot_be_found_before_its_day_and_a_risky_search_can_be_caught() {
    // Day gate: c3 wants day 9 (term day, from 0).
    let mut early = sleuth("whispers", 6);
    early.game.player.learn("clue_c1_register");
    early.game.player.place = "old_wing".into();
    early.game.player.slot = u8::try_from(slot_of(&early.game, "evening")).unwrap();
    assert!(early.game.next_clue().is_none());
    let mut later = sleuth("whispers", 10);
    later.game.player.learn("clue_c1_register");
    later.game.player.place = "old_wing".into();
    later.game.player.slot = u8::try_from(slot_of(&later.game, "evening")).unwrap();
    assert_eq!(later.game.next_clue().map(|c| c.id.as_str()), Some("c3_arrows"));
    // A clumsy searcher at a risky place is sometimes caught: a line on the file, no clue, a period gone.
    let mut caught = 0;
    for seed in 0..60 {
        let mut g = later.game.clone();
        g.player.xp.clear();
        g.player.cool = 0;
        let slot = g.player.slot;
        let _ = g.apply(&Action::Search, &mut DoorRng::from_seed(seed));
        assert_eq!(g.player.slot, slot + 1, "the period is spent either way");
        if !g.player.knows("clue_c3_arrows") {
            caught += 1;
            assert!(g.player.file.iter().any(|e| e.kind == "out_of_bounds"));
        }
    }
    assert!(caught > 0, "a risk-1 search is sometimes caught");
    assert!(caught < 60, "and sometimes not");
    // A practised, cool searcher at the same place is never caught at risk 1.
    let mut sb = sleuth("whispers", 10);
    sb.game.player.learn("clue_c1_register");
    sb.game.player.place = "old_wing".into();
    sb.game.player.slot = u8::try_from(slot_of(&sb.game, "evening")).unwrap();
    for seed in 0..40 {
        let mut g = sb.game.clone();
        let _ = g.apply(&Action::Search, &mut DoorRng::from_seed(seed));
        assert!(g.player.knows("clue_c3_arrows"), "seed {seed}");
    }
}

#[test]
fn there_are_three_ways_to_the_key_and_a_build_with_none_stays_locked_out() {
    let day = monday() + 25;
    // (a) lift it from the shed: a pupil who can sneak.
    let mut a = sleuth("key", 25);
    a.game.player.learn("clue_c7_trunk");
    find(&mut a, "caretaker_shed", "after", "c8_shed");
    assert!(a.game.player.count("east_wing_key") == 1 && a.game.player.knows("key_lifted"));
    // (b) the caretaker gives it to a friend who has found the locket.
    let mut b = sleuth("key", 25);
    b.game.player.xp.clear();
    b.game.player.learn("ines_locket");
    b.game.player.life.add_bond("fenn", 8);
    meet(&mut b.game, "fenn");
    let talks: Vec<String> = b.game.talks_here().iter().map(|t| t.id.clone()).collect();
    let at = talks.iter().position(|t| t == "wing_fenn_key").unwrap_or_else(|| panic!("{talks:?}"));
    b.ok(Action::Talk(talks[at].clone()));
    b.ok(Action::Choose(0));
    assert_eq!(b.game.player.count("east_wing_key"), 1);
    assert_eq!(step_of(&b), "wing");
    assert!(!b.game.player.knows("key_lifted"), "it was given, not lifted");
    // (c) the chaplain gives it to a pupil he trusts (the East Light story's `shared` ending).
    let mut c = sleuth("key", 25);
    c.game.player.xp.clear();
    c.game.player.learn("soames_trust");
    meet(&mut c.game, "soames");
    let id = c.game.talks_here().iter().find(|t| t.id == "wing_soames_key").map(|t| t.id.clone()).unwrap();
    c.ok(Action::Talk(id));
    c.ok(Action::Choose(0));
    assert_eq!(c.game.player.count("east_wing_key"), 1);
    // A pupil with none of the three has no way in: no sneak, no friend, no trust.
    let mut none = sleuth("key", 25);
    none.game.player.xp.clear();
    none.game.player.learn("clue_c7_trunk");
    none.game.player.place = "caretaker_shed".into();
    none.game.player.slot = u8::try_from(slot_of(&none.game, "after")).unwrap();
    assert!(none.game.next_clue().is_none(), "the nail is for someone who can sneak");
    meet(&mut none.game, "fenn");
    assert!(!none.game.talks_here().iter().any(|t| t.id == "wing_fenn_key"));
    meet(&mut none.game, "soames");
    assert!(!none.game.talks_here().iter().any(|t| t.id == "wing_soames_key"));
    assert!(!none.game.place_open("east_wing", day, 8));
}

// ---------------------------------------------------------------- the six endings

const ENDINGS: &[(&str, &str, &str, usize)] = &[
    // (talk, ending it reaches, verdict it casts, the choice pressed)
    ("wing_fenn_choice", "fenn", "fenn", 0),
    ("wing_soames_choice", "soames", "soames", 0),
    ("wing_vane_choice", "vane", "vane", 0),
    ("wing_wren_choice", "paper", "wren", 0),
    ("wing_sol_choice", "bonfire", "bonfire", 0),
    ("wing_tilly_walk_away", "walk_away", "", 0),
];

/// A pupil at the choice, built to be able to do `talk`'s first choice, standing where its
/// character is.
fn at_the_choice(talk: &str, term_day: i64) -> Player {
    let c = content();
    let spec = c.talk(talk).unwrap().clone();
    let mut sb = sleuth("choice", term_day);
    sb.game.player.xp.clear();
    for cond in &spec.when {
        satisfy(&mut sb.game, cond);
    }
    for cond in &spec.node[0].choice[0].needs {
        satisfy(&mut sb.game, cond);
    }
    // `satisfy` moves the world's term start for a day condition; keep the pupil in the same term day.
    sb.game.player.knowledge.retain(|k| !k.starts_with("clue_filler"));
    for f in ["diary_page", "bolt_outside", "fire_report", "clue_c10_diary"] {
        sb.game.player.learn(f);
    }
    meet(&mut sb.game, &spec.npc);
    sb.game.player.clone()
}

fn keys(text: &str) -> Vec<DoorKey> {
    text.chars().map(DoorKey::Char).collect()
}

struct Branch {
    player: Player,
    world: World,
}

/// One recorded branch: a store seeded with a world and a save, a session that presses `text`.
async fn run_branch(player: &Player, world: &World, text: &str, seed: u64) -> Branch {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Hallpass::shared().unwrap();
    let day = player.day;
    store.set_now(Day::from_raw(day).start());
    store
        .write_save(
            &door.info().id,
            AccountId::from_raw(1),
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: SAVE_VERSION,
                bytes: &encode_player(player),
            },
        )
        .await
        .unwrap();
    store
        .write_world(
            &door.info().id,
            &WorldWrite {
                expected: Revision::NONE,
                version: WORLD_VERSION,
                bytes: &world.to_bytes(),
                tick: None,
            },
        )
        .await
        .unwrap();
    let clock = Arc::new(ManualClock::new(Day::from_raw(day).start()));
    let c: Arc<dyn Clock> = clock;
    let env = DoorEnv::for_tests(&player.name);
    let mut run = Running::open(&store, door.clone(), AccountId::from_raw(1), &env, c, seed)
        .await
        .unwrap();
    for k in keys(text) {
        run.key(k);
        run.flush(&store).await.unwrap();
    }
    let saved = store
        .load_save(&door.info().id, AccountId::from_raw(1))
        .await
        .unwrap()
        .unwrap();
    let w = store.load_world(&door.info().id).await.unwrap().unwrap();
    Branch {
        player: decode_player(&saved.bytes).unwrap(),
        world: decode_world(&w.bytes).unwrap(),
    }
}

fn world_at(term_day: i64) -> World {
    let mut w = World::new(monday() + term_day);
    w.term_start = monday();
    w.last_day = monday() + term_day - 1;
    w
}

#[tokio::test]
async fn every_ending_of_the_pupils_story_is_replayable_and_casts_its_own_verdict() {
    let c = content();
    let term_day = 30;
    for (talk, ending, verdict, choice) in ENDINGS {
        let player = at_the_choice(talk, term_day);
        let world = world_at(term_day);
        // Which entry in the talk list is this conversation?
        let (probe, _) = Game::open(&c, &player.name, Some(player.clone()), Some(&world.to_bytes()), player.day);
        let here: Vec<String> = probe.talks_here().iter().map(|t| t.id.clone()).collect();
        let at = here
            .iter()
            .position(|t| t == talk)
            .unwrap_or_else(|| panic!("{talk} not on offer: {here:?}"));
        let text = format!("t{}{}", at + 1, choice + 1);
        let one = run_branch(&player, &world, &text, 9).await;
        let two = run_branch(&player, &world, &text, 9).await;
        assert_eq!(
            encode_player(&one.player),
            encode_player(&two.player),
            "{talk}: the same keys give the same pupil"
        );
        assert_eq!(one.world, two.world, "{talk}: and the same school");
        let arc = one.player.life.arc("the_wing").unwrap();
        assert_eq!(arc.ended.as_deref(), Some(*ending), "{talk}");
        if verdict.is_empty() {
            assert_eq!(one.player.social.verdict, None, "walking away casts nothing");
            assert!(one.world.commons.verdicts.is_empty());
        } else {
            assert_eq!(one.player.social.verdict.as_deref(), Some(*verdict), "{talk}");
            assert_eq!(one.world.commons.verdict_of(&one.player.name), Some(*verdict));
        }
        // The choice closes the doors of the other branches: no other ending's talk is offered.
        let (after, _) = Game::open(&c, &one.player.name, Some(one.player.clone()), Some(&one.world.to_bytes()), one.player.day);
        for other in ENDINGS.iter().filter(|e| e.0 != *talk) {
            let mut g = after.clone();
            meet(&mut g, &c.talk(other.0).unwrap().npc);
            assert!(
                !g.talks_here().iter().any(|t| t.id == other.0),
                "after {talk}, {} is shut",
                other.0
            );
        }
    }
}

#[test]
fn each_ending_is_closed_to_the_builds_that_have_not_earned_it() {
    let c = content();
    let open = |talk: &str, f: &dyn Fn(&mut Game)| -> bool {
        let p = at_the_choice(talk, 30);
        let name = p.name.clone();
        let (mut g, _) = Game::open(&c, &name, Some(p), Some(&world_at(30).to_bytes()), monday() + 30);
        f(&mut g);
        meet(&mut g, &c.talk(talk).unwrap().npc);
        g.talks_here().iter().any(|t| t.id == talk)
    };
    // Trusting the headmistress is for a pupil with a clean file who is not a known troublemaker.
    assert!(open("wing_vane_choice", &|_| {}));
    assert!(!open("wing_vane_choice", &|g| {
        g.player.file.push(russh_bbs_door_hallpass::state::FileEntry { day: 1, kind: "caught".into(), sev: 6 });
    }));
    assert!(!open("wing_vane_choice", &|g| {
        g.player.traits.push("known_troublemaker".into());
    }));
    // The paper wants a name in the Ink Society; the chaplain wants his trust; the caretaker a friendship.
    assert!(!open("wing_wren_choice", &|g| g.player.add_standing("ink_society", -100)));
    assert!(!open("wing_soames_choice", &|g| g.player.knowledge.retain(|k| k != "soames_trust")));
    assert!(!open("wing_fenn_choice", &|g| g.player.life.add_bond("fenn", -100)));
    // Burning it needs the diary; walking away needs only to have read it.
    assert!(!open("wing_sol_choice", &|g| g.player.knowledge.retain(|k| k != "diary_page")));
    // No ending is offered before the story gets there, however the build.
    for (talk, ..) in ENDINGS {
        let p = at_the_choice(talk, 30);
        let name = p.name.clone();
        let (mut g, _) = Game::open(&c, &name, Some(p), Some(&world_at(30).to_bytes()), monday() + 30);
        g.player.life.arcs.iter_mut().for_each(|a| a.step = "truth".into());
        meet(&mut g, &c.talk(talk).unwrap().npc);
        assert!(!g.talks_here().iter().any(|t| t.id == *talk), "{talk} before the choice");
    }
}

// ---------------------------------------------------------------- the finale

/// A school on its finale day with these pupils' verdicts cast, counted by the real tick.
fn finale_with(verdicts: &[(&str, &str, i64)]) -> World {
    let c = content();
    let mut w = world_at(c.mystery.rules.finale_day - 1);
    for (by, trust, day) in verdicts {
        WorldDelta::Verdict {
            by: (*by).to_owned(),
            trust: (*trust).to_owned(),
            day: monday() + day,
        }
        .apply(&mut w, &c);
    }
    tick_day(&mut w, &c, monday() + c.mystery.rules.finale_day, &mut DoorRng::from_seed(1));
    w
}

fn pupil_can_enter(w: &World, place: &str, with_key: bool) -> bool {
    let c = content();
    let day = monday() + c.mystery.rules.finale_day;
    let (mut g, _) = Game::open(&c, "visitor", None, Some(&w.to_bytes()), day);
    g.apply(&Action::Enrol("fast_runner".into()), &mut DoorRng::from_seed(1)).unwrap();
    if with_key {
        map_set(&mut g.player.inventory, "east_wing_key", 1);
        map_set(&mut g.player.inventory, "tunnel_map", 1);
    }
    let night = slot_of(&g, "night");
    g.place_open(place, g.player.day, night)
}

#[test]
fn each_verdict_the_school_reaches_changes_the_east_wing_for_everyone() {
    let c = content();
    // Opened for all: the caretaker's truth, the chaplain's vigil, the paper's front page.
    for (trust, effect) in [("fenn", "wing_opened"), ("soames", "wing_vigil"), ("wren", "truth_printed")] {
        let w = finale_with(&[("ann", trust, 31)]);
        assert!(w.has(effect), "{trust}");
        assert_eq!(w.commons.finale.as_deref(), Some(trust));
        assert!(
            pupil_can_enter(&w, "east_wing", false) && pupil_can_enter(&w, "tunnels", false),
            "{trust}: the wing and the tunnels open to a pupil with no key"
        );
        assert!(w.paper.iter().any(|h| h.text.contains("ann")), "the paper names whoever brought it: {:?}", w.paper);
    }
    // Shut for good: the headmistress's bricks and the bonfire close it even to a pupil with the key.
    for (trust, effect) in [("vane", "wing_sealed"), ("bonfire", "bonfire")] {
        let w = finale_with(&[("ann", trust, 31)]);
        assert!(w.has(effect), "{trust}");
        assert!(
            !pupil_can_enter(&w, "east_wing", true) && !pupil_can_enter(&w, "tunnels", true),
            "{trust}: closed even with the key"
        );
    }
    // Nobody settled it: the wing stays as it was, and the paper says so.
    let w = finale_with(&[]);
    assert_eq!(w.commons.finale.as_deref(), Some(""));
    assert!(c.mystery.verdicts.iter().all(|v| !w.has(&v.effect)));
    assert!(!pupil_can_enter(&w, "east_wing", false));
    assert!(w.paper.iter().any(|h| h.text.contains("stays shut")));
    // The effects differ in what they do to the term too.
    assert!(c.effect("wing_sealed").unwrap().patrol_mod > c.effect("wing_vigil").unwrap().patrol_mod);
    assert!(c.effect("truth_printed").unwrap().legend_pct > c.effect("bonfire").unwrap().legend_pct);
}

#[test]
fn the_school_counts_verdicts_most_trusted_wins_and_a_tie_goes_by_the_table_order() {
    // Two for the paper beat one for the caretaker.
    let w = finale_with(&[("ann", "fenn", 31), ("bea", "wren", 32), ("cal", "wren", 33)]);
    assert_eq!(w.commons.finale.as_deref(), Some("wren"));
    assert!(w.has("truth_printed") && !w.has("wing_opened"));
    // A tie: the lower order (the caretaker) wins, whoever cast first.
    let w = finale_with(&[("ann", "vane", 31), ("bea", "fenn", 32)]);
    assert_eq!(w.commons.finale.as_deref(), Some("fenn"));
    // One verdict a pupil: a second from the same hand is ignored, and so is one that is not a verdict.
    let c = content();
    let mut w = world_at(30);
    for (by, trust) in [("ann", "vane"), ("ann", "fenn"), ("bea", "nonsense")] {
        WorldDelta::Verdict { by: by.into(), trust: trust.into(), day: 1 }.apply(&mut w, &c);
    }
    assert_eq!(w.commons.verdicts.len(), 1);
    assert_eq!(w.commons.verdicts[0].trust, "vane");
    // Once the finale has happened, a late verdict changes nothing.
    let mut w = finale_with(&[("ann", "vane", 31)]);
    let before = w.clone();
    WorldDelta::Verdict { by: "late".into(), trust: "fenn".into(), day: 40 }.apply(&mut w, &c);
    tick_day(&mut w, &c, monday() + 40, &mut DoorRng::from_seed(1));
    assert_eq!(w.commons.finale, before.commons.finale);
    assert!(w.has("wing_sealed") && !w.has("wing_opened"));
}

#[test]
fn the_finale_is_told_to_everyone_once_through_the_real_board() {
    // Two pupils choose on the same board through the real keys, the daily tick counts them, and a
    // third pupil who never touched the mystery wakes to a different school.
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let store = MemoryDoorStore::new();
        let door: Arc<dyn Door> = Hallpass::shared().unwrap();
        let c = content();
        let finale_day = monday() + c.mystery.rules.finale_day;
        let term_day = c.mystery.rules.finale_day - 2;
        store.set_now(Day::from_raw(monday() + term_day).start());
        let clock = Arc::new(ManualClock::new(Day::from_raw(monday() + term_day).start()));
        let dyn_clock: Arc<dyn Clock> = clock.clone();
        store
            .write_world(
                &door.info().id,
                &WorldWrite {
                    expected: Revision::NONE,
                    version: WORLD_VERSION,
                    bytes: &world_at(term_day).to_bytes(),
                    tick: None,
                },
            )
            .await
            .unwrap();
        for (account, name, talk) in [(1, "ann", "wing_wren_choice"), (2, "bea", "wing_sol_choice"), (3, "cal", "wing_wren_choice")] {
            let mut p = at_the_choice(talk, term_day);
            p.name = name.to_owned();
            store
                .write_save(
                    &door.info().id,
                    AccountId::from_raw(account),
                    &SaveWrite {
                        expected: Revision::NONE,
                        season: 1,
                        version: SAVE_VERSION,
                        bytes: &encode_player(&p),
                    },
                )
                .await
                .unwrap();
            let env = DoorEnv::for_tests(name);
            let mut run = Running::open(&store, door.clone(), AccountId::from_raw(account), &env, dyn_clock.clone(), account as u64)
                .await
                .unwrap();
            let (probe, _) = Game::open(&c, name, Some(p.clone()), None, p.day);
            let _ = probe;
            for k in keys("t") {
                run.key(k);
                run.flush(&store).await.unwrap();
            }
            // Pick the conversation by its title on screen: it is the only one with the character.
            let screen = run.look_around(&env).join("\n");
            let index = screen
                .lines()
                .find(|l| l.contains(&c.talk(talk).unwrap().title))
                .and_then(|l| l.trim_start().chars().find(char::is_ascii_digit))
                .unwrap_or_else(|| panic!("{screen}"));
            for k in [DoorKey::Char(index), DoorKey::Char('1')] {
                run.key(k);
                run.flush(&store).await.unwrap();
            }
        }
        let mid = decode_world(&store.load_world(&door.info().id).await.unwrap().unwrap().bytes).unwrap();
        assert_eq!(mid.commons.verdicts.len(), 3, "three pupils cast: {:?}", mid.commons.verdicts);
        assert!(mid.commons.finale.is_none());
        // The board's daily tick, once the finale day comes.
        for d in (monday() + term_day + 1)..=finale_day {
            clock.set(Day::from_raw(d).start());
            store.set_now(Day::from_raw(d).start());
            run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
                .await
                .unwrap();
        }
        let after = decode_world(&store.load_world(&door.info().id).await.unwrap().unwrap().bytes).unwrap();
        assert_eq!(after.commons.finale.as_deref(), Some("wren"), "two of three: {:?}", after.commons.verdicts);
        assert!(after.has("truth_printed"));
        assert!(pupil_can_enter(&after, "east_wing", false));
        // A fourth pupil who has done nothing hears of it in the morning, once.
        let (mut dee, events) = Game::open(&c, "dee", None, Some(&after.to_bytes()), finale_day);
        dee.apply(&Action::Enrol("fast_runner".into()), &mut DoorRng::from_seed(1)).unwrap();
        let _ = events;
        let (_, events) = Game::open(&c, "dee", Some(dee.player.clone()), Some(&after.to_bytes()), finale_day + 1);
        assert!(events.iter().any(|e| e.describe().contains("the paper prints the diary")), "{events:?}");
        let mut told = dee.clone();
        told.player.social.told_finale = told.player.term;
        let (_, events) = Game::open(&c, "dee", Some(told.player), Some(&after.to_bytes()), finale_day + 2);
        assert!(!events.iter().any(|e| e.describe().contains("the paper prints the diary")));
    });
}
