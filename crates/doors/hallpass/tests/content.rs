//! Content is data: the schema tests, the tone rules, and breaking each table on purpose.
#![allow(
    clippy::cast_precision_loss,
    clippy::needless_pass_by_value,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::type_complexity,
    clippy::collapsible_if,
    clippy::cast_possible_wrap,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::print_stderr,
    clippy::panic
)]

use russh_bbs_door::content::{ContentError, plain_text};
use russh_bbs_door_hallpass::content::{Content, EMBEDDED, Sources};

fn broken(
    edit: impl FnOnce(&mut Sources<'static>, &mut Vec<String>),
) -> Result<Content, ContentError> {
    let mut sources = Sources { ..EMBEDDED };
    let mut owned: Vec<String> = Vec::new();
    edit(&mut sources, &mut owned);
    Content::from_sources(&sources)
}

fn replace(text: &str, from: &str, to: &str) -> &'static str {
    assert!(text.contains(from), "the test's edit must hit: {from}");
    Box::leak(text.replacen(from, to, 1).into_boxed_str())
}

fn problems(result: Result<Content, ContentError>) -> String {
    match result {
        Err(ContentError::Invalid { problems, .. }) => problems.join("; "),
        Err(other) => other.to_string(),
        Ok(_) => String::new(),
    }
}

#[test]
fn the_shipped_content_loads_and_has_the_amount_this_slice_promises() {
    let c = Content::embedded().unwrap();
    assert_eq!(c.problems(), Vec::<String>::new());
    assert_eq!(c.periods.len(), 9);
    assert_eq!(c.subjects.len(), 5);
    assert_eq!(c.cliques.len(), 6);
    assert!(c.places.len() >= 25, "{}", c.places.len());
    assert!(c.npcs.len() >= 18, "{}", c.npcs.len());
    assert!(c.methods.iter().filter(|m| !m.big).count() >= 9);
    assert_eq!(
        c.methods.iter().filter(|m| m.big).count(),
        4,
        "one big scheme per world effect"
    );
    assert_eq!(c.effects.len(), 4);
    assert!(c.twists.len() >= 6 && c.getaways.len() >= 6);
    assert!(c.endings.len() >= 5);
    // Every big scheme names an effect, and every effect is used by one.
    for e in &c.effects {
        assert!(
            c.methods
                .iter()
                .any(|m| m.effect.as_deref() == Some(e.id.as_str())),
            "{} unused",
            e.id
        );
    }
}

#[test]
fn the_tone_rules_hold_mischief_against_authority_and_rivals_never_the_vulnerable() {
    let c = Content::embedded().unwrap();
    for t in &c.targets {
        assert_ne!(t.clique.as_deref(), Some("strays"), "{} is a Stray", t.id);
        assert!(
            ["staff", "prefect", "clique", "school"].contains(&t.kind.as_str()),
            "{}",
            t.id
        );
        if let Some(npc) = t.npc.as_deref().and_then(|n| c.npc(n)) {
            assert!(
                npc.is_authority() && npc.prankable,
                "{} is not authority",
                npc.id
            );
        }
    }
    // No student can ever be a target on their own, and the Strays are all helpers.
    assert!(c.targets.iter().all(|t| {
        t.npc
            .as_deref()
            .and_then(|n| c.npc(n))
            .is_none_or(|n| n.role != "student")
    }));
    for n in c
        .npcs
        .iter()
        .filter(|n| n.clique.as_deref() == Some("strays"))
    {
        assert!(
            !n.prankable && !n.ally.is_empty(),
            "{} should be an ally",
            n.id
        );
    }
    // There is no way to name a target in text: targets are ids from this table, never typed.
    assert!(
        c.targets
            .iter()
            .all(|t| !t.id.is_empty() && plain_text(&t.name))
    );
    // No text in the tables contains anything the board's sanitizer would strip.
    let texts: Vec<&str> = c
        .methods
        .iter()
        .flat_map(|m| [m.name.as_str(), m.blurb.as_str(), m.story.as_str()])
        .chain(c.mishaps.iter().map(|m| m.text.as_str()))
        .chain(c.lines.iter().map(|l| l.text.as_str()))
        .chain(c.effects.iter().map(|e| e.headline.as_str()))
        .chain(c.endings.iter().map(|e| e.text.as_str()))
        .collect();
    assert!(texts.iter().all(|t| plain_text(t)));
}

#[test]
fn the_bad_edits_a_writer_could_make_are_each_named() {
    // An unknown field.
    assert!(matches!(
        broken(|s, _| s.rules = replace(
            EMBEDDED.rules,
            "term_days = 42",
            "term_days = 42\nterms = 3"
        )),
        Err(ContentError::Parse { .. })
    ));
    // A condition that does not parse.
    assert!(matches!(
        broken(|s, _| s.places = replace(EMBEDDED.places, "know:staff_rota", "knows:staff_rota")),
        Err(ContentError::Parse { .. })
    ));
    // A reference that does not resolve.
    assert!(
        problems(broken(|s, _| s.places = replace(
            EMBEDDED.places,
            "exits = [\"common_room\", \"quad\"]",
            "exits = [\"common_room\", \"nowhere\"]"
        )))
        .contains("nowhere")
    );
    assert!(
        problems(broken(|s, _| s.schemes = replace(
            EMBEDDED.schemes,
            "needs = [\"salt\"]",
            "needs = [\"unobtainium\"]"
        )))
        .contains("unobtainium")
    );
    assert!(
        problems(broken(|s, _| s.schemes = replace(
            EMBEDDED.schemes,
            "skills = [\"sneak\", \"bluff\", \"fleet\"]\nstory = \"{target} took one sip",
            "skills = [\"sneak\", \"charm\", \"fleet\"]\nstory = \"{target} took one sip"
        )))
        .contains("charm")
    );
    // Duplicate ids.
    assert!(
        problems(broken(
            |s, _| s.items = replace(EMBEDDED.items, "id = \"balls\"", "id = \"salt\"")
        ))
        .contains("used twice")
    );
    // Text the sanitizer would change.
    assert!(
        problems(broken(
            |s, _| s.world = replace(EMBEDDED.world, "Expelled\"", "\\u001b[31mExpelled\"")
        ))
        .contains("strips")
    );
    // A timetable that is the wrong length.
    assert!(
        problems(broken(|s, _| s.timetable = replace(
            EMBEDDED.timetable,
            "\"night\"]\n[[day]]\nweekday = 1",
            "]\n[[day]]\nweekday = 1"
        )))
        .contains("slots")
    );
    // A lock with no key.
    assert!(
        problems(broken(|s, _| s.places = replace(
            EMBEDDED.places,
            "unlock = [\"item:master_key\"]\n\n[[place]]\nid = \"deputy_office\"",
            "unlock = []\n\n[[place]]\nid = \"deputy_office\""
        )))
        .contains("lock")
    );
    // A big scheme without an effect, and an effect nobody asked for.
    assert!(
        problems(broken(
            |s, _| s.schemes = replace(EMBEDDED.schemes, "effect = \"tuck_shop_closed\"\n", "")
        ))
        .contains("effect")
    );
    // A method that needs a helper nobody offers.
    assert!(
        problems(broken(
            |s, _| s.cast = replace(EMBEDDED.cast, "role = \"gadget\"", "role = \"cover\"")
        ))
        .contains("gadget")
    );
}

#[test]
fn the_tone_check_refuses_a_stray_target() {
    let stray = broken(|s, _| {
        s.schemes = replace(
            EMBEDDED.schemes,
            "[[target]]\nid = \"the_bell\"",
            "[[target]]\nid = \"strays\"\nname = \"the Strays\"\nkind = \"clique\"\nclique = \"strays\"\n[[target]]\nid = \"the_bell\"",
        );
    });
    assert!(problems(stray).contains("Strays are allies, never targets"));
}

#[test]
fn a_prefect_or_student_who_is_not_prankable_cannot_be_targeted() {
    let bad = broken(|s, _| {
        s.schemes = replace(
            EMBEDDED.schemes,
            "[[target]]\nid = \"the_bell\"",
            "[[target]]\nid = \"hollis\"\nname = \"Rafe\"\nkind = \"prefect\"\nnpc = \"hollis\"\n[[target]]\nid = \"the_bell\"",
        );
    });
    assert!(problems(bad).contains("must be a prankable staff member or prefect"));
}
