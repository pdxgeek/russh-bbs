//! Schema tests for the content tables: they load, they validate, and a broken edit is refused.
#![allow(clippy::unwrap_used)]

use russh_bbs_door::content::{self, ContentError};
use russh_bbs_door_nullroute::content::{
    CONTRACTS_TOML, CORPS_TOML, COUNTERMEASURES_TOML, CmKind, CmTable, Content, ContractsTable,
    CorpsTable, FRAGMENTS_TOML, FragKind, FragmentsTable, NODES_TOML, NodesTable, RIG_TOML,
    RigTable, Role, SCRIPTS_TOML, ScriptsTable,
};

#[test]
fn every_embedded_table_passes_its_schema_test() {
    assert_eq!(content::check::<CorpsTable>("corps", CORPS_TOML), Ok(()));
    assert_eq!(content::check::<NodesTable>("nodes", NODES_TOML), Ok(()));
    assert_eq!(
        content::check::<ScriptsTable>("scripts", SCRIPTS_TOML),
        Ok(())
    );
    assert_eq!(
        content::check::<ContractsTable>("contracts", CONTRACTS_TOML),
        Ok(())
    );
    assert_eq!(content::check::<RigTable>("rig", RIG_TOML), Ok(()));
    assert_eq!(
        content::check::<CmTable>("countermeasures", COUNTERMEASURES_TOML),
        Ok(())
    );
    assert_eq!(
        content::check::<FragmentsTable>("fragments", FRAGMENTS_TOML),
        Ok(())
    );
    assert!(Content::try_embedded().is_ok());
}

#[test]
fn the_shipped_content_covers_what_this_slice_promises() {
    let c = Content::try_embedded().unwrap();
    assert!(
        c.corps.corp.len() >= 5,
        "five corporations, four+ distinct styles"
    );
    assert_eq!(c.corps.fixer.len(), 5);
    assert!(c.scripts.script.len() >= 27);
    assert!(c.contracts.contract.len() >= 11);
    assert_eq!(c.rig.part.len(), 5);
    assert!(
        c.nodes
            .node
            .iter()
            .filter(|n| n.role == Role::Objective)
            .count()
            >= 3
    );
}

#[test]
fn contracts_point_at_real_nodes_and_every_objective_has_a_tool_for_sale_or_owned() {
    let c = Content::try_embedded().unwrap();
    for t in &c.contracts.contract {
        let node = c.node_by_code(&t.target).unwrap();
        assert_eq!(node.role, Role::Objective, "{}", t.title);
        let tool = c
            .scripts
            .script
            .iter()
            .any(|s| s.effect == t.effect && (s.starter || s.price > 0));
        assert!(
            tool,
            "nobody can get a {:?} script for `{}`",
            t.effect, t.title
        );
    }
}

#[test]
fn every_corporation_has_a_patron_and_no_fixer_is_without_clients() {
    let c = Content::try_embedded().unwrap();
    for corp in &c.corps.corp {
        assert!(c.fixer(corp.patron).is_some(), "{}", corp.name);
    }
    for fixer in &c.corps.fixer {
        assert!(
            c.corps.corp.iter().any(|co| co.patron == fixer.num),
            "{} has no clients",
            fixer.name
        );
    }
}

#[test]
fn the_starter_deck_is_a_choice() {
    let c = Content::try_embedded().unwrap();
    let mem: u32 = c
        .scripts
        .script
        .iter()
        .filter(|s| s.starter)
        .map(|s| s.memory)
        .sum();
    assert!(
        mem > 8,
        "the starter scripts ({mem} memory) must not all fit a stock deck"
    );
}

fn broken<T: content::Table>(text: &str) -> ContentError {
    content::check::<T>("t", text).unwrap_err()
}

#[test]
fn broken_edits_are_refused_not_loaded() {
    // Not TOML for the type.
    assert!(matches!(
        broken::<ScriptsTable>("script = 3"),
        ContentError::Parse { .. }
    ));
    // An unknown field.
    let extra = SCRIPTS_TOML.replacen("power = 2", "power = 2\nflavour = \"x\"", 1);
    assert!(matches!(
        broken::<ScriptsTable>(&extra),
        ContentError::Parse { .. }
    ));
    // A duplicate number.
    let dup = SCRIPTS_TOML.replacen("num = 2\n", "num = 1\n", 1);
    assert!(matches!(
        broken::<ScriptsTable>(&dup),
        ContentError::Invalid { .. }
    ));
    // An effect in the wrong family.
    let wrong = SCRIPTS_TOML.replacen("family = \"probe\"", "family = \"hide\"", 1);
    assert!(matches!(
        broken::<ScriptsTable>(&wrong),
        ContentError::Invalid { .. }
    ));
    // A control character in a name.
    let ansi = SCRIPTS_TOML.replacen("name = \"ping-sweep\"", "name = \"ping\\u001b[31m\"", 1);
    assert!(matches!(
        broken::<ScriptsTable>(&ansi),
        ContentError::Invalid { .. }
    ));
    // A corporation with a patron nobody is.
    let patron = CORPS_TOML.replacen("patron = 0", "patron = 9", 1);
    assert!(matches!(
        broken::<CorpsTable>(&patron),
        ContentError::Invalid { .. }
    ));
    // A contract whose objective is not copy, plant or shred.
    let effect = CONTRACTS_TOML.replacen("effect = \"copy\"", "effect = \"scan\"", 1);
    assert!(matches!(
        broken::<ContractsTable>(&effect),
        ContentError::Invalid { .. }
    ));
    // A rig with two parts.
    let part = RIG_TOML.replacen("num = 4", "num = 1", 1);
    assert!(matches!(
        broken::<RigTable>(&part),
        ContentError::Invalid { .. }
    ));
    // Too large.
    let huge = format!("{NODES_TOML}\n# {}", "x".repeat(content::MAX_TABLE_BYTES));
    assert!(matches!(
        broken::<NodesTable>(&huge),
        ContentError::TooLarge { .. }
    ));
}

#[test]
fn the_n2_content_budget_is_met() {
    let c = Content::try_embedded().unwrap();
    let patrols = c.cms.cm.iter().filter(|d| d.kind != CmKind::Warden).count();
    assert_eq!(patrols, 20, "20 countermeasures");
    assert_eq!(
        c.cms.cm.iter().filter(|d| d.kind == CmKind::Warden).count(),
        3,
        "3 wardens"
    );
    assert_eq!(c.scripts.script.len(), 60, "60 scripts");
    assert_eq!(c.fragments.fragment.len(), 30, "30 fragments");
    for kind in [FragKind::Effect, FragKind::Target, FragKind::Modifier] {
        assert!(c.fragments.fragment.iter().any(|f| f.kind == kind));
    }
    // Every kind of countermeasure comes in a handful of variants, so a pattern is worth learning.
    for kind in [CmKind::Sentry, CmKind::Tracer, CmKind::Wall] {
        assert!(c.cms.cm.iter().filter(|d| d.kind == kind).count() >= 6);
    }
}

#[test]
fn corporation_styles_point_at_real_tags_and_real_wardens() {
    let c = Content::try_embedded().unwrap();
    for corp in &c.corps.corp {
        assert!(
            !corp.cm_bias.is_empty(),
            "{} has a countermeasure style",
            corp.name
        );
        for tag in &corp.cm_bias {
            assert!(
                c.cms.cm.iter().any(|d| d.tags.contains(tag)),
                "{}: no countermeasure is tagged `{tag}`",
                corp.name
            );
        }
        let w = c.cm(corp.warden).unwrap();
        assert_eq!(w.kind, CmKind::Warden, "{}", corp.name);
    }
    // The styles differ: no two corporations have the same bias.
    for (i, a) in c.corps.corp.iter().enumerate() {
        for b in &c.corps.corp[i + 1..] {
            assert_ne!(a.cm_bias, b.cm_bias, "{} and {}", a.name, b.name);
        }
    }
}

#[test]
fn stock_scripts_are_never_tuned_to_wardens_and_signatures_are_unique() {
    let c = Content::try_embedded().unwrap();
    let mut sigs = std::collections::BTreeSet::new();
    for s in &c.scripts.script {
        assert_ne!(s.target, Some(CmKind::Warden), "{}", s.name);
        assert!(s.pierce <= 2, "{}", s.name);
        assert!(sigs.insert(s.sig), "{} shares a signature", s.name);
        assert_eq!(s.sig, u16::from(s.num));
    }
    // Only the break, jam and purge families carry a tuning or pierce: the rest cannot hurt a thing.
    for s in &c.scripts.script {
        if s.target.is_some() || s.pierce > 0 {
            assert!(
                matches!(
                    s.effect,
                    russh_bbs_door_nullroute::content::Effect::Jam
                        | russh_bbs_door_nullroute::content::Effect::Purge
                ),
                "{}",
                s.name
            );
        }
    }
}

#[test]
fn a_broken_countermeasure_or_fragment_table_is_refused() {
    let broken = |toml: String| content::check::<CmTable>("countermeasures", &toml);
    // A pattern that mends needs a mend amount, and the other way round.
    let bad = COUNTERMEASURES_TOML.replacen("mend = 3", "mend = 0", 1);
    assert!(matches!(broken(bad), Err(ContentError::Invalid { .. })));
    // A duplicate id.
    let dup = COUNTERMEASURES_TOML.replacen("num = 2\n", "num = 1\n", 1);
    assert!(matches!(broken(dup), Err(ContentError::Invalid { .. })));
    // An unknown field is refused, not ignored.
    let extra = COUNTERMEASURES_TOML.replacen("armor = 0", "armour = 0", 1);
    assert!(broken(extra).is_err());
    // A fragment must fill exactly the slot its kind names, and cannot code a utility effect.
    let frag = FRAGMENTS_TOML.replacen("effect = \"crack\"", "effect = \"copy\"", 1);
    assert!(matches!(
        content::check::<FragmentsTable>("fragments", &frag),
        Err(ContentError::Invalid { .. })
    ));
    let slot = FRAGMENTS_TOML.replacen("kind = \"effect\"", "kind = \"modifier\"", 1);
    assert!(matches!(
        content::check::<FragmentsTable>("fragments", &slot),
        Err(ContentError::Invalid { .. })
    ));
}
