//! Storylines (M4-H2): replay logs for each branch of both arcs, branches that close doors, a
//! storyline that begins by itself, and the arc that changes the shared school through the
//! framework's real world edit.
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

use std::collections::BTreeSet;
use std::sync::Arc;

use common::{Sandbox, content, monday};
use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::replay::{self, ReplayLog};
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey};
use russh_bbs_door_hallpass::game::Game;
use russh_bbs_door_hallpass::save::{decode_player, decode_world, encode_player};
use russh_bbs_door_hallpass::state::{ArcProgress, Membership, Player, World, map_set};
use russh_bbs_door_hallpass::{DOOR_ID, Hallpass, SAVE_VERSION};

fn keys(text: &str) -> Vec<DoorKey> {
    text.chars().map(DoorKey::Char).collect()
}

/// A pupil with a storyline at `step`, begun on `day`.
fn pupil(trait_id: &str, arc: &str, step: &str, day: i64) -> Player {
    let mut sb = Sandbox::on(trait_id, 1, day);
    sb.game.player.life.arcs.push(ArcProgress {
        id: arc.into(),
        step: step.into(),
        since: day,
        ended: None,
    });
    sb.game.player.clone()
}

/// One recorded conversation: the pupil is where the character is, `days` after the last step, and
/// presses the keys the screen needs to open `talk` and choose option `choice`.
fn step(player: &Player, talk: &str, choice: usize, days: i64, seed: u64) -> (Player, usize) {
    let c = content();
    let spec = c.talk(talk).unwrap();
    let mut p = player.clone();
    p.day += days.max(spec.wait);
    p.slot = 0;
    p.attended = false;
    p.moves = 5;
    p.nerve = p.nerve_max;
    let (probe, _) = Game::open(&c, "ann", Some(p.clone()), None, p.day);
    let mut probe = probe;
    let npc = c.npc(&spec.npc).unwrap().clone();
    probe.player.place = probe.npc_place(&npc, p.day, 0).to_owned();
    p.place.clone_from(&probe.player.place);
    let here: Vec<String> = probe.talks_here().iter().map(|t| t.id.clone()).collect();
    let at = here
        .iter()
        .position(|t| t == talk)
        .unwrap_or_else(|| panic!("{talk} is not on offer: {here:?}"));
    let log = ReplayLog {
        door: DOOR_ID.to_owned(),
        seed,
        start: Day::from_raw(p.day).start().as_raw(),
        season: 1,
        save: Some(encode_player(&p)),
        keys: keys(&format!("t{}{}", at + 1, choice + 1)),
    };
    let door = Hallpass::new().unwrap();
    let env = DoorEnv::for_tests("ann");
    let run = replay::run_checked(&door, &log, &env).unwrap();
    assert_eq!(
        run,
        replay::run_checked(&door, &log, &env).unwrap(),
        "the same log gives the same run"
    );
    (
        decode_player(run.save.as_deref().unwrap()).unwrap(),
        run.world_edits,
    )
}

/// Play a route of (talk, choice) steps, two days apart. Returns the pupil and the world edits asked
/// for by the last step.
fn route(mut player: Player, steps: &[(&str, usize)]) -> (Player, usize) {
    let mut edits = 0;
    for (i, (talk, choice)) in steps.iter().enumerate() {
        let (next, e) = step(&player, talk, *choice, 2, 7 + i as u64);
        player = next;
        edits = e;
    }
    (player, edits)
}

fn with_paper_card(mut p: Player) -> Player {
    p.life.clubs.push(Membership {
        club: "paper".into(),
        rank: 2,
        merit: 5,
        last_meet: -1,
    });
    p
}

#[test]
fn every_branch_of_the_ledger_ends_its_own_way_and_is_replayable() {
    let start = monday() + 10;
    let base = || pupil("smooth_talker", "ledger", "found", start);
    let routes: &[(&str, &[(&str, usize)], &str, bool)] = &[
        ("looked away", &[("ledger_tilly", 2)], "shrugged", false),
        (
            "asked and told to return it",
            &[("ledger_tilly", 0), ("ledger_bramley", 1)],
            "returned",
            false,
        ),
        (
            "asked and left it",
            &[("ledger_tilly", 0), ("ledger_bramley", 2)],
            "shrugged",
            false,
        ),
        (
            "took the shakedown",
            &[
                ("ledger_tilly", 0),
                ("ledger_bramley", 0),
                ("ledger_owes", 0),
            ],
            "owed",
            false,
        ),
        (
            "took the money and told the head prefect",
            &[
                ("ledger_tilly", 0),
                ("ledger_bramley", 0),
                ("ledger_owes", 1),
                ("ledger_wick", 0),
            ],
            "purged",
            false,
        ),
        (
            "ran it as the front page",
            &[("ledger_tilly", 1), ("ledger_wren", 0)],
            "headline",
            true,
        ),
        (
            "brought the proof to Wick",
            &[("ledger_tilly", 1), ("ledger_wren", 1), ("ledger_wick", 0)],
            "purged",
            false,
        ),
        (
            "said she was mistaken",
            &[("ledger_tilly", 1), ("ledger_wren", 2)],
            "shrugged",
            false,
        ),
        (
            "took it back from Wick",
            &[("ledger_tilly", 1), ("ledger_wren", 1), ("ledger_wick", 1)],
            "shrugged",
            false,
        ),
    ];
    let mut seen = BTreeSet::new();
    for (name, steps, ending, paper) in routes {
        let player = if *paper {
            with_paper_card(base())
        } else {
            base()
        };
        let (end, edits) = route(player, steps);
        let arc = end.life.arc("ledger").unwrap();
        assert_eq!(arc.ended.as_deref(), Some(*ending), "{name}");
        seen.insert((*ending).to_owned());
        // The world edit is asked for exactly when the ending is a world change.
        let changes_world = matches!(*ending, "purged" | "headline");
        assert_eq!(edits > 0, changes_world, "{name}: {edits} edits");
        // A betrayed prefect stays betrayed.
        if *name == "took the money and told the head prefect" {
            assert!(end.knows("betrayed_bramley"));
            assert!(end.life.bond_with("bramley") <= -30);
        }
        if *ending == "owed" {
            assert!(end.knows("bramley_owes") && end.cash > 12);
        }
    }
    let all: BTreeSet<String> = content()
        .arc("ledger")
        .unwrap()
        .ending
        .iter()
        .map(|e| e.id.clone())
        .collect();
    assert_eq!(
        seen, all,
        "every ending of the story has a recorded way to it"
    );
}

#[test]
fn every_branch_of_the_east_wing_light_ends_its_own_way_and_is_replayable() {
    let start = monday() + 12;
    let base = |bluff: bool| {
        let mut p = pupil("smooth_talker", "east_light", "rumour", start);
        p.nerve_max = 6;
        if bluff {
            map_set(&mut p.xp, "bluff", 40);
        }
        p
    };
    let watch = [("light_fenn", 0), ("light_watch", 0)];
    let seen_it = [watch[0], watch[1], ("light_seen", 0)];
    let routes: Vec<(&str, Vec<(&str, usize)>, &str, bool)> = vec![
        ("left it alone", vec![("light_fenn", 2)], "ignored", false),
        (
            "watched, then slept",
            vec![("light_fenn", 0), ("light_watch", 1)],
            "quiet",
            false,
        ),
        (
            "told Marsh and kept watch",
            vec![("light_fenn", 1), ("light_told", 0)],
            "informer",
            false,
        ),
        (
            "told Marsh and said no more",
            vec![("light_fenn", 1), ("light_told", 1)],
            "ignored",
            false,
        ),
        (
            "kept the page",
            vec![watch[0], watch[1], ("light_seen", 1)],
            "secret",
            false,
        ),
        (
            "took it to the chaplain",
            vec![seen_it[0], seen_it[1], seen_it[2], ("light_soames", 0)],
            "shared",
            false,
        ),
        (
            "lied to the chaplain",
            vec![seen_it[0], seen_it[1], seen_it[2], ("light_soames", 1)],
            "quiet",
            true,
        ),
        (
            "took it back",
            vec![seen_it[0], seen_it[1], seen_it[2], ("light_soames", 2)],
            "secret",
            false,
        ),
    ];
    let mut seen = BTreeSet::new();
    for (name, steps, ending, bluff) in &routes {
        let (end, edits) = route(base(*bluff), steps);
        assert_eq!(
            end.life.arc("east_light").unwrap().ended.as_deref(),
            Some(*ending),
            "{name}"
        );
        assert_eq!(edits, 0, "this story does not change the school");
        seen.insert((*ending).to_owned());
        if *ending == "shared" {
            assert!(end.knows("soames_trust") && end.knows("diary_page"));
        }
        if *ending == "informer" {
            assert!(end.life.bond_with("marsh") > 0);
        }
    }
    let all: BTreeSet<String> = content()
        .arc("east_light")
        .unwrap()
        .ending
        .iter()
        .map(|e| e.id.clone())
        .collect();
    assert_eq!(seen, all);
}

#[test]
fn a_choice_that_moves_the_story_on_closes_the_doors_of_the_other_branch() {
    let c = content();
    let start = monday() + 10;
    let ready = |p: &Player, talk: &str| {
        let (g, _) = Game::open(&c, "ann", Some(p.clone()), None, p.day);
        g.talk_ready(c.talk(talk).unwrap())
    };
    // From the first step, Bramley and Wren are both a choice away, never both.
    let found = pupil("smooth_talker", "ledger", "found", start);
    let (to_bramley, _) = step(&found, "ledger_tilly", 0, 1, 1);
    let (to_wren, _) = step(&found, "ledger_tilly", 1, 1, 1);
    for (p, open, shut) in [
        (&to_bramley, "ledger_bramley", "ledger_wren"),
        (&to_wren, "ledger_wren", "ledger_bramley"),
    ] {
        let mut later = p.clone();
        later.day += 3;
        assert!(ready(&later, open), "{open}");
        assert!(!ready(&later, shut), "{shut}");
        assert!(
            !ready(&later, "ledger_tilly"),
            "and the first talk is behind them"
        );
    }
    // The wait is real: the next morning's talk is not yet on offer on the same day.
    assert!(!ready(&to_bramley, "ledger_bramley"), "give it a day");
    // Telling Marsh shuts Fenn's watch for good.
    let rumour = pupil("smooth_talker", "east_light", "rumour", start);
    let (told, _) = step(&rumour, "light_fenn", 1, 1, 1);
    let mut later = told.clone();
    later.day += 3;
    assert!(ready(&later, "light_told") && !ready(&later, "light_watch"));
    // An ended story offers nothing more.
    let (done, _) = step(&rumour, "light_fenn", 2, 1, 1);
    let mut later = done.clone();
    later.day += 5;
    for t in c
        .talks
        .iter()
        .filter(|t| t.arc.as_deref() == Some("east_light"))
    {
        assert!(!ready(&later, &t.id), "{}", t.id);
    }
}

#[test]
fn storylines_begin_by_themselves_on_their_day_and_in_a_new_term_begin_again() {
    let c = content();
    let start = monday();
    let mut world = World::new(start);
    let (mut g, _) = Game::open(&c, "ann", None, Some(&world.to_bytes()), start);
    g.apply(
        &russh_bbs_door_hallpass::game::Action::Enrol("smooth_talker".into()),
        &mut russh_bbs_door::DoorRng::from_seed(1),
    )
    .unwrap();
    world.last_day = start;
    let began =
        |g: &Game| -> Vec<String> { g.player.life.arcs.iter().map(|a| a.id.clone()).collect() };
    g.world = world.clone();
    let events = g.sync_day(start + 3);
    assert!(began(&g).is_empty() && events.iter().all(|e| !e.describe().contains("story")));
    let events = g.sync_day(start + 4);
    assert_eq!(began(&g), ["ledger"]);
    assert!(
        events
            .iter()
            .any(|e| e.describe().contains("A story begins"))
    );
    g.sync_day(start + 8);
    assert_eq!(
        began(&g),
        ["ledger", "east_light"],
        "two storylines in the term"
    );
    // The journal tells where each stands.
    let journal = g.journal();
    assert_eq!(journal.len(), 2);
    assert!(journal[0].contains("The prefects' ledger"), "{journal:?}");
    // A new term is a new character: the stories start fresh.
    let term_days = c.rules.term_days;
    let mut next = world.clone();
    next.term = 2;
    next.term_start = start + term_days;
    let (fresh, _) = Game::open(
        &c,
        "ann",
        Some(g.player.clone()),
        Some(&next.to_bytes()),
        start + term_days,
    );
    assert!(fresh.player.life.arcs.is_empty());
    assert_eq!(
        fresh.player.history.len(),
        1,
        "and the old term is closed in the book"
    );
}

// ---------------------------------------------------------------- the shared world, for real

async fn store_save(store: &MemoryDoorStore, door: &dyn Door, account: i64, player: &Player) {
    store
        .write_save(
            &door.info().id,
            AccountId::from_raw(account),
            &SaveWrite {
                expected: Revision::NONE,
                season: 1,
                version: SAVE_VERSION,
                bytes: &encode_player(player),
            },
        )
        .await
        .unwrap();
}

async fn world_of(store: &MemoryDoorStore, door: &dyn Door) -> World {
    decode_world(
        &store
            .load_world(&door.info().id)
            .await
            .unwrap()
            .unwrap()
            .bytes,
    )
    .unwrap()
}

#[tokio::test]
async fn the_ledger_story_changes_the_school_for_everyone_through_the_real_world_edit() {
    let store = MemoryDoorStore::new();
    let door: Arc<dyn Door> = Hallpass::shared().unwrap();
    let start = monday() + 10;
    // Ann has taken the proof to Cordelia Wick, with the story at its last step.
    let mut ann = pupil("smooth_talker", "ledger", "expose", start);
    ann.name = "ann".into();
    // Place her with Wick on a later day, the way a morning would.
    ann.day += 2;
    ann.slot = 0;
    let c = content();
    let (g, _) = Game::open(&c, "ann", Some(ann.clone()), None, ann.day);
    let wick = c.npc("wick").unwrap().clone();
    ann.place = g.npc_place(&wick, ann.day, 0).to_owned();
    ann.moves = 5;
    let today = ann.day;
    let clock = Arc::new(ManualClock::new(Day::from_raw(today).start()));
    store.set_now(Day::from_raw(today).start());
    run_due(&store, door.as_ref(), clock.as_ref(), TickPolicy::default())
        .await
        .unwrap();
    let before = world_of(&store, door.as_ref()).await;
    assert!(before.active.is_empty() && before.paper.is_empty());

    store_save(&store, door.as_ref(), 1, &ann).await;
    let env = DoorEnv::for_tests("ann");
    let c: Arc<dyn Clock> = clock.clone();
    let mut run = Running::open(&store, door.clone(), AccountId::from_raw(1), &env, c, 5)
        .await
        .unwrap();
    // Open the talk with Wick and hand over the ledger.
    let (probe, _) = Game::open(&content(), "ann", Some(ann.clone()), None, today);
    let at = probe
        .talks_here()
        .iter()
        .position(|t| t.id == "ledger_wick")
        .unwrap();
    for key in keys(&format!("t{}1", at + 1)) {
        run.key(key);
        run.flush(&store).await.unwrap();
    }

    let after = world_of(&store, door.as_ref()).await;
    assert!(after.has("prefects_stood_down"), "the world remembers");
    assert_eq!(after.changes, 1);
    assert!(
        after
            .paper
            .iter()
            .any(|h| h.text.contains("ann") && h.text.contains("stood down")),
        "the paper names the culprit: {:?}",
        after.paper
    );
    assert_eq!(after.hall.first().map(|h| h.name.as_str()), Some("ann"));
    // Ann's own save has the ending, the legend and the friendship it earned.
    let saved = decode_player(
        &store
            .load_save(&door.info().id, AccountId::from_raw(1))
            .await
            .unwrap()
            .unwrap()
            .bytes,
    )
    .unwrap();
    assert_eq!(
        saved.life.arc("ledger").unwrap().ended.as_deref(),
        Some("purged")
    );
    assert!(saved.legend >= 10 && saved.big == 1);

    // Another pupil who has never met Ann sees a different school.
    let c = content();
    let (bea, _) = Game::open(&c, "bea", None, Some(&after.to_bytes()), today);
    assert!(bea.world.has("prefects_stood_down"));
    let (plain, _) = Game::open(&c, "bea", None, None, today);
    assert_eq!(
        bea.patrol_level("admin_corridor") + 1,
        plain.patrol_level("admin_corridor"),
        "patrols are thinner for everyone"
    );
    let bramley = c.npc("bramley").unwrap();
    assert_eq!(
        bea.npc_place(bramley, today, 5),
        "common_room",
        "and Bramley is moved"
    );
    assert_ne!(plain.npc_place(bramley, today, 5), "common_room");
    // Conversations read the school and the pupil's own deeds: Bea, who never met the ledger, finds
    // Wick changed by the stand-down but cannot take credit for it; Ann can.
    let wick = c.npc("wick").unwrap().clone();
    let aftermath = c.talk("wick_aftermath").unwrap().clone();
    let tomorrow = today + 1;
    let mut rng = russh_bbs_door::DoorRng::from_seed(3);
    let mut ann_now = saved.clone();
    ann_now.day = tomorrow;
    let (mut ann_game, _) = Game::open(&c, "ann", Some(ann_now), Some(&after.to_bytes()), tomorrow);
    let (mut bea, _) = Game::open(&c, "bea", None, Some(&after.to_bytes()), tomorrow);
    bea.apply(
        &russh_bbs_door_hallpass::game::Action::Enrol("fast_runner".into()),
        &mut rng,
    )
    .unwrap();
    let mut views = Vec::new();
    for g in [&mut ann_game, &mut bea] {
        g.player.slot = 0;
        g.player.place = g.npc_place(&wick, g.player.day, 0).to_owned();
        assert!(
            g.talk_open(&aftermath),
            "{}: the school changed for them too",
            g.player.name
        );
        g.apply(
            &russh_bbs_door_hallpass::game::Action::Talk("wick_aftermath".into()),
            &mut rng,
        )
        .unwrap();
        views.push(g.scene_view().unwrap());
    }
    assert!(
        views[0].choices[0].open,
        "Ann's own deed opens the first choice"
    );
    assert!(
        !views[1].choices[0].open && views[1].choices[1].open,
        "Bea sees the same scene without the credit"
    );
    // The framework's retry of the same edit does not stack it, and the tick keeps it all term.
    let delta = russh_bbs_door_hallpass::world::WorldDelta::Effect {
        id: "prefects_stood_down".into(),
        by: "ann".into(),
        day: today,
    };
    let mut again = after.clone();
    delta.apply(&mut again, &c);
    assert_eq!(again.active.len(), after.active.len());
    let mut w = after.clone();
    let mut rng = russh_bbs_door::DoorRng::from_seed(1);
    for d in 1..30 {
        russh_bbs_door_hallpass::world::tick_day(&mut w, &c, today + d, &mut rng);
    }
    assert!(w.has("prefects_stood_down"), "an arc ending lasts the term");
}
