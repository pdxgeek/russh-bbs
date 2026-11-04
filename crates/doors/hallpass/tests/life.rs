//! School life (M4-H2): class minigames, clubs, friendships and rivalries, conversations. The RPG
//! rule is tested in the open: the same scene shown to different builds offers different options
//! and ends differently.
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

use common::{Sandbox, Setup, arrange, content, meet, monday, satisfy};
use russh_bbs_door::DoorRng;
use russh_bbs_door_hallpass::game::{Action, Event, Game};
use russh_bbs_door_hallpass::save::{decode_player, encode_player};
use russh_bbs_door_hallpass::state::{Membership, map_get, map_set};
use russh_bbs_door_hallpass::talk::Regard;

/// The first day and period of the week on which `subject` is taught.
fn lesson_slot(subject: &str) -> (i64, usize) {
    let sb = Sandbox::new("fast_runner", 1);
    for day in monday()..monday() + 7 {
        for slot in 0..sb.game.periods() {
            if sb.game.activity(day, slot) == format!("class:{subject}") {
                return (day, slot);
            }
        }
    }
    panic!("{subject} is never taught");
}

/// A character standing in `subject`'s classroom at lesson time.
fn in_class(trait_id: &str, subject: &str, seed: u64) -> Sandbox {
    let (day, slot) = lesson_slot(subject);
    let mut sb = Sandbox::on(trait_id, seed, monday());
    sb.game.sync_day(day);
    sb.game.player.slot = u8::try_from(slot).unwrap();
    let room = sb.game.content.subject(subject).unwrap().room.clone();
    sb.game.player.place = room;
    sb
}

/// A character at `place` in the first period of kind `kind` on a day the weekday suits.
fn at(trait_id: &str, place: &str, kind: &str, weekday: usize, seed: u64) -> Sandbox {
    let day = (monday()..monday() + 7)
        .find(|d| Game::weekday(*d) == weekday)
        .unwrap();
    let mut sb = Sandbox::on(trait_id, seed, monday());
    sb.game.sync_day(day);
    let slot = (0..sb.game.periods())
        .find(|s| sb.game.kind(day, *s) == kind)
        .unwrap();
    sb.game.player.slot = u8::try_from(slot).unwrap();
    place.clone_into(&mut sb.game.player.place);
    sb
}

fn join_club(sb: &mut Sandbox, club: &str, rank: u8, merit: u16) {
    sb.game.player.life.clubs.push(Membership {
        club: club.into(),
        rank,
        merit,
        last_meet: -1,
    });
}

// ------------------------------------------------------------------ class minigames

#[test]
fn every_subject_has_its_own_minigame_and_the_result_feeds_grade_skill_and_teacher() {
    let c = content();
    assert!(c.subjects.len() >= 6);
    let kinds: std::collections::BTreeSet<&str> =
        c.lessons.iter().map(|l| l.kind.as_str()).collect();
    assert_eq!(
        kinds.len(),
        c.subjects.len(),
        "one minigame kind per subject: {kinds:?}"
    );
    let gains = c.rules.lesson_gains.clone();
    for subject in &c.subjects {
        let spec = c.lesson_for(&subject.id).unwrap();
        // Everything right.
        let mut sb = in_class("fast_runner", &subject.id, 3);
        let before = i32::from(
            map_get(&sb.game.player.grades, &subject.id)
                .copied()
                .unwrap(),
        );
        let slot_before = sb.game.player.slot;
        let events = sb.attend(c.rules.lesson_rounds);
        let after = i32::from(
            map_get(&sb.game.player.grades, &subject.id)
                .copied()
                .unwrap(),
        );
        assert_eq!(
            after - before,
            gains[c.rules.lesson_rounds],
            "{}",
            subject.id
        );
        assert_eq!(
            sb.game.player.slot,
            slot_before + 1,
            "the lesson takes the period"
        );
        assert_eq!(sb.game.player.life.perfect, 1);
        assert!(
            map_get(&sb.game.player.xp, &spec.skill)
                .copied()
                .unwrap_or(0)
                >= 3,
            "{} trains {}",
            subject.id,
            spec.skill
        );
        assert_eq!(sb.game.player.life.bond_with(&subject.teacher), 2);
        assert!(events.iter().any(|e| e.describe().contains("right, grade")));
        // Everything wrong.
        let mut bad = in_class("fast_runner", &subject.id, 3);
        bad.attend(0);
        let low = i32::from(
            map_get(&bad.game.player.grades, &subject.id)
                .copied()
                .unwrap(),
        );
        assert_eq!(low - before, gains[0], "{}", subject.id);
        assert_eq!(bad.game.player.life.bond_with(&subject.teacher), -1);
    }
}

#[test]
fn a_lesson_is_a_pure_function_of_its_seed_and_survives_leaving_the_door() {
    let mut sb = in_class("fast_runner", "maths", 9);
    sb.ok(Action::Attend);
    let first = sb.game.lesson_view().unwrap();
    sb.ok(Action::Answer(u8::try_from(first.round.answer).unwrap()));
    let second = sb.game.lesson_view().unwrap();
    assert_eq!(second.number, 1);
    // Save and come back: the same puzzle, and the answered one is not re-asked.
    let bytes = encode_player(&sb.game.player);
    let back = decode_player(&bytes).unwrap();
    let (game, _) = Game::open(&content(), "ann", Some(back), None, sb.game.player.day);
    assert_eq!(game.lesson_view().unwrap(), second);
    // Nothing else can be done mid-lesson.
    assert!(sb.try_(Action::Rest).is_err());
    assert!(sb.try_(Action::Sleep).is_err());
    // Walking out costs the period like a skipped class.
    let slot = sb.game.player.slot;
    sb.ok(Action::Abandon);
    assert_eq!(sb.game.player.slot, slot + 1);
    assert!(sb.game.player.life.lesson.is_none());
    // A lesson left open overnight is over by morning.
    let mut late = in_class("fast_runner", "maths", 9);
    late.ok(Action::Attend);
    let events = late.game.sync_day(late.game.player.day + 1);
    assert!(late.game.player.life.lesson.is_none());
    assert!(events.iter().any(|e| e.describe().contains("unfinished")));
}

#[test]
fn the_same_lesson_shows_two_builds_different_options_and_grades_make_it_harder() {
    let mut plain = in_class("fast_runner", "maths", 5);
    let mut handy = in_class("tinkerer", "maths", 5);
    map_set(&mut handy.game.player.xp, "tinker", 40);
    plain.ok(Action::Attend);
    handy.ok(Action::Attend);
    let a = plain.game.lesson_view().unwrap();
    let b = handy.game.lesson_view().unwrap();
    assert_eq!(a.round, b.round, "the same puzzle");
    assert!(a.open.iter().all(|o| *o), "no help without the skill");
    assert!(
        b.open.iter().filter(|o| **o).count() < b.open.len(),
        "a practised tinkerer can rule answers out: {b:?}"
    );
    let struck = b.open.iter().position(|o| !*o).unwrap();
    assert!(
        handy
            .try_(Action::Answer(u8::try_from(struck).unwrap()))
            .is_err()
    );
    assert_ne!(struck, b.round.answer, "the right answer is never struck");
    // Harder puzzles for a better pupil: the level comes from the grade.
    let mut star = in_class("fast_runner", "maths", 5);
    map_set(&mut star.game.player.grades, "maths", 90);
    star.ok(Action::Attend);
    assert_eq!(star.game.player.life.lesson.as_ref().unwrap().level, 2);
    assert_eq!(plain.game.player.life.lesson.as_ref().unwrap().level, 0);
}

#[test]
fn grades_change_what_the_staff_allow_and_what_they_suspect() {
    // What they allow: the restricted stacks open for a pupil with the history mark.
    let day = monday();
    let mut sb = Sandbox::new("fast_runner", 1);
    let slot = (0..9).find(|s| sb.game.kind(day, *s) == "free").unwrap();
    assert!(!sb.game.place_open("restricted_stacks", day, slot));
    map_set(&mut sb.game.player.grades, "history", 70);
    assert!(sb.game.place_open("restricted_stacks", day, slot));
    // ...and Mr. Alder lends the key only to a pupil he likes who knows his subject.
    let alder = sb.game.content.npc("alder").unwrap().clone();
    let keys = alder.ally.iter().find(|a| a.role == "keys").unwrap();
    assert!(!sb.game.all_hold(&keys.needs));
    sb.game.player.life.add_bond("alder", 6);
    assert!(sb.game.all_hold(&keys.needs));
    // What they suspect: better marks make a pulled-off scheme harder to pin on you.
    let mut s = Sandbox::new("smooth_talker", 2);
    let id = arrange(&mut s, "pell", "salt_swap", &Setup::default());
    let scheme = s.game.player.scheme(id).unwrap().clone();
    s.game.player.heat = 40;
    let trace = |s: &mut Sandbox, grade: u8| {
        for (_, g) in &mut s.game.player.grades {
            *g = grade;
        }
        s.game.trace_chance(&scheme, 0)
    };
    let (good, poor) = (trace(&mut s, 95), trace(&mut s, 25));
    assert!(poor >= good + 12, "{poor} vs {good}");
    // And they notice an empty seat more readily when the marks are poor and the teacher no friend.
    let skip = |grade: u8, bond: i32| -> u32 {
        (0..300u64)
            .map(|seed| {
                let mut sb = in_class("fast_runner", "maths", seed);
                map_set(&mut sb.game.player.grades, "maths", grade);
                sb.game.player.life.add_bond("pell", bond);
                let events = sb.ok(Action::Rest);
                u32::from(events.iter().any(|e| e.describe().contains("empty seat")))
            })
            .sum()
    };
    let (kept, lost) = (skip(95, 12), skip(30, -12));
    assert!(lost > kept * 2, "{lost} vs {kept}");
}

#[test]
fn fridays_report_rewards_good_marks_and_files_poor_ones() {
    let friday = (monday()..monday() + 7)
        .find(|d| Game::weekday(*d) == 4)
        .unwrap();
    let mut poor = Sandbox::new("fast_runner", 1);
    map_set(&mut poor.game.player.grades, "art", 30);
    let events = poor.game.sync_day(friday);
    assert!(
        poor.game
            .player
            .file
            .iter()
            .any(|e| e.kind == "poor_report")
    );
    assert!(events.iter().any(|e| e.describe().contains("poor mark")));
    let mut star = Sandbox::new("fast_runner", 1);
    for (_, g) in &mut star.game.player.grades {
        *g = 95;
    }
    let cool = star.game.player.cool;
    star.game.sync_day(friday);
    assert!(star.game.player.cool > cool);
    assert!(star.game.player.file.is_empty());
    assert!(
        star.game.player.life.bond_with("pell") >= 1,
        "teachers remember"
    );
}

// ------------------------------------------------------------------ clubs

#[test]
fn joining_needs_the_place_the_conditions_and_a_free_slot_and_rivals_shut_each_other_out() {
    // Robotics wants a tinker's hands and the notice in the Old Wing.
    let mut talker = at("smooth_talker", "old_wing", "after", 1, 1);
    let why = talker.try_(Action::Join("robotics".into())).unwrap_err();
    assert!(why.0.contains("tinker at least 1"), "{why}");
    let mut tinkerer = at("tinkerer", "old_wing", "after", 1, 1);
    let mut elsewhere = at("tinkerer", "common_room", "after", 1, 1);
    assert!(elsewhere.try_(Action::Join("robotics".into())).is_err());
    tinkerer.ok(Action::Join("robotics".into()));
    assert_eq!(tinkerer.game.player.life.rank_in("robotics"), 1);
    // The Rowing Club and Robotics are rivals: the Club would never forgive it.
    tinkerer.game.player.add_standing("rowing_eight", 20);
    tinkerer.game.player.place = "boathouse".into();
    let why = tinkerer.try_(Action::Join("rowing".into())).unwrap_err();
    assert!(why.0.contains("never forgive"), "{why}");
    // Paper is not Robotics' rival, so a second club is fine; a third is too many.
    tinkerer.game.player.add_standing("ink_society", 20);
    tinkerer.game.player.place = "old_wing".into();
    tinkerer.ok(Action::Join("paper".into()));
    tinkerer.game.player.add_standing("rowing_eight", 20);
    tinkerer.game.player.place = "library".into();
    map_set(&mut tinkerer.game.player.grades, "maths", 80);
    let why = tinkerer.try_(Action::Join("chess".into())).unwrap_err();
    assert!(why.0.contains("third club"), "{why}");
    // Once the day is over nothing can be joined.
    let mut late = Sandbox::new("tinkerer", 1);
    late.game.player.slot = 9;
    let robotics = late.game.content.club("robotics").unwrap().clone();
    assert!(late.game.join_block(&robotics).is_some());
    assert!(late.try_(Action::Join("robotics".into())).is_err());
    // Walking out is remembered for the term, and costs standing.
    let before = tinkerer.game.player.standing_with("workshop");
    let _ = before;
    let cool = tinkerer.game.player.cool;
    tinkerer.ok(Action::Quit("paper".into()));
    assert!(tinkerer.game.player.cool < cool);
    tinkerer.game.player.place = "old_wing".into();
    let why = tinkerer.try_(Action::Join("paper".into())).unwrap_err();
    assert!(why.0.contains("remember"), "{why}");
}

#[test]
fn meetings_earn_merit_and_ranks_and_a_trained_skill_doubles_the_merit() {
    // Robotics meets on Tuesday evenings in the Workshop.
    let meeting = |skill_xp: u16, seed: u64| {
        let mut sb = at("smooth_talker", "workshop", "evening", 1, seed);
        join_club(&mut sb, "robotics", 1, 0);
        map_set(&mut sb.game.player.xp, "tinker", skill_xp);
        sb.ok(Action::Meet);
        sb
    };
    let novice = meeting(0, 1);
    let adept = meeting(40, 1);
    let merit = |sb: &Sandbox| sb.game.player.life.clubs[0].merit;
    assert_eq!((merit(&novice), merit(&adept)), (1, 2));
    // Pip is there and warms to a regular.
    assert!(novice.game.player.life.bond_with("pip") >= 1);
    assert!(
        novice.game.player.life.bond_with("orrin") >= 1,
        "the advisor too"
    );
    // Ranks come with merit: four is a Builder.
    let mut sb = at("smooth_talker", "workshop", "evening", 1, 1);
    join_club(&mut sb, "robotics", 1, 3);
    let events = sb.ok(Action::Meet);
    assert!(events.iter().any(|e| e.describe().contains("promotes you")));
    assert_eq!(sb.game.player.life.rank_in("robotics"), 2);
    // Once a day, in the right place at the right time.
    sb.game.player.slot = 7;
    assert!(sb.try_(Action::Meet).is_err(), "already been today");
    let mut wrong_day = at("smooth_talker", "workshop", "evening", 0, 1);
    join_club(&mut wrong_day, "robotics", 1, 0);
    assert!(
        wrong_day.try_(Action::Meet).is_err(),
        "Mondays it does not meet"
    );
    let mut wrong_place = at("smooth_talker", "library", "evening", 1, 1);
    join_club(&mut wrong_place, "robotics", 1, 0);
    assert!(wrong_place.try_(Action::Meet).is_err());
    // A detention owed comes first.
    let mut owing = at("smooth_talker", "library", "after", 1, 1);
    join_club(&mut owing, "chess", 1, 0);
    owing.game.player.due = 1;
    assert!(owing.try_(Action::Meet).is_err());
}

#[test]
fn rank_opens_places_items_and_getaways_for_members_only() {
    let day = monday();
    let free = |sb: &Sandbox| (0..9).find(|s| sb.game.kind(day, *s) == "free").unwrap();
    // A place only the Players' leads can enter.
    let mut sb = Sandbox::new("smooth_talker", 1);
    let slot = free(&sb);
    assert!(!sb.game.place_open("green_room", day, slot));
    join_club(&mut sb, "drama", 2, 4);
    assert!(sb.game.place_open("green_room", day, slot));
    // An item the shop sells to nobody else, which a twist then asks for.
    sb.game.player.place = "green_room".into();
    sb.game.player.slot = u8::try_from(slot).unwrap();
    sb.game.player.cash = 50;
    sb.ok(Action::Buy("greasepaint".into()));
    let mut outsider = Sandbox::new("smooth_talker", 1);
    outsider.game.player.place = "green_room".into();
    outsider.game.player.slot = u8::try_from(slot).unwrap();
    outsider.game.player.cash = 50;
    assert!(outsider.try_(Action::Buy("greasepaint".into())).is_err());
    // The twist exists only for someone carrying it.
    let id = arrange(&mut sb, "pell", "salt_swap", &Setup::default());
    sb.ok(Action::AddTwist(id, "disguise".into()));
    let mut bare = Sandbox::new("smooth_talker", 1);
    let id = arrange(&mut bare, "pell", "salt_swap", &Setup::default());
    assert!(bare.try_(Action::AddTwist(id, "disguise".into())).is_err());
    // A getaway for the Rowing Club's stroke and above.
    let mut oar = Sandbox::new("smooth_talker", 1);
    let id = arrange(&mut oar, "pell", "salt_swap", &Setup::default());
    assert!(
        oar.try_(Action::SetGetaway(id, "boat_store".into()))
            .is_err()
    );
    join_club(&mut oar, "rowing", 2, 4);
    oar.ok(Action::SetGetaway(id, "boat_store".into()));
    // The secret club's den is open to its members, and to them the cellars.
    let mut candle = Sandbox::new("smooth_talker", 1);
    let night = (0..9)
        .find(|s| candle.game.kind(day, *s) == "night")
        .unwrap();
    assert!(!candle.game.place_open("candle_den", day, night));
    assert!(!candle.game.place_open("cellars", day, night));
    join_club(&mut candle, "candle", 1, 0);
    assert!(candle.game.place_open("candle_den", day, night));
    assert!(candle.game.place_open("cellars", day, night));
}

#[test]
fn the_term_ends_with_each_clubs_own_event() {
    let mut sb = Sandbox::new("smooth_talker", 1);
    join_club(&mut sb, "chess", 3, 10);
    join_club(&mut sb, "robotics", 1, 0);
    let legend = sb.game.player.legend;
    let events = sb.game.finish_term();
    let said: Vec<String> = events.iter().map(Event::describe).collect();
    assert!(
        said.iter().any(|l| l.contains("Circle's term-end match")),
        "{said:?}"
    );
    assert!(
        !said.iter().any(|l| l.contains("robot crosses")),
        "a newcomer has no part in the finale"
    );
    let chess = sb.game.content.club("chess").unwrap().event_legend;
    assert_eq!(
        sb.game.player.legend,
        legend + u32::try_from(chess * 2).unwrap()
    );
}

// ------------------------------------------------------------------ friendships and rivalries

#[test]
fn friendships_grow_by_doing_and_a_friend_opens_roles_a_stranger_will_not() {
    // Passing time together.
    let mut sb = at("smooth_talker", "paper_office", "after", 1, 1);
    sb.ok(Action::Loiter);
    assert_eq!(sb.game.player.life.bond_with("wren"), 1);
    // Rafe will not play distraction for a stranger, but will for a friend.
    let mut sb = Sandbox::new("fast_runner", 1);
    let id = arrange(&mut sb, "pell", "salt_swap", &Setup::default());
    meet(&mut sb.game, "hollis");
    let scheme = sb.game.player.scheme(id).unwrap().clone();
    let roles = |sb: &Sandbox| -> Vec<String> {
        sb.game
            .recruitable(&scheme)
            .into_iter()
            .filter(|(n, _, _)| n == "hollis")
            .map(|(_, r, _)| r)
            .collect()
    };
    assert!(roles(&sb).is_empty());
    sb.game.player.life.add_bond("hollis", 12);
    assert_eq!(roles(&sb), ["distraction"]);
    assert_eq!(sb.game.regard("hollis"), Regard::Friend);
    // Recruiting and a clean job deepen it; a bust sours it.
    sb.game.player.cash = 20;
    let before = sb.game.bond("hollis");
    sb.ok(Action::Recruit {
        scheme: id,
        npc: "hollis".into(),
        role: "distraction".into(),
    });
    assert!(sb.game.bond("hollis") > before);
    let _ = sb
        .game
        .allies_remember(&[("hollis".into(), "distraction".into())], true);
    assert!(sb.game.bond("hollis") < before + 1);
}

#[test]
fn a_betrayal_closes_a_helper_and_making_amends_reopens_the_door_on_a_cooler_footing() {
    let mut sb = Sandbox::new("smooth_talker", 1);
    sb.game.player.add_standing("the_bench", 20);
    let id = arrange(&mut sb, "pell", "salt_swap", &Setup::default());
    sb.game.player.life.last_allies = vec!["sol".into()];
    let sol_roles = |sb: &mut Sandbox| {
        meet(&mut sb.game, "sol");
        let scheme = sb.game.player.scheme(id).unwrap().clone();
        sb.game
            .recruitable(&scheme)
            .into_iter()
            .filter(|(n, _, _)| n == "sol")
            .count()
    };
    assert!(
        sol_roles(&mut sb) > 0,
        "Sol would help a friend of the Bench"
    );
    // The headmistress asks who helped; the pupil names Sol.
    sb.game
        .player
        .file
        .push(russh_bbs_door_hallpass::state::FileEntry {
            day: sb.game.player.day,
            kind: "caught".into(),
            sev: 3,
        });
    sb.game.player.caught = 1;
    meet(&mut sb.game, "vane");
    sb.ok(Action::Talk("vane_question".into()));
    let heat = sb.game.player.heat;
    sb.game.player.heat = 30;
    sb.ok(Action::Choose(1));
    assert!(sb.game.player.heat < 30 && heat <= 30, "the heat falls off");
    assert_eq!(sb.game.regard("sol"), Regard::Betrayed);
    assert!(sb.game.player.knows("betrayed_sol"));
    sb.game.player.slot = 0;
    assert_eq!(sol_roles(&mut sb), 0, "a betrayal closes them");
    // They will not talk, except about making it up.
    meet(&mut sb.game, "sol");
    let open: Vec<String> = sb.game.talks_here().iter().map(|t| t.id.clone()).collect();
    assert_eq!(open, ["sol_amends"], "{open:?}");
    sb.game.player.cash = 20;
    sb.ok(Action::Talk("sol_amends".into()));
    sb.ok(Action::Choose(0));
    assert!(sb.game.bond("sol") > -30, "{}", sb.game.bond("sol"));
    assert_eq!(
        sb.game.regard("sol"),
        Regard::Rival,
        "no longer closed, but no friend"
    );
    assert!(
        sb.game.player.knows("betrayed_sol"),
        "and it is not forgotten"
    );
    // The Strays are never given up in a script: the last ally here is a Stray, so nobody is named.
    let mut stray = Sandbox::new("smooth_talker", 1);
    stray.game.player.life.last_allies = vec!["tilly".into()];
    let before = stray.game.bond("tilly");
    let betray = russh_bbs_door_hallpass::effect::Eff::parse("betray:last_ally").unwrap();
    let events = stray.game.apply_effects(&[betray], 0);
    assert!(
        events.iter().any(|e| e.describe().contains("nobody")),
        "{events:?}"
    );
    assert_eq!(stray.game.bond("tilly"), before);
}

#[test]
fn staff_and_pupils_remember_a_rival_eye_costs_odds_and_a_friend_helping_adds() {
    let mut sb = Sandbox::new("fast_runner", 1);
    let id = arrange(&mut sb, "pell", "salt_swap", &Setup::default());
    let scheme = sb.game.player.scheme(id).unwrap().clone();
    let (day, slot) = scheme.when.unwrap();
    let place = sb.game.scheme_place(&scheme).unwrap();
    // Find a student who is at the deed's place at that moment to make a rival of; if none, move one.
    let phase = russh_bbs_door_hallpass::game::Phase::Sneak;
    let base = sb.game.chance(&scheme, phase, 0, 0).unwrap();
    let rival = sb
        .game
        .content
        .npcs
        .iter()
        .find(|n| !n.is_authority() && sb.game.npc_place(n, day, usize::from(slot)) == place)
        .map(|n| n.id.clone());
    if let Some(rival) = rival {
        sb.game.player.life.add_bond(&rival, -20);
        let worse = sb.game.chance(&scheme, phase, 0, 0).unwrap();
        assert!(
            worse.total < base.total,
            "{} vs {}",
            worse.total,
            base.total
        );
        assert!(worse.parts.iter().any(|(n, _)| n.contains("rival")));
    }
    // A friend in the crew adds to every phase.
    let mut with_friend = scheme.clone();
    with_friend.allies = vec![("wren".into(), "cover".into())];
    let alone = sb.game.chance(&with_friend, phase, 0, 0).unwrap().total;
    sb.game.player.life.add_bond("wren", 15);
    let backed = sb.game.chance(&with_friend, phase, 0, 0).unwrap();
    assert!(backed.total > alone);
    assert!(backed.parts.iter().any(|(n, _)| n.contains("friends")));
}

// ------------------------------------------------------------------ conversations

#[test]
fn there_are_twenty_conversations_and_every_choice_is_open_to_some_build() {
    let c = content();
    assert!(c.talks.len() >= 20, "{}", c.talks.len());
    for talk in &c.talks {
        let mut sb = Sandbox::new("smooth_talker", 1);
        // Late enough in the term for anything to be on offer.
        sb.game.world.term_start = sb.game.player.day - 20;
        for cond in &talk.when {
            satisfy(&mut sb.game, cond);
        }
        if let (Some(arc), Some(step)) = (&talk.arc, &talk.step) {
            satisfy(
                &mut sb.game,
                &russh_bbs_door_hallpass::cond::Cond::Arc {
                    arc: arc.clone(),
                    step: step.clone(),
                },
            );
        }
        meet(&mut sb.game, &talk.npc);
        assert!(sb.game.talk_open(talk), "{} can never be started", talk.id);
        for node in &talk.node {
            for choice in &node.choice {
                let mut build = Sandbox::new("smooth_talker", 1);
                build.game.world.term_start = build.game.player.day - 20;
                for cond in &choice.needs {
                    satisfy(&mut build.game, cond);
                }
                assert!(
                    build.game.all_hold(&choice.needs),
                    "{}: `{}` can never be chosen",
                    talk.id,
                    choice.label
                );
            }
        }
    }
}

/// The scene a build sees when it starts `talk`.
fn scene_for(
    trait_id: &str,
    talk: &str,
    prep: impl FnOnce(&mut Sandbox),
) -> (Sandbox, Vec<(String, bool)>) {
    let mut sb = Sandbox::new(trait_id, 1);
    sb.game.world.term_start = sb.game.player.day - 20;
    let spec = sb.game.content.talk(talk).unwrap().clone();
    meet(&mut sb.game, &spec.npc);
    prep(&mut sb);
    sb.ok(Action::Talk(talk.into()));
    let view = sb.game.scene_view().unwrap();
    let choices = view
        .choices
        .into_iter()
        .map(|c| (c.label, c.open))
        .collect();
    (sb, choices)
}

#[test]
fn the_same_scene_offers_different_builds_different_options_and_they_end_differently() {
    let prep = |sb: &mut Sandbox| {
        sb.game.player.life.arcs.clear();
        satisfy(
            &mut sb.game,
            &russh_bbs_door_hallpass::cond::Cond::parse("wary:marsh>=2").unwrap(),
        );
    };
    // Mr. Marsh has noticed three pupils.
    let (mut talker, a) = scene_for("smooth_talker", "marsh_clipboard", prep);
    let (mut pet, b) = scene_for("teachers_pet", "marsh_clipboard", |sb| {
        prep(sb);
        for (_, g) in &mut sb.game.player.grades {
            *g = 80;
        }
    });
    let (runner, c) = scene_for("fast_runner", "marsh_clipboard", prep);
    let open = |v: &[(String, bool)]| -> Vec<String> {
        v.iter()
            .filter(|(_, o)| *o)
            .map(|(l, _)| l.clone())
            .collect()
    };
    assert!(open(&a).contains(&"Apologise, smoothly".to_owned()));
    assert!(!open(&a).contains(&"Remind him of your fine grades".to_owned()));
    assert!(open(&b).contains(&"Remind him of your fine grades".to_owned()));
    assert!(!open(&b).contains(&"Apologise, smoothly".to_owned()));
    assert_eq!(
        open(&c),
        ["Stare at the floor"],
        "a plain runner has only the floor"
    );
    assert_ne!(open(&a), open(&b));
    // A shut choice says what it wants, and refuses to be picked.
    let shut = runner.game.scene_view().unwrap();
    assert!(
        shut.choices[0].wants.contains("charm"),
        "{:?}",
        shut.choices[0]
    );
    // They end differently: the talker's apology and the pet's marks both cool Mr. Marsh, the runner's silence does not.
    let wary = |sb: &Sandbox| sb.game.player.wary_of("marsh");
    let (before_a, before_b) = (wary(&talker), wary(&pet));
    talker.ok(Action::Choose(0));
    pet.ok(Action::Choose(1));
    assert!(wary(&talker) < before_a && wary(&pet) < before_b);
    let mut runner = runner;
    let before_c = wary(&runner);
    assert!(runner.try_(Action::Choose(0)).is_err());
    runner.ok(Action::Choose(2));
    assert_eq!(wary(&runner), before_c);
    assert!(
        pet.game.player.life.bond_with("marsh") > talker.game.player.life.bond_with("marsh"),
        "and only the pet earns his regard"
    );
    // A teacher's pet cannot try the shakedown at the heart of the ledger story, a troublemaker can.
    let blackmail = |trait_id: &str| {
        let (_, v) = scene_for(trait_id, "ledger_bramley", |sb| {
            satisfy(
                &mut sb.game,
                &russh_bbs_door_hallpass::cond::Cond::parse("arc:ledger@confront").unwrap(),
            );
        });
        v[0].1
    };
    assert!(!blackmail("teachers_pet") && blackmail("fast_runner"));
}

#[test]
fn choices_change_standing_close_some_doors_and_open_others_for_the_rest_of_the_term() {
    // Turning Rafe down costs the friendship and the offer never comes again.
    let (mut sb, _) = scene_for("fast_runner", "hollis_race", |_| {});
    let events = sb.ok(Action::Choose(2));
    assert!(events.iter().any(|e| e.describe().contains("Chicken")));
    assert_eq!(sb.game.bond("hollis"), -8);
    meet(&mut sb.game, "hollis");
    sb.game.player.slot = 5;
    let race = sb.game.content.talk("hollis_race").unwrap().clone();
    assert!(
        !sb.game.talk_ready(&race),
        "once said no, it is not offered twice"
    );
    // Sour it further and a different conversation opens: the apology.
    sb.game.player.life.add_bond("hollis", -6);
    assert_eq!(sb.game.regard("hollis"), Regard::Rival);
    sb.game.player.cash = 10;
    meet(&mut sb.game, "hollis");
    assert!(sb.game.talks_here().iter().any(|t| t.id == "hollis_amends"));
    sb.ok(Action::Talk("hollis_amends".into()));
    sb.ok(Action::Choose(0));
    assert_eq!(sb.game.regard("hollis"), Regard::Neutral);
    meet(&mut sb.game, "hollis");
    sb.game.player.slot = 6;
    assert!(
        !sb.game.talks_here().iter().any(|t| t.id == "hollis_amends"),
        "the door that opened for the quarrel closes with it"
    );
    // A choice moves standing and sets a flag that other content reads: the head prefect's offer
    // shuts a troublemaker out and teaches a plain pupil the rounds.
    let offer = |trait_id: &str, troublemaker: bool| {
        let (mut sb, shown) = scene_for(trait_id, "wick_rounds", |sb| {
            sb.game.player.add_standing("lamplighters", 10);
            if troublemaker {
                sb.game.player.traits.push("known_troublemaker".into());
            }
        });
        let first_open = shown[0].1;
        if first_open {
            sb.ok(Action::Choose(0));
        }
        (
            first_open,
            sb.game.player.knows("prefect_rounds"),
            sb.game.player.standing_with("lamplighters"),
        )
    };
    let (open, knows, standing) = offer("smooth_talker", false);
    assert!(open && knows && standing > 10);
    let (open, ..) = offer("smooth_talker", true);
    assert!(!open, "a known troublemaker is not offered the rounds");
    // Standing up to Crouch: the Strays remember, Crouch does too.
    let (mut sb, shown) = scene_for("fast_runner", "crouch_stand", |sb| {
        sb.game.player.nerve = 6;
    });
    assert!(shown[0].1);
    let (strays, crouch) = (
        sb.game.player.standing_with("strays"),
        sb.game.bond("crouch"),
    );
    sb.ok(Action::Choose(0));
    assert!(sb.game.player.standing_with("strays") > strays && sb.game.bond("crouch") < crouch);
    assert!(sb.game.player.knows("stood_up_to_crouch"));
}

#[test]
fn a_conversation_takes_the_period_when_it_ends_and_nothing_else_can_be_done_in_the_middle() {
    let (mut sb, _) = scene_for("smooth_talker", "pell_tutoring", |sb| {
        map_set(&mut sb.game.player.grades, "maths", 50);
    });
    let slot = sb.game.player.slot;
    assert!(
        sb.try_(Action::Rest).is_err(),
        "no wandering off mid-sentence"
    );
    assert!(sb.try_(Action::Go("quad".into())).is_err());
    // Leaving before saying anything costs nothing.
    let mut quick = scene_for("smooth_talker", "pell_tutoring", |sb| {
        map_set(&mut sb.game.player.grades, "maths", 50);
    })
    .0;
    quick.ok(Action::Abandon);
    assert_eq!(quick.game.player.slot, slot);
    assert!(quick.game.player.life.scene.is_none());
    // Taking an option ends the talk and the period.
    let grade = i32::from(map_get(&sb.game.player.grades, "maths").copied().unwrap());
    sb.ok(Action::Choose(0));
    assert_eq!(sb.game.player.slot, slot + 1);
    assert!(i32::from(map_get(&sb.game.player.grades, "maths").copied().unwrap()) > grade);
    // A scene saved mid-way comes back mid-way; a night's sleep closes it.
    let (mid, _) = scene_for("smooth_talker", "light_fenn", |sb| {
        satisfy(
            &mut sb.game,
            &russh_bbs_door_hallpass::cond::Cond::parse("arc:east_light@rumour").unwrap(),
        );
    });
    let bytes = encode_player(&mid.game.player);
    let back = decode_player(&bytes).unwrap();
    assert!(back.life.scene.is_some());
    let mut late = mid;
    late.game.sync_day(late.game.player.day + 1);
    assert!(late.game.player.life.scene.is_none());
    let _ = DoorRng::from_seed(0);
}

#[test]
fn a_word_with_the_classs_own_teacher_in_their_room_counts_as_the_lesson_and_any_other_does_not() {
    // Tutoring from Mr. Pell during his own maths lesson is not a skipped class.
    let (mut sb, _) = scene_for("smooth_talker", "pell_tutoring", |sb| {
        let (day, slot) = lesson_slot("maths");
        sb.game.sync_day(day);
        sb.game.player.slot = u8::try_from(slot).unwrap();
        sb.game.player.place = "room_maths".into();
        map_set(&mut sb.game.player.grades, "maths", 50);
    });
    let events = sb.ok(Action::Choose(0));
    assert!(
        !events.iter().any(|e| e.describe().contains("empty seat") || e.describe().contains("Nobody noticed")),
        "{events:?}"
    );
    assert!(i32::from(map_get(&sb.game.player.grades, "maths").copied().unwrap()) >= 56);
    // A word with a prefect in the corridor in the middle of a lesson is a skipped class.
    let mut away = in_class("smooth_talker", "maths", 4);
    away.game.player.add_standing("lamplighters", 10);
    meet(&mut away.game, "wick");
    away.game.player.slot = u8::try_from(lesson_slot("maths").1).unwrap();
    let wick = away.game.content.npc("wick").unwrap().clone();
    let day = away.game.player.day;
    away.game.player.place = away
        .game
        .npc_place(&wick, day, usize::from(away.game.player.slot))
        .to_owned();
    away.ok(Action::Talk("wick_rounds".into()));
    let events = away.ok(Action::Choose(2));
    assert!(
        events.iter().any(|e| e.describe().contains("empty seat") || e.describe().contains("Nobody noticed")),
        "{events:?}"
    );
}

#[test]
fn a_conversation_is_a_tree_a_choice_leads_to_the_next_node_and_the_period_is_spent_at_the_end() {
    // The head prefect's offer has a second beat: a name.
    let (mut sb, shown) = scene_for("smooth_talker", "wick_rounds", |sb| {
        sb.game.player.add_standing("lamplighters", 10);
    });
    assert!(shown[0].1);
    let slot = sb.game.player.slot;
    sb.ok(Action::Choose(0));
    let second = sb.game.scene_view().unwrap();
    assert!(
        second.text.contains("Bench keeps leaving notes"),
        "{}",
        second.text
    );
    assert_eq!(
        sb.game.player.slot, slot,
        "the period is spent when the talk ends, not before"
    );
    // A scene saved at the second node comes back at the second node.
    let back = decode_player(&encode_player(&sb.game.player)).unwrap();
    assert_eq!(back.life.scene.as_ref().unwrap().node, "notes");
    // Walking away mid-way costs the period; naming Sol costs a friend.
    let mut walked = decode_scene_game(&sb);
    walked.ok(Action::Abandon);
    assert_eq!(walked.game.player.slot, slot + 1);
    assert_eq!(walked.game.bond("sol"), 0);
    sb.ok(Action::Choose(0));
    assert_eq!(sb.game.player.slot, slot + 1);
    assert!(sb.game.player.knows("betrayed_sol"));
    assert_eq!(sb.game.regard("sol"), Regard::Betrayed);
    assert!(
        sb.game.bond("wick") >= 9,
        "and the prefect is pleased: {}",
        sb.game.bond("wick")
    );
    // A second beat can have a choice that is open to one build and shut to another.
    let stare = |nerve: u8| {
        let (mut sb, _) = scene_for("fast_runner", "crouch_stand", |sb| {
            sb.game.player.nerve = 6;
            sb.game.player.nerve_max = 6;
        });
        sb.ok(Action::Choose(0));
        sb.game.player.nerve = nerve;
        sb.game.scene_view().unwrap().choices[0].open
    };
    assert!(stare(2) && !stare(1));
    // The stakes, the doubt and the sign each have a second node too.
    for (talk, node) in [
        ("hollis_race", "stakes"),
        ("vane_question", "doubt"),
        ("sol_candle", "sign"),
    ] {
        let spec = content().talk(talk).unwrap().clone();
        assert!(spec.node.iter().any(|n| n.id == node), "{talk}");
    }
}

/// A second copy of the game in the same scene, as a reconnect would give.
fn decode_scene_game(sb: &Sandbox) -> Sandbox {
    let player = decode_player(&encode_player(&sb.game.player)).unwrap();
    let (game, _) = Game::open(&content(), "ann", Some(player), None, sb.game.player.day);
    Sandbox {
        game,
        rng: DoorRng::from_seed(2),
    }
}
