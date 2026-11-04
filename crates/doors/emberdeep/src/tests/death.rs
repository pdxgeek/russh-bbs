//! Death, the three-day recovery window, and rescue.

use super::support::{TODAY, content, game_of, place, run, said};
use crate::game::{Action, Game, Place};
use crate::world::Change;

fn dead_at_depth(seed: u64) -> Game {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], seed);
    place(&mut g, "sooty-gallery");
    // Make sure the death save fails, whatever the roll.
    g.hero.attrs[3] = 1;
    g.hero.hp = 0;
    let mut ev = Vec::new();
    for _ in 0..200 {
        if g.hero_down(&c, &mut ev) {
            return g;
        }
        g.hero.hp = 0;
    }
    panic!("never died");
}

#[test]
fn dying_below_leaves_a_body_and_tells_the_world() {
    let g = dead_at_depth(1);
    assert!(matches!(g.place, Place::Dead { since, .. } if since == TODAY));
    assert!(g.hero.deaths >= 1);
    let fell = g
        .world
        .fallen
        .iter()
        .any(|f| f.name == "Tester" && f.day == TODAY);
    assert!(fell);
}

#[test]
fn a_dead_hero_waits_three_days_then_wakes_in_the_temple_without_their_gear() {
    let c = content();
    let mut g = dead_at_depth(2);
    g.hero.weapon = "stiletto".into();
    g.hero.armour = "pilgrim-robe".into();
    g.hero.coin = 100;
    g.hero.salvage = 30;
    let ev = g.new_day(&c, TODAY + 1);
    assert!(matches!(g.place, Place::Dead { .. }), "{}", said(&ev));
    assert!(said(&ev).contains("2 day"), "{}", said(&ev));
    let _ = g.new_day(&c, TODAY + 2);
    assert!(matches!(g.place, Place::Dead { .. }));
    let ev = g.new_day(&c, TODAY + 3);
    assert_eq!(g.place, Place::Town, "{}", said(&ev));
    assert_eq!(
        g.hero.weapon, "iron-halberd",
        "gear is back to the calling's kit"
    );
    assert_eq!(g.hero.coin, 75);
    assert_eq!(g.hero.salvage, 0);
    assert!(g.hero.wounds.iter().any(|(w, _)| w == "weakened"));
}

#[test]
fn giving_up_is_the_same_as_waiting_it_out() {
    let c = content();
    let mut g = dead_at_depth(3);
    g.hero.weapon = "stiletto".into();
    run(&mut g, Action::GiveUp);
    assert_eq!(g.place, Place::Town);
    assert_eq!(g.hero.weapon, "iron-halberd");
    assert!(
        g.apply(&c, Action::Descend(1)).is_ok(),
        "the weakened hero is back in play"
    );
}

#[test]
fn a_dead_hero_can_do_nothing_else() {
    let c = content();
    let mut g = dead_at_depth(4);
    assert!(g.apply(&c, Action::Descend(1)).is_err());
    assert!(g.apply(&c, Action::Climb).is_err());
}

#[test]
fn another_hero_can_carry_a_fallen_one_home_in_time() {
    let c = content();
    let mut dead = dead_at_depth(5);
    dead.hero.weapon = "stiletto".into();
    dead.hero.coin = 100;
    let fallen = dead.world.fallen[0].clone();
    // The rescuer stands in the same room, same season, sees the same world.
    let mut rescuer = game_of("cutthroat", ["quick", "stout"], 6);
    rescuer.hero.name = "Helper".into();
    rescuer.world = dead.world.clone();
    rescuer.place = Place::Delve {
        depth: fallen.depth,
        room: fallen.room,
    };
    let labels = super::support::labels(&rescuer);
    assert!(
        labels.iter().any(|l| l == "Carry Tester home"),
        "{labels:?}"
    );
    let coin = rescuer.hero.coin;
    let turns = rescuer.hero.turns;
    let idx = labels
        .iter()
        .position(|l| l == "Carry Tester home")
        .unwrap();
    run(&mut rescuer, Action::Option(idx));
    assert_eq!(rescuer.hero.turns, turns - 3);
    assert!(rescuer.hero.coin > coin, "the town pays");
    let changes = rescuer.take_changes();
    assert!(
        changes
            .iter()
            .any(|ch| matches!(ch, Change::Rescue(n, by) if n == "Tester" && by == "Helper"))
    );
    // It is not offered twice.
    assert!(
        !super::support::labels(&rescuer)
            .iter()
            .any(|l| l.starts_with("Carry"))
    );

    // Next morning the dead hero opens the door: carried home, gear and pack intact.
    dead.world = rescuer.world.clone();
    let ev = dead.new_day(&c, TODAY + 1);
    assert_eq!(dead.place, Place::Town, "{}", said(&ev));
    assert_eq!(dead.hero.weapon, "stiletto");
    assert_eq!(dead.hero.coin, 100);
    assert!(dead.hero.wounds.iter().any(|(w, _)| w == "dragged-home"));
    assert!(
        dead.hero.hp < dead.derived(&c).max_hp,
        "they come back hurt"
    );
}

#[test]
fn a_rescue_is_not_lost_if_the_hero_comes_back_late() {
    let c = content();
    let mut dead = dead_at_depth(7);
    dead.world
        .apply(&Change::Rescue("Tester".into(), "Helper".into()));
    // Ten days later, and the world has ticked every day in between.
    for day in 1..=10 {
        dead.world.tick(TODAY + day);
    }
    let _ = dead.new_day(&c, TODAY + 10);
    assert_eq!(dead.place, Place::Town);
    assert_eq!(dead.hero.weapon, "iron-halberd");
    assert!(
        dead.hero.wounds.iter().any(|(w, _)| w == "dragged-home"),
        "rescued, not the temple"
    );
}

#[test]
fn nobody_can_rescue_themselves_or_from_the_wrong_room() {
    let dead = dead_at_depth(8);
    let fallen = dead.world.fallen[0].clone();
    let mut other = game_of("warden", ["quick", "stout"], 9);
    other.world = dead.world.clone();
    other.place = Place::Delve {
        depth: fallen.depth,
        room: fallen.room.wrapping_add(1) % 10,
    };
    assert!(
        !super::support::labels(&other)
            .iter()
            .any(|l| l.starts_with("Carry"))
    );
    let mut me = dead.clone();
    me.place = Place::Delve {
        depth: fallen.depth,
        room: fallen.room,
    };
    assert!(
        !super::support::labels(&me)
            .iter()
            .any(|l| l.starts_with("Carry")),
        "not yourself"
    );
}

#[test]
fn the_death_save_depends_on_the_build() {
    let c = content();
    let survive = |calling: &str, traits: [&str; 2], spirit: u8| -> usize {
        (0..600u64)
            .filter(|seed| {
                let mut g = game_of(calling, traits, *seed);
                place(&mut g, "sooty-gallery");
                g.hero.attrs[3] = spirit;
                g.hero.hp = 0;
                let mut ev = Vec::new();
                !g.hero_down(&c, &mut ev)
            })
            .count()
    };
    let frail = survive("cutthroat", ["quick", "stout"], 6);
    let sturdy = survive("ember-priest", ["stubborn", "lucky"], 14);
    assert!(
        sturdy > frail + 100,
        "stubborn priest {sturdy}/600 vs frail cutthroat {frail}/600"
    );
    // A survivor lives at the brink: one hit point and the wound that says so.
    let mut g = game_of("ember-priest", ["stubborn", "lucky"], 1);
    place(&mut g, "sooty-gallery");
    g.hero.attrs[3] = 16;
    g.hero.hp = 0;
    let mut ev = Vec::new();
    for _ in 0..20 {
        if !g.hero_down(&c, &mut ev) {
            assert_eq!(g.hero.hp, 1);
            assert!(g.hero.wounds.iter().any(|(w, _)| w == "brink"));
            return;
        }
        g.hero.hp = 0;
        g.place = Place::Delve { depth: 1, room: 0 };
    }
    panic!("never survived");
}
