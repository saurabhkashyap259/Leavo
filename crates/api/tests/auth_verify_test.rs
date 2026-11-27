//! T056 — POST /auth/verify (FR-002, FR-005, FR-013).
mod common;
use chrono::{Duration, NaiveDate};
use common::factories::{create_leave_type, create_org, create_user, NewLeaveType, NewUser};
use common::{codes_sent_to, post_json, TestApp};
use leavo_core::clock::Clock;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;

async fn code_for(app: &TestApp, email: &str, purpose: &str, extra: serde_json::Value) -> String {
    let mut body = json!({ "email": email, "purpose": purpose });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let (status, _) = post_json(&app.anon(), app.url("/api/v1/auth/code"), body).await;
    assert_eq!(status, 202);
    codes_sent_to(app, email).await.pop().expect("code email")
}

async fn verify(app: &TestApp, body: serde_json::Value) -> reqwest::Response {
    app.anon()
        .post(app.url("/api/v1/auth/verify"))
        .json(&body)
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn correct_code_signs_in_with_a_secure_cookie() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let user = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let code = code_for(&app, "asha@acme.test", "login", json!({})).await;
    let res = verify(
        &app,
        json!({"email": "asha@acme.test", "code": code, "purpose": "login"}),
    )
    .await;
    assert_eq!(res.status(), 200);
    let cookie = res.headers()["set-cookie"].to_str().unwrap().to_string();
    assert!(cookie.starts_with("leavo_sid="));
    for attr in ["HttpOnly", "Secure", "SameSite=Lax", "Path=/"] {
        assert!(cookie.contains(attr), "{attr} missing in {cookie}");
    }
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["user"]["id"], user.id.to_string());

    let token = cookie
        .trim_start_matches("leavo_sid=")
        .split(';')
        .next()
        .unwrap();
    assert_eq!(
        app.with_token(token)
            .get(app.url("/api/v1/me"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}

#[tokio::test]
async fn session_lasts_7_days_or_30_with_keep_signed_in() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    for (email, keep, days) in [("short@acme.test", false, 7), ("long@acme.test", true, 30)] {
        create_user(&app, &org, NewUser::employee(email)).await;
        let code = code_for(&app, email, "login", json!({})).await;
        let res = verify(
            &app,
            json!({"email": email, "code": code, "purpose": "login", "keep_signed_in": keep}),
        )
        .await;
        assert_eq!(res.status(), 200);
        let exp: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
            "SELECT s.expires_at FROM sessions s JOIN users u ON u.id = s.user_id WHERE u.email = $1",
        ).bind(email).fetch_one(&app.db.owner).await.unwrap();
        assert_eq!(exp, app.clock.now() + Duration::days(days), "{email}");
    }
}

#[tokio::test]
async fn code_expires_after_10_minutes() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let code = code_for(&app, "asha@acme.test", "login", json!({})).await;
    app.clock
        .advance(Duration::minutes(10) + Duration::seconds(1));
    let res = verify(
        &app,
        json!({"email": "asha@acme.test", "code": code, "purpose": "login"}),
    )
    .await;
    assert_eq!(res.status(), 422);
    assert_eq!(
        res.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        "CODE_INVALID"
    );
}

#[tokio::test]
async fn code_works_only_once() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let code = code_for(&app, "asha@acme.test", "login", json!({})).await;
    let body = json!({"email": "asha@acme.test", "code": code, "purpose": "login"});
    assert_eq!(verify(&app, body.clone()).await.status(), 200);
    assert_eq!(verify(&app, body).await.status(), 422);
}

#[tokio::test]
async fn five_wrong_codes_lock_the_email_for_15_minutes() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let code = code_for(&app, "asha@acme.test", "login", json!({})).await;
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for i in 1..=4 {
        let res = verify(
            &app,
            json!({"email": "asha@acme.test", "code": wrong, "purpose": "login"}),
        )
        .await;
        assert_eq!(res.status(), 422, "attempt {i}");
    }
    let fifth = verify(
        &app,
        json!({"email": "asha@acme.test", "code": wrong, "purpose": "login"}),
    )
    .await;
    assert_eq!(fifth.status(), 429);
    assert_eq!(fifth.headers()["retry-after"], "900");
    assert_eq!(
        fifth.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        "LOCKED"
    );

    // Even the right code is refused while locked, and a fresh code doesn't bypass the lock.
    let right = verify(
        &app,
        json!({"email": "asha@acme.test", "code": code, "purpose": "login"}),
    )
    .await;
    assert_eq!(right.status(), 429);
    app.clock.advance(Duration::seconds(31));
    let fresh = code_for(&app, "asha@acme.test", "login", json!({})).await;
    assert_eq!(
        verify(
            &app,
            json!({"email": "asha@acme.test", "code": fresh, "purpose": "login"})
        )
        .await
        .status(),
        429
    );

    app.clock.advance(Duration::minutes(15));
    let after = code_for(&app, "asha@acme.test", "login", json!({})).await;
    assert_eq!(
        verify(
            &app,
            json!({"email": "asha@acme.test", "code": after, "purpose": "login"})
        )
        .await
        .status(),
        200
    );
}

#[tokio::test]
async fn deactivated_user_cannot_sign_in() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(
        &app,
        &org,
        NewUser::employee("gone@acme.test").status("deactivated"),
    )
    .await;
    let res = verify(
        &app,
        json!({"email": "gone@acme.test", "code": "123456", "purpose": "login"}),
    )
    .await;
    assert_eq!(res.status(), 422);
}

#[tokio::test]
async fn invited_user_is_activated_by_signing_in() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let user = create_user(
        &app,
        &org,
        NewUser::employee("new@acme.test").status("invited"),
    )
    .await;
    let code = code_for(&app, "new@acme.test", "login", json!({})).await;
    assert_eq!(
        verify(
            &app,
            json!({"email": "new@acme.test", "code": code, "purpose": "login"})
        )
        .await
        .status(),
        200
    );
    let status: String = sqlx::query_scalar("SELECT status::text FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    assert_eq!(status, "active");
}

#[tokio::test]
async fn signup_creates_an_employee_with_prorated_allotments() {
    let app = TestApp::spawn().await; // clock: 24 Nov 2026, leave year Jan–Dec
    let org = create_org(&app, "Acme", "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let sl = create_leave_type(&app, &org, NewLeaveType::all("SL", dec!(6))).await;
    let mut ml = NewLeaveType::all("ML", dec!(180));
    ml.availability = "restricted";
    create_leave_type(&app, &org, ml).await;
    let mut old = NewLeaveType::all("OLD", dec!(5));
    old.active = false;
    create_leave_type(&app, &org, old).await;
    create_leave_type(&app, &org, NewLeaveType::comp_off()).await;

    let code = code_for(
        &app,
        "ravi@acme.test",
        "signup",
        json!({"full_name": "Ravi Kumar", "date_of_joining": "2026-11-03"}),
    )
    .await;
    let res = verify(
        &app,
        json!({"email": "ravi@acme.test", "code": code, "purpose": "signup"}),
    )
    .await;
    assert_eq!(res.status(), 200);

    let (role, status, name, doj, org_id): (String, String, String, Option<NaiveDate>, uuid::Uuid) = sqlx::query_as(
        "SELECT role::text, status::text, full_name, date_of_joining, org_id FROM users WHERE email = 'ravi@acme.test'",
    ).fetch_one(&app.db.owner).await.unwrap();
    assert_eq!(
        (role.as_str(), status.as_str(), name.as_str()),
        ("employee", "active", "Ravi Kumar")
    );
    assert_eq!(doj, NaiveDate::from_ymd_opt(2026, 11, 3));
    assert_eq!(org_id, org.id);

    // Joined 3 Nov (≤ 15th) → Nov + Dec = 2 of 12 months: CL 12 → 2.0, SL 6 → 1.0.
    let rows: Vec<(uuid::Uuid, Decimal, String, i32)> = sqlx::query_as(
        "SELECT l.leave_type_id, l.delta, l.reason::text, l.leave_year FROM balance_ledger l
         JOIN users u ON u.id = l.user_id WHERE u.email = 'ravi@acme.test' ORDER BY l.delta DESC",
    )
    .fetch_all(&app.db.owner)
    .await
    .unwrap();
    assert_eq!(
        rows.len(),
        2,
        "only active, non-restricted, quota types: {rows:?}"
    );
    assert_eq!(rows[0], (cl, dec!(2.0), "prorate".into(), 2026));
    assert_eq!(rows[1], (sl, dec!(1.0), "prorate".into(), 2026));
}

#[tokio::test]
async fn signup_without_joining_date_gets_full_quota() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let code = code_for(
        &app,
        "ravi@acme.test",
        "signup",
        json!({"full_name": "Ravi"}),
    )
    .await;
    assert_eq!(
        verify(
            &app,
            json!({"email": "ravi@acme.test", "code": code, "purpose": "signup"})
        )
        .await
        .status(),
        200
    );
    let (delta, reason): (Decimal, String) =
        sqlx::query_as("SELECT delta, reason::text FROM balance_ledger")
            .fetch_one(&app.db.owner)
            .await
            .unwrap();
    assert_eq!((delta, reason.as_str()), (dec!(12), "allotment"));
}

#[tokio::test]
async fn signup_is_audited() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let code = code_for(
        &app,
        "ravi@acme.test",
        "signup",
        json!({"full_name": "Ravi"}),
    )
    .await;
    verify(
        &app,
        json!({"email": "ravi@acme.test", "code": code, "purpose": "signup"}),
    )
    .await;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log WHERE org_id = $1 AND action = 'user.signup'",
    )
    .bind(org.id)
    .fetch_one(&app.db.owner)
    .await
    .unwrap();
    assert_eq!(n, 1);
}
