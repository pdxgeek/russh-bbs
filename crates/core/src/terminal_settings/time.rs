//! Formatting a stored timestamp for the login screen. Pure arithmetic, no clock, no zone: the
//! board shows UTC so two members compare like with like.

use crate::store::UnixSeconds;

const SECONDS_PER_DAY: i64 = 86_400;

/// `2026-11-02 10:15 UTC`. Days before 1970 and far in the future still format; the civil date
/// uses the proleptic Gregorian calendar.
#[must_use]
pub fn format_utc(at: UnixSeconds) -> String {
    let secs = at.as_raw();
    let days = secs.div_euclid(SECONDS_PER_DAY);
    let in_day = secs.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let hour = in_day / 3_600;
    let minute = in_day % 3_600 / 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
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
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
