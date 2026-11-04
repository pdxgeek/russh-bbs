//! The schema test for Emberdeep's content tables, and the cross-references between them.

#![allow(clippy::unwrap_used, clippy::panic)]

use russh_bbs_door::content;
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
}

#[test]
fn the_tables_agree_with_each_other() {
    let c = Content::embedded().unwrap();
    assert_eq!(c.cross_check(), Vec::<String>::new());
}
