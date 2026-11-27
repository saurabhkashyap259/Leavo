//! T128 — every balance number can be explained from the ledger (SC-010).
//!
//! Random scenarios (seeded, so failures reproduce): allotments, carry-forward and manual
//! adjustments, then random requests that are approved, rejected or left pending. Each
//! `/me/balances` figure must equal the sum of the matching ledger rows (plus pending days).
mod common;

use common::factories::*;
use common::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use uuid::Uuid;

/// xorshift64*: tiny, deterministic, no extra dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[derive(Debug, PartialEq)]
struct Expected {
    allotted: Decimal,
    carried: Decimal,
    taken: Decimal,
    adjusted: Decimal,
    pending: Decimal,
    available: Decimal,
}

/// The card figures recomputed from raw ledger rows and booked days.
async fn from_ledger(app: &TestApp, user: Uuid, leave_type: Uuid, year: i32) -> Expected {
    let rows: Vec<(String, Decimal)> = sqlx::query_as(
        "SELECT reason::text, delta FROM balance_ledger
         WHERE user_id = $1 AND leave_type_id = $2 AND leave_year = $3",
    )
    .bind(user)
    .bind(leave_type)
    .bind(year)
    .fetch_all(&app.db.owner)
    .await
    .unwrap();
    let sum = |reasons: &[&str]| -> Decimal {
        rows.iter()
            .filter(|(r, _)| reasons.contains(&r.as_str()))
            .map(|(_, d)| *d)
            .sum()
    };
    let pending: Decimal = sqlx::query_scalar(
        "SELECT COALESCE(SUM(d.portion), 0) FROM leave_request_days d
         JOIN leave_requests r ON r.id = d.leave_request_id
         WHERE d.user_id = $1 AND r.leave_type_id = $2 AND d.leave_year = $3
           AND d.active AND r.status = 'pending'",
    )
    .bind(user)
    .bind(leave_type)
    .bind(year)
    .fetch_one(&app.db.owner)
    .await
    .unwrap();
    let total: Decimal = rows.iter().map(|(_, d)| *d).sum();
    Expected {
        allotted: sum(&["allotment", "prorate", "quota_change", "import"]),
        carried: sum(&["carry_forward", "negative_carry"]),
        taken: -sum(&["leave_taken", "leave_reversed"]),
        adjusted: sum(&[
            "manual_adjustment",
            "comp_off_credit",
            "comp_off_expired",
            "comp_off_revoked",
            "lapse",
        ]),
        pending,
        available: total - pending,
    }
}

fn dec_of(v: &serde_json::Value) -> Decimal {
    Decimal::try_from(v.as_f64().unwrap()).unwrap().normalize()
}

#[tokio::test]
async fn every_balance_number_equals_its_ledger_rows() {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let sl = create_leave_type(&app, &org, NewLeaveType::all("SL", dec!(6))).await;
    let types = [("CL", cl), ("SL", sl)];

    let mut created = 0;
    let mut decided = 0;
    for seed in [7_u64, 42, 1234, 98765] {
        let mut rng = Rng(seed);
        let user = create_user(
            &app,
            &org,
            NewUser::employee(&format!("user{seed}@acme.test")),
        )
        .await;
        let client = app.login_as(&user).await;
        for (_, t) in types {
            let allot = Decimal::from(2 + rng.below(12));
            add_ledger_row(&app, &user, t, 2026, allot, "allotment").await;
            if rng.below(2) == 0 {
                let carry = Decimal::from(rng.below(5));
                add_ledger_row(&app, &user, t, 2026, carry, "carry_forward").await;
            }
            if rng.below(2) == 0 {
                let adj = Decimal::new(rng.below(6) as i64 * 5 - 10, 1); // −1.0 … +1.5
                add_ledger_row(&app, &user, t, 2026, adj, "manual_adjustment").await;
            }
        }
        for _ in 0..8 {
            let (_, t) = types[rng.below(2) as usize];
            let start = 1 + rng.below(28) as u32;
            let len = rng.below(3) as u32;
            let mut body = leave_body(
                t,
                &format!("2026-12-{start:02}"),
                &format!("2026-12-{:02}", (start + len).min(31)),
            );
            if rng.below(3) == 0 {
                body["end_half"] = json!("first");
            }
            // Overlaps, weekends-only ranges and short balances are refused; that is fine.
            let (status, out) = post_leave(&app, &client, &body, &key()).await;
            if status != 201 {
                continue;
            }
            created += 1;
            let id = out["id"].as_str().unwrap();
            match rng.below(3) {
                0 => {
                    let (s, o) = decide(&app, &admin, id, approve()).await;
                    assert_eq!(s, 200, "{o}");
                    decided += 1;
                }
                1 => {
                    let (s, o) = decide(
                        &app,
                        &admin,
                        id,
                        json!({ "decision": "reject", "version": 1, "comment": "no" }),
                    )
                    .await;
                    assert_eq!(s, 200, "{o}");
                    decided += 1;
                }
                _ => {}
            }
        }

        for (code, t) in types {
            let card = balance_card(&app, &client, code, Some(2026)).await;
            let got = Expected {
                allotted: dec_of(&card["allotted"]),
                carried: dec_of(&card["carried"]),
                taken: dec_of(&card["taken"]),
                adjusted: dec_of(&card["adjusted"]),
                pending: dec_of(&card["pending"]),
                available: dec_of(&card["available"]),
            };
            let mut want = from_ledger(&app, user.id, t, 2026).await;
            for v in [
                &mut want.allotted,
                &mut want.carried,
                &mut want.taken,
                &mut want.adjusted,
                &mut want.pending,
                &mut want.available,
            ] {
                *v = v.normalize();
            }
            assert_eq!(got, want, "seed {seed}, {code}: {card}");
            // The card is self-consistent too.
            assert_eq!(
                got.allotted + got.carried - got.taken + got.adjusted - got.pending,
                got.available,
                "seed {seed}, {code}"
            );
        }
    }
    // The scenarios must actually exercise pending, approved and rejected requests.
    assert!(
        created >= 8 && decided >= 4,
        "created {created}, decided {decided}"
    );
}
