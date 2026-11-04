//! The torch-turn economy: turns, the cost of climbing back, the dark, and shortcuts.

use super::support::{TODAY, content, game_of, labels, place_from, run, said};
use crate::content::ShortcutKind;
use crate::game::{Action, Place};
use crate::rules::TURNS_PER_DAY;
use crate::world::Change;

fn delve(g: &mut crate::game::Game, depth: u8) {
    let c = content();
    let level = crate::dungeon::generate(&c, g.season, g.epoch, depth);
    g.place = Place::Delve {
        depth,
        room: level.entry,
    };
    g.prev_room = level.entry;
}

#[test]
fn a_day_is_twenty_five_torch_turns_and_a_new_day_refills_them() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 1);
    assert_eq!(g.hero.turns, TURNS_PER_DAY);
    run(&mut g, Action::Descend(1));
    assert_eq!(g.hero.turns, 24);
    assert!(
        g.new_day(&c, TODAY).is_empty(),
        "the same day is not a new day"
    );
    assert_eq!(g.hero.turns, 24);
    run(&mut g, Action::Climb);
    let _ = g.new_day(&c, TODAY + 1);
    assert_eq!(g.hero.turns, 25);
}

#[test]
fn climbing_back_costs_a_turn_per_level() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 2);
    for depth in [1u8, 3, 6, 12] {
        delve(&mut g, depth);
        g.hero.salvage = 0;
        g.hero.fatigue = 0;
        let ease = g.derived(&c).climb_ease;
        let want = (i32::from(depth) - ease).max(1);
        assert_eq!(
            i32::try_from(g.ascent_cost(&c)).unwrap(),
            want,
            "depth {depth}"
        );
    }
}

#[test]
fn the_build_changes_the_cost_of_the_climb() {
    let c = content();
    let cost = |calling: &str, traits: [&str; 2]| {
        let mut g = game_of(calling, traits, 3);
        delve(&mut g, 8);
        g.hero.salvage = 0;
        g.ascent_cost(&c)
    };
    let plain = cost("cutthroat", ["quick", "stout"]);
    assert_eq!(plain, 8);
    assert_eq!(cost("cutthroat", ["night-eyed", "stout"]), 7, "night-eyed");
    // A wanderer who has taken the climbing nodes.
    let mut g = game_of("wanderer", ["night-eyed", "stout"], 3);
    g.hero.nodes = vec![
        "wn-forager".into(),
        "wn-ropework".into(),
        "wn-bridgewright".into(),
        "wn-pathfinder".into(),
        "wn-deepwalker".into(),
    ];
    delve(&mut g, 8);
    g.hero.salvage = 0;
    assert!(
        g.ascent_cost(&c) <= 8 - 1 - 1 - 2 - 2,
        "wanderer climbs back {}",
        g.ascent_cost(&c)
    );
    // A limp makes it dearer.
    let mut hurt = game_of("cutthroat", ["quick", "stout"], 3);
    delve(&mut hurt, 8);
    hurt.hero.scars.push("limp".into());
    assert_eq!(hurt.ascent_cost(&c), 9);
}

#[test]
fn too_much_salvage_and_too_much_fatigue_slow_the_climb() {
    let c = content();
    let mut g = game_of("cutthroat", ["quick", "stout"], 4);
    delve(&mut g, 4);
    g.hero.salvage = 0;
    let base = g.ascent_cost(&c);
    let carry = g.derived(&c).carry;
    g.hero.salvage = u32::try_from(carry).unwrap() + 25;
    assert_eq!(g.ascent_cost(&c), base + 3);
    g.hero.salvage = 0;
    g.hero.fatigue = 80;
    assert_eq!(g.ascent_cost(&c), base + 1);
    // Strong carriers feel it later.
    let mut w = game_of("warden", ["pack-rat", "stout"], 4);
    delve(&mut w, 4);
    w.hero.salvage = u32::try_from(carry).unwrap() + 5;
    assert_eq!(w.ascent_cost(&c), 4);
}

#[test]
fn shortcuts_cut_the_climb_and_the_descent_for_everyone() {
    let c = content();
    let mut opener = game_of("runesmith", ["quick", "stout"], 5);
    let (depth, _) = place_from(&mut opener, "broken-lift", 6);
    let opts = labels(&opener);
    let at = opts
        .iter()
        .position(|l| l.contains("rune-work"))
        .expect("runesmith sees rune-work");
    run(&mut opener, Action::Option(at));
    assert_eq!(opener.world.shortcut_at(depth), Some(ShortcutKind::Lift));
    let changes = opener.take_changes();
    assert!(matches!(changes[0], Change::Shortcut(d, ShortcutKind::Lift, _) if d == depth));

    // A different hero, later, on the same world.
    let mut other = game_of("duelist", ["quick", "stout"], 6);
    for ch in &changes {
        other.world.apply(ch);
    }
    delve(&mut other, depth + 3);
    other.hero.salvage = 0;
    other.hero.fatigue = 0;
    let ease = other.derived(&c).climb_ease;
    let expect = ((3 + 1) - i64::from(ease)).max(1);
    assert_eq!(i64::from(other.ascent_cost(&c)), expect);
    assert!(
        other.ascent_cost(&c) < u32::from(depth + 3),
        "cheaper than {} levels",
        depth + 3
    );
    // And on the way down: straight from town to that level for the flat cost.
    other.place = Place::Town;
    other.hero.turns = 25;
    run(&mut other, Action::Descend(depth));
    assert_eq!(other.hero.turns, 24, "a lift costs one turn");
    assert!(matches!(other.place, Place::Delve { depth: d, .. } if d == depth));
}

#[test]
fn a_lift_beats_a_bridge_beats_a_rope_and_the_better_one_wins() {
    let mut w = crate::world::World::new(1);
    w.apply(&Change::Shortcut(4, ShortcutKind::Rope, "A".into()));
    assert_eq!(w.shortcut_at(4), Some(ShortcutKind::Rope));
    w.apply(&Change::Shortcut(4, ShortcutKind::Lift, "B".into()));
    assert_eq!(w.shortcut_at(4), Some(ShortcutKind::Lift));
    w.apply(&Change::Shortcut(4, ShortcutKind::Bridge, "C".into()));
    assert_eq!(
        w.shortcut_at(4),
        Some(ShortcutKind::Lift),
        "never replaced by a worse one"
    );
    assert_eq!(
        [ShortcutKind::Lift, ShortcutKind::Bridge, ShortcutKind::Rope].map(ShortcutKind::flat_cost),
        [1, 2, 3]
    );
}

#[test]
fn there_is_no_express_to_a_level_without_a_shortcut() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 6);
    assert!(g.apply(&c, Action::Descend(7)).is_err());
    assert_eq!(g.hero.turns, TURNS_PER_DAY, "a refusal costs nothing");
}

#[test]
fn when_the_torch_runs_out_the_dark_gets_you() {
    let c = content();
    let mut hurt_total = 0;
    let mut deaths = 0;
    for seed in 0..100 {
        let mut g = game_of("warden", ["quick", "stout"], seed);
        delve(&mut g, 9);
        g.hero.salvage = 0;
        g.hero.turns = 2;
        let hp = g.hero.hp;
        let cost = g.ascent_cost(&c);
        assert!(cost > 2);
        let ev = run(&mut g, Action::Climb);
        assert!(said(&ev).contains("in the dark"), "{}", said(&ev));
        match g.place {
            Place::Town => hurt_total += hp - g.hero.hp,
            Place::Dead { .. } => deaths += 1,
            Place::Delve { .. } => panic!("still below"),
        }
        assert_eq!(g.hero.turns, 0);
    }
    assert!(hurt_total > 0, "the dark costs hit points");
    let _ = deaths;
    // With the turns to spare there is no cost at all.
    let mut safe = game_of("warden", ["quick", "stout"], 1);
    delve(&mut safe, 9);
    safe.hero.salvage = 0;
    let hp = safe.hero.hp;
    run(&mut safe, Action::Climb);
    assert_eq!(safe.hero.hp, hp);
    assert_eq!(safe.place, Place::Town);
}

#[test]
fn with_no_turns_left_below_the_only_move_is_up() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 7);
    delve(&mut g, 2);
    g.hero.turns = 0;
    for action in [Action::Rest, Action::Stairs, Action::Option(0)] {
        let err = g.apply(&c, action).unwrap_err();
        assert!(err.0.contains("torch is out"), "{err:?}");
    }
    assert!(g.apply(&c, Action::Climb).is_ok());
}

#[test]
fn walking_costs_a_turn_and_wading_costs_two_unless_you_can_find_the_way() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 8);
    run(&mut g, Action::Descend(1));
    let level = crate::dungeon::generate(&c, 1, 0, 1);
    let Place::Delve { room, .. } = g.place else {
        panic!()
    };
    let r = &level.rooms[usize::from(room)];
    let (dir, to) = crate::dungeon::Dir::ALL
        .iter()
        .find_map(|d| r.links[d.index()].map(|t| (*d, t)))
        .unwrap();
    let cost = level.template(&c, to).map_or(1, |t| t.move_cost.max(1));
    let before = g.hero.turns;
    // A pack that bars the way would refuse; walk back and forth otherwise.
    if g.apply(&c, Action::Move(dir)).is_ok() {
        assert_eq!(g.hero.turns, before - cost);
    }
}

#[test]
fn a_night_below_costs_torches_and_sometimes_a_fight() {
    let c = content();
    let mut fights = 0;
    for seed in 0..120 {
        let mut g = game_of("warden", ["quick", "stout"], seed);
        delve(&mut g, 2);
        g.hero.hp = 10;
        let ev = g.new_day(&c, TODAY + 1);
        assert!(said(&ev).contains("night below"), "{}", said(&ev));
        if said(&ev).contains("found your camp") {
            fights += 1;
        }
        if matches!(g.place, Place::Delve { .. }) {
            assert_eq!(
                g.hero.turns,
                TURNS_PER_DAY - 8,
                "the torches burned keeping watch"
            );
        }
    }
    assert!((20..=80).contains(&fights), "{fights}/120 camps were found");
}

#[test]
fn resting_costs_two_turns_and_eases_fatigue() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 9);
    run(&mut g, Action::Descend(1));
    g.hero.fatigue = 50;
    g.hero.hp = 5;
    let turns = g.hero.turns;
    let level = crate::dungeon::generate(&c, 1, 0, 1);
    let Place::Delve { room, .. } = g.place else {
        panic!()
    };
    assert!(!g.is_hostile(&c, &level, room));
    run(&mut g, Action::Rest);
    assert_eq!(g.hero.turns, turns - 2);
    assert!(g.hero.fatigue < 50 && g.hero.hp > 5);
}

#[test]
fn salvage_is_banked_in_town_so_the_next_climb_is_light() {
    let mut g = game_of("warden", ["quick", "stout"], 11);
    delve(&mut g, 3);
    g.hero.salvage = 80;
    run(&mut g, Action::Climb);
    assert_eq!((g.hero.salvage, g.hero.stash), (0, 80));
    run(&mut g, Action::Descend(1));
    assert_eq!(g.hero.salvage, 0, "the stash stays in town");
}
