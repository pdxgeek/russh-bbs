//! Schema tests for the content tables: they load, they validate, and a broken edit is refused.
#![allow(clippy::unwrap_used)]

use russh_bbs_door::content::{self, ContentError};
use russh_bbs_door_nullroute::content::{
    CONTRACTS_TOML, CORPS_TOML, Content, ContractsTable, CorpsTable, NODES_TOML, NodesTable,
    RIG_TOML, RigTable, Role, SCRIPTS_TOML, ScriptsTable,
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
    assert!(Content::try_embedded().is_ok());
}

#[test]
fn the_shipped_content_covers_what_this_slice_promises() {
    let c = Content::try_embedded().unwrap();
    assert!(
        c.corps.corp.len() >= 5,
        "five corporations, four+ distinct styles"
    );
    assert_eq!(c.corps.fixer.len(), 3);
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
