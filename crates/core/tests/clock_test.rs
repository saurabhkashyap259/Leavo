//! T014 — clock abstraction used to make time-based rules testable.
use chrono::{Duration, TimeZone, Utc};
use leavo_core::clock::{Clock, FixedClock, SystemClock};

#[test]
fn fixed_clock_returns_set_time_and_advances() {
    let t0 = Utc.with_ymd_and_hms(2026, 11, 24, 10, 0, 0).unwrap();
    let c = FixedClock::new(t0);
    assert_eq!(c.now(), t0);
    c.advance(Duration::minutes(11));
    assert_eq!(c.now(), t0 + Duration::minutes(11));
    let t1 = Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap();
    c.set(t1);
    assert_eq!(c.now(), t1);
}

#[test]
fn system_clock_does_not_go_backwards() {
    let c = SystemClock;
    let a = c.now();
    let b = c.now();
    assert!(b >= a);
}

#[test]
fn clocks_are_usable_as_trait_objects() {
    let c: std::sync::Arc<dyn Clock> = std::sync::Arc::new(SystemClock);
    let _ = c.now();
}
