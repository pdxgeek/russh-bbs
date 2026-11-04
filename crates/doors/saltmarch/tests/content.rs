//! The schema tests for Saltmarch's content tables: every embedded table passes its own
//! validation, a bad edit is named and not swallowed, and the cross-table links hold.

#![allow(clippy::unwrap_used, clippy::panic)]

use russh_bbs_door::content::{self, ContentError};
use russh_bbs_door_saltmarch::content::Cause;
use russh_bbs_door_saltmarch::content::{
    BUILDINGS_TOML, Buildings, Catalog, EPITAPHS_TOML, Epitaphs, INJURIES_TOML, Injuries,
    NAMES_TOML, Names, POIS_TOML, Pois, RECIPES_TOML, Recipes, TRAITS_TOML, Texts, Traits,
};
use russh_bbs_door_saltmarch::hold::{COLS, PLOTS, ROWS};
use russh_bbs_door_saltmarch::lore::{
    Behaviour, Cond, EVENTS_TOML, Effect, Events, FOES_TOML, Foes, GATHER_TOML, Gathers, Req,
};
use russh_bbs_door_saltmarch::map::PoiKind;
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
    assert_eq!(content::check::<Events>("events", EVENTS_TOML), Ok(()));
    assert_eq!(content::check::<Foes>("foes", FOES_TOML), Ok(()));
    assert_eq!(content::check::<Gathers>("gather", GATHER_TOML), Ok(()));
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
    let err = Catalog::from_texts(&Texts {
        recipes: &bad_workshop,
        ..Texts::embedded()
    })
    .unwrap_err();
    assert!(err.to_string().contains("not a building"), "{err}");
    let bad_scar = INJURIES_TOML.replace("scar = \"limp\"", "scar = \"steady\"");
    let err = Catalog::from_texts(&Texts {
        injuries: &bad_scar,
        ..Texts::embedded()
    })
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
    assert!(any(&|t| t.fight_atk < 0), "fight attack");
    assert!(any(&|t| t.fight_def > 0), "fight armour");
    assert!(any(&|t| t.fight_spd != 0), "fight speed");
    assert!(any(&|t| t.fight_hp > 0), "fight hit points");
}

// ------------------------------------------------------------------ expeditions: schema

/// A valid one-event table to break in one place at a time.
const BASE: &str = r#"
[[event]]
id = "x"
title = "T"
text = "Body."
[[event.option]]
label = "A"
[event.option.win]
text = "ok"
[[event.option]]
label = "B"
[event.option.win]
text = "ok"
"#;

/// The problems a one-event table has, ignoring the table-wide count checks.
fn event_problems(text: &str) -> Vec<String> {
    match content::check::<Events>("events", text) {
        Err(ContentError::Invalid { problems, .. }) => problems
            .into_iter()
            .filter(|p| !p.contains("events are needed"))
            .collect(),
        Err(other) => vec![other.to_string()],
        Ok(()) => Vec::new(),
    }
}

fn says(text: &str, needle: &str) {
    let found = event_problems(text);
    assert!(
        found.iter().any(|p| p.contains(needle)),
        "wanted `{needle}` in {found:?}"
    );
}

#[test]
fn a_bad_event_edit_is_named() {
    assert_eq!(event_problems(BASE), Vec::<String>::new());
    let with = |from: &str, to: &str| BASE.replacen(from, to, 1);
    // Effects and conditions are parsed once, at load.
    says(
        &with("text = \"ok\"", "text = \"ok\"\nfx = [\"explode\"]"),
        "not an effect",
    );
    says(
        &with("text = \"ok\"", "text = \"ok\"\nfx = [\"gain:gold:3\"]"),
        "not a resource",
    );
    says(
        &with("text = \"ok\"", "text = \"ok\"\nfx = [\"gain:salt:5-2\"]"),
        "backwards",
    );
    says(
        &with("label = \"A\"", "label = \"A\"\nneeds = [\"trade:jester\"]"),
        "not a trade",
    );
    says(
        &with("label = \"A\"", "label = \"A\"\nneeds = [\"whim\"]"),
        "not a condition",
    );
    says(
        &with("text = \"ok\"", "text = \"ok\"\nfx = [\"hurt:any:bruise\"]"),
        "not an injury cause",
    );
    says(
        &with("text = \"ok\"", "text = \"ok\"\nsurprise = 1"),
        "surprise",
    );
    // Rules of the table.
    says(&with("id = \"x\"", "id = \"Bad Id\""), "must be a-z");
    says(&format!("{BASE}{BASE}"), "used twice");
    says(
        &with("text = \"Body.\"", "text = \"Body.\\u001b[31m\""),
        "plain text",
    );
    says(
        &with("label = \"A\"", "label = \"A\"\nrisk = 30"),
        "`lose` goes with a risk",
    );
    says(
        &with("label = \"A\"", "label = \"A\"\nskill = \"wader\""),
        "skill matters only with a risk",
    );
    let one_option = BASE.replace(
        "[[event.option]]\nlabel = \"B\"\n[event.option.win]\ntext = \"ok\"\n",
        "",
    );
    says(&one_option, "two to four options");
    let gated = BASE
        .replace("label = \"A\"", "label = \"A\"\nneeds = [\"scar\"]")
        .replace("label = \"B\"", "label = \"B\"\nneeds = [\"bond\"]");
    says(&gated, "at least one option must have no requirements");
    says(
        &with(
            "text = \"ok\"",
            "text = \"ok\"\nfx = [\"fight:g\", \"turns:1\"]",
        ),
        "`fight` must be the last effect",
    );
    says(
        &with(
            "title = \"T\"",
            "title = \"T\"\npoi = [\"camp\"]\ntiles = [\"mudflat\"]",
        ),
        "does not name tiles",
    );
    says(
        &with("title = \"T\"", "title = \"T\"\nrare = true\nweight = 9"),
        "weight must be 1 to 3",
    );
    says(
        &with("title = \"T\"", "title = \"T\"\nweight = 99"),
        "weight must be 2 to 20",
    );
    says(
        &with("text = \"ok\"", "text = \"ok\"\nfx = [\"mark:Bad Mark\"]"),
        "must be a-z and -",
    );
    says(
        &with("text = \"ok\"", "text = \"ok\"\nfx = [\"turns:40\"]"),
        "turns is 1 to 8",
    );
    says(
        &with("text = \"ok\"", "text = \"ok\"\nfx = [\"gain:salt:99\"]"),
        "a gain is 1 to 60",
    );
    // The table as a whole must be big enough.
    match content::check::<Events>("events", BASE) {
        Err(ContentError::Invalid { problems, .. }) => {
            assert!(problems.iter().any(|p| p.contains("at least 6 rare")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_bad_cross_link_in_events_or_foes_fails_the_catalog() {
    let embedded = Texts::embedded();
    let bad = |events: &str, foes: &str| {
        Catalog::from_texts(&Texts {
            events,
            foes,
            ..embedded
        })
        .unwrap_err()
        .to_string()
    };
    let ev = |from: &str, to: &str| EVENTS_TOML.replacen(from, to, 1);
    assert!(
        bad(&ev("fight:eel-pair", "fight:ghost-pair"), FOES_TOML).contains("unknown fight group")
    );
    assert!(bad(&ev("trait:haunted", "trait:spooked"), FOES_TOML).contains("unknown trait"));
    assert!(
        bad(
            &ev("recipe:fuse-silt-glass", "recipe:braid-rope"),
            FOES_TOML
        )
        .contains("not a recipe learned from a ruin")
    );
    let foes = |from: &str, to: &str| FOES_TOML.replacen(from, to, 1);
    assert!(
        bad(
            EVENTS_TOML,
            &foes(
                "foes = [\"mire-eel\", \"mire-eel\"]",
                "foes = [\"mire-eel\", \"kraken\"]"
            )
        )
        .contains("unknown foe")
    );
    assert!(
        bad(
            EVENTS_TOML,
            &foes(
                "row = \"back\"\nbehaviour = \"hurler\"",
                "row = \"front\"\nbehaviour = \"hurler\""
            )
        )
        .contains("hurler")
    );
    assert!(bad(EVENTS_TOML, &foes("hp = 12\natk = 4", "hp = 1\natk = 4")).contains("hp 3-60"));
    let gather = |from: &str, to: &str| GATHER_TOML.replacen(from, to, 1);
    let err = Catalog::from_texts(&Texts {
        gather: &gather("find_pct = 3", "find_pct = 0"),
        ..embedded
    })
    .unwrap_err()
    .to_string();
    assert!(err.contains("find and find_pct"), "{err}");
    let err = Catalog::from_texts(&Texts {
        gather: &format!("{GATHER_TOML}\n[[gather]]\ntile = \"reedbed\"\ntrade = \"raker\"\nyields = {{ reed = 1 }}\n"),
        ..embedded
    })
    .unwrap_err()
    .to_string();
    assert!(err.contains("listed twice"), "{err}");
}

// ------------------------------------------------------------------ expeditions: shape

fn all_conds(cat: &Catalog) -> Vec<&Cond> {
    cat.events
        .iter()
        .flat_map(|e| {
            e.needs
                .iter()
                .chain(&e.draws)
                .chain(e.options.iter().flat_map(|o| &o.needs))
        })
        .collect()
}

fn all_effects(cat: &Catalog) -> Vec<&Effect> {
    cat.events
        .iter()
        .flat_map(|e| &e.options)
        .flat_map(|o| std::iter::once(&o.win).chain(o.lose.as_ref()))
        .flat_map(|o| &o.fx)
        .collect()
}

#[test]
fn the_first_batch_of_events_has_the_promised_size_and_spread() {
    let cat = Catalog::embedded().unwrap();
    assert!(cat.events.len() >= 40, "{}", cat.events.len());
    assert!(cat.events.iter().filter(|e| e.rare).count() >= 6);
    // Every kind of place has a scene, and so does every kind of ground that can be walked.
    for kind in [
        PoiKind::Camp,
        PoiKind::Shrine,
        PoiKind::Salvage,
        PoiKind::Lair,
        PoiKind::Hermit,
        PoiKind::Trader,
        PoiKind::Ruin,
    ] {
        let n = cat.events.iter().filter(|e| e.poi.contains(&kind)).count();
        assert!(n >= 1, "{kind:?}");
    }
    for kind in [
        TileKind::Mudflat,
        TileKind::Reedbed,
        TileKind::PeatBog,
        TileKind::SaltPan,
        TileKind::Channel,
        TileKind::Island,
        TileKind::DrownedTown,
        TileKind::Wreck,
        TileKind::Mere,
        TileKind::Deep,
    ] {
        let n = cat
            .events
            .iter()
            .filter(|e| e.poi.is_empty() && !e.camp && e.tiles.contains(&kind))
            .count();
        assert!(n >= 2, "{kind:?} has {n} scenes");
        assert!(cat.gather(kind).is_some(), "{kind:?} can be worked");
    }
    assert!(
        cat.events.iter().filter(|e| e.camp).count() >= 3,
        "camp scenes"
    );
    // Times of day, water and the season each decide some scene.
    assert!(cat.events.iter().any(|e| !e.day.is_empty()));
    assert!(cat.events.iter().any(|e| !e.water.is_empty()));
    assert!(cat.events.iter().any(|e| !e.stage.is_empty()));
    // Foes: ten or more kinds, every one in a group and every group met in some scene.
    assert!(cat.foes.len() >= 10 && cat.groups.len() >= 10);
    for f in &cat.foes {
        assert!(
            cat.groups.iter().any(|g| g.foes.contains(&f.id)),
            "{}",
            f.id
        );
    }
    let fought: Vec<&str> = all_effects(&cat)
        .into_iter()
        .filter_map(|e| match e {
            Effect::Fight(g) => Some(g.as_str()),
            _ => None,
        })
        .collect();
    for g in &cat.groups {
        assert!(
            fought.contains(&g.id.as_str()),
            "group {} is never fought",
            g.id
        );
    }
    // Each way a foe fights is used, and the water matters to some of them.
    for b in [
        Behaviour::Bruiser,
        Behaviour::Lurker,
        Behaviour::Hurler,
        Behaviour::Skittish,
        Behaviour::Swarm,
    ] {
        assert!(cat.foes.iter().any(|f| f.behaviour == b), "{b:?}");
    }
    assert!(cat.foes.iter().any(|f| f.aquatic) && cat.foes.iter().any(|f| f.tidal));
}

#[test]
fn every_trade_and_every_kind_of_history_decides_some_option() {
    let cat = Catalog::embedded().unwrap();
    // Each of the eight trades opens options of its own, and uses its skill to tip odds.
    for t in Trade::ALL {
        let opens = all_conds(&cat)
            .into_iter()
            .filter(|c| matches!(&c.req, Req::Trade(x, _) | Req::Skill(x, _) if *x == t) && !c.not)
            .count();
        let tips = cat
            .events
            .iter()
            .flat_map(|e| &e.options)
            .filter(|o| o.skill == Some(t))
            .count();
        let grows = all_effects(&cat)
            .into_iter()
            .filter(|e| matches!(e, Effect::Xp(x, _) if *x == t))
            .count();
        assert!(opens >= 1, "{t:?} opens no option");
        assert!(
            tips + grows >= 2,
            "{t:?} is never tested or trained ({tips}+{grows})"
        );
    }
    let count = |f: &dyn Fn(&Cond) -> bool| all_conds(&cat).into_iter().filter(|c| f(c)).count();
    assert!(
        count(&|c| matches!(&c.req, Req::Trait(_)) && !c.not) >= 3,
        "traits open options"
    );
    assert!(
        count(&|c| matches!(&c.req, Req::Scar) && !c.not) >= 1,
        "scars open options"
    );
    assert!(
        count(&|c| c.not && matches!(&c.req, Req::Trait(_))) >= 1,
        "scars close options"
    );
    assert!(
        count(&|c| matches!(&c.req, Req::Bond)) >= 1,
        "bonds open options"
    );
    assert!(
        count(&|c| matches!(&c.req, Req::Carry(..))) >= 6,
        "supplies open options"
    );
    assert!(
        count(&|c| matches!(&c.req, Req::Boat)) >= 1,
        "a boat opens options"
    );
    assert!(
        count(&|c| matches!(&c.req, Req::Size(_))) >= 2,
        "party size opens options"
    );
    assert!(
        count(&|c| c.not && matches!(&c.req, Req::Mark(_))) >= 2,
        "deeds close options"
    );
    // Many scenes have more than one way through.
    let gated = cat
        .events
        .iter()
        .filter(|e| e.options.iter().filter(|o| !o.needs.is_empty()).count() >= 1)
        .count();
    assert!(
        gated * 10 >= cat.events.len() * 8,
        "{gated} of {} scenes have a gated way",
        cat.events.len()
    );
}

#[test]
fn every_deed_the_clan_is_marked_with_is_remembered_by_some_scene() {
    let cat = Catalog::embedded().unwrap();
    let set: Vec<&str> = all_effects(&cat)
        .into_iter()
        .filter_map(|e| match e {
            Effect::Mark(m) => Some(m.as_str()),
            _ => None,
        })
        .collect();
    let read: Vec<&str> = all_conds(&cat)
        .into_iter()
        .filter_map(|c| match &c.req {
            Req::Mark(m) => Some(m.as_str()),
            _ => None,
        })
        .collect();
    assert!(set.len() >= 15, "{} deeds are recorded", set.len());
    for m in &set {
        assert!(
            read.contains(m),
            "`{m}` is recorded but no scene asks for it"
        );
    }
    for m in &read {
        assert!(set.contains(m), "`{m}` is asked for but nothing records it");
    }
    // Some scenes undo a deed.
    assert!(
        all_effects(&cat)
            .iter()
            .any(|e| matches!(e, Effect::Unmark(_)))
    );
}

#[test]
fn fights_hurt_in_their_own_way_and_scars_feed_back_into_the_scenes() {
    let cat = Catalog::embedded().unwrap();
    let fight: Vec<_> = cat
        .injuries
        .iter()
        .filter(|i| i.cause == Cause::Fight)
        .collect();
    assert!(fight.len() >= 4);
    assert!(
        fight.iter().all(|i| i.scar.is_some()),
        "every wound can scar"
    );
    // A scar earned in a fight is a trait the scenes or the fights read.
    for scar in fight.iter().filter_map(|i| i.scar.as_deref()) {
        let t = cat.trait_def(scar).unwrap();
        let read_by_scene = all_conds(&cat)
            .iter()
            .any(|c| matches!(&c.req, Req::Trait(x) if x == scar) || matches!(&c.req, Req::Scar));
        let read_by_fight =
            t.fight_atk != 0 || t.fight_def != 0 || t.fight_spd != 0 || t.fight_hp != 0;
        assert!(read_by_scene || read_by_fight, "{scar}");
    }
    assert!(
        fight.iter().any(|i| i.scar.as_deref() == Some("limp")),
        "limp closes the run for it"
    );
}
