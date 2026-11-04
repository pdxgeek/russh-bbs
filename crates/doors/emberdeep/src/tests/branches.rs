//! The RPG rule: a build unlocks different options in the same room, the options lead to
//! different outcomes, and the outcomes persist (for the hero, or for everyone).

use super::support::{TODAY, content, game_of, hero_of, labels, place, run};
use crate::content::ShortcutKind;
use crate::game::{Action, Game, Place};

fn at(game: &mut Game, template: &str) -> (u8, u8) {
    place(game, template)
}

/// Run the option whose label contains `needle`, returning whether it was there.
fn choose(game: &mut Game, needle: &str) -> bool {
    let c = content();
    let options = game.options(&c);
    match options.iter().position(|o| o.label.contains(needle)) {
        Some(i) => game.apply(&c, Action::Option(i)).is_ok(),
        None => false,
    }
}

#[test]
fn the_same_broken_lift_offers_each_build_something_different() {
    let seeds = |calling: &str, traits: [&str; 2]| {
        let mut g = game_of(calling, traits, 1);
        at(&mut g, "broken-lift");
        labels(&g)
    };
    let smith = seeds("runesmith", ["quick", "stout"]);
    let warden = seeds("warden", ["quick", "stout"]);
    let duelist = seeds("duelist", ["quick", "stout"]);
    let tinkerer = seeds("duelist", ["tinker", "stout"]);
    assert!(smith.iter().any(|l| l.contains("rune-work")), "{smith:?}");
    assert!(!smith.iter().any(|l| l.contains("Haul the chain")));
    assert!(
        warden.iter().any(|l| l.contains("Haul the chain")),
        "{warden:?}"
    );
    assert!(!warden.iter().any(|l| l.contains("rune-work")));
    assert_eq!(
        duelist,
        ["Study the broken lift"],
        "no tools, no brawn: only a look"
    );
    assert!(
        tinkerer.iter().any(|l| l.contains("tinker's touch")),
        "a trait changes the room too: {tinkerer:?}"
    );
    // The four lists are all different.
    let all = [&smith, &warden, &duelist, &tinkerer];
    for i in 0..4 {
        for j in i + 1..4 {
            assert_ne!(all[i], all[j]);
        }
    }
}

#[test]
fn what_the_build_does_there_changes_the_world_in_different_ways() {
    // The runesmith always raises a lift. The warden, on a good roll, raises only a rope line.
    let mut smith = game_of("runesmith", ["quick", "stout"], 2);
    let (depth, _) = at(&mut smith, "broken-lift");
    assert!(choose(&mut smith, "rune-work"));
    assert_eq!(smith.world.shortcut_at(depth), Some(ShortcutKind::Lift));
    assert_eq!(smith.take_changes().len(), 1);

    let (mut raised, mut hurt) = (0, 0);
    for seed in 0..60 {
        let mut warden = game_of("warden", ["quick", "stout"], seed);
        at(&mut warden, "broken-lift");
        let hp = warden.hero.hp;
        assert!(choose(&mut warden, "Haul the chain"));
        match warden.world.shortcut_at(depth) {
            Some(kind) => {
                assert_eq!(kind, ShortcutKind::Rope);
                raised += 1;
            }
            None => {
                assert!(warden.hero.hp < hp, "a failed haul costs blood");
                hurt += 1;
            }
        }
    }
    assert!(raised > 10 && hurt > 5, "{raised} raised, {hurt} hurt");
}

#[test]
fn the_same_locked_coffer_is_four_different_problems() {
    let list = |calling: &str, traits: [&str; 2]| {
        let mut g = game_of(calling, traits, 3);
        at(&mut g, "coffer-room");
        labels(&g)
    };
    let thief = list("cutthroat", ["quick", "stout"]);
    let warden = list("warden", ["quick", "stout"]);
    let priest = list("ember-priest", ["quick", "stout"]);
    let smith = list("runesmith", ["quick", "stout"]);
    assert_eq!(thief, ["Pick the lock"]);
    assert_eq!(warden, ["Force the lid"]);
    assert_eq!(priest, ["Smash the hasp with whatever you have"]);
    assert_eq!(
        smith,
        [
            "Smash the hasp with whatever you have",
            "Read the maker's mark on the coffer"
        ]
    );
}

#[test]
fn the_pick_always_pays_while_the_hammer_does_not() {
    let (mut thief_coin, mut priest_coin) = (0, 0);
    for seed in 0..50 {
        let mut thief = game_of("cutthroat", ["quick", "stout"], seed);
        at(&mut thief, "coffer-room");
        let coin = thief.hero.coin;
        assert!(choose(&mut thief, "Pick the lock"));
        thief_coin += thief.hero.coin - coin;
        let mut priest = game_of("ember-priest", ["quick", "stout"], seed);
        at(&mut priest, "coffer-room");
        let coin = priest.hero.coin;
        assert!(choose(&mut priest, "Smash"));
        priest_coin += priest.hero.coin - coin;
    }
    assert!(
        thief_coin > priest_coin * 2,
        "{thief_coin} vs {priest_coin}"
    );
}

#[test]
fn a_skill_node_opens_an_option_that_was_not_there() {
    let mut g = game_of("wanderer", ["quick", "stout"], 4);
    at(&mut g, "rope-shaft");
    assert!(labels(&g).iter().any(|l| l.contains("Free-climb")));
    assert!(!labels(&g).iter().any(|l| l.contains("Fix the pitons")));
    g.hero.level = 3;
    g.hero.skill_points = 2;
    g.hero.take_node(&content(), "wn-forager").unwrap();
    g.hero.take_node(&content(), "wn-ropework").unwrap();
    let after = labels(&g);
    assert!(
        after.iter().any(|l| l.contains("Fix the pitons")),
        "{after:?}"
    );
    assert!(
        !after.iter().any(|l| l.contains("Free-climb")),
        "the better way replaces the worse"
    );
}

#[test]
fn a_scar_can_open_a_door() {
    let mut g = game_of("warden", ["quick", "stout"], 5);
    at(&mut g, "slag-den");
    assert!(!labels(&g).iter().any(|l| l.contains("Stare")));
    g.hero.scars.push("grim-visage".into());
    assert!(labels(&g).iter().any(|l| l.contains("Stare them down")));
}

#[test]
fn a_lair_has_a_different_way_past_for_each_kind_of_hero() {
    let list = |calling: &str, traits: [&str; 2]| {
        let mut g = game_of(calling, traits, 6);
        at(&mut g, "slag-den");
        labels(&g)
    };
    assert_eq!(list("warden", ["quick", "stout"]), ["Fight the pack"]);
    assert!(
        list("cutthroat", ["quick", "stout"])
            .iter()
            .any(|l| l.contains("Creep past"))
    );
    assert!(
        list("wanderer", ["quick", "stout"])
            .iter()
            .any(|l| l.contains("Lure them"))
    );
    assert!(
        list("warden", ["sharp-tongued", "stout"])
            .iter()
            .any(|l| l.contains("Stare"))
    );
}

#[test]
fn sneaking_past_a_lair_clears_the_way_without_a_fight_and_failing_starts_one() {
    let (mut clean, mut fought) = (0, 0);
    for seed in 0..60 {
        let mut g = game_of("cutthroat", ["quick", "stout"], seed);
        let (depth, room) = at(&mut g, "slag-den");
        let hp = g.hero.hp;
        assert!(choose(&mut g, "Creep past"));
        if g.fight.is_some() {
            fought += 1;
        } else {
            clean += 1;
            assert!(g.today.is_cleared(depth, room));
            assert_eq!(g.hero.hp, hp, "no fight, no damage");
        }
    }
    assert!(clean > 15 && fought > 5, "{clean} clean, {fought} fights");
}

#[test]
fn a_pack_bars_the_way_until_it_is_dealt_with() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 7);
    let (depth, room) = at(&mut g, "slag-den");
    let level = crate::dungeon::generate(&c, 1, 0, depth);
    let r = &level.rooms[usize::from(room)];
    let forward = crate::dungeon::Dir::ALL
        .iter()
        .find(|d| r.links[d.index()].is_some_and(|t| t != g.prev_room))
        .copied();
    if let Some(dir) = forward {
        let err = g.apply(&c, Action::Move(dir)).unwrap_err();
        assert!(err.0.contains("bars the way"), "{err:?}");
        g.today.cleared.push((depth, room));
        assert!(g.apply(&c, Action::Move(dir)).is_ok());
    }
}

#[test]
fn a_wounded_hero_is_a_different_hero_until_the_wound_heals() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 8);
    let healthy = g.derived(&c);
    g.hero.inflict(&c, "concussion");
    let hurt = g.derived(&c);
    assert_eq!(
        hurt.attr(crate::content::Attr::Wits),
        healthy.attr(crate::content::Attr::Wits) - 2
    );
    // That alone changes what a room lets them do: some options need wits.
    let _ = g.new_day(&c, TODAY + 4);
    assert!(g.hero.wounds.is_empty());
    assert_eq!(
        g.derived(&c).attr(crate::content::Attr::Wits),
        healthy.attr(crate::content::Attr::Wits)
            + i32::from(g.hero.scars.contains(&"dented-skull".to_owned())) * -1
    );
}

#[test]
fn the_same_skill_forks_leave_two_different_heroes() {
    // Same calling, same level, opposite choices at the fork: different moves, different
    // sheets, and neither can ever have what the other has.
    let c = content();
    let take = |first: &str| {
        let mut h = hero_of("warden", ["quick", "stout"]);
        h.level = 9;
        h.skill_points = 6;
        h.take_node(&c, "wa-iron-skin").unwrap();
        h.take_node(&c, "wa-cleave").unwrap();
        h.take_node(&c, "wa-grim-resolve").unwrap();
        h.take_node(&c, "wa-stand-firm").unwrap();
        h.take_node(&c, first).unwrap();
        h
    };
    let shover = take("wa-shove");
    let wall = take("wa-wall");
    let a = crate::hero::derive(&shover, &c);
    let b = crate::hero::derive(&wall, &c);
    assert!(a.moves.contains("shove-back") && !b.moves.contains("shove-back"));
    assert!(b.has_tag("bulwark") && !a.has_tag("bulwark"));
    assert!(b.armor > a.armor);
    let mut s = shover;
    s.skill_points = 3;
    assert!(s.take_node(&c, "wa-wall").is_err());
    let mut w = wall;
    w.skill_points = 3;
    assert!(w.take_node(&c, "wa-shove").is_err());
}

#[test]
fn every_calling_has_a_fork_that_closes_a_door_for_good() {
    let c = content();
    for calling in &c.callings.calling {
        let tree: Vec<_> = c
            .nodes
            .node
            .iter()
            .filter(|n| n.calling == calling.id)
            .collect();
        for node in tree.iter().filter(|n| !n.excludes.is_empty()) {
            for other in &node.excludes {
                let mut h = hero_of(&calling.id, ["quick", "stout"]);
                h.level = 15;
                h.skill_points = 15;
                // Take every ancestor, then the node, then try its rival.
                let mut path = vec![node.id.clone()];
                let mut frontier = vec![node.id.clone()];
                while let Some(id) = frontier.pop() {
                    let n = c.node(&id).unwrap();
                    if let Some(parent) = n.requires.first() {
                        path.push(parent.clone());
                        frontier.push(parent.clone());
                    }
                }
                path.reverse();
                for id in &path {
                    h.nodes.push(id.clone());
                }
                let refusal = h.node_refusal(&c, other);
                assert!(refusal.is_some(), "{} then {other}", node.id);
            }
        }
    }
}

#[test]
fn a_hero_at_a_guardian_door_can_only_fight_it() {
    let c = content();
    let mut g = game_of("cutthroat", ["quick", "stout"], 9);
    let (depth, room) = at(&mut g, "gate-bellows");
    assert_eq!(labels(&g), ["Face the Bellows Golem"]);
    let level = crate::dungeon::generate(&c, 1, 0, depth);
    assert!(g.is_hostile(&c, &level, room));
    assert!(
        g.apply(&c, Action::Stairs).is_err(),
        "the guardian holds the stair"
    );
    // Once the guardian is dead for the board, the stair is free for everyone.
    g.world
        .apply(&crate::world::Change::Slain(1, "Someone".into()));
    assert!(!g.is_hostile(&c, &level, room));
    assert!(matches!(g.place, Place::Delve { .. }));
    run(&mut g, Action::Stairs);
    assert!(matches!(g.place, Place::Delve { depth: d, .. } if d == depth + 1));
}
