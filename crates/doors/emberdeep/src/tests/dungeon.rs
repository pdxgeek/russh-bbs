//! The seasonal shared dungeon: deterministic, connected, shaped as the design says.

use super::support::content;
use crate::content::RoomKind;
use crate::dungeon::{self, distances, has_guardian};

#[test]
fn a_level_is_a_pure_function_of_season_epoch_and_depth() {
    let c = content();
    let a = dungeon::generate(&c, 1, 0, 7);
    assert_eq!(a, dungeon::generate(&c, 1, 0, 7));
    assert_ne!(
        a,
        dungeon::generate(&c, 2, 0, 7),
        "another season is another dungeon"
    );
    assert_ne!(
        a,
        dungeon::generate(&c, 1, 1, 7),
        "the collapse re-seeds the dungeon"
    );
    assert_ne!(a, dungeon::generate(&c, 1, 0, 8));
}

#[test]
fn every_level_of_many_seasons_is_connected_and_well_formed() {
    let c = content();
    for season in 1..=6 {
        for depth in 1..=25u8 {
            let level = dungeon::generate(&c, season, 0, depth);
            let n = level.rooms.len();
            assert!((10..=13).contains(&n), "{n} rooms");
            let links: Vec<_> = level.rooms.iter().map(|r| r.links).collect();
            let dist = distances(&links, 0);
            assert!(dist.iter().all(|d| *d != u32::MAX), "unreachable room");
            assert_ne!(level.exit, level.entry);
            assert_ne!(level.shaft, level.entry);
            assert_ne!(level.shaft, level.exit);
            assert_eq!(level.rooms[0].kind, RoomKind::Entry);
            let exit_kind = level.rooms[usize::from(level.exit)].kind;
            assert_eq!(exit_kind == RoomKind::Guardian, has_guardian(depth));
            assert_eq!(level.rooms[usize::from(level.shaft)].kind, RoomKind::Shaft);
            assert!(
                level
                    .rooms
                    .iter()
                    .filter(|r| r.kind == RoomKind::Lair)
                    .count()
                    >= 2
            );
            // The exit is the farthest room from the entry.
            assert_eq!(dist[usize::from(level.exit)], *dist.iter().max().unwrap());
            // Grid positions are unique, and every link is mirrored.
            for (i, r) in level.rooms.iter().enumerate() {
                for (d, to) in r.links.iter().enumerate() {
                    if let Some(to) = to {
                        let back = level.rooms[usize::from(*to)].links[(d + 2) % 4];
                        assert_eq!(back, Some(u8::try_from(i).unwrap()));
                    }
                }
                for other in &level.rooms[i + 1..] {
                    assert!((r.x, r.y) != (other.x, other.y));
                }
            }
        }
    }
}

#[test]
fn depth_bands_are_five_levels_with_a_guardian_at_the_bottom_of_each() {
    assert_eq!(dungeon::band_of(1), 1);
    assert_eq!(dungeon::band_of(5), 1);
    assert_eq!(dungeon::band_of(6), 2);
    assert_eq!(dungeon::band_of(25), 5);
    let guardian_levels: Vec<u8> = (1..=25).filter(|d| has_guardian(*d)).collect();
    assert_eq!(guardian_levels, [5, 10, 15, 20, 25]);
}

#[test]
fn rooms_use_templates_of_their_band_and_kind() {
    let c = content();
    for depth in 1..=25u8 {
        let level = dungeon::generate(&c, 3, 0, depth);
        let band = u32::from(dungeon::band_of(depth));
        for r in &level.rooms {
            let t = &c.rooms.room[r.template];
            assert_eq!(t.kind, r.kind);
            assert!(t.bands.contains(&band), "{} in band {band}", t.id);
        }
    }
}

#[test]
fn what_changes_by_day_is_also_a_pure_function_and_shared() {
    let c = content();
    let level = dungeon::generate(&c, 1, 0, 3);
    let lair = u8::try_from(
        level
            .rooms
            .iter()
            .position(|r| r.kind == RoomKind::Lair)
            .unwrap(),
    )
    .unwrap();
    let ids = |day| -> Vec<String> {
        dungeon::pack_at(&c, &level, 1, 0, lair, day)
            .iter()
            .map(|m| m.id.clone())
            .collect()
    };
    assert_eq!(ids(100), ids(100));
    assert!((1..=3).contains(&ids(100).len()));
    let differ = (100..140).any(|d| ids(d) != ids(100));
    assert!(differ, "the dungeon shifts from day to day");
    for room in 0..level.rooms.len() {
        let room = u8::try_from(room).unwrap();
        let a = dungeon::trap_at(&c, &level, 1, 0, room, 77).map(|t| t.id.clone());
        assert_eq!(
            a,
            dungeon::trap_at(&c, &level, 1, 0, room, 77).map(|t| t.id.clone())
        );
    }
}

#[test]
fn a_guardian_room_holds_the_band_guardian_and_its_company() {
    let c = content();
    for band in 1..=5u8 {
        let depth = band * 5;
        let level = dungeon::generate(&c, 1, 0, depth);
        let pack = dungeon::pack_at(&c, &level, 1, 0, level.exit, 9);
        assert!(pack[0].guardian);
        assert_eq!(pack[0].band, u32::from(band));
        assert!(pack.len() >= 2, "guardians come with company");
    }
}
