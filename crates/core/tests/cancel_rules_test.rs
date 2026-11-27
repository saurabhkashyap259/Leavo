//! T137/T139 — which cancellation applies, and the days a cancellation returns (FR-020).
use chrono::NaiveDate;
use leavo_core::approval::{cancel_event, days_to_return, RequestEvent as E, RequestStatus as S};
use leavo_core::calendar::{BookedDay, Half};
use leavo_core::error::ErrorCode;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

fn day(s: &str, portion: Decimal, leave_year: i32) -> BookedDay {
    BookedDay {
        day: d(s),
        portion,
        half: (portion == dec!(0.5)).then_some(Half::First),
        leave_year,
    }
}

#[test]
fn the_event_depends_on_status_and_whether_the_leave_has_started() {
    let from = d("2026-11-30");
    // Pending: always a withdrawal, even after the start date.
    assert_eq!(
        cancel_event(S::Pending, from, d("2026-11-24")),
        Ok(E::Withdraw)
    );
    assert_eq!(
        cancel_event(S::Pending, from, d("2026-12-01")),
        Ok(E::Withdraw)
    );
    // Approved, before the first day: cancelled outright.
    assert_eq!(
        cancel_event(S::Approved, from, d("2026-11-29")),
        Ok(E::CancelBeforeStart)
    );
    // From the first day on, the leave has started and an approver must accept.
    assert_eq!(
        cancel_event(S::Approved, from, d("2026-11-30")),
        Ok(E::RequestCancel)
    );
    assert_eq!(
        cancel_event(S::Approved, from, d("2026-12-02")),
        Ok(E::RequestCancel)
    );
    for s in [S::Rejected, S::Cancelled, S::CancelRequested] {
        assert_eq!(
            cancel_event(s, from, d("2026-11-24")),
            Err(ErrorCode::AlreadyDecided),
            "{s:?}"
        );
    }
}

#[test]
fn only_days_on_or_after_the_request_date_are_returned_per_leave_year() {
    // Mon 28 Dec 2026 – Fri 1 Jan 2027, last day a half day: two leave years.
    let days = [
        day("2026-12-28", dec!(1), 2026),
        day("2026-12-29", dec!(1), 2026),
        day("2026-12-30", dec!(1), 2026),
        day("2026-12-31", dec!(1), 2026),
        day("2027-01-01", dec!(0.5), 2027),
    ];
    // Before the start: everything.
    assert_eq!(
        days_to_return(&days, d("2026-12-01")),
        [(2026, dec!(4)), (2027, dec!(0.5))]
    );
    // Requested on the 30th: the 30th onward; the 28th and 29th stay taken.
    assert_eq!(
        days_to_return(&days, d("2026-12-30")),
        [(2026, dec!(2)), (2027, dec!(0.5))]
    );
    assert_eq!(days_to_return(&days, d("2027-01-01")), [(2027, dec!(0.5))]);
    // After the last day: nothing to return.
    assert!(days_to_return(&days, d("2027-01-02")).is_empty());
}
