//! The credits ledger against any backend (M3-C, D79): derived balance, atomic overdraft,
//! append-only by the database, no transfer.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use russh_bbs_core::access::Slug;
use russh_bbs_core::audit::FixedClock;
use russh_bbs_core::credits::{
    Amount, CreditError, CreditStore, LedgerId, Movement, balance, history, post,
};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, StoreError, UnixSeconds,
};

use super::Harness;

async fn active<S: AccountStore>(store: &S, name: &str) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

fn earn(n: u64) -> Movement {
    Movement::Earn(Amount::new(n).unwrap())
}

fn spend(n: u64) -> Movement {
    Movement::Spend(Amount::new(n).unwrap())
}

fn why() -> Slug {
    Slug::parse("test-grant").unwrap()
}

pub(crate) async fn earn_and_spend_derive_the_balance<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = Arc::new(FixedClock::new(500));
    h.set_clock(clock.clone());
    let s = h.store();
    let a = active(s, "ann").await;
    assert_eq!(balance(s, a.id).await.unwrap(), 0);

    let first = post(s, a.id, earn(10), &why()).await.unwrap();
    assert_eq!(first.balance, 10);
    clock.set(600);
    let second = post(s, a.id, spend(4), &Slug::parse("buy-thing").unwrap())
        .await
        .unwrap();
    assert_eq!(second.balance, 6);
    assert!(second.id > first.id);
    assert_eq!(balance(s, a.id).await.unwrap(), 6);

    let rows = history(s, a.id, None, 50).await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].delta, rows[0].at),
        (-4, UnixSeconds::from_raw(600))
    );
    assert_eq!(rows[0].reason.as_str(), "buy-thing");
    assert_eq!(
        (rows[1].delta, rows[1].at),
        (10, UnixSeconds::from_raw(500))
    );
    assert!(rows.iter().all(|r| r.account == a.id));
}

pub(crate) async fn an_overdraft_is_refused_and_changes_nothing<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let a = active(s, "ann").await;
    post(s, a.id, earn(5), &why()).await.unwrap();

    let refused = post(s, a.id, spend(6), &why()).await;
    assert!(matches!(
        refused,
        Err(CreditError::Overdraft {
            balance: 5,
            needed: 6
        })
    ));
    assert_eq!(balance(s, a.id).await.unwrap(), 5);
    assert_eq!(history(s, a.id, None, 50).await.unwrap().len(), 1);
    // Spending exactly the balance is allowed and leaves zero.
    assert_eq!(post(s, a.id, spend(5), &why()).await.unwrap().balance, 0);
    assert!(matches!(
        post(s, a.id, spend(1), &why()).await,
        Err(CreditError::Overdraft {
            balance: 0,
            needed: 1
        })
    ));
}

pub(crate) async fn concurrent_spends_never_overdraw<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let a = active(s, "ann").await;
    post(s, a.id, earn(10), &why()).await.unwrap();
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let (store, id) = (s.clone(), a.id);
        tasks.push(tokio::spawn(async move {
            post(&store, id, spend(3), &why()).await
        }));
    }
    let mut spent = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => spent += 1,
            Err(CreditError::Overdraft { .. }) => {}
            Err(other) => panic!("{other}"),
        }
    }
    assert_eq!(
        spent, 3,
        "ten credits pay for exactly three spends of three"
    );
    assert_eq!(balance(s, a.id).await.unwrap(), 1);
    assert_eq!(history(s, a.id, None, 100).await.unwrap().len(), 4);
}

pub(crate) async fn accounts_are_separate_and_unknown_accounts_are_refused<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let (a, b) = (active(s, "ann").await, active(s, "bob").await);
    post(s, a.id, earn(10), &why()).await.unwrap();
    assert_eq!(balance(s, b.id).await.unwrap(), 0);
    assert!(matches!(
        post(s, b.id, spend(1), &why()).await,
        Err(CreditError::Overdraft { balance: 0, .. })
    ));
    assert_eq!(history(s, b.id, None, 50).await.unwrap(), Vec::new());

    let nobody = AccountId::from_raw(9_999);
    assert!(matches!(
        post(s, nobody, earn(1), &why()).await,
        Err(CreditError::Store(StoreError::AccountNotFound))
    ));
    assert_eq!(balance(s, nobody).await.unwrap(), 0);
}

pub(crate) async fn history_pages_newest_first_without_skipping<H: Harness>() {
    let h = H::fresh().await;
    let s = h.store();
    let a = active(s, "ann").await;
    let mut ids = Vec::new();
    for _ in 0..5 {
        ids.push(post(s, a.id, earn(1), &why()).await.unwrap().id);
    }
    let first = history(s, a.id, None, 2).await.unwrap();
    assert_eq!(
        first.iter().map(|e| e.id).collect::<Vec<_>>(),
        [ids[4], ids[3]]
    );
    let next = history(s, a.id, Some(first[1].id), 2).await.unwrap();
    assert_eq!(
        next.iter().map(|e| e.id).collect::<Vec<_>>(),
        [ids[2], ids[1]]
    );
    let last = history(s, a.id, Some(next[1].id), 2).await.unwrap();
    assert_eq!(last.iter().map(|e| e.id).collect::<Vec<_>>(), [ids[0]]);
    assert_eq!(
        history(s, a.id, Some(LedgerId::from_raw(1)), 2)
            .await
            .unwrap(),
        Vec::new()
    );
}

pub(crate) async fn the_database_refuses_to_change_or_remove_a_row<H: Harness>() {
    let mut h = H::fresh().await;
    h.set_clock(Arc::new(FixedClock::new(42)));
    let s = h.store();
    let a = active(s, "ann").await;
    let posted = post(s, a.id, earn(10), &why()).await.unwrap();

    for sql in [
        "UPDATE credit_ledger SET delta = 1000000",
        "UPDATE credit_ledger SET account_id = 2 WHERE id > 0",
        "DELETE FROM credit_ledger",
        "DELETE FROM credit_ledger WHERE id > 0",
    ] {
        let error = h.raw(sql).await.unwrap_err();
        assert!(error.contains("append-only"), "{sql}: {error}");
    }
    // Rows the schema forbids cannot be made either (a zero move, an out-of-range one).
    for sql in [
        "INSERT INTO credit_ledger (account_id, at, delta, reason) VALUES (1, 1, 0, 'x')",
        "INSERT INTO credit_ledger (account_id, at, delta, reason) VALUES (1, 1, 2000000000, 'x')",
        "INSERT INTO credit_ledger (account_id, at, delta, reason) VALUES (1, 1, 1, '')",
    ] {
        assert!(h.raw(sql).await.is_err(), "{sql}");
    }

    let rows = history(s, a.id, None, 50).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].id, rows[0].delta), (posted.id, 10));
    assert_eq!(rows[0].at, UnixSeconds::from_raw(42));
    assert_eq!(balance(s, a.id).await.unwrap(), 10);
}

pub(crate) async fn the_ledger_survives_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let a = active(h.store(), "ann").await;
    post(h.store(), a.id, earn(7), &why()).await.unwrap();
    h.reopen().await;
    let s = h.store();
    assert_eq!(balance(s, a.id).await.unwrap(), 7);
    assert_eq!(post(s, a.id, spend(7), &why()).await.unwrap().balance, 0);
    assert_eq!(history(s, a.id, None, 50).await.unwrap().len(), 2);
}

pub(crate) async fn a_broken_store_errors_instead_of_reading_as_zero<H: Harness>() {
    let h = H::fresh().await;
    let a = active(h.store(), "ann").await;
    post(h.store(), a.id, earn(3), &why()).await.unwrap();
    h.close().await;
    let s = h.store();
    assert!(balance(s, a.id).await.is_err());
    assert!(history(s, a.id, None, 5).await.is_err());
    assert!(matches!(
        post(s, a.id, earn(1), &why()).await,
        Err(CreditError::Store(_))
    ));
    assert!(s.balance(a.id).await.is_err());
}
