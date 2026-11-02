//! The invite quota: how many codes an account may create per calendar month (M2-D, D67).
//!
//! The number is M2-H's `invites_per_month` limit (`core::limits`: a value per role, a
//! per-member override, the sysop unlimited), resolved by [`crate::limits::load_effective`]. This
//! module only turns that [`Limit`] into a [`Quota`] and answers "which month is it", because
//! the invite rows are the counter (M2-H's `limit_usage` is per day).
//!
//! "Per month" is the **UTC calendar month**, counted over every code the account created in it,
//! whatever became of it: an unused or expired code does not give the slot back, so creating and
//! abandoning codes cannot be used to mint without bound. The count restarts at 00:00 UTC on the
//! first.

use crate::limits::Limit;
use crate::store::UnixSeconds;

/// How long a code lives after it is created: 7 days. Fixed, not a setting (D67); if it becomes
/// one it belongs in `docs/config.md`.
pub const INVITE_TTL_SECS: i64 = 7 * 24 * 60 * 60;

const SECONDS_PER_DAY: i64 = 86_400;

/// How many codes an account may create in a month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quota {
    /// No limit (the sysop).
    Unlimited,
    /// At most `limit` codes per calendar month.
    Limited(u32),
}

impl Quota {
    /// The limit, if there is one.
    #[must_use]
    pub const fn limit(self) -> Option<u32> {
        match self {
            Self::Unlimited => None,
            Self::Limited(limit) => Some(limit),
        }
    }

    /// The quota for an `invites_per_month` [`Limit`]. A number too big for `u32` is no limit
    /// anyone could reach, so it is clamped to the largest `u32`.
    #[must_use]
    pub fn from_limit(limit: Limit) -> Self {
        match limit {
            Limit::Unlimited => Self::Unlimited,
            Limit::AtMost(n) => Self::Limited(u32::try_from(n).unwrap_or(u32::MAX)),
        }
    }
}

/// 00:00 UTC on the first day of the month containing `now`.
#[must_use]
pub fn month_start(now: UnixSeconds) -> UnixSeconds {
    let (year, month, _) = civil_from_days(now.as_raw().div_euclid(SECONDS_PER_DAY));
    UnixSeconds::from_raw(days_from_civil(year, month, 1).saturating_mul(SECONDS_PER_DAY))
}

/// 00:00 UTC on the first day of the month after the one containing `now`: when the count
/// restarts.
#[must_use]
pub fn next_month_start(now: UnixSeconds) -> UnixSeconds {
    let (year, month, _) = civil_from_days(now.as_raw().div_euclid(SECONDS_PER_DAY));
    let (year, month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    UnixSeconds::from_raw(days_from_civil(year, month, 1).saturating_mul(SECONDS_PER_DAY))
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

/// (year, month, day) to days since 1970-01-01. Howard Hinnant's `days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let year_of_era = y.rem_euclid(400);
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> UnixSeconds {
        UnixSeconds::from_raw(secs)
    }

    #[test]
    fn a_limit_becomes_a_quota() {
        assert_eq!(Quota::from_limit(Limit::Unlimited), Quota::Unlimited);
        assert_eq!(Quota::from_limit(Limit::AtMost(0)), Quota::Limited(0));
        assert_eq!(Quota::from_limit(Limit::AtMost(7)), Quota::Limited(7));
        assert_eq!(
            Quota::from_limit(Limit::AtMost(u64::MAX)),
            Quota::Limited(u32::MAX)
        );
    }

    #[test]
    fn the_month_runs_from_the_first_at_midnight_utc() {
        // 2026-11-02 11:00:00 UTC
        let now = at(1_793_617_200);
        assert_eq!(month_start(now), at(1_793_491_200)); // 2026-11-01 00:00
        assert_eq!(next_month_start(now), at(1_796_083_200)); // 2026-12-01 00:00
        // The first second of a month belongs to it; the last second of the one before does not.
        assert_eq!(month_start(at(1_793_491_200)), at(1_793_491_200));
        assert_eq!(month_start(at(1_793_491_199)), at(1_790_812_800)); // 2026-10-01
        assert_eq!(next_month_start(at(1_793_491_199)), at(1_793_491_200));
    }

    #[test]
    fn december_rolls_into_the_next_year_and_the_epoch_works() {
        // 2026-12-31 23:59:59 -> 2027-01-01 00:00:00
        assert_eq!(next_month_start(at(1_798_761_599)), at(1_798_761_600));
        assert_eq!(month_start(at(0)), at(0));
        assert_eq!(next_month_start(at(0)), at(31 * SECONDS_PER_DAY));
        // February in a leap year: 2028-02-12 -> 2028-03-01
        assert_eq!(next_month_start(at(1_834_000_000)), at(1_835_481_600));
    }
}
