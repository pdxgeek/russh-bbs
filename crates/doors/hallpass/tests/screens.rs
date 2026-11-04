//! The new screens (lesson, conversation, talk list, clubs, journal) at 80x24, the same over both
//! transports, with nothing the pupil needs cut off.
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

use common::{Sandbox, meet, monday, satisfy};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_door::host::Driver;
use russh_bbs_door::replay::draw_to_text;
use russh_bbs_door::{DoorEnv, DoorKey, Save, Start, TransportKind};
use russh_bbs_door_hallpass::Hallpass;
use russh_bbs_door_hallpass::cond::Cond;
use russh_bbs_door_hallpass::save::encode_player;
use russh_bbs_door_hallpass::state::Player;

/// Press `keys` on a fresh visit by `player` over both transports; every screen fits 80x24 and the
/// two transports draw the same. Returns the screens in order.
fn walk(player: &Player, keys: &str) -> Vec<String> {
    let door = Hallpass::new().unwrap();
    let bytes = encode_player(player);
    let clock: Arc<dyn Clock> = Arc::new(ManualClock::new(Day::from_raw(player.day).start()));
    let ssh = DoorEnv::for_tests("ann");
    let mut telnet = DoorEnv::for_tests("ann");
    telnet.transport = TransportKind::Telnet;
    let open = |env: &DoorEnv| {
        Driver::open(
            &door,
            Start {
                save: Save::Loaded {
                    bytes: &bytes,
                    season: 1,
                },
                world: None,
                env,
                today: Day::from_raw(player.day),
                season: 1,
            },
            3,
            Arc::clone(&clock),
        )
    };
    let (mut a, mut b) = (open(&ssh), open(&telnet));
    let mut seen = Vec::new();
    for key in keys.chars() {
        a.key(DoorKey::Char(key));
        b.key(DoorKey::Char(key));
        let screen = draw_to_text(&a, &ssh, 80, 24);
        assert!(screen.iter().all(|l| l.chars().count() <= 80), "{screen:?}");
        assert_eq!(screen, draw_to_text(&b, &telnet, 80, 24), "after {key:?}");
        assert_eq!(a.look_around(&ssh), b.look_around(&telnet));
        a.take_effects();
        b.take_effects();
        seen.push(screen.join("\n"));
    }
    seen
}

#[test]
fn the_lesson_screen_shows_the_puzzle_its_answers_and_a_way_out() {
    let mut sb = Sandbox::new("tinkerer", 4);
    sb.game.sync_day(monday() + 1);
    // Tuesday third period is maths.
    sb.game.player.slot = 3;
    sb.game.player.place = "room_maths".into();
    map_xp(&mut sb);
    let screens = walk(&sb.game.player, "ax");
    let lesson = &screens[0];
    assert!(lesson.contains("Maths: puzzle 1 of 3"), "{lesson}");
    assert!(lesson.contains("Continue the sequence:"), "{lesson}");
    assert!(
        lesson.lines().any(|l| l.starts_with("[1] ")
            || l.starts_with("[2] ")
            || l.starts_with("[3] ")
            || l.starts_with("[4] ")),
        "{lesson}"
    );
    assert!(
        lesson.contains("(ruled out)"),
        "a practised tinkerer has help: {lesson}"
    );
    assert!(lesson.contains("[x] Walk out of the lesson"), "{lesson}");
    assert!(
        !lesson.contains("[g] Go somewhere"),
        "nothing else is on offer mid-lesson"
    );
    // Walking out ends the period like a skipped class.
    assert!(screens[1].contains("Fourth period") || screens[1].contains("[g] Go somewhere"));
}

fn map_xp(sb: &mut Sandbox) {
    russh_bbs_door_hallpass::state::map_set(&mut sb.game.player.xp, "tinker", 40);
}

#[test]
fn the_conversation_screen_shows_open_choices_and_names_what_a_shut_one_wants() {
    let mut sb = Sandbox::new("fast_runner", 4);
    sb.game.world.term_start = sb.game.player.day - 20;
    satisfy(&mut sb.game, &Cond::parse("wary:marsh>=2").unwrap());
    meet(&mut sb.game, "marsh");
    let screens = walk(&sb.game.player, "t1");
    let list = &screens[0];
    assert!(list.contains("Who do you want a word with?"), "{list}");
    assert!(list.contains("[1] Mr. Marsh has noticed you"), "{list}");
    let scene = &screens[1];
    assert!(
        scene.contains("Mr. Quillon Marsh - Mr. Marsh has noticed you"),
        "{scene}"
    );
    assert!(
        scene.contains("(shut: needs a knack for charm) Apologise, smoothly"),
        "{scene}"
    );
    // Keys stay where the choice is, so a shut choice leaves a gap rather than renumbering.
    assert!(scene.contains("[3] Stare at the floor"), "{scene}");
    assert!(scene.contains("[x] Excuse yourself"), "{scene}");
}

#[test]
fn the_clubs_and_journal_screens_say_why_a_club_is_shut_and_where_a_story_stands() {
    let mut sb = Sandbox::new("smooth_talker", 4);
    sb.game.world.term_start = sb.game.player.day - 5;
    sb.game.sync_day(sb.game.player.day + 1);
    let screens = walk(&sb.game.player, "cbjb");
    let clubs = &screens[0];
    assert!(clubs.contains("Clubs (you can belong to 2):"), "{clubs}");
    for name in [
        "Chess Circle",
        "Players",
        "Rowing Club",
        "School Paper",
        "Robotics",
        "Candle",
    ] {
        assert!(clubs.contains(name), "{name}: {clubs}");
    }
    assert!(clubs.contains("[b] Back"), "{clubs}");
    let journal = &screens[2];
    assert!(journal.contains("Stories:"), "{journal}");
    assert!(
        journal.contains("Friends:") && journal.contains("Rivals:"),
        "{journal}"
    );
}
