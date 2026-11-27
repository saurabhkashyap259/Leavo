//! T147 — comp-off expiry (FR-032, FR-060), driven through the real scheduler and job runner.
//! Kept with the API tests (not `crates/worker/tests`) so credits are created the way users
//! create them.
//!
//! The org's expiry period is 10 days: Sat 21 Nov expires on Tue 1 Dec, Sun 22 Nov on Wed 2 Dec.
//! The daily job runs from 00:15 in Asia/Kolkata. The clock starts on Tue 24 Nov, 14:30.
mod common;

use chrono::{TimeZone, Utc};
use common::factories::*;
use common::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
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
    sqlx::query("UPDATE organizations SET comp_off_expiry_days = 10 WHERE id = $1")
        .bind(org.id)
        .execute(&app.db.owner)
        .await
        .unwrap();
    let co = create_leave_type(&app, &org, NewLeaveType::comp_off()).await;
    let priya_u = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    let asha_user = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    World {
        priya: long_session(&app, &priya_u).await,
        asha: long_session(&app, &asha_user).await,
        asha_user,
        co,
        app,
    }
}

/// 00:20 in Kolkata on `day` of Nov/Dec 2026.
fn just_after_midnight(month: u32, day: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, month, day, 0, 20, 0).unwrap() - chrono::Duration::minutes(330)
}

async fn warnings(w: &World) -> Vec<String> {
    w.app
        .email
        .take()
        .into_iter()
        .filter(|m| m.to == "asha@acme.test" && m.subject.to_lowercase().contains("expir"))
        .map(|m| m.text)
        .collect()
}

async fn expired_rows(w: &World) -> Vec<Decimal> {
    ledger_rows(&w.app, w.asha_user.id, w.co)
        .await
        .into_iter()
        .filter(|(r, ..)| r == "comp_off_expired")
        .map(|(_, d, ..)| d)
        .collect()
}

#[tokio::test]
async fn warns_once_seven_days_ahead_then_expires_unused_days() {
    let w = world().await;
    earn_comp_off(
        &w.app,
        &w.asha,
        &w.priya,
        &[("2026-11-21", 1.0), ("2026-11-22", 1.0)],
    )
    .await;
    w.app.run_jobs().await;
    w.app.email.take();

    // Today (24 Nov) is 7 days before 1 Dec: one warning for the 21 Nov credit.
    w.app.tick().await;
    let sent = warnings(&w).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains("1 Dec 2026"), "{}", sent[0]);
    assert!(sent[0].contains("21 Nov 2026"), "{}", sent[0]);
    // Ticking again the same day sends nothing new.
    w.app.tick().await;
    assert!(warnings(&w).await.is_empty());

    // Wed 25 Nov: the 22 Nov credit's turn; the first one is not warned again.
    w.app.clock.set(just_after_midnight(11, 25));
    w.app.tick().await;
    let sent = warnings(&w).await;
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains("22 Nov 2026"), "{}", sent[0]);

    // Tue 1 Dec: the 21 Nov credit expires.
    w.app.clock.set(just_after_midnight(12, 1));
    w.app.tick().await;
    assert_eq!(expired_rows(&w).await, [dec!(-1)]);
    let mine = comp_offs(&w.app, &w.asha).await;
    assert_eq!(mine["available"], 1.0);
    assert_eq!(mine["credits"][0]["status"], "expired");
    assert_eq!(mine["credits"][0]["days_expired"], 1.0);
    assert_eq!(mine["credits"][1]["status"], "active");
    // Running again changes nothing.
    w.app.tick().await;
    assert_eq!(expired_rows(&w).await.len(), 1);
}

#[tokio::test]
async fn nothing_runs_before_a_quarter_past_midnight() {
    let w = world().await;
    earn_comp_off(&w.app, &w.asha, &w.priya, &[("2026-11-21", 1.0)]).await;
    // 00:10 on 1 Dec in Kolkata.
    w.app
        .clock
        .set(just_after_midnight(12, 1) - chrono::Duration::minutes(10));
    w.app.tick().await;
    assert!(expired_rows(&w).await.is_empty());
}

#[tokio::test]
async fn reserved_days_wait_for_the_decision_then_expire_at_once() {
    let w = world().await;
    earn_comp_off(&w.app, &w.asha, &w.priya, &[("2026-11-21", 1.0)]).await;
    let leave = submit(
        &w.app,
        &w.asha,
        &leave_body(w.co, "2026-12-07", "2026-12-07"),
    )
    .await;

    w.app.clock.set(just_after_midnight(12, 1));
    w.app.tick().await;
    assert!(
        expired_rows(&w).await.is_empty(),
        "reserved days must not expire"
    );
    let mine = comp_offs(&w.app, &w.asha).await;
    assert_eq!(mine["credits"][0]["days_reserved"], 1.0);

    // Rejected after the expiry date: the released day expires at that moment.
    let (status, out) = decide(
        &w.app,
        &w.priya,
        &leave,
        json!({ "decision": "reject", "version": 1, "comment": "Release" }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(expired_rows(&w).await, [dec!(-1)]);
    let mine = comp_offs(&w.app, &w.asha).await;
    assert_eq!(mine["available"], 0.0);
    assert_eq!(mine["credits"][0]["status"], "expired");
}

#[tokio::test]
async fn approved_spending_survives_expiry() {
    let w = world().await;
    earn_comp_off(&w.app, &w.asha, &w.priya, &[("2026-11-21", 1.0)]).await;
    let leave = submit(
        &w.app,
        &w.asha,
        &leave_body(w.co, "2026-11-30", "2026-11-30"),
    )
    .await;
    decide(&w.app, &w.priya, &leave, approve()).await;
    w.app.clock.set(just_after_midnight(12, 1));
    w.app.tick().await;
    assert!(expired_rows(&w).await.is_empty());
    let mine = comp_offs(&w.app, &w.asha).await;
    assert_eq!(mine["credits"][0]["status"], "used");
}
