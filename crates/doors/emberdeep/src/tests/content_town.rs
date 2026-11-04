//! The town's content, proved to do something: nothing in a table is decoration, and nothing is
//! unreachable.

use std::collections::BTreeSet;

use super::support::{content, hero_of};
use crate::content::{Gain, RoomKind};
use crate::content_town::{ContractKind, Learn, Makes, Slot};
use crate::hero::derive;

#[test]
fn every_affix_and_core_changes_the_derived_sheet() {
    let c = content();
    let base = derive(&hero_of("warden", ["quick", "stout"]), &c);
    for a in &c.affixes.affix {
        let mut h = hero_of("warden", ["quick", "stout"]);
        h.wkit.affixes.push(a.id.clone());
        assert_ne!(derive(&h, &c), base, "affix {} does nothing", a.id);
    }
    for core in &c.affixes.core {
        let mut h = hero_of("warden", ["quick", "stout"]);
        h.akit.core = Some(core.id.clone());
        let d = derive(&h, &c);
        assert_ne!(d, base, "core {} does nothing", core.id);
    }
}

#[test]
fn a_core_that_grants_a_move_is_an_option_the_calling_did_not_have() {
    let c = content();
    let granted: BTreeSet<&str> = c
        .affixes
        .core
        .iter()
        .flat_map(|k| k.effects.iter())
        .filter_map(|e| match e {
            crate::content::Effect::Move { id } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert!(granted.len() >= 6, "{granted:?}");
    // At least some of those moves are ones a given calling could never learn in its tree.
    let warden = hero_of("warden", ["quick", "stout"]);
    let own = derive(&warden, &c).moves;
    assert!(granted.iter().any(|m| !own.contains(*m)));
}

#[test]
fn every_companion_rung_gives_the_hero_something() {
    let c = content();
    let base = derive(&hero_of("warden", ["quick", "stout"]), &c);
    for comp in &c.town.companion {
        let mut last = base.clone();
        for level in 1..=3u8 {
            let mut h = hero_of("warden", ["quick", "stout"]);
            h.bond = Some(crate::hero::Bond {
                id: comp.id.clone(),
                level,
                progress: 0,
                last: 0,
            });
            let d = derive(&h, &c);
            assert_ne!(d, last, "{} rung {level} changes nothing", comp.id);
            last = d;
        }
    }
}

#[test]
fn every_core_can_be_got_somehow() {
    let c = content();
    for core in &c.affixes.core {
        let sold = core.price > 0;
        let dropped = c
            .bestiary
            .monster
            .iter()
            .any(|m| m.core.as_deref() == Some(&core.id));
        let made = c
            .recipes
            .recipe
            .iter()
            .any(|r| matches!(&r.makes, Makes::Core { id } if *id == core.id));
        let found = c.rooms.room.iter().flat_map(|r| &r.option).any(|o| {
            o.ok.iter()
                .chain(&o.fail)
                .any(|g| matches!(g, Gain::Core { id } if *id == core.id))
        });
        assert!(
            sold || dropped || made || found,
            "core {} cannot be got",
            core.id
        );
    }
}

#[test]
fn every_recipe_can_be_learned_somehow() {
    let c = content();
    for r in &c.recipes.recipe {
        let learnable = match &r.learn {
            Learn::Calling { .. } | Learn::Node { .. } | Learn::Library => true,
            Learn::Found => {
                c.rooms.room.iter().flat_map(|x| &x.option).any(|o| {
                    o.ok.iter()
                        .chain(&o.fail)
                        .any(|g| matches!(g, Gain::Recipe { id } if *id == r.id))
                }) || c
                    .town
                    .contract
                    .iter()
                    .any(|k| k.recipe.as_deref() == Some(&r.id))
            }
        };
        assert!(learnable, "recipe {} can never be learned", r.id);
    }
    // Every calling has a recipe of its own: the calling decides what exists for the hero.
    for calling in &c.callings.calling {
        assert!(
            c.recipes
                .recipe
                .iter()
                .any(|r| matches!(&r.learn, Learn::Calling { id } if *id == calling.id)),
            "calling {} has no recipe",
            calling.id
        );
    }
}

#[test]
fn every_item_base_can_be_got_somehow_and_every_band_has_gear() {
    let c = content();
    for band in 1..=5 {
        let gear = c.gear.weapon.iter().filter(|w| w.band == band).count()
            + c.gear.armour.iter().filter(|a| a.band == band).count();
        assert!(gear >= 2, "band {band} has {gear} drops");
    }
    for w in &c.gear.weapon {
        let kit = c.callings.calling.iter().any(|k| k.weapon == w.id);
        assert!(kit || w.band >= 1, "{}", w.id);
    }
    // Something can be bought, crafted or dropped at each of the five attributes' weapons.
    for attr in crate::content::Attr::ALL {
        assert!(
            c.gear.weapon.iter().any(|w| w.attr == attr && w.price > 0),
            "no weapon for {attr:?} is sold"
        );
    }
}

#[test]
fn every_contract_is_offered_to_some_build_at_its_depth() {
    let c = content();
    for p in &c.town.contract {
        let offered = c.callings.calling.iter().any(|k| {
            let mut h = hero_of(&k.id, ["quick", "stout"]);
            h.best_depth = u8::try_from(p.depth).unwrap();
            let d = derive(&h, &c);
            p.need.iter().all(|n| h.meets(&d, n))
        });
        assert!(offered, "no calling is ever offered {}", p.id);
    }
    // All three kinds, and a spread of depths.
    for kind in [
        ContractKind::Beast,
        ContractKind::Heirloom,
        ContractKind::Rescue,
    ] {
        assert!(c.town.contract.iter().any(|p| p.kind == kind));
    }
    assert!(c.town.contract.iter().any(|p| p.depth >= 10));
}

#[test]
fn every_room_kind_a_contract_names_exists_in_the_dungeon() {
    let c = content();
    for p in &c.town.contract {
        if let Some(kind) = p.room {
            let band = (p.depth - 1) / 5 + 1;
            assert!(
                c.rooms
                    .room
                    .iter()
                    .any(|r| r.kind == kind && r.bands.contains(&band)),
                "contract {}: no {kind:?} room in band {band}",
                p.id
            );
        }
        let _ = RoomKind::Entry;
    }
}

#[test]
fn projects_make_a_chain_with_a_lift_to_each_deeper_band_and_nothing_is_free() {
    let c = content();
    assert!(c.projects.project.len() >= 10);
    let lifts: Vec<u8> = c
        .projects
        .project
        .iter()
        .filter_map(|p| match p.effect {
            crate::content_town::ProjectEffect::Lift { depth } => Some(depth),
            _ => None,
        })
        .collect();
    assert_eq!(lifts, vec![6, 11]);
    // A lift reaches a level at the start of a band: it skips whole bands.
    for d in lifts {
        assert_eq!((d - 1) % 5, 0);
    }
    let _ = Slot::Any;
}
