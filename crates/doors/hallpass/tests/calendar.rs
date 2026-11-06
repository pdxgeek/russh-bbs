//! The school calendar: the house match, the winter play, the formal dance and exam week.
//!
//! Each is a place, a window of days and the periods it runs in, with scenes whose choices depend
//! on the build, close the doors of the other paths, and pay out at the end. These tests show the
//! windows, that a scene is open only where and when it should be, that a path shuts the others,
//! what each outcome does (the pupil, and the school through the world edit), and that a recorded
//! session through the framework's real keys and store replays to the same result.
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

use common::{Sandbox, content, monday, replay_twice, run_branch, world_at, world_log};
use russh_bbs_door_hallpass::game::{Action, Game};
use russh_bbs_door_hallpass::save::encode_player;
use russh_bbs_door_hallpass::state::{Membership, map_set};

/// A pupil `term_day` days into the term, standing at `event`'s place in one of its periods.
fn at(trait_id: &str, event: &str, term_day: i64) -> Sandbox {
    let mut sb = Sandbox::on(trait_id, 7, monday() + term_day);
    sb.game.world.term_start = monday();
    stand_at(&mut sb, event);
    sb
}

fn stand_at(sb: &mut Sandbox, event: &str) {
    let spec = sb
        .game
        .content
        .calendar
        .iter()
        .find(|e| e.id == event)
        .unwrap()
        .clone();
    let day = sb.game.player.day;
    let slot = (0..sb.game.periods())
        .find(|s| {
            spec.kinds.iter().any(|k| k == sb.game.kind(day, *s))
                && sb.game.place_open(&spec.place, day, *s)
        })
        .expect("the event has a period");
    sb.game.player.slot = u8::try_from(slot).unwrap();
    sb.game.player.place = spec.place;
    sb.game.player.nerve = sb.game.player.nerve_max;
}

fn join(sb: &mut Sandbox, club: &str, rank: u8) {
    sb.game.player.life.clubs.push(Membership {
        club: club.into(),
        rank,
        merit: 20,
        last_meet: -1,
    });
}

fn scenes(sb: &Sandbox) -> Vec<String> {
    sb.game.talks_here().iter().map(|t| t.id.clone()).collect()
}

/// Start the scene and choose the answer whose label contains `label`.
fn say(sb: &mut Sandbox, talk: &str, label: &str) {
    sb.ok(Action::Talk(talk.into()));
    answer(sb, label);
}

fn answer(sb: &mut Sandbox, label: &str) {
    let view = sb.game.scene_view().expect("a scene is under way");
    let (i, c) = view
        .choices
        .iter()
        .enumerate()
        .find(|(_, c)| c.label.contains(label))
        .unwrap_or_else(|| panic!("no answer `{label}` in {:?}", view.choices));
    assert!(c.open, "`{}` is shut: it wants {}", c.label, c.wants);
    sb.ok(Action::Choose(u8::try_from(i).unwrap()));
}

fn open_answers(sb: &mut Sandbox, talk: &str) -> Vec<String> {
    sb.ok(Action::Talk(talk.into()));
    let v = sb.game.scene_view().unwrap();
    let open = v
        .choices
        .into_iter()
        .filter(|c| c.open)
        .map(|c| c.label)
        .collect();
    sb.ok(Action::Abandon);
    open
}

// ------------------------------------------------------------------ the shape of the calendar

#[test]
fn the_calendar_has_four_events_in_the_term_each_with_enough_scenes_to_matter() {
    let c = content();
    let ids: Vec<&str> = c.calendar.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        ids,
        ["house_match", "winter_play", "formal_dance", "exam_week"]
    );
    let mut last_to = -1;
    for e in &c.calendar {
        assert!(e.to < c.rules.term_days, "{} ends inside the term", e.id);
        assert!(e.from > last_to, "{} does not overlap the one before", e.id);
        last_to = e.to;
        let mine: Vec<_> = c
            .talks
            .iter()
            .filter(|t| t.event.as_deref() == Some(e.id.as_str()))
            .collect();
        assert!(mine.len() >= 7, "{} has {} scenes", e.id, mine.len());
        // A path is chosen first and followed later: the opening scene sets flags the others ask for.
        let choices: usize = mine
            .iter()
            .flat_map(|t| t.node.iter())
            .map(|n| n.choice.len())
            .sum();
        assert!(choices >= 24, "{} has {choices} choices", e.id);
    }
    // The mystery and the term events do not crowd the finale.
    assert!(last_to < c.mystery.rules.finale_day);
}

#[test]
fn a_scene_is_open_only_at_the_place_in_the_periods_and_on_the_days_of_its_event() {
    let c = content();
    for e in &c.calendar {
        let first = c
            .talks
            .iter()
            .find(|t| t.event.as_deref() == Some(e.id.as_str()) && t.when.len() <= 2)
            .unwrap();
        let day = (e.from..=e.to)
            .find(|d| {
                let mut sb = at("smooth_talker", &e.id, *d);
                for cond in &first.when {
                    common::satisfy(&mut sb.game, cond);
                }
                sb.game.world.term_start = monday();
                sb.game.talk_open(first)
            })
            .unwrap_or_else(|| panic!("{} never opens", first.id));
        let ready = || {
            let mut sb = at("smooth_talker", &e.id, day);
            for cond in &first.when {
                common::satisfy(&mut sb.game, cond);
            }
            sb.game.world.term_start = monday() + 0;
            sb
        };
        let mut sb = ready();
        sb.game.world.term_start = monday();
        assert!(sb.game.talk_open(first), "{} is open", first.id);
        // The wrong place.
        let mut elsewhere = ready();
        elsewhere.game.player.place = "dorm".into();
        assert!(
            !elsewhere.game.talk_open(first),
            "{}: wrong place",
            first.id
        );
        // The wrong period: a class.
        let mut in_class = ready();
        in_class.game.player.slot = 0;
        assert!(!in_class.game.talk_open(first), "{}: in class", first.id);
        // Before and after its days.
        for off in [-5_i64, 40] {
            let mut sb = ready();
            sb.game.world.term_start = monday() - (day - monday() + 0) + off;
            let term_day = sb.game.term_day();
            if !(e.from..=e.to).contains(&term_day) {
                assert!(!sb.game.talk_open(first), "{}: day {term_day}", first.id);
            }
        }
    }
}

#[test]
fn the_morning_bell_names_the_event_on_its_days_and_only_then() {
    let c = content();
    for e in &c.calendar {
        let mut on = Sandbox::on("smooth_talker", 3, monday() + e.from - 1);
        on.game.world.term_start = monday();
        let said: Vec<String> = on
            .game
            .sync_day(monday() + e.from)
            .iter()
            .map(|ev| ev.describe())
            .collect();
        assert!(
            said.iter()
                .any(|l| l.contains("On the calendar") && l.contains(&e.name)),
            "{}: {said:?}",
            e.id
        );
        let mut off = Sandbox::on("smooth_talker", 3, monday() + e.to + 1);
        off.game.world.term_start = monday();
        let said: Vec<String> = off
            .game
            .sync_day(monday() + e.to + 2)
            .iter()
            .map(|ev| ev.describe())
            .collect();
        assert!(
            !said.iter().any(|l| l.contains(&format!("{}:", e.name))),
            "{}: not after its days",
            e.id
        );
    }
}

// ------------------------------------------------------------------ the house match

#[test]
fn the_house_match_a_place_in_the_boat_needs_a_club_rank_and_the_choice_shuts_the_other_jobs() {
    // A plain pupil is not offered the boat; a rower is.
    let mut plain = at("fast_runner", "house_match", 17);
    let open = open_answers(&mut plain, "match_pick");
    assert!(!open.iter().any(|l| l.contains("Eight")), "{open:?}");
    assert!(open.iter().any(|l| l.contains("stands")));
    let mut rower = at("fast_runner", "house_match", 17);
    join(&mut rower, "rowing", 2);
    let open = open_answers(&mut rower, "match_pick");
    assert!(open.iter().any(|l| l.contains("Eight")), "{open:?}");
    // Different builds are offered different jobs on the same team sheet.
    let mut tinker = at("tinkerer", "house_match", 17);
    let tinker_jobs = open_answers(&mut tinker, "match_pick");
    assert!(tinker_jobs.iter().any(|l| l.contains("scoreboard")));
    let mut talker = at("smooth_talker", "house_match", 17);
    talker.game.player.cool = 60;
    let talker_jobs = open_answers(&mut talker, "match_pick");
    assert!(talker_jobs.iter().any(|l| l.contains("singing")));
    assert_ne!(tinker_jobs, talker_jobs);
    // Taking the boat opens the boat's scene on the day and shuts the touchline and the board.
    say(&mut rower, "match_pick", "Eight");
    assert!(rower.game.player.knows("match_crew"));
    rower.game.world.term_start = monday() - 1;
    stand_at(&mut rower, "house_match");
    let here = scenes(&rower);
    assert!(here.contains(&"match_crew".to_owned()), "{here:?}");
    assert!(!here.contains(&"match_board".to_owned()), "{here:?}");
    assert!(!here.contains(&"match_line".to_owned()), "{here:?}");
    assert!(!here.contains(&"match_stands".to_owned()), "{here:?}");
    // The team sheet is had once.
    assert!(!here.contains(&"match_pick".to_owned()));
}

#[test]
fn winning_the_match_takes_nerve_and_the_school_is_changed_for_everyone_losing_changes_only_you() {
    let race = |nerve: u8| {
        let mut sb = at("fast_runner", "house_match", 18);
        join(&mut sb, "rowing", 2);
        sb.game.player.learn("match_crew");
        sb.game.player.nerve = nerve;
        let before = sb.game.player.life.bond_with("imogen");
        sb.try_(Action::Talk("match_crew".into()))
            .expect("the race is on");
        (sb, before)
    };
    // With nerve to spare the push is open, and it wins the match for the school.
    let (mut strong, _) = race(5);
    let view = strong.game.scene_view().unwrap();
    assert!(view.choices[0].open && view.choices[0].label.contains("Push"));
    strong.ok(Action::Choose(0));
    assert!(strong.game.player.knows("match_won"));
    assert!(
        strong.game.world.has("match_glory"),
        "the whole school feels it"
    );
    assert!(strong.game.player.life.bond_with("imogen") >= 3);
    // Without it the push is shut and says what it wants; the steady row loses and costs nobody else.
    let (mut tired, before) = race(1);
    let view = tired.game.scene_view().unwrap();
    assert!(!view.choices[0].open && view.choices[0].wants.contains("nerve"));
    assert!(tired.try_(Action::Choose(0)).is_err());
    tired.ok(Action::Choose(3));
    assert!(tired.game.player.knows("match_lost") && !tired.game.player.knows("match_won"));
    assert!(!tired.game.world.has("match_glory"));
    assert!(tired.game.player.life.bond_with("imogen") < before);
    // The troublemaker's win is a different one: a line on the file.
    let mut rough = at("fast_runner", "house_match", 18);
    join(&mut rough, "rowing", 2);
    rough.game.player.learn("match_crew");
    rough.game.player.traits.push("known_troublemaker".into());
    let file = rough.game.player.file_points();
    say(&mut rough, "match_crew", "crab");
    assert!(rough.game.player.knows("match_won") && rough.game.player.file_points() > file);
    assert!(!rough.game.world.has("match_glory"), "no glory for a foul");
}

#[test]
fn the_supper_pays_out_what_the_pupil_did_not_a_flat_reward() {
    let supper = |prep: &dyn Fn(&mut Sandbox)| -> Vec<String> {
        let mut sb = at("fast_runner", "house_match", 19);
        prep(&mut sb);
        open_answers(&mut sb, "match_supper")
    };
    let winner = supper(&|sb| {
        sb.game.player.learn("match_crew");
        sb.game.player.learn("match_won");
    });
    let loser = supper(&|sb| {
        sb.game.player.learn("match_crew");
        sb.game.player.learn("match_lost");
    });
    let spy = supper(&|sb| sb.game.player.learn("match_signals"));
    let nobody = supper(&|_| {});
    assert!(winner.iter().any(|l| l.contains("ribbon")));
    assert!(loser.iter().any(|l| l.contains("well rowed")));
    assert!(spy.iter().any(|l| l.contains("Hollin House's shed")));
    assert_eq!(nobody, ["Eat your cake and go quietly"]);
    for set in [&winner, &loser, &spy] {
        assert_ne!(set, &nobody);
    }
    // And it opens a door afterwards: the signals unlock a scheme nobody else has.
    let mut spy = at("fast_runner", "house_match", 19);
    spy.game.player.learn("match_signals");
    let has = |sb: &Sandbox| {
        sb.game
            .content
            .methods
            .iter()
            .any(|m| m.id == "hollin_flag" && sb.game.all_hold(&m.requires))
    };
    assert!(has(&spy));
    assert!(!has(&at("fast_runner", "house_match", 19)));
}

// ------------------------------------------------------------------ the winter play

#[test]
fn the_winter_play_gives_each_build_its_own_part_and_the_part_its_own_evening() {
    let casting = |trait_id: &str, prep: &dyn Fn(&mut Sandbox)| {
        let mut sb = at(trait_id, "winter_play", 23);
        prep(&mut sb);
        open_answers(&mut sb, "play_casting")
    };
    let lead = casting("smooth_talker", &|sb| join(sb, "drama", 2));
    let villain = casting("smooth_talker", &|sb| {
        map_set(&mut sb.game.player.xp, "bluff", 20);
    });
    let weather = casting("tinkerer", &|_| {});
    let nothing = casting("fast_runner", &|_| {});
    assert!(lead.iter().any(|l| l.contains("understudy")));
    assert!(villain.iter().any(|l| l.contains("Bursar")));
    assert!(weather.iter().any(|l| l.contains("lighting rig")));
    assert_eq!(
        nothing,
        ["Sell programmes at the door, and see everything for free"]
    );
    assert!(lead != villain && villain != weather);
    // A part is a path: the lead's scene is closed to the pupil who sold programmes.
    let mut door = at("fast_runner", "winter_play", 23);
    say(&mut door, "play_casting", "programmes");
    door.game.world.term_start = monday() - 2;
    stand_at(&mut door, "winter_play");
    let here = scenes(&door);
    assert!(here.contains(&"play_door".to_owned()), "{here:?}");
    assert!(!here.contains(&"play_lead".to_owned()));
    assert!(!here.contains(&"play_villain".to_owned()));
}

#[test]
fn the_lead_in_two_acts_a_risk_in_the_dress_rehearsal_pays_on_the_night_and_triumph_is_shared() {
    let mut sb = at("smooth_talker", "winter_play", 25);
    join(&mut sb, "drama", 2);
    map_set(&mut sb.game.player.xp, "bluff", 20);
    sb.game.player.learn("play_lead");
    sb.ok(Action::Talk("play_lead".into()));
    // The dress rehearsal: the risky speech is on offer to a pupil with the bluff for it.
    answer(&mut sb, "risk with the big speech");
    assert!(sb.game.player.knows("play_risk"));
    // The same scene carries on to the night (one conversation, two beats), and only now is the
    // speech she promised herself she would not cry through open.
    let view = sb.game.scene_view().unwrap();
    assert!(view.text.contains("Opening night"));
    answer(&mut sb, "speech you promised yourself");
    assert!(sb.game.player.knows("play_hit"));
    assert!(sb.game.world.has("play_triumph"), "the hall remembers it");
    // The pupil who played it straight both times has a competent review and no world change.
    let mut flat = at("smooth_talker", "winter_play", 25);
    flat.game.player.learn("play_lead");
    flat.ok(Action::Talk("play_lead".into()));
    answer(&mut flat, "straight, as written");
    let open: Vec<_> = flat
        .game
        .scene_view()
        .unwrap()
        .choices
        .into_iter()
        .filter(|c| c.open)
        .map(|c| c.label)
        .collect();
    assert_eq!(
        open,
        ["Play it straight"],
        "the risk was the only road to the speech"
    );
    answer(&mut flat, "Play it straight");
    assert!(flat.game.player.knows("play_flat") && !flat.game.world.has("play_triumph"));
    // The curtain call pays by what happened: the star is thanked, the flat cast are comforted.
    let mut star = at("smooth_talker", "winter_play", 26);
    star.game.player.learn("play_lead");
    star.game.player.learn("play_hit");
    let star_open = open_answers(&mut star, "play_curtain");
    let mut crew = at("tinkerer", "winter_play", 26);
    crew.game.player.learn("play_flat");
    let flat_open = open_answers(&mut crew, "play_curtain");
    assert!(star_open.iter().any(|l| l.contains("bow with the cast")));
    assert!(!flat_open.iter().any(|l| l.contains("bow with the cast")));
    assert!(flat_open.iter().any(|l| l.contains("whatever happened")));
}

#[test]
fn the_prompt_corner_finds_what_the_stage_hides_and_a_haunted_hat_box_opens_a_conversation() {
    let mut sb = at("smooth_talker", "winter_play", 25);
    join(&mut sb, "drama", 1);
    map_set(&mut sb.game.player.xp, "sneak", 10);
    sb.game.player.learn("play_prompt");
    say(&mut sb, "play_prompt", "green-room corridor");
    assert!(sb.game.player.knows("old_programme"));
    // That programme is what Madame Thorne is shown after the last curtain; she tells her story.
    let mut after = at("smooth_talker", "winter_play", 26);
    after.game.player.learn("old_programme");
    say(&mut after, "play_curtain", "old programme");
    assert!(after.game.player.knows("thorne_knew"));
    assert!(after.game.player.life.bond_with("thorne") >= 5);
}

// ------------------------------------------------------------------ the formal dance

#[test]
fn the_dance_partner_is_chosen_by_friendship_and_the_partners_scene_is_closed_to_everyone_else() {
    let invite = |prep: &dyn Fn(&mut Sandbox)| {
        let mut sb = at("smooth_talker", "formal_dance", 30);
        prep(&mut sb);
        open_answers(&mut sb, "dance_invite")
    };
    let friend = invite(&|sb| sb.game.player.life.add_bond("wren", 10));
    let rebel = invite(&|sb| sb.game.player.add_standing("the_bench", 8));
    let alone = invite(&|_| {});
    assert!(friend.iter().any(|l| l.contains("Wren")));
    assert!(!friend.iter().any(|l| l.contains("Sol")));
    assert!(rebel.iter().any(|l| l.contains("Bench")));
    assert_eq!(
        alone,
        ["Go on your own, and stand near the door where the view is best"]
    );
    // Asking Wren opens Wren's dance and no other.
    let mut sb = at("smooth_talker", "formal_dance", 30);
    sb.game.player.life.add_bond("wren", 10);
    sb.game.player.life.add_bond("sol", 10);
    say(&mut sb, "dance_invite", "Wren");
    sb.game.world.term_start = monday() - 1;
    stand_at(&mut sb, "formal_dance");
    let here = scenes(&sb);
    assert!(here.contains(&"dance_wren".to_owned()), "{here:?}");
    assert!(!here.contains(&"dance_sol".to_owned()), "{here:?}");
    assert!(!here.contains(&"dance_gang".to_owned()));
    // The headmistress's waltz is for everyone who dares, and she remembers it.
    assert!(here.contains(&"dance_waltz".to_owned()));
}

#[test]
fn pulling_the_fuse_at_the_dance_needs_the_hands_and_the_friend_and_darkens_the_school_for_a_day() {
    let on_the_roof = |tinker: bool, bond: i32| {
        let mut sb = at("fast_runner", "formal_dance", 31);
        sb.game.player.learn("dance_sol");
        sb.game.player.life.add_bond("sol", bond);
        if tinker {
            map_set(&mut sb.game.player.xp, "tinker", 10);
        }
        sb.ok(Action::Talk("dance_sol".into()));
        sb
    };
    let mut ready = on_the_roof(true, 12);
    let view = ready.game.scene_view().unwrap();
    let fuse = view
        .choices
        .iter()
        .position(|c| c.label.contains("fuse"))
        .unwrap();
    assert!(view.choices[fuse].open);
    ready.ok(Action::Choose(u8::try_from(fuse).unwrap()));
    assert!(ready.game.world.has("dance_blackout"));
    assert!(ready.game.player.heat >= 5, "and there is heat for it");
    for (tinker, bond) in [(false, 12), (true, 0)] {
        let mut sb = on_the_roof(tinker, bond);
        let view = sb.game.scene_view().unwrap();
        let fuse = view
            .choices
            .iter()
            .position(|c| c.label.contains("fuse"))
            .unwrap();
        assert!(!view.choices[fuse].open, "tinker {tinker} bond {bond}");
        assert!(
            sb.try_(Action::Choose(u8::try_from(fuse).unwrap()))
                .is_err()
        );
    }
}

#[test]
fn the_morning_after_remembers_who_waltzed_who_watched_and_who_took_the_east_door() {
    let morning = |prep: &dyn Fn(&mut Sandbox)| {
        let mut sb = at("fast_runner", "formal_dance", 32);
        prep(&mut sb);
        open_answers(&mut sb, "dance_last")
    };
    let waltzer = morning(&|sb| sb.game.player.learn("waltzed_with_vane"));
    let watcher = morning(&|sb| {
        sb.game.player.learn("fenn_watches_the_wing");
        map_set(&mut sb.game.player.xp, "sneak", 10);
    });
    let nobody = morning(&|_| {});
    assert!(waltzer.iter().any(|l| l.contains("Own up")));
    assert!(watcher.iter().any(|l| l.contains("East door")));
    assert!(
        !nobody
            .iter()
            .any(|l| l.contains("Own up") || l.contains("East door"))
    );
}

// ------------------------------------------------------------------ exam week

#[test]
fn exam_week_preparation_decides_what_the_papers_offer_and_the_marks_move() {
    let prep = |build: &dyn Fn(&mut Sandbox), pick: &str| -> Sandbox {
        let mut sb = at("smooth_talker", "exam_week", 34);
        build(&mut sb);
        say(&mut sb, "exam_prep", pick);
        sb
    };
    let swot = prep(&|_| {}, "Revise properly");
    assert!(swot.game.player.knows("exam_swot"));
    let notes = prep(
        &|sb| sb.game.player.life.add_bond("jonty", 8),
        "Jonty's notes",
    );
    assert!(notes.game.player.knows("exam_notes"));
    // The notes open the history paper's best answer, and only the notes do.
    let words = |sb: &Sandbox| {
        let mut sb = Sandbox {
            game: sb.game.clone(),
            rng: russh_bbs_door::DoorRng::from_seed(2),
        };
        sb.game.world.term_start = monday() - 1;
        stand_at(&mut sb, "exam_week");
        open_answers(&mut sb, "exam_words")
    };
    assert!(words(&notes).iter().any(|l| l.contains("pictures")));
    assert!(!words(&swot).iter().any(|l| l.contains("pictures")));
    // A crib is a risk: it helps the marks and heats the pupil.
    let mut crib = prep(
        &|sb| {
            map_set(&mut sb.game.player.xp, "bluff", 20);
            sb.give("forgery_kit", 1);
        },
        "crib",
    );
    crib.game.world.term_start = monday() - 1;
    stand_at(&mut crib, "exam_week");
    let heat = crib.game.player.heat;
    say(&mut crib, "exam_numbers", "Read the crib");
    assert!(crib.game.player.heat > heat);
}

#[test]
fn a_bought_paper_is_found_out_a_reported_one_resets_the_school_and_a_swapped_one_goes_unsolved() {
    // Buying: top marks on the day, a line on the file at the results.
    let mut bought = at("smooth_talker", "exam_week", 35);
    bought.game.player.cash = 10;
    bought.game.player.life.add_bond("dot", 4);
    say(&mut bought, "exam_leak", "Buy the paper");
    assert!(bought.game.player.knows("exam_cheated"));
    let file = bought.game.player.file_points();
    bought.game.world.term_start = monday() - 2;
    stand_at(&mut bought, "exam_week");
    say(&mut bought, "exam_results", "called to the front");
    assert!(bought.game.player.file_points() > file, "found out");
    // Reporting: the school's patrols thicken for three days, whoever else it touches.
    let mut told = at("smooth_talker", "exam_week", 35);
    say(&mut told, "exam_leak", "Tell Mr. Marsh");
    assert!(told.game.player.knows("exam_reported"));
    assert!(told.game.world.has("exam_resit"));
    assert!(
        told.game.player.life.bond_with("dot") < 0,
        "and Dot never forgets"
    );
    // Swapping: an anonymous joke on the notice board, and the cheats are baffled.
    let mut swapped = at("smooth_talker", "exam_week", 35);
    map_set(&mut swapped.game.player.xp, "bluff", 20);
    swapped.give("forgery_kit", 1);
    say(&mut swapped, "exam_leak", "joke paper");
    assert!(swapped.game.player.knows("exam_swapped"));
    assert!(!swapped.game.world.has("exam_resit"));
    // A pupil who walks away is offered the plain results and nothing about any of it.
    let mut clean = at("smooth_talker", "exam_week", 37);
    let open = open_answers(&mut clean, "exam_results");
    assert!(
        !open
            .iter()
            .any(|l| l.contains("front") || l.contains("joke"))
    );
}

#[test]
fn exam_week_changes_marks_by_the_work_not_by_a_flat_prize() {
    let mark = |prep: &dyn Fn(&mut Sandbox), talk: &str, pick: &str, subject: &str| -> i32 {
        let mut sb = at("smooth_talker", "exam_week", 36);
        for (_, g) in &mut sb.game.player.grades {
            *g = 70;
        }
        prep(&mut sb);
        say(&mut sb, talk, pick);
        sb.game
            .stat(&russh_bbs_door_hallpass::cond::Stat::Grade(subject.into()))
    };
    let steady = mark(&|_| {}, "exam_numbers", "line by line", "maths");
    let blank = mark(&|_| {}, "exam_numbers", "leave the rest blank", "maths");
    assert!(steady > blank + 2, "{steady} vs {blank}");
    let painter = mark(&|_| {}, "exam_viva", "kettle and the onion", "art");
    let sketch = mark(&|_| {}, "exam_viva", "Sketch an onion", "art");
    assert!(painter > sketch + 2, "{painter} vs {sketch}");
}

// ------------------------------------------------------------------ recorded and replayed

fn staged(
    trait_id: &str,
    event: &str,
    term_day: i64,
    prep: &dyn Fn(&mut Sandbox),
) -> (Game, Sandbox) {
    let mut sb = at(trait_id, event, term_day);
    prep(&mut sb);
    (sb.game.clone(), sb)
}

/// Replay the keys as a world-aware log (twice, identically), and once through the framework's
/// real store and keys; the two must leave the same pupil. Returns what the store run left.
async fn replay(sb: &Sandbox, term_day: i64, keys: &str) -> common::Branch {
    let world = {
        let mut w = world_at(term_day);
        w.active = sb.game.world.active.clone();
        w
    };
    let log = world_log(&sb.game.player, &world, keys, 11);
    let run = replay_twice(&log);
    let stored = run_branch(&sb.game.player, &world, keys, 11).await;
    assert_eq!(
        run.save.as_deref(),
        Some(encode_player(&stored.player).as_slice()),
        "{keys}: the replay and the store leave the same pupil"
    );
    assert!(run.world_edits >= 1, "{keys}: it changed the school");
    stored
}

/// The key text that starts `talk` (a position in the day's list) and presses `choices`.
fn route(sb: &Sandbox, talk: &str, choices: &str) -> String {
    let here = scenes(sb);
    let at = here
        .iter()
        .position(|t| t == talk)
        .unwrap_or_else(|| panic!("{talk} not on offer: {here:?}"));
    format!("t{}{}", at + 1, choices)
}

#[tokio::test]
async fn every_event_replays_a_recorded_session_to_the_same_result() {
    // The house match: the stroke pushes for the finish.
    let (_, mut m) = staged("fast_runner", "house_match", 18, &|sb| {
        join(sb, "rowing", 2);
        sb.game.player.learn("match_crew");
        sb.game.player.nerve = 6;
    });
    m.game.player.nerve = 6;
    let keys = route(&m, "match_crew", "1");
    let m = replay(&m, 18, &keys).await;
    assert!(m.player.knows("match_won"), "{:?}", m.player.knowledge);
    assert!(m.world.has("match_glory"));
    // The winter play: the lead takes the risk, then the speech.
    let (_, p) = staged("smooth_talker", "winter_play", 25, &|sb| {
        join(sb, "drama", 2);
        map_set(&mut sb.game.player.xp, "bluff", 20);
        sb.game.player.learn("play_lead");
    });
    let keys = route(&p, "play_lead", "21");
    let p = replay(&p, 25, &keys).await;
    assert!(p.player.knows("play_hit") && p.world.has("play_triumph"));
    // The dance: pulling the fuse from the roof.
    let (_, d) = staged("fast_runner", "formal_dance", 31, &|sb| {
        sb.game.player.learn("dance_sol");
        sb.game.player.life.add_bond("sol", 12);
        map_set(&mut sb.game.player.xp, "tinker", 10);
    });
    let keys = route(&d, "dance_sol", "2");
    let d = replay(&d, 31, &keys).await;
    assert!(d.world.has("dance_blackout"));
    // Exam week: telling Mr. Marsh about the paper.
    let (_, e) = staged("smooth_talker", "exam_week", 35, &|_| {});
    let keys = route(&e, "exam_leak", "2");
    let e = replay(&e, 35, &keys).await;
    assert!(e.player.knows("exam_reported") && e.world.has("exam_resit"));
}
