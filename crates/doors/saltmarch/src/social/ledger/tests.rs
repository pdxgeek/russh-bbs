#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

use super::*;
use crate::clan::tests::fixture;
use crate::social::Receipt;
use crate::social::market::{Memo, Parcel};
use crate::social::pact::{Notice, NoticeKind};
use crate::social::raid::{Fate, Outcome, Report, Role};

fn pend(id: u16, kind: Kind, goods: Option<(Resource, u32)>, tallies: u32) -> Pending {
    Pending {
        id,
        day: 20_000,
        kind,
        about: "10 salt at 5".into(),
        goods,
        tallies,
        turns: 0,
        crew: Vec::new(),
    }
}

fn mail(deliveries: Vec<Delivery>, receipts: Vec<Receipt>) -> Mail {
    Mail {
        deliveries,
        receipts,
    }
}

fn delivery(seq: u32, body: Body) -> Delivery {
    Delivery { seq, to: 0, body }
}

#[test]
fn a_dispatch_the_world_never_saw_is_taken_back_exactly_once() {
    let (cat, _, mut clan) = fixture();
    clan.stock.take_up_to(Resource::Salt, 100);
    clan.stock.add(Resource::Salt, 4);
    let id = clan
        .social
        .begin(pend(0, Kind::Ask, Some((Resource::Salt, 5)), 2));
    let tallies = clan.tallies;
    let mut events = Vec::new();
    // The edit never ran: no receipt.
    let acks = fold(&cat, &mut clan, &mail(vec![], vec![]), 20_000, &mut events);
    assert_eq!(clan.stock.get(Resource::Salt), 9);
    assert_eq!(clan.tallies, tallies + 2);
    assert!(clan.social.pending.is_empty());
    assert!(matches!(events.as_slice(), [Event::Unsent { .. }]));
    assert!(acks.is_empty(), "nothing to clear in the world: {id}");
    // Folding again changes nothing.
    fold(&cat, &mut clan, &mail(vec![], vec![]), 20_000, &mut events);
    assert_eq!(clan.stock.get(Resource::Salt), 9);
}

#[test]
fn an_accepted_dispatch_keeps_the_escrow_in_the_world_and_a_refused_one_returns_it() {
    let (cat, _, mut clan) = fixture();
    clan.stock.take_up_to(Resource::Salt, 100);
    let a = clan
        .social
        .begin(pend(0, Kind::Ask, Some((Resource::Salt, 5)), 1));
    let b = clan.social.begin(pend(0, Kind::Bid, None, 30));
    let tallies = clan.tallies;
    let mut events = Vec::new();
    let receipts = vec![
        Receipt {
            clan: 0,
            id: a,
            refused: None,
        },
        Receipt {
            clan: 0,
            id: b,
            refused: Some(Refusal::TooMany),
        },
    ];
    let acks = fold(
        &cat,
        &mut clan,
        &mail(vec![], receipts),
        20_000,
        &mut events,
    );
    assert_eq!(
        clan.stock.get(Resource::Salt),
        0,
        "the ask's goods stay in the book"
    );
    assert_eq!(
        clan.tallies,
        tallies + 30,
        "the refused bid's tallies come back"
    );
    assert_eq!(acks.receipts, vec![a, b]);
    assert!(clan.social.pending.is_empty());
    assert!(matches!(
        events.as_slice(),
        [Event::Refused {
            why: Refusal::TooMany,
            ..
        }]
    ));
}

#[test]
fn a_delivery_is_folded_once_even_when_the_world_still_holds_it() {
    let (cat, _, mut clan) = fixture();
    let before = clan.tallies;
    let p = Body::Parcel(Parcel {
        goods: Some((Resource::Iron, 3)),
        tallies: 7,
        memo: Memo::Sold,
        about: Some((Resource::Iron, 3)),
    });
    let m = mail(vec![delivery(5, p)], vec![]);
    let mut events = Vec::new();
    let acks = fold(&cat, &mut clan, &m, 20_000, &mut events);
    assert_eq!(acks.upto, 5);
    assert_eq!(clan.tallies, before + 7);
    assert_eq!(clan.stock.get(Resource::Iron), 3);
    assert_eq!(clan.stats.earned, 7);
    // The acknowledgement was lost: the same mail comes again.
    let again = fold(&cat, &mut clan, &m, 20_000, &mut events);
    assert_eq!(clan.tallies, before + 7, "not paid twice");
    assert_eq!(clan.stock.get(Resource::Iron), 3);
    assert_eq!(again.upto, 5, "and the world is told to forget it again");
}

fn report(role: Role, outcome: Outcome) -> Report {
    Report {
        order: 1,
        role,
        outcome,
        raider: "Cove".into(),
        defender: "Brindle".into(),
        minute: 400,
        level: 40,
        flooded: false,
        rounds: 5,
        loot: vec![(Resource::Salt, 10), (Resource::Iron, 2)],
        tallies: 50,
        fates: Vec::new(),
        hurt: Vec::new(),
        taunt: String::new(),
    }
}

#[test]
fn a_won_raid_pays_the_loot_frees_the_party_and_writes_the_fates() {
    let (cat, _, mut clan) = fixture();
    let ids: Vec<u16> = clan.crew.iter().map(|c| c.id).collect();
    clan.social.away = ids.clone();
    let mut r = report(Role::Raider, Outcome::Won);
    r.fates = vec![
        (ids[0], Fate::Unhurt),
        (ids[1], Fate::Hurt),
        (ids[2], Fate::Dead),
    ];
    let (salt, tallies) = (clan.stock.get(Resource::Salt), clan.tallies);
    let mut events = Vec::new();
    fold(
        &cat,
        &mut clan,
        &mail(vec![delivery(1, Body::Raid(r))], vec![]),
        20_000,
        &mut events,
    );
    assert_eq!(clan.stock.get(Resource::Salt), salt + 10);
    assert_eq!(clan.tallies, tallies + 50);
    assert!(clan.social.away.is_empty(), "the party is home");
    assert_eq!(
        (clan.social.raids_won, clan.social.plundered),
        (1, 10 * 4 + 2 * 10 + 50)
    );
    assert_eq!(clan.crew.len(), 2, "one did not come back");
    assert!(clan.crew_by(ids[2]).is_none());
    assert!(clan.crew_by(ids[1]).is_some_and(|c| !c.injuries.is_empty()));
    assert!(clan.hall.iter().any(|h| h.text.contains("Brindle")));
    assert!(events.iter().any(|e| matches!(e, Event::RaidWon { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::Died { .. })));
}

#[test]
fn a_lost_raid_pays_nothing_and_costs_morale() {
    let (cat, _, mut clan) = fixture();
    let ids: Vec<u16> = clan.crew.iter().map(|c| c.id).collect();
    clan.social.away = vec![ids[0]];
    let mut r = report(Role::Raider, Outcome::Repelled);
    r.loot.clear();
    r.tallies = 0;
    r.fates = vec![(ids[0], Fate::Hurt)];
    let (salt, morale) = (clan.stock.get(Resource::Salt), clan.morale);
    let mut events = Vec::new();
    fold(
        &cat,
        &mut clan,
        &mail(vec![delivery(1, Body::Raid(r))], vec![]),
        20_000,
        &mut events,
    );
    assert_eq!(clan.stock.get(Resource::Salt), salt);
    assert_eq!(
        (clan.social.raids_lost, clan.morale),
        (1, morale - LOSS_MORALE)
    );
    assert!(clan.social.away.is_empty());
}

#[test]
fn a_raid_on_us_takes_only_what_we_still_have_and_hurts_the_guard() {
    let (cat, _, mut clan) = fixture();
    clan.stock.take_up_to(Resource::Salt, 1_000);
    clan.stock.add(Resource::Salt, 3);
    clan.tallies = 20;
    let guard = clan.crew[0].id;
    let mut r = report(Role::Defender, Outcome::Won);
    r.hurt = vec![guard, 999];
    r.taunt = "taunt-keeps".into();
    let morale = clan.morale;
    let mut events = Vec::new();
    fold(
        &cat,
        &mut clan,
        &mail(vec![delivery(4, Body::Raid(r))], vec![]),
        20_000,
        &mut events,
    );
    assert_eq!(
        clan.stock.get(Resource::Salt),
        0,
        "three, not the ten posted"
    );
    assert_eq!(clan.tallies, 0, "twenty, not the fifty posted");
    assert_eq!(clan.social.raided, 1);
    assert_eq!(clan.morale, morale - 5);
    assert!(clan.crew_by(guard).is_some_and(|c| !c.injuries.is_empty()));
    assert!(
        clan.social.grudges.contains(&"Cove".to_owned()),
        "the clan remembers"
    );
    // The taunt left with the raid is in the inbox, as a letter from the raider.
    let l = &clan.social.inbox[0];
    assert_eq!(
        (l.from.as_str(), l.template.as_str()),
        ("Cove", "taunt-keeps")
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::RaidSuffered { .. }))
    );
}

#[test]
fn a_raid_beaten_off_heartens_the_hold() {
    let (cat, _, mut clan) = fixture();
    let (salt, morale) = (clan.stock.get(Resource::Salt), clan.morale);
    let mut r = report(Role::Defender, Outcome::Repelled);
    r.loot.clear();
    r.tallies = 0;
    let mut events = Vec::new();
    fold(
        &cat,
        &mut clan,
        &mail(vec![delivery(2, Body::Raid(r))], vec![]),
        20_000,
        &mut events,
    );
    assert_eq!(clan.stock.get(Resource::Salt), salt);
    assert_eq!((clan.social.repelled, clan.morale), (1, morale + 3));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::RaidRepelled { .. }))
    );
}

#[test]
fn a_pact_offer_becomes_a_letter_that_carries_it_and_a_broken_pact_is_remembered() {
    let (cat, _, mut clan) = fixture();
    let offer = Notice {
        kind: NoticeKind::Offered,
        pact: 9,
        with: "Cove".into(),
        terms: 3,
        template: "offer-trade".into(),
        line: Some("fair seas".into()),
    };
    let broken = Notice {
        kind: NoticeKind::Broken,
        pact: 8,
        with: "Dun".into(),
        terms: 1,
        template: String::new(),
        line: None,
    };
    let mut events = Vec::new();
    fold(
        &cat,
        &mut clan,
        &mail(
            vec![
                delivery(1, Body::Pact(offer)),
                delivery(2, Body::Pact(broken)),
            ],
            vec![],
        ),
        20_000,
        &mut events,
    );
    let l = &clan.social.inbox[0];
    assert_eq!(l.offer, Some((9, 3)));
    assert_eq!(l.template, "offer-trade");
    assert_eq!(l.line.as_deref(), Some("fair seas"));
    assert!(clan.social.grudges.contains(&"Dun".to_owned()));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::PactOffered { .. }))
    );
    assert!(events.iter().any(|e| matches!(e, Event::PactBroken { .. })));
}

#[test]
fn the_inbox_is_capped_and_a_read_letter_makes_room_before_an_unread_one() {
    let (_, _, mut clan) = fixture();
    for i in 0..caps::INBOX_CAP {
        clan.social.receive(Letter {
            seq: u32::try_from(i).unwrap() + 1,
            from: format!("c{i}"),
            template: "greeting-reeds".into(),
            line: None,
            day: 0,
            offer: None,
            read: i == 3,
        });
    }
    clan.social.receive(Letter {
        seq: 99,
        from: "new".into(),
        template: "greeting-reeds".into(),
        line: None,
        day: 0,
        offer: None,
        read: false,
    });
    assert_eq!(clan.social.inbox.len(), caps::INBOX_CAP);
    assert!(
        !clan.social.inbox.iter().any(|l| l.from == "c3"),
        "the read one went"
    );
    // The same delivery twice is one letter.
    clan.social.receive(clan.social.inbox[0].clone());
    assert_eq!(clan.social.inbox.len(), caps::INBOX_CAP);
}

#[test]
fn the_clans_social_state_round_trips() {
    let s = ClanSocial {
        seq: 7,
        pending: vec![pend(7, Kind::Raid, Some((Resource::Boats, 1)), 0)],
        credited_to: 12,
        away: vec![1, 2],
        away_since: 20_000,
        inbox: vec![Letter {
            seq: 3,
            from: "Cove".into(),
            template: "greeting-reeds".into(),
            line: Some("hello".into()),
            day: 5,
            offer: Some((4, 3)),
            read: true,
        }],
        raids_sent: 1,
        raids_won: 2,
        raids_lost: 3,
        raided: 4,
        repelled: 5,
        plundered: 60,
        robbed: 70,
        grudges: vec!["Dun".into()],
        deep: true,
    };
    let mut w = Writer::new();
    s.write(&mut w);
    let bytes = w.finish();
    assert_eq!(ClanSocial::read(&mut Reader::new(&bytes)), Some(s));
}

#[test]
fn the_posted_defence_is_the_fit_crew_at_home_and_a_scouts_guess_of_the_stores() {
    let (cat, _, mut clan) = fixture();
    clan.stock.add(Resource::Salt, 17);
    clan.tallies = 47;
    let all = Posted::of(&cat, &clan, 20_000);
    assert_eq!(all.guard.len(), clan.crew.len().min(caps::MAX_DEFENDERS));
    assert_eq!(usize::from(all.crew), clan.crew.len());
    assert_eq!(all.stock[Resource::Salt.index()] % 5, 0, "in fives");
    assert_eq!(all.tallies % 10, 0, "and tens");
    assert!(u32::from(all.tallies) <= clan.tallies);
    // A crew member away on a raid is not at the gate.
    clan.social.away = vec![clan.crew[0].id];
    let some = Posted::of(&cat, &clan, 20_000);
    assert_eq!(some.guard.len(), all.guard.len() - 1);
    assert!(!some.guard.iter().any(|d| d.id == clan.crew[0].id));
    // Arms go to the first hands.
    clan.social.away.clear();
    clan.stock.add(Resource::Arms, 1);
    let armed = Posted::of(&cat, &clan, 20_000);
    assert_eq!(armed.guard.iter().filter(|d| d.armed).count(), 1);
}

#[test]
fn a_party_whose_raid_report_never_comes_is_sent_home_after_a_few_days() {
    let (cat, _, mut clan) = fixture();
    clan.social.away = vec![clan.crew[0].id];
    clan.social.away_since = 20_000;
    let mut events = Vec::new();
    fold(
        &cat,
        &mut clan,
        &mail(vec![], vec![]),
        20_000 + AWAY_DAYS - 1,
        &mut events,
    );
    assert!(!clan.social.away.is_empty(), "not yet");
    fold(
        &cat,
        &mut clan,
        &mail(vec![], vec![]),
        20_000 + AWAY_DAYS,
        &mut events,
    );
    assert!(clan.social.away.is_empty());
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Told(t) if t.contains("no word")))
    );
}
