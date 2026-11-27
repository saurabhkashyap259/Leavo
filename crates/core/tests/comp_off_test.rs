//! T144 — comp-off rules: which dates can be claimed (FR-031), oldest-expiry-first spending
//! (FR-032) and what may expire while requests are pending (FR-060).
use chrono::{NaiveDate, Weekday};
use leavo_core::calendar::{Holiday, HolidayKind, WeekendMask};
use leavo_core::comp_off::{allocate, check_claim_date, expirable, CreditLeft};
use leavo_core::error::ErrorCode;
use rust_decimal_macros::dec;
use uuid::Uuid;

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

fn holidays() -> Vec<Holiday> {
    vec![
        Holiday {
            date: d("2026-11-09"),
            name: "Diwali".into(),
            kind: HolidayKind::Mandatory,
        },
        Holiday {
            date: d("2026-11-16"),
            name: "Regional".into(),
            kind: HolidayKind::Optional,
        },
    ]
}

#[test]
fn only_past_weekends_and_mandatory_holidays_can_be_claimed() {
    let weekend = WeekendMask::from_weekdays(&[Weekday::Sat, Weekday::Sun]);
    let today = d("2026-11-24"); // Tuesday
    let ok = |date: &str| check_claim_date(d(date), today, weekend, &holidays());
    assert_eq!(ok("2026-11-21"), Ok(())); // last Saturday
    assert_eq!(ok("2026-11-22"), Ok(())); // last Sunday
    assert_eq!(ok("2026-11-09"), Ok(())); // Diwali, mandatory (a Monday)
    for bad in [
        "2026-11-18", // a Wednesday
        "2026-11-16", // optional holiday: an ordinary working day
        "2026-11-24", // today
        "2026-11-28", // next Saturday
    ] {
        assert_eq!(ok(bad), Err(ErrorCode::CompOffDateInvalid), "{bad}");
    }
}

fn credit(id: u128, expires: &str, left: rust_decimal::Decimal) -> CreditLeft {
    CreditLeft {
        id: Uuid::from_u128(id),
        expires_on: d(expires),
        left,
    }
}

#[test]
fn spending_uses_the_soonest_expiring_credit_first() {
    let credits = [
        credit(1, "2027-02-20", dec!(1)),
        credit(2, "2027-01-10", dec!(0.5)),
        credit(3, "2027-01-30", dec!(1)),
    ];
    assert_eq!(
        allocate(&credits, dec!(1)),
        [
            (Uuid::from_u128(2), dec!(0.5)),
            (Uuid::from_u128(3), dec!(0.5))
        ]
    );
    assert_eq!(
        allocate(&credits, dec!(2.5)),
        [
            (Uuid::from_u128(2), dec!(0.5)),
            (Uuid::from_u128(3), dec!(1)),
            (Uuid::from_u128(1), dec!(1)),
        ]
    );
    // More than the credits hold: everything they have; the rest is not backed by a credit.
    assert_eq!(allocate(&credits, dec!(4)).len(), 3);
    // Empty credits are skipped.
    assert_eq!(
        allocate(&[credit(9, "2027-01-01", dec!(0)), credits[0]], dec!(0.5)),
        [(Uuid::from_u128(1), dec!(0.5))]
    );
}

#[test]
fn reserved_and_used_days_never_expire() {
    assert_eq!(expirable(dec!(1), dec!(0), dec!(0), dec!(0)), dec!(1));
    assert_eq!(expirable(dec!(1), dec!(0.5), dec!(0), dec!(0)), dec!(0.5));
    assert_eq!(expirable(dec!(1), dec!(0), dec!(0.5), dec!(0)), dec!(0.5));
    assert_eq!(expirable(dec!(1), dec!(0.5), dec!(0.5), dec!(0)), dec!(0));
    // Already expired days are not expired twice.
    assert_eq!(expirable(dec!(1), dec!(0), dec!(0.5), dec!(0.5)), dec!(0));
}
