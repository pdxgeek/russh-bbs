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
        post_update(Some(&w.encode()), 1, "Ash", &posted, Some(40)),
        None
    );
    posted.rating = 3;
    let bytes = post_update(Some(&w.encode()), 1, "Ash", &posted, Some(41)).unwrap();
    let after = World::decode(&bytes).unwrap();
    assert_eq!(
        (after.clans[0].posted.rating, after.clans[0].score),
        (3, 41)
    );
    assert_eq!(after.clans[1], w.clans[1], "nobody else changes");
    assert_eq!(
        post_update(Some(&w.encode()), 1, "Nobody", &posted, None),
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
