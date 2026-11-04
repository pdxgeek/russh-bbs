//! Saves and worlds written by the E1 engine (format version 1), pinned as bytes, so a later
//! slice can prove they still load. They were produced by the E1 code itself (commit ecb9a05, a
//! runesmith with a gash and a limp; a world with one lift, one hall entry and two news lines).
//! Never regenerate these from newer code: they are what real boards hold.

pub(crate) const V1_HERO: &str = "065465737465720972756e65736d6974680b090d090382000000010038000000001900000089000000000000002a000000020000020c72756e652d746f75636865640573746f757401046c696d70010467617368030b72756e652d68616d6d65720b736d6974682d6170726f6e00000000000000e320a0cb6c6f4fd7b2d4e4215868db5b2daaf8471779e65c7503c2a19a45d3b20000204e0000000000000000010000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";

pub(crate) const V1_WORLD: &str = "010000000000204e000000000000010300000001034164610400021d416461206f70656e65642061206c696674206f6e206c6576656c20332e22426f20646964206e6f7420636f6d65206261636b2066726f6d206c6576656c20322e";

pub(crate) fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

use super::support::content;
use crate::game::{Game, Place};
use crate::world::World;

#[test]
fn an_e1_save_is_not_a_version_two_save_until_it_is_migrated() {
    let bytes = unhex(V1_HERO);
    assert!(
        Game::decode(&bytes).is_none(),
        "version 1 lacks the town section"
    );
    let up = crate::save::upgrade_v1(&bytes).expect("a real E1 save migrates");
    let g = Game::decode(&up).expect("and then reads");
    // Everything E1 wrote is still there...
    assert_eq!(g.hero.name, "Tester");
    assert_eq!(g.hero.calling, "runesmith");
    assert_eq!((g.hero.coin, g.hero.stash, g.hero.relics), (137, 42, 2));
    assert_eq!((g.hero.level, g.hero.xp), (3, 130));
    assert_eq!(
        g.hero.traits,
        vec!["rune-touched".to_owned(), "stout".to_owned()]
    );
    assert_eq!(g.hero.scars, vec!["limp".to_owned()]);
    assert!(g.hero.wounds.iter().any(|(w, _)| w == "gash"));
    assert_eq!(g.hero.weapon, "rune-hammer");
    assert_eq!(g.place, Place::Town);
    // ...and the town section is empty and sound.
    assert!(g.hero.pack.is_empty() && g.hero.cores.is_empty() && g.hero.recipes.is_empty());
    assert_eq!((g.hero.wkit.wear, g.hero.akit.wear, g.hero.debt), (0, 0, 0));
    assert!(g.hero.bond.is_none() && g.hero.contract.is_none() && g.hero.given.is_empty());
    // The migrated bytes are exactly what the new code writes.
    assert_eq!(g.encode(), up);
    // The calling's recipe is there for the old hero without any migration of data.
    let c = content();
    assert!(g.hero.knows(c.recipe("runesmith-mallet").unwrap()));
}

#[test]
fn an_e1_world_migrates_with_its_shortcuts_hall_and_news_and_no_pledges() {
    let bytes = unhex(V1_WORLD);
    assert!(World::decode(&bytes).is_none());
    let up = crate::world::upgrade_v1(&bytes).expect("migrates");
    let w = World::decode(&up).expect("reads");
    assert_eq!(w.shortcuts, vec![(3, crate::content::ShortcutKind::Lift)]);
    assert_eq!(w.deepest, vec![("Ada".to_owned(), 4)]);
    assert_eq!(w.day, 20_000);
    assert!(w.news.iter().any(|n| n.contains("opened a lift")));
    assert!(w.pledges.is_empty());
    assert_eq!(w.encode(), up);
}

#[test]
fn damaged_version_one_blobs_are_refused_by_the_migration() {
    let hero = unhex(V1_HERO);
    assert!(crate::save::upgrade_v1(&hero[..hero.len() - 3]).is_none());
    assert!(crate::save::upgrade_v1(&[]).is_none());
    let mut extra = hero.clone();
    extra.push(9);
    assert!(crate::save::upgrade_v1(&extra).is_none());
    let world = unhex(V1_WORLD);
    assert!(crate::world::upgrade_v1(&world[..world.len() - 2]).is_none());
}

#[test]
fn the_door_migrates_version_one_and_nothing_else() {
    use russh_bbs_door::{BlobKind, Door, MigrateError};
    let door = crate::Emberdeep::new().unwrap();
    let up = door.migrate(BlobKind::Save, 1, &unhex(V1_HERO)).unwrap();
    assert_eq!(up, crate::save::upgrade_v1(&unhex(V1_HERO)).unwrap());
    let w = door.migrate(BlobKind::World, 1, &unhex(V1_WORLD)).unwrap();
    assert!(World::decode(&w).is_some());
    assert_eq!(
        door.migrate(BlobKind::Save, 1, &[1, 2, 3]),
        Err(MigrateError::Corrupt(1))
    );
    assert_eq!(
        door.migrate(BlobKind::Save, 2, &[]),
        Err(MigrateError::Unsupported(2))
    );
    assert_eq!(
        door.migrate(BlobKind::World, 0, &[]),
        Err(MigrateError::Unsupported(0))
    );
    assert_eq!(door.info().save_version, 2);
    assert_eq!(door.info().world_version, 2);
}
