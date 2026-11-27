//! Balance arithmetic: half-day rounding and pro-rating (FR-013).
use chrono::{Datelike, NaiveDate};
use rust_decimal::{Decimal, RoundingStrategy};

/// Round to the nearest half day (ties away from zero).
pub fn round_half(x: Decimal) -> Decimal {
    let two = Decimal::from(2);
    ((x * two).round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero) / two).round_dp(1)
}

/// Months of the leave year still to come for someone who joined on `doj`.
///
/// The joining month counts as a full month when joined on or before the 15th; otherwise
/// counting starts the following month. Joining before the leave year (or unknown) → 12.
pub fn remaining_months(doj: Option<NaiveDate>, year_start: NaiveDate, start_month: u32) -> u32 {
    debug_assert_eq!(year_start.month(), start_month);
    let Some(doj) = doj else { return 12 };
    if doj < year_start {
        return 12;
    }
    let months_in = (doj.year() - year_start.year()) * 12 + doj.month() as i32 - start_month as i32;
    let first_counted = if doj.day() <= 15 {
        months_in
    } else {
        months_in + 1
    };
    (12 - first_counted).clamp(0, 12) as u32
}

/// A yearly quota pro-rated for the months remaining, rounded to the nearest half day.
pub fn prorate(
    quota: Decimal,
    doj: Option<NaiveDate>,
    year_start: NaiveDate,
    start_month: u32,
) -> Decimal {
    let months = remaining_months(doj, year_start, start_month);
    if months == 12 {
        return quota;
    }
    round_half(quota * Decimal::from(months) / Decimal::from(12))
}
