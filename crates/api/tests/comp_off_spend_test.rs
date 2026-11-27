//! T146 — spending comp-off (FR-032, FR-060): a comp-off leave reserves credits when submitted,
//! consumes them soonest-expiry first when approved, and gives them back on rejection,
//! withdrawal or cancellation.
//!
//! Credits: Sat 7 Nov (1 day, expires 5 Feb), Diwali Mon 9 Nov (half, expires 7 Feb),
//! Sat 21 Nov (1 day, expires 19 Feb). The clock starts on Tue 24 Nov 2026.
mod common;

use chrono::{NaiveDate, TimeZone, Utc};
use common::factories::*;
use common::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use uuid::Uuid;

struct World {
    app: TestApp,
    priya: reqwest::Client,
    asha_user: User,
    asha: reqwest::Client,
    co: Uuid,
}

async fn long_session(app: &TestApp, user: &User) -> reqwest::Client {
    app.with_token(&create_session(app, user, true).await)
}

async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, _, _) = org_with_admin(&app, "acme.test").await;
    let co = create_leave_type(&app, &org, NewLeaveType::comp_off()).await;
    add_holiday(
        &app,
        &org,
        NaiveDate::from_ymd_opt(2026, 11, 9).unwrap(),
        "Diwali",
        "mandatory",
    )
    .await;
    let priya_u = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    let asha_user = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    let w = World {
        priya: long_session(&app, &priya_u).await,
        asha: long_session(&app, &asha_user).await,
        asha_user,
        co,
        app,
    };
    earn_comp_off(
        &w.app,
        &w.asha,
        &w.priya,
        &[
            ("2026-11-21", 1.0),
            ("2026-11-09", 0.5),
            ("2026-11-07", 1.0),
        ],
    )
    .await;
    w
}

/// `(worked_date, days_reserved, days_used, status)` per credit, soonest expiry first.
async fn credits(w: &World) -> Vec<(String, f64, f64, String)> {
    comp_offs(&w.app, &w.asha).await["credits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["worked_date"].as_str().unwrap().to_string(),
                c["days_reserved"].as_f64().unwrap(),
                c["days_used"].as_f64().unwrap(),
                c["status"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn row(date: &str, reserved: f64, used: f64, status: &str) -> (String, f64, f64, String) {
    (date.into(), reserved, used, status.into())
}

async fn available(w: &World) -> Value {
    comp_offs(&w.app, &w.asha).await["available"].clone()
}

async fn spend(w: &World, from: &str, to: &str) -> String {
    submit(&w.app, &w.asha, &leave_body(w.co, from, to)).await
}

#[tokio::test]
async fn submit_reserves_and_approval_consumes_the_soonest_expiring_credits() {
    let w = world().await;
    assert_eq!(available(&w).await, 2.5);
    let id = spend(&w, "2026-11-30", "2026-12-01").await;
    assert_eq!(
        credits(&w).await,
        [
            row("2026-11-07", 1.0, 0.0, "active"),
            row("2026-11-09", 0.5, 0.0, "active"),
            row("2026-11-21", 0.5, 0.0, "active"),
        ]
    );
    assert_eq!(available(&w).await, 0.5);

    let (status, out) = decide(&w.app, &w.priya, &id, approve()).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(
        credits(&w).await,
        [
            row("2026-11-07", 0.0, 1.0, "used"),
            row("2026-11-09", 0.0, 0.5, "used"),
            row("2026-11-21", 0.0, 0.5, "active"),
        ]
    );
    assert_eq!(available(&w).await, 0.5);
    let mine = comp_offs(&w.app, &w.asha).await;
    assert_eq!(
        mine["credits"][0]["used_on"],
        json!([{ "leave_request_id": id, "from": "2026-11-30", "to": "2026-12-01", "days": 1.0 }])
    );
    let usages: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM comp_off_usages WHERE leave_request_id = $1::uuid AND days_used > 0",
    )
    .bind(&id)
    .fetch_one(&w.app.db.owner)
    .await
    .unwrap();
    assert_eq!(usages, 3);
    let taken: Decimal = ledger_rows(&w.app, w.asha_user.id, w.co)
        .await
        .into_iter()
        .filter(|(r, ..)| r == "leave_taken")
        .map(|(_, d, ..)| d)
        .sum();
    assert_eq!(taken, dec!(-2));
}

#[tokio::test]
async fn rejection_and_withdrawal_release_the_reservation() {
    let w = world().await;
    let rejected = spend(&w, "2026-11-30", "2026-11-30").await;
    let (status, _) = decide(
        &w.app,
        &w.priya,
        &rejected,
        json!({ "decision": "reject", "version": 1, "comment": "Busy" }),
    )
    .await;
    assert_eq!(status, 200);
    assert!(credits(&w).await.iter().all(|c| c.1 == 0.0 && c.2 == 0.0));
    assert_eq!(available(&w).await, 2.5);

    let withdrawn = spend(&w, "2026-12-01", "2026-12-02").await;
    assert_eq!(available(&w).await, 0.5);
    let (status, _) = post_json(
        &w.asha,
        w.app.url(&format!("/api/v1/leaves/{withdrawn}/cancel")),
        json!({}),
    )
    .await;
    assert_eq!(status, 200);
    assert!(credits(&w).await.iter().all(|c| c.1 == 0.0 && c.2 == 0.0));
    assert_eq!(available(&w).await, 2.5);
}

#[tokio::test]
async fn cancelling_approved_comp_off_leave_returns_the_credit() {
    let w = world().await;
    let id = spend(&w, "2026-11-30", "2026-12-01").await;
    decide(&w.app, &w.priya, &id, approve()).await;
    let (status, out) = post_json(
        &w.asha,
        w.app.url(&format!("/api/v1/leaves/{id}/cancel")),
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert!(credits(&w).await.iter().all(|c| c.1 == 0.0 && c.2 == 0.0));
    assert_eq!(available(&w).await, 2.5);
}

#[tokio::test]
async fn a_partial_cancellation_returns_the_latest_expiring_usage_first() {
    let w = world().await;
    let id = spend(&w, "2026-11-30", "2026-12-01").await;
    decide(&w.app, &w.priya, &id, approve()).await;
    // On Tue 1 Dec Asha cancels the rest: 1 day comes back.
    w.app
        .clock
        .set(Utc.with_ymd_and_hms(2026, 12, 1, 4, 30, 0).unwrap());
    let (status, out) = post_json(
        &w.asha,
        w.app.url(&format!("/api/v1/leaves/{id}/cancel")),
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "cancel_requested");
    let (status, out) = decide_kind(
        &w.app,
        &w.priya,
        "cancel",
        &id,
        json!({ "decision": "approve", "version": 3 }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    // The day kept stays on the credit expiring first; the later-expiring usages come back.
    assert_eq!(
        credits(&w).await,
        [
            row("2026-11-07", 0.0, 1.0, "used"),
            row("2026-11-09", 0.0, 0.0, "active"),
            row("2026-11-21", 0.0, 0.0, "active"),
        ]
    );
    assert_eq!(available(&w).await, 1.5);
}
