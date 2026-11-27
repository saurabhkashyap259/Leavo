//! T089 — day counting (FR-016, FR-055, research R7).
use chrono::{Datelike, NaiveDate, Weekday};
use leavo_core::calendar::{
    leave_year_of, working_days, BookedDay, ExcludedReason, Half, Holiday, HolidayKind, WeekendMask,
};
use proptest::prelude::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

fn mandatory(date: NaiveDate, name: &str) -> Holiday {
    Holiday {
        date,
        name: name.to_string(),
        kind: HolidayKind::Mandatory,
    }
}

fn total(days: &[BookedDay]) -> Decimal {
    days.iter().map(|b| b.portion).sum()
}

#[test]
fn weekend_and_mandatory_holiday_are_excluded_with_reasons() {
    // Mon 21 Dec 2026 … Mon 28 Dec 2026, Christmas (Fri 25) is a mandatory holiday.
    let hols = [mandatory(d(2026, 12, 25), "Christmas")];
    let r = working_days(
        d(2026, 12, 21),
        d(2026, 12, 28),
        None,
        None,
        WeekendMask::default(),
        &hols,
        1,
    );
    let dates: Vec<_> = r.days.iter().map(|b| b.day).collect();
    assert_eq!(
        dates,
        [
            d(2026, 12, 21),
            d(2026, 12, 22),
            d(2026, 12, 23),
            d(2026, 12, 24),
            d(2026, 12, 28)
        ]
    );
    assert_eq!(r.total(), dec!(5));
    let excluded: Vec<_> = r
        .excluded
        .iter()
        .map(|e| (e.date, e.reason, e.name.clone()))
        .collect();
    assert_eq!(
        excluded,
        [
            (
                d(2026, 12, 25),
                ExcludedReason::Holiday,
                Some("Christmas".into())
            ),
            (d(2026, 12, 26), ExcludedReason::Weekend, None),
            (d(2026, 12, 27), ExcludedReason::Weekend, None),
        ]
    );
}

#[test]
fn optional_holiday_is_a_working_day() {
    let hols = [Holiday {
        date: d(2026, 12, 24),
        name: "Christmas Eve".into(),
        kind: HolidayKind::Optional,
    }];
    let r = working_days(
        d(2026, 12, 24),
        d(2026, 12, 24),
        None,
        None,
        WeekendMask::default(),
        &hols,
        1,
    );
    assert_eq!(r.total(), dec!(1));
    assert!(r.excluded.is_empty());
}

#[test]
fn a_holiday_on_a_weekend_is_reported_as_a_holiday() {
    let hols = [mandatory(d(2026, 12, 26), "Boxing Day")];
    let r = working_days(
        d(2026, 12, 26),
        d(2026, 12, 26),
        None,
        None,
        WeekendMask::default(),
        &hols,
        1,
    );
    assert_eq!(r.total(), dec!(0));
    assert_eq!(r.excluded[0].reason, ExcludedReason::Holiday);
}

#[test]
fn half_days_apply_only_to_the_edges() {
    // Tue 1 Dec – Thu 3 Dec, afternoon of the first day and morning of the last.
    let r = working_days(
        d(2026, 12, 1),
        d(2026, 12, 3),
        Some(Half::Second),
        Some(Half::First),
        WeekendMask::default(),
        &[],
        1,
    );
    let shape: Vec<_> = r.days.iter().map(|b| (b.portion, b.half)).collect();
    assert_eq!(
        shape,
        [
            (dec!(0.5), Some(Half::Second)),
            (dec!(1), None),
            (dec!(0.5), Some(Half::First))
        ]
    );
    assert_eq!(r.total(), dec!(2));
}

#[test]
fn single_day_with_a_half_counts_half() {
    for (start, end) in [
        (Some(Half::First), None),
        (None, Some(Half::Second)),
        (Some(Half::First), Some(Half::First)),
    ] {
        let r = working_days(
            d(2026, 12, 2),
            d(2026, 12, 2),
            start,
            end,
            WeekendMask::default(),
            &[],
            1,
        );
        assert_eq!(r.total(), dec!(0.5), "{start:?} {end:?}");
        assert_eq!(r.days[0].half, start.or(end));
    }
}

#[test]
fn half_on_a_non_working_edge_is_dropped() {
    // Sat 5 Dec – Mon 7 Dec with a half on Saturday: Monday stays a full day.
    let r = working_days(
        d(2026, 12, 5),
        d(2026, 12, 7),
        Some(Half::Second),
        None,
        WeekendMask::default(),
        &[],
        1,
    );
    assert_eq!(r.days.len(), 1);
    assert_eq!(r.days[0].day, d(2026, 12, 7));
    assert_eq!(r.days[0].portion, dec!(1));
    assert_eq!(r.days[0].half, None);
}

#[test]
fn days_carry_the_leave_year_they_fall_in() {
    // 29 Dec 2026 – 2 Jan 2027, January leave year: Tue, Wed, Thu in 2026; Fri 1 Jan in 2027.
    let r = working_days(
        d(2026, 12, 29),
        d(2027, 1, 2),
        None,
        None,
        WeekendMask::default(),
        &[],
        1,
    );
    let years: Vec<_> = r.days.iter().map(|b| (b.day.day(), b.leave_year)).collect();
    assert_eq!(years, [(29, 2026), (30, 2026), (31, 2026), (1, 2027)]);
    assert_eq!(r.by_leave_year(), vec![(2026, dec!(3)), (2027, dec!(1))]);

    // April leave year: 31 Mar belongs to 2025, 1 Apr to 2026.
    let r = working_days(
        d(2026, 3, 31),
        d(2026, 4, 1),
        None,
        None,
        WeekendMask::default(),
        &[],
        4,
    );
    assert_eq!(
        r.days.iter().map(|b| b.leave_year).collect::<Vec<_>>(),
        [2025, 2026]
    );
}

#[test]
fn reversed_range_counts_nothing() {
    let r = working_days(
        d(2026, 12, 3),
        d(2026, 12, 1),
        None,
        None,
        WeekendMask::default(),
        &[],
        1,
    );
    assert!(r.days.is_empty());
    assert!(r.excluded.is_empty());
}

fn arb_mask() -> impl Strategy<Value = WeekendMask> {
    (0u8..127).prop_map(|b| WeekendMask::from_bits(b).unwrap())
}

proptest! {
    #[test]
    fn counting_invariants(
        start in 0i64..800, len in 0i64..40, mask in arb_mask(),
        hol_offsets in proptest::collection::vec(0i64..40, 0..6),
        optional in proptest::bool::ANY,
        start_half in proptest::option::of(prop_oneof![Just(Half::First), Just(Half::Second)]),
        end_half in proptest::option::of(prop_oneof![Just(Half::First), Just(Half::Second)]),
        start_month in 1u32..=12,
    ) {
        let from = d(2026, 1, 1) + chrono::Duration::days(start);
        let to = from + chrono::Duration::days(len);
        let hols: Vec<Holiday> = hol_offsets.iter().map(|o| Holiday {
            date: from + chrono::Duration::days(*o),
            name: format!("H{o}"),
            kind: if optional { HolidayKind::Optional } else { HolidayKind::Mandatory },
        }).collect();
        let r = working_days(from, to, start_half, end_half, mask, &hols, start_month);

        prop_assert!(total(&r.days) <= Decimal::from(len + 1));
        for b in &r.days {
            prop_assert!(b.day >= from && b.day <= to);
            prop_assert!(!mask.contains(b.day.weekday()));
            if !optional {
                prop_assert!(!hols.iter().any(|h| h.date == b.day));
            }
            prop_assert_eq!(b.leave_year, leave_year_of(b.day, start_month));
            if b.half.is_some() {
                prop_assert!(b.day == from || b.day == to);
                prop_assert_eq!(b.portion, dec!(0.5));
            } else {
                prop_assert_eq!(b.portion, dec!(1));
            }
        }
        // Every calendar day is either booked or excluded, never both.
        prop_assert_eq!(r.days.len() + r.excluded.len(), (len + 1) as usize);
        prop_assert_eq!(r.total(), total(&r.days));
    }

    #[test]
    fn a_normal_week_is_five_days(start in 0i64..400) {
        let monday = d(2026, 1, 5) + chrono::Duration::weeks(start / 7);
        prop_assume!(monday.weekday() == Weekday::Mon);
        let r = working_days(monday, monday + chrono::Duration::days(6), None, None, WeekendMask::default(), &[], 1);
        prop_assert_eq!(r.total(), dec!(5));
    }
}
