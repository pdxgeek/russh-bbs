//! Companies, rescues between heroes, the duel ring, notes, quest choices, the Furnace's verdict
//! and the endings: the rules in `social.rs`, `company.rs`, `story.rs` and `endings.rs`.

use super::support::*;
use crate::company::SocialAct;
use crate::game::{Action, Game, Place};
use crate::social::{BOUTS_PER_DAY, Bout, Commons, Invite, SocialChange, WAGERS};
use crate::world::{Change, Fallen, World};

fn named(name: &str, level: u8, seed: u64) -> Game {
    let mut hero = hero_of("warden", ["stout", "keen-eyed"]);
    hero.name = name.to_owned();
    hero.level = level;
    game_with(hero, seed)
}

/// Both games see one shared world, as two sessions would after a merge.
fn share(a: &mut Game, b: &mut Game) {
    for ch in a.take_changes() {
        b.world.apply(&ch);
    }
    for ch in b.take_changes() {
        a.world.apply(&ch);
    }
    let w = a.world.clone();
    b.world = w;
}

fn act(g: &mut Game, a: SocialAct) -> Result<(), String> {
    g.apply(&content(), Action::Social(a))
        .map(|_| ())
        .map_err(|r| r.0)
}

fn pair() -> (Game, Game) {
    let mut a = named("Ann", 4, 1);
    let mut b = named("Bo", 4, 2);
    a.sync_roster(&content());
    b.sync_roster(&content());
    share(&mut a, &mut b);
    (a, b)
}

#[test]
fn an_invitation_then_acceptance_makes_a_company_and_no_other_way_does() {
    let (mut a, mut b) = pair();
    // Joining with no invitation changes nothing.
    b.world.apply(&Change::Social(SocialChange::Join {
        leader: "Ann".into(),
        name: "Bo".into(),
    }));
    assert!(b.world.commons.parties.is_empty());
    assert!(act(&mut b, SocialAct::Accept("Ann".into())).is_err());

    act(&mut a, SocialAct::Invite("Bo".into())).unwrap();
    assert!(act(&mut a, SocialAct::Invite("Bo".into())).is_err(), "once");
    share(&mut a, &mut b);
    act(&mut b, SocialAct::Accept("Ann".into())).unwrap();
    share(&mut a, &mut b);
    let p = a.world.commons.party_of("Ann").unwrap();
    assert_eq!(p.members, vec!["Ann".to_owned(), "Bo".to_owned()]);
    assert!(a.world.commons.companions("Ann", "Bo"));
    // A third and no more than a third.
    let mut c = named("Cy", 4, 3);
    let mut d = named("Di", 4, 4);
    for g in [&mut c, &mut d] {
        g.sync_roster(&content());
    }
    share(&mut a, &mut c);
    share(&mut a, &mut d);
    act(&mut a, SocialAct::Invite("Cy".into())).unwrap();
    share(&mut a, &mut c);
    act(&mut c, SocialAct::Accept("Ann".into())).unwrap();
    share(&mut a, &mut c);
    assert_eq!(a.world.commons.party_of("Ann").unwrap().members.len(), 3);
    assert!(act(&mut a, SocialAct::Invite("Di".into())).is_err(), "full");
    // Leaving works and a company of one is no company.
    act(&mut c, SocialAct::Leave).unwrap();
    act(&mut b, SocialAct::Leave).unwrap();
    share(&mut b, &mut c);
    assert!(b.world.commons.parties.is_empty());
}

#[test]
fn opting_out_of_invitations_is_respected() {
    let (mut a, mut b) = pair();
    act(&mut b, SocialAct::Open(false)).unwrap();
    share(&mut a, &mut b);
    assert!(act(&mut a, SocialAct::Invite("Bo".into())).is_err());
}

#[test]
fn a_company_shares_the_packs_a_member_dealt_with_today_and_only_today() {
    let (mut a, mut b) = pair();
    act(&mut a, SocialAct::Invite("Bo".into())).unwrap();
    share(&mut a, &mut b);
    act(&mut b, SocialAct::Accept("Ann".into())).unwrap();
    share(&mut a, &mut b);
    let (depth, room) = place(&mut a, "slag-den");
    assert!(a.pack_cleared(depth, room) == a.today.is_cleared(depth, room));
    a.share_clear(depth, room);
    share(&mut a, &mut b);
    assert!(b.pack_cleared(depth, room), "Bo walks through Ann's work");
    assert!(!b.today.is_cleared(depth, room), "but it is not Bo's save");
    // A companion who walks into the den takes a third of the spoils, once.
    let c = content();
    let level = crate::dungeon::generate(&c, b.season, b.epoch, depth);
    let (coin, xp) = (b.hero.coin, b.hero.xp);
    let mut ev = Vec::new();
    b.company_spoils(&c, &level, room, &mut ev);
    assert!(b.hero.coin > coin && b.hero.xp > xp, "{}", said(&ev));
    let (coin, xp) = (b.hero.coin, b.hero.xp);
    b.company_spoils(&c, &level, room, &mut ev);
    assert_eq!((b.hero.coin, b.hero.xp), (coin, xp), "once a day");
    // The next day it is all fresh again.
    b.day += 1;
    assert!(!b.pack_cleared(depth, room));
}

#[test]
fn a_rescue_pays_the_rescuer_by_note_once_and_a_companion_costs_less() {
    let c = content();
    // Ann falls; Bo, a stranger, carries her home.
    let (mut ann, mut bo) = pair();
    ann.place = Place::Dead {
        depth: 3,
        room: 2,
        since: TODAY,
    };
    ann.hero.coin = 100;
    ann.emit(Change::Fall(Fallen {
        name: "Ann".into(),
        depth: 3,
        room: 2,
        day: TODAY,
        rescued_by: None,
    }));
    share(&mut ann, &mut bo);
    bo.place = Place::Delve { depth: 3, room: 2 };
    let at = bo
        .options(&c)
        .iter()
        .position(|o| o.label == "Carry Ann home")
        .unwrap();
    assert_eq!(bo.options(&c)[at].cost, 3);
    bo.hero.turns = 25;
    run(&mut bo, Action::Option(at));
    assert_eq!(bo.hero.social.rescues, 1);
    share(&mut ann, &mut bo);
    let ev = ann.new_day(&c, TODAY + 1);
    assert_eq!(ann.place, Place::Town, "{}", said(&ev));
    assert_eq!(ann.hero.social.rescued, 1);
    let paid = 100 - ann.hero.coin;
    assert!(paid > 0 && paid <= 19);
    share(&mut ann, &mut bo);
    // Bo collects the thanks once, whatever happens to the world's copy.
    let before = bo.hero.coin;
    let mut ev = Vec::new();
    bo.collect_notes(&c, &mut ev);
    assert_eq!(bo.hero.coin, before + paid);
    bo.world
        .commons
        .notes
        .push(crate::social::Commons::thanks_note(
            "Ann",
            "Bo",
            TODAY + 1,
            paid,
        ));
    bo.collect_notes(&c, &mut ev);
    assert_eq!(bo.hero.coin, before + paid, "never twice");
}

#[test]
fn a_companion_carries_you_home_for_a_turn_less_and_with_no_wound() {
    let c = content();
    let (mut ann, mut bo) = pair();
    act(&mut ann, SocialAct::Invite("Bo".into())).unwrap();
    share(&mut ann, &mut bo);
    act(&mut bo, SocialAct::Accept("Ann".into())).unwrap();
    share(&mut ann, &mut bo);
    ann.place = Place::Dead {
        depth: 3,
        room: 2,
        since: TODAY,
    };
    ann.hero.coin = 100;
    ann.emit(Change::Fall(Fallen {
        name: "Ann".into(),
        depth: 3,
        room: 2,
        day: TODAY,
        rescued_by: None,
    }));
    share(&mut ann, &mut bo);
    bo.place = Place::Delve { depth: 3, room: 2 };
    bo.hero.turns = 25;
    let at = bo
        .options(&c)
        .iter()
        .position(|o| o.label == "Carry Ann home")
        .unwrap();
    assert_eq!(bo.options(&c)[at].cost, 2);
    run(&mut bo, Action::Option(at));
    share(&mut ann, &mut bo);
    assert_eq!(bo.world.commons.party_of("Bo").unwrap().rescues, 1);
    let _ = ann.new_day(&c, TODAY + 1);
    assert_eq!(ann.hero.coin, 100, "no thanks owed between companions");
    assert!(!ann.hero.wounds.iter().any(|(w, _)| w == "dragged-home"));
}

#[test]
fn the_ring_is_opt_in_capped_and_leaves_marks() {
    let c = content();
    let (mut a, mut b) = pair();
    assert!(
        act(
            &mut a,
            SocialAct::Challenge {
                target: "Bo".into(),
                wager: 0
            }
        )
        .is_err()
    );
    act(&mut a, SocialAct::Ring(true)).unwrap();
    share(&mut a, &mut b);
    // Bo is not in the ring, so cannot be called out.
    assert!(
        act(
            &mut a,
            SocialAct::Challenge {
                target: "Bo".into(),
                wager: 0
            }
        )
        .is_err()
    );
    act(&mut b, SocialAct::Ring(true)).unwrap();
    share(&mut a, &mut b);
    assert!(
        act(
            &mut a,
            SocialAct::Challenge {
                target: "Bo".into(),
                wager: 7
            }
        )
        .is_err(),
        "stakes are fixed"
    );
    a.hero.coin = 500;
    a.hero.turns = 25;
    act(
        &mut a,
        SocialAct::Challenge {
            target: "Bo".into(),
            wager: 20,
        },
    )
    .unwrap();
    assert!(
        a.take_changes()
            .iter()
            .any(|ch| matches!(ch, Change::Social(SocialChange::Duel(_))))
    );
    assert!(
        act(
            &mut a,
            SocialAct::Challenge {
                target: "Bo".into(),
                wager: 0
            }
        )
        .is_err(),
        "once a day per foe"
    );
    assert_eq!(a.today.bouts, 1);
    assert_eq!(a.world.commons.ladder.len(), 2, "both are on the ladder");
    assert!(a.hero.social.wins + a.hero.social.losses == 1);
    // The daily cap holds whoever the foes are.
    for (i, name) in ["Cy", "Di"].iter().enumerate() {
        let mut g = named(name, 4, 10 + i as u64);
        g.hero.social.ring = true;
        g.sync_roster(&c);
        share(&mut a, &mut g);
    }
    a.hero.turns = 25;
    let first = act(
        &mut a,
        SocialAct::Challenge {
            target: "Cy".into(),
            wager: 0,
        },
    );
    let second = act(
        &mut a,
        SocialAct::Challenge {
            target: "Di".into(),
            wager: 0,
        },
    );
    assert!(first.is_ok());
    assert_eq!(a.today.bouts, BOUTS_PER_DAY);
    assert!(second.is_err(), "the third call of the day is refused");
    // Heroes too far apart are not matched.
    let mut far = named("Zed", 12, 99);
    far.hero.social.ring = true;
    far.sync_roster(&c);
    share(&mut a, &mut far);
    a.today.bouts = 0;
    assert!(
        act(
            &mut a,
            SocialAct::Challenge {
                target: "Zed".into(),
                wager: 0
            }
        )
        .is_err()
    );
    assert!(WAGERS.contains(&20));
}

#[test]
fn a_duel_is_counted_once_and_a_loss_feeds_the_works_and_bruises_the_target() {
    let mut c = Commons::default();
    let bout = Bout {
        id: "Ann:Bo:1:0".into(),
        challenger: "Ann".into(),
        target: "Bo".into(),
        challenger_won: false,
        wager: 20,
        day: 5,
    };
    c.apply(&SocialChange::Duel(bout.clone()));
    c.apply(&SocialChange::Duel(bout));
    assert_eq!(c.duels, 1);
    assert_eq!(c.ring_fund, 20);
    assert_eq!(c.rank_of("Bo").unwrap().wins, 1);
    assert_eq!(c.rank_of("Ann").unwrap().losses, 1);
    assert!(
        c.notes.is_empty(),
        "the challenger lost: no bruise note for the target"
    );
    // And the ring fund goes to the works in the night.
    let content = content();
    let mut w = World::new(1);
    w.commons.ring_fund = 60;
    w.tick_full(5, &content);
    assert_eq!(w.commons.ring_fund, 0);
    let first = &content.projects.project[0].id;
    assert!(
        w.pledges
            .iter()
            .any(|p| p.hero == "the Ring" && &p.project == first)
    );
}

#[test]
fn social_changes_merge_in_any_order() {
    let invite = SocialChange::Invite(Invite {
        from: "Ann".into(),
        to: "Bo".into(),
        day: 1,
    });
    let join = SocialChange::Join {
        leader: "Ann".into(),
        name: "Bo".into(),
    };
    let mut once = Commons::default();
    for ch in [&invite, &join] {
        once.apply(ch);
    }
    let mut twice = Commons::default();
    for ch in [&invite, &join, &invite, &join, &join] {
        twice.apply(ch);
    }
    assert_eq!(once.parties, twice.parties);
    assert_eq!(twice.parties.len(), 1);
    // Round trip through bytes.
    let mut w = crate::codec::Writer::default();
    twice.encode(&mut w);
    let back = Commons::decode(&mut crate::codec::Reader(&w.0)).unwrap();
    assert_eq!(back, twice);
}

#[test]
fn the_quest_chain_advances_pays_and_a_choice_sets_a_flag_that_opens_a_verdict() {
    let c = content();
    let mut g = named("Ann", 8, 7);
    let mut ev = Vec::new();
    g.hero.best_depth = 25;
    g.hero.story.guardians = 0b1_1111;
    g.hero.given.push(("x".into(), 500, 0));
    g.hero.contracts_done = 20;
    g.hero.social.rescues = 20;
    g.hero.social.wins = 20;
    for q in &c.quests.quest {
        for s in &q.step {
            if let crate::content_story::Goal::Slay { monster, count } = &s.goal {
                for _ in 0..*count {
                    g.note_kill(monster);
                }
            }
            if let crate::content_story::Goal::Visit { room, count } = &s.goal {
                for _ in 0..*count {
                    g.note_new_room(*room);
                }
            }
        }
    }
    g.hero.level = 20;
    g.story_advance(&c, &mut ev);
    // Every line is now stopped at a choice, and only the choice moves it on.
    for q in &c.quests.quest {
        let waiting = g.quest_waiting(q).is_some();
        let done = g.quest_done(&q.id);
        assert!(waiting || done >= q.step.len(), "{} stuck at {done}", q.id);
        if let Some(step) = g.quest_waiting(q) {
            let first = step.choose[0].id.clone();
            g.choose(&c, &q.id, &first, &mut ev).unwrap();
        }
    }
    assert!(!g.hero.story.flags.is_empty());
    // A verdict needs the flag its choice set; quenching needs nothing.
    g.hero.story.verdict_due = true;
    let open: Vec<_> = g.verdicts().iter().map(|v| v.id).collect();
    assert!(open.contains(&"quench"));
    assert!(g.give_verdict("claim", &mut ev).is_ok() == open.contains(&"claim"));
}

#[test]
fn every_verdict_is_reachable_and_every_ending_is_reachable() {
    use crate::endings::{ENDINGS, collective};
    let c = content();
    let mut seen = std::collections::BTreeSet::new();
    for kind in ["quench", "bank", "claim", "open"] {
        for rescues in [0u16, 9] {
            for finished in [false, true] {
                let mut w = World::new(1);
                w.commons.verdict = Some((kind.to_owned(), "X".to_owned()));
                w.commons.rescues = rescues;
                w.commons.parties.push(crate::social::Party {
                    leader: "A".into(),
                    members: vec!["A".into(), "B".into()],
                    formed: 0,
                    day: 0,
                    cleared: vec![],
                    rescues: 1,
                    deepest: 0,
                });
                if finished {
                    for d in 1..=8 {
                        w.apply(&Change::Shortcut(
                            d,
                            crate::content::ShortcutKind::Lift,
                            "X".into(),
                        ));
                    }
                    for p in &c.projects.project {
                        finish(&mut w, &p.id);
                    }
                }
                seen.insert(collective(&w, &c));
            }
        }
    }
    assert_eq!(seen.len(), ENDINGS.len(), "{seen:?}");
}

#[test]
fn the_fall_of_the_furnace_writes_the_chronicle_then_collapses_the_dungeon() {
    let c = content();
    let mut w = World::new(1);
    w.apply(&Change::Slain(5, "Ann".into()));
    w.apply(&Change::Social(SocialChange::Verdict {
        kind: "quench".into(),
        by: "Ann".into(),
    }));
    w.apply(&Change::Social(SocialChange::Verdict {
        kind: "claim".into(),
        by: "Bo".into(),
    }));
    assert_eq!(
        w.commons.verdict.as_ref().unwrap().0,
        "quench",
        "the slayer's word stands"
    );
    w.tick_full(TODAY, &c);
    assert_eq!(w.epoch, 1);
    assert_eq!(w.commons.chronicle.len(), 1);
    assert!(w.commons.verdict.is_none());
}

#[test]
fn a_hero_not_found_leaves_an_epitaph_in_the_hall() {
    let c = content();
    let mut g = named("Ann", 4, 5);
    act(&mut g, SocialAct::Epitaph("She read the walls.".into())).unwrap();
    g.place = Place::Dead {
        depth: 4,
        room: 1,
        since: TODAY,
    };
    let _ = g.apply(&c, Action::GiveUp).unwrap();
    assert!(
        g.world
            .commons
            .epitaphs
            .iter()
            .any(|e| e.name == "Ann" && e.text == "She read the walls.")
    );
}

#[test]
fn events_lay_over_ordinary_rooms_and_give_builds_different_choices() {
    let c = content();
    assert!(!c.events.event.is_empty());
    let mut found = 0;
    for depth in 1..=25u8 {
        let level = crate::dungeon::generate(&c, 1, 0, depth);
        for (i, r) in level.rooms.iter().enumerate() {
            if r.event.is_some() {
                found += 1;
                assert!(level.event(&c, u8::try_from(i).unwrap()).is_some());
            }
        }
    }
    assert!(found > 0, "no level in the dungeon carries an event");
}
