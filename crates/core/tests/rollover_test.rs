//! T166 — the year-end plan (FR-037, FR-056, FR-057, data-model.md §rollover_runs).
//!
//! Closing leave year 2026 (January start) into 2027 with a carry-forward cap of 5.
use chrono::NaiveDate;
use leavo_core::rollover::{
    plan_rollover, OldBalance, Planned, PlannedKind as K, RolloverInput, TypePolicy, UserInfo,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::BTreeSet;
use uuid::Uuid;

const CL: Uuid = Uuid::from_u128(1);
const SL: Uuid = Uuid::from_u128(2);
const ML: Uuid = Uuid::from_u128(3);
const CO: Uuid = Uuid::from_u128(4);
const OLD: Uuid = Uuid::from_u128(5);
const ASHA: Uuid = Uuid::from_u128(10);
const RAVI: Uuid = Uuid::from_u128(11);
const NEW_JOINER: Uuid = Uuid::from_u128(12);

fn ty(id: Uuid, quota: Option<Decimal>) -> TypePolicy {
    TypePolicy {
        id,
        quota,
        is_comp_off: false,
        receives_carry_forward: false,
        restricted: false,
        active: true,
    }
}

fn input() -> RolloverInput {
    RolloverInput {
        closing_year: 2026,
        start_month: 1,
        cap: dec!(5),
        types: vec![
            TypePolicy {
                receives_carry_forward: true,
                ..ty(CL, Some(dec!(12)))
            },
            // SL's quota changes from next year (pending quota already resolved by the caller).
            ty(SL, Some(dec!(8))),
            TypePolicy {
                restricted: true,
                ..ty(ML, Some(dec!(130)))
            },
            TypePolicy {
                is_comp_off: true,
                ..ty(CO, None)
            },
            TypePolicy {
                active: false,
                ..ty(OLD, Some(dec!(3)))
            },
        ],
        users: vec![
            UserInfo {
                id: ASHA,
                date_of_joining: None,
                granted: BTreeSet::from([ML]),
            },
            UserInfo {
                id: RAVI,
                date_of_joining: Some(NaiveDate::from_ymd_opt(2024, 3, 1).unwrap()),
                granted: BTreeSet::new(),
            },
            // Joins on 20 Mar 2027: 9 months of 2027 count.
            UserInfo {
                id: NEW_JOINER,
                date_of_joining: Some(NaiveDate::from_ymd_opt(2027, 3, 20).unwrap()),
                granted: BTreeSet::new(),
            },
        ],
        balances: vec![
            OldBalance {
                user: ASHA,
                leave_type: CL,
                balance: dec!(8),
            },
            OldBalance {
                user: ASHA,
                leave_type: SL,
                balance: dec!(2.5),
            },
            OldBalance {
                user: ASHA,
                leave_type: CO,
                balance: dec!(3),
            },
            OldBalance {
                user: ASHA,
                leave_type: OLD,
                balance: dec!(1),
            },
            OldBalance {
                user: RAVI,
                leave_type: CL,
                balance: dec!(-2),
            },
            OldBalance {
                user: RAVI,
                leave_type: SL,
                balance: dec!(0),
            },
        ],
        already_allotted: BTreeSet::from([(RAVI, SL)]),
    }
}

fn p(user: Uuid, leave_type: Uuid, year: i32, delta: Decimal, kind: K) -> Planned {
    Planned {
        user,
        leave_type,
        leave_year: year,
        delta,
        kind,
    }
}

fn of(plan: &[Planned], user: Uuid, leave_type: Uuid) -> Vec<Planned> {
    plan.iter()
        .filter(|x| x.user == user && x.leave_type == leave_type)
        .cloned()
        .collect()
}

#[test]
fn casual_leave_carries_up_to_the_cap_and_the_rest_lapses() {
    let plan = plan_rollover(&input());
    assert_eq!(
        of(&plan, ASHA, CL),
        [
            p(ASHA, CL, 2026, dec!(-3), K::Lapse),
            p(ASHA, CL, 2027, dec!(5), K::CarryForward),
            p(ASHA, CL, 2027, dec!(12), K::Allotment),
        ]
    );
}

#[test]
fn other_types_lapse_and_comp_off_is_untouched() {
    let plan = plan_rollover(&input());
    assert_eq!(
        of(&plan, ASHA, SL),
        [
            p(ASHA, SL, 2026, dec!(-2.5), K::Lapse),
            p(ASHA, SL, 2027, dec!(8), K::Allotment),
        ]
    );
    assert!(of(&plan, ASHA, CO).is_empty());
    // An inactive type's balance lapses; it gets no new quota.
    assert_eq!(
        of(&plan, ASHA, OLD),
        [p(ASHA, OLD, 2026, dec!(-1), K::Lapse)]
    );
}

#[test]
fn a_negative_balance_is_closed_and_carried_as_a_deduction() {
    let plan = plan_rollover(&input());
    assert_eq!(
        of(&plan, RAVI, CL),
        [
            p(RAVI, CL, 2026, dec!(2), K::Lapse),
            p(RAVI, CL, 2027, dec!(-2), K::NegativeCarry),
            p(RAVI, CL, 2027, dec!(12), K::Allotment),
        ]
    );
}

#[test]
fn allotments_follow_grants_joining_dates_and_existing_rows() {
    let plan = plan_rollover(&input());
    // Restricted: only grantees, full quota.
    assert_eq!(
        of(&plan, ASHA, ML),
        [p(ASHA, ML, 2027, dec!(130), K::Allotment)]
    );
    assert!(of(&plan, RAVI, ML).is_empty());
    // Already allotted for 2027 (e.g. joined on 1 Jan): nothing more.
    assert!(of(&plan, RAVI, SL).is_empty());
    // Joining in 2027: pro-rated 12 × 9/12 = 9.
    assert_eq!(
        of(&plan, NEW_JOINER, CL),
        [p(NEW_JOINER, CL, 2027, dec!(9), K::Prorate)]
    );
    assert_eq!(
        of(&plan, NEW_JOINER, SL),
        [p(NEW_JOINER, SL, 2027, dec!(6), K::Prorate)]
    );
}

#[test]
fn the_plan_is_deterministic_and_zero_cap_lapses_everything() {
    assert_eq!(plan_rollover(&input()), plan_rollover(&input()));
    let mut no_cap = input();
    no_cap.cap = Decimal::ZERO;
    assert_eq!(
        of(&plan_rollover(&no_cap), ASHA, CL)[..1],
        [p(ASHA, CL, 2026, dec!(-8), K::Lapse)]
    );
}
