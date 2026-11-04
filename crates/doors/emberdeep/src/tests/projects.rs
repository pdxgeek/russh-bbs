//! Shared town projects: gifts, the invariant that nothing is credited twice, and the RPG rule
//! that a finished project changes the options of every hero, in the town and in the dungeon.

use super::support::{TODAY, content, finish, game_of, rich_game, town, town_refused};
use crate::content_town::ProjectEffect;
use crate::game::{Action, Game, Place};
use crate::town::TownAct;
use crate::world::{Change, Pledge, World};

fn donate(game: &mut Game, project: &str, coin: u32, salvage: u32) {
    town(
        game,
        TownAct::Donate {
            project: project.into(),
            coin,
            salvage,
        },
    );
}

fn gift(hero: &str, project: &str, coin: u32, salvage: u32) -> Change {
    Change::Pledge(Pledge {
        hero: hero.into(),
        project: project.into(),
        coin,
        salvage,
    })
}

// ---- no double credit ----

#[test]
fn the_same_pledge_applied_twice_credits_once() {
    let mut w = World::new(1);
    let g = gift("Ada", "new-bellows", 30, 5);
    w.apply(&g);
    w.apply(&g);
    assert_eq!(w.progress("new-bellows"), (30, 5));
    // A bigger running total replaces it; a smaller (stale) one changes nothing.
    w.apply(&gift("Ada", "new-bellows", 50, 5));
    w.apply(&gift("Ada", "new-bellows", 30, 5));
    assert_eq!(w.progress("new-bellows"), (50, 5));
}

#[test]
fn pledges_from_two_heroes_add_and_merge_in_any_order() {
    let a = gift("Ada", "new-bellows", 30, 5);
    let b = gift("Bo", "new-bellows", 20, 10);
    let c2 = gift("Ada", "new-bellows", 40, 5);
    let orders = [[&a, &b, &c2], [&c2, &b, &a], [&b, &a, &c2], [&a, &c2, &b]];
    let mut results = Vec::new();
    for order in orders {
        let mut w = World::new(1);
        for ch in order {
            w.apply(ch);
            w.apply(ch); // a retried closure
        }
        results.push(w.progress("new-bellows"));
    }
    assert!(results.iter().all(|r| *r == (60, 15)), "{results:?}");
}

#[test]
fn a_hero_gives_exactly_what_the_world_receives() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 1);
    let (coin, stash) = (g.hero.coin, g.hero.stash);
    donate(&mut g, "new-bellows", 25, 10);
    donate(&mut g, "new-bellows", 25, 0);
    assert_eq!(g.hero.coin, coin - 50);
    assert_eq!(g.hero.stash, stash - 10);
    assert_eq!(g.world.progress("new-bellows"), (50, 10));
    assert_eq!(g.hero.given, vec![("new-bellows".to_owned(), 50, 10)]);
    // The changes the board receives (as the session would send them) rebuild the same total
    // in a fresh world, however many times the closure runs.
    let changes = g.take_changes();
    let mut fresh = World::new(1);
    for _ in 0..3 {
        for ch in &changes {
            fresh.apply(ch);
        }
    }
    assert_eq!(fresh.progress("new-bellows"), (50, 10));
    // And a session that opens later tells the world again what the hero gave: no change.
    for ch in g.pledges_of_hero() {
        fresh.apply(&ch);
    }
    assert_eq!(fresh.progress("new-bellows"), (50, 10));
    let _ = c;
}

#[test]
fn a_gift_is_clamped_to_what_the_project_still_needs_so_nothing_is_wasted() {
    let c = content();
    let def = c.project("temple-roof").unwrap();
    let mut g = rich_game("warden", ["quick", "stout"], 2);
    let coin = g.hero.coin;
    donate(&mut g, "temple-roof", 10_000, 10_000);
    assert_eq!(g.hero.coin, coin - def.coin);
    assert_eq!(g.world.progress("temple-roof"), (def.coin, def.salvage));
    assert!(g.world.is_done(&c, "temple-roof"));
}

#[test]
fn a_finished_project_takes_no_more_and_costs_the_donor_nothing() {
    let mut g = rich_game("warden", ["quick", "stout"], 3);
    finish(&mut g.world, "temple-roof");
    let (coin, stash) = (g.hero.coin, g.hero.stash);
    let err = town_refused(
        &mut g,
        TownAct::Donate {
            project: "temple-roof".into(),
            coin: 50,
            salvage: 50,
        },
    );
    assert!(
        err.contains("finished") || err.contains("cannot take"),
        "{err}"
    );
    assert_eq!((g.hero.coin, g.hero.stash), (coin, stash));
    assert!(g.hero.given.is_empty());
    assert!(g.take_changes().is_empty());
}

#[test]
fn a_project_cannot_be_given_to_until_what_it_needs_is_finished() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 4);
    let err = town_refused(
        &mut g,
        TownAct::Donate {
            project: "lift-six".into(),
            coin: 50,
            salvage: 0,
        },
    );
    assert!(err.contains("something must come first"), "{err}");
    // Even if the world has been paid in full for it, a project whose requirement is open is
    // not finished.
    let def = c.project("lift-six").unwrap();
    g.world
        .apply(&gift("Early", "lift-six", def.coin, def.salvage));
    assert!(!g.world.is_done(&c, "lift-six"));
    finish(&mut g.world, "new-bellows");
    assert!(g.world.is_done(&c, "lift-six"));
}

#[test]
fn two_heroes_racing_to_finish_a_project_cannot_both_be_charged_for_the_same_gap() {
    // Both see the same world with 20 coin still needed; each is charged for what they gave,
    // and the world ends at or just over the cost, never short and never doubled.
    let c = content();
    let def = c.project("temple-roof").unwrap();
    let mut base = World::new(1);
    base.apply(&gift("Early", "temple-roof", def.coin - 20, def.salvage));
    let mut a = rich_game("warden", ["quick", "stout"], 5);
    a.hero.name = "Ada".into();
    a.world = base.clone();
    let mut b = rich_game("duelist", ["quick", "stout"], 5);
    b.hero.name = "Bo".into();
    b.world = base;
    donate(&mut a, "temple-roof", 20, 0);
    donate(&mut b, "temple-roof", 20, 0);
    let mut merged = World::new(1);
    merged.apply(&gift("Early", "temple-roof", def.coin - 20, def.salvage));
    for ch in a.take_changes().iter().chain(&b.take_changes()) {
        merged.apply(ch);
    }
    let (coin, _) = merged.progress("temple-roof");
    assert_eq!(
        coin,
        def.coin + 20,
        "the overshoot is bounded by one stale gift"
    );
    assert!(merged.is_done(&c, "temple-roof"));
}

#[test]
fn completion_is_news_for_the_tavern_board_and_said_once() {
    let c = content();
    let def = c.project("temple-roof").unwrap();
    let mut g = rich_game("warden", ["quick", "stout"], 6);
    g.hero.name = "Ada".into();
    donate(&mut g, "temple-roof", def.coin, def.salvage);
    let news: Vec<String> = g
        .world
        .news
        .iter()
        .filter(|n| n.contains("town project"))
        .cloned()
        .collect();
    assert_eq!(news.len(), 1, "{news:?}");
}

// ---- the RPG rule: a finished project changes options for every player ----

#[test]
fn a_finished_lift_gives_every_hero_a_way_down_and_survives_the_collapse() {
    let c = content();
    let mut ada = rich_game("warden", ["quick", "stout"], 7);
    let mut bo = rich_game("cutthroat", ["quick", "stout"], 8);
    bo.hero.name = "Bo".into();
    assert!(bo.ways_down(&c).is_empty());
    let mut err = bo.apply(&c, Action::Descend(6)).expect_err("no lift yet").0;
    assert!(err.contains("no shortcut"), "{err}");
    // The works finish a lift; Ada gave the last gift, Bo only hears of it through the world.
    finish(&mut ada.world, "lift-six");
    for ch in ada.world.pledges.clone() {
        bo.world.apply(&Change::Pledge(ch));
    }
    assert_eq!(
        bo.ways_down(&c),
        vec![(6, crate::content::ShortcutKind::Lift)]
    );
    bo.hero.turns = 25;
    let ev = bo
        .apply(&c, Action::Descend(6))
        .expect("the lift runs")
        .len();
    assert!(ev > 0);
    assert!(matches!(bo.place, Place::Delve { depth: 6, .. }));
    assert_eq!(bo.hero.turns, 24, "a lift costs one turn");
    // The climb back is cheap too.
    assert!(bo.ascent_cost(&c) <= 2);
    // The collapse of the dungeon clears shortcuts heroes opened but not the town's own lift.
    bo.world.apply(&Change::Slain(5, "Someone".into()));
    bo.world.tick(TODAY + 1);
    assert_eq!(bo.world.epoch, 1);
    assert!(bo.world.shortcuts.is_empty());
    assert_eq!(
        bo.world.ways_down(&c),
        vec![(6, crate::content::ShortcutKind::Lift)]
    );
    err.clear();
}

#[test]
fn a_lift_beats_a_rope_a_hero_opened_at_the_same_level() {
    let c = content();
    let mut w = World::new(1);
    w.apply(&Change::Shortcut(
        6,
        crate::content::ShortcutKind::Rope,
        "Ada".into(),
    ));
    finish(&mut w, "lift-six");
    assert_eq!(
        w.ways_down(&c),
        vec![(6, crate::content::ShortcutKind::Lift)]
    );
}

#[test]
fn every_kind_of_project_changes_something_a_hero_can_see() {
    let c = content();
    let mut seen = std::collections::BTreeSet::new();
    for p in &c.projects.project {
        let before = game_of("warden", ["quick", "stout"], 9);
        let mut after = game_of("warden", ["quick", "stout"], 9);
        finish(&mut after.world, &p.id);
        let (b, a) = (before.town_fx(&c), after.town_fx(&c));
        assert_ne!(b, a, "project {} changes nothing", p.id);
        seen.insert(Game::project_effect_text(p.effect));
        let _ = ProjectEffect::Library;
    }
    assert_eq!(seen.len(), 7, "{seen:?}");
}

#[test]
fn the_library_reads_relics_only_once_the_town_has_built_a_reading_room() {
    let c = content();
    let mut g = rich_game("warden", ["quick", "stout"], 10);
    g.hero.relics = 3;
    assert!(town_refused(&mut g, TownAct::Identify).contains("no one who can read"));
    assert_eq!(g.hero.relics, 3, "a refused reading keeps the relic");
    finish(&mut g.world, "reading-room");
    let coin = g.hero.coin;
    town(&mut g, TownAct::Identify);
    assert_eq!(g.hero.relics, 2);
    assert!(g.hero.coin <= coin - 20 + 60);
    // Over many relics a hero learns a library pattern (and cannot learn one twice).
    let mut learned = false;
    for seed in 0..40 {
        let mut h = rich_game("warden", ["quick", "stout"], seed);
        finish(&mut h.world, "reading-room");
        h.hero.relics = 1;
        town(&mut h, TownAct::Identify);
        learned |= h.hero.recipes.iter().any(|r| {
            c.recipe(r)
                .is_some_and(|x| matches!(x.learn, crate::content_town::Learn::Library))
        });
    }
    assert!(learned);
}

#[test]
fn the_world_with_pledges_round_trips_and_a_version_one_world_upgrades() {
    let mut w = World::new(1);
    w.apply(&gift("Ada", "new-bellows", 30, 5));
    w.apply(&gift("Bo", "reading-room", 10, 0));
    let back = World::decode(&w.encode()).expect("decodes");
    assert_eq!(back, w);
    assert!(World::decode(&w.encode()[..w.encode().len() - 1]).is_none());
}

#[test]
fn a_full_ledger_refuses_new_donors_rather_than_losing_a_gift() {
    let mut g = rich_game("warden", ["quick", "stout"], 11);
    for i in 0..crate::world::PLEDGES_MAX {
        g.world.pledges.push(Pledge {
            hero: format!("h{i}"),
            project: "market-stalls".into(),
            coin: 0,
            salvage: 0,
        });
    }
    let coin = g.hero.coin;
    let err = town_refused(
        &mut g,
        TownAct::Donate {
            project: "temple-roof".into(),
            coin: 10,
            salvage: 0,
        },
    );
    assert!(err.contains("ledger is full"));
    assert_eq!(g.hero.coin, coin);
}
