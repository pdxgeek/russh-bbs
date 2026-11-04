//! The schema tests for Saltmarch's content tables: every embedded table passes its own
//! validation, a bad edit is named and not swallowed, and the cross-table links hold.

#![allow(clippy::unwrap_used, clippy::panic)]

use russh_bbs_door::content::{self, ContentError};
use russh_bbs_door_saltmarch::content::{
    BUILDINGS_TOML, Buildings, Catalog, EPITAPHS_TOML, Epitaphs, INJURIES_TOML, Injuries,
    NAMES_TOML, Names, POIS_TOML, Pois, RECIPES_TOML, Recipes, TRAITS_TOML, Traits,
};
use russh_bbs_door_saltmarch::hold::{COLS, PLOTS, ROWS};
use russh_bbs_door_saltmarch::map::TileKind;
use russh_bbs_door_saltmarch::types::{Resource, Track, Trade};

#[test]
fn every_embedded_table_passes_its_schema() {
    assert_eq!(
        content::check::<Buildings>("buildings", BUILDINGS_TOML),
        Ok(())
    );
    assert_eq!(content::check::<Recipes>("recipes", RECIPES_TOML), Ok(()));
    assert_eq!(content::check::<Traits>("traits", TRAITS_TOML), Ok(()));
    assert_eq!(
        content::check::<Injuries>("injuries", INJURIES_TOML),
        Ok(())
    );
    assert_eq!(content::check::<Pois>("pois", POIS_TOML), Ok(()));
    assert_eq!(content::check::<Names>("names", NAMES_TOML), Ok(()));
    assert_eq!(
        content::check::<Epitaphs>("epitaphs", EPITAPHS_TOML),
        Ok(())
    );
    Catalog::embedded().unwrap();
}

fn problems(result: Result<(), ContentError>) -> Vec<String> {
    match result {
        Err(ContentError::Invalid { problems, .. }) => problems,
        other => panic!("expected invalid, got {other:?}"),
    }
}

#[test]
fn a_bad_building_edit_is_named() {
    let dup = format!(
        "{BUILDINGS_TOML}\n[[building]]\nid = \"hut\"\nname = \"Again\"\ntrack = \"shelter\"\ntier = 1\nblurb = \"x\"\ncost = {{ timber = 1 }}\nbuild_turns = 1\n"
    );
    assert!(
        problems(content::check::<Buildings>("b", &dup))
            .iter()
            .any(|p| p.contains("used twice"))
    );
    let unknown = BUILDINGS_TOML.replace(
        "requires = [\"hut\"]\ncost = { timber = 14",
        "requires = [\"palace\"]\ncost = { timber = 14",
    );
    assert!(
        problems(content::check::<Buildings>("b", &unknown))
            .iter()
            .any(|p| p.contains("unknown `palace`"))
    );
    let cycle = BUILDINGS_TOML
        .replace("id = \"hut\"\nname = \"Reed hut\"\ntrack = \"shelter\"\ntier = 1\nblurb = \"Reed walls, a peat fire. Room for two more crew.\"", "id = \"hut\"\nname = \"Reed hut\"\ntrack = \"shelter\"\ntier = 1\nrequires = [\"longhouse\"]\nblurb = \"Reed walls, a peat fire. Room for two more crew.\"");
    assert!(
        problems(content::check::<Buildings>("b", &cycle))
            .iter()
            .any(|p| p.contains("cycle"))
    );
    let free = BUILDINGS_TOML.replace("cost = { timber = 6, reed = 4 }", "cost = {}");
    assert!(
        problems(content::check::<Buildings>("b", &free))
            .iter()
            .any(|p| p.contains("must cost"))
    );
    let esc = BUILDINGS_TOML.replace("Reed hut", "Reed \\u001b[31mhut");
    assert!(
        problems(content::check::<Buildings>("b", &esc))
            .iter()
            .any(|p| p.contains("plain text"))
    );
}

#[test]
fn unknown_fields_resources_and_kinds_do_not_parse() {
    assert!(matches!(
        content::check::<Buildings>(
            "b",
            &BUILDINGS_TOML.replace("build_turns = 3", "build_turnz = 3")
        ),
        Err(ContentError::Parse { .. })
    ));
    assert!(matches!(
        content::check::<Buildings>("b", &BUILDINGS_TOML.replace("timber = 6", "gold = 6")),
        Err(ContentError::Parse { .. })
    ));
    assert!(matches!(
        content::check::<Pois>(
            "p",
            &POIS_TOML.replace("\"reedbed\", \"island\"", "\"lava\"")
        ),
        Err(ContentError::Parse { .. })
    ));
}

#[test]
fn a_bad_cross_link_fails_the_catalog() {
    let bad_workshop = RECIPES_TOML.replace("workshop = \"smithy\"", "workshop = \"forge\"");
    let err = Catalog::from_texts(
        BUILDINGS_TOML,
        &bad_workshop,
        TRAITS_TOML,
        INJURIES_TOML,
        POIS_TOML,
        NAMES_TOML,
        EPITAPHS_TOML,
    )
    .unwrap_err();
    assert!(err.to_string().contains("not a building"), "{err}");
    let bad_scar = INJURIES_TOML.replace("scar = \"limp\"", "scar = \"steady\"");
    let err = Catalog::from_texts(
        BUILDINGS_TOML,
        RECIPES_TOML,
        TRAITS_TOML,
        &bad_scar,
        POIS_TOML,
        NAMES_TOML,
        EPITAPHS_TOML,
    )
    .unwrap_err();
    assert!(err.to_string().contains("not a scar trait"), "{err}");
}

#[test]
fn the_launch_shape_of_the_tables_is_what_this_slice_promises() {
    let cat = Catalog::embedded().unwrap();
    // Three tracks, every tier-1 building affordable from a founding clan's stores.
    for track in Track::ALL {
        assert!(
            cat.buildings.iter().filter(|b| b.track == track).count() >= 4,
            "{track:?}"
        );
    }
    for b in cat.buildings.iter().filter(|b| b.tier == 1) {
        assert!(
            b.cost.of(Resource::Timber) <= 12
                && b.cost.of(Resource::Reed) <= 10
                && b.cost.tallies <= 40,
            "{}",
            b.id
        );
    }
    // Crew housing reaches the cap of twelve exactly, and the plot grid is the documented one.
    assert_eq!((COLS, ROWS, PLOTS), (5, 4, 20));
    let caps: u32 = cat
        .buildings
        .iter()
        .map(|b| u32::from(b.crew_cap) * u32::from(b.max))
        .sum();
    assert_eq!(3 + caps, 12);
    // Every workshop recipe has a building to learn it from and every trade has a use.
    for r in &cat.recipes {
        assert!(cat.building(&r.workshop).is_some(), "{}", r.id);
    }
    for t in Trade::ALL {
        let uses = cat.buildings.iter().filter(|b| b.trade == Some(t)).count()
            + cat.recipes.iter().filter(|r| r.skill == t).count();
        // Fighter, healer and reed-witch act in fights, healing and omens (later slices) or in
        // the engine directly (tending, building defence).
        assert!(
            uses > 0
                || matches!(
                    t,
                    Trade::Fighter
                        | Trade::Healer
                        | Trade::ReedWitch
                        | Trade::Wader
                        | Trade::Salvager
                ),
            "{t:?}"
        );
    }
    assert!(cat.traits.len() >= 12 && cat.traits.iter().filter(|t| t.scar).count() >= 4);
    assert!(cat.injuries.len() >= 8);
    assert!(cat.pois.iter().map(|p| usize::from(p.count)).sum::<usize>() >= 30);
    assert!(
        cat.pois.iter().any(|p| p.rooms >= 6),
        "a Deep ruin with many rooms"
    );
    for kind in [TileKind::DrownedTown, TileKind::Wreck] {
        assert!(cat.pois.iter().any(|p| p.paints == Some(kind)), "{kind:?}");
    }
}

#[test]
fn every_trait_field_the_engine_reads_is_used_by_some_trait() {
    let cat = Catalog::embedded().unwrap();
    let any =
        |f: &dyn Fn(&russh_bbs_door_saltmarch::content::TraitDef) -> bool| cat.traits.iter().any(f);
    assert!(any(&|t| t.yield_pct != 0), "yield");
    assert!(
        any(&|t| t.accident_pct < 0) && any(&|t| t.accident_pct > 0),
        "accident both ways"
    );
    assert!(any(&|t| t.heal_pct > 0), "heal");
    assert!(any(&|t| t.morale != 0), "morale");
    assert!(any(&|t| t.pocket_pct > 0), "pocket");
    assert!(any(&|t| t.wet_pct != 0), "wet");
    assert!(any(&|t| t.luck_pct > 0), "luck");
    assert!(any(&|t| t.learn_pct > 0), "learn");
    assert!(any(&|t| t.upkeep_tallies > 0), "upkeep");
    assert!(any(&|t| t.refuses_high), "refuses");
    assert!(any(&|t| t.hire_pct != 0), "hire");
}
