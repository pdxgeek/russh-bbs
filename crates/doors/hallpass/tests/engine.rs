//! The school: timetable, map, schedules, locks, the school day, the file and the term.
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

use common::{Sandbox, content, monday};
use russh_bbs_door::DoorRng;
use russh_bbs_door_hallpass::game::{Action, Event, Game};
use russh_bbs_door_hallpass::state::{Active, Status, World, map_get};

fn active(id: &str) -> Active {
    Active {
        id: id.into(),
        since: 0,
        until: None,
        by: "bea".into(),
    }
}

#[test]
fn the_timetable_is_data_and_follows_the_weekday() {
    let sb = Sandbox::new("fast_runner", 1);
    let g = &sb.game;
    assert_eq!(g.periods(), 9);
    assert_eq!(Game::weekday(monday()), 0);
    assert_eq!(g.activity(monday(), 0), "class:maths");
    assert_eq!(g.kind(monday(), 2), "lunch");
    assert_eq!(g.kind(monday(), 8), "night");
    // Saturday has afternoons free; Sunday starts with chapel.
    assert_eq!(g.activity(monday() + 5, 3), "free");
    assert_eq!(g.activity(monday() + 6, 0), "chapel");
    // Every weekday names a room and a teacher for each lesson.
    for day in monday()..monday() + 7 {
        for slot in 0..g.periods() {
            if let Some(subject) = g.activity(day, slot).strip_prefix("class:") {
                let s = g.content.subject(subject).unwrap();
                assert!(g.content.place(&s.room).is_some() && g.content.npc(&s.teacher).is_some());
            }
        }
    }
}

#[test]
fn character_schedules_decide_who_is_where_and_big_schemes_move_them() {
    let sb = Sandbox::new("fast_runner", 1);
    let g = &sb.game;
    let pell = g.content.npc("pell").unwrap();
    assert_eq!(g.npc_place(pell, monday(), 0), "room_maths");
    assert_eq!(g.npc_place(pell, monday(), 2), "staffroom");
    assert_eq!(g.npc_place(pell, monday(), 8), "staff_quarters");
    assert!(
        g.npcs_at("room_maths", monday(), 0)
            .iter()
            .any(|n| n.id == "pell")
    );
    assert_eq!(g.witnesses_at("room_maths", monday(), 0), 1);
    let marsh = g.content.npc("marsh").unwrap();
    assert_eq!(g.npc_place(marsh, monday(), 5), "admin_corridor");
    let mut moved = sb;
    moved.game.world.active.push(active("deputy_moved"));
    let marsh = moved.game.content.npc("marsh").unwrap().clone();
    for slot in 0..9 {
        assert_eq!(moved.game.npc_place(&marsh, monday(), slot), "old_wing");
    }
    // Students keep to their own day; the prefect on patrol is a witness at night, the rest asleep.
    assert_eq!(moved.game.witnesses_at("quad", monday(), 8), 1);
    assert_eq!(
        moved.game.witnesses_at("dorm", monday(), 8),
        1,
        "Bramley sleeps in the dorm and still counts as a prefect"
    );
}

#[test]
fn places_change_by_period_and_close_with_the_hours() {
    let mut sb = Sandbox::new("fast_runner", 1);
    let day = monday();
    let g = &sb.game;
    assert!(!g.place_open("tuck_shop", day, 0), "shut in class time");
    assert!(g.place_open("tuck_shop", day, 2));
    assert!(g.place_open("dorm", day, 8));
    assert!(!g.place_open("library", day, 8), "closed after lights-out");
    sb.game.player.place = "quad".into();
    let in_class = sb.game.describe_place("quad");
    sb.game.player.slot = 2;
    assert_ne!(
        in_class,
        sb.game.describe_place("quad"),
        "the quad looks different at lunch"
    );
    sb.game.player.place = "main_hall".into();
    assert!(
        sb.game.describe_place("main_hall").contains("trays"),
        "the hall fills at lunch"
    );
    sb.game.player.slot = 3;
    assert!(!sb.game.describe_place("main_hall").contains("trays"));
}

#[test]
fn a_locked_place_opens_for_any_one_of_its_conditions_and_the_world_can_open_it() {
    let sb = Sandbox::new("smooth_talker", 1);
    let day = monday();
    assert!(!sb.game.place_open("staffroom", day, 2));
    let tries: [(&str, Box<dyn Fn(&mut Sandbox)>); 4] = [
        (
            "learned routine",
            Box::new(|s| s.game.player.learn("staff_rota")),
        ),
        ("lockpicks", Box::new(|s| s.give("lockpicks", 1))),
        ("master key", Box::new(|s| s.give("master_key", 1))),
        (
            "deputy moved",
            Box::new(|s| s.game.world.active.push(active("deputy_moved"))),
        ),
    ];
    for (name, open_it) in &tries {
        let mut other = Sandbox::new("smooth_talker", 1);
        open_it(&mut other);
        assert!(
            other.game.place_open("staffroom", day, 2),
            "{name} opens the staffroom"
        );
        assert!(
            !other.game.place_open("staffroom", day, 0),
            "but not while the room is shut"
        );
    }
    // The east wing has no way in yet: the mystery slices add the key.
    let mut with_everything = Sandbox::new("smooth_talker", 1);
    with_everything.give("master_key", 1);
    assert!(!with_everything.game.place_open("east_wing", day, 8));
    // A closed shop: the tuck shop is shut for everyone while the effect lasts.
    let mut closed = Sandbox::new("smooth_talker", 1);
    closed.game.world.active.push(active("tuck_shop_closed"));
    assert!(!closed.game.place_open("tuck_shop", day, 2));
}

#[test]
fn walking_is_limited_each_period_and_the_map_has_a_cost() {
    let mut sb = Sandbox::new("fast_runner", 1);
    let per_period = sb.game.player.moves;
    assert_eq!(
        usize::from(per_period),
        sb.game.content.rules.moves_per_slot
    );
    // Dorm, quad, corridor, maths room: three corridors.
    sb.goto("room_maths");
    assert_eq!(sb.game.player.moves, per_period - 3);
    sb.ok(Action::Go("classroom_corridor".into()));
    sb.ok(Action::Go("quad".into()));
    assert_eq!(sb.game.player.moves, 0);
    assert!(
        sb.try_(Action::Go("dorm".into())).is_err(),
        "out of corridors this period"
    );
    sb.ok(Action::Rest);
    assert_eq!(
        sb.game.player.moves, per_period,
        "the next period starts fresh"
    );
    // One hop away is cheaper than three.
    sb.ok(Action::Go("classroom_corridor".into()));
    assert_eq!(sb.game.player.moves, per_period - 1);
    // A place that is shut cannot be walked to, even if it is close.
    assert!(
        sb.game
            .reachable()
            .iter()
            .all(|p| sb.game.place_open(p, sb.game.player.day, 1))
    );
    assert!(!sb.game.reachable().contains(&"staffroom".to_owned()));
}

#[test]
fn slipping_out_after_lights_out_costs_nerve_and_can_end_in_the_file() {
    let outcomes: Vec<_> = (0..60)
        .map(|seed| {
            let mut sb = Sandbox::new("fast_runner", seed);
            sb.game.player.slot = 8;
            sb.game.player.moves = 3;
            let nerve = sb.game.player.nerve;
            let events = sb.ok(Action::Go("quad".into()));
            (sb, nerve, events)
        })
        .collect();
    assert!(
        outcomes
            .iter()
            .any(|(sb, _, _)| sb.game.player.place == "quad"),
        "sometimes you get out"
    );
    let caught: Vec<_> = outcomes
        .iter()
        .filter(|(sb, _, _)| sb.game.player.place == "dorm")
        .collect();
    assert!(!caught.is_empty(), "sometimes a torch finds you");
    for (sb, nerve, _) in &outcomes {
        assert_eq!(
            sb.game.player.nerve,
            nerve - 1,
            "the attempt costs nerve either way"
        );
    }
    for (sb, _, events) in caught {
        assert_eq!(
            sb.game.player.file.last().map(|e| e.kind.as_str()),
            Some("out_of_bounds")
        );
        assert!(sb.game.player.detention >= 1);
        assert!(events.iter().any(|e| matches!(e, Event::FileEntry { .. })));
    }
    // With no nerve left you cannot even try.
    let mut sb = Sandbox::new("fast_runner", 1);
    sb.game.player.slot = 8;
    sb.game.player.nerve = 0;
    assert!(sb.try_(Action::Go("quad".into())).is_err());
}

#[test]
fn skipping_class_costs_grades_and_may_be_noticed_more_when_the_teacher_is_watching() {
    let skip = |seed: u64, wary: u8| {
        let mut sb = Sandbox::new("smooth_talker", seed);
        sb.game.player.cool = 0;
        sb.game.bump_wary("pell", i32::from(wary));
        let grade = map_get(&sb.game.player.grades, "maths").copied().unwrap();
        sb.ok(Action::Rest);
        (
            sb.game.player.detention,
            grade - map_get(&sb.game.player.grades, "maths").copied().unwrap(),
        )
    };
    let calm: u32 = (0..300).map(|s| u32::from(skip(s, 0).0)).sum();
    let watched: u32 = (0..300).map(|s| u32::from(skip(s, 4).0)).sum();
    assert!(watched > calm * 2, "{watched} vs {calm}");
    assert!(
        (0..20).all(|s| skip(s, 0).1 == 2),
        "missing a lesson always costs the grade"
    );
}

#[test]
fn attending_teaches_and_a_teachers_pet_learns_faster() {
    let learn = |trait_id: &str| {
        let mut sb = Sandbox::new(trait_id, 3);
        sb.goto("room_maths");
        let before = map_get(&sb.game.player.grades, "maths").copied().unwrap();
        sb.ok(Action::Attend);
        map_get(&sb.game.player.grades, "maths").copied().unwrap() - before
    };
    assert!(learn("teachers_pet") > learn("fast_runner"));
    // The wrong room is refused.
    let mut sb = Sandbox::new("fast_runner", 1);
    assert!(sb.try_(Action::Attend).is_err());
}

#[test]
fn detention_is_served_or_it_goes_on_the_file() {
    let owed = |sb: &mut Sandbox| {
        sb.game.player.detention = 1;
        sb.game.sync_day(monday() + 1);
        assert_eq!(sb.game.player.due, 1);
        sb.game.player.slot = 6;
    };
    let mut served = Sandbox::new("fast_runner", 1);
    owed(&mut served);
    served.game.player.place = "detention_room".into();
    served.ok(Action::Attend);
    assert_eq!(
        (
            served.game.player.detention,
            served.game.player.file_points()
        ),
        (0, 0)
    );

    let mut skipped = Sandbox::new("fast_runner", 1);
    owed(&mut skipped);
    skipped.game.player.place = "quad".into();
    skipped.ok(Action::Rest);
    assert!(
        skipped.game.player.detention >= 1,
        "it is still owed (and the file adds more)"
    );
    assert_eq!(
        skipped
            .game
            .player
            .file
            .last()
            .map(|e| (e.kind.as_str(), e.sev)),
        Some(("skipped_detention", 2))
    );
}

#[test]
fn the_file_has_consequences_at_each_level_and_a_full_file_ends_the_term() {
    let mut sb = Sandbox::new("fast_runner", 1);
    let rules = sb.game.content.rules.clone();
    let tiers = sb.game.content.tiers.clone();
    let add_to = |sb: &mut Sandbox, points: i32| {
        while sb.game.player.file_points() < points {
            sb.game.add_file("caught", 1);
        }
    };
    // One level at a time, each does what the table says.
    add_to(&mut sb, tiers[0].at - 1);
    assert_eq!(
        sb.game.player.detention, 0,
        "below the first level nothing happens"
    );
    add_to(&mut sb, tiers[0].at);
    assert_eq!(
        i32::from(sb.game.player.detention),
        tiers[0].amount,
        "detentions"
    );
    add_to(&mut sb, tiers[1].at);
    assert!(
        sb.game.player.privileges_until > sb.game.player.day,
        "privileges lost"
    );
    assert!(sb.try_(Action::Buy("salt".into())).is_err());
    add_to(&mut sb, tiers[2].at);
    assert!(sb.game.player.letter, "a letter home");
    add_to(&mut sb, tiers[3].at);
    assert!(sb.game.player.summons, "the headmistress");
    // The next morning she keeps you for the first period and remembers you.
    let events = sb.game.sync_day(monday() + 1);
    assert!(
        events
            .iter()
            .any(|e| e.describe().contains("headmistress's office"))
    );
    assert_eq!(sb.game.player.slot, 1);
    assert!(sb.game.player.wary_of("vane") >= 1);
    assert!(!sb.game.player.summons);
    // Privileges are lost, so no allowance that morning.
    assert_eq!(sb.game.player.cash, rules.start_cash);
    add_to(&mut sb, rules.file_full);
    assert_eq!(sb.game.player.file_points(), rules.file_full);
    assert!(
        sb.game.player.final_warning,
        "a full file is a final warning"
    );
    let events = sb.game.add_file("caught", 1);
    assert!(
        events.iter().any(|e| matches!(e, Event::Expelled)),
        "one more line ends it"
    );
    assert_eq!(sb.game.player.status, Status::Expelled);
    assert_eq!(
        sb.game.player.history.last().map(|r| r.ending.as_str()),
        Some("expelled")
    );
    assert!(
        sb.try_(Action::Rest).is_err(),
        "an expelled character cannot act"
    );
}

#[test]
fn a_week_of_good_conduct_clears_a_file_point_and_a_teachers_pet_clears_faster() {
    let after = |trait_id: &str, days: i64| {
        let mut sb = Sandbox::new(trait_id, 1);
        sb.game.add_file("caught", 2);
        sb.game.sync_day(monday() + days);
        sb.game.player.file_points()
    };
    assert_eq!(after("fast_runner", 6), 2);
    assert_eq!(after("fast_runner", 7), 1);
    assert_eq!(after("teachers_pet", 5), 1);
    assert_eq!(after("fast_runner", 5), 2);
    assert_eq!(after("fast_runner", 14), 0);
}

#[test]
fn the_term_ends_with_one_of_several_endings_depending_on_what_you_did() {
    let end = |setup: &dyn Fn(&mut Sandbox)| {
        let mut sb = Sandbox::new("fast_runner", 1);
        let last = sb.game.world.term_start + sb.game.content.rules.term_days;
        sb.game.sync_day(last - 1);
        setup(&mut sb);
        let events = sb.game.sync_day(last);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::TermEnded(_) | Event::Expelled)),
            "{events:?}"
        );
        sb.game.player.history.last().unwrap().ending.clone()
    };
    assert_eq!(end(&|_| {}), "scraped_through");
    assert_eq!(end(&|s| s.game.player.legend = 90), "phantom");
    assert_eq!(
        end(&|s| {
            s.game.player.legend = 60;
            s.game.player.file = vec![russh_bbs_door_hallpass::state::FileEntry {
                day: 0,
                kind: "caught".into(),
                sev: 5,
            }];
        }),
        "infamous"
    );
    assert_eq!(
        end(&|s| {
            s.game.player.legend = 12;
            for (_, g) in &mut s.game.player.grades {
                *g = 90;
            }
        }),
        "model_pupil"
    );
    assert_eq!(
        end(
            &|s| s.game.player.file = vec![russh_bbs_door_hallpass::state::FileEntry {
                day: 0,
                kind: "caught".into(),
                sev: 12
            }]
        ),
        "expelled"
    );
    // After the end, nothing more can be done until the next term.
    let mut sb = Sandbox::new("fast_runner", 1);
    sb.game.sync_day(sb.game.world.term_start + 42);
    assert!(!sb.game.playing());
    assert!(sb.try_(Action::Rest).is_err());
}

#[test]
fn a_new_term_starts_a_new_character_and_remembers_the_old_one() {
    let mut sb = Sandbox::new("fast_runner", 1);
    sb.game.player.legend = 33;
    let saved = sb.game.player.clone();
    let mut next = World::new(monday());
    next.term = 2;
    next.term_start = monday() + 42;
    let (game, events) = Game::open(
        &content(),
        "ann",
        Some(saved),
        Some(&next.to_bytes()),
        monday() + 42,
    );
    assert_eq!(
        game.player.status,
        Status::New,
        "a fresh character picks a trait again"
    );
    assert_eq!((game.player.term, game.player.legend), (2, 0));
    assert_eq!(game.player.history.len(), 1);
    assert_eq!(game.player.history[0].legend, 33);
    assert!(events.iter().any(|e| matches!(e, Event::TermEnded(_))));
}

#[test]
fn traits_are_earned_by_what_you_do() {
    let mut sb = Sandbox::new("fast_runner", 1);
    sb.game.player.notoriety = 50;
    let events = sb.game.check_traits();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::TraitEarned(t) if t == "known_troublemaker"))
    );
    assert!(sb.game.has_tag("troublemaker"));
    assert!(
        sb.game.player.standing_with("the_bench") >= 10,
        "the rough crowd likes you for it"
    );
    // A floor on notoriety comes with the reputation.
    sb.game.player.notoriety = 0;
    sb.game.sync_day(monday() + 3);
    assert!(sb.game.player.notoriety >= 20);
    // A clean sheet earns a different reputation.
    let mut ghost = Sandbox::new("fast_runner", 1);
    ghost.game.player.pulled = 5;
    assert!(
        ghost
            .game
            .check_traits()
            .iter()
            .any(|e| matches!(e, Event::TraitEarned(t) if t == "ghost"))
    );
    ghost.game.player.caught = 1;
    let mut caught = Sandbox::new("fast_runner", 1);
    caught.game.player.pulled = 5;
    caught.game.player.caught = 1;
    assert!(caught.game.check_traits().is_empty());
}

#[test]
fn the_shop_sells_by_hours_cash_and_what_you_have_earned() {
    let mut sb = Sandbox::new("fast_runner", 1);
    sb.game.player.slot = 2;
    sb.game.player.place = "tuck_shop".into();
    sb.ok(Action::Buy("salt".into()));
    assert_eq!(sb.game.player.count("salt"), 1);
    assert_eq!(sb.game.player.cash, sb.game.content.rules.start_cash - 2);
    assert!(
        sb.try_(Action::Buy("balls".into())).is_err(),
        "not sold here"
    );
    sb.game.player.place = "workshop".into();
    sb.game.player.slot = 5;
    assert!(
        sb.try_(Action::Buy("lockpicks".into())).is_err(),
        "the Workshop does not sell those to just anyone"
    );
    sb.game.player.legend = 30;
    sb.game.player.cash = 100;
    sb.ok(Action::Buy("lockpicks".into()));
    sb.game.player.cash = 1;
    assert!(sb.try_(Action::Buy("wire".into())).is_err(), "no cash");
    let _ = DoorRng::from_seed(0);
}

#[test]
fn the_legal_action_list_never_offers_what_apply_refuses() {
    let mut sb = Sandbox::new("smooth_talker", 5);
    for _ in 0..30 {
        let legal = sb.game.legal_actions();
        for action in &legal {
            let mut copy = sb.game.clone();
            assert!(
                copy.apply(action, &mut DoorRng::from_seed(9)).is_ok(),
                "{action:?}"
            );
        }
        let Some(action) = legal
            .into_iter()
            .find(|a| matches!(a, Action::Go(_) | Action::Attend | Action::Rest))
        else {
            break;
        };
        sb.ok(action);
        if sb.game.current_slot().is_none() {
            break;
        }
    }
}
