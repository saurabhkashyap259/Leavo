//! T177 — the admin dashboard (FR-036, FR-044). The clock is Tue 24 Nov 2026, 14:30 IST.
mod common;

use chrono::{Duration, NaiveDate};
use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::{json, Value};

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

#[tokio::test]
async fn shows_who_is_away_what_waits_and_upcoming_holidays() {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    create_leave_type(&app, &org, NewLeaveType::comp_off()).await;
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    for u in [&ravi, &asha] {
        add_ledger_row(&app, u, cl, 2026, dec!(12), "allotment").await;
    }
    for (date, name) in [
        ("2026-11-08", "Diwali"),
        ("2026-12-25", "Christmas"),
        ("2026-12-31", "New Year's Eve"),
    ] {
        add_holiday(&app, &org, d(date), name, "mandatory").await;
    }
    let ravi_c = app.login_as(&ravi).await;
    let asha_c = app.login_as(&asha).await;
    // Ravi is away today.
    let id = submit(&app, &ravi_c, &leave_body(cl, "2026-11-24", "2026-11-25")).await;
    decide(&app, &admin, &id, approve()).await;
    // Asha's request has waited 3 days; another one just arrived; plus a comp-off request.
    app.clock.set(t0() - Duration::days(3));
    submit(&app, &asha_c, &leave_body(cl, "2026-12-07", "2026-12-07")).await;
    app.clock.set(t0());
    submit(&app, &asha_c, &leave_body(cl, "2026-12-08", "2026-12-08")).await;
    let (status, out) = post_comp_off(&app, &asha_c, &comp_off_body(&[("2026-11-21", 1.0)])).await;
    assert_eq!(status, 201, "{out}");

    let (status, out) = get_json(&admin, app.url("/api/v1/admin/dashboard")).await;
    assert_eq!(status, 200, "{out}");
    let away = out["away_today"].as_array().unwrap();
    assert_eq!(away.len(), 1);
    assert_eq!(away[0]["user"]["full_name"], "ravi");
    assert_eq!(away[0]["leave_type"]["code"], "CL");
    assert_eq!(away[0]["to"], "2026-11-25");
    assert_eq!(out["pending"], 3);
    assert_eq!(out["pending_over_48h"], 1);
    let names: Vec<&str> = out["upcoming_holidays"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Christmas", "New Year's Eve"]);
    assert_eq!(out["next_year_holidays_missing"], true);
    assert_eq!(
        out["setup_checklist"],
        json!({ "holidays": true, "approvers": false, "invite": true })
    );

    // Once next year has holidays, the reminder goes away.
    add_holiday(&app, &org, d("2027-01-26"), "Republic Day", "mandatory").await;
    let out: Value = get_json(&admin, app.url("/api/v1/admin/dashboard")).await.1;
    assert_eq!(out["next_year_holidays_missing"], false);
    assert_eq!(out["upcoming_holidays"].as_array().unwrap().len(), 3);

    let (status, _) = get_json(&asha_c, app.url("/api/v1/admin/dashboard")).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn the_holiday_reminder_starts_on_the_first_of_november() {
    let app = TestApp::spawn().await;
    let (_, _, _) = org_with_admin(&app, "acme.test").await;
    // 31 Oct 2026, 20:00 IST.
    app.clock
        .set(chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 10, 31, 14, 30, 0).unwrap());
    let admin = app.with_token(&create_session(&app, &admin_of(&app).await, true).await);
    let out = get_json(&admin, app.url("/api/v1/admin/dashboard")).await.1;
    assert_eq!(out["next_year_holidays_missing"], false);
    // 1 Nov 2026, 00:30 IST.
    app.clock
        .set(chrono::TimeZone::with_ymd_and_hms(&chrono::Utc, 2026, 10, 31, 19, 0, 0).unwrap());
    let out = get_json(&admin, app.url("/api/v1/admin/dashboard")).await.1;
    assert_eq!(out["next_year_holidays_missing"], true);
    assert_eq!(out["setup_checklist"]["invite"], false);
}

async fn admin_of(app: &TestApp) -> User {
    let (id, org_id): (uuid::Uuid, uuid::Uuid) =
        sqlx::query_as("SELECT id, org_id FROM users WHERE role = 'admin'")
            .fetch_one(&app.db.owner)
            .await
            .unwrap();
    User {
        id,
        org_id,
        email: "admin@acme.test".into(),
    }
}
