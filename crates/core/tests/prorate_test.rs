//! T013 — round_half and pro-rating (FR-013).
use chrono::NaiveDate;
use leavo_core::balance::{prorate, remaining_months, round_half};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[test]
fn round_half_rounds_to_nearest_half_day() {
    assert_eq!(round_half(dec!(2.24)), dec!(2.0));
    assert_eq!(round_half(dec!(2.25)), dec!(2.5));
    assert_eq!(round_half(dec!(2.74)), dec!(2.5));
    assert_eq!(round_half(dec!(2.75)), dec!(3.0));
    assert_eq!(round_half(dec!(-1.25)), dec!(-1.5));
}

#[test]
fn joining_on_or_before_15th_counts_the_month() {
    let year_start = d(2026, 1, 1);
    assert_eq!(remaining_months(Some(d(2026, 7, 15)), year_start, 1), 6);
    assert_eq!(remaining_months(Some(d(2026, 7, 16)), year_start, 1), 5);
    assert_eq!(remaining_months(Some(d(2026, 12, 1)), year_start, 1), 1);
    assert_eq!(remaining_months(Some(d(2026, 12, 20)), year_start, 1), 0);
}

#[test]
fn joined_before_year_start_or_unknown_gets_full_year() {
    assert_eq!(remaining_months(Some(d(2020, 5, 3)), d(2026, 1, 1), 1), 12);
    assert_eq!(remaining_months(None, d(2026, 1, 1), 1), 12);
}

#[test]
fn works_for_leave_years_starting_in_april() {
    // Leave year 2026 runs Apr 2026 – Mar 2027.
    assert_eq!(remaining_months(Some(d(2027, 1, 10)), d(2026, 4, 1), 4), 3);
}

#[test]
fn prorate_examples() {
    let start = d(2026, 1, 1);
    assert_eq!(prorate(dec!(12), Some(d(2026, 7, 1)), start, 1), dec!(6.0));
    assert_eq!(prorate(dec!(6), Some(d(2026, 7, 20)), start, 1), dec!(2.5)); // 6*5/12 = 2.5
    assert_eq!(prorate(dec!(12), None, start, 1), dec!(12));
}

proptest! {
    #[test]
    fn prorated_quota_is_a_half_multiple_within_bounds(
        quota_halves in 0i64..120, m in 1u32..=12, day in 1u32..=28
    ) {
        let quota = Decimal::new(quota_halves * 5, 1);
        let p = prorate(quota, Some(d(2026, m, day)), d(2026, 1, 1), 1);
        prop_assert!(p >= Decimal::ZERO && p <= quota + dec!(0.5));
        prop_assert_eq!((p * dec!(2)).fract(), Decimal::ZERO);
    }
}
