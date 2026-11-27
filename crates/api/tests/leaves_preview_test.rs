//! T092 — `POST /leaves/preview` (FR-016, FR-055): working days, excluded dates with names,
//! per-leave-year charges and catalogue error codes. Nothing is stored.
mod common;

use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::json;

fn d(y: i32, m: u32, day: u32) -> chrono::NaiveDate {
    chrono::NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

struct Fixture {
    app: TestApp,
    org: Org,
    asha: User,
    client: reqwest::Client,
    cl: uuid::Uuid,
}

/// Today is Tue 24 Nov 2026. Asha has 12 days of CL for 2026; an admin exists to approve.
async fn fixture() -> Fixture {
    let app = TestApp::spawn().await;
    let (org, _admin, _) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let asha = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    add_ledger_row(&app, &asha, cl, 2026, dec!(12), "allotment").await;
    add_holiday(&app, &org, d(2026, 12, 25), "Christmas", "mandatory").await;
    add_holiday(&app, &org, d(2026, 12, 24), "Christmas Eve", "optional").await;
    let client = app.login_as(&asha).await;
    Fixture {
        app,
        org,
        asha,
        client,
        cl,
    }
}

async fn preview(f: &Fixture, body: serde_json::Value) -> serde_json::Value {
    let (status, out) = post_json(&f.client, f.app.url("/api/v1/leaves/preview"), body).await;
    assert_eq!(status, 200, "{out}");
    out
}

fn error_codes(out: &serde_json::Value) -> Vec<String> {
    out["errors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["code"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn excludes_weekends_and_mandatory_holidays_with_names() {
    let f = fixture().await;
    // Mon 21 Dec – Mon 28 Dec: the optional holiday (24th) counts, Christmas does not.
    let out = preview(&f, leave_body(f.cl, "2026-12-21", "2026-12-28")).await;
    assert_eq!(out["days"], 5.0);
    assert_eq!(
        out["excluded"],
        json!([
            { "date": "2026-12-25", "reason": "holiday", "name": "Christmas" },
            { "date": "2026-12-26", "reason": "weekend" },
            { "date": "2026-12-27", "reason": "weekend" },
        ])
    );
    assert_eq!(
        out["by_leave_year"],
        json!([{ "leave_year": 2026, "days": 5.0, "available_after": 7.0 }])
    );
    assert_eq!(out["errors"], json!([]));

    // Nothing was stored.
    let (n,): (i64,) = sqlx::query_as("SELECT count(*) FROM leave_requests")
        .fetch_one(&f.app.db.owner)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn a_range_across_the_leave_year_boundary_is_charged_to_each_year() {
    let f = fixture().await;
    // Tue 29 Dec – Sat 2 Jan: 3 days in 2026, Fri 1 Jan in 2027. 2027 has not started, so its
    // available balance is the projected full quota (12).
    let out = preview(&f, leave_body(f.cl, "2026-12-29", "2027-01-02")).await;
    assert_eq!(out["days"], 4.0);
    assert_eq!(
        out["by_leave_year"],
        json!([
            { "leave_year": 2026, "days": 3.0, "available_after": 9.0 },
            { "leave_year": 2027, "days": 1.0, "available_after": 11.0 },
        ])
    );
    assert_eq!(out["errors"], json!([]));
}

#[tokio::test]
async fn errors_use_catalogue_codes_and_messages() {
    let f = fixture().await;
    let mut body = leave_body(f.cl, "2026-12-28", "2026-12-31");
    body["notify"] = json!((0..21)
        .map(|i| json!({ "email": format!("p{i}@example.com") }))
        .collect::<Vec<_>>());
    sqlx::query("UPDATE leave_types SET max_days_per_request = 2 WHERE id = $1")
        .bind(f.cl)
        .execute(&f.app.db.owner)
        .await
        .unwrap();
    let out = preview(&f, body).await;
    assert_eq!(error_codes(&out), ["TOO_MANY_NOTIFY", "MAX_DAYS_EXCEEDED"]);
    assert_eq!(
        out["errors"][0]["message"],
        "You can notify at most 20 people."
    );

    let out = preview(&f, leave_body(f.cl, "2026-12-05", "2026-12-06")).await;
    assert_eq!(error_codes(&out), ["ZERO_WORKING_DAYS"]);
    assert_eq!(out["days"], 0.0);

    let out = preview(&f, leave_body(f.cl, "2026-12-10", "2026-12-09")).await;
    assert_eq!(error_codes(&out), ["INVALID_RANGE"]);
}

#[tokio::test]
async fn an_empty_reason_is_not_an_error_while_previewing() {
    let f = fixture().await;
    let mut body = leave_body(f.cl, "2026-12-01", "2026-12-01");
    body["reason"] = json!("");
    assert_eq!(error_codes(&preview(&f, body).await), Vec::<String>::new());
}

#[tokio::test]
async fn restricted_type_without_an_active_grant_is_not_available() {
    let f = fixture().await;
    let ml = create_leave_type(&f.app, &f.org, NewLeaveType::restricted("ML", dec!(130))).await;
    let out = preview(&f, leave_body(ml, "2026-12-01", "2026-12-02")).await;
    assert!(
        error_codes(&out).contains(&"TYPE_NOT_AVAILABLE".to_string()),
        "{out}"
    );

    // An inactive type and an unknown id are not available either.
    let old = create_leave_type(&f.app, &f.org, NewLeaveType::all("OLD", dec!(5)).inactive()).await;
    let out = preview(&f, leave_body(old, "2026-12-01", "2026-12-01")).await;
    assert!(error_codes(&out).contains(&"TYPE_NOT_AVAILABLE".to_string()));
    let out = preview(
        &f,
        leave_body(uuid::Uuid::now_v7(), "2026-12-01", "2026-12-01"),
    )
    .await;
    assert!(error_codes(&out).contains(&"TYPE_NOT_AVAILABLE".to_string()));

    // Once granted, it is available.
    sqlx::query(
        "INSERT INTO leave_type_grants (id, org_id, leave_type_id, user_id) VALUES ($1, $2, $3, $4)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(f.org.id)
    .bind(ml)
    .bind(f.asha.id)
    .execute(&f.app.db.owner)
    .await
    .unwrap();
    add_ledger_row(&f.app, &f.asha, ml, 2026, dec!(130), "allotment").await;
    let out = preview(&f, leave_body(ml, "2026-12-01", "2026-12-02")).await;
    assert_eq!(out["errors"], json!([]), "{out}");
}

#[tokio::test]
async fn preview_requires_a_session() {
    let f = fixture().await;
    let (status, _) = post_json(
        &f.app.anon(),
        f.app.url("/api/v1/leaves/preview"),
        leave_body(f.cl, "2026-12-01", "2026-12-01"),
    )
    .await;
    assert_eq!(status, 401);
}
