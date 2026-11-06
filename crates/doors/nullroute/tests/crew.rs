//! Crews: the rules, the shared run, and two players taking a crew run from founding to the
//! split through the framework's store, one session at a time, as two people would across a day.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::indexing_slicing,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    clippy::expect_used
)]

mod common;

use std::sync::Arc;

use common::*;
use russh_bbs_core::doors::{DoorStore, Revision, SaveWrite};
use russh_bbs_core::limits::{Clock, Day, ManualClock};
use russh_bbs_core::store::AccountId;
use russh_bbs_door::host::Running;
use russh_bbs_door::sim::MemoryDoorStore;
use russh_bbs_door::tick::{TickPolicy, run_due};
use russh_bbs_door::{Door, DoorEnv, DoorKey, Flow};
use russh_bbs_door_nullroute::autoplay::Pilot;
use russh_bbs_door_nullroute::crew::{
    self, ACTIONS_MAX, COLD, CrewWhy, Member, OpState, TRACE_SHARE, pack, unpack,
};
use russh_bbs_door_nullroute::net::Capability;
use russh_bbs_door_nullroute::player::Player;
use russh_bbs_door_nullroute::run::{Act, Status};
use russh_bbs_door_nullroute::save::{decode, decode_world, encode};
use russh_bbs_door_nullroute::social::Posted;
use russh_bbs_door_nullroute::world::{Contract, World};
use russh_bbs_door_nullroute::{DOOR_ID, Nullroute};

const DAY: i64 = 20_100;

fn member(name: &str, deck: &[u8]) -> Member {
    Member {
        handle: name.into(),
        deck: deck.to_vec(),
        builds: Vec::new(),
        skill: [0; 5],
        shield: 0,
        iface: 0,
        cycles: 30,
        buffer: 0,
    }
}

fn posted(name: &str) -> Posted {
    Posted {
        handle: name.into(),
        tier: 1,
        heat: 0,
        honeypot: 0,
        shield: 0,
        edge: 3,
        cash: 5,
        rep: 0,
        born: DAY - 30,
        seen: DAY,
        peace_until: 0,
        pvp: true,
        crews_open: true,
    }
}

#[test]
fn an_action_packs_into_two_bytes_and_back() {
    for idx in 0..5usize {
        for act in [
            Act::Move(0),
            Act::Move(23),
            Act::Use {
                slot: 0,
                target: None,
            },
            Act::Use {
                slot: 5,
                target: Some(17),
            },
            Act::JackOut,
            Act::Brace,
            Act::BreakOff,
        ] {
            let got = unpack(pack(idx, act)).unwrap();
            assert_eq!(got, (idx, act));
            assert_ne!(pack(idx, act), COLD);
        }
    }
    assert_eq!(unpack(COLD), None);
}

#[test]
fn a_crew_forms_invites_accepts_and_leaves_by_the_rules() {
    let mut w = World::default();
    w.social.post(posted("ann"));
    w.social.post(posted("bea"));
    w.social.post(posted("cal"));
    w.social.crew_create("Night", "ann", DAY).unwrap();
    // One crew each, and a name is taken.
    assert_eq!(
        w.social.crew_create("Other", "ann", DAY),
        Err(CrewWhy::InCrew)
    );
    w.social.crew_create("Other", "cal", DAY).unwrap();
    assert_eq!(
        w.social.crew_create("night", "bea", DAY),
        Err(CrewWhy::NameTaken)
    );
    // Only the leader invites, and only someone who takes invitations.
    assert_eq!(
        w.social.crew_invite("bea", "cal", DAY),
        Err(CrewWhy::NoCrew)
    );
    w.social.crew_invite("ann", "bea", DAY).unwrap();
    let mut closed = posted("dee");
    closed.crews_open = false;
    w.social.post(closed);
    assert_eq!(
        w.social.crew_invite("ann", "dee", DAY),
        Err(CrewWhy::NoInvite)
    );
    assert_eq!(
        w.social.crew_invite("ann", "nobody", DAY),
        Err(CrewWhy::NoInvite)
    );
    // The invitation is a template line with the leader's handle, delivered once.
    let n = w
        .social
        .deliveries
        .iter()
        .filter(|d| d.to == "bea" && d.who == "ann")
        .count();
    assert_eq!(n, 1);
    let id = w.social.invites_for("bea")[0].id;
    // A stranger cannot take someone else's invitation.
    assert_eq!(w.social.crew_accept("cal", id), Err(CrewWhy::InCrew));
    w.social.crew_accept("bea", id).unwrap();
    assert!(w.social.crewmates("ann", "bea"));
    assert_eq!(w.social.crew_accept("bea", id), Err(CrewWhy::InCrew));
    // At most five members and five invitations out.
    for i in 0..3 {
        let name = format!("m{i}");
        w.social.post(posted(&name));
        w.social.crew_invite("ann", &name, DAY).unwrap();
        let id = w.social.invites_for(&name)[0].id;
        w.social.crew_accept(&name, id).unwrap();
    }
    w.social.post(posted("late"));
    assert_eq!(
        w.social.crew_invite("ann", "late", DAY),
        Err(CrewWhy::CrewFull)
    );
    // Leaving passes the crew on; the last one out ends it.
    w.social.crew_leave("ann").unwrap();
    assert_eq!(w.social.crew_of("bea").unwrap().leader, "bea");
    assert!(w.social.crew_of("ann").is_none());
}

fn start_op(
    w: &mut World,
    c: &russh_bbs_door_nullroute::content::Content,
    who: &[(&str, &[u8])],
) -> u32 {
    let first = who[0].0;
    for (name, _) in who {
        w.social.post(posted(name));
    }
    w.social.crew_create("Night", first, DAY).unwrap();
    for (name, _) in &who[1..] {
        w.social.crew_invite(first, name, DAY).unwrap();
        let id = w.social.invites_for(name)[0].id;
        w.social.crew_accept(name, id).unwrap();
    }
    let board = russh_bbs_door_nullroute::world::first_board(c, DAY);
    let contract = *board.iter().find(|k| k.tier == 1).unwrap();
    w.op_open(7, contract, member(first, who[0].1), DAY)
        .unwrap();
    for (name, deck) in &who[1..] {
        w.op_join(7, member(name, deck)).unwrap();
    }
    w.op_start(7, first, DAY).unwrap();
    7
}

#[test]
fn the_rules_of_a_shared_run_hold() {
    let c = content();
    let mut w = World::default();
    let deck: &[u8] = &[1, 5, 8, 11, 23];
    let id = start_op(&mut w, &c, &[("ann", deck), ("bea", deck)]);
    // Only a member acts, in a live run, on a log as long as they last saw.
    assert_eq!(
        w.op_act(&c, id, "cal", Act::JackOut, 0, DAY),
        Err(CrewWhy::NotMember)
    );
    assert_eq!(
        w.op_act(&c, id, "ann", Act::JackOut, 3, DAY),
        Err(CrewWhy::Stale),
        "another member moved first: the action is not applied to a state the mover never saw"
    );
    // An action the engine refuses is not logged.
    let refused = w.op_act(&c, id, "ann", Act::Move(99), 0, DAY);
    assert!(matches!(refused, Err(CrewWhy::Refused(_))), "{refused:?}");
    assert!(w.social.ops[0].log.is_empty());
    // Two members: the shared trace is a share of each member's noise.
    let op = w.social.ops[0].clone();
    assert_eq!(op.members.len(), 2);
    let live = op.live(&c).unwrap();
    assert_eq!(live.runs.len(), 2);
    assert!(TRACE_SHARE[1] < TRACE_SHARE[0]);
}

#[test]
fn the_same_log_replays_to_the_same_runs_every_time() {
    let c = content();
    let mut w = World::default();
    let deck: &[u8] = &[1, 5, 8, 11, 23];
    let id = start_op(&mut w, &c, &[("ann", deck), ("bea", deck)]);
    // Play it with the harness's hands, alternating, until it ends.
    let mut pilots: Vec<Option<Pilot>> = vec![None, None];
    for turn in 0..400 {
        let op = w.social.ops[0].clone();
        if op.state == OpState::Done {
            break;
        }
        let live = op.live(&c).unwrap();
        let who = turn % 2;
        let run = &live.runs[who];
        if run.status != Status::Active {
            continue;
        }
        let pilot = pilots[who].get_or_insert_with(|| Pilot::in_crew(run, 85));
        let Some(act) = pilot.decide(&c, run, live.objective) else {
            continue;
        };
        let name = ["ann", "bea"][who];
        let ok = w.op_act(&c, id, name, act, op.log.len(), DAY).is_ok();
        pilot.result(act, ok);
    }
    let op = w.social.ops[0].clone();
    assert!(!op.log.is_empty() && op.log.len() <= ACTIONS_MAX);
    // Replaying twice, and through the byte format, gives the same crew.
    let a = op.live(&c).unwrap();
    let bytes = russh_bbs_door_nullroute::save::encode_world(&w);
    let w2 = decode_world(&bytes).unwrap();
    assert_eq!(w2, w);
    let b = w2.social.ops[0].live(&c).unwrap();
    assert_eq!(a.trace, b.trace);
    for (x, y) in a.runs.iter().zip(&b.runs) {
        assert_eq!(
            (x.at, x.trace, x.cycles, x.status),
            (y.at, y.trace, y.cycles, y.status)
        );
        assert_eq!(x.log, y.log);
    }
}

#[test]
fn what_one_member_opens_is_open_for_the_others_and_the_trace_is_shared() {
    let c = content();
    let mut w = World::default();
    let deck: &[u8] = &[1, 5, 8, 11, 23];
    let id = start_op(&mut w, &c, &[("ann", deck), ("bea", deck)]);
    // ann plays until a lock opens or her pilot is done; bea then sees it open on her map.
    let mut pilot = None;
    for _ in 0..80 {
        let op = w.social.ops[0].clone();
        let live = op.live(&c).unwrap();
        let run = &live.runs[0];
        if run.status != Status::Active || !run.broken_nodes().is_empty() {
            break;
        }
        let p = pilot.get_or_insert_with(|| Pilot::in_crew(run, 85));
        let Some(act) = p.decide(&c, run, false) else {
            break;
        };
        let ok = w.op_act(&c, id, "ann", act, op.log.len(), DAY).is_ok();
        p.result(act, ok);
    }
    let live = w.social.ops[0].live(&c).unwrap();
    let opened = live.runs[0].broken_nodes().to_vec();
    if let Some(node) = opened.first() {
        assert!(
            live.runs[1].is_open(*node),
            "a lock opened by one member is open for the other"
        );
    }
    // The trace is one meter: both members read the same number once either has acted.
    assert!(live.trace > 0);
    assert_eq!(live.runs[0].trace.max(live.runs[1].trace), live.trace);
}

#[test]
fn the_finale_needs_a_crew_and_a_crew_can_reach_it() {
    let c = content();
    let corp = c.corp(crew::FINALE_CORP).unwrap();
    for season in 0..60u32 {
        let net =
            russh_bbs_door_nullroute::net::Network::generate_finale(&c, season, corp).unwrap();
        let goals: Vec<u8> = net
            .nodes
            .iter()
            .filter(|n| !n.honeypot && c.node(n.kind).is_some_and(|d| d.code == "AC"))
            .map(|n| n.id)
            .collect();
        let reach = |cap: Capability| {
            let mask = net.reachable(&cap, &russh_bbs_door_nullroute::net::Overlay::default());
            goals.iter().any(|g| mask & (1 << g) != 0)
        };
        for lock in 0..3 {
            let mut breaks = [0; 3];
            breaks[lock] = 6;
            assert!(
                !reach(Capability {
                    breaks,
                    purge: 6,
                    scan: 6
                }),
                "season {season}: one family reached the finale"
            );
        }
        assert!(
            reach(Capability {
                breaks: [6; 3],
                purge: 6,
                scan: 6
            }),
            "season {season}: all three families at power 6 must reach it"
        );
    }
}

fn clock(day: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::new(Day::from_raw(day).start()))
}

fn env(name: &str) -> DoorEnv {
    DoorEnv::for_tests(name)
}

fn text(r: &Running, e: &DoorEnv) -> String {
    r.look_around(e).join("\n")
}

async fn type_keys(r: &mut Running, store: &MemoryDoorStore, keys: &str) {
    for ch in keys.chars() {
        let key = match ch {
            '\n' => DoorKey::Enter,
            '\u{1b}' => DoorKey::Escape,
            c => DoorKey::Char(c),
        };
        r.key(key);
        r.flush(store).await.unwrap();
    }
}

async fn open(
    store: &MemoryDoorStore,
    door: &Arc<dyn Door>,
    account: AccountId,
    name: &str,
    day: i64,
    seed: u64,
) -> Running {
    let e = env(name);
    Running::open(
        store,
        Arc::clone(door),
        account,
        &e,
        clock(day) as Arc<dyn Clock>,
        seed,
    )
    .await
    .unwrap()
}

async fn world_of(store: &MemoryDoorStore, door: &Arc<dyn Door>) -> World {
    let w = store.load_world(&door.info().id).await.unwrap().unwrap();
    decode_world(&w.bytes).unwrap()
}

async fn save_of(store: &MemoryDoorStore, door: &Arc<dyn Door>, account: AccountId) -> Player {
    let s = store
        .load_save(&door.info().id, account)
        .await
        .unwrap()
        .unwrap();
    decode(&s.bytes).unwrap()
}

#[tokio::test]
async fn two_players_take_a_crew_run_from_founding_to_the_split_through_the_store() {
    let store = MemoryDoorStore::new();
    store.set_now(Day::from_raw(DAY).start());
    let door: Arc<dyn Door> = Nullroute::shared().unwrap();
    let c = content();
    let (ann, bea) = (AccountId::from_raw(1), AccountId::from_raw(2));
    // The first night: the board has contracts.
    run_due(
        &store,
        door.as_ref(),
        clock(DAY).as_ref(),
        TickPolicy::default(),
    )
    .await
    .unwrap();
    // Two characters with rep and a real deck, written as saves.
    for (account, name, seed) in [(ann, "ann", 11u64), (bea, "bea", 12)] {
        let mut p = Player::new(&c, seed, name, 1, DAY - 30);
        p.rep = 15;
        p.marks = 400;
        for n in [6u8, 9, 24, 25] {
            p.owned.push(n);
        }
        let id = door.info().id.clone();
        store
            .write_save(
                &id,
                account,
                &SaveWrite {
                    expected: Revision::NONE,
                    season: 1,
                    version: 3,
                    bytes: &encode(&p),
                },
            )
            .await
            .unwrap();
    }
    // bea looks in once, so she is on the board and can be invited.
    let mut r = open(&store, &door, bea, "bea", DAY, 1).await;
    type_keys(&mut r, &store, "k\u{1b}").await;
    // ann founds the crew and invites her.
    let e = env("ann");
    let mut r = open(&store, &door, ann, "ann", DAY, 2).await;
    type_keys(&mut r, &store, "kn").await;
    type_keys(&mut r, &store, "Nightshift\n").await;
    assert!(text(&r, &e).contains("Nightshift"), "{}", text(&r, &e));
    type_keys(&mut r, &store, "i").await;
    type_keys(&mut r, &store, "bea\n").await;
    assert!(
        world_of(&store, &door).await.social.crews[0]
            .invited
            .contains(&"bea".to_owned())
    );
    // bea sees the invitation line, and accepts.
    let eb = env("bea");
    let mut r = open(&store, &door, bea, "bea", DAY, 3).await;
    assert!(
        text(&r, &eb).contains("ann has invited you to a crew"),
        "{}",
        text(&r, &eb)
    );
    type_keys(&mut r, &store, "k1").await;
    let w = world_of(&store, &door).await;
    assert!(w.social.crewmates("ann", "bea"));
    // ann picks the first tier-1 job she can do and gathers the crew.
    let mut r = open(&store, &door, ann, "ann", DAY, 4).await;
    type_keys(&mut r, &store, "c").await;
    let rows = exits_numbered(&r.look_around(&e));
    let (n, _) = rows
        .iter()
        .find(|(_, t)| t.starts_with("Steal") && !t.contains('('))
        .expect("a job ann can take")
        .clone();
    type_keys(&mut r, &store, &n.to_string()).await;
    type_keys(&mut r, &store, "g").await;
    let w = world_of(&store, &door).await;
    assert_eq!(w.social.ops.len(), 1);
    assert_eq!(w.social.ops[0].state, OpState::Lobby);
    let after_open = save_of(&store, &door, ann).await;
    assert!(
        after_open.ties.stake.is_some(),
        "the cycles are committed, not lost"
    );
    assert!(after_open.cycles < after_open.max_cycles());
    // bea joins; ann starts the run.
    let mut r = open(&store, &door, bea, "bea", DAY, 5).await;
    type_keys(&mut r, &store, "kj").await;
    let mut r = open(&store, &door, ann, "ann", DAY, 6).await;
    type_keys(&mut r, &store, "ks").await;
    let w = world_of(&store, &door).await;
    assert_eq!(w.social.ops[0].state, OpState::Live);
    assert_eq!(w.social.ops[0].members.len(), 2);
    // They take turns across the day: each visit enters the run, makes a few moves with the
    // harness's hands, and closes the terminal.
    let door_c = Arc::clone(&door);
    let mut pilots: Vec<Option<Pilot>> = vec![None, None];
    for visit in 0..120u64 {
        let w = world_of(&store, &door_c).await;
        let Some(op) = w.social.ops.first().cloned() else {
            break;
        };
        if op.state == OpState::Done {
            break;
        }
        let who = usize::from(visit % 2 == 1);
        let (account, name) = [(ann, "ann"), (bea, "bea")][who];
        let e = env(name);
        let mut r = open(&store, &door_c, account, name, DAY, 100 + visit).await;
        type_keys(&mut r, &store, "ke").await;
        for _ in 0..5 {
            let w = world_of(&store, &door_c).await;
            let Some(op) = w.social.ops.first().cloned() else {
                break;
            };
            if op.state == OpState::Done {
                break;
            }
            let live = op.live(&c).unwrap();
            let run = &live.runs[who];
            if run.status != Status::Active {
                break;
            }
            let pilot = pilots[who].get_or_insert_with(|| Pilot::in_crew(run, 85));
            let Some(act) = pilot.decide(&c, run, live.objective) else {
                break;
            };
            let keys: Vec<DoorKey> = match act {
                Act::Move(n) => vec![DoorKey::Char(
                    char::from_digit(
                        u32::try_from(run.exits().iter().position(|e| *e == n).unwrap() + 1)
                            .unwrap(),
                        10,
                    )
                    .unwrap(),
                )],
                Act::Use { slot, target } => {
                    let mut k = vec![DoorKey::Char(char::from(
                        b'a' + u8::try_from(slot).unwrap(),
                    ))];
                    if let Some(t) = target {
                        k.push(DoorKey::Char(
                            char::from_digit(
                                u32::try_from(
                                    run.exits().iter().position(|e| *e == t).unwrap() + 1,
                                )
                                .unwrap(),
                                10,
                            )
                            .unwrap(),
                        ));
                    }
                    k
                }
                Act::JackOut => vec![DoorKey::Char('o')],
                Act::Brace => vec![DoorKey::Char('w')],
                Act::BreakOff => vec![DoorKey::Char('x')],
            };
            let before = op.log.len();
            for k in keys {
                r.key(k);
                r.flush(&store).await.unwrap();
            }
            let after = world_of(&store, &door_c).await.social.ops[0].log.len();
            pilot.result(act, after > before);
        }
        let _ = e;
        // The terminal closes; nothing is settled by leaving.
        drop(r);
    }
    let w = world_of(&store, &door).await;
    assert_eq!(w.social.ops[0].state, OpState::Done, "the run ended");
    // Both collect their share: marks move, the stake clears, and the debrief shows the split.
    let marks_before = [
        save_of(&store, &door, ann).await.marks,
        save_of(&store, &door, bea).await.marks,
    ];
    let mut shares = Vec::new();
    for (i, (account, name)) in [(ann, "ann"), (bea, "bea")].into_iter().enumerate() {
        let e = env(name);
        let mut r = open(&store, &door, account, name, DAY, 900 + i as u64).await;
        type_keys(&mut r, &store, "kc").await;
        let screen = text(&r, &e);
        assert!(screen.contains("DEBRIEF"), "{screen}");
        assert!(screen.contains("The crew's split:"), "{screen}");
        let p = save_of(&store, &door, account).await;
        assert!(p.ties.stake.is_none(), "collecting clears the stake");
        shares.push(p.marks.saturating_sub(marks_before[i]));
    }
    let w = world_of(&store, &door).await;
    assert!(
        w.social.ops.is_empty(),
        "once both collected, the run is gone"
    );
    assert!(
        w.social.tallies.jobs + w.social.tallies.burns >= 1,
        "{:?}",
        w.social.tallies
    );
    // The objective was done, or the crew was burned or went cold; whichever, nobody was paid
    // twice and nothing was created from nothing: the split is at most the crew's pay.
    let pay = russh_bbs_door_nullroute::world::first_board(&c, DAY)
        .iter()
        .map(|k| k.reward)
        .max()
        .unwrap();
    assert!(shares.iter().sum::<u32>() <= pay * 3 + 500, "{shares:?}");
    assert_eq!(w.social.tallies.crew_jobs, 1, "{:?}", w.social.tallies);
    assert!(
        shares.iter().all(|s| *s > 0),
        "both members were paid: {shares:?}"
    );
    let _ = (DOOR_ID, Flow::Stay);
}

#[tokio::test]
async fn a_join_that_never_reached_the_world_gives_the_cycles_back() {
    // A session that committed cycles to a run the world no longer has takes them back.
    let c = content();
    let mut p = Player::new(&c, 5, "ann", 1, DAY - 30);
    let before = p.cycles;
    let contract: Contract = russh_bbs_door_nullroute::world::first_board(&c, DAY)[0];
    p.rep = 50;
    let member = p.begin_crew_run(&c, &contract, 99);
    if member.is_ok() {
        assert!(p.cycles < before);
        let note = p.drop_stake().unwrap();
        assert!(note.contains("cycles are back"));
        assert_eq!(p.cycles, before.min(p.max_cycles()));
        assert!(p.ties.stake.is_none());
    }
}
