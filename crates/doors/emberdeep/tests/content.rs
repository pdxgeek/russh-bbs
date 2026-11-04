//! The schema test for Emberdeep's content tables, and the cross-references between them.

#![allow(clippy::unwrap_used, clippy::panic)]

use russh_bbs_door::content;
use russh_bbs_door_emberdeep::content_town::{
    AFFIXES_TOML, Affixes, PROJECTS_TOML, Projects, RECIPES_TOML, Recipes, TOWN_TOML, TownTables,
};
use russh_bbs_door_emberdeep::content::{
    BESTIARY_TOML, Bestiary, CALLINGS_TOML, Callings, Content, GEAR_TOML, Gear, MOVES_TOML, Moves,
    NODES_TOML, Nodes, ROOMS_TOML, Rooms, TRAITS_TOML, Traits, WOUNDS_TOML, Wounds,
};

#[test]
fn every_table_passes_its_schema() {
    assert_eq!(
        content::check::<Callings>("callings", CALLINGS_TOML),
        Ok(())
    );
    assert_eq!(content::check::<Nodes>("nodes", NODES_TOML), Ok(()));
    assert_eq!(content::check::<Traits>("traits", TRAITS_TOML), Ok(()));
    assert_eq!(content::check::<Moves>("moves", MOVES_TOML), Ok(()));
    assert_eq!(
        content::check::<Bestiary>("bestiary", BESTIARY_TOML),
        Ok(())
    );
    assert_eq!(content::check::<Rooms>("rooms", ROOMS_TOML), Ok(()));
    assert_eq!(content::check::<Wounds>("wounds", WOUNDS_TOML), Ok(()));
    assert_eq!(content::check::<Gear>("gear", GEAR_TOML), Ok(()));
    assert_eq!(content::check::<Affixes>("affixes", AFFIXES_TOML), Ok(()));
    assert_eq!(content::check::<Recipes>("recipes", RECIPES_TOML), Ok(()));
    assert_eq!(content::check::<Projects>("projects", PROJECTS_TOML), Ok(()));
    assert_eq!(content::check::<TownTables>("town", TOWN_TOML), Ok(()));
}

#[test]
fn a_broken_town_table_is_refused_with_every_problem_named() {
    // A project that needs itself, costs nothing and goes to a level that does not exist.
    let bad = r#"
project = [
  { id = "Bad Id", name = "x", text = "t", coin = 0, salvage = 0, requires = ["Bad Id"], effect = { kind = "lift", depth = 40 }, done_text = "d" },
]"#;
    let err = content::check::<Projects>("projects", bad).unwrap_err().to_string();
    assert!(err.contains("must be a-z"), "{err}");
    assert!(err.contains("must cost something"), "{err}");
    assert!(err.contains("requires itself"), "{err}");
    assert!(err.contains("level 2 to 25"), "{err}");
    // A beast with a room, a rescue with a monster, and a thread that is too short.
    let bad = r#"
contract = [
  { id = "a", name = "A", text = "t", kind = "beast", depth = 1, monster = "slag-rat", room = "vault", coin = 1, xp = 1, days = 3, done_text = "d" },
  { id = "b", name = "B", text = "t", kind = "rescue", depth = 1, monster = "slag-rat", coin = 1, xp = 1, days = 3, done_text = "d" },
  { id = "c", name = "C", text = "t", kind = "heirloom", depth = 1, room = "vault", escort = true, coin = 1, xp = 1, days = 9, done_text = "d" },
]
rumour = []
companion = [
  { id = "x", name = "X", text = "t", attr = "wits", dc = 12, gift = 1, steps = [] },
]"#;
    let err = content::check::<TownTables>("town", bad).unwrap_err().to_string();
    assert!(err.contains("a beast names a monster only"), "{err}");
    assert!(err.contains("it names a room kind only"), "{err}");
    assert!(err.contains("only a rescue escorts"), "{err}");
    assert!(err.contains("days must be 1 to 7"), "{err}");
    assert!(err.contains("three rungs"), "{err}");
}

#[test]
fn the_tables_agree_with_each_other() {
    let c = Content::embedded().unwrap();
    assert_eq!(c.cross_check(), Vec::<String>::new());
}
