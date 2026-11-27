//! T012 — leave-year and calendar primitives (FR-010, FR-049).
use chrono::{NaiveDate, TimeZone, Utc, Weekday};
use chrono_tz::Asia::Kolkata;
use leavo_core::calendar::{leave_year_bounds, leave_year_of, today_in, WeekendMask};
use proptest::prelude::*;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[test]
fn january_start_leave_year_is_calendar_year() {
    assert_eq!(leave_year_of(d(2026, 1, 1), 1), 2026);
    assert_eq!(leave_year_of(d(2026, 12, 31), 1), 2026);
}

#[test]
fn april_start_leave_year_is_named_after_start_year() {
    assert_eq!(leave_year_of(d(2026, 3, 31), 4), 2025);
    assert_eq!(leave_year_of(d(2026, 4, 1), 4), 2026);
    assert_eq!(leave_year_of(d(2027, 3, 31), 4), 2026);
}

#[test]
fn bounds_cover_twelve_months() {
    assert_eq!(leave_year_bounds(2026, 1), (d(2026, 1, 1), d(2026, 12, 31)));
    assert_eq!(leave_year_bounds(2026, 4), (d(2026, 4, 1), d(2027, 3, 31)));
    assert_eq!(
        leave_year_bounds(2027, 12),
        (d(2027, 12, 1), d(2028, 11, 30))
    );
}

proptest! {
    #[test]
    fn every_date_lies_inside_its_own_leave_year(
        y in 2000i32..2100, m in 1u32..=12, day in 1u32..=28, start in 1u32..=12
    ) {
        let date = d(y, m, day);
        let ly = leave_year_of(date, start);
        let (from, to) = leave_year_bounds(ly, start);
        prop_assert!(from <= date && date <= to);
    }
}

#[test]
fn today_uses_org_time_zone_not_utc() {
    // 2026-12-31 20:00 UTC is already 2027-01-01 01:30 in Kolkata.
    let now = Utc.with_ymd_and_hms(2026, 12, 31, 20, 0, 0).unwrap();
    assert_eq!(today_in(Kolkata, now), d(2027, 1, 1));
    assert_eq!(today_in(chrono_tz::UTC, now), d(2026, 12, 31));
}

#[test]
fn default_weekend_is_saturday_and_sunday() {
    let m = WeekendMask::default();
    assert!(m.contains(Weekday::Sat));
    assert!(m.contains(Weekday::Sun));
    assert!(!m.contains(Weekday::Mon));
    assert_eq!(m.bits(), 96);
}

#[test]
fn weekend_mask_round_trips_weekdays() {
    let m = WeekendMask::from_weekdays(&[Weekday::Fri, Weekday::Sat]);
    assert_eq!(m.weekdays(), vec![Weekday::Fri, Weekday::Sat]);
    assert_eq!(WeekendMask::from_bits(m.bits()).unwrap(), m);
}

#[test]
fn weekend_mask_must_leave_a_working_day() {
    assert!(WeekendMask::from_bits(127).is_err());
    assert!(WeekendMask::from_bits(128).is_err());
    assert!(WeekendMask::from_bits(0).is_ok()); // a 7-day working week is allowed
}
