//! T067 — first-run setup wizard (FR-007, FR-008, FR-011, FR-034).
mod common;

use chrono::Duration;
use common::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};

async fn token(app: &TestApp) -> String {
    leavo_api::setup::token::regenerate(&app.state.db)
        .await
        .unwrap()
}

async fn admin_code(app: &TestApp, token: &str, email: &str) -> String {
    let (status, body) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/code"),
        json!({ "email": email, "purpose": "setup", "setup_token": token }),
    )
    .await;
    assert_eq!(status, 202, "{body}");
    codes_sent_to(app, email)
        .await
        .pop()
        .expect("setup code emailed")
}

fn complete_body(token: &str, code: &str) -> Value {
    json!({
        "setup_token": token,
        "org": { "name": "Acme", "timezone": "Asia/Kolkata", "weekend": ["sat", "sun"], "leave_year_start_month": 1 },
        "admin": { "full_name": "Asha Rao", "email": "asha@acme.test", "code": code },
        "extra_domains": ["Acme.IN"],
        "holiday_pack": "IN/2026",
        "seed_leave_types": true
    })
}

#[tokio::test]
async fn status_reports_setup_required_on_an_empty_instance() {
    let app = TestApp::spawn().await;
    let (status, body) = get_json(&app.anon(), app.url("/api/v1/setup/status")).await;
    assert_eq!(status, 200);
    assert_eq!(body["setup_required"], true);
    assert_eq!(body["mode"], "single");
    let packs = body["holiday_packs"].as_array().unwrap();
    assert!(packs
        .iter()
        .any(|p| p["id"] == "IN/2026" && p["count"].as_u64().unwrap() > 0));
    assert!(packs.iter().any(|p| p["id"] == "IN/2027"));
}

#[tokio::test]
async fn verify_token_accepts_the_right_token_and_locks_after_five_wrong_tries() {
    let app = TestApp::spawn().await;
    let t = token(&app).await;
    let url = app.url("/api/v1/setup/verify-token");
    let client = app.anon();

    let (status, _) = post_json(&client, url.clone(), json!({ "setup_token": t })).await;
    assert_eq!(status, 204);

    for _ in 0..5 {
        let (status, body) =
            post_json(&client, url.clone(), json!({ "setup_token": "nope" })).await;
        assert_eq!(status, 403);
        assert_eq!(body["error"]["code"], "SETUP_TOKEN_INVALID");
    }
    // Locked for 15 minutes, even with the right token.
    let res = client
        .post(url.clone())
        .json(&json!({ "setup_token": t }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 429);
    assert!(res.headers().get("retry-after").is_some());
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "LOCKED");

    app.clock.advance(Duration::minutes(15));
    let (status, _) = post_json(&client, url, json!({ "setup_token": t })).await;
    assert_eq!(status, 204);
}

#[tokio::test]
async fn test_email_sends_now_and_reports_the_smtp_error_text() {
    let app = TestApp::spawn().await;
    let t = token(&app).await;
    let url = app.url("/api/v1/setup/test-email");

    let (status, body) = post_json(
        &app.anon(),
        url.clone(),
        json!({ "setup_token": t, "to": "ops@acme.test" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["ok"], true);
    assert!(app.email.sent().iter().any(|m| m.to == "ops@acme.test"));

    app.email.fail_next("554 relay access denied");
    let (status, body) = post_json(
        &app.anon(),
        url.clone(),
        json!({ "setup_token": t, "to": "ops@acme.test" }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["ok"], false);
    assert!(body["error"]
        .as_str()
        .unwrap()
        .contains("554 relay access denied"));

    let (status, body) = post_json(
        &app.anon(),
        url,
        json!({ "setup_token": "wrong", "to": "ops@acme.test" }),
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["code"], "SETUP_TOKEN_INVALID");
}

#[tokio::test]
async fn setup_code_requires_the_setup_token_and_a_company_domain() {
    let app = TestApp::spawn().await;
    let t = token(&app).await;
    let url = app.url("/api/v1/auth/code");
    let (status, body) = post_json(
        &app.anon(),
        url.clone(),
        json!({ "email": "asha@acme.test", "purpose": "setup", "setup_token": "wrong" }),
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["code"], "SETUP_TOKEN_INVALID");

    let (status, body) = post_json(
        &app.anon(),
        url,
        json!({ "email": "asha@gmail.com", "purpose": "setup", "setup_token": t }),
    )
    .await;
    assert_eq!(status, 422);
    assert_eq!(body["error"]["code"], "PUBLIC_DOMAIN");
}

#[tokio::test]
async fn complete_creates_org_admin_domains_types_and_holidays_then_retires_the_token() {
    let app = TestApp::spawn().await;
    let t = token(&app).await;
    let code = admin_code(&app, &t, "asha@acme.test").await;

    let res = app
        .anon()
        .post(app.url("/api/v1/setup/complete"))
        .json(&complete_body(&t, &code))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let sid = session_cookie(&res).expect("admin is signed in");
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["user"]["role"], "admin");
    assert_eq!(body["user"]["email"], "asha@acme.test");

    // Signed in as admin of the new org.
    let admin = app.with_token(&sid);
    let (status, me) = get_json(&admin, app.url("/api/v1/me")).await;
    assert_eq!(status, 200);
    assert_eq!(me["role"], "admin");
    assert_eq!(me["org"]["name"], "Acme");
    assert_eq!(me["org"]["timezone"], "Asia/Kolkata");
    assert_eq!(me["org"]["weekend"], json!(["sat", "sun"]));

    let owner = &app.db.owner;
    let domains: Vec<String> =
        sqlx::query_scalar("SELECT domain::text FROM org_domains ORDER BY domain")
            .fetch_all(owner)
            .await
            .unwrap();
    assert_eq!(domains, ["acme.in", "acme.test"]);

    // Seeded types (data-model.md §leave_types).
    #[allow(clippy::type_complexity)]
    let types: Vec<(
        String,
        Option<Decimal>,
        String,
        i32,
        Option<Decimal>,
        bool,
        bool,
    )> = sqlx::query_as(
        "SELECT code, yearly_quota, availability::text, max_backdate_days,
                attachment_required_above_days, is_comp_off, receives_carry_forward
         FROM leave_types ORDER BY sort_order",
    )
    .fetch_all(owner)
    .await
    .unwrap();
    let codes: Vec<&str> = types.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(codes, ["CL", "SL", "CCL", "ML", "PL", "CO"]);
    let by = |c: &str| types.iter().find(|t| t.0 == c).unwrap().clone();
    let cl = by("CL");
    assert_eq!(
        (cl.1, cl.2.as_str(), cl.3, cl.6),
        (Some(dec!(12)), "all", 0, true)
    );
    let sl = by("SL");
    assert_eq!(
        (sl.1, sl.2.as_str(), sl.3, sl.4),
        (Some(dec!(6)), "all", 7, Some(dec!(2)))
    );
    for restricted in ["CCL", "ML", "PL"] {
        assert_eq!(by(restricted).2, "restricted", "{restricted}");
        assert_eq!(by(restricted).3, 0);
    }
    let co = by("CO");
    assert!(co.5);
    assert_eq!(co.1, None);

    // Holiday pack imported.
    let holidays: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM holidays WHERE source = 'pack:IN/2026' AND date_part('year', date) = 2026",
    )
    .fetch_one(owner)
    .await
    .unwrap();
    assert!(holidays >= 10, "{holidays}");

    // The admin is an employee too: full CL and SL for this leave year.
    let (status, balances) = get_json(&admin, app.url("/api/v1/me/balances")).await;
    assert_eq!(status, 200);
    let codes: Vec<&str> = balances
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["leave_type"]["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["CL", "SL", "CO"]);
    assert_eq!(balances[0]["allotted"], 12.0);

    // Token retired; setup is closed.
    let hash: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT setup_token_hash FROM instance_settings")
            .fetch_one(owner)
            .await
            .unwrap();
    assert!(hash.is_none());
    let (_, status_body) = get_json(&app.anon(), app.url("/api/v1/setup/status")).await;
    assert_eq!(status_body["setup_required"], false);
    let (status, body) = post_json(
        &app.anon(),
        app.url("/api/v1/setup/verify-token"),
        json!({ "setup_token": t }),
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["code"], "SETUP_TOKEN_INVALID");
    let (status, body) = post_json(
        &app.anon(),
        app.url("/api/v1/setup/complete"),
        complete_body(&t, &code),
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(body["error"]["code"], "SETUP_TOKEN_INVALID");
    // Regenerating is refused once an organization exists.
    assert!(leavo_api::setup::token::regenerate(&app.state.db)
        .await
        .is_err());

    let audit: i64 =
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'org.setup'")
            .fetch_one(owner)
            .await
            .unwrap();
    assert_eq!(audit, 1);
}

#[tokio::test]
async fn complete_without_seeding_or_pack_creates_only_the_org() {
    let app = TestApp::spawn().await;
    let t = token(&app).await;
    let code = admin_code(&app, &t, "asha@acme.test").await;
    let mut body = complete_body(&t, &code);
    body["seed_leave_types"] = json!(false);
    body["holiday_pack"] = Value::Null;
    body["extra_domains"] = json!([]);
    let (status, res) = post_json(&app.anon(), app.url("/api/v1/setup/complete"), body).await;
    assert_eq!(status, 200, "{res}");
    let types: i64 = sqlx::query_scalar("SELECT count(*) FROM leave_types")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    let holidays: i64 = sqlx::query_scalar("SELECT count(*) FROM holidays")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    assert_eq!((types, holidays), (0, 0));
}

#[tokio::test]
async fn complete_validates_its_input_and_the_code() {
    let app = TestApp::spawn().await;
    let t = token(&app).await;
    let code = admin_code(&app, &t, "asha@acme.test").await;
    let url = app.url("/api/v1/setup/complete");

    let mut bad_tz = complete_body(&t, &code);
    bad_tz["org"]["timezone"] = json!("Mars/Olympus");
    let (status, body) = post_json(&app.anon(), url.clone(), bad_tz).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (422, Some("VALIDATION"))
    );
    assert_eq!(body["error"]["details"]["field"], "org.timezone");

    let mut bad_pack = complete_body(&t, &code);
    bad_pack["holiday_pack"] = json!("XX/1999");
    let (status, body) = post_json(&app.anon(), url.clone(), bad_pack).await;
    assert_eq!(
        (status, body["error"]["details"]["field"].as_str()),
        (422, Some("holiday_pack"))
    );

    let mut public_extra = complete_body(&t, &code);
    public_extra["extra_domains"] = json!(["gmail.com"]);
    let (status, body) = post_json(&app.anon(), url.clone(), public_extra).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (422, Some("PUBLIC_DOMAIN"))
    );

    let (status, body) = post_json(&app.anon(), url.clone(), complete_body(&t, "000000")).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (422, Some("CODE_INVALID"))
    );

    // Nothing was created by the failed attempts; the right code still works.
    let orgs: i64 = sqlx::query_scalar("SELECT count(*) FROM organizations")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    assert_eq!(orgs, 0);
    let (status, body) = post_json(&app.anon(), url, complete_body(&t, &code)).await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test]
async fn single_mode_has_no_public_org_registration() {
    let app = TestApp::spawn().await;
    let (status, body) = post_json(
        &app.anon(),
        app.url("/api/v1/orgs"),
        json!({ "ticket": "x", "name": "Other", "timezone": "Asia/Kolkata", "leave_year_start_month": 1, "full_name": "X" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (404, Some("NOT_FOUND"))
    );
    let (status, _) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/code"),
        json!({ "email": "ceo@newco.test", "purpose": "register_org" }),
    )
    .await;
    assert_eq!(status, 404);
}
