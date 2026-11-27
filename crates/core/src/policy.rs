//! Leave policy rules: quota changes (FR-056) and request validation (FR-017, FR-055, FR-057).
//! Every rule reads the leave type's fields; none depends on the type's code or name.
use crate::balance::{remaining_months, round_half};
use crate::calendar::{working_days, DayCount, Half, Holiday, WeekendMask};
use crate::error::{DomainError, ErrorCode};
use chrono::{Days, NaiveDate};
use rust_decimal::Decimal;
use serde_json::json;
use std::collections::BTreeMap;

pub const MAX_NOTIFY: usize = 20;
pub const MAX_REASON_CHARS: usize = 500;

/// Ledger adjustment for a quota change applied "from now":
/// `round_half((new − old) × remaining_months / 12)`, months counted from `change_date` with
/// the FR-013 rule (the change month counts when on or before the 15th).
pub fn quota_change(
    old: Decimal,
    new: Decimal,
    change_date: NaiveDate,
    year_start: NaiveDate,
    start_month: u32,
) -> Decimal {
    let months = remaining_months(Some(change_date), year_start, start_month);
    round_half((new - old) * Decimal::from(months) / Decimal::from(12))
}

/// The leave type's request rules (data-model.md §leave_types).
#[derive(Debug, Clone, PartialEq)]
pub struct LeaveRules {
    /// Active, and for restricted types granted to the requester.
    pub requestable: bool,
    pub allow_half_day: bool,
    pub allow_negative: bool,
    pub negative_limit: Decimal,
    pub max_days_per_request: Option<Decimal>,
    pub min_notice_days: i32,
    pub max_backdate_days: i32,
    pub attachment_required_above_days: Option<Decimal>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeaveRequestInput {
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub start_half: Option<Half>,
    pub end_half: Option<Half>,
    pub reason: String,
    pub has_attachment: bool,
    pub notify_count: usize,
}

/// What the requester can still spend. Pending days are already subtracted.
#[derive(Debug, Clone, PartialEq)]
pub enum Availability {
    /// Per leave year; a missing year has nothing available (FR-055).
    PerYear(BTreeMap<i32, Decimal>),
    /// One balance across years (comp-off, FR-032).
    Pool(Decimal),
}

/// A day the requester already has in a pending or approved request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExistingDay {
    pub day: NaiveDate,
    pub half: Option<Half>,
}

pub struct RequestContext<'a> {
    /// Today in the org's time zone.
    pub today: NaiveDate,
    pub weekend: WeekendMask,
    pub holidays: &'a [Holiday],
    pub leave_year_start_month: u32,
    pub rules: &'a LeaveRules,
    pub available: &'a Availability,
    pub existing: &'a [ExistingDay],
}

#[derive(Debug, Clone, PartialEq)]
pub struct YearCharge {
    pub leave_year: i32,
    pub days: Decimal,
    pub available_after: Decimal,
}

/// Day count, per-year charges and every rule the request breaks (empty → valid).
#[derive(Debug, Clone, PartialEq)]
pub struct LeaveAssessment {
    pub count: DayCount,
    pub by_leave_year: Vec<YearCharge>,
    pub errors: Vec<DomainError>,
}

impl LeaveAssessment {
    pub fn total(&self) -> Decimal {
        self.count.total()
    }
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

fn overlaps(new_half: Option<Half>, old_half: Option<Half>) -> bool {
    match (new_half, old_half) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    }
}

/// Validate a leave request (FR-017). Errors come in a fixed order: input, type, dates,
/// size, overlap, balance. A reversed range stops after `INVALID_RANGE`.
pub fn validate_leave_request(
    input: &LeaveRequestInput,
    ctx: &RequestContext<'_>,
) -> LeaveAssessment {
    let rules = ctx.rules;
    let mut errors: Vec<DomainError> = Vec::new();
    let reason = input.reason.trim();
    if reason.is_empty() {
        errors.push(ErrorCode::ReasonRequired.into());
    } else if reason.chars().count() > MAX_REASON_CHARS {
        errors.push(ErrorCode::ReasonTooLong.into());
    }
    if input.notify_count > MAX_NOTIFY {
        errors.push(ErrorCode::TooManyNotify.into());
    }
    if !rules.requestable {
        errors.push(ErrorCode::TypeNotAvailable.into());
    }
    if input.to < input.from {
        errors.push(ErrorCode::InvalidRange.into());
        return LeaveAssessment {
            count: DayCount::default(),
            by_leave_year: Vec::new(),
            errors,
        };
    }
    if !rules.allow_half_day && (input.start_half.is_some() || input.end_half.is_some()) {
        errors.push(ErrorCode::HalfDayNotAllowed.into());
    }
    let earliest = ctx
        .today
        .checked_sub_days(Days::new(rules.max_backdate_days.max(0) as u64))
        .unwrap_or(ctx.today);
    if input.from < earliest {
        errors.push(
            DomainError::new(ErrorCode::BackdateNotAllowed)
                .with_details(json!({ "max_backdate_days": rules.max_backdate_days })),
        );
    } else if input.from >= ctx.today && rules.min_notice_days > 0 {
        let notice = (input.from - ctx.today).num_days();
        if notice < i64::from(rules.min_notice_days) {
            errors.push(
                DomainError::new(ErrorCode::NoticeTooShort)
                    .with_details(json!({ "min_notice_days": rules.min_notice_days })),
            );
        }
    }

    let count = working_days(
        input.from,
        input.to,
        input.start_half,
        input.end_half,
        ctx.weekend,
        ctx.holidays,
        ctx.leave_year_start_month,
    );
    let total = count.total();
    if total.is_zero() {
        errors.push(ErrorCode::ZeroWorkingDays.into());
    }
    if let Some(max) = rules.max_days_per_request {
        if total > max {
            errors.push(
                DomainError::new(ErrorCode::MaxDaysExceeded)
                    .with_details(json!({ "max_days_per_request": max })),
            );
        }
    }
    if let Some(threshold) = rules.attachment_required_above_days {
        if total > threshold && !input.has_attachment {
            errors.push(
                DomainError::new(ErrorCode::AttachmentRequired)
                    .with_details(json!({ "above_days": threshold })),
            );
        }
    }
    let clash: Vec<NaiveDate> = count
        .days
        .iter()
        .filter(|b| {
            ctx.existing
                .iter()
                .any(|e| e.day == b.day && overlaps(b.half, e.half))
        })
        .map(|b| b.day)
        .collect();
    if !clash.is_empty() {
        errors.push(DomainError::new(ErrorCode::Overlap).with_details(json!({ "dates": clash })));
    }

    // Balance, per leave year (or one pool for comp-off).
    let floor = if rules.allow_negative {
        -rules.negative_limit
    } else {
        Decimal::ZERO
    };
    let balance_code = if rules.allow_negative {
        ErrorCode::NegativeLimitExceeded
    } else {
        ErrorCode::InsufficientBalance
    };
    let mut by_leave_year = Vec::new();
    let mut pool_left = match ctx.available {
        Availability::Pool(p) => Some(*p),
        Availability::PerYear(_) => None,
    };
    let mut short_year = None;
    for (year, days) in count.by_leave_year() {
        let available_after = match (ctx.available, pool_left.as_mut()) {
            (_, Some(left)) => {
                *left -= days;
                *left
            }
            (Availability::PerYear(map), None) => {
                map.get(&year).copied().unwrap_or_default() - days
            }
            (Availability::Pool(_), None) => unreachable!("pool always has a remainder"),
        };
        if available_after < floor && short_year.is_none() {
            short_year = Some(year);
        }
        by_leave_year.push(YearCharge {
            leave_year: year,
            days,
            available_after,
        });
    }
    if let Some(year) = short_year {
        errors.push(DomainError::new(balance_code).with_details(json!({ "leave_year": year })));
    }

    LeaveAssessment {
        count,
        by_leave_year,
        errors,
    }
}
