//! The hero: creation, derived numbers, levelling, skill trees, wounds and scars.

use super::support::{content, game_of, hero_of};
use crate::content::{Attr, Effect};
use crate::hero::{CreateError, Hero, derive};
use crate::rules::{LEVEL_MAX, xp_for_level};

#[test]
fn creation_checks_calling_traits_and_points() {
    let c = content();
    let t = |a: &str, b: &str| vec![a.to_owned(), b.to_owned()];
    assert!(Hero::create(&c, "A", "warden", &t("quick", "stout"), [4, 0, 0, 0]).is_ok());
    assert_eq!(
        Hero::create(&c, "A", "nope", &t("quick", "stout"), [4, 0, 0, 0]).err(),
        Some(CreateError::UnknownCalling)
    );
    assert_eq!(
        Hero::create(&c, "A", "warden", &t("quick", "nope"), [4, 0, 0, 0]).err(),
        Some(CreateError::UnknownTrait)
    );
    assert_eq!(
        Hero::create(&c, "A", "warden", &t("quick", "quick"), [4, 0, 0, 0]).err(),
        Some(CreateError::DuplicateTrait)
    );
    assert_eq!(
        Hero::create(&c, "A", "warden", &["quick".to_owned()], [4, 0, 0, 0]).err(),
        Some(CreateError::TraitCount)
    );
    assert_eq!(
        Hero::create(&c, "A", "warden", &t("quick", "stout"), [3, 0, 0, 0]).err(),
        Some(CreateError::Points)
    );
    assert_eq!(
        Hero::create(&c, "A", "warden", &t("quick", "stout"), [5, 0, 0, 0]).err(),
        Some(CreateError::Points)
    );
}

#[test]
fn the_calling_the_traits_and_the_points_all_shape_the_sheet() {
    let c = content();
    let warden = derive(&hero_of("warden", ["quick", "stout"]), &c);
    let cutthroat = derive(&hero_of("cutthroat", ["quick", "stout"]), &c);
    assert!(warden.max_hp > cutthroat.max_hp, "wardens are tougher");
    assert!(warden.armor > cutthroat.armor);
    assert!(cutthroat.has_tag("lockpick") && !warden.has_tag("lockpick"));
    assert!(warden.moves.contains("shield-bash") && !cutthroat.moves.contains("shield-bash"));
    // A trait is on top of the calling.
    let with = derive(&hero_of("warden", ["stout", "quick"]), &c);
    let without = derive(&hero_of("warden", ["keen-eyed", "quick"]), &c);
    assert_eq!(with.max_hp, without.max_hp + 8);
    assert_eq!(with.trap_sense + 2, without.trap_sense);
}

fn differs(c: &crate::content::Content, effects_of: impl Fn(&mut Hero)) -> bool {
    c.callings.calling.iter().any(|calling| {
        let mut base = hero_of(&calling.id, ["quick", "stout"]);
        base.traits.clear();
        let mut changed = base.clone();
        effects_of(&mut changed);
        derive(&base, c) != derive(&changed, c)
    })
}

#[test]
fn every_skill_node_changes_the_derived_sheet() {
    let c = content();
    assert_eq!(c.nodes.node.len(), 90);
    for node in &c.nodes.node {
        let mut base = hero_of(&node.calling, ["quick", "stout"]);
        base.traits.clear();
        let mut taken = base.clone();
        taken.nodes.push(node.id.clone());
        assert_ne!(
            derive(&base, &c),
            derive(&taken, &c),
            "node {} does nothing",
            node.id
        );
    }
}

#[test]
fn every_trait_scar_and_wound_changes_the_derived_sheet() {
    let c = content();
    for t in &c.traits.traits {
        assert!(
            differs(&c, |h| h.traits.push(t.id.clone())),
            "trait {}",
            t.id
        );
    }
    for s in &c.traits.scar {
        assert!(differs(&c, |h| h.scars.push(s.id.clone())), "scar {}", s.id);
    }
    for w in &c.wounds.wound {
        assert!(
            differs(&c, |h| h.wounds.push((w.id.clone(), 3))),
            "wound {}",
            w.id
        );
    }
}

#[test]
fn every_calling_has_a_fifteen_node_tree_with_forks() {
    let c = content();
    assert_eq!(c.callings.calling.len(), 6);
    for calling in &c.callings.calling {
        let tree: Vec<_> = c
            .nodes
            .node
            .iter()
            .filter(|n| n.calling == calling.id)
            .collect();
        assert_eq!(tree.len(), 15);
        let forks = tree.iter().filter(|n| !n.excludes.is_empty()).count();
        assert!(forks >= 4, "{} has {forks} fork nodes", calling.id);
    }
    assert_eq!(c.traits.traits.len(), 24);
}

#[test]
fn experience_levels_the_hero_and_pays_points() {
    let mut h = hero_of("warden", ["quick", "stout"]);
    assert_eq!((h.level, h.skill_points, h.attr_points), (1, 1, 0));
    assert_eq!(h.gain_xp(xp_for_level(2) - 1), 0);
    assert_eq!(h.gain_xp(1), 1);
    assert_eq!((h.level, h.skill_points), (2, 2));
    h.gain_xp(xp_for_level(4));
    assert_eq!(h.level, 4);
    assert_eq!(h.attr_points, 1, "an attribute point at level 4");
    h.gain_xp(1_000_000);
    assert_eq!(h.level, LEVEL_MAX);
    assert_eq!(
        h.skill_points, 12,
        "points stop at level 12: no tree is ever finished"
    );
    assert_eq!(h.attr_points, 5);
}

#[test]
fn skills_need_points_level_and_an_earlier_node_and_forks_close_for_good() {
    let c = content();
    let mut h = hero_of("warden", ["quick", "stout"]);
    assert!(h.take_node(&c, "wa-iron-skin").is_ok());
    assert!(h.take_node(&c, "wa-iron-skin").is_err(), "not twice");
    assert!(h.take_node(&c, "wa-cleave").is_err(), "no points left");
    h.gain_xp(xp_for_level(5));
    assert!(
        h.take_node(&c, "wa-shove").is_err(),
        "needs wa-cleave first"
    );
    assert!(h.take_node(&c, "wa-cleave").is_ok());
    assert!(
        h.take_node(&c, "cu-quick-hands").is_err(),
        "another calling's tree"
    );
    assert!(h.take_node(&c, "wa-shove").is_ok());
    // The fork: taking Shoving Match closes Living Wall for good, even when its own path opens.
    assert!(
        h.take_node(&c, "wa-stand-firm").is_err(),
        "needs wa-grim-resolve"
    );
    assert!(h.take_node(&c, "wa-grim-resolve").is_ok());
    assert!(h.take_node(&c, "wa-stand-firm").is_ok());
    let why = h.node_refusal(&c, "wa-wall").expect("closed");
    assert!(why.contains("closed"), "{why}");
    let level = h.level;
    assert!(level >= 5);
}

#[test]
fn a_tier_needs_the_level_for_it() {
    let c = content();
    let mut h = hero_of("duelist", ["quick", "stout"]);
    h.skill_points = 10;
    h.nodes = vec![
        "du-keen-edge".into(),
        "du-flourish".into(),
        "du-showman".into(),
    ];
    assert!(
        h.node_refusal(&c, "du-blademaster")
            .is_some_and(|w| w.contains("experienced"))
    );
    h.level = 7;
    assert_eq!(h.node_refusal(&c, "du-blademaster"), None);
}

#[test]
fn attribute_points_raise_attributes_to_a_cap() {
    let mut h = hero_of("warden", ["quick", "stout"]);
    assert!(h.raise(Attr::Might).is_err());
    h.attr_points = 9;
    while h.raise(Attr::Might).is_ok() {}
    assert_eq!(h.attrs[0], crate::rules::ATTR_MAX);
    assert!(h.attr_points > 0, "points are kept when the cap stops them");
}

#[test]
fn wounds_cost_something_while_they_last_and_may_leave_a_scar() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 5);
    let healthy = g.derived(&c);
    g.hero.inflict(&c, "broken-ribs");
    let hurt = g.derived(&c);
    assert!(hurt.max_hp < healthy.max_hp);
    assert!(hurt.fatigue_guard < healthy.fatigue_guard);
    // Heal over the days: across many seeds some leave a scar and some do not.
    let mut scarred = 0;
    for seed in 0..200 {
        let mut g = game_of("warden", ["quick", "stout"], seed);
        g.hero.inflict(&c, "crushed-leg");
        let _ = g.new_day(&c, 20_005);
        assert!(g.hero.wounds.is_empty(), "five days heal a crushed leg");
        if g.hero.scars.contains(&"limp".to_owned()) {
            scarred += 1;
        }
    }
    assert!(
        (40..=110).contains(&scarred),
        "35% scar chance, got {scarred}/200"
    );
}

#[test]
fn a_hero_never_carries_more_than_three_scars() {
    let c = content();
    let mut h = hero_of("warden", ["quick", "stout"]);
    for s in ["limp", "nightmares", "cracked-rib"] {
        assert!(h.can_scar(s));
        h.scars.push(s.to_owned());
    }
    assert!(!h.can_scar("dented-skull"));
    assert!(
        derive(&h, &c).climb_ease < 0,
        "a limp makes the climb dearer"
    );
}

#[test]
fn unknown_ids_in_an_old_save_are_ignored_not_fatal() {
    let c = content();
    let mut h = hero_of("warden", ["quick", "stout"]);
    h.nodes.push("removed-node".into());
    h.traits.push("removed-trait".into());
    h.wounds.push(("removed-wound".into(), 2));
    let _ = derive(&h, &c);
    assert!(matches!(
        c.calling("warden")
            .unwrap()
            .tags
            .first()
            .map(String::as_str),
        Some("brawn")
    ));
    let _ = Effect::Tag { tag: "x".into() };
}
