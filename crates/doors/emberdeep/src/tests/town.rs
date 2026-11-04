//! The town's places: market, forge, temple, guild hall and tavern.

use super::support::{TODAY, content, finish, game_of, rich_game, town, town_refused};
use crate::game::{Action, Place};
use crate::hero::{Item, Kit};
use crate::town::{TownAct, Wares, Worn, repair_cost};
use crate::world::World;

// ---- market ----

#[test]
fn the_market_stock_is_the_same_for_everyone_today_and_changes_tomorrow() {
    let c = content();
    let a = game_of("warden", ["quick", "stout"], 1);
    let b = game_of("cutthroat", ["keen-eyed", "lucky"], 2);
    assert_eq!(a.stock(&c), b.stock(&c));
    let mut later = game_of("warden", ["quick", "stout"], 1);
    let _ = later.new_day(&c, TODAY + 1);
    assert_ne!(a.stock(&c), later.stock(&c));
}

#[test]
fn the_shelves_follow_the_shared_town() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 1);
    let base = g.stock(&c);
    assert_eq!(base.len(), 4, "a plain market has four stalls and no cores");
    assert!(base.iter().all(|s| matches!(s.wares, Wares::Gear(_))));
    let top = |g: &crate::game::Game| {
        g.stock(&c)
            .iter()
            .filter_map(|s| match &s.wares {
                Wares::Gear(i) => Some(crate::town::base_band(&c, &i.base)),
                Wares::Core(_) => None,
            })
            .max()
            .unwrap_or(0)
    };
    assert_eq!(top(&g), 1, "only the Upper Works' goods while nobody has gone deep");
    // A hero reaches level 9: the whole town's shelves get better.
    g.world.deepest.push(("Someone".into(), 9));
    assert!(g.stock(&c).iter().any(|s| matches!(&s.wares, Wares::Gear(i) if crate::town::base_band(&c, &i.base) >= 3)));
    // And a bigger market adds stalls and cores.
    finish(&mut g.world, "market-stalls");
    let big = g.stock(&c);
    assert_eq!(big.len(), 4 + 2 + 1);
    assert!(big.iter().any(|s| matches!(s.wares, Wares::Core(_))));
}

#[test]
fn buying_spends_coin_once_and_a_full_pack_refuses() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 3);
    let stock = g.stock(&c);
    let price = stock[0].price;
    let before = g.hero.coin;
    town(&mut g, TownAct::BuyStock(0));
    assert_eq!(g.hero.coin, before - price);
    assert_eq!(g.hero.pack.len(), 1);
    assert!(town_refused(&mut g, TownAct::BuyStock(0)).contains("sold"));
    assert_eq!(g.hero.coin, before - price, "no second charge");
    // Fill the pack.
    g.hero.pack = vec![
        Item {
            base: "hand-axe".into(),
            kit: Kit::default()
        };
        6
    ];
    assert!(town_refused(&mut g, TownAct::BuyStock(1)).contains("full"));
    // A poor hero is refused with the price.
    let mut poor = game_of("warden", ["quick", "stout"], 3);
    poor.hero.coin = 1;
    assert!(town_refused(&mut poor, TownAct::BuyStock(0)).contains("coin"));
}

#[test]
fn selling_pays_by_value_and_wear_and_the_haggler_gets_more() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 4);
    let mut item = Item {
        base: "forge-axe".into(),
        kit: Kit::default(),
    };
    g.hero.pack.push(item.clone());
    let fresh = g.sell_price(&c, &item);
    item.kit.wear = 60;
    let worn = g.sell_price(&c, &item);
    assert!(worn < fresh, "{worn} < {fresh}");
    let mut greedy = rich_game("warden", ["scavenger", "stout"], 4);
    greedy.hero.pack.push(Item {
        base: "forge-axe".into(),
        kit: Kit::default(),
    });
    assert!(greedy.sell_price(&c, &item.clone()) >= worn);
    assert!(greedy.sell_price(&c, &Item { base: "forge-axe".into(), kit: Kit::default() }) > fresh);
    let before = g.hero.coin;
    town(&mut g, TownAct::Sell(0));
    assert_eq!(g.hero.coin, before + fresh);
    assert!(g.hero.pack.is_empty());
}

#[test]
fn a_core_must_come_out_before_the_market_will_buy_the_gear() {
    let mut g = rich_game("warden", ["quick", "stout"], 4);
    g.hero.pack.push(Item {
        base: "hand-axe".into(),
        kit: Kit {
            core: Some("cinder-core".into()),
            ..Kit::default()
        },
    });
    assert!(town_refused(&mut g, TownAct::Sell(0)).contains("core"));
}

#[test]
fn wielding_needs_the_level_and_swaps_with_what_is_worn() {
    let mut g = rich_game("warden", ["quick", "stout"], 4);
    g.hero.pack.push(Item {
        base: "glass-cleaver".into(),
        kit: Kit::default(),
    });
    g.hero.level = 3;
    assert!(town_refused(&mut g, TownAct::Equip(0)).contains("level 7"));
    g.hero.level = 7;
    town(&mut g, TownAct::Equip(0));
    assert_eq!(g.hero.weapon, "glass-cleaver");
    assert_eq!(g.hero.pack[0].base, "iron-halberd", "the old weapon is kept");
}

// ---- forge ----

#[test]
fn repairs_cost_by_wear_forge_level_and_the_smiths_own_hand() {
    assert_eq!(repair_cost(0, 1, 1, false), 0);
    assert!(repair_cost(80, 1, 1, false) > repair_cost(40, 1, 1, false));
    assert!(repair_cost(80, 1, 2, false) < repair_cost(80, 1, 1, false));
    assert!(repair_cost(80, 1, 3, false) < repair_cost(80, 1, 2, false));
    assert!(repair_cost(80, 3, 1, false) > repair_cost(80, 1, 1, false));
    assert!(repair_cost(80, 1, 1, true) < repair_cost(80, 1, 1, false));
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 5);
    g.hero.wkit.wear = 80;
    let plain = g.repair_price(&c, Worn::Weapon);
    finish(&mut g.world, "new-bellows");
    assert!(
        g.repair_price(&c, Worn::Weapon) < plain,
        "a finished project makes repairs cheaper for everyone"
    );
    let coin = g.hero.coin;
    let price = g.repair_price(&c, Worn::Weapon);
    town(&mut g, TownAct::Repair(Worn::Weapon));
    assert_eq!(g.hero.wkit.wear, 0);
    assert_eq!(g.hero.coin, coin - price);
    assert!(town_refused(&mut g, TownAct::Repair(Worn::Weapon)).contains("no repair"));
}

// ---- temple ----

#[test]
fn the_temple_tends_for_coin_and_turns_until_the_town_builds_a_roof() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 6);
    g.hero.hp = 5;
    g.hero.fatigue = 40;
    assert_eq!(g.heal_turns(&c), 2);
    let price = g.heal_price(&c);
    assert!(price > 0);
    let (coin, turns) = (g.hero.coin, g.hero.turns);
    town(&mut g, TownAct::Heal);
    assert_eq!(g.hero.hp, g.derived(&c).max_hp);
    assert_eq!(g.hero.fatigue, 0);
    assert_eq!((g.hero.coin, g.hero.turns), (coin - price, turns - 2));
    assert!(town_refused(&mut g, TownAct::Heal).contains("nothing to mend"));

    // The roof goes on: the same visit costs no turns, for everyone.
    let mut h = rich_game("duelist", ["quick", "stout"], 6);
    finish(&mut h.world, "temple-roof");
    h.hero.hp = 5;
    assert_eq!(h.heal_turns(&c), 0);
    let turns = h.hero.turns;
    town(&mut h, TownAct::Heal);
    assert_eq!(h.hero.turns, turns);
}

#[test]
fn an_ember_priest_pays_less_for_the_sisters_work() {
    let c = content();
    let mut priest = rich_game("ember-priest", ["quick", "stout"], 6);
    let mut warden = rich_game("warden", ["quick", "stout"], 6);
    for g in [&mut priest, &mut warden] {
        g.hero.hp = 1;
        g.hero.hp = g.hero.hp.min(1);
    }
    // Same missing hit points: compare per-point price.
    let p = priest.heal_price(&c) * 1000 / u32::try_from(priest.derived(&c).max_hp - 1).unwrap();
    let w = warden.heal_price(&c) * 1000 / u32::try_from(warden.derived(&c).max_hp - 1).unwrap();
    assert!(p < w, "{p} < {w}");
}

#[test]
fn a_cure_lifts_a_wound_and_leaves_no_scar_where_waiting_might() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 7);
    g.hero.inflict(&c, "crushed-leg");
    g.hero.inflict(&c, "merry");
    let n = g.hero.wounds.iter().position(|(w, _)| w == "crushed-leg").unwrap();
    let m = g.hero.wounds.iter().position(|(w, _)| w == "merry").unwrap();
    assert!(g.cure_price(&c, n).unwrap() >= 10);
    assert!(g.cure_price(&c, m).is_none(), "a boon is not an illness");
    assert!(town_refused(&mut g, TownAct::Cure(m)).contains("nothing"));
    town(&mut g, TownAct::Cure(n));
    assert!(g.hero.wounds.iter().all(|(w, _)| w != "crushed-leg"));
    assert!(g.hero.scars.is_empty());
}

#[test]
fn the_rite_that_follows_a_death_is_a_cure_for_the_weakened_wound() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 7);
    g.hero.inflict(&c, "weakened");
    let before = g.derived(&c).attr(crate::content::Attr::Might);
    let n = g.hero.wounds.iter().position(|(w, _)| w == "weakened").unwrap();
    town(&mut g, TownAct::Cure(n));
    assert!(g.derived(&c).attr(crate::content::Attr::Might) > before);
}

// ---- guild hall ----

#[test]
fn the_guild_drills_a_hero_toward_the_next_level_a_day_or_twice_with_the_annex() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 8);
    g.hero.level = 2;
    g.hero.xp = crate::rules::xp_for_level(2);
    let (xp, coin, turns) = (g.hero.xp, g.hero.coin, g.hero.turns);
    town(&mut g, TownAct::Train);
    assert!(g.hero.xp > xp);
    assert_eq!(g.hero.coin, coin - 16);
    assert_eq!(g.hero.turns, turns - 1);
    assert!(g.hero.wounds.iter().any(|(w, _)| w == "drilled"));
    assert!(town_refused(&mut g, TownAct::Train).contains("enough"));
    // A new yard: two drills a day for everyone.
    finish(&mut g.world, "guild-annex");
    town(&mut g, TownAct::Train);
    assert!(town_refused(&mut g, TownAct::Train).contains("enough"));
    // The hall teaches nothing past its limit.
    let mut h = rich_game("warden", ["quick", "stout"], 8);
    h.hero.level = 8;
    assert!(town_refused(&mut h, TownAct::Train).contains("nothing left"));
    let _ = c;
}

// ---- tavern ----

#[test]
fn a_drink_buys_a_rumour_and_the_third_buys_a_headache() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 9);
    let coin = g.hero.coin;
    let ev = town(&mut g, TownAct::Drink);
    assert!(!ev.is_empty());
    town(&mut g, TownAct::Drink);
    assert!(g.hero.wounds.iter().all(|(w, _)| w != "hangover"));
    town(&mut g, TownAct::Drink);
    assert!(g.hero.wounds.iter().any(|(w, _)| w == "hangover"));
    assert!(g.hero.coin <= coin - 12 + 40, "three drinks cost 12 (rumours may pay a little)");
    assert!(town_refused(&mut g, TownAct::Drink).contains("cut you off"));
    let _ = c;
}

#[test]
fn a_rumour_tip_opens_the_whole_map_of_that_level_once() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 10);
    g.today.tips.push(2);
    g.hero.best_depth = 2;
    // Walk to level 2 via a project lift? Use the engine's own entry: stairs from level 1.
    g.place = Place::Delve { depth: 1, room: 0 };
    let level1 = crate::dungeon::generate(&c, g.season, g.epoch, 1);
    g.place = Place::Delve { depth: 1, room: level1.exit };
    g.today.cleared.push((1, level1.exit));
    let ev = run_stairs(&mut g);
    assert!(ev.contains("whole level is on your map"), "{ev}");
    let level2 = crate::dungeon::generate(&c, g.season, g.epoch, 2);
    assert_eq!(g.explored[1], level2.all_mask());
    assert!(g.today.tips.is_empty(), "a tip is used once");
}

fn run_stairs(g: &mut crate::game::Game) -> String {
    let c = content();
    match g.apply(&c, Action::Stairs) {
        Ok(ev) => super::support::said(&ev),
        Err(e) => e.0,
    }
}

#[test]
fn dice_move_coin_and_borrowing_makes_a_debt_that_grows() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 11);
    g.hero.coin = 3;
    town(&mut g, TownAct::Gamble { stake: 5, cheat: false });
    // Either the 3 coin were refunded with the 5 staked won, or the loss left a debt of 2.
    assert!(g.hero.coin >= 7 || g.hero.debt == 2, "coin {} debt {}", g.hero.coin, g.hero.debt);
    g.hero.debt = 20;
    g.hero.coin = 100;
    assert!(town_refused(&mut g, TownAct::Gamble { stake: 5, cheat: false }).contains("owes"));
    // The debt grows overnight, then the collectors come.
    let _ = g.new_day(&c, TODAY + 1);
    assert_eq!(g.hero.debt, 22);
    g.hero.debt = 60;
    g.hero.coin = 10;
    let _ = g.new_day(&c, TODAY + 2);
    assert_eq!(g.hero.coin, 0);
    assert!(g.hero.wounds.iter().any(|(w, _)| w == "roughed-up"), "{:?}", g.hero.wounds);
    g.hero.coin = 100;
    g.hero.debt = 30;
    town(&mut g, TownAct::Repay);
    assert_eq!((g.hero.coin, g.hero.debt), (70, 0));
}

#[test]
fn only_those_with_nimble_hands_can_load_the_dice_and_caught_cheats_pay_for_it() {
    let mut warden = rich_game("warden", ["quick", "stout"], 12);
    assert!(
        town_refused(&mut warden, TownAct::Gamble { stake: 5, cheat: true }).contains("hands")
    );
    // A cutthroat can; over many nights some get caught, shut out and owe a fine.
    let mut caught = 0;
    let mut won = 0;
    for seed in 0..60 {
        let mut g = rich_game("cutthroat", ["quick", "stout"], seed);
        g.hero.coin = 100;
        town(&mut g, TownAct::Gamble { stake: 20, cheat: true });
        if g.today.shut_out {
            caught += 1;
            assert_eq!(g.hero.debt, 40);
            assert!(g.hero.wounds.iter().any(|(w, _)| w == "roughed-up"));
            assert!(town_refused(&mut g, TownAct::Gamble { stake: 5, cheat: false }).contains("thrown you out"));
        } else if g.hero.coin > 100 {
            won += 1;
        }
    }
    assert!(caught > 0 && won > 0, "caught {caught}, won {won}");
}

#[test]
fn courting_builds_a_bond_that_changes_the_hero_and_only_one_at_a_time() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 13);
    let sense = g.derived(&c).trap_sense;
    // Call on Maren over many days until the first rung.
    let mut day = TODAY;
    for _ in 0..30 {
        let ev = town(&mut g, TownAct::Court("maren".into()));
        let _ = ev;
        if g.hero.bond.as_ref().is_some_and(|b| b.level >= 1) {
            break;
        }
        day += 1;
        let _ = g.new_day(&c, day);
    }
    let bond = g.hero.bond.clone().expect("a bond");
    assert!(bond.level >= 1);
    assert!(g.derived(&c).trap_sense > sense, "the companion gives the hero something");
    // Another day, another call: but not another companion, and not twice in a day.
    assert!(town_refused(&mut g, TownAct::Court("maren".into())).contains("Once a day"));
    day += 1;
    let _ = g.new_day(&c, day);
    assert!(town_refused(&mut g, TownAct::Court("odo".into())).contains("someone else"));
}

#[test]
fn a_neglected_companion_cools_and_a_stranger_moves_on_for_good() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 14);
    g.hero.bond = Some(crate::hero::Bond {
        id: "maren".into(),
        level: 2,
        progress: 1,
        last: TODAY,
    });
    let _ = g.new_day(&c, TODAY + 7);
    let b = g.hero.bond.clone().unwrap();
    assert_eq!((b.level, b.progress), (1, 0));
    // A first acquaintance who is left alone moves on, and does not come back this season.
    let mut h = rich_game("warden", ["quick", "stout"], 14);
    h.hero.bond = Some(crate::hero::Bond {
        id: "odo".into(),
        level: 0,
        progress: 0,
        last: TODAY,
    });
    let _ = h.new_day(&c, TODAY + 7);
    assert!(h.hero.bond.is_none());
    assert!(town_refused(&mut h, TownAct::Court("odo".into())).contains("moved on"));
    // ...but someone else is open to them.
    town(&mut h, TownAct::Court("maren".into()));
    assert!(h.hero.bond.is_some());
}

#[test]
fn town_actions_need_the_hero_to_be_in_town() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 15);
    g.place = Place::Delve { depth: 1, room: 0 };
    let err = g
        .apply(&c, Action::Town(TownAct::Heal))
        .expect_err("not in town");
    assert!(err.0.contains("cannot do that here"));
    let _ = World::new(1);
}
