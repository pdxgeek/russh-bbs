//! The social layer: hacks on hideouts, honeypots, bounties, heat between players, the caps on
//! each, the opt-out, and the receipts that make a dropped edit cost time and nothing else.
#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::many_single_char_names,
    clippy::assert_is_empty,
    clippy::expect_used
)]

mod common;

use common::*;
use russh_bbs_door::DoorRng;
use russh_bbs_door_nullroute::player::{Player, SENT_MAX, stat};
use russh_bbs_door_nullroute::social::{
    AUTO_BOUNTIES_MAX, AUTO_BOUNTY_HEAT, BOUNTY_STAKE, Effect, HACK_CYCLES, HONEYPOT_MAX, Lines,
    PAIR_DAYS, PEACE_DAYS, PROTECT_DAYS, Posted, Social, Why,
};
use russh_bbs_door_nullroute::world::World;

const DAY: i64 = 20_200;

fn post(name: &str, tier: u8) -> Posted {
    Posted {
        handle: name.into(),
        tier,
        heat: 0,
        honeypot: 0,
        shield: 0,
        edge: 6,
        cash: 20,
        rep: u32::from(tier) * 20,
        born: DAY - 30,
        seen: DAY,
        peace_until: 0,
        pvp: true,
        crews_open: true,
    }
}

fn board(names: &[(&str, u8)]) -> Social {
    let mut s = Social::default();
    for (n, t) in names {
        s.post(post(n, *t));
    }
    s
}

#[test]
fn every_cap_on_a_hack_is_enforced_in_one_place() {
    let mut s = board(&[("ann", 2), ("bob", 2), ("cat", 2), ("dee", 2), ("low", 1)]);
    s.post(Posted {
        born: DAY - 2,
        ..post("new", 2)
    });
    // A new character is safe for a week, and counts from the day they were made.
    assert_eq!(s.check_hack("ann", "new", DAY), Err(Why::Protected));
    assert_eq!(s.check_hack("ann", "new", DAY + PROTECT_DAYS), Ok(()));
    // Nobody hacks themselves, and no tier 3 reaches down two tiers (one tier is allowed).
    assert_eq!(s.check_hack("ann", "ann", DAY), Err(Why::SelfTarget));
    s.post(post("big", 3));
    assert_eq!(s.check_hack("big", "low", DAY), Err(Why::TooLow));
    assert_eq!(s.check_hack("big", "bob", DAY), Ok(()));
    // A day's limit per attacker, per target, and per pair.
    s.file_hack("ann", "bob", DAY, 1).unwrap();
    assert_eq!(s.check_hack("ann", "bob", DAY + 1), Err(Why::Cooldown));
    assert_eq!(s.check_hack("ann", "bob", DAY + PAIR_DAYS), Ok(()));
    s.file_hack("ann", "cat", DAY, 2).unwrap();
    assert_eq!(s.check_hack("ann", "dee", DAY), Err(Why::TooMany));
    s.file_hack("dee", "bob", DAY, 3).unwrap();
    assert_eq!(s.check_hack("cat", "bob", DAY), Err(Why::Swamped));
    // A refusal always hands the cycles back; a filing always leaves a receipt.
    assert_eq!(s.file_hack("cat", "bob", DAY, 4), Err(Why::Swamped));
    let back = s.deliveries.iter().find(|d| d.nonce == 4).unwrap();
    assert_eq!(
        (back.effect, back.n, back.to.as_str()),
        (Effect::Cycles, i32::try_from(HACK_CYCLES).unwrap(), "cat")
    );
    for nonce in [1, 2, 3] {
        assert!(
            s.deliveries
                .iter()
                .any(|d| d.nonce == nonce && d.effect == Effect::None)
        );
    }
    // The orders are capped in all.
    let mut full = Social::default();
    for i in 0..60 {
        full.post(post(&format!("a{i}"), 2));
    }
    let mut filed = 0;
    for i in 0..60 {
        for j in 0..60 {
            if i != j
                && full
                    .file_hack(&format!("a{i}"), &format!("a{j}"), DAY, 9)
                    .is_ok()
            {
                filed += 1;
            }
        }
    }
    assert!(
        filed <= russh_bbs_door_nullroute::social::ORDERS_MAX,
        "{filed}"
    );
    assert!(full.orders.len() <= russh_bbs_door_nullroute::social::ORDERS_MAX);
}

#[test]
fn the_opt_out_takes_a_player_out_of_hacks_both_ways_and_out_of_bounties() {
    let mut s = board(&[("ann", 2), ("bob", 2)]);
    s.post(Posted {
        pvp: false,
        ..post("bob", 2)
    });
    assert_eq!(s.check_hack("ann", "bob", DAY), Err(Why::TargetGone));
    assert_eq!(s.check_hack("bob", "ann", DAY), Err(Why::NotPosted));
    assert_eq!(s.check_bounty("ann", "bob", 100), Err(Why::TargetGone));
    assert_eq!(s.check_bounty("bob", "ann", 100), Err(Why::NotPosted));
    // Crews are not PvP: an opted-out player still takes an invitation.
    s.crew_create("Night", "ann", DAY).unwrap();
    s.crew_invite("ann", "bob", DAY).unwrap();
    // The character: off is immediate, back on takes a couple of days, and a hack costs nothing
    // while off.
    let c = content();
    let mut p = Player::new(&c, 1, "bob", 1, DAY - 30);
    p.ties.pvp = true;
    assert!(p.set_pvp(false, DAY).is_ok());
    assert!(p.begin_hack().is_err());
    assert!(p.begin_bounty(100).is_err());
    assert!(p.set_pvp(true, DAY + 1).is_err());
    assert!(p.set_pvp(true, DAY + 2).is_ok());
    assert!(!p.posted(DAY).pvp || p.ties.pvp);
}

#[test]
fn a_breach_takes_a_tenth_up_to_the_cap_leaves_peace_and_tells_both_sides() {
    let c = content();
    let mut s = board(&[("ann", 2), ("bob", 2)]);
    // A fortune on hand: the take is capped.
    s.posted
        .iter_mut()
        .find(|p| p.handle == "bob")
        .unwrap()
        .cash = 5_000;
    s.posted
        .iter_mut()
        .find(|p| p.handle == "ann")
        .unwrap()
        .edge = 10;
    // Try nights until one gets in (the roll is seeded, so this is the same every time).
    let mut got = None;
    for night in 0..30 {
        let mut t = s.clone();
        t.file_hack("ann", "bob", DAY, 1).unwrap();
        t.settle(&c, &mut DoorRng::from_label("hack", night), DAY + 1);
        if t.tallies.hacks_in == 1 {
            got = Some(t);
            break;
        }
    }
    let t = got.expect("a hack with an edge of 10 against no shield gets in on some night");
    let mine = t
        .deliveries
        .iter()
        .find(|d| d.to == "ann" && d.line == Lines::HACK_ATT)
        .unwrap()
        .clone();
    let theirs = t
        .deliveries
        .iter()
        .find(|d| d.to == "bob" && d.line == Lines::HACK_DEF)
        .unwrap()
        .clone();
    assert_eq!(
        mine.n,
        i32::try_from(russh_bbs_door_nullroute::social::STEAL_MAX).unwrap()
    );
    assert_eq!(theirs.n, -mine.n, "what one gets the other loses");
    let peace = t.posted_of("bob").unwrap().peace_until;
    assert_eq!(peace, DAY + 1 + PEACE_DAYS);
    // Left in peace: a second hack on the same night is refused.
    let mut t = t;
    t.post(post("cat", 2));
    t.post(post("dee", 2));
    assert_eq!(t.check_hack("cat", "bob", DAY + 1), Err(Why::Peace));
    // The player folds the delivery in once, and a debit never takes more than they have.
    let mut bob = Player::new(&c, 2, "bob", 1, DAY - 30);
    bob.marks = 10;
    let notes = bob.fold(&c, &t, DAY + 1);
    assert_eq!(bob.marks, 0, "a debit takes what is there, no more");
    assert!(
        notes
            .iter()
            .any(|n| n.contains("ann got into your hideout")),
        "{notes:?}"
    );
    assert!(
        bob.fold(&c, &t, DAY + 1).is_empty(),
        "applied once, never twice"
    );
    let mut ann = Player::new(&c, 3, "ann", 1, DAY - 30);
    let before = ann.marks;
    ann.fold(&c, &t, DAY + 1);
    assert_eq!(ann.marks, before + u32::try_from(mine.n).unwrap());
    assert_eq!(ann.ties.stats[stat::IN], 1);
}

#[test]
fn a_honeypot_catches_hacks_more_often_the_more_levels_it_has() {
    let c = content();
    let mut caught = [0u32; 4];
    for level in 0..=HONEYPOT_MAX {
        for night in 0..200 {
            let mut s = board(&[("ann", 2), ("bob", 2)]);
            s.posted
                .iter_mut()
                .find(|p| p.handle == "bob")
                .unwrap()
                .honeypot = level;
            s.file_hack("ann", "bob", DAY, 1).unwrap();
            s.settle(&c, &mut DoorRng::from_label("trap", night), DAY + 1);
            caught[usize::from(level)] += s.tallies.hacks_caught;
        }
    }
    assert_eq!(caught[0], 0, "no honeypot catches nobody");
    assert!(caught[1] < caught[2] && caught[2] < caught[3], "{caught:?}");
    // 25 percent a level, give or take the dice.
    assert!((30..=70).contains(&caught[1]), "{caught:?}");
    assert!(caught[3] > 110, "{caught:?}");
    // A catch fines the hacker, warms their heat, and pays the defender in reputation.
    let mut s = board(&[("ann", 2), ("bob", 2)]);
    s.posted
        .iter_mut()
        .find(|p| p.handle == "bob")
        .unwrap()
        .honeypot = 3;
    for night in 0..50 {
        let mut t = s.clone();
        t.file_hack("ann", "bob", DAY, 1).unwrap();
        t.settle(&c, &mut DoorRng::from_label("trap2", night), DAY + 1);
        if t.tallies.hacks_caught == 1 {
            assert!(
                t.deliveries
                    .iter()
                    .any(|d| d.to == "ann" && d.effect == Effect::Marks && d.n < 0)
            );
            assert!(
                t.deliveries
                    .iter()
                    .any(|d| d.to == "ann" && d.effect == Effect::Heat && d.n > 0)
            );
            assert!(
                t.deliveries
                    .iter()
                    .any(|d| d.to == "bob" && d.effect == Effect::Rep)
            );
            return;
        }
    }
    panic!("no catch in 50 nights at three levels");
}

#[test]
fn a_honeypot_is_bought_a_level_at_a_time_and_stops_at_three() {
    let c = content();
    let mut p = Player::new(&c, 1, "ann", 1, DAY);
    p.marks = 10_000;
    let mut spent = 0;
    for level in 1..=HONEYPOT_MAX {
        let before = p.marks;
        assert!(p.buy_honeypot().is_ok());
        spent += before - p.marks;
        assert_eq!(p.ties.honeypot, level);
    }
    assert_eq!(spent, 60 + 120 + 240);
    assert!(p.buy_honeypot().is_err(), "three levels is the most");
    let mut poor = Player::new(&c, 2, "bea", 1, DAY);
    poor.marks = 59;
    assert!(poor.buy_honeypot().is_err());
    assert_eq!(poor.marks, 59, "a refusal costs nothing");
}

#[test]
fn bounties_are_capped_by_stake_by_poster_and_by_target_and_pay_a_hack_that_gets_in() {
    let c = content();
    let mut s = board(&[("ann", 2), ("bob", 2), ("cat", 3), ("low", 1)]);
    // The stake is 100 to 400; a refusal gives it back.
    assert_eq!(
        s.check_bounty("ann", "bob", BOUNTY_STAKE.0 - 1),
        Err(Why::Stake)
    );
    assert_eq!(
        s.check_bounty("ann", "bob", BOUNTY_STAKE.1 + 1),
        Err(Why::Stake)
    );
    let r = s.post_bounty(&c, "ann", "low", 100, "", DAY, 5);
    assert_eq!(
        r,
        Err(Why::Below),
        "a bounty is for rivals at the poster's tier or above"
    );
    let back = s.deliveries.iter().find(|d| d.nonce == 5).unwrap();
    assert_eq!((back.effect, back.n), (Effect::Marks, 100));
    s.post_bounty(&c, "ann", "bob", 200, "pay up", DAY, 6)
        .unwrap();
    // The target is told, and the fixer keeps a fifth.
    let b = &s.bounties[0];
    assert_eq!(b.reward, 160);
    assert!(
        s.deliveries
            .iter()
            .any(|d| d.to == "bob" && d.line == Lines::BOUNTY_ON_YOU && d.n == 160)
    );
    // One bounty per poster per target, a few per poster, a few per target.
    assert_eq!(s.check_bounty("ann", "bob", 100), Err(Why::TargetCap));
    s.post_bounty(&c, "ann", "cat", 100, "", DAY, 7).unwrap();
    s.post("dee_".into_post());
    assert_eq!(s.check_bounty("ann", "dee_", 100), Err(Why::PosterCap));
    s.post(post("zed", 2));
    s.post_bounty(&c, "zed", "bob", 100, "", DAY, 8).unwrap();
    s.post("eve".into_post());
    assert_eq!(s.check_bounty("eve", "bob", 100), Err(Why::TargetCap));
    // A hack that gets in claims one: the hacker is paid, the target's rig is hurt and warmed,
    // the poster is told, the street hears, and the bounty is gone. Never one's own bounty, and
    // not for a hacker above the target's tier (cat is tier 3; bob is tier 2).
    let mut s2 = s.clone();
    s2.posted
        .iter_mut()
        .find(|p| p.handle == "ann")
        .unwrap()
        .edge = 10;
    s2.posted
        .iter_mut()
        .find(|p| p.handle == "cat")
        .unwrap()
        .edge = 10;
    let mut done = false;
    for night in 0..40 {
        let mut t = s2.clone();
        t.file_hack("ann", "bob", DAY, 11).unwrap();
        t.settle(&c, &mut DoorRng::from_label("bounty", night), DAY + 1);
        if t.tallies.bounties_paid > 0 {
            assert!(
                t.deliveries
                    .iter()
                    .any(|d| d.to == "ann" && d.line == Lines::BOUNTY_ATT && d.n == 80)
            );
            assert!(
                t.deliveries
                    .iter()
                    .any(|d| d.to == "bob" && d.effect == Effect::Fry)
            );
            assert!(
                t.deliveries
                    .iter()
                    .any(|d| d.to == "zed" && d.line == Lines::BOUNTY_DONE)
            );
            assert!(
                !t.deliveries
                    .iter()
                    .any(|d| d.to == "ann" && d.line == Lines::BOUNTY_DONE),
                "ann cannot claim the bounty she posted"
            );
            assert!(t.deeds.iter().any(|d| d.line == Lines::DEED_BOUNTY));
            assert_eq!(t.bounties.iter().filter(|b| b.target == "bob").count(), 1);
            done = true;
            break;
        }
    }
    assert!(done, "a bounty is paid to a hack that gets in");
    let mut high = s2.clone();
    for night in 0..40 {
        let mut t = high.clone();
        t.file_hack("cat", "bob", DAY, 12).unwrap();
        t.settle(&c, &mut DoorRng::from_label("bounty2", night), DAY + 1);
        assert_eq!(
            t.tallies.bounties_paid, 0,
            "a tier 3 hacker cannot claim a price on a tier 2 head"
        );
        high = s2.clone();
    }
}

trait IntoPost {
    fn into_post(self) -> Posted;
}
impl IntoPost for &str {
    fn into_post(self) -> Posted {
        post(self, 2)
    }
}

#[test]
fn an_unclaimed_bounty_lapses_and_gives_back_the_stake_less_the_fixers_fifth() {
    let c = content();
    let mut s = board(&[("ann", 2), ("bob", 2)]);
    s.post_bounty(&c, "ann", "bob", 200, "", DAY, 1).unwrap();
    s.settle(&c, &mut DoorRng::from_seed(1), DAY + 20);
    assert!(s.bounties.is_empty());
    let back = s
        .deliveries
        .iter()
        .find(|d| d.line == Lines::BOUNTY_LAPSED)
        .unwrap();
    assert_eq!(
        (back.to.as_str(), back.effect, back.n),
        ("ann", Effect::Marks, 160)
    );
}

#[test]
fn a_fixer_puts_a_price_on_the_hottest_names_and_no_more_than_a_few() {
    let c = content();
    let mut s = Social::default();
    for i in 0..12 {
        s.post(Posted {
            heat: u8::try_from(AUTO_BOUNTY_HEAT).unwrap() + u8::try_from(i).unwrap(),
            ..post(&format!("h{i}"), 2)
        });
    }
    s.post(Posted {
        heat: 10,
        ..post("cool", 2)
    });
    s.settle(&c, &mut DoorRng::from_seed(3), DAY);
    let autos = s.bounties.iter().filter(|b| b.poster.is_empty()).count();
    assert_eq!(autos, AUTO_BOUNTIES_MAX);
    assert!(!s.bounties.iter().any(|b| b.target == "cool"));
    // The hottest first; and the target is told.
    assert!(s.bounties.iter().any(|b| b.target == "h11"));
    assert!(
        s.deliveries
            .iter()
            .any(|d| d.to == "h11" && d.line == Lines::BOUNTY_ON_YOU)
    );
    // Settling again does not stack a second price on the same head.
    s.settle(&c, &mut DoorRng::from_seed(4), DAY);
    assert_eq!(s.bounties.iter().filter(|b| b.target == "h11").count(), 1);
}

#[test]
fn an_edit_that_never_ran_gives_the_stake_back_and_a_ran_one_does_not_double() {
    let c = content();
    let mut p = Player::new(&c, 9, "ann", 1, DAY);
    p.marks = 500;
    let cycles = p.cycles;
    // A hack and a bounty are sent; the world drops both edits (no receipt anywhere).
    let a = p.begin_hack().unwrap();
    let b = p.begin_bounty(200).unwrap();
    assert_ne!(a, b);
    assert_eq!((p.marks, p.cycles), (300, cycles - HACK_CYCLES));
    let empty = Social::default();
    let notes = p.fold(&c, &empty, DAY + 1);
    assert_eq!((p.marks, p.cycles.min(cycles)), (500, p.cycles));
    assert_eq!(p.cycles, (cycles).min(p.max_cycles()), "{notes:?}");
    assert!(p.ties.sent.is_empty());
    // The same two edits, this time with receipts: the bounty is a bounty, the hack an order.
    let mut s = board(&[("ann", 2), ("bob", 2)]);
    let a = p.begin_hack().unwrap();
    let b = p.begin_bounty(200).unwrap();
    s.file_hack("ann", "bob", DAY, a).unwrap();
    s.post_bounty(&c, "ann", "bob", 200, "", DAY, b).unwrap();
    let marks = p.marks;
    p.fold(&c, &s, DAY + 1);
    assert_eq!(p.marks, marks, "a stake the world kept is not given back");
    assert!(p.ties.sent.is_empty());
    // And a receipt is folded once.
    let again = p.fold(&c, &s, DAY + 1);
    assert!(again.is_empty());
    // Too many waiting orders at once is refused up front.
    let mut q = Player::new(&c, 10, "bea", 1, DAY);
    q.marks = 10_000;
    q.cycles = 1_000;
    for _ in 0..SENT_MAX {
        q.begin_bounty(100).unwrap();
    }
    assert!(q.begin_bounty(100).is_err());
}

#[test]
fn deeds_and_the_season_are_bounded_and_the_season_closes_into_the_hall() {
    let c = content();
    let mut w = World::default();
    w.sync_season(&c, 1);
    for i in 0..60 {
        w.social.deed(DAY, Lines::DEED_WARDEN, &format!("h{i}"), 1);
    }
    assert_eq!(
        w.social.deeds.len(),
        russh_bbs_door_nullroute::social::DEEDS_MAX
    );
    w.social.post(post("ann", 3));
    w.social.tallies.wardens = 9;
    w.sync_season(&c, 2);
    assert_eq!(w.social.hall.len(), 1);
    let rec = &w.social.hall[0];
    assert_eq!(rec.season, 1);
    assert_eq!(rec.top[0].0, "ann");
    // Nine wardens down is a season of its own kind, and the tallies start over.
    let title = c
        .seasons
        .season_ending
        .iter()
        .find(|e| e.num == rec.ending)
        .unwrap();
    assert_eq!(title.title, "Wardens' winter");
    assert_eq!(w.social.tallies.wardens, 0);
    // The hall keeps eight seasons.
    for s in 3..30 {
        w.sync_season(&c, s);
    }
    assert_eq!(
        w.social.hall.len(),
        russh_bbs_door_nullroute::social::HALL_MAX
    );
}

#[test]
fn text_one_player_shows_another_is_a_template_or_one_vetted_line() {
    let c = content();
    // Every line a delivery or a deed can say is in the table and has only the two placeholders.
    for n in [
        Lines::FILED,
        Lines::REFUSED,
        Lines::HACK_ATT,
        Lines::HACK_DEF,
        Lines::FAIL_ATT,
        Lines::FAIL_DEF,
        Lines::TRAP_ATT,
        Lines::TRAP_HEAT,
        Lines::TRAP_DEF,
        Lines::GONE,
        Lines::BOUNTY_ATT,
        Lines::BOUNTY_DEF,
        Lines::BOUNTY_HEAT,
        Lines::BOUNTY_DONE,
        Lines::BOUNTY_FILED,
        Lines::REFUSED_BOUNTY,
        Lines::BOUNTY_LAPSED,
        Lines::BOUNTY_ON_YOU,
        Lines::DEED_PRICE,
        Lines::DEED_BOUNTY,
        Lines::DEED_WARDEN,
        Lines::DEED_BURN,
        Lines::DEED_CREW,
        Lines::DEED_FINALE,
        Lines::DEED_DEEP,
        Lines::CREW_INVITED,
    ] {
        assert!(!c.line(n).is_empty(), "line {n} is missing");
        let said = c.say(n, "Brindle", 42);
        assert!(!said.contains('{') && !said.contains('}'), "{said}");
        assert!(said.chars().all(|ch| !ch.is_control()));
    }
}
