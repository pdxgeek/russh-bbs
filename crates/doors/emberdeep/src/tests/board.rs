//! The tavern board: a daily draw from data that depends on the build, and contracts that are
//! done below and turned in above.

use super::support::{TODAY, content, game_of, place, rich_game, run, said, town, town_refused};
use crate::content::RoomKind;
use crate::game::{Action, Game, Place};
use crate::hero::Taken;
use crate::town::TownAct;

fn eligible_over_days(calling: &str, traits: [&str; 2]) -> std::collections::BTreeSet<String> {
    let c = content();
    let mut g = game_of(calling, traits, 1);
    g.hero.best_depth = 8;
    let mut seen = std::collections::BTreeSet::new();
    for day in 0..40 {
        let _ = g.new_day(&c, TODAY + 1 + day);
        for p in g.board(&c) {
            seen.insert(p.id.clone());
        }
    }
    seen
}

#[test]
fn the_board_is_three_postings_the_same_for_equal_builds_and_new_each_day() {
    let c = content();
    let mut a = rich_game("warden", ["quick", "stout"], 1);
    let b = rich_game("warden", ["quick", "stout"], 2);
    a.hero.best_depth = 8;
    let mut b = b;
    b.hero.best_depth = 8;
    assert_eq!(a.board(&c).len(), 3);
    let ids = |g: &Game| g.board(&c).iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&a), ids(&b));
    let today = ids(&a);
    let _ = a.new_day(&c, TODAY + 1);
    assert_ne!(ids(&a), today);
}

#[test]
fn two_builds_read_two_boards() {
    let cutthroat = eligible_over_days("cutthroat", ["quick", "stout"]);
    let smith = eligible_over_days("runesmith", ["quick", "stout"]);
    let priest = eligible_over_days("ember-priest", ["quick", "stout"]);
    let warden = eligible_over_days("warden", ["quick", "stout"]);
    assert!(
        cutthroat.contains("quiet-ledger"),
        "only a sneak is offered the quiet work"
    );
    assert!(!warden.contains("quiet-ledger") && !smith.contains("quiet-ledger"));
    assert!(smith.contains("maker-plate") && !warden.contains("maker-plate"));
    assert!(priest.contains("the-locket") && !cutthroat.contains("the-locket"));
}

#[test]
fn the_board_does_not_offer_what_a_novice_cannot_reach() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 3);
    g.hero.best_depth = 0;
    for day in 0..20 {
        let _ = g.new_day(&c, TODAY + 1 + day);
        for p in g.board(&c) {
            assert!(p.depth <= 4, "{} wants level {}", p.name, p.depth);
        }
    }
}

fn take_by_id(g: &mut Game, id: &str) {
    let c = content();
    for day in 0..60 {
        if let Some(n) = g.board(&c).iter().position(|p| p.id == id) {
            town(g, TownAct::Take(n));
            return;
        }
        let _ = g.new_day(&c, TODAY + 1 + day);
    }
    panic!("{id} never on the board");
}

#[test]
fn a_beast_contract_is_done_by_killing_the_beast_deep_enough_and_paid_in_town() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 4);
    g.hero.level = 10;
    g.hero.best_depth = 4;
    take_by_id(&mut g, "old-scorchtail");
    assert!(town_refused(&mut g, TownAct::Take(0)).contains("already carry"));
    assert!(town_refused(&mut g, TownAct::TurnIn).contains("not done"));
    place(&mut g, "sooty-gallery");
    let mut ev = Vec::new();
    // A different beast does nothing.
    g.start_fight(&c, &["cinder-imp".to_owned()], false, &mut ev);
    g.auto_resolve(&c, &mut ev);
    assert!(!g.hero.contract.as_ref().unwrap().done);
    // The named beast's kind, at its depth, does.
    g.start_fight(&c, &["slag-rat".to_owned()], false, &mut ev);
    g.auto_resolve(&c, &mut ev);
    assert!(g.hero.contract.as_ref().unwrap().done, "{}", said(&ev));
    let (coin, xp, done) = (g.hero.coin, g.hero.xp, g.hero.contracts_done);
    g.place = Place::Town;
    town(&mut g, TownAct::TurnIn);
    assert!(g.hero.contract.is_none());
    assert_eq!(g.hero.contracts_done, done + 1);
    assert!(g.hero.coin >= coin + 30 && g.hero.xp > xp);
    assert!(
        g.world
            .news
            .iter()
            .any(|n| n.contains("finished the posting"))
    );
}

#[test]
fn a_beast_too_shallow_does_not_count() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 5);
    g.hero.best_depth = 8;
    g.hero.contract = Some(Taken {
        id: "blister-brood".into(),
        day: g.day,
        done: false,
    });
    place(&mut g, "sooty-gallery");
    let mut ev = Vec::new();
    g.contract_beast(&c, &["slag-crawler".to_owned()], 3, &mut ev);
    assert!(
        !g.hero.contract.as_ref().unwrap().done,
        "level 3 is too shallow"
    );
    g.contract_beast(&c, &["slag-crawler".to_owned()], 6, &mut ev);
    assert!(g.hero.contract.as_ref().unwrap().done);
}

#[test]
fn an_heirloom_is_done_by_reaching_its_kind_of_room_and_a_rescue_slows_the_climb() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 6);
    g.hero.contract = Some(Taken {
        id: "lost-lamplighter".into(),
        day: g.day,
        done: false,
    });
    let mut ev = Vec::new();
    g.contract_room(&c, RoomKind::Shrine, 4, &mut ev);
    assert!(
        !g.hero.contract.as_ref().unwrap().done,
        "a shrine is not a corridor"
    );
    g.place = Place::Delve { depth: 3, room: 0 };
    let before = g.ascent_cost(&c);
    g.contract_room(&c, RoomKind::Corridor, 1, &mut ev);
    assert!(
        !g.hero.contract.as_ref().unwrap().done,
        "level 1 is too shallow"
    );
    g.contract_room(&c, RoomKind::Corridor, 3, &mut ev);
    assert!(g.hero.contract.as_ref().unwrap().done);
    assert_eq!(
        g.ascent_cost(&c),
        before + 1,
        "the soul you carry slows you by a turn"
    );
    // An heirloom does not.
    let mut h = rich_game("warden", ["quick", "stout"], 6);
    h.place = Place::Delve { depth: 3, room: 0 };
    let base = h.ascent_cost(&c);
    h.hero.contract = Some(Taken {
        id: "pell-strongbox".into(),
        day: h.day,
        done: true,
    });
    assert_eq!(h.ascent_cost(&c), base);
}

#[test]
fn a_posting_lapses_and_can_be_dropped() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 7);
    g.hero.contract = Some(Taken {
        id: "old-scorchtail".into(),
        day: g.day,
        done: false,
    });
    let ev = g.new_day(&c, TODAY + 3);
    assert!(g.hero.contract.is_none(), "{}", said(&ev));
    assert!(said(&ev).contains("taken down"));
    g.hero.contract = Some(Taken {
        id: "old-scorchtail".into(),
        day: g.day,
        done: false,
    });
    town(&mut g, TownAct::Drop);
    assert!(g.hero.contract.is_none());
}

#[test]
fn a_contract_can_teach_a_recipe_and_give_a_core() {
    // The shipped table pays coin and xp; the engine also pays recipes and cores when a
    // posting says so, proved with a table that does.
    let mut c = (*content()).clone();
    let posting = c
        .town
        .contract
        .iter_mut()
        .find(|p| p.id == "pell-strongbox")
        .unwrap();
    posting.recipe = Some("found-forge-axe".into());
    posting.core = Some("cinder-core".into());
    let mut g = rich_game("warden", ["quick", "stout"], 8);
    g.hero.contract = Some(Taken {
        id: "pell-strongbox".into(),
        day: g.day,
        done: true,
    });
    g.apply(&c, Action::Town(TownAct::TurnIn))
        .expect("turns in");
    assert!(g.hero.recipes.contains(&"found-forge-axe".to_owned()));
    assert_eq!(g.hero.cores, vec!["cinder-core".to_owned()]);
    let _ = run;
}
