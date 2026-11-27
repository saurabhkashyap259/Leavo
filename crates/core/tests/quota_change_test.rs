//! T071 — mid-year quota change "apply from now" (FR-056):
//! round_half((new − old) × remaining_months / 12), months counted from the change date with
//! the FR-013 15th rule.
use chrono::NaiveDate;
use leavo_core::policy::quota_change;
use rust_decimal_macros::dec;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[test]
fn change_on_or_before_the_15th_counts_that_month() {
    let ys = d(2026, 1, 1);
    assert_eq!(
        quota_change(dec!(12), dec!(18), d(2026, 7, 10), ys, 1),
        dec!(3.0)
    );
    assert_eq!(
        quota_change(dec!(12), dec!(18), d(2026, 7, 15), ys, 1),
        dec!(3.0)
    );
}

#[test]
fn change_after_the_15th_starts_counting_next_month() {
    let ys = d(2026, 1, 1);
    assert_eq!(
        quota_change(dec!(12), dec!(18), d(2026, 7, 16), ys, 1),
        dec!(2.5)
    );
    assert_eq!(
        quota_change(dec!(12), dec!(18), d(2026, 12, 20), ys, 1),
        dec!(0)
    );
}

#[test]
fn decreases_are_negative() {
    let ys = d(2026, 1, 1);
    assert_eq!(
        quota_change(dec!(12), dec!(6), d(2026, 7, 10), ys, 1),
        dec!(-3.0)
    );
}

#[test]
fn rounds_to_the_nearest_half_day() {
    let ys = d(2026, 1, 1);
    // 1 × 5/12 = 0.42 → 0.5;  1 × 3/12 = 0.25 → 0.5;  1 × 2/12 = 0.17 → 0
    assert_eq!(
        quota_change(dec!(12), dec!(13), d(2026, 8, 1), ys, 1),
        dec!(0.5)
    );
    assert_eq!(
        quota_change(dec!(12), dec!(13), d(2026, 10, 1), ys, 1),
        dec!(0.5)
    );
    assert_eq!(
        quota_change(dec!(12), dec!(13), d(2026, 11, 1), ys, 1),
        dec!(0)
    );
}

#[test]
fn uses_the_org_leave_year_start() {
    // Leave year April 2026 – March 2027; a change on 10 Jan 2027 leaves Jan, Feb, Mar.
    let ys = d(2026, 4, 1);
    assert_eq!(
        quota_change(dec!(12), dec!(24), d(2027, 1, 10), ys, 4),
        dec!(3.0)
    );
}

#[test]
fn change_at_year_start_is_the_full_difference() {
    let ys = d(2026, 1, 1);
    assert_eq!(
        quota_change(dec!(12), dec!(15), d(2026, 1, 1), ys, 1),
        dec!(3)
    );
}
