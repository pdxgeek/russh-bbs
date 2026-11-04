//! Gear and crafting: wear and repair, affixes, ember-cores, recipes, drops, and the runesmith's
//! edge. The RPG rule here: a core changes what a hero can *do*, a calling decides which recipes
//! exist for them, and a smith works differently from everyone else.

use super::support::{content, finish, labels, place, rich_game, run, said, town, town_refused};
use crate::combat::FightAct;
use crate::game::{Action, Place};
use crate::hero::{Item, Kit};
use crate::town::{TownAct, Worn};

fn fight_ready(calling: &str, seed: u64) -> crate::game::Game {
    let mut g = rich_game(calling, ["quick", "stout"], seed);
    g.hero.level = 10;
    place(&mut g, "sooty-gallery");
    let c = content();
    let d = g.derived(&c);
    g.hero.hp = d.max_hp;
    g
}

fn start(g: &mut crate::game::Game, ids: &[&str]) {
    let c = content();
    let ids: Vec<String> = ids.iter().map(|s| (*s).to_owned()).collect();
    let mut ev = Vec::new();
    g.start_fight(&c, &ids, false, &mut ev);
}

// ---- wear ----

#[test]
fn wear_blunts_a_weapon_and_thins_armour_until_repaired() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 1);
    let fresh = g.derived(&c);
    g.hero.wkit.wear = 100;
    g.hero.akit.wear = 100;
    let worn = g.derived(&c);
    assert_eq!(worn.damage, fresh.damage - 4);
    assert_eq!(
        worn.armor,
        fresh.armor - 4 + i32::from(fresh.armor < 4) * (4 - fresh.armor)
    );
    town(&mut g, TownAct::Repair(Worn::Weapon));
    town(&mut g, TownAct::Repair(Worn::Armour));
    assert_eq!(g.derived(&c), fresh);
}

#[test]
fn a_won_fight_wears_the_gear_and_a_guardians_fight_wears_it_twice_as_much() {
    let c = content();
    let mut g = fight_ready("warden", 2);
    start(&mut g, &["slag-rat"]);
    let mut ev = Vec::new();
    g.auto_resolve(&c, &mut ev);
    assert!(g.fight.is_none());
    assert!(g.hero.hp > 0, "the test hero wins");
    assert_eq!(g.hero.wkit.wear, 2);
    assert_eq!(g.hero.akit.wear, 2);
    let mut h = fight_ready("warden", 3);
    let mut ev = Vec::new();
    h.start_fight(&c, &["slag-rat".to_owned()], true, &mut ev);
    h.auto_resolve(&c, &mut ev);
    assert!(h.fight.is_none() && h.hero.hp > 0, "the test hero wins");
    assert_eq!(h.hero.wkit.wear, 4);
}

// ---- affixes ----

#[test]
fn an_affix_changes_the_derived_sheet_and_its_tag_changes_the_rooms() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 4);
    let base = g.derived(&c);
    g.hero.wkit.affixes.push("jagged".into());
    assert_eq!(g.derived(&c).damage, base.damage + 2);
    // A lock-wright's weapon opens the lockpick option in a coffer room; a plain one does not.
    let mut plain = rich_game("warden", ["quick", "stout"], 5);
    place(&mut plain, "coffer-room");
    let before = labels(&plain);
    plain.hero.wkit.affixes.push("lockwrights".into());
    let after = labels(&plain);
    assert!(
        after.len() != before.len() || after != before,
        "{before:?} vs {after:?}"
    );
    assert!(plain.derived(&c).has_tag("lockpick"));
}

// ---- ember-cores ----

#[test]
fn a_core_changes_what_a_hero_can_do_in_a_fight_not_just_the_numbers() {
    let c = content();
    // Without the core, the move is refused.
    let mut g = fight_ready("warden", 6);
    start(&mut g, &["slag-rat"]);
    let refused = g.apply(&c, Action::Fight(FightAct::Use("cinder-bolt".into(), 0)));
    assert!(refused.is_err());
    assert!(refused.unwrap_err().0.contains("do not know"));
    // Slot a Cinder Core into the weapon and the same hero can throw a bolt.
    let mut h = rich_game("warden", ["quick", "stout"], 6);
    h.hero.cores.push("cinder-core".into());
    assert!(!h.derived(&c).moves.contains("cinder-bolt"));
    town(&mut h, TownAct::Slot(0, Worn::Weapon));
    assert!(h.derived(&c).moves.contains("cinder-bolt"));
    place(&mut h, "sooty-gallery");
    h.hero.hp = h.derived(&c).max_hp;
    start(&mut h, &["slag-rat"]);
    if let Some(f) = h.fight.as_mut() {
        f.focus = 5;
    }
    let ev = run(
        &mut h,
        Action::Fight(FightAct::Use("cinder-bolt".into(), 0)),
    );
    assert!(
        said(&ev).to_lowercase().contains("cinder bolt"),
        "{}",
        said(&ev)
    );
}

#[test]
fn a_core_can_also_grant_a_capability_that_opens_a_room_option() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 7);
    place(&mut g, "sunless-hollow");
    let before = labels(&g);
    assert!(!before.iter().any(|l| l.contains("lantern")));
    g.hero.akit.core = Some("tinder-core".into());
    let after = labels(&g);
    assert!(after.iter().any(|l| l.contains("lantern")), "{after:?}");
    assert!(g.derived(&c).has_tag("lantern"));
}

#[test]
fn a_core_fits_only_the_slot_it_was_made_for() {
    let mut g = rich_game("warden", ["quick", "stout"], 8);
    g.hero.cores.push("cinder-core".into());
    assert!(town_refused(&mut g, TownAct::Slot(0, Worn::Armour)).contains("does not take"));
    assert_eq!(g.hero.cores.len(), 1, "nothing was spent");
}

#[test]
fn only_a_runesmith_lifts_a_core_out_whole_anyone_else_cracks_the_old_one() {
    let c = content();
    let mut smith = rich_game("runesmith", ["quick", "stout"], 9);
    smith.hero.wkit.core = Some("cinder-core".into());
    smith.hero.cores.push("bite-core".into());
    town(&mut smith, TownAct::Slot(0, Worn::Weapon));
    assert_eq!(smith.hero.wkit.core.as_deref(), Some("bite-core"));
    assert_eq!(
        smith.hero.cores,
        vec!["cinder-core".to_owned()],
        "the old core came out whole"
    );
    town(&mut smith, TownAct::Unslot(Worn::Weapon));
    assert!(smith.hero.wkit.core.is_none());
    assert_eq!(smith.hero.cores.len(), 2);

    let mut other = rich_game("warden", ["quick", "stout"], 9);
    other.hero.wkit.core = Some("cinder-core".into());
    other.hero.cores.push("bite-core".into());
    town(&mut other, TownAct::Slot(0, Worn::Weapon));
    assert!(other.hero.cores.is_empty(), "the old one cracked");
    assert!(town_refused(&mut other, TownAct::Unslot(Worn::Weapon)).contains("runesmith"));
    assert!(other.derived(&c).moves.contains("open-vein"));
}

// ---- recipes ----

#[test]
fn a_calling_decides_which_recipes_a_hero_has_and_a_node_unlocks_another() {
    let c = content();
    let smith = rich_game("runesmith", ["quick", "stout"], 10);
    let warden = rich_game("warden", ["quick", "stout"], 10);
    let mallet = c.recipe("runesmith-mallet").unwrap();
    let plate = c.recipe("warden-plate").unwrap();
    assert!(smith.hero.knows(mallet) && !smith.hero.knows(plate));
    assert!(warden.hero.knows(plate) && !warden.hero.knows(mallet));
    // A skill node teaches a recipe the calling did not start with.
    let mut w = rich_game("warden", ["quick", "stout"], 10);
    let coat = c.recipe("haulers-coat").unwrap();
    assert!(!w.hero.knows(coat));
    w.hero.level = 5;
    w.hero.skill_points = 3;
    w.hero.take_node(&c, "wa-pack-mule").ok();
    w.hero.take_node(&c, "wa-broad-back").ok();
    w.hero.take_node(&c, "wa-pack-mule").ok();
    assert!(w.hero.nodes.contains(&"wa-pack-mule".to_owned()));
    assert!(w.hero.knows(coat));
    // And a calling's recipe refuses another calling's smith.
    let mut w2 = rich_game("warden", ["quick", "stout"], 10);
    assert!(
        town_refused(&mut w2, TownAct::Craft("runesmith-mallet".into())).contains("do not know")
    );
}

#[test]
fn crafting_spends_the_store_and_coin_and_a_runesmith_pays_a_quarter_less_salvage() {
    let c = content();
    let r = c.recipe("runesmith-mallet").unwrap();
    let mut smith = rich_game("runesmith", ["quick", "stout"], 11);
    let (s0, c0) = (smith.hero.stash, smith.hero.coin);
    town(&mut smith, TownAct::Craft(r.id.clone()));
    assert_eq!(smith.hero.pack.len(), 1);
    assert_eq!(smith.hero.pack[0].base, "rune-mallet");
    assert!(smith.hero.pack[0].kit.affixes.contains(&"runed".to_owned()));
    assert_eq!(smith.hero.stash, s0 - (r.salvage * 3).div_ceil(4));
    assert_eq!(smith.hero.coin, c0 - r.coin);
    // A non-smith who could make the same thing would pay the full salvage; compare via a
    // recipe both can read: a node-free found recipe.
    let mut a = rich_game("runesmith", ["quick", "stout"], 12);
    let mut b = rich_game("warden", ["quick", "stout"], 12);
    for g in [&mut a, &mut b] {
        g.hero.learn("found-forge-axe");
        finish(&mut g.world, "new-bellows");
    }
    let (sa, sb) = (a.hero.stash, b.hero.stash);
    town(&mut a, TownAct::Craft("found-forge-axe".into()));
    town(&mut b, TownAct::Craft("found-forge-axe".into()));
    assert!(sa - a.hero.stash < sb - b.hero.stash);
}

#[test]
fn crafting_is_refused_without_stock_forge_or_the_craft_for_it() {
    let c = content();
    let mut g = rich_game("runesmith", ["quick", "stout"], 13);
    g.hero.stash = 1;
    assert!(town_refused(&mut g, TownAct::Craft("runesmith-mallet".into())).contains("salvage"));
    g.hero.stash = 500;
    g.hero.coin = 1;
    assert!(town_refused(&mut g, TownAct::Craft("runesmith-mallet".into())).contains("coin"));
    g.hero.coin = 500;
    // A level-2 pattern needs the better forge, for the whole town.
    g.hero.learn("found-slag-mail");
    assert!(town_refused(&mut g, TownAct::Craft("found-slag-mail".into())).contains("forge"));
    finish(&mut g.world, "new-bellows");
    town(&mut g, TownAct::Craft("found-slag-mail".into()));
    // A core recipe needs runecraft: a warden who learned one cannot work it.
    let mut w = rich_game("warden", ["quick", "stout"], 13);
    w.hero.learn("found-cinder-core");
    assert!(
        town_refused(&mut w, TownAct::Craft("found-cinder-core".into())).contains("lack the craft")
    );
    // ...until a runed weapon gives them the knack.
    w.hero.wkit.affixes.push("runed".into());
    town(&mut w, TownAct::Craft("found-cinder-core".into()));
    assert_eq!(w.hero.cores, vec!["cinder-core".to_owned()]);
    let _ = c;
}

#[test]
fn a_runesmiths_masterwork_adds_an_affix_and_nobody_else_gets_one() {
    let mut master = 0;
    let mut other = 0;
    for seed in 0..80 {
        let mut s = rich_game("runesmith", ["bookish", "stout"], seed);
        town(&mut s, TownAct::Craft("runesmith-mallet".into()));
        if s.hero.pack[0].kit.affixes.len() > 1 {
            master += 1;
        }
        let mut w = rich_game("warden", ["quick", "stout"], seed);
        town(&mut w, TownAct::Craft("warden-plate".into()));
        if w.hero.pack[0].kit.affixes.len() > 1 {
            other += 1;
        }
    }
    assert!(
        master > 5,
        "a smith with a book-learned head masters {master} of 80"
    );
    assert_eq!(other, 0);
}

// ---- finds ----

#[test]
fn a_guardian_drops_its_core_and_anyone_can_use_it() {
    let c = content();
    let mut g = fight_ready("warden", 20);
    g.hero.attrs[0] = 16;
    let mut ev = Vec::new();
    g.start_fight(&c, &["bellows-golem".to_owned()], true, &mut ev);
    if let Some(f) = g.fight.as_mut() {
        f.enemies[0].hp = 1;
    }
    for _ in 0..20 {
        if g.fight.is_none() {
            break;
        }
        let _ = g.apply(&c, Action::Fight(FightAct::Attack(0)));
    }
    assert!(g.fight.is_none());
    assert!(
        g.hero.cores.contains(&"hearth-core".to_owned()),
        "{:?}",
        g.hero.cores
    );
}

#[test]
fn found_gear_comes_home_with_the_hero_and_is_lost_with_them() {
    let c = content();
    let mut g = fight_ready("warden", 21);
    g.hero.found.push(Item {
        base: "hand-axe".into(),
        kit: Kit::default(),
    });
    g.place = Place::Delve { depth: 1, room: 0 };
    run(&mut g, Action::Climb);
    assert!(g.hero.found.is_empty());
    assert_eq!(
        g.hero.pack.len(),
        1,
        "found gear is banked at home on arrival"
    );
    // The same find, carried to the temple floor, is gone; the spare at home is not.
    let mut h = fight_ready("warden", 22);
    h.hero.pack.push(Item {
        base: "forge-axe".into(),
        kit: Kit::default(),
    });
    h.hero.found.push(Item {
        base: "hand-axe".into(),
        kit: Kit::default(),
    });
    h.place = Place::Dead {
        depth: 1,
        room: 0,
        since: super::support::TODAY,
    };
    let _ = h.new_day(&c, super::support::TODAY + 3);
    assert!(h.hero.found.is_empty());
    assert_eq!(h.hero.pack.len(), 1, "spares at home are safe");
}

#[test]
fn drops_come_in_bands_with_affixes_no_better_than_the_band_allows() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 23);
    for _ in 0..200 {
        let item = g.roll_drop(&c, 1).expect("a band-1 drop");
        let band = crate::town::base_band(&c, &item.base);
        assert_eq!(band, 1);
        for a in &item.kit.affixes {
            assert!(c.affix(a).unwrap().band <= 1);
        }
    }
}

#[test]
fn the_reliquary_keeps_a_fallen_heros_weapon_and_nothing_else() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 24);
    g.hero.weapon = "forge-axe".into();
    g.hero.wkit.affixes.push("jagged".into());
    g.hero.armour = "slag-mail".into();
    g.place = Place::Dead {
        depth: 2,
        room: 0,
        since: super::support::TODAY,
    };
    let mut plain = g.clone();
    finish(&mut g.world, "reliquary");
    let _ = g.new_day(&c, super::support::TODAY + 3);
    assert_eq!(g.hero.weapon, "forge-axe");
    assert_eq!(g.hero.wkit.affixes, vec!["jagged".to_owned()]);
    assert_eq!(g.hero.armour, "ringmail", "the armour is lost");
    let _ = plain.new_day(&c, super::support::TODAY + 3);
    assert_eq!(plain.hero.weapon, "iron-halberd", "no reliquary, no weapon");
}
