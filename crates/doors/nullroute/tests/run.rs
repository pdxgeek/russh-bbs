//! The run engine: the trace, the deck, the locks, and the RPG rule that the deck decides what
//! you can enter.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::assert_is_empty,
    clippy::identity_op
)]

mod common;

use common::*;
use russh_bbs_door::DoorRng;
use russh_bbs_door_nullroute::content::{Effect, Lock};
use russh_bbs_door_nullroute::net::{Capability, Overlay, Patrol};
use russh_bbs_door_nullroute::run::{Act, Refused, Run, Status};

fn rng() -> DoorRng {
    DoorRng::from_seed(11)
}

/// Run the script in `slot` at `target` until it works (at most 12 tries).
fn force(
    run: &mut Run,
    content: &russh_bbs_door_nullroute::content::Content,
    slot: usize,
    target: u8,
    rng: &mut DoorRng,
) {
    for _ in 0..12 {
        if run.is_open(target) {
            return;
        }
        let _ = run.act(
            content,
            Act::Use {
                slot,
                target: Some(target),
            },
            rng,
        );
    }
}

#[test]
fn a_locked_node_needs_the_right_family_at_enough_power() {
    let (c, mut run) = start(&[DICTIONARY, BADGE, STACK_SMASH, PING]);
    let mut r = rng();
    assert!(!run.exits().is_empty());
    assert_eq!(run.act(&c, Act::Move(2), &mut r), Err(Refused::Locked));
    // The wrong family: it spends cycles and makes noise, and the lock is still there.
    let (cycles, trace) = (run.cycles, run.trace);
    run.act(
        &c,
        Act::Use {
            slot: 0,
            target: Some(2),
        },
        &mut r,
    )
    .unwrap();
    assert!(run.cycles < cycles && run.trace > trace);
    assert!(!run.is_open(2));
    assert!(run.log.iter().any(|l| l.contains("not a password lock")));
    // The right family opens it.
    force(&mut run, &c, 2, 2, &mut r);
    assert!(run.is_open(2));
    run.act(&c, Act::Move(2), &mut r).unwrap();
    assert_eq!(run.at, 2);
}

#[test]
fn a_script_weaker_than_the_lock_never_works() {
    let content = content();
    let mut net = hand_net();
    net.nodes[2].security = 3;
    let mut run = Run::new(&content, setup(net, &[STACK_SMASH, PING])).unwrap();
    let mut r = rng();
    for _ in 0..5 {
        let _ = run.act(
            &content,
            Act::Use {
                slot: 0,
                target: Some(2),
            },
            &mut r,
        );
    }
    assert!(!run.is_open(2), "power 2 against level 3 must always fail");
    assert!(run.log.iter().any(|l| l.contains("holds (level 3)")));
}

#[test]
fn an_alert_raises_every_level_so_a_weak_deck_is_shut_out() {
    let content = content();
    let mut s = setup(hand_net(), &[STACK_SMASH]);
    s.alert = 1;
    let mut run = Run::new(&content, s).unwrap();
    let mut r = rng();
    for _ in 0..4 {
        let _ = run.act(
            &content,
            Act::Use {
                slot: 0,
                target: Some(2),
            },
            &mut r,
        );
    }
    assert!(!run.is_open(2), "level 2 + alert 1 = 3 beats power 2");
}

#[test]
fn a_hole_from_an_earlier_run_is_open_without_a_script() {
    let content = content();
    let mut s = setup(hand_net(), &[PING]);
    s.holes = vec![2];
    let mut run = Run::new(&content, s).unwrap();
    assert!(run.is_open(2));
    run.act(&content, Act::Move(2), &mut rng()).unwrap();
}

#[test]
fn moving_costs_a_cycle_and_the_trace_follows_noise_and_the_corporation() {
    let (c, mut run) = start(&[PING]);
    run.act(&c, Act::Move(1), &mut rng()).unwrap();
    assert_eq!(run.cycles, 39);
    // A router has noise 1; corporation 0 is x1.0 at tier 1.
    assert_eq!(run.trace, 1);
    // Halcyon Freight (corporation 1) is x1.35: noise 3 on a camera becomes 4.
    let content = content();
    let mut net = hand_net();
    net.corp = 1;
    let mut run = Run::new(&content, setup(net, &[PING])).unwrap();
    run.act(&content, Act::Move(1), &mut rng()).unwrap();
    run.act(&content, Act::Move(8), &mut rng()).unwrap();
    assert_eq!(
        run.trace,
        1 + 4 + 1,
        "router 1, camera 3 x1.35, live feed adds 1 on the move"
    );
}

#[test]
fn the_watchful_corporation_adds_trace_to_every_action() {
    let content = content();
    let mut net = hand_net();
    net.corp = 3; // Pennywhistle: watch 2
    let mut run = Run::new(&content, setup(net, &[PING])).unwrap();
    run.act(&content, Act::Move(1), &mut rng()).unwrap();
    assert_eq!(run.trace, 1 + 2);
}

#[test]
fn shield_mask_proxies_and_decoys_each_change_the_trace() {
    let content = content();
    let climb = |shield: u8, buffer: u32, deck: &[u8], pre: Option<usize>| {
        let mut s = setup(hand_net(), deck);
        s.shield = shield;
        s.buffer = buffer;
        let mut run = Run::new(&content, s).unwrap();
        let mut r = rng();
        if let Some(slot) = pre {
            run.act(&content, Act::Use { slot, target: None }, &mut r)
                .unwrap();
        }
        let before = run.trace;
        // Spoof the badge reader open and walk in: the break is noisy and so is the node.
        run.act(&content, Act::Move(1), &mut r).unwrap();
        run.act(&content, Act::Move(8), &mut r).unwrap();
        run.trace - before
    };
    let base = climb(0, 0, &[PING, VEIL], None);
    assert!(base >= 5);
    assert!(
        climb(3, 0, &[PING, VEIL], None) < base,
        "a shield trims every gain"
    );
    assert_eq!(climb(0, 100, &[PING, VEIL], None), 0, "proxies soak it all");
    assert!(
        climb(0, 0, &[PING, VEIL], Some(1)) < base,
        "a mask halves noisy actions"
    );
    assert_eq!(
        climb(0, 0, &[PING, DECOY], Some(1)),
        0,
        "a decoy swarm soaks it"
    );
}

#[test]
fn at_100_you_are_burned_and_the_run_is_over() {
    let content = content();
    let mut s = setup(hand_net(), &[STACK_SMASH]);
    s.cycles = 200;
    let mut net = hand_net();
    net.nodes[2].security = 3; // power 2 fails and fails loudly
    s.net = net;
    let mut run = Run::new(&content, s).unwrap();
    let mut r = rng();
    for _ in 0..30 {
        if run.status != Status::Active {
            break;
        }
        let _ = run.act(
            &content,
            Act::Use {
                slot: 0,
                target: Some(2),
            },
            &mut r,
        );
    }
    assert_eq!(run.status, Status::Burned);
    assert!(run.trace >= 100);
    assert_eq!(run.act(&content, Act::JackOut, &mut r), Err(Refused::Over));
    assert!(run.log.last().unwrap().contains("burned"));
}

#[test]
fn the_run_ends_when_cycles_do() {
    let content = content();
    let mut s = setup(hand_net(), &[PING]);
    s.cycles = 1;
    let mut run = Run::new(&content, s).unwrap();
    run.act(&content, Act::Move(1), &mut rng()).unwrap();
    assert_eq!(
        run.act(&content, Act::Move(8), &mut rng()),
        Err(Refused::NoCycles)
    );
    // You can always jack out.
    run.act(&content, Act::JackOut, &mut rng()).unwrap();
    assert_eq!(run.status, Status::Out);
}

#[test]
fn a_better_interface_makes_two_cycle_scripts_cost_one() {
    let content = content();
    let cost = |iface: u8| {
        let mut s = setup(hand_net(), &[BRUTE]);
        s.iface = iface;
        s.net.nodes[4].security = 1;
        let mut run = Run::new(&content, s).unwrap();
        let before = run.cycles;
        run.act(
            &content,
            Act::Use {
                slot: 0,
                target: Some(4),
            },
            &mut rng(),
        )
        .unwrap();
        before - run.cycles
    };
    assert_eq!((cost(0), cost(2)), (2, 1));
}

#[test]
fn hidden_links_appear_only_to_a_scan_with_the_power() {
    let (c, mut run) = start(&[PING, DEEP_SWEEP]);
    let mut r = rng();
    run.act(&c, Act::Move(1), &mut r).unwrap();
    assert!(!run.exits().contains(&5));
    run.act(
        &c,
        Act::Use {
            slot: 0,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert!(
        !run.exits().contains(&5),
        "power 2 cannot find a stealth-3 link"
    );
    run.act(
        &c,
        Act::Use {
            slot: 1,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert!(run.exits().contains(&5), "power 4 can");
}

#[test]
fn a_scan_shows_the_lock_and_a_sweep_shows_a_honeypot_only_to_the_strong() {
    let (c, mut run) = start(&[PING, DEEP_SWEEP]);
    let mut r = rng();
    assert!(run.describe(&c, 2).contains("locked?"));
    run.act(
        &c,
        Act::Use {
            slot: 0,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert!(
        run.describe(&c, 2).contains("buffer lock 2"),
        "{}",
        run.describe(&c, 2)
    );
    run.act(&c, Act::Move(4), &mut r).unwrap_err();
    // Terminal 4 is locked (crack): walk around via a hole instead.
    let content = content();
    let mut s = setup(hand_net(), &[PING, DEEP_SWEEP]);
    s.holes = vec![4];
    let mut run = Run::new(&content, s).unwrap();
    run.act(&content, Act::Move(4), &mut r).unwrap();
    run.act(
        &content,
        Act::Use {
            slot: 0,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert!(!run.describe(&content, 7).contains("HONEYPOT"));
    run.act(
        &content,
        Act::Use {
            slot: 1,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert!(run.describe(&content, 7).contains("HONEYPOT"));
}

#[test]
fn a_honeypot_trips_the_alarm_and_copying_it_yields_nothing() {
    let content = content();
    let mut s = setup(hand_net(), &[SIPHON]);
    s.holes = vec![4];
    let mut run = Run::new(&content, s).unwrap();
    let mut r = rng();
    run.act(&content, Act::Move(4), &mut r).unwrap();
    run.act(&content, Act::Move(7), &mut r).unwrap();
    assert!(run.alarm());
    assert!(run.trace >= 25);
    run.act(
        &content,
        Act::Use {
            slot: 0,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert!(!run.objective_done());
    assert_eq!(run.result().loot, 0);
}

#[test]
fn copying_the_real_vault_completes_the_objective_and_loot_is_kept_on_jack_out() {
    let content = content();
    let mut s = setup(hand_net(), &[SIPHON]);
    s.holes = vec![2];
    let mut run = Run::new(&content, s).unwrap();
    let mut r = rng();
    run.act(&content, Act::Move(2), &mut r).unwrap();
    run.act(&content, Act::Move(5), &mut r).unwrap();
    run.act(
        &content,
        Act::Use {
            slot: 0,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert!(run.objective_done());
    // Copying twice is refused for free.
    assert_eq!(
        run.act(
            &content,
            Act::Use {
                slot: 0,
                target: None
            },
            &mut r
        ),
        Err(Refused::NothingToDo)
    );
    run.act(&content, Act::JackOut, &mut r).unwrap();
    let result = run.result();
    assert_eq!(
        (result.status, result.objective, result.loot),
        (Status::Out, true, 60)
    );
}

#[test]
fn a_camera_feed_adds_trace_to_every_move_until_it_is_cut() {
    let content = content();
    let mut s = setup(hand_net(), &[PING, BLIND_EYE]);
    s.holes = vec![2];
    let mut run = Run::new(&content, s).unwrap();
    let mut r = rng();
    run.act(&content, Act::Move(1), &mut r).unwrap();
    run.act(&content, Act::Move(8), &mut r).unwrap();
    assert!(run.feed_live());
    let t = run.trace;
    run.act(&content, Act::Move(1), &mut r).unwrap();
    assert_eq!(run.trace, t + 1 + 1, "router noise 1, feed 1");
    // Cutting needs the feed to be live, and stops the drip.
    run.act(
        &content,
        Act::Use {
            slot: 1,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert!(!run.feed_live());
    assert_eq!(
        run.act(
            &content,
            Act::Use {
                slot: 1,
                target: None
            },
            &mut r
        ),
        Err(Refused::NothingToDo)
    );
}

#[test]
fn a_tracer_counts_every_action_until_a_reroute_ends_it() {
    let content = content();
    let mut net = hand_net();
    net.nodes[1].patrol = Patrol::Tracer;
    let mut s = setup(net, &[PING, REROUTE]);
    s.holes = vec![2];
    let mut run = Run::new(&content, s).unwrap();
    let mut r = rng();
    run.act(&content, Act::Move(1), &mut r).unwrap();
    assert!(run.tracer());
    let t = run.trace;
    run.act(
        &content,
        Act::Use {
            slot: 0,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert_eq!(run.trace, t + 2 + 2, "ping noise 2, tracer 2");
    run.act(
        &content,
        Act::Use {
            slot: 1,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert!(!run.tracer());
    assert!(run.trace < t + 4);
}

#[test]
fn a_sentry_alarms_unless_jammed_or_forked() {
    let content = content();
    let enter = |deck: &[u8], pre: Option<(usize, Option<u8>)>| {
        let mut net = hand_net();
        net.nodes[1].patrol = Patrol::Sentry;
        net.nodes[1].security = 2;
        let mut run = Run::new(&content, setup(net, deck)).unwrap();
        let mut r = rng();
        if let Some((slot, target)) = pre {
            for _ in 0..8 {
                if run
                    .act(&content, Act::Use { slot, target }, &mut r)
                    .is_err()
                {
                    break;
                }
                if run.log.iter().any(|l| l.contains("goes blind")) || target.is_none() {
                    break;
                }
            }
        }
        let before = run.trace;
        run.act(&content, Act::Move(1), &mut r).unwrap();
        run.trace - before
    };
    let hit = enter(&[PING, JAMMER, FORK], None);
    assert!(hit >= 6 + 4, "a level-2 sentry costs 6 + 2 x 2, got {hit}");
    let jammed = enter(&[PING, JAMMER, FORK], Some((1, Some(1))));
    assert!(jammed < hit, "{jammed} < {hit}");
    let forked = enter(&[PING, JAMMER, FORK], Some((2, None)));
    assert!(forked < hit + 4);
}

#[test]
fn a_black_wall_cannot_be_entered_until_purged() {
    let content = content();
    let mut net = hand_net();
    net.nodes[1].patrol = Patrol::BlackWall;
    net.nodes[1].security = 3;
    let mut run = Run::new(&content, setup(net.clone(), &[PING, PURGE])).unwrap();
    let mut r = rng();
    assert_eq!(
        run.act(&content, Act::Move(1), &mut r),
        Err(Refused::Locked)
    );
    for _ in 0..12 {
        if run.is_open(1) {
            break;
        }
        let _ = run.act(
            &content,
            Act::Use {
                slot: 1,
                target: Some(1),
            },
            &mut r,
        );
    }
    assert!(run.is_open(1));
    run.act(&content, Act::Move(1), &mut r).unwrap();
    // Without a purge there is no way in at all.
    let cap = Capability {
        breaks: [9; 3],
        purge: 0,
        scan: 9,
    };
    assert!(!net.can_enter(1, &cap, &Overlay::default()));
    assert!(net.can_enter(1, &Capability { purge: 3, ..cap }, &Overlay::default()));
}

#[test]
fn charges_run_out_and_aimed_scripts_need_a_neighbour() {
    let content = content();
    let mut s = setup(hand_net(), &[VEIL, STACK_SMASH]);
    s.holes = vec![];
    let mut run = Run::new(&content, s).unwrap();
    let mut r = rng();
    run.act(
        &content,
        Act::Use {
            slot: 0,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    run.act(
        &content,
        Act::Use {
            slot: 0,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert_eq!(
        run.act(
            &content,
            Act::Use {
                slot: 0,
                target: None
            },
            &mut r
        ),
        Err(Refused::NoCharges)
    );
    assert_eq!(
        run.act(
            &content,
            Act::Use {
                slot: 1,
                target: None
            },
            &mut r
        ),
        Err(Refused::NeedsTarget)
    );
    assert_eq!(
        run.act(
            &content,
            Act::Use {
                slot: 1,
                target: Some(7)
            },
            &mut r
        ),
        Err(Refused::NotAdjacent)
    );
    assert_eq!(
        run.act(
            &content,
            Act::Use {
                slot: 5,
                target: None
            },
            &mut r
        ),
        Err(Refused::NoSlot)
    );
    assert_eq!(
        run.act(&content, Act::Move(5), &mut r),
        Err(Refused::NotAdjacent)
    );
}

#[test]
fn objectives_need_the_matching_node_and_effect() {
    let content = content();
    let mut s = setup(hand_net(), &[IMPLANT, SIPHON]);
    s.target = "AC".into();
    s.effect = Effect::Plant;
    s.holes = vec![3, 6];
    let mut run = Run::new(&content, s).unwrap();
    let mut r = rng();
    run.act(&content, Act::Move(3), &mut r).unwrap();
    assert_eq!(
        run.act(
            &content,
            Act::Use {
                slot: 0,
                target: None
            },
            &mut r
        ),
        Err(Refused::NothingToDo),
        "not an AC or SR node"
    );
    run.act(&content, Act::Move(6), &mut r).unwrap();
    run.act(
        &content,
        Act::Use {
            slot: 0,
            target: None,
        },
        &mut r,
    )
    .unwrap();
    assert!(run.objective_done());
    assert!(run.result().planted);
}

#[test]
fn the_same_seed_gives_the_same_run_and_another_seed_a_different_one() {
    let play = |seed: u64| {
        let content = content();
        let mut s = setup(hand_net(), &[PING, STACK_SMASH, BADGE, DICTIONARY]);
        s.cycles = 60;
        let mut run = Run::new(&content, s).unwrap();
        let mut r = DoorRng::from_seed(seed);
        for slot in [0usize, 1, 1, 1, 2, 2, 3, 3] {
            let target = [2u8, 3, 4].get(slot.saturating_sub(1)).copied();
            let _ = run.act(&content, Act::Use { slot, target }, &mut r);
        }
        (run.trace, run.cycles, run.log.clone())
    };
    assert_eq!(play(5), play(5));
    assert!((0..40).any(|s| play(s) != play(5)), "the dice matter");
}

#[test]
fn the_best_of_each_kind_is_what_reachability_sees() {
    let content = content();
    let cap = russh_bbs_door_nullroute::run::capability_of(
        &content,
        [BRUTE, BADGE, DEEP_SWEEP, PURGE].into_iter(),
    );
    assert_eq!(
        cap,
        Capability {
            breaks: [4, 2, 0],
            purge: 3,
            scan: 4
        }
    );
    assert_eq!(Capability::only(Lock::Spoof).breaks, [0, 9, 0]);
}
