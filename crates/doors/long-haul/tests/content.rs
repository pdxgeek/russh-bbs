//! The schema test for the embedded tables, and a few promises the engine relies on.
#![allow(clippy::unwrap_used)]

use russh_bbs_door_long_haul::content::{Content, GoodClass, TABLES};

#[test]
fn every_embedded_table_loads_and_validates() {
    let content = Content::embedded().unwrap();
    assert_eq!(TABLES.len(), 6);
    assert_eq!(content.goods.len(), 30);
    assert_eq!(content.economies.len(), 6);
    assert_eq!(content.hulls.len(), 4);
    assert_eq!(content.backgrounds.len(), 4);
    assert!(content.news.len() >= 12);
    assert!(content.modules.len() >= 15);
    assert!(
        content
            .goods
            .iter()
            .filter(|g| g.class == GoodClass::Contraband)
            .count()
            >= 4
    );
}

#[test]
fn a_bad_edit_is_caught_with_a_readable_message() {
    use russh_bbs_door::content::check;
    // Not a public table type, so go through the public loader with a broken names table.
    let broken = "system_prefix = [\"A\"]\n";
    let err = check::<russh_bbs_door_long_haul::content::Names>("names", broken).unwrap_err();
    assert!(err.to_string().contains("names"));
}

#[test]
fn every_background_has_its_own_perk_and_every_perk_is_known() {
    let content = Content::embedded().unwrap();
    let mut perks: Vec<_> = content
        .backgrounds
        .iter()
        .map(|b| b.perk().unwrap())
        .collect();
    perks.dedup();
    assert_eq!(perks.len(), content.backgrounds.len());
}
