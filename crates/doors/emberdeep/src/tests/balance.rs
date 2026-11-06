//! Content budget and branch coverage (M4-E4): the launch budget is met or the shortfall is
//! pinned here; every event, every ending, every verdict and every flag is reachable.

use super::support::*;
use crate::content::Need;
use crate::dungeon;

#[test]
fn the_launch_budget_is_met_or_the_shortfall_is_exactly_this() {
    let c = content();
    let ordinary = c.bestiary.monster.iter().filter(|m| !m.guardian).count();
    let guardians = c.bestiary.monster.iter().filter(|m| m.guardian).count();
    let steps: usize = c.quests.quest.iter().map(|q| q.step.len()).sum();
    let nodes = c.nodes.node.len();
    let weapons_armour = c.gear.weapon.len() + c.gear.armour.len();
    assert!(c.rooms.room.len() >= 60, "room templates");
    assert!(ordinary >= 40 && guardians == 5, "monsters and guardians");
    assert!(c.bestiary.trap.len() >= 30, "traps");
    assert_eq!(c.callings.calling.len(), 6);
    assert_eq!(nodes, 90, "skill nodes");
    assert!(weapons_armour >= 120, "item bases");
    assert!(c.affixes.affix.len() >= 50, "affixes");
    assert!(c.affixes.core.len() >= 20, "ember-cores");
    assert!(c.events.event.len() >= 100, "dungeon events");
    assert!(c.town.contract.len() >= 40, "tavern contracts");
    assert_eq!(c.quests.quest.len(), 3, "questlines");
    assert!(steps >= 30, "quest steps");
    assert!(c.projects.project.len() >= 12, "town projects");
}

#[test]
fn most_events_appear_somewhere_in_the_dungeon_over_a_few_seasons() {
    let c = content();
    let mut seen = std::collections::BTreeSet::new();
    for season in 1..=40u32 {
        for depth in 1..=25u8 {
            let level = dungeon::generate(&c, season, 0, depth);
            for r in &level.rooms {
                if let Some(e) = r.event {
                    seen.insert(e);
                }
            }
        }
    }
    let total = c.events.event.len();
    assert!(seen.len() * 100 >= total * 95, "{}/{total}", seen.len());
}

#[test]
fn every_event_option_can_be_taken_by_some_build_and_every_flag_is_a_real_choice() {
    let c = content();
    let flags: Vec<&str> = c
        .quests
        .quest
        .iter()
        .flat_map(|q| &q.step)
        .flat_map(|s| &s.choose)
        .map(|ch| ch.flag.as_str())
        .collect();
    for e in &c.events.event {
        for o in &e.option {
            // Needs that nothing can satisfy would be dead content.
            for n in &o.need {
                match n {
                    Need::Tag { tag } => {
                        let known = c.callings.calling.iter().any(|k| k.tags.contains(tag))
                            || c.nodes.node.iter().any(|nd| {
                                nd.effects.iter().any(|ef| {
                                    matches!(ef, crate::content::Effect::Tag { tag: t } if t == tag)
                                })
                            })
                            || c.traits.traits.iter().chain(&c.traits.scar).any(|t| {
                                t.effects.iter().any(|ef| {
                                    matches!(ef, crate::content::Effect::Tag { tag: t } if t == tag)
                                })
                            })
                            || c.affixes.core.iter().any(|k| {
                                k.effects.iter().any(|ef| {
                                    matches!(ef, crate::content::Effect::Tag { tag: t } if t == tag)
                                })
                            })
                            || c.wounds.wound.iter().any(|w| {
                                w.effects.iter().any(|ef| {
                                    matches!(ef, crate::content::Effect::Tag { tag: t } if t == tag)
                                })
                            });
                        assert!(
                            known,
                            "event `{}` needs tag `{tag}`, which nothing grants",
                            e.id
                        );
                    }
                    Need::Flag { id } => assert!(flags.contains(&id.as_str())),
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn every_quest_flag_pair_closes_a_door() {
    // Choices close doors: each line's final choice sets a flag that a room option or the
    // verdict reads, and its alternatives do not.
    let c = content();
    for q in &c.quests.quest {
        let last = q.step.last().unwrap();
        assert!(last.choose.len() >= 2, "{}", q.id);
        let flags: Vec<&str> = last.choose.iter().map(|ch| ch.flag.as_str()).collect();
        let read: usize = flags
            .iter()
            .filter(|f| {
                crate::endings::verdict_flags().any(|v| v == **f)
                    || c.events.event.iter().any(|e| {
                        e.option.iter().any(|o| {
                            o.need.iter().any(|n| matches!(n, Need::Flag { id } | Need::NoFlag { id } if id == **f))
                        })
                    })
            })
            .count();
        assert!(read >= 1, "line {} has no flag anything reads", q.id);
    }
}

/// Endings under uniform-random inputs: every collective ending shows up. (The season
/// simulation in `tests/sim.rs` reports the endings the bots actually reach: none, because no
/// bot felled the Cold Furnace in a season.)
#[test]
fn the_endings_are_all_reachable_under_random_seasons() {
    use crate::endings::{VERDICTS, collective};
    use russh_bbs_door::DoorRng;
    let c = content();
    let mut rng = DoorRng::from_seed(11);
    let mut tally = [0u32; 6];
    for _ in 0..4000 {
        let mut w = crate::world::World::new(1);
        let v = &VERDICTS[usize::try_from(rng.below(4)).unwrap()];
        w.commons.verdict = Some((v.id.to_owned(), "X".to_owned()));
        w.commons.rescues = u16::try_from(rng.below(12)).unwrap();
        if rng.below(2) == 0 {
            w.commons.parties.push(crate::social::Party {
                leader: "A".into(),
                members: vec!["A".into(), "B".into()],
                formed: 0,
                day: 0,
                cleared: vec![],
                rescues: 1,
                deepest: 0,
            });
        }
        let done = usize::try_from(rng.below(13)).unwrap();
        for p in c.projects.project.iter().take(done) {
            finish(&mut w, &p.id);
        }
        for d in 1..=u8::try_from(rng.below(10)).unwrap() {
            w.apply(&crate::world::Change::Shortcut(
                d,
                crate::content::ShortcutKind::Rope,
                "X".into(),
            ));
        }
        tally[usize::from(collective(&w, &c))] += 1;
    }
    assert!(tally.iter().all(|n| *n > 40), "{tally:?}");
}
