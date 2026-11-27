//! T159 — holiday changes inside booked leave (FR-035).
//!
//! Adding a mandatory holiday frees that day: approved leave gets it back with `leave_reversed`,
//! pending leave is recounted. Deleting a mandatory holiday, or making it optional, books the
//! day again: approved leave is charged with `leave_taken` even below zero, pending leave is
//! recounted, and the employee and admins are emailed.
//!
//! The clock is Tue 24 Nov 2026. Asha (Eng, approver Priya) has 12 days of CL unless a test
//! says otherwise.
mod common;

use chrono::NaiveDate;
use common::factories::*;
use common::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use uuid::Uuid;

struct World {
    app: TestApp,
    org: Org,
    admin: reqwest::Client,
    priya: reqwest::Client,
    asha_user: User,
    asha: reqwest::Client,
    cl: Uuid,
}

async fn world(quota: Decimal) -> World {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let priya_u = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    let asha_user = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    add_ledger_row(&app, &asha_user, cl, 2026, quota, "allotment").await;
    World {
        priya: app.login_as(&priya_u).await,
        asha: app.login_as(&asha_user).await,
        asha_user,
        admin,
        cl,
        org,
        app,
    }
}

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

async fn request(w: &World, body: Value) -> String {
    submit(&w.app, &w.asha, &body).await
}

async fn approved(w: &World, body: Value) -> String {
    let id = request(w, body).await;
    let (status, out) = decide(&w.app, &w.priya, &id, approve()).await;
    assert_eq!(status, 200, "{out}");
    id
}

async fn add(w: &World, date: &str, kind: &str) -> String {
    let (status, out) = post_json(
        &w.admin,
        w.app.url("/api/v1/admin/holidays"),
        json!({ "date": date, "name": format!("Holiday {date}"), "kind": kind }),
    )
    .await;
    assert_eq!(status, 201, "{out}");
    out["id"].as_str().unwrap().to_string()
}

async fn ledger(w: &World) -> Vec<(String, Decimal)> {
    ledger_rows(&w.app, w.asha_user.id, w.cl)
        .await
        .into_iter()
        .map(|(r, d, ..)| (r, d))
        .collect()
}

fn entry(reason: &str, delta: Decimal) -> (String, Decimal) {
    (reason.into(), delta)
}

/// Active booked days of a request as `(day, portion)`.
async fn booked(w: &World, id: &str) -> Vec<(String, Decimal)> {
    sqlx::query_as(
        "SELECT day::text, portion FROM leave_request_days
         WHERE leave_request_id = $1::uuid AND active ORDER BY day",
    )
    .bind(id)
    .fetch_all(&w.app.db.owner)
    .await
    .unwrap()
}

async fn detail_days(w: &World, id: &str) -> Value {
    get_json(&w.asha, w.app.url(&format!("/api/v1/leaves/{id}")))
        .await
        .1["days"]
        .clone()
}

async fn card(w: &World) -> (Value, Value, Value) {
    let c = balance_card(&w.app, &w.asha, "CL", None).await;
    (
        c["taken"].clone(),
        c["pending"].clone(),
        c["available"].clone(),
    )
}

/// `(to, subject)` of emails sent since the last call.
async fn mail(w: &World) -> Vec<(String, String)> {
    w.app.run_jobs().await;
    w.app
        .email
        .take()
        .into_iter()
        .map(|m| (m.to, m.subject))
        .collect()
}

fn holiday_mail(sent: &[(String, String)]) -> Vec<String> {
    let mut to: Vec<String> = sent
        .iter()
        .filter(|m| m.1.to_lowercase().contains("holiday"))
        .map(|m| m.0.clone())
        .collect();
    to.sort();
    to
}

#[tokio::test]
async fn a_new_mandatory_holiday_inside_approved_leave_is_given_back() {
    let w = world(dec!(12)).await;
    let id = approved(&w, leave_body(w.cl, "2026-11-30", "2026-12-02")).await;
    mail(&w).await;
    add(&w, "2026-12-01", "mandatory").await;

    assert_eq!(
        ledger(&w).await,
        [
            entry("allotment", dec!(12)),
            entry("leave_taken", dec!(-3)),
            entry("leave_reversed", dec!(1))
        ]
    );
    assert_eq!(
        booked(&w, &id).await,
        [
            ("2026-11-30".into(), dec!(1)),
            ("2026-12-02".into(), dec!(1))
        ]
    );
    assert_eq!(detail_days(&w, &id).await, 2.0);
    assert_eq!(card(&w).await, (json!(2.0), json!(0.0), json!(10.0)));
    assert_eq!(holiday_mail(&mail(&w).await), ["asha@acme.test"]);

    // An optional holiday is a working day: nothing changes.
    add(&w, "2026-12-02", "optional").await;
    assert_eq!(ledger(&w).await.len(), 3);
    assert_eq!(booked(&w, &id).await.len(), 2);
}

#[tokio::test]
async fn a_new_mandatory_holiday_inside_pending_leave_is_recounted_without_ledger() {
    let w = world(dec!(12)).await;
    let id = request(&w, leave_body(w.cl, "2026-12-07", "2026-12-09")).await;
    add(&w, "2026-12-08", "mandatory").await;
    assert_eq!(ledger(&w).await, [entry("allotment", dec!(12))]);
    assert_eq!(booked(&w, &id).await.len(), 2);
    assert_eq!(detail_days(&w, &id).await, 2.0);
    assert_eq!(card(&w).await, (json!(0.0), json!(2.0), json!(10.0)));
}

#[tokio::test]
async fn deleting_a_holiday_inside_approved_leave_charges_it_even_below_zero() {
    let w = world(dec!(2)).await;
    let holiday = add(&w, "2026-12-01", "mandatory").await;
    // 30 Nov – 2 Dec around the holiday: 2 days, all Asha has.
    let id = approved(&w, leave_body(w.cl, "2026-11-30", "2026-12-02")).await;
    assert_eq!(card(&w).await, (json!(2.0), json!(0.0), json!(0.0)));
    mail(&w).await;

    let (status, _) = delete(
        &w.admin,
        w.app.url(&format!("/api/v1/admin/holidays/{holiday}")),
    )
    .await;
    assert_eq!(status, 204);
    assert_eq!(
        ledger(&w).await,
        [
            entry("allotment", dec!(2)),
            entry("leave_taken", dec!(-2)),
            entry("leave_taken", dec!(-1))
        ]
    );
    assert_eq!(booked(&w, &id).await.len(), 3);
    assert_eq!(detail_days(&w, &id).await, 3.0);
    assert_eq!(card(&w).await, (json!(3.0), json!(0.0), json!(-1.0)));
    assert_eq!(
        holiday_mail(&mail(&w).await),
        ["admin@acme.test", "asha@acme.test"]
    );
}

#[tokio::test]
async fn making_a_holiday_optional_books_the_day_again_with_half_day_edges() {
    let w = world(dec!(12)).await;
    let holiday = add(&w, "2026-11-30", "mandatory").await;
    // Starts on the holiday with the second half only: 30 Nov is not counted.
    let mut body = leave_body(w.cl, "2026-11-30", "2026-12-01");
    body["start_half"] = json!("second");
    let id = approved(&w, body).await;
    assert_eq!(booked(&w, &id).await, [("2026-12-01".into(), dec!(1))]);

    let (status, out) = patch_json(
        &w.admin,
        w.app.url(&format!("/api/v1/admin/holidays/{holiday}")),
        json!({ "kind": "optional" }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    // The day comes back as the half the request asked for.
    assert_eq!(
        booked(&w, &id).await,
        [
            ("2026-11-30".into(), dec!(0.5)),
            ("2026-12-01".into(), dec!(1))
        ]
    );
    assert_eq!(
        ledger(&w).await.last().unwrap(),
        &entry("leave_taken", dec!(-0.5))
    );
    assert_eq!(detail_days(&w, &id).await, 1.5);
}

#[tokio::test]
async fn moving_a_holiday_frees_the_new_date_and_books_the_old_one() {
    let w = world(dec!(12)).await;
    let holiday = add(&w, "2026-12-01", "mandatory").await;
    let pending = request(&w, leave_body(w.cl, "2026-11-30", "2026-12-04")).await;
    assert_eq!(detail_days(&w, &pending).await, 4.0);
    let (status, _) = patch_json(
        &w.admin,
        w.app.url(&format!("/api/v1/admin/holidays/{holiday}")),
        json!({ "date": "2026-12-03" }),
    )
    .await;
    assert_eq!(status, 200);
    let days: Vec<String> = booked(&w, &pending)
        .await
        .into_iter()
        .map(|b| b.0)
        .collect();
    assert_eq!(
        days,
        ["2026-11-30", "2026-12-01", "2026-12-02", "2026-12-04"]
    );
    assert_eq!(ledger(&w).await, [entry("allotment", dec!(12))]);
}

#[tokio::test]
async fn imported_holidays_have_the_same_effect() {
    let w = world(dec!(12)).await;
    let id = approved(&w, leave_body(w.cl, "2026-12-07", "2026-12-08")).await;
    add_holiday(&w.app, &w.org, d("2026-12-25"), "Christmas", "mandatory").await;
    let (status, out) = post_json(
        &w.admin,
        w.app.url("/api/v1/admin/holidays/import?dry_run=false"),
        json!({ "copy_from_year": 2025, "to_year": 2026 }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    let form = reqwest::multipart::Form::new().part(
        "file",
        reqwest::multipart::Part::bytes(b"date,name\n2026-12-08,Founders Day\n".to_vec())
            .file_name("h.csv"),
    );
    let res = w
        .admin
        .post(w.app.url("/api/v1/admin/holidays/import?dry_run=false"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(booked(&w, &id).await, [("2026-12-07".into(), dec!(1))]);
    assert_eq!(
        ledger(&w).await.last().unwrap(),
        &entry("leave_reversed", dec!(1))
    );
}
