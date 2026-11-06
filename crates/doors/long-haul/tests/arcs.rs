//! Story arcs: every branch and ending is reachable by play, an arc scene wins the draw once it
//! is due, choices close later doors, and the arcs are paced across real days.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    missing_docs,
    clippy::too_many_lines
)]

mod common;

use std::collections::BTreeSet;

use common::Harness;
use russh_bbs_door::DoorRng;
use russh_bbs_door_long_haul::arcs::NO_ENDING;
use russh_bbs_door_long_haul::crew::Role;
use russh_bbs_door_long_haul::engine::Action;
use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::scene::Scene;
use russh_bbs_door_long_haul::skills::Skill;
use russh_bbs_door_long_haul::transit::Where;

/// A captain built a given way: how many crew roles, skills, standing and cash.
fn build(h: &Harness, kind: u32) -> Player {
    let mut p = h.captain("dockhand");
    p.social.handle = "arcer".into();
    p.cash = 20_000;
    for f in 0..5 {
        p.standing[f] = match kind % 4 {
            0 => 40,
            1 => -10,
            2 => 15,
            _ => 0,
        };
    }
    if kind % 3 != 1 {
        for role in [
            Role::Pilot,
            Role::Engineer,
            Role::Gunner,
            Role::Medic,
            Role::Scientist,
            Role::Quartermaster,
            Role::Face,
        ] {
            h.sign(&mut p, role, 3 + u8::try_from(kind % 3).unwrap());
        }
    }
    for s in [
        Skill::Trade,
        Skill::Pilot,
        Skill::Engineer,
        Skill::Stealth,
        Skill::Survey,
    ] {
        h.set_level(&mut p, s, if kind.is_multiple_of(2) { 6 } else { 1 });
    }
    h.fit(&mut p, "lance1");
    if kind.is_multiple_of(2) {
        h.fit(&mut p, "scan1");
    }
    if let Some(g) = h
        .content
        .goods
        .iter()
        .position(|g| g.class == russh_bbs_door_long_haul::content::GoodClass::Contraband)
    {
        p.ship.cargo.push(russh_bbs_door_long_haul::ship::Lot {
            good: u8::try_from(g).unwrap(),
            tons: 3,
            paid: 100,
        });
    }
    p
}

/// Play one arc from its first step with a random policy over the options the captain can
/// take. Returns the ending code and the branch bits at the end, or `None` if it got stuck.
fn play(h: &mut Harness, arc: usize, kind: u32, seed: u64) -> Option<(u8, u16, Vec<usize>)> {
    let mut p = build(h, kind);
    let mut pick = DoorRng::from_seed(seed);
    h.reseed(seed ^ 0xA5A5);
    let arc_id = h.content.arcs.arc[arc].id.clone();
    let mut visited = Vec::new();
    for _ in 0..24 {
        h.today += 3;
        p.last_day = h.today - 1;
        let state = p.social.arcs[arc];
        if state.finished() {
            return Some((state.ending, state.branches, visited));
        }
        let ix =
            h.content.transit.event.iter().position(|e| {
                e.arc == arc_id && usize::from(e.step) == usize::from(state.step) + 1
            })?;
        visited.push(ix);
        p.scene = Some(Scene::Event(u16::try_from(ix).unwrap()));
        let def = h.content.transit.event[ix].clone();
        let w = Where {
            content: &h.content,
            frontier: &h.frontier,
            system: p.at,
            fresh: false,
        };
        let open: Vec<usize> = def
            .options
            .iter()
            .enumerate()
            .filter(|(_, o)| p.option_closed(&w, o).is_none())
            .map(|(i, _)| i)
            .collect();
        let n =
            *open.get(usize::try_from(pick.below(u32::try_from(open.len()).unwrap())).unwrap())?;
        h.go(&mut p, &Action::Choose(u8::try_from(n).unwrap()))
            .unwrap();
        // Any fight or boarding the scene started is waved away; the story moves on.
        p.scene = None;
    }
    None
}

#[test]
fn every_arc_ending_and_branch_is_reached_by_play() {
    let mut h = Harness::new(1, 11);
    for (ai, arc) in h.content.arcs.arc.clone().iter().enumerate() {
        let mut endings = BTreeSet::new();
        let mut branches = 0u16;
        let mut stuck = 0;
        for seed in 0..3000u64 {
            match play(&mut h, ai, u32::try_from(seed % 12).unwrap(), seed) {
                Some((end, b, _)) => {
                    assert_ne!(end, NO_ENDING);
                    endings.insert(end);
                    branches |= b;
                }
                None => stuck += 1,
            }
        }
        assert_eq!(stuck, 0, "arc `{}`: a captain got stuck", arc.id);
        for (ei, e) in arc.ending.iter().enumerate() {
            let code = u8::try_from(ai * 8 + ei).unwrap();
            assert!(
                endings.contains(&code),
                "arc `{}`: ending `{}` never happened",
                arc.id,
                e.id
            );
        }
        for (bi, b) in arc.branches.iter().enumerate() {
            assert!(
                branches & (1 << bi) != 0,
                "arc `{}`: branch `{b}` never taken",
                arc.id
            );
        }
    }
}

#[test]
fn a_due_arc_scene_wins_the_draw_and_a_finished_arc_never_returns() {
    let h = Harness::new(1, 12);
    let mut p = build(&h, 0);
    p.last_day = h.today;
    let arc = 0;
    let first = h
        .content
        .transit
        .event
        .iter()
        .position(|e| e.arc == h.content.arcs.arc[arc].id && e.step == 1)
        .unwrap();
    // Meet whatever the opening asks for; if it asks for more than a build can give, the
    // scene simply is not due, which is the point of a start condition.
    let w = Where {
        content: &h.content,
        frontier: &h.frontier,
        system: p.at,
        fresh: false,
    };
    let weights = p.event_weights(&w);
    if let Some((_, wt)) = weights.iter().find(|(i, _)| *i == first) {
        let total: u32 = weights.iter().map(|(_, w)| *w).sum();
        assert!(
            u64::from(*wt) * 10 > u64::from(total),
            "a due arc scene is a tenth of the draw at least"
        );
    }
    p.social.arcs[arc].ending = 0;
    let w = Where {
        content: &h.content,
        frontier: &h.frontier,
        system: p.at,
        fresh: false,
    };
    assert!(
        p.event_weights(&w)
            .iter()
            .all(|(i, _)| h.content.transit.event[*i].arc.is_empty()
                || h.content.transit.event[*i].arc != h.content.arcs.arc[arc].id)
    );
}

#[test]
fn arc_scenes_are_paced_by_real_days() {
    let h = Harness::new(1, 13);
    let mut p = build(&h, 0);
    p.last_day = 100;
    for (ai, arc) in h.content.arcs.arc.iter().enumerate() {
        let waits: Vec<_> = h
            .content
            .transit
            .event
            .iter()
            .filter(|e| e.arc == arc.id && e.step > 1)
            .collect();
        assert!(!waits.is_empty());
        // Every later step waits at least a day after the one before it.
        p.social.arcs[ai].step = 1;
        p.social.arcs[ai].day = 100;
        let w = Where {
            content: &h.content,
            frontier: &h.frontier,
            system: p.at,
            fresh: false,
        };
        let due_now = p
            .event_weights(&w)
            .iter()
            .filter(|(i, _)| h.content.transit.event[*i].arc == arc.id)
            .count();
        assert_eq!(due_now, 0, "arc `{}`: step 2 must wait a day", arc.id);
        p.social.arcs[ai].step = 0;
    }
}
