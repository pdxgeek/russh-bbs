//! Property tests: map generation and the tide are pure functions of their seed and keep their
//! promises for any seed.

#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use russh_bbs_door_saltmarch::content::Catalog;
use russh_bbs_door_saltmarch::map::{
    DEEP_ROWS, MAX_SITES, MIN_SITES, Map, SITE_SPACING, SIZE, TIDECOURT, TileKind,
};
use russh_bbs_door_saltmarch::tide::{Access, MID, NEAP_AMP, SPRING_AMP, STORM_SURGE, Tide};

fn catalog() -> Catalog {
    Catalog::embedded().unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn the_same_seed_always_gives_the_same_map(seed in any::<u64>()) {
        let cat = catalog();
        prop_assert_eq!(Map::generate(seed, &cat.pois), Map::generate(seed, &cat.pois));
    }

    #[test]
    fn different_seeds_give_different_marshes(a in any::<u64>(), b in any::<u64>()) {
        prop_assume!(a != b);
        let cat = catalog();
        prop_assert_ne!(Map::generate(a, &cat.pois), Map::generate(b, &cat.pois));
    }

    #[test]
    fn every_map_is_well_formed(seed in any::<u64>()) {
        let cat = catalog();
        let map = Map::generate(seed, &cat.pois);
        for ((x, y), tile) in map.iter() {
            prop_assert!(tile.elevation <= 100);
            // The Deep is the far edge and nothing else is the Deep.
            // (A wreck painted by a Deep template replaces a Deep tile, and only there.)
            if y < DEEP_ROWS {
                prop_assert!(matches!(tile.kind, TileKind::Deep | TileKind::Wreck), "({}, {})", x, y);
            } else {
                prop_assert_ne!(tile.kind, TileKind::Deep, "({}, {})", x, y);
            }
            // Causeways are built by players, never generated.
            prop_assert_ne!(tile.kind, TileKind::Causeway);
        }
        let court = map.tile(usize::from(TIDECOURT.0), usize::from(TIDECOURT.1)).unwrap();
        prop_assert_eq!(court.kind, TileKind::Island);
        // The share of each kind is steady across seeds: ranks are mapped through a fixed curve.
        let census = map.census();
        let total = SIZE * SIZE;
        let share = |k: TileKind| census[TileKind::ALL.iter().position(|x| *x == k).unwrap()] * 100 / total;
        prop_assert!((5..=20).contains(&share(TileKind::Island)), "islands {}", share(TileKind::Island));
        prop_assert!(share(TileKind::Mudflat) >= 12, "mud {}", share(TileKind::Mudflat));
        prop_assert!((7..=9).contains(&share(TileKind::Deep)), "deep {}", share(TileKind::Deep));
        for kind in [TileKind::Mere, TileKind::Channel, TileKind::SaltPan, TileKind::Reedbed] {
            prop_assert!(census[TileKind::ALL.iter().position(|x| *x == kind).unwrap()] > 0, "{kind:?}");
        }
    }

    #[test]
    fn hold_sites_are_spaced_on_high_ground_and_all_reachable_on_foot_at_a_neap_low(seed in any::<u64>()) {
        let cat = catalog();
        let map = Map::generate(seed, &cat.pois);
        prop_assert!((MIN_SITES..=MAX_SITES).contains(&map.sites.len()), "{}", map.sites.len());
        for (i, a) in map.sites.iter().enumerate() {
            prop_assert!(usize::from(a.1) >= DEEP_ROWS + 8);
            prop_assert!(map.tile(usize::from(a.0), usize::from(a.1)).unwrap().elevation >= 62);
            for b in &map.sites[i + 1..] {
                let d = (i32::from(a.0) - i32::from(b.0)).abs().max((i32::from(a.1) - i32::from(b.1)).abs());
                prop_assert!(d >= SITE_SPACING);
            }
        }
        // The lowest low of the weakest tide is the water at its highest low: MID - NEAP_AMP.
        let neap_low = MID - NEAP_AMP;
        let reach = map.reachable_on_foot(TIDECOURT, neap_low);
        for site in &map.sites {
            prop_assert!(reach.contains(&(usize::from(site.0), usize::from(site.1))), "{:?}", site);
        }
    }

    #[test]
    fn points_of_interest_stand_where_their_templates_say(seed in any::<u64>()) {
        let cat = catalog();
        let map = Map::generate(seed, &cat.pois);
        prop_assert!(map.pois.len() >= 20, "{}", map.pois.len());
        for (i, p) in map.pois.iter().enumerate() {
            let template = &cat.pois[usize::from(p.template)];
            prop_assert_eq!(template.kind, p.kind);
            let tile = map.tile(usize::from(p.x), usize::from(p.y)).unwrap();
            // A painted tile has become the painted kind; others stand on one of the allowed ones.
            match template.paints {
                Some(kind) => prop_assert_eq!(tile.kind, kind),
                None => prop_assert!(template.tiles.contains(&tile.kind), "{} on {:?}", template.id, tile.kind),
            }
            prop_assert_eq!(usize::from(p.y) < DEEP_ROWS, template.tiles == [TileKind::Deep]);
            for q in &map.pois[i + 1..] {
                let d = (i32::from(p.x) - i32::from(q.x)).abs().max((i32::from(p.y) - i32::from(q.y)).abs());
                prop_assert!(d >= 3);
            }
            // No point of interest sits on a hold site or the Tidecourt.
            prop_assert!(!map.sites.contains(&(p.x, p.y)) && (p.x, p.y) != TIDECOURT);
            prop_assert!(map.poi_at(p.x, p.y).is_some());
        }
        // Painted places respect the tide: a drowned town is dry only at a spring low.
        for ((_, _), tile) in map.iter().filter(|(_, t)| t.kind == TileKind::DrownedTown) {
            prop_assert!((14..=28).contains(&tile.elevation));
            prop_assert_eq!(Access::of(i32::from(tile.elevation), MID - SPRING_AMP), Access::Dry);
            prop_assert_eq!(Access::of(i32::from(tile.elevation), MID), Access::Drowned);
        }
    }

    #[test]
    fn step_costs_follow_the_water(seed in any::<u64>(), level in 0i32..110) {
        let cat = catalog();
        let map = Map::generate(seed, &cat.pois);
        for ((x, y), tile) in map.iter().step_by(37) {
            let on_foot = map.step_cost(x, y, level, false, 0);
            let by_boat = map.step_cost(x, y, level, true, 0);
            match Access::of(i32::from(tile.elevation), level) {
                Access::Dry => prop_assert_eq!(on_foot, Some(tile.kind.base_cost())),
                Access::Wet => prop_assert_eq!(on_foot, Some(tile.kind.base_cost() + 1)),
                Access::Drowned => {
                    prop_assert_eq!(on_foot, None);
                    prop_assert_eq!(by_boat, Some(tile.kind.base_cost() + 1));
                }
            }
            // A sea-born party's discount never goes below the dry cost.
            prop_assert!(map.step_cost(x, y, level, true, 9).is_none_or(|c| c >= tile.kind.base_cost()));
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn the_tide_is_a_pure_function_of_seed_and_day(seed in any::<u64>(), day in 0i64..60_000) {
        let a = Tide::new(seed);
        let b = Tide::new(seed);
        prop_assert_eq!(a.table(day), b.table(day));
        prop_assert_eq!(a.level(day * 1440 + 700), b.level(day * 1440 + 700));
        prop_assert_eq!(a.storm(day), b.storm(day));
    }

    #[test]
    fn the_water_moves_smoothly_and_stays_in_bounds(seed in any::<u64>(), day in 0i64..60_000) {
        let tide = Tide::new(seed);
        let start = day * 1440;
        let mut last = tide.level(start);
        for m in 1..=1440 {
            let l = tide.level(start + m);
            prop_assert!((l - last).abs() <= 3, "jump {} -> {} at minute {}", last, l, m);
            prop_assert!((MID - SPRING_AMP..=MID + SPRING_AMP + STORM_SURGE).contains(&l));
            last = l;
        }
    }

    #[test]
    fn every_days_table_lists_alternating_turns_that_match_the_curve(seed in any::<u64>(), day in 0i64..60_000) {
        let tide = Tide::new(seed);
        let table = tide.table(day);
        prop_assert!((3..=4).contains(&table.extremes.len()));
        for pair in table.extremes.windows(2) {
            prop_assert!(pair[0].high != pair[1].high && pair[0].minute < pair[1].minute);
            // A tidal half-cycle is a little over six hours.
            prop_assert_eq!(i32::from(pair[1].minute) - i32::from(pair[0].minute), 372);
        }
        for e in &table.extremes {
            prop_assert_eq!(tide.level(day * 1440 + i64::from(e.minute)), e.level);
            // Highs are above the mean and lows below it, whatever the phase.
            prop_assert_eq!(e.level > MID, e.high);
        }
        let amp = Tide::amplitude(day);
        prop_assert!((NEAP_AMP..=SPRING_AMP).contains(&amp));
    }

    #[test]
    fn the_next_turn_is_always_within_half_a_cycle(seed in any::<u64>(), minute in 0i64..100_000_000) {
        let tide = Tide::new(seed);
        let (wait, high) = tide.next_turn(minute);
        prop_assert!((1..=372).contains(&wait));
        prop_assert_eq!(tide.level(minute + wait) > MID, high);
    }
}
