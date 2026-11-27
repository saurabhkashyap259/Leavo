//! T090 — request validation, one case per error code (FR-017, FR-055, FR-057).
//! Every rule comes from the type's fields; none depends on the type's code or name.
use chrono::NaiveDate;
use leavo_core::calendar::{Half, Holiday, HolidayKind, WeekendMask};
use leavo_core::error::ErrorCode;
use leavo_core::policy::{
    validate_leave_request, Availability, ExistingDay, LeaveAssessment, LeaveRequestInput,
    LeaveRules, RequestContext,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeMap;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

/// Today is Tuesday 24 Nov 2026.
const TODAY: (i32, u32, u32) = (2026, 11, 24);

fn today() -> NaiveDate {
    d(TODAY.0, TODAY.1, TODAY.2)
}

fn rules() -> LeaveRules {
    LeaveRules {
        requestable: true,
        allow_half_day: true,
        allow_negative: false,
        negative_limit: dec!(5),
        max_days_per_request: None,
        min_notice_days: 0,
        max_backdate_days: 0,
        attachment_required_above_days: None,
    }
}

/// Mon 30 Nov – Wed 2 Dec 2026: three working days.
fn input() -> LeaveRequestInput {
    LeaveRequestInput {
        from: d(2026, 11, 30),
        to: d(2026, 12, 2),
        start_half: None,
        end_half: None,
        reason: "Family function".into(),
        has_attachment: false,
        notify_count: 0,
    }
}

fn per_year(entries: &[(i32, Decimal)]) -> Availability {
    Availability::PerYear(entries.iter().copied().collect::<BTreeMap<_, _>>())
}

struct Case {
    input: LeaveRequestInput,
    rules: LeaveRules,
    available: Availability,
    existing: Vec<ExistingDay>,
    holidays: Vec<Holiday>,
}

impl Case {
    fn new() -> Self {
        Self {
            input: input(),
            rules: rules(),
            available: per_year(&[(2026, dec!(10)), (2027, dec!(10))]),
            existing: vec![],
            holidays: vec![],
        }
    }
    fn run(&self) -> LeaveAssessment {
        validate_leave_request(
            &self.input,
            &RequestContext {
                today: today(),
                weekend: WeekendMask::default(),
                holidays: &self.holidays,
                leave_year_start_month: 1,
                rules: &self.rules,
                available: &self.available,
                existing: &self.existing,
            },
        )
    }
    fn codes(&self) -> Vec<ErrorCode> {
        self.run().errors.iter().map(|e| e.code).collect()
    }
}

#[test]
fn a_valid_request_has_no_errors_and_charges_its_leave_year() {
    let c = Case::new();
    let a = c.run();
    assert!(a.errors.is_empty(), "{:?}", a.errors);
    assert_eq!(a.total(), dec!(3));
    assert_eq!(a.by_leave_year.len(), 1);
    assert_eq!(a.by_leave_year[0].leave_year, 2026);
    assert_eq!(a.by_leave_year[0].days, dec!(3));
    assert_eq!(a.by_leave_year[0].available_after, dec!(7));
}

#[test]
fn invalid_range() {
    let mut c = Case::new();
    c.input.to = d(2026, 11, 29);
    assert_eq!(c.codes(), [ErrorCode::InvalidRange]);
}

#[test]
fn zero_working_days() {
    let mut c = Case::new();
    c.input.from = d(2026, 11, 28);
    c.input.to = d(2026, 11, 29);
    assert_eq!(c.codes(), [ErrorCode::ZeroWorkingDays]);

    // A weekday that is a mandatory holiday is also zero.
    let mut c = Case::new();
    c.input.from = d(2026, 12, 25);
    c.input.to = d(2026, 12, 25);
    c.holidays = vec![Holiday {
        date: d(2026, 12, 25),
        name: "Christmas".into(),
        kind: HolidayKind::Mandatory,
    }];
    assert_eq!(c.codes(), [ErrorCode::ZeroWorkingDays]);
}

#[test]
fn half_day_not_allowed() {
    let mut c = Case::new();
    c.rules.allow_half_day = false;
    c.input.end_half = Some(Half::First);
    assert_eq!(c.codes(), [ErrorCode::HalfDayNotAllowed]);
}

#[test]
fn insufficient_balance_is_checked_per_leave_year() {
    let mut c = Case::new();
    c.available = per_year(&[(2026, dec!(2.5))]);
    assert_eq!(c.codes(), [ErrorCode::InsufficientBalance]);

    // 29 Dec – 4 Jan: 3 days in 2026, 2 days in 2027 (Fri 1 Jan, Mon 4 Jan).
    let mut c = Case::new();
    c.input.from = d(2026, 12, 29);
    c.input.to = d(2027, 1, 4);
    c.available = per_year(&[(2026, dec!(3)), (2027, dec!(1.5))]);
    let a = c.run();
    assert_eq!(
        a.errors.iter().map(|e| e.code).collect::<Vec<_>>(),
        [ErrorCode::InsufficientBalance]
    );
    assert_eq!(a.errors[0].details.as_ref().unwrap()["leave_year"], 2027);
    let years: Vec<_> = a
        .by_leave_year
        .iter()
        .map(|y| (y.leave_year, y.days, y.available_after))
        .collect();
    assert_eq!(
        years,
        [(2026, dec!(3), dec!(0)), (2027, dec!(2), dec!(-0.5))]
    );

    // Each year fits on its own: no error even though the total exceeds either year.
    c.available = per_year(&[(2026, dec!(3)), (2027, dec!(2))]);
    assert!(c.codes().is_empty());

    // A leave year with no balance at all is zero.
    c.available = per_year(&[(2026, dec!(3))]);
    assert_eq!(c.codes(), [ErrorCode::InsufficientBalance]);
}

#[test]
fn negative_limit_exceeded() {
    let mut c = Case::new();
    c.rules.allow_negative = true;
    c.rules.negative_limit = dec!(2);
    c.available = per_year(&[(2026, dec!(1))]);
    // 1 − 3 = −2: exactly at the limit is allowed.
    assert!(c.codes().is_empty());
    c.available = per_year(&[(2026, dec!(0.5))]);
    assert_eq!(c.codes(), [ErrorCode::NegativeLimitExceeded]);
}

#[test]
fn comp_off_uses_one_pool_across_years() {
    let mut c = Case::new();
    c.input.from = d(2026, 12, 31);
    c.input.to = d(2027, 1, 1);
    c.available = Availability::Pool(dec!(2));
    assert!(c.codes().is_empty());
    c.available = Availability::Pool(dec!(1.5));
    assert_eq!(c.codes(), [ErrorCode::InsufficientBalance]);
}

#[test]
fn overlap_with_full_and_half_days() {
    // Existing full day on Tue 1 Dec.
    let mut c = Case::new();
    c.existing = vec![ExistingDay {
        day: d(2026, 12, 1),
        half: None,
    }];
    assert_eq!(c.codes(), [ErrorCode::Overlap]);

    // Existing morning half on Wed 2 Dec; the new request ends with an afternoon half → fine.
    let mut c = Case::new();
    c.existing = vec![ExistingDay {
        day: d(2026, 12, 2),
        half: Some(Half::First),
    }];
    c.input.end_half = Some(Half::Second);
    assert!(c.codes().is_empty());
    // Same half → overlap.
    c.input.end_half = Some(Half::First);
    assert_eq!(c.codes(), [ErrorCode::Overlap]);
    // A full day against an existing half → overlap.
    c.input.end_half = None;
    assert_eq!(c.codes(), [ErrorCode::Overlap]);
}

#[test]
fn backdating_follows_the_types_maximum() {
    // Type with 0 cannot start in the past.
    let mut c = Case::new();
    c.input.from = d(2026, 11, 23);
    c.input.to = d(2026, 11, 24);
    assert_eq!(c.codes(), [ErrorCode::BackdateNotAllowed]);

    // A custom type with 3 back-dating days: 3 days back is fine, 4 is not.
    c.rules.max_backdate_days = 3;
    c.input.from = d(2026, 11, 21);
    assert!(c.codes().is_empty(), "{:?}", c.codes());
    c.input.from = d(2026, 11, 20);
    assert_eq!(c.codes(), [ErrorCode::BackdateNotAllowed]);
}

#[test]
fn notice_applies_only_to_requests_starting_today_or_later() {
    let mut c = Case::new();
    c.rules.min_notice_days = 7;
    // Starts in 6 days → too short.
    assert_eq!(c.codes(), [ErrorCode::NoticeTooShort]);
    // Starts in 7 days → fine.
    c.input.from = d(2026, 12, 1);
    c.input.to = d(2026, 12, 2);
    assert!(c.codes().is_empty());
    // Starting today with notice → too short.
    c.input.from = today();
    assert_eq!(c.codes(), [ErrorCode::NoticeTooShort]);
    // A back-dated request (allowed by back-dating days) is exempt from notice.
    c.rules.max_backdate_days = 7;
    c.input.from = d(2026, 11, 20);
    c.input.to = d(2026, 11, 20);
    assert!(c.codes().is_empty());
}

#[test]
fn max_days_exceeded() {
    let mut c = Case::new();
    c.rules.max_days_per_request = Some(dec!(2.5));
    assert_eq!(c.codes(), [ErrorCode::MaxDaysExceeded]);
    c.input.end_half = Some(Half::First);
    assert!(c.codes().is_empty());
}

#[test]
fn attachment_required_above_threshold() {
    let mut c = Case::new();
    c.rules.attachment_required_above_days = Some(dec!(2));
    assert_eq!(c.codes(), [ErrorCode::AttachmentRequired]);
    c.input.has_attachment = true;
    assert!(c.codes().is_empty());
    // Exactly at the threshold needs no attachment.
    c.input.has_attachment = false;
    c.input.to = d(2026, 12, 1);
    assert!(c.codes().is_empty());
}

#[test]
fn type_not_available() {
    let mut c = Case::new();
    c.rules.requestable = false;
    assert_eq!(c.codes(), [ErrorCode::TypeNotAvailable]);
}

#[test]
fn too_many_notify() {
    let mut c = Case::new();
    c.input.notify_count = 20;
    assert!(c.codes().is_empty());
    c.input.notify_count = 21;
    assert_eq!(c.codes(), [ErrorCode::TooManyNotify]);
}

#[test]
fn reason_required_and_too_long() {
    let mut c = Case::new();
    c.input.reason = "   ".into();
    assert_eq!(c.codes(), [ErrorCode::ReasonRequired]);
    c.input.reason = "é".repeat(500);
    assert!(c.codes().is_empty());
    c.input.reason = "é".repeat(501);
    assert_eq!(c.codes(), [ErrorCode::ReasonTooLong]);
}

#[test]
fn several_problems_are_all_reported() {
    let mut c = Case::new();
    c.input.reason = String::new();
    c.input.notify_count = 25;
    c.available = per_year(&[(2026, dec!(0))]);
    assert_eq!(
        c.codes(),
        [
            ErrorCode::ReasonRequired,
            ErrorCode::TooManyNotify,
            ErrorCode::InsufficientBalance
        ]
    );
}
