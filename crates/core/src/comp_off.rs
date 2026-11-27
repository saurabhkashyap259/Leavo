//! Comp-off rules (FR-030–FR-032, FR-060): claimable dates, oldest-expiry-first spending and
//! what may expire.
use crate::calendar::{Holiday, HolidayKind, WeekendMask};
use crate::error::ErrorCode;
use chrono::{Datelike, NaiveDate};
use rust_decimal::Decimal;
use uuid::Uuid;

/// A worked date can be claimed when it is in the past and is a weekend day (org mask) or a
/// mandatory holiday. Optional holidays are ordinary working days (FR-031). Whether the date
/// is already claimed is checked against stored claims.
pub fn check_claim_date(
    date: NaiveDate,
    today: NaiveDate,
    weekend: WeekendMask,
    holidays: &[Holiday],
) -> Result<(), ErrorCode> {
    let off = weekend.contains(date.weekday())
        || holidays
            .iter()
            .any(|h| h.date == date && h.kind == HolidayKind::Mandatory);
    if date < today && off {
        Ok(())
    } else {
        Err(ErrorCode::CompOffDateInvalid)
    }
}

/// What is left on a credit to spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreditLeft {
    pub id: Uuid,
    pub expires_on: NaiveDate,
    pub left: Decimal,
}

/// Take `days` from `credits`, soonest expiry first (FR-032). Returns `(credit, days)` for each
/// credit drawn on. If the credits hold less than `days`, all of it is taken.
pub fn allocate(credits: &[CreditLeft], days: Decimal) -> Vec<(Uuid, Decimal)> {
    let mut sorted: Vec<&CreditLeft> = credits.iter().filter(|c| c.left > Decimal::ZERO).collect();
    sorted.sort_by_key(|c| (c.expires_on, c.id));
    let mut need = days;
    let mut out = Vec::new();
    for c in sorted {
        if need <= Decimal::ZERO {
            break;
        }
        let take = c.left.min(need);
        out.push((c.id, take));
        need -= take;
    }
    out
}

/// Days of a credit that may expire: never what is used or reserved by a pending request, and
/// never what has already expired (FR-060).
pub fn expirable(days: Decimal, used: Decimal, reserved: Decimal, expired: Decimal) -> Decimal {
    (days - used - reserved - expired).max(Decimal::ZERO)
}
