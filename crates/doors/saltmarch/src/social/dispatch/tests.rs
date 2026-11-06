#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

use super::*;
use crate::social::{Body, Refusal};
use crate::types::Trade;

fn cat() -> Catalog {
    Catalog::embedded().unwrap()
}

fn world() -> World {
    let mut w = World::new(1, 7, 20_000);
    for n in ["Ash", "Brindle", "Cove"] {
        w.register_founded(n, 0, 19_900);
    }
    for c in &mut w.clans {
        c.posted.seen = 20_000;
        c.posted.buildings = 4;
        c.score = 40;
    }
    w
}

fn d(from: &str, id: u16, act: Act) -> Dispatch {
    Dispatch {
        from: from.into(),
        id,
        day: 20_000,
        act,
    }
}

fn ask(qty: u32, price: u16) -> Act {
    Act::Ask {
        what: Resource::Salt,
        qty,
        price,
    }
}

#[test]
fn a_dispatch_is_applied_and_leaves_a_receipt() {
    let w = world();
    let bytes = run(Some(&w.encode()), 1, &d("Ash", 1, ask(5, 6)), &cat()).unwrap();
    let after = World::decode(&bytes).unwrap();
    assert_eq!(after.social.exchange.asks.len(), 1);
    assert_eq!(
        after.social.receipts,
        [Receipt {
            clan: 0,
            id: 1,
            refused: None
        }]
    );
}

#[test]
fn a_retried_dispatch_is_not_applied_twice() {
    let w = world();
    let first = run(Some(&w.encode()), 1, &d("Ash", 1, ask(5, 6)), &cat()).unwrap();
    // The board retried after a conflict that had in fact gone through.
    assert_eq!(run(Some(&first), 1, &d("Ash", 1, ask(5, 6)), &cat()), None);
}

#[test]
fn a_refusal_leaves_the_world_as_it_was_but_for_its_receipt() {
    let w = world();
    let bytes = run(Some(&w.encode()), 1, &d("Ash", 1, ask(0, 6)), &cat()).unwrap();
    let after = World::decode(&bytes).unwrap();
    assert!(after.social.exchange.asks.is_empty());
    assert_eq!(after.social.receipts[0].refused, Some(Refusal::BadOrder));
    let mut expect = w;
    expect.social.receipts = after.social.receipts.clone();
    assert_eq!(after, expect);
}

#[test]
fn an_unknown_clan_a_wrong_season_and_a_missing_world_change_nothing() {
    let w = world();
    assert_eq!(
        run(Some(&w.encode()), 1, &d("Nobody", 1, ask(5, 6)), &cat()),
        None
    );
    assert_eq!(
        run(Some(&w.encode()), 2, &d("Ash", 1, ask(5, 6)), &cat()),
        None
    );
    assert_eq!(run(None, 1, &d("Ash", 1, ask(5, 6)), &cat()), None);
    assert_eq!(
        run(Some(&[1, 2, 3]), 1, &d("Ash", 1, ask(5, 6)), &cat()),
        None
    );
}

#[test]
fn a_clan_with_too_many_unseen_receipts_cannot_send_more() {
    let mut bytes = world().encode();
    for id in 1..=u16::try_from(caps::RECEIPTS_PER_CLAN).unwrap() {
        bytes = run(
            Some(&bytes),
            1,
            &d("Ash", id, Act::Block { who: "Cove".into() }),
            &cat(),
        )
        .unwrap();
    }
    let next = u16::try_from(caps::RECEIPTS_PER_CLAN).unwrap() + 1;
    assert_eq!(
        run(
            Some(&bytes),
            1,
            &d(
                "Ash",
                next,
                Act::Block {
                    who: "Brindle".into()
                }
            ),
            &cat()
        ),
        None,
        "no receipt, so the clan takes back what it risked: nothing happened"
    );
    // Another clan is not affected.
    assert!(
        run(
            Some(&bytes),
            1,
            &d("Cove", 1, Act::Block { who: "Ash".into() }),
            &cat()
        )
        .is_some()
    );
}

#[test]
fn a_world_too_full_to_grow_turns_the_order_away_or_changes_nothing() {
    let mut w = world();
    let line = "x".repeat(250);
    for i in 0..1_000u32 {
        w.social.mail.push(crate::social::Delivery {
            seq: i + 1,
            to: 2,
            body: Body::Letter(crate::social::letters::LetterBody {
                from: "Cove".into(),
                template: "greeting-reeds".into(),
                line: Some(line.clone()),
                day: 1,
            }),
        });
    }
    assert!(w.encode().len() > caps::WORLD_SOFT_CAP);
    // Even the receipt would not fit: nothing is written and the clan takes its goods back.
    assert_eq!(
        run(Some(&w.encode()), 1, &d("Ash", 1, ask(5, 6)), &cat()),
        None
    );
}

#[test]
fn the_post_is_rewritten_only_when_it_changed() {
    let w = world();
    let mut posted = w.clans[0].posted.clone();
    assert_eq!(
        post_update(
            Some(&w.encode()),
            1,
            "Ash",
            &posted,
            Some(40),
            crate::world::Badge::default()
        ),
        None
    );
    posted.rating = 3;
    let bytes = post_update(
        Some(&w.encode()),
        1,
        "Ash",
        &posted,
        Some(41),
        crate::world::Badge::default(),
    )
    .unwrap();
    let after = World::decode(&bytes).unwrap();
    assert_eq!(
        (after.clans[0].posted.rating, after.clans[0].score),
        (3, 41)
    );
    assert_eq!(after.clans[1], w.clans[1], "nobody else changes");
    assert_eq!(
        post_update(
            Some(&w.encode()),
            1,
            "Nobody",
            &posted,
            None,
            crate::world::Badge::default()
        ),
        None
    );
}

#[test]
fn an_acknowledgement_clears_only_that_clans_things() {
    let mut w = world();
    w.social.deliver(
        0,
        Body::Parcel(crate::social::market::Parcel {
            goods: None,
            tallies: 1,
            memo: crate::social::market::Memo::Sold,
            about: None,
        }),
    );
    w.social.deliver(
        1,
        Body::Parcel(crate::social::market::Parcel {
            goods: None,
            tallies: 2,
            memo: crate::social::market::Memo::Sold,
            about: None,
        }),
    );
    w.social.deliver(
        0,
        Body::Parcel(crate::social::market::Parcel {
            goods: None,
            tallies: 3,
            memo: crate::social::market::Memo::Sold,
            about: None,
        }),
    );
    w.social.receipts.push(Receipt {
        clan: 0,
        id: 4,
        refused: None,
    });
    w.social.receipts.push(Receipt {
        clan: 1,
        id: 4,
        refused: None,
    });
    let bytes = ack(Some(&w.encode()), 1, "Ash", 2, &[4]).unwrap();
    let after = World::decode(&bytes).unwrap();
    let seqs: Vec<(u8, u32)> = after.social.mail.iter().map(|m| (m.to, m.seq)).collect();
    assert_eq!(
        seqs,
        [(1, 2), (0, 3)],
        "Ash's first one went, the rest stayed"
    );
    assert_eq!(after.social.receipts.len(), 1);
    assert_eq!(after.social.receipts[0].clan, 1);
    assert_eq!(
        ack(Some(&bytes), 1, "Ash", 2, &[4]),
        None,
        "nothing left to clear"
    );
}

#[test]
fn mail_for_a_clan_is_its_own_deliveries_and_receipts() {
    let mut w = world();
    w.social.deliver(
        1,
        Body::Parcel(crate::social::market::Parcel {
            goods: None,
            tallies: 2,
            memo: crate::social::market::Memo::Sold,
            about: None,
        }),
    );
    w.social.receipts.push(Receipt {
        clan: 1,
        id: 1,
        refused: None,
    });
    let m = mail_for(&w, "Brindle");
    assert_eq!((m.deliveries.len(), m.receipts.len()), (1, 1));
    assert!(mail_for(&w, "Ash").is_empty());
    assert!(mail_for(&w, "Nobody").is_empty());
}

#[test]
fn every_act_goes_through_the_same_rules_as_the_direct_call() {
    let cat = cat();
    let bytes = std::cell::RefCell::new(world().encode());
    let id = std::cell::Cell::new(0u16);
    let go = |from: &str, act: Act| {
        id.set(id.get() + 1);
        let next = run(Some(&bytes.borrow()), 1, &d(from, id.get(), act), &cat).unwrap();
        *bytes.borrow_mut() = next;
        World::decode(&bytes.borrow())
            .unwrap()
            .social
            .receipts
            .iter()
            .rev()
            .find(|r| r.id == id.get())
            .copied()
            .unwrap()
    };
    // A letter, an offer, an answer, a break, a block.
    assert_eq!(
        go(
            "Ash",
            Act::Letter {
                to: "Brindle".into(),
                template: "greeting-reeds".into(),
                line: Some("hello".into())
            }
        )
        .refused,
        None
    );
    assert_eq!(
        go(
            "Ash",
            Act::Letter {
                to: "Brindle".into(),
                template: "taunt-keeps".into(),
                line: None
            }
        )
        .refused,
        Some(Refusal::Terms)
    );
    let r = go(
        "Ash",
        Act::Offer {
            to: "Brindle".into(),
            terms: crate::social::pact::TRUCE,
            template: "offer-truce".into(),
            line: None,
        },
    );
    assert_eq!(r.refused, None);
    let pact = World::decode(&bytes.borrow()).unwrap().social.pacts[0].id;
    assert_eq!(
        go("Cove", Act::Accept { pact }).refused,
        Some(Refusal::NotYours)
    );
    assert_eq!(go("Brindle", Act::Accept { pact }).refused, None);
    assert_eq!(go("Ash", Act::Break { pact }).refused, None);
    assert_eq!(
        go("Brindle", Act::Block { who: "Ash".into() }).refused,
        None
    );
    // An offer may only use an offer template, and only one the clan has the history for.
    assert_eq!(
        go(
            "Ash",
            Act::Offer {
                to: "Cove".into(),
                terms: crate::social::pact::TRUCE,
                template: "greeting-reeds".into(),
                line: None
            }
        )
        .refused,
        Some(Refusal::Terms)
    );
    let _ = Trade::Fighter;
}

// ------------------------------------------------------------------ causeways and the marsh

fn after(w: &World, who: &str, id: u16, act: Act) -> (World, Option<Refusal>) {
    let bytes = run(Some(&w.encode()), 1, &d(who, id, act), &cat()).unwrap();
    let w2 = World::decode(&bytes).unwrap();
    let r = w2
        .social
        .receipts
        .iter()
        .find(|r| r.id == id)
        .and_then(|r| r.refused);
    (w2, r)
}

/// A tile of the world's marsh that can take a causeway, by the order of the map.
fn lay_sites(w: &World, n: usize) -> Vec<(u8, u8)> {
    let map = crate::map::Map::generate(w.seed, &cat().pois);
    map.iter()
        .filter(|((x, y), t)| {
            crate::map::can_lay_on(t.kind)
                && map
                    .poi_at(u8::try_from(*x).unwrap(), u8::try_from(*y).unwrap())
                    .is_none()
        })
        .map(|((x, y), _)| (u8::try_from(x).unwrap(), u8::try_from(y).unwrap()))
        .take(n)
        .collect()
}

#[test]
fn a_toll_becomes_a_parcel_for_the_builder_and_never_one_for_oneself() {
    let w = world();
    let (w2, why) = after(
        &w,
        "Brindle",
        1,
        Act::Toll {
            owner: "Ash".into(),
            tallies: 3,
        },
    );
    assert_eq!(why, None);
    let parcels: Vec<_> = w2
        .social
        .mail
        .iter()
        .filter(|m| m.to == 0)
        .filter_map(|m| match &m.body {
            Body::Parcel(p) => Some((p.tallies, p.memo)),
            _ => None,
        })
        .collect();
    assert_eq!(parcels, [(3, crate::social::market::Memo::Toll)]);
    for bad in [
        Act::Toll {
            owner: "Brindle".into(),
            tallies: 3,
        },
        Act::Toll {
            owner: "Nobody".into(),
            tallies: 3,
        },
        Act::Toll {
            owner: "Ash".into(),
            tallies: 0,
        },
    ] {
        let (w3, why) = after(&w, "Brindle", 2, bad);
        assert!(why.is_some());
        assert!(w3.social.mail.is_empty());
    }
}

#[test]
fn laying_a_causeway_follows_the_ground_and_the_caps() {
    let w = world();
    let sites = lay_sites(&w, 12);
    let (a, b) = (sites[0], sites[1]);
    let (w2, why) = after(
        &w,
        "Ash",
        1,
        Act::Lay {
            x: a.0,
            y: a.1,
            toll: 9,
        },
    );
    assert_eq!(why, None);
    assert_eq!(w2.causeways.len(), 1);
    assert_eq!(
        (
            w2.causeways[0].x,
            w2.causeways[0].owner,
            w2.causeways[0].toll
        ),
        (a.0, 0, crate::story::TOLL_MAX),
        "the toll is capped"
    );
    // The same tile twice, and ground that is not for it (the Deep).
    let (_, why) = after(
        &w2,
        "Brindle",
        2,
        Act::Lay {
            x: a.0,
            y: a.1,
            toll: 1,
        },
    );
    assert_eq!(why, Some(Refusal::Blocked));
    let map = crate::map::Map::generate(w.seed, &cat().pois);
    let deep = map
        .iter()
        .find(|(_, t)| t.kind == crate::map::TileKind::Deep)
        .map(|((x, y), _)| (u8::try_from(x).unwrap(), u8::try_from(y).unwrap()))
        .unwrap();
    let (_, why) = after(
        &w,
        "Ash",
        3,
        Act::Lay {
            x: deep.0,
            y: deep.1,
            toll: 1,
        },
    );
    assert_eq!(why, Some(Refusal::Blocked));
    let poi = map.pois.first().unwrap();
    let (_, why) = after(
        &w,
        "Ash",
        4,
        Act::Lay {
            x: poi.x,
            y: poi.y,
            toll: 1,
        },
    );
    assert_eq!(why, Some(Refusal::Blocked), "a place cannot be built over");
    // One clan may lay only so much a season.
    let mut full = w.clone();
    for s in sites.iter().take(usize::from(crate::story::LAY_MAX)) {
        full.causeways.push(crate::world::Span {
            x: s.0,
            y: s.1,
            owner: 0,
            toll: 1,
            laid: 0,
        });
    }
    let (_, why) = after(
        &full,
        "Ash",
        5,
        Act::Lay {
            x: sites[10].0,
            y: sites[10].1,
            toll: 1,
        },
    );
    assert_eq!(why, Some(Refusal::TooMany));
    let (w3, why) = after(
        &full,
        "Brindle",
        6,
        Act::Lay {
            x: sites[10].0,
            y: sites[10].1,
            toll: 1,
        },
    );
    assert_eq!(
        (why, w3.causeways.len()),
        (None, usize::from(crate::story::LAY_MAX) + 1)
    );
    let _ = b;
}

#[test]
fn a_quest_changes_the_marsh_for_everyone_who_opens_the_door_next() {
    use crate::map::{Map, TileKind};
    use crate::story::Flag;
    let w = world();
    let cat = cat();
    let base = Map::generate(w.seed, &cat.pois);
    let channels = |m: &Map| {
        m.iter()
            .filter(|(_, t)| t.kind == TileKind::Channel)
            .count()
    };
    let lairs = |m: &Map, id: &str| {
        m.pois
            .iter()
            .filter(|p| cat.pois[usize::from(p.template)].id == id)
            .count()
    };
    // Dredging turns the channels by the Tidecourt into flats, and only those.
    let (dredged, _) = after(&w, "Ash", 1, Act::Flag { flag: Flag::Dredge });
    assert_eq!(dredged.flags, Flag::Dredge.bit());
    let mut m = base.clone();
    dredged.overlay(&mut m, &cat, Some("Brindle"));
    assert!(
        channels(&m) < channels(&base),
        "{} !< {}",
        channels(&m),
        channels(&base)
    );
    assert!(channels(&m) > 0, "far channels stay channels");
    // The lairs of a kind vanish, the others stay.
    assert!(lairs(&base, "eel-lair") > 0 && lairs(&base, "bog-hag-lair") > 0);
    let (cleared, _) = after(
        &w,
        "Ash",
        2,
        Act::Flag {
            flag: Flag::ClearEels,
        },
    );
    let mut m = base.clone();
    cleared.overlay(&mut m, &cat, None);
    assert_eq!(lairs(&m, "eel-lair"), 0);
    assert_eq!(lairs(&m, "bog-hag-lair"), lairs(&base, "bog-hag-lair"));
    let (hags, _) = after(
        &cleared,
        "Brindle",
        3,
        Act::Flag {
            flag: Flag::ClearHags,
        },
    );
    assert_eq!(hags.flags, Flag::ClearEels.bit() | Flag::ClearHags.bit());
    // The same change twice is the same world.
    let (again, _) = after(&dredged, "Cove", 4, Act::Flag { flag: Flag::Dredge });
    assert_eq!(again.flags, dredged.flags);
    // A surge takes the oldest causeway, once; it sets no lasting bit.
    let mut w = world();
    for (i, laid) in [30i64, 10, 20].into_iter().enumerate() {
        w.causeways.push(crate::world::Span {
            x: 10 + u8::try_from(i).unwrap(),
            y: 40,
            owner: 0,
            toll: 1,
            laid,
        });
    }
    let (washed, _) = after(
        &w,
        "Ash",
        5,
        Act::Flag {
            flag: Flag::Washout,
        },
    );
    assert_eq!(washed.flags, 0);
    assert_eq!(washed.causeways.len(), 2);
    assert!(
        washed.causeways.iter().all(|c| c.laid != 10),
        "the oldest went"
    );
}
