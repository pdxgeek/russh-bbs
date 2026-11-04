//! Property tests for frontier generation: every seed gives a connected, well-formed map with
//! the economies, shipyards and starting neighbourhood the game depends on.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::collections::HashSet;

use proptest::prelude::*;
use russh_bbs_door_long_haul::content::{Content, GoodClass};
use russh_bbs_door_long_haul::frontier::{
    Frontier, HOME_CHARTED_HOPS, MAP_H, MAP_W, Role, SYSTEM_COUNT,
};

fn content() -> Content {
    Content::embedded().unwrap()
}

fn check(frontier: &Frontier, content: &Content) {
    assert_eq!(frontier.systems.len(), SYSTEM_COUNT);
    // Positions in bounds, names unique.
    let names: HashSet<&str> = frontier.systems.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names.len(), SYSTEM_COUNT, "system names are unique");
    for s in &frontier.systems {
        assert!(u32::from(s.x) < MAP_W && u32::from(s.y) < MAP_H);
        assert!((1..=10).contains(&s.richness));
        assert_eq!(
            usize::from(s.id),
            frontier.systems.iter().position(|o| o.id == s.id).unwrap()
        );
    }
    // Lanes: no self-loops, no duplicates, symmetric adjacency, lengths match the map.
    let mut seen = HashSet::new();
    for l in &frontier.lanes {
        assert_ne!(l.a, l.b);
        assert!(l.a < l.b, "lanes are stored once, low end first");
        assert!(seen.insert((l.a, l.b)), "duplicate lane");
        assert!(l.dist >= 1);
        assert_eq!(frontier.lane(l.a, l.b), Some(l.dist));
        assert_eq!(frontier.lane(l.b, l.a), Some(l.dist));
        let straight = frontier.distance(l.a, l.b);
        assert!(u32::from(l.dist).abs_diff(straight) <= 1);
    }
    // Connected.
    let hops = frontier.hops_from(0);
    assert!(
        hops.iter().all(|&h| h < 255),
        "the frontier is one connected map"
    );
    // Stations.
    let names: HashSet<&str> = frontier.stations.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names.len(),
        frontier.stations.len(),
        "station names are unique"
    );
    for (i, st) in frontier.stations.iter().enumerate() {
        assert_eq!(usize::from(st.id), i);
        assert!((1..=3).contains(&st.size));
        assert!(
            frontier.systems[usize::from(st.system)]
                .stations
                .contains(&st.id)
        );
        let goods: HashSet<u8> = st.goods.iter().map(|g| g.good).collect();
        assert_eq!(
            goods.len(),
            st.goods.len(),
            "a good appears once per station"
        );
        assert!(
            st.goods.len() >= 9 && st.goods.len() <= 16,
            "{} goods",
            st.goods.len()
        );
        assert!(st.goods.iter().any(|g| g.role == Role::Produces));
        assert!(st.goods.iter().any(|g| g.role == Role::Needs));
        let eco = &content.economies[usize::from(st.economy)];
        for g in &st.goods {
            let def = &content.goods[usize::from(g.good)];
            if def.class == GoodClass::Contraband && g.role == Role::Produces {
                assert_eq!(eco.id, "haven", "only havens sell contraband");
            }
            if def.class == GoodClass::Contraband {
                assert_ne!(eco.id, "military", "military posts never trade contraband");
            }
        }
        assert!(!st.shipyard || eco.id != "haven");
    }
    // Every economy is present, at least eight shipyards, every legal good is made somewhere.
    for (i, e) in content.economies.iter().enumerate() {
        assert!(
            frontier
                .stations
                .iter()
                .any(|s| usize::from(s.economy) == i),
            "no {} station",
            e.id
        );
    }
    assert!(frontier.stations.iter().filter(|s| s.shipyard).count() >= 8);
    for (gi, g) in content.goods.iter().enumerate() {
        let made = frontier.stations.iter().any(|s| {
            s.goods
                .iter()
                .any(|m| usize::from(m.good) == gi && m.role == Role::Produces)
        });
        assert!(
            made || g.class == GoodClass::Contraband,
            "{} is made nowhere",
            g.id
        );
    }
    // Home: a shipyard with repairs, charted neighbourhood, somewhere to trade within a few hops.
    let home = frontier.station(frontier.home_station).unwrap();
    assert!(home.shipyard && home.repair && home.size >= 2);
    let hops = frontier.hops_from(home.system);
    for s in &frontier.systems {
        if hops[usize::from(s.id)] <= HOME_CHARTED_HOPS {
            assert!(!s.uncharted, "the home ring starts charted");
        }
    }
    let near = frontier
        .stations
        .iter()
        .filter(|s| hops[usize::from(s.system)] <= 3)
        .count();
    assert!(near >= 6, "only {near} stations within three hops of home");
    let uncharted = frontier.systems.iter().filter(|s| s.uncharted).count();
    assert!(
        (8..=60).contains(&uncharted),
        "{uncharted} uncharted systems"
    );
}

#[test]
fn the_frontier_for_a_few_seasons_is_well_formed() {
    let content = content();
    for season in 1..=40 {
        check(&Frontier::generate(&content, season), &content);
    }
}

#[test]
fn generation_is_deterministic_and_seasons_differ() {
    let content = content();
    let a = Frontier::generate(&content, 7);
    let b = Frontier::generate(&content, 7);
    let c = Frontier::generate(&content, 8);
    assert_eq!(a.lanes, b.lanes);
    assert_eq!(a.home_station, b.home_station);
    for (x, y) in a.stations.iter().zip(&b.stations) {
        assert_eq!(
            (&x.name, x.economy, x.size, &x.goods),
            (&y.name, y.economy, y.size, &y.goods)
        );
    }
    assert_ne!(
        a.systems.iter().map(|s| (s.x, s.y)).collect::<Vec<_>>(),
        c.systems.iter().map(|s| (s.x, s.y)).collect::<Vec<_>>()
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn any_season_gives_a_well_formed_frontier(season in 0u32..u32::MAX) {
        let content = content();
        check(&Frontier::generate(&content, season), &content);
    }
}
