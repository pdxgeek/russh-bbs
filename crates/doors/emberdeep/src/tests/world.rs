//! The shared world: idempotent changes, the nightly tick, the collapse and the seasons.

use super::support::{TODAY, content, game_of, place};
use crate::content::ShortcutKind;
use crate::game::Place;
use crate::world::{Change, Fallen, World};

fn fallen(name: &str, day: i64) -> Fallen {
    Fallen {
        name: name.into(),
        depth: 3,
        room: 2,
        day,
        rescued_by: None,
    }
}

#[test]
fn changes_are_idempotent_so_a_retried_edit_cannot_double_up() {
    let changes = [
        Change::Shortcut(3, ShortcutKind::Lift, "A".into()),
        Change::Slain(1, "A".into()),
        Change::Depth("A".into(), 6),
        Change::Fall(fallen("B", 100)),
        Change::Rescue("B".into(), "A".into()),
    ];
    let mut once = World::new(1);
    let mut twice = World::new(1);
    for c in &changes {
        once.apply(c);
        twice.apply(c);
        twice.apply(c);
    }
    assert_eq!(once, twice);
    assert_eq!(once.shortcuts.len(), 1);
    assert_eq!(once.fallen[0].rescued_by.as_deref(), Some("A"));
}

#[test]
fn changes_from_two_heroes_merge_in_any_order() {
    let a = Change::Shortcut(3, ShortcutKind::Lift, "A".into());
    let b = Change::Slain(1, "B".into());
    let c = Change::Depth("A".into(), 4);
    let d = Change::Depth("B".into(), 7);
    let mut x = World::new(1);
    let mut y = World::new(1);
    for ch in [&a, &b, &c, &d] {
        x.apply(ch);
    }
    for ch in [&d, &c, &b, &a] {
        y.apply(ch);
    }
    assert_eq!(x.shortcuts, y.shortcuts);
    assert_eq!(x.slain, y.slain);
    assert_eq!(x.deepest, y.deepest);
    assert_eq!(
        x.deepest[0],
        ("B".to_owned(), 7),
        "the hall is deepest first"
    );
}

#[test]
fn the_hall_keeps_the_deepest_eight_and_news_the_last_six() {
    let mut w = World::new(1);
    for i in 0..20u8 {
        w.apply(&Change::Depth(format!("H{i}"), i + 1));
        w.add_news(format!("line {i}"));
    }
    assert_eq!(w.deepest.len(), 8);
    assert_eq!(w.deepest[0].1, 20);
    assert_eq!(w.news.len(), 6);
    assert_eq!(w.news.last().map(String::as_str), Some("line 19"));
}

#[test]
fn the_nightly_tick_forgets_old_bodies_but_remembers_rescues_longer() {
    let mut w = World::new(1);
    w.apply(&Change::Fall(fallen("Lost", 100)));
    w.apply(&Change::Fall(fallen("Saved", 100)));
    w.apply(&Change::Rescue("Saved".into(), "Hero".into()));
    w.tick(102);
    assert_eq!(w.fallen.len(), 2);
    w.tick(105);
    assert_eq!(
        w.fallen.len(),
        1,
        "the unrescued one is gone after the window"
    );
    assert_eq!(w.fallen[0].name, "Saved");
    w.tick(120);
    assert!(w.fallen.is_empty());
    assert_eq!(w.day, 120);
}

#[test]
fn felling_the_cold_furnace_collapses_the_dungeon_at_the_next_tick() {
    let c = content();
    let mut w = World::new(1);
    w.apply(&Change::Shortcut(3, ShortcutKind::Lift, "A".into()));
    w.apply(&Change::Slain(1, "A".into()));
    w.apply(&Change::Slain(5, "Furnace-Breaker".into()));
    assert_eq!(w.finale.as_deref(), Some("Furnace-Breaker"));
    assert_eq!(w.epoch, 0, "the dungeon stands until the night");
    let before = crate::dungeon::generate(&c, 1, w.epoch, 3);
    w.tick(TODAY + 1);
    assert_eq!(w.epoch, 1);
    assert!(w.shortcuts.is_empty() && w.slain.is_empty() && w.finale.is_none());
    assert!(w.news.last().unwrap().contains("Cold Furnace"));
    assert_ne!(
        before,
        crate::dungeon::generate(&c, 1, w.epoch, 3),
        "a new layout"
    );
}

#[test]
fn a_hero_below_when_the_dungeon_collapses_is_thrown_out_with_what_they_carry() {
    let c = content();
    let mut g = game_of("warden", ["quick", "stout"], 4);
    place(&mut g, "sooty-gallery");
    g.hero.coin = 55;
    g.explored[0] = 0b111;
    g.world.epoch = 1;
    let ev = super::support::said(&g.new_day(&c, TODAY + 1));
    assert_eq!(g.place, Place::Town, "{ev}");
    assert_eq!(g.epoch, 1);
    assert_eq!(g.hero.coin, 55);
    assert!(g.explored.iter().all(|m| *m == 0), "the old map is useless");
}

#[test]
fn the_world_blob_round_trips_and_rejects_damage() {
    let mut w = World::new(3);
    w.apply(&Change::Shortcut(3, ShortcutKind::Bridge, "A".into()));
    w.apply(&Change::Slain(2, "B".into()));
    w.apply(&Change::Depth("A".into(), 9));
    w.apply(&Change::Fall(fallen("C", 50)));
    w.apply(&Change::Rescue("C".into(), "A".into()));
    w.tick(51);
    let bytes = w.encode();
    assert_eq!(World::decode(&bytes), Some(w.clone()));
    assert!(World::decode(&bytes[..bytes.len() - 1]).is_none());
    let mut extra = bytes.clone();
    extra.push(9);
    assert!(World::decode(&extra).is_none());
    assert!(World::decode(&[]).is_none());
    assert!(bytes.len() < 2_000);
}

#[test]
fn another_season_starts_a_clean_world() {
    let mut w = World::new(1);
    w.apply(&Change::Shortcut(3, ShortcutKind::Lift, "A".into()));
    let bytes = w.encode();
    assert_eq!(World::for_season(Some(&bytes), 1), w);
    assert_eq!(World::for_season(Some(&bytes), 2), World::new(2));
    assert_eq!(World::for_season(None, 4), World::new(4));
    assert_eq!(World::for_season(Some(&[1, 2]), 4), World::new(4));
}
