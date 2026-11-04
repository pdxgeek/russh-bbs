//! Saves: round trip, damage, and the rule that reopening a save is not a re-roll.

use super::support::{content, game_of, place};
use crate::combat::FightAct;
use crate::game::{Action, Game, Place};
use crate::save::{Whereabouts, summarize};

fn busy_game() -> Game {
    let c = content();
    let mut g = game_of("cutthroat", ["quick", "stout"], 77);
    g.hero.nodes.push("cu-quick-hands".into());
    g.hero.scars.push("limp".into());
    g.hero.inflict(&c, "gash");
    place(&mut g, "bone-pit");
    g.today.spent.push((1, 2, 0));
    g.today.cleared.push((1, 3));
    g.today.traps.push((1, 4, 1));
    g.explored[0] = 0b1011;
    let mut ev = Vec::new();
    g.start_fight(
        &c,
        &["slag-rat".to_owned(), "cinder-imp".to_owned()],
        false,
        &mut ev,
    );
    g.apply(&c, Action::Fight(FightAct::Attack(0))).ok();
    g
}

#[test]
fn a_save_round_trips_exactly_including_the_fight_and_the_rng() {
    let g = busy_game();
    let bytes = g.encode();
    let back = Game::decode(&bytes).expect("decodes");
    assert_eq!(back.encode(), bytes);
    assert_eq!(back.hero, g.hero);
    assert_eq!(back.fight, g.fight);
    assert_eq!(back.place, g.place);
    assert_eq!(back.today, g.today);
    assert_eq!(back.rng.state(), g.rng.state());
    assert!(
        bytes.len() < 4_000,
        "a mid-fight save is {} bytes",
        bytes.len()
    );
}

#[test]
fn reopening_a_save_continues_the_same_dice_it_is_not_a_re_roll() {
    let c = content();
    let g = busy_game();
    let mut a = Game::decode(&g.encode()).unwrap();
    let mut b = Game::decode(&g.encode()).unwrap();
    for game in [&mut a, &mut b] {
        for _ in 0..4 {
            let _ = game.apply(&c, Action::Fight(FightAct::Attack(0)));
        }
    }
    assert_eq!(a.encode(), b.encode());
}

#[test]
fn damaged_saves_are_refused_not_guessed_at() {
    let bytes = busy_game().encode();
    assert!(Game::decode(&[]).is_none());
    assert!(
        Game::decode(&bytes[..bytes.len() - 1]).is_none(),
        "truncated"
    );
    let mut extra = bytes.clone();
    extra.push(0);
    assert!(Game::decode(&extra).is_none(), "trailing bytes");
    for cut in [1, 5, 40, bytes.len() / 2] {
        assert!(Game::decode(&bytes[..cut]).is_none(), "cut at {cut}");
    }
    let mut bad = bytes;
    // Flip the place tag somewhere in the middle: whatever it lands on must not panic.
    for i in 0..bad.len() {
        let saved = bad[i];
        bad[i] = 0xFF;
        let _ = Game::decode(&bad);
        bad[i] = saved;
    }
}

#[test]
fn the_summary_reads_a_save_for_tests_and_the_harness() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 3);
    let s = summarize(&g.encode(), &c).unwrap();
    assert_eq!(
        (s.level, s.whereabouts, s.deaths),
        (1, Whereabouts::Town, 0)
    );
    assert_eq!(s.calling, "warden");
    g.apply(&c, Action::Descend(1)).unwrap();
    let s = summarize(&g.encode(), &c).unwrap();
    assert_eq!(s.whereabouts, Whereabouts::Below(1));
    assert_eq!(s.turns, 24);
    assert!(summarize(&[1, 2, 3], &c).is_none());
    let _ = Place::Town;
}

#[test]
fn a_save_stays_far_under_the_framework_cap() {
    let c = content();
    let mut g = busy_game();
    g.hero.nodes = c.nodes.node.iter().take(15).map(|n| n.id.clone()).collect();
    g.hero.traits = c
        .traits
        .traits
        .iter()
        .take(2)
        .map(|n| n.id.clone())
        .collect();
    assert!(g.encode().len() < 64 * 1024 / 8);
}

#[test]
fn a_save_that_puts_the_hero_where_there_is_no_room_is_refused() {
    // Whatever the bytes, a hero is never placed on level 0, below the bottom, or in room 40,
    // so nothing downstream can index or shift out of range.
    let mut g = busy_game();
    for (depth, room) in [(0u8, 0u8), (26, 0), (255, 0), (3, 32), (3, 200)] {
        g.place = Place::Delve { depth, room };
        assert!(
            Game::decode(&g.encode()).is_none(),
            "depth {depth} room {room}"
        );
        g.place = Place::Dead {
            depth,
            room,
            since: 1,
        };
        assert!(Game::decode(&g.encode()).is_none());
    }
    g.place = Place::Delve {
        depth: 25,
        room: 31,
    };
    assert!(Game::decode(&g.encode()).is_some());
}
