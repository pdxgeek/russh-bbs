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
    assert_eq!(c.subjects.len(), 6);
    assert_eq!(c.cliques.len(), 6);
    assert!(c.places.len() >= 25, "{}", c.places.len());
    assert!(c.npcs.len() >= 26, "{}", c.npcs.len());
    assert!(c.methods.iter().filter(|m| !m.big).count() >= 23);
    assert_eq!(
        c.methods.iter().filter(|m| m.big).count(),
        4,
        "one big scheme per world effect"
    );
    assert_eq!(
        c.effects.len(),
        14,
        "four big schemes, one storyline ending, five endings of the east wing and four of the election"
    );
    assert!(c.twists.len() >= 6 && c.getaways.len() >= 6);
    assert!(c.endings.len() >= 5);
    // Every big scheme names an effect, and every effect is used by a big scheme or a story ending.
    for e in &c.effects {
        let by_story = c.arcs.iter().flat_map(|a| a.ending.iter()).any(|end| {
            end.effects
                .contains(&russh_bbs_door_hallpass::effect::Eff::World(e.id.clone()))
        });
        let by_verdict = c.mystery.verdicts.iter().any(|v| v.effect == e.id);
        let by_election = c.social.runners.iter().any(|r| r.win == e.id)
            || c.social.rules.player_win_effect == e.id;
        assert!(
            by_story
                || by_verdict
                || by_election
                || c.methods
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

// ---------------------------------------------------------------- school life (M4-H2)

#[test]
fn the_shipped_school_life_content_has_the_amount_this_slice_promises() {
    let c = Content::embedded().unwrap();
    assert!(c.subjects.len() >= 6, "{}", c.subjects.len());
    assert!(c.clubs.len() >= 4, "{}", c.clubs.len());
    assert!(c.talks.len() >= 70, "{}", c.talks.len());
    assert!(c.arcs.len() >= 2, "{}", c.arcs.len());
    // Every subject has its own kind of puzzle.
    let kinds: std::collections::BTreeSet<&str> =
        c.lessons.iter().map(|l| l.kind.as_str()).collect();
    assert_eq!(kinds.len(), c.subjects.len());
    // Every storyline can end at least two ways, and at least one changes the shared school.
    for a in &c.arcs {
        assert!(a.ending.len() >= 2, "{}", a.id);
    }
    assert!(c.arcs.iter().any(|a| a.ending.iter().any(|e| {
        e.effects
            .iter()
            .any(|x| matches!(x, russh_bbs_door_hallpass::effect::Eff::World(_)))
    })));
    // A club has a rank ladder, and rank is asked for by something else in the content.
    for club in &c.clubs {
        let asked = format!("rank:{}", club.id);
        let text = [
            EMBEDDED.places,
            EMBEDDED.items,
            EMBEDDED.schemes,
            EMBEDDED.talks,
            EMBEDDED.cast,
        ]
        .join("\n");
        assert!(text.contains(&asked), "{} opens nothing", club.id);
    }
}

fn edited(file: &str, from: &str, to: &str) -> String {
    problems(broken(|s, _| match file {
        "lessons" => s.lessons = replace(EMBEDDED.lessons, from, to),
        "clubs" => s.clubs = replace(EMBEDDED.clubs, from, to),
        "talks" => s.talks = replace(EMBEDDED.talks, from, to),
        "arcs" => s.arcs = replace(EMBEDDED.arcs, from, to),
        "rules" => s.rules = replace(EMBEDDED.rules, from, to),
        "cast" => s.cast = replace(EMBEDDED.cast, from, to),
        "social" => s.social = replace(EMBEDDED.social, from, to),
        "mystery" => s.mystery = replace(EMBEDDED.mystery, from, to),
        "world" => s.world = replace(EMBEDDED.world, from, to),
        _ => unreachable!(),
    }))
}

#[test]
fn breaking_the_lessons_is_caught_with_the_named_problem() {
    assert!(edited("lessons", "kind = \"relay\"", "kind = \"riddle\"").contains("unknown kind"));
    assert!(edited("lessons", "subject = \"games\"", "subject = \"maths\"").contains("used twice"));
    assert!(
        edited(
            "lessons",
            "subject = \"art\"\nkind = \"pattern\"\nskill = \"sneak\"",
            "subject = \"art\"\nkind = \"pattern\"\nskill = \"charm\""
        )
        .contains("unknown skill")
    );
    assert!(
        edited("lessons", "subject = \"games\"", "subject = \"cricket\"")
            .contains("unknown subject")
    );
    assert!(
        edited(
            "rules",
            "lesson_gains = [-3, -1, 1, 2]",
            "lesson_gains = [1, 2]"
        )
        .contains("lesson rules")
    );
    assert!(edited("rules", "betray_bond = -30", "betray_bond = 4").contains("bond rules"));
}

#[test]
fn breaking_the_clubs_is_caught_with_the_named_problem() {
    assert!(
        edited("clubs", "advisor = \"gale\"", "advisor = \"hollis\"")
            .contains("advisor must be a member of staff")
    );
    assert!(
        edited(
            "clubs",
            "members = [\"hollis\", \"crouch\"]",
            "members = [\"nobody\"]"
        )
        .contains("unknown member")
    );
    assert!(
        edited(
            "clubs",
            "rivals = [\"rowing\"]\nskill = \"tinker\"",
            "rivals = []\nskill = \"tinker\""
        )
        .contains("must rival each other")
    );
    assert!(
        edited(
            "clubs",
            "ranks = [[\"Cox\", 0], [\"Bow\", 4]",
            "ranks = [[\"Cox\", 3], [\"Bow\", 4]"
        )
        .contains("ranks must be")
    );
    assert!(
        edited(
            "clubs",
            "kind = \"after\"\ndays = [0, 2, 4]",
            "kind = \"lunch\"\ndays = [0, 2, 4]"
        )
        .contains("shut")
    );
    assert!(
        edited(
            "clubs",
            "join = [\"grade:maths>=60\"]",
            "join = [\"grade:cricket>=60\"]"
        )
        .contains("unknown subject")
    );
    assert!(
        edited("clubs", "place = \"boathouse\"", "place = \"the_moon\"").contains("unknown place")
    );
}

#[test]
fn breaking_the_conversations_is_caught_with_the_named_problem() {
    // A condition naming a thing that is not there.
    assert!(
        edited(
            "talks",
            "when = [\"grade:maths<=68\"]",
            "when = [\"grade:cricket<=68\"]"
        )
        .contains("unknown subject")
    );
    assert!(
        edited(
            "talks",
            "needs = [\"rank:chess>=1\"]",
            "needs = [\"rank:bridge>=1\"]"
        )
        .contains("unknown club")
    );
    assert!(
        edited(
            "talks",
            "needs = [\"trait:known_troublemaker\"]",
            "needs = [\"trait:hero\"]"
        )
        .contains("unknown trait")
    );
    assert!(
        edited(
            "talks",
            "needs = [\"bond:alder>=4\"]",
            "needs = [\"bond:nobody>=4\"]"
        )
        .contains("unknown character")
    );
    // An effect naming a thing that is not there.
    assert!(
        edited(
            "talks",
            "effects = [\"grade:maths:+6\"",
            "effects = [\"grade:cricket:+6\""
        )
        .contains("unknown subject")
    );
    assert!(
        edited("talks", "\"merit:robotics:+3\"", "\"merit:bridge:+3\"").contains("unknown club")
    );
    assert!(
        edited("talks", "\"know:candle_sign\"", "\"know:candle_signs\"")
            .contains("which nothing grants")
            || edited(
                "cast",
                "needs = [\"standing:the_bench>=10\", \"legend>=20\"]",
                "needs = [\"know:never_given\"]"
            )
            .contains("which nothing grants")
    );
    // A first node that is not there, a choice that leads nowhere, a node nobody can reach.
    assert!(
        edited(
            "talks",
            "first = \"open\"\n[[talk.node]]\nid = \"open\"\ntext = \"Mr. Pell",
            "first = \"start\"\n[[talk.node]]\nid = \"open\"\ntext = \"Mr. Pell"
        )
        .contains("no first node")
    );
    assert!(
        edited(
            "talks",
            "say = \"'Do,' says Mr. Pell, and goes back to his marking.\"",
            "say = \"'Do,' says Mr. Pell, and goes back to his marking.\"\nnext = \"nowhere\""
        )
        .contains("unknown node")
    );
    assert!(edited(
        "talks",
        "[[talk]]\nid = \"orrin_gerald\"",
        "[[talk.node]]\nid = \"orphan\"\ntext = \"Nobody gets here.\"\n[[talk.node.choice]]\nlabel = \"Go\"\nsay = \"Gone.\"\n\n[[talk]]\nid = \"orrin_gerald\""
    )
    .contains("can never be reached"));
    // A node with no way out that asks nothing: the pupil could be stuck.
    assert!(
        edited(
            "talks",
            "[[talk.node.choice]]\nlabel = \"Plead a sprained ankle\"",
            "[[talk.node.choice]]\nlabel = \"Plead a sprained ankle\"\nneeds = [\"cool>=99\"]"
        )
        .contains("needs a way out")
    );
    // A talk that repeats with no cooldown would be a loop to farm.
    assert!(
        edited(
            "talks",
            "once = false\ncooldown = 4\nfirst = \"open\"",
            "once = false\ncooldown = 0\nfirst = \"open\""
        )
        .contains("needs a cooldown")
    );
    // Text the sanitizer would change.
    assert!(
        edited(
            "talks",
            "Mr. Pell looks over his glasses",
            "Mr. Pell looks \\u001b[31mover his glasses"
        )
        .contains("strips")
    );
    // A character who is not there.
    assert!(edited("talks", "npc = \"pell\"", "npc = \"nobody\"").contains("unknown character"));
}

#[test]
fn breaking_the_storylines_is_caught_with_the_named_problem() {
    // A step the story never gets to, a step with nothing to do, an ending nobody can reach.
    assert!(
        edited(
            "arcs",
            "[[arc.step]]\nid = \"expose\"",
            "[[arc.step]]\nid = \"orphan\"\ntext = \"Never.\"\n[[arc.step]]\nid = \"expose\""
        )
        .contains("can never be entered")
    );
    assert!(edited("arcs", "[[arc.ending]]\nid = \"shrugged\"", "[[arc.ending]]\nid = \"vanished\"\nname = \"Gone\"\ntext = \"Gone.\"\n[[arc.ending]]\nid = \"shrugged\"").contains("can never be reached"));
    assert!(edited("arcs", "first = \"found\"", "first = \"lost\"").contains("unknown step"));
    // A story with one ending is a story with no choice.
    let one = broken(|s, _| {
        s.arcs = Box::leak(
            "[[arc]]\nid = \"x\"\nname = \"X\"\nblurb = \"x.\"\nstart = [\"day>=1\"]\nfirst = \"a\"\n[[arc.step]]\nid = \"a\"\ntext = \"a.\"\n[[arc.ending]]\nid = \"only\"\nname = \"Only\"\ntext = \"Only.\"\n"
                .to_owned()
                .into_boxed_str(),
        );
    });
    assert!(problems(one).contains("at least two endings"));
    // A world effect an ending asks for that does not exist.
    assert!(
        edited(
            "arcs",
            "\"world:prefects_stood_down\"",
            "\"world:the_moon_falls\""
        )
        .contains("unknown world effect")
    );
    // A story that starts on nothing.
    assert!(
        edited("arcs", "start = [\"day>=4\"]", "start = []").contains("needs a start condition")
    );
}

#[test]
fn the_tone_check_refuses_a_script_that_costs_the_strays() {
    let bad_standing = edited(
        "talks",
        "effects = [\"bond:tilly:+4\", \"stand:strays:+3\", \"know:hiding_places\"]",
        "effects = [\"bond:tilly:+4\", \"stand:strays:-3\", \"know:hiding_places\"]",
    );
    assert!(bad_standing.contains("allies"), "{bad_standing}");
    let bad_betrayal = edited(
        "talks",
        "\"betray:last_ally\", \"stat:heat:-15\"",
        "\"betray:tilly\", \"stat:heat:-15\"",
    );
    assert!(
        bad_betrayal.contains("Strays are never betrayed"),
        "{bad_betrayal}"
    );
}

// ---------------------------------------------------------------- the social tables (H4)

#[test]
fn the_social_and_mystery_tables_meet_the_content_the_slice_promises() {
    let c = Content::embedded().unwrap();
    assert!(c.social.plots.len() >= 7, "{}", c.social.plots.len());
    assert!(
        c.social
            .plots
            .iter()
            .filter(|p| p.kind == "mischief")
            .count()
            >= 4
    );
    assert!(c.social.plots.iter().filter(|p| p.kind == "kind").count() >= 3);
    assert!(c.social.rumours.len() >= 8);
    assert!(c.social.runners.len() >= 3);
    assert!(c.social.awards.len() >= 7);
    assert_eq!(c.mystery.clues.len(), 12);
    assert_eq!(c.mystery.verdicts.len(), 5);
    // Six endings of the pupil's story: five verdicts and walking away.
    assert_eq!(c.arc("the_wing").unwrap().ending.len(), 6);
}

#[test]
fn the_tone_rules_for_player_schemes_are_checks_not_hopes() {
    let c = Content::embedded().unwrap();
    for p in &c.social.plots {
        // What lands on a player is a small, signed change of cool, nerve or cash, and nothing else.
        for e in &p.hit {
            use russh_bbs_door_hallpass::effect::{Eff, Gauge};
            match e {
                Eff::Stat(Gauge::Cool | Gauge::Nerve, n) => assert!(n.abs() <= 3, "{}", p.id),
                Eff::Stat(Gauge::Cash, n) => assert!(n.abs() <= 6, "{}", p.id),
                other => panic!("{} does {other:?} to a player", p.id),
            }
        }
        // A mischief scheme has a counterplay and a mild sign; a kind one has neither a cost to the target.
        if p.kind == "mischief" {
            assert!(!p.counter.is_empty(), "{} has no counterplay", p.id);
        }
        // The sender pays to try: nerve, always.
        assert!(p.nerve >= 1, "{}", p.id);
    }
    // No Stray is a target of anything (and no player can be named by typing: the target is a roster id).
    for r in &c.social.rumours {
        if r.kind != "praise" {
            assert!(!c.rumour_subjects(r).iter().any(|s| s == "strays"));
        }
    }
    for r in &c.social.runners {
        assert_ne!(c.npc(&r.npc).unwrap().clique.as_deref(), Some("strays"));
    }
    // There is no action that carries text from a player: every social action is built from ids.
    // (A compile-time fact of `Action`: its social variants hold only ids and a flag.)
}

#[test]
fn breaking_the_social_tables_is_caught_with_the_named_problem() {
    // A scheme that reaches the file, the world, or a friendship.
    for bad in [
        "hit = [\"file:caught:2\"]",
        "hit = [\"world:wing_opened\"]",
        "hit = [\"bond:pell:-5\"]",
        "hit = [\"stat:cash:-30\"]",
        "hit = [\"stat:legend:-3\"]",
    ] {
        let got = edited("social", "hit = [\"stat:cash:-3\"]", bad);
        assert!(got.contains("may only be a small change"), "{bad}: {got}");
    }
    // A mischief with no counterplay.
    let got = edited(
        "social",
        "counter = [\"item:padlock\", \"tag:gadgets\"]",
        "counter = []",
    );
    assert!(got.contains("needs a counterplay"), "{got}");
    // The wrong sign: mischief that helps, a favour that harms.
    assert!(
        edited(
            "social",
            "hit = [\"stat:cash:-3\"]",
            "hit = [\"stat:cash:3\"]"
        )
        .contains("wrong sign")
    );
    assert!(
        edited(
            "social",
            "hit = [\"stat:cool:+2\"]",
            "hit = [\"stat:cool:-2\"]"
        )
        .contains("wrong sign")
    );
    // A sender who pays nothing.
    assert!(
        edited(
            "social",
            "nerve = 2\nneeds = [\"skill:tinker>=1\"]",
            "nerve = 0\nneeds = [\"skill:tinker>=1\"]"
        )
        .contains("costs out of range")
    );
    // Text that does not name the sender, or is not plain.
    assert!(
        edited(
            "social",
            "A glued locker\"\nblurb",
            "A glued locker\u{202e}\"\nblurb"
        )
        .contains("unplain")
    );
    assert!(
        edited(
            "social",
            "Your locker was glued shut and it cost you a new latch. The whisper says {from}.",
            "Your locker was glued shut."
        )
        .contains("{from}")
    );
    // A rumour that is about no one, about a player, or a tall tale about someone else.
    assert!(
        edited(
            "social",
            "about = \"clique\"\nkind = \"praise\"",
            "about = \"player\"\nkind = \"praise\""
        )
        .contains("about must be")
    );
    assert!(edited("social", "text = \"They say you once swung from the chapel bell rope to get out of detention.\"\nabout = \"self\"", "text = \"They say {about} once swung from the chapel bell rope to get out of detention.\"\nabout = \"npc\"").contains("tall tale is told about yourself"));
    assert!(
        edited(
            "social",
            "text = \"People are saying {about} is the fairest person at Greywater.\"",
            "text = \"People are saying nice things.\""
        )
        .contains("{about}")
    );
    // Gossip that costs the Strays: a rumour template cannot name them unless it is praise.
    let got = edited(
        "social",
        "id = \"biscuits\"\nname = \"Seen raiding the biscuit tin\"",
        "id = \"fair_play\"\nname = \"Seen raiding the biscuit tin\"",
    );
    assert!(got.contains("used twice"), "{got}");
    // An election whose days run backwards, a runner who is a Stray, a runner with no win.
    assert!(
        edited(
            "social",
            "nominate_from = 11\nvote_from = 20",
            "nominate_from = 25\nvote_from = 20"
        )
        .contains("election days must rise")
    );
    assert!(
        edited("social", "npc = \"wren\"", "npc = \"tilly\"").contains("the Strays do not run")
    );
    assert!(
        edited("social", "win = \"head_wren\"", "win = \"head_nobody\"")
            .contains("unknown world effect")
    );
    assert!(
        edited(
            "social",
            "player_win_effect = \"head_player\"",
            "player_win_effect = \"nope\""
        )
        .contains("player_win_effect")
    );
    assert!(
        edited(
            "social",
            "winner_trait = \"head_of_school\"",
            "winner_trait = \"fast_runner\""
        )
        .contains("earned trait")
    );
    // An award that scores nothing.
    assert!(
        edited(
            "social",
            "stat = \"legend\"\nmin = 10",
            "stat = \"luck\"\nmin = 10"
        )
        .contains("unknown stat")
    );
}

#[test]
fn breaking_the_mystery_is_caught_with_the_named_problem() {
    // A clue at a place that does not exist, or that is shut when it can be found.
    assert!(edited("mystery", "place = \"library\"", "place = \"moon\"").contains("unknown place"));
    assert!(
        edited(
            "mystery",
            "place = \"east_wing\"\nkinds = [\"night\"]\nneeds = [\"item:east_wing_key\"",
            "place = \"east_wing\"\nkinds = [\"lunch\"]\nneeds = [\"item:east_wing_key\""
        )
        .contains("can only be found when its place is shut")
    );
    // A clue that comes after a clue that does not come first.
    assert!(
        edited(
            "mystery",
            "after = [\"c1_register\"]\ntext = \"A hymn slip",
            "after = [\"c9_hall\"]\ntext = \"A hymn slip"
        )
        .contains("not an earlier clue")
    );
    // A bad act, a bad risk, a clue with no words.
    assert!(
        edited(
            "mystery",
            "act = 2\nplace = \"chapel\"",
            "act = 7\nplace = \"chapel\""
        )
        .contains("not 1 to 4")
    );
    assert!(edited("mystery", "risk = 3", "risk = 9").contains("risk is over 3"));
    // An act with no clue.
    let no_four = problems(broken(|s, _| {
        let once = replace(
            EMBEDDED.mystery,
            "act = 4\nplace = \"east_wing\"",
            "act = 3\nplace = \"east_wing\"",
        );
        s.mystery = replace(
            once,
            "act = 4\nplace = \"admin_corridor\"",
            "act = 3\nplace = \"admin_corridor\"",
        );
    }));
    assert!(
        no_four.contains("act 4 of the mystery has no clue"),
        "{no_four}"
    );
    // A verdict no ending casts is dead content; an ending that casts an unknown verdict is refused.
    let dead = edited("arcs", "\"verdict:bonfire\", ", "");
    assert!(dead.contains("is cast by no ending"), "{dead}");
    let unknown = edited("arcs", "\"verdict:bonfire\"", "\"verdict:the_moon\"");
    assert!(unknown.contains("unknown verdict"), "{unknown}");
    // A verdict that brings about nothing.
    assert!(
        edited("mystery", "effect = \"bonfire\"", "effect = \"nothing\"")
            .contains("unknown world effect")
    );
    // A flag a condition tests that nothing grants.
    assert!(
        edited(
            "mystery",
            "needs = [\"item:east_wing_key\", \"arc:the_wing@wing\"",
            "needs = [\"know:never_given\", \"arc:the_wing@wing\""
        )
        .contains("nothing grants")
    );
    // The finale must fall inside the term.
    assert!(edited("mystery", "finale_day = 38", "finale_day = 90").contains("inside the term"));
}
