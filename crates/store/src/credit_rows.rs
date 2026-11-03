//! Backend-independent mapping for the credit ledger: what a stored row means and the one
//! overdraft rule, written once (D24). Each backend runs its own SQL and hands the numbers here.

use russh_bbs_core::access::Slug;
use russh_bbs_core::credits::{CreditError, LedgerEntry, LedgerId, MAX_BALANCE};
use russh_bbs_core::store::{AccountId, StoreError, UnixSeconds};

/// One `credit_ledger` row.
pub(crate) struct LedgerRow {
    pub(crate) id: i64,
    pub(crate) account_id: i64,
    pub(crate) at: i64,
    pub(crate) delta: i64,
    pub(crate) reason: String,
}

impl LedgerRow {
    pub(crate) fn into_entry(self) -> Result<LedgerEntry, StoreError> {
        Ok(LedgerEntry {
            id: LedgerId::from_raw(self.id),
            account: AccountId::from_raw(self.account_id),
            at: UnixSeconds::from_raw(self.at),
            delta: self.delta,
            reason: Slug::parse(&self.reason)
                .map_err(|_| StoreError::Corrupt("stored credit reason is not valid".into()))?,
        })
    }
}

/// A stored balance as a number. Negative means the ledger was edited behind our back.
pub(crate) fn balance(sum: i64) -> Result<u64, StoreError> {
    u64::try_from(sum).map_err(|_| StoreError::Corrupt("a credit balance is negative".into()))
}

/// Decide a movement against the balance read inside the transaction: the new balance, or the
/// refusal. This is the only overdraft rule; both backends call it after serializing writers.
pub(crate) fn apply(current: i64, delta: i64) -> Result<u64, CreditError> {
    let current = balance(current)?;
    let new = i128::from(current) + i128::from(delta);
    if new < 0 {
        return Err(CreditError::Overdraft {
            balance: current,
            needed: delta.unsigned_abs(),
        });
    }
    if new > i128::from(MAX_BALANCE) {
        return Err(CreditError::BalanceFull { balance: current });
    }
    // 0 <= new <= MAX_BALANCE
    Ok(u64::try_from(new).unwrap_or(MAX_BALANCE))
}

/// A page size as the database's integer.
pub(crate) fn page(limit: u32) -> i64 {
    i64::from(limit)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn the_rule_refuses_overdraft_and_overflow_and_allows_exact_spends() {
        assert_eq!(apply(10, -10).unwrap(), 0);
        assert!(matches!(
            apply(10, -11),
            Err(CreditError::Overdraft {
                balance: 10,
                needed: 11
            })
        ));
        assert!(matches!(apply(0, -1), Err(CreditError::Overdraft { .. })));
        let top = i64::try_from(MAX_BALANCE).unwrap();
        assert_eq!(apply(top - 1, 1).unwrap(), MAX_BALANCE);
        assert!(matches!(
            apply(top, 1),
            Err(CreditError::BalanceFull { .. })
        ));
        assert!(apply(-1, 1).is_err(), "a negative stored sum is corruption");
    }
}
