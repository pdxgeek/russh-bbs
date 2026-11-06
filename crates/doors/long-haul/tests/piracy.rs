//! Opt-in piracy, convoy and outpost rules at the engine level: who may be hunted, the caps,
//! the cooldowns, and that nothing a player sends can grow the world without bound.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::panic,
    missing_docs,
    clippy::too_many_lines
)]

mod common;

use common::Harness;
use russh_bbs_door_long_haul::engine::Action;
use russh_bbs_door_long_haul::events::Refusal;
use russh_bbs_door_long_haul::player::Player;
use russh_bbs_door_long_haul::social::{
    self, CONVOY_MAX, Intent, MAX_CONVOYS, MAX_INTENTS, MAX_ROSTER, Member, Pirate, RaidOutcome,
    Social, SocialDelta,
};
use russh_bbs_door_long_haul::socialplay::Say;
use russh_bbs_door_long_haul::world::Delta;

fn pirate(handle: &str, power: u16, seen: i64) -> Pirate {
    Pirate {
        handle: handle.into(),
        name: handle.to_uppercase(),
        since: seen - 3,
        seen,
        power,
        hull: 100,
        speed: 3,
        infamy: 0,
        renown: 0,
        worth: 1,
    }
}

fn armed(h: &Harness, handle: &str) -> Player {
    let mut p = h.captain("dockhand");
    h.fit(&mut p, "lance1");
    p.social.handle = handle.into();
    p
}

fn list(h: &mut Harness, who: &[(&str, u16)]) {
    for (handle, power) in who {
        let entry = pirate(handle, *power, h.today);
        h.world.social.apply(&SocialDelta::Roster(entry), h.today);
    }
}

fn say(text: &str) -> Say {
    Say::for_tests(text)
}

#[test]
fn only_flagged_captains_hunt_and_only_flagged_captains_can_be_hunted() {
    let mut h = Harness::new(1, 1);
    let mut hunter = armed(&h, "hunter");
    list(&mut h, &[("prey", 1)]);
    // Not flagged: refused, and nothing is sent to the world.
    let r = h.go(&mut hunter, &Action::Intercept("prey".into()));
    assert!(matches!(r, Err(Refusal::Social(_))), "{r:?}");
    assert!(h.deltas.is_empty());
    // Flag up, but the colours are too new to hunt.
    h.go(&mut hunter, &Action::Flag(true)).unwrap();
    assert!(matches!(
        h.go(&mut hunter, &Action::Intercept("prey".into())),
        Err(Refusal::Social(_))
    ));
    h.next_day();
    // A captain who never flagged is not on the roster, whatever they carry.
    assert!(matches!(
        h.go(&mut hunter, &Action::Intercept("clean_hauler".into())),
        Err(Refusal::Social(_))
    ));
    // No gun, no hunt.
    let mut unarmed = h.captain("dockhand");
    unarmed.social.handle = "pacifist".into();
    h.go(&mut unarmed, &Action::Flag(true)).unwrap();
    h.next_day();
    assert!(matches!(
        h.go(&mut unarmed, &Action::Intercept("prey".into())),
        Err(Refusal::Social(_))
    ));
    // The flagged, armed, settled hunter can.
    h.world
        .social
        .apply(&SocialDelta::Roster(pirate("prey", 1, h.today)), h.today);
    h.go(&mut hunter, &Action::Intercept("prey".into()))
        .unwrap();
    // Not oneself, not the same target twice, and a stale roster entry is gone.
    assert!(
        h.go(&mut hunter, &Action::Intercept("hunter".into()))
            .is_err()
    );
    assert!(
        h.go(&mut hunter, &Action::Intercept("prey".into()))
            .is_err()
    );
    list(&mut h, &[("old", 1)]);
    h.world.social.roster.last_mut().unwrap().seen = h.today - 9;
    assert!(h.go(&mut hunter, &Action::Intercept("old".into())).is_err());
}

#[test]
fn a_captain_can_start_only_two_hunts_a_day() {
    let mut h = Harness::new(1, 2);
    let mut hunter = armed(&h, "hunter");
    h.go(&mut hunter, &Action::Flag(true)).unwrap();
    h.next_day();
    list(&mut h, &[("a", 1), ("b", 1), ("c", 1)]);
    h.go(&mut hunter, &Action::Intercept("a".into())).unwrap();
    h.go(&mut hunter, &Action::Intercept("b".into())).unwrap();
    let third = h.go(&mut hunter, &Action::Intercept("c".into()));
    assert!(matches!(third, Err(Refusal::Social(_))), "{third:?}");
    // Tomorrow the count starts again.
    h.next_day();
    list(&mut h, &[("c", 1)]);
    h.go(&mut hunter, &Action::Intercept("c".into())).unwrap();
}

#[test]
fn the_night_caps_hits_on_one_target_and_a_pair_cools_down() {
    let mut s = Social::new(0);
    for h in ["prey", "x", "y", "z"] {
        s.apply(&SocialDelta::Roster(pirate(h, 1, 10)), 10);
    }
    // Three hunters, one target, a strong ship each: only two hits land in a day.
    for (i, hunter) in ["x", "y", "z"].into_iter().enumerate() {
        s.apply(
            &SocialDelta::Intent(Intent {
                id: 100 + u32::try_from(i).unwrap(),
                day: 10,
                attacker: hunter.into(),
                aname: hunter.into(),
                target: "prey".into(),
                ship: pirate(hunter, 9, 10),
            }),
            10,
        );
    }
    s.tick(10, &[], &mut russh_bbs_door::DoorRng::from_seed(1));
    let live = s
        .raids
        .iter()
        .filter(|r| r.outcome != RaidOutcome::Void)
        .count();
    assert_eq!(live, social::TARGET_PER_DAY);
    assert_eq!(
        s.raids
            .iter()
            .filter(|r| r.outcome == RaidOutcome::Void)
            .count(),
        1
    );
    // The same pair cannot meet again for the cooldown.
    let pair = s
        .raids
        .iter()
        .find(|r| r.outcome != RaidOutcome::Void)
        .unwrap()
        .attacker
        .clone();
    s.apply(
        &SocialDelta::Intent(Intent {
            id: 900,
            day: 11,
            attacker: pair.clone(),
            aname: pair.clone(),
            target: "prey".into(),
            ship: pirate(&pair, 9, 11),
        }),
        11,
    );
    for p in &mut s.roster {
        p.seen = 11;
    }
    s.tick(11, &[], &mut russh_bbs_door::DoorRng::from_seed(2));
    assert_eq!(s.raids.last().unwrap().outcome, RaidOutcome::Void);
}

#[test]
fn a_prize_is_never_more_than_the_cap_and_a_stronger_ship_wins_more_often() {
    let mut wins = [0u32; 2];
    for (k, power) in [1u16, 9].into_iter().enumerate() {
        for i in 0..200u32 {
            let mut s = Social::new(0);
            s.apply(&SocialDelta::Roster(pirate("prey", 3, 10)), 10);
            s.apply(
                &SocialDelta::Intent(Intent {
                    id: 1 + i * 7 + u32::try_from(k).unwrap(),
                    day: 10,
                    attacker: "x".into(),
                    aname: "X".into(),
                    target: "prey".into(),
                    ship: pirate("x", power, 10),
                }),
                10,
            );
            s.tick(10, &[], &mut russh_bbs_door::DoorRng::from_seed(1));
            let raid = s.raids[0].clone();
            assert!(i64::from(raid.cap) <= social::LOOT_CAP);
            if raid.outcome == RaidOutcome::Raided {
                wins[k] += 1;
            }
            // A victim cannot be made to pay more than the cap, whatever is claimed.
            s.apply(
                &SocialDelta::Settle {
                    raid: raid.id,
                    paid: 1_000_000,
                },
                10,
            );
            if raid.outcome == RaidOutcome::Raided {
                assert!(s.raids[0].paid <= raid.cap);
            }
        }
    }
    assert!(
        wins[1] > wins[0] + 40,
        "weak {} strong {}",
        wins[0],
        wins[1]
    );
}

#[test]
fn world_lists_are_capped_whatever_players_send() {
    let mut s = Social::new(0);
    for i in 0..(MAX_ROSTER + 20) {
        s.apply(&SocialDelta::Roster(pirate(&format!("p{i}"), 1, 5)), 5);
    }
    assert_eq!(s.roster.len(), MAX_ROSTER);
    for i in 0..(MAX_INTENTS + 20) {
        s.apply(
            &SocialDelta::Intent(Intent {
                id: u32::try_from(i).unwrap() + 1,
                day: 5,
                attacker: format!("a{i}"),
                aname: "A".into(),
                target: "p1".into(),
                ship: pirate("a", 1, 5),
            }),
            5,
        );
    }
    assert!(s.intents.len() <= MAX_INTENTS);
    // A single hunter cannot queue more than the daily cap.
    let mut s = Social::new(0);
    for i in 0..10u32 {
        s.apply(
            &SocialDelta::Intent(Intent {
                id: i + 1,
                day: 5,
                attacker: "one".into(),
                aname: "One".into(),
                target: format!("t{i}"),
                ship: pirate("one", 1, 5),
            }),
            5,
        );
    }
    assert_eq!(s.intents.len(), usize::from(social::INTERCEPTS_PER_DAY));
    let member = |h: &str| Member {
        handle: h.into(),
        name: h.into(),
        joined: 5,
        seen: 5,
        power: 1,
        earned: 0,
    };
    for i in 0..(MAX_CONVOYS + 5) {
        s.apply(
            &SocialDelta::ConvoyForm {
                id: u32::try_from(i).unwrap() + 1,
                name: "C".into(),
                member: member(&format!("m{i}")),
                day: 5,
            },
            5,
        );
    }
    assert_eq!(s.convoys.len(), MAX_CONVOYS);
    for i in 0..10 {
        s.apply(
            &SocialDelta::ConvoyJoin {
                id: 1,
                member: member(&format!("j{i}")),
            },
            5,
        );
    }
    assert_eq!(s.convoys[0].members.len(), CONVOY_MAX);
    // A captain is in one convoy at a time.
    s.apply(
        &SocialDelta::ConvoyJoin {
            id: 2,
            member: member("m0"),
        },
        5,
    );
    assert_eq!(s.convoys[1].members.len(), 1);
    // Notes keep the newest few, and one author is limited per day.
    for _ in 0..20 {
        s.apply(
            &SocialDelta::ConvoyNote {
                id: 1,
                note: social::Note {
                    day: 5,
                    who: "m0".into(),
                    text: "x".repeat(900),
                },
            },
            5,
        );
    }
    assert!(s.convoys[0].notes.len() <= usize::from(social::NOTES_PER_DAY));
    assert!(s.convoys[0].notes.iter().all(|n| n.text.len() <= 240));
}

#[test]
fn convoy_actions_follow_their_rules_and_chat_is_limited() {
    let mut h = Harness::new(1, 3);
    let mut a = h.captain("dockhand");
    a.social.handle = "alpha".into();
    let mut b = h.captain("dockhand");
    b.social.handle = "beta".into();
    h.go(&mut a, &Action::ConvoyForm).unwrap();
    assert!(
        h.go(&mut a, &Action::ConvoyForm).is_err(),
        "one convoy at a time"
    );
    let id = a.social.convoy;
    // The local world copy has the convoy because the engine applied the delta.
    h.go(&mut b, &Action::ConvoyJoin(id)).unwrap();
    assert!(h.go(&mut b, &Action::ConvoyJoin(id)).is_err());
    assert!(h.go(&mut b, &Action::ConvoyJoin(9999)).is_err() || b.social.convoy == id);
    for i in 0..social::NOTES_PER_DAY {
        h.go(&mut a, &Action::ConvoySay(say(&format!("line {i}"))))
            .unwrap();
    }
    assert!(matches!(
        h.go(&mut a, &Action::ConvoySay(say("one too many"))),
        Err(Refusal::Social(_))
    ));
    assert!(h.go(&mut a, &Action::ConvoySay(say("   "))).is_err());
    // A hunter does not hunt their own convoy.
    h.fit(&mut a, "lance1");
    h.go(&mut a, &Action::Flag(true)).unwrap();
    h.go(&mut b, &Action::Flag(true)).unwrap();
    h.next_day();
    list(&mut h, &[("alpha", 1), ("beta", 1)]);
    let r = h.go(&mut a, &Action::Intercept("beta".into()));
    assert!(matches!(r, Err(Refusal::Social(_))), "{r:?}");
    // Leaving frees the seat; the pool pays a share of profit only while in a convoy.
    h.go(&mut b, &Action::ConvoyLeave).unwrap();
    assert_eq!(b.social.convoy, 0);
    assert!(
        h.deltas
            .iter()
            .any(|d| matches!(d, Delta::Social(SocialDelta::ConvoyLeave { .. })))
    );
}

#[test]
fn outposts_need_the_season_to_be_late_a_charted_empty_system_and_the_credits() {
    let mut h = Harness::new(1, 4);
    let frontier = h.frontier.clone();
    let site = frontier
        .systems
        .iter()
        .find(|s| !s.uncharted && s.stations.is_empty())
        .unwrap()
        .id;
    let mut p = h.captain("dockhand");
    p.social.handle = "builder".into();
    p.at = site;
    p.docked = None;
    p.cash = 50_000;
    // Early in the season: refused.
    assert!(matches!(
        h.go(
            &mut p,
            &Action::Pledge {
                system: site,
                amount: 600
            }
        ),
        Err(Refusal::Social(_))
    ));
    h.today += social::OUTPOST_MIN_DAY;
    // Too small, too large, and in the wrong place.
    assert!(
        h.go(
            &mut p,
            &Action::Pledge {
                system: site,
                amount: 10
            }
        )
        .is_err()
    );
    assert!(
        h.go(
            &mut p,
            &Action::Pledge {
                system: site,
                amount: 90_000
            }
        )
        .is_err()
    );
    let other = frontier.systems.iter().find(|s| s.id != site).unwrap().id;
    assert!(
        h.go(
            &mut p,
            &Action::Pledge {
                system: other,
                amount: 600
            }
        )
        .is_err()
    );
    // Where a station already stands: refused.
    let built = frontier
        .systems
        .iter()
        .find(|s| !s.stations.is_empty() && !s.uncharted)
        .unwrap()
        .id;
    p.at = built;
    assert!(
        h.go(
            &mut p,
            &Action::Pledge {
                system: built,
                amount: 600
            }
        )
        .is_err()
    );
    p.at = site;
    let before = p.cash;
    h.go(
        &mut p,
        &Action::Pledge {
            system: site,
            amount: 600,
        },
    )
    .unwrap();
    assert_eq!(p.cash, before - 600);
    assert_eq!(h.world.social.site_at(site).unwrap().fund, 600);
    // Cannot pledge what you do not have.
    p.cash = 100;
    assert!(
        h.go(
            &mut p,
            &Action::Pledge {
                system: site,
                amount: 600
            }
        )
        .is_err()
    );
}

#[test]
fn a_convoys_guard_makes_pirate_events_rarer_and_nothing_else() {
    use russh_bbs_door_long_haul::transit::Where;
    let h = Harness::new(1, 5);
    let mut alone = h.captain("dockhand");
    let mut guarded = h.captain("dockhand");
    guarded.social.guard = 100;
    alone.social.guard = 0;
    let w = Where {
        content: &h.content,
        frontier: &h.frontier,
        system: alone.at,
        fresh: false,
    };
    let sum = |p: &Player, pirate: bool| -> u64 {
        p.event_weights(&w)
            .iter()
            .filter(|(i, _)| (h.content.transit.event[*i].family == "pirate") == pirate)
            .map(|(_, wt)| u64::from(*wt))
            .sum()
    };
    assert!(sum(&alone, true) > 0);
    assert!(sum(&guarded, true) < sum(&alone, true) * 7 / 10);
    assert_eq!(
        sum(&guarded, false),
        sum(&alone, false),
        "only pirates are kept away"
    );
}
