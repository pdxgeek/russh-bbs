//! The schema test for the embedded tables, and a few promises the engine relies on.
#![allow(clippy::unwrap_used)]

use russh_bbs_door_long_haul::content::{Content, GoodClass, TABLES};

#[test]
fn every_embedded_table_loads_and_validates() {
    let content = Content::embedded().unwrap();
    assert_eq!(TABLES.len(), 13);
    assert_eq!(content.goods.len(), 30);
    assert_eq!(content.economies.len(), 6);
    assert_eq!(content.hulls.len(), 4);
    assert_eq!(content.backgrounds.len(), 4);
    // The launch budget (docs/doors/long-haul.md, "Content budget").
    assert_eq!(content.news.len(), 20);
    assert_eq!(content.modules.len(), 50);
    assert_eq!(content.transit.event.len() - content.arcs.event.len(), 120);
    assert_eq!(content.derelicts.derelict.len(), 40);
    assert_eq!(content.enemies.enemy.len(), 15);
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

mod l2_schema {
    use russh_bbs_door::content::check;
    use russh_bbs_door_long_haul::boarding::DerelictTable;
    use russh_bbs_door_long_haul::combat::EnemyTable;
    use russh_bbs_door_long_haul::contracts::JobTable;
    use russh_bbs_door_long_haul::crew::CrewTable;
    use russh_bbs_door_long_haul::transit::TransitTable;

    fn problems<T: russh_bbs_door::content::Table>(name: &'static str, text: &str) -> String {
        check::<T>(name, text).unwrap_err().to_string()
    }

    #[test]
    fn the_shipped_tables_pass_their_own_schemas() {
        use russh_bbs_door_long_haul::content::TABLES;
        let text = |name: &str| TABLES.iter().find(|(n, _)| *n == name).unwrap().1;
        assert_eq!(check::<CrewTable>("crew", text("crew")), Ok(()));
        assert_eq!(check::<TransitTable>("transit", text("transit")), Ok(()));
        assert_eq!(check::<EnemyTable>("enemies", text("enemies")), Ok(()));
        assert_eq!(
            check::<DerelictTable>("derelicts", text("derelicts")),
            Ok(())
        );
        assert_eq!(check::<JobTable>("jobs", text("jobs")), Ok(()));
    }

    #[test]
    fn an_event_with_no_open_option_is_rejected() {
        let bad = r#"
flags = ["a"]
[[event]]
id = "trap"
family = "omen"
title = "A trap"
text = "No way out."
weight = 3
[[event.options]]
label = "Only for experts"
req = [{ kind = "crew", min = 3 }]
needs = "three crew"
[[event.options.outcomes]]
weight = 1
text = "Fine."
"#;
        let msg = problems::<TransitTable>("transit", bad);
        assert!(msg.contains("an option anyone can take"), "{msg}");
    }

    #[test]
    fn unknown_flags_families_and_requirement_words_are_rejected() {
        let bad = r#"
flags = ["a"]
[[event]]
id = "odd"
family = "weather"
title = "Odd"
text = "Odd."
weight = 3
when = [{ kind = "flag", name = "zzz" }]
[[event.options]]
label = "Go"
[[event.options.outcomes]]
weight = 1
text = "Gone."
effects = [{ kind = "flag", name = "nope" }]
"#;
        let msg = problems::<TransitTable>("transit", bad);
        assert!(msg.contains("unknown family"), "{msg}");
        assert!(msg.contains("unknown flag `zzz`"), "{msg}");
        assert!(msg.contains("unknown flag `nope`"), "{msg}");
        // A kind of requirement the engine does not have is a parse error, not a silent no-op.
        let typo = bad.replace("kind = \"flag\", name = \"zzz\"", "kind = \"moon_phase\"");
        assert!(check::<TransitTable>("transit", &typo).is_err());
    }

    #[test]
    fn a_room_nobody_can_reach_and_a_lockless_airlock_rule_are_rejected() {
        let bad = r#"
[[derelict]]
id = "maze"
name = "Maze"
blurb = "x"
foe_name = "Things"
foe_hp = 5
foe_dmg = 2
rooms = [
  { name = "Airlock", text = "a", foes = 1, exits = [1] },
  { name = "Hall", text = "b", exits = [0] },
  { name = "Island", text = "c", exits = [2] },
]
"#;
        let msg = problems::<DerelictTable>("derelicts", bad);
        assert!(msg.contains("airlock must be open"), "{msg}");
        assert!(msg.contains("cannot be reached"), "{msg}");
    }

    #[test]
    fn enemy_stats_out_of_range_and_missing_roles_are_rejected() {
        let bad = r#"
[[enemy]]
id = "titan"
name = "Titan"
faction = "free"
blurb = "Too big."
hull = 5000
shield = 0
speed = 9
accuracy = 99
damage = 99
reach = "mid"
style = "rusher"
feint_pm = 0
boarders = 0
flee_pct = 0
loot = [10, 5]
kill_standing = 5
"#;
        let msg = problems::<EnemyTable>("enemies", bad);
        assert!(
            msg.contains("a stat is out of range") || msg.contains("between 8 and 40"),
            "{msg}"
        );
        let crew = "first = [\"A\"]\nlast = [\"B\"]\ngoal = []\nrole = []\n";
        let msg = problems::<CrewTable>("crew", crew);
        assert!(msg.contains("must have exactly one entry"), "{msg}");
    }

    #[test]
    fn every_faction_and_job_pair_needs_a_client_line() {
        let bad = "passenger = []\nline = []\n";
        let msg = problems::<JobTable>("jobs", bad);
        assert!(msg.contains("no client line for"), "{msg}");
    }
}
