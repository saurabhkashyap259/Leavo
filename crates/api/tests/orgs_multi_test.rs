//! T068 — public organization registration in multi-org mode (FR-008, FR-009).
mod common;

use common::factories::*;
use common::*;
use serde_json::{json, Value};

const MULTI: &[(&str, &str)] = &[("LEAVO_MODE", "multi")];

async fn ticket_for(app: &TestApp, email: &str) -> String {
    let (status, body) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/code"),
        json!({ "email": email, "purpose": "register_org" }),
    )
    .await;
    assert_eq!(status, 202, "{body}");
    let code = codes_sent_to(app, email).await.pop().unwrap();
    let (status, body) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/verify"),
        json!({ "email": email, "code": code, "purpose": "register_org" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["verified"], true);
    body["ticket"].as_str().unwrap().to_string()
}

fn org_body(ticket: &str) -> Value {
    json!({
        "ticket": ticket, "name": "NewCo", "timezone": "Europe/London",
        "leave_year_start_month": 4, "full_name": "Chen Li"
    })
}

#[tokio::test]
async fn verified_founder_creates_an_org_and_becomes_its_admin() {
    let app = TestApp::spawn_with(MULTI).await;
    let (status, _) = get_json(&app.anon(), app.url("/api/v1/setup/status")).await;
    assert_eq!(status, 200);
    let ticket = ticket_for(&app, "chen@newco.test").await;

    let res = app
        .anon()
        .post(app.url("/api/v1/orgs"))
        .json(&org_body(&ticket))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 201);
    let sid = session_cookie(&res).expect("signed in");
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["org"]["name"], "NewCo");
    assert_eq!(body["org"]["leave_year_start_month"], 4);
    assert_eq!(body["user"]["role"], "admin");

    let (_, me) = get_json(&app.with_token(&sid), app.url("/api/v1/me")).await;
    assert_eq!(me["role"], "admin");
    assert_eq!(me["full_name"], "Chen Li");
    assert_eq!(me["org"]["timezone"], "Europe/London");

    let domain: String = sqlx::query_scalar("SELECT domain::text FROM org_domains")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    assert_eq!(domain, "newco.test");
    let types: i64 = sqlx::query_scalar("SELECT count(*) FROM leave_types")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    assert_eq!(types, 6, "default types are seeded");

    // Colleagues can now sign up with the new domain.
    let (status, _) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/code"),
        json!({ "email": "ravi@newco.test", "purpose": "signup", "full_name": "Ravi" }),
    )
    .await;
    assert_eq!(status, 202);
}

#[tokio::test]
async fn register_code_refuses_public_and_taken_domains() {
    let app = TestApp::spawn_with(MULTI).await;
    create_org(&app, "Acme", "acme.test").await;
    let url = app.url("/api/v1/auth/code");

    let (status, body) = post_json(
        &app.anon(),
        url.clone(),
        json!({ "email": "me@gmail.com", "purpose": "register_org" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (422, Some("PUBLIC_DOMAIN"))
    );

    let (status, body) = post_json(
        &app.anon(),
        url,
        json!({ "email": "boss@acme.test", "purpose": "register_org" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("DOMAIN_TAKEN"))
    );
}

#[tokio::test]
async fn ticket_is_required_and_domain_is_rechecked_on_create() {
    let app = TestApp::spawn_with(MULTI).await;
    let (status, body) =
        post_json(&app.anon(), app.url("/api/v1/orgs"), org_body("forged.1.x")).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (422, Some("CODE_INVALID"))
    );

    let ticket = ticket_for(&app, "chen@newco.test").await;
    // Someone else registered the domain after the ticket was issued.
    create_org(&app, "Squatter", "newco.test").await;
    let (status, body) = post_json(&app.anon(), app.url("/api/v1/orgs"), org_body(&ticket)).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (409, Some("DOMAIN_TAKEN"))
    );
}

#[tokio::test]
async fn org_fields_are_validated() {
    let app = TestApp::spawn_with(MULTI).await;
    let ticket = ticket_for(&app, "chen@newco.test").await;
    let mut body = org_body(&ticket);
    body["leave_year_start_month"] = json!(13);
    let (status, res) = post_json(&app.anon(), app.url("/api/v1/orgs"), body).await;
    assert_eq!(
        (status, res["error"]["details"]["field"].as_str()),
        (422, Some("leave_year_start_month"))
    );
}
