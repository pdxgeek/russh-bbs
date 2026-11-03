//! Credit rules against an in-memory ledger. The database-enforced parts (append-only
//! triggers, atomic overdraft under concurrency) are in the shared store suite.
#![allow(clippy::unwrap_used, clippy::panic, clippy::unused_async_trait_impl)] // unused_async_trait_impl: in-memory fakes of async traits

use std::sync::Mutex;

use super::*;
use crate::store::UnixSeconds;

#[derive(Default)]
struct Memory(Mutex<Vec<LedgerEntry>>);

impl CreditStore for Memory {
    async fn append(
        &self,
        account: AccountId,
        delta: i64,
        reason: &Slug,
    ) -> Result<Posted, CreditError> {
        let mut rows = self.0.lock().unwrap();
        let balance: i64 = rows
            .iter()
            .filter(|r| r.account == account)
            .map(|r| r.delta)
            .sum();
        let new = balance + delta;
        if new < 0 {
            return Err(CreditError::Overdraft {
                balance: u64::try_from(balance).unwrap(),
                needed: delta.unsigned_abs(),
            });
        }
        let id = LedgerId::from_raw(i64::try_from(rows.len()).unwrap() + 1);
        rows.push(LedgerEntry {
            id,
            account,
            at: UnixSeconds::from_raw(0),
            delta,
            reason: reason.clone(),
        });
        Ok(Posted {
            id,
            balance: u64::try_from(new).unwrap(),
        })
    }

    async fn balance(&self, account: AccountId) -> Result<u64, StoreError> {
        let rows = self.0.lock().unwrap();
        let sum: i64 = rows
            .iter()
            .filter(|r| r.account == account)
            .map(|r| r.delta)
            .sum();
        Ok(u64::try_from(sum).unwrap())
    }

    async fn entries(
        &self,
        account: AccountId,
        before: Option<LedgerId>,
        limit: u32,
    ) -> Result<Vec<LedgerEntry>, StoreError> {
        let rows = self.0.lock().unwrap();
        Ok(rows
            .iter()
            .rev()
            .filter(|r| r.account == account && before.is_none_or(|b| r.id < b))
            .take(usize::try_from(limit).unwrap())
            .cloned()
            .collect())
    }
}

fn amount(n: u64) -> Amount {
    Amount::new(n).unwrap()
}

const ALICE: AccountId = AccountId::from_raw(1);
const BOB: AccountId = AccountId::from_raw(2);

fn reason() -> Slug {
    Slug::parse("test-grant").unwrap()
}

#[test]
fn amounts_are_positive_and_bounded() {
    assert_eq!(Amount::new(0), Err(InvalidAmount));
    assert_eq!(Amount::new(MAX_AMOUNT + 1), Err(InvalidAmount));
    assert_eq!(Amount::new(1).unwrap().get(), 1);
    assert_eq!(Amount::new(MAX_AMOUNT).unwrap().get(), MAX_AMOUNT);
}

#[test]
fn a_movement_is_a_signed_delta() {
    assert_eq!(Movement::Earn(amount(5)).delta(), 5);
    assert_eq!(Movement::Spend(amount(5)).delta(), -5);
    assert_eq!(Movement::Earn(amount(MAX_AMOUNT)).delta(), 1_000_000_000);
}

#[tokio::test]
async fn earn_then_spend_derives_the_balance_and_overdraft_changes_nothing() {
    let store = Memory::default();
    assert_eq!(balance(&store, ALICE).await.unwrap(), 0);
    let earned = post(&store, ALICE, Movement::Earn(amount(10)), &reason())
        .await
        .unwrap();
    assert_eq!(earned.balance, 10);
    assert_eq!(
        post(&store, ALICE, Movement::Spend(amount(4)), &reason())
            .await
            .unwrap()
            .balance,
        6
    );

    let refused = post(&store, ALICE, Movement::Spend(amount(7)), &reason()).await;
    assert!(matches!(
        refused,
        Err(CreditError::Overdraft {
            balance: 6,
            needed: 7
        })
    ));
    assert_eq!(balance(&store, ALICE).await.unwrap(), 6);
    assert_eq!(history(&store, ALICE, None, 50).await.unwrap().len(), 2);
}

#[tokio::test]
async fn accounts_are_separate_and_nothing_moves_between_them() {
    let store = Memory::default();
    post(&store, ALICE, Movement::Earn(amount(10)), &reason())
        .await
        .unwrap();
    assert_eq!(balance(&store, BOB).await.unwrap(), 0);
    let refused = post(&store, BOB, Movement::Spend(amount(1)), &reason()).await;
    assert!(matches!(
        refused,
        Err(CreditError::Overdraft {
            balance: 0,
            needed: 1
        })
    ));
    assert_eq!(history(&store, BOB, None, 50).await.unwrap(), Vec::new());
}

#[tokio::test]
async fn history_is_newest_first_paged_and_bounded() {
    let store = Memory::default();
    for _ in 0..5 {
        post(&store, ALICE, Movement::Earn(amount(1)), &reason())
            .await
            .unwrap();
    }
    let first = history(&store, ALICE, None, 2).await.unwrap();
    assert_eq!(
        first.iter().map(|e| e.id.as_raw()).collect::<Vec<_>>(),
        [5, 4]
    );
    let next = history(&store, ALICE, Some(first[1].id), 2).await.unwrap();
    assert_eq!(
        next.iter().map(|e| e.id.as_raw()).collect::<Vec<_>>(),
        [3, 2]
    );
    // A zero limit is raised to one and a huge one is cut to the maximum page.
    assert_eq!(history(&store, ALICE, None, 0).await.unwrap().len(), 1);
    assert!(history(&store, ALICE, None, u32::MAX).await.unwrap().len() <= MAX_PAGE as usize);
}
