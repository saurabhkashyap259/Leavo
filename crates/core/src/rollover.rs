//! Year-end rollover plan (T169, FR-037, FR-056, FR-057): pure, so the preview and the job write
//! exactly the same thing.
//!
//! For each active user and leave type, closing leave year `Y` into `Y + 1`:
//! - positive unused days of a carry-forward type: `carry_forward +min(unused, cap)` in `Y + 1`,
//!   `lapse −rest` in `Y`; of any other type: `lapse −unused` in `Y`;
//! - a negative balance: `lapse +|deficit|` in `Y` (closing it at 0) and `negative_carry −|deficit|`
//!   in `Y + 1`;
//! - comp-off is never touched (one pool across years);
//! - the new quota of every active type (restricted types only for active grantees, at the full
//!   quota), pro-rated by joining date, unless allotment rows already exist for `Y + 1`.
use crate::balance::prorate;
use crate::calendar::leave_year_bounds;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypePolicy {
    pub id: Uuid,
    /// The quota for the new year: the pending quota if one is scheduled, else the current one.
    pub quota: Option<Decimal>,
    pub is_comp_off: bool,
    pub receives_carry_forward: bool,
    pub restricted: bool,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserInfo {
    pub id: Uuid,
    pub date_of_joining: Option<NaiveDate>,
    /// Restricted types with an active grant.
    pub granted: BTreeSet<Uuid>,
}

/// The ledger balance (entries only, not pending days) of a user and type in the closing year.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OldBalance {
    pub user: Uuid,
    pub leave_type: Uuid,
    pub balance: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloverInput {
    pub closing_year: i32,
    pub start_month: u32,
    /// Org-wide cap on carried days.
    pub cap: Decimal,
    pub types: Vec<TypePolicy>,
    /// Active users only.
    pub users: Vec<UserInfo>,
    pub balances: Vec<OldBalance>,
    /// `(user, type)` pairs that already have an allotment or pro-rating row in the new year.
    pub already_allotted: BTreeSet<(Uuid, Uuid)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlannedKind {
    Allotment,
    Prorate,
    CarryForward,
    NegativeCarry,
    Lapse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub user: Uuid,
    pub leave_type: Uuid,
    pub leave_year: i32,
    pub delta: Decimal,
    pub kind: PlannedKind,
}

/// The entries to write, in user then type order (old-year entries, carry, then allotment).
pub fn plan_rollover(input: &RolloverInput) -> Vec<Planned> {
    let closing = input.closing_year;
    let new = closing + 1;
    let (new_start, _) = leave_year_bounds(new, input.start_month);
    let mut out = Vec::new();
    for u in &input.users {
        for t in input.types.iter().filter(|t| !t.is_comp_off) {
            let entry = |leave_year, delta, kind| Planned {
                user: u.id,
                leave_type: t.id,
                leave_year,
                delta,
                kind,
            };
            let balance = input
                .balances
                .iter()
                .filter(|b| b.user == u.id && b.leave_type == t.id)
                .map(|b| b.balance)
                .sum::<Decimal>();
            if balance > Decimal::ZERO {
                let carry = if t.receives_carry_forward {
                    balance.min(input.cap.max(Decimal::ZERO))
                } else {
                    Decimal::ZERO
                };
                let lapse = balance - carry;
                if lapse > Decimal::ZERO {
                    out.push(entry(closing, -lapse, PlannedKind::Lapse));
                }
                if carry > Decimal::ZERO {
                    out.push(entry(new, carry, PlannedKind::CarryForward));
                }
            } else if balance < Decimal::ZERO {
                out.push(entry(closing, -balance, PlannedKind::Lapse));
                out.push(entry(new, balance, PlannedKind::NegativeCarry));
            }

            let Some(quota) = t.quota.filter(|q| *q > Decimal::ZERO) else {
                continue;
            };
            if !t.active || input.already_allotted.contains(&(u.id, t.id)) {
                continue;
            }
            let amount = if t.restricted {
                if !u.granted.contains(&t.id) {
                    continue;
                }
                quota
            } else {
                prorate(quota, u.date_of_joining, new_start, input.start_month)
            };
            if amount > Decimal::ZERO {
                let kind = if amount == quota {
                    PlannedKind::Allotment
                } else {
                    PlannedKind::Prorate
                };
                out.push(entry(new, amount, kind));
            }
        }
    }
    out
}
