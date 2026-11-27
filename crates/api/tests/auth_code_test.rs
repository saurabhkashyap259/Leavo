//! T055 — POST /auth/code (FR-001–FR-004).
mod common;
use chrono::Duration;
use common::factories::{create_org, create_user, NewUser};
use common::{codes_sent_to, post_json, TestApp};
use serde_json::json;

async fn request_code(app: &TestApp, email: &str, purpose: &str) -> (u16, serde_json::Value) {
    post_json(
        &app.anon(),
        app.url("/api/v1/auth/code"),
        json!({ "email": email, "purpose": purpose, "full_name": "Asha Rao" }),
    )
    .await
}

#[tokio::test]
async fn login_response_is_identical_for_active_unknown_and_deactivated() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(&app, &org, NewUser::employee("active@acme.test")).await;
    create_user(
        &app,
        &org,
        NewUser::employee("gone@acme.test").status("deactivated"),
    )
    .await;

    let a = request_code(&app, "active@acme.test", "login").await;
    let u = request_code(&app, "nobody@acme.test", "login").await;
    let d = request_code(&app, "gone@acme.test", "login").await;
    assert_eq!(a, (202, json!({"status": "sent"})));
    assert_eq!(u, a);
    assert_eq!(d, a);

    assert_eq!(codes_sent_to(&app, "active@acme.test").await.len(), 1);
    assert!(codes_sent_to(&app, "nobody@acme.test").await.is_empty());
    assert!(codes_sent_to(&app, "gone@acme.test").await.is_empty());
}

#[tokio::test]
async fn code_email_has_the_code_in_the_subject() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    request_code(&app, "asha@acme.test", "login").await;
    app.run_jobs().await;
    let mail = app.email.sent().pop().unwrap();
    let code = mail.subject.split_whitespace().next().unwrap().to_string();
    assert_eq!(mail.subject, format!("{code} is your Leavo code"));
    assert!(mail.text.contains(&code));
    assert!(mail.html.contains(&code));
}

#[tokio::test]
async fn signup_with_unregistered_domain_is_refused() {
    let app = TestApp::spawn().await;
    create_org(&app, "Acme", "acme.test").await;
    let (status, body) = request_code(&app, "asha@gmail.com", "signup").await;
    assert_eq!(status, 422);
    assert_eq!(body["error"]["code"], "DOMAIN_NOT_REGISTERED");
    assert_eq!(
        body["error"]["message"],
        "Your company isn't on Leavo yet. Ask your HR to register."
    );
}

#[tokio::test]
async fn signup_requires_a_name() {
    let app = TestApp::spawn().await;
    create_org(&app, "Acme", "acme.test").await;
    let (status, body) = post_json(
        &app.anon(),
        app.url("/api/v1/auth/code"),
        json!({"email": "new@acme.test", "purpose": "signup"}),
    )
    .await;
    assert_eq!(status, 422);
    assert_eq!(body["error"]["code"], "VALIDATION");
}

#[tokio::test]
async fn signup_for_existing_account_sends_a_login_code_instead() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let (status, body) = request_code(&app, "asha@acme.test", "signup").await;
    assert_eq!((status, body), (202, json!({"status": "sent"})));
    assert_eq!(codes_sent_to(&app, "asha@acme.test").await.len(), 1);
    let purpose: String =
        sqlx::query_scalar("SELECT purpose::text FROM login_codes WHERE email = 'asha@acme.test'")
            .fetch_one(&app.db.owner)
            .await
            .unwrap();
    assert_eq!(purpose, "login");
}

#[tokio::test]
async fn invalid_email_is_a_validation_error() {
    let app = TestApp::spawn().await;
    let (status, body) = request_code(&app, "not-an-email", "login").await;
    assert_eq!(status, 422);
    assert_eq!(body["error"]["code"], "VALIDATION");
}

#[tokio::test]
async fn one_code_per_30_seconds_per_email() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    assert_eq!(request_code(&app, "asha@acme.test", "login").await.0, 202);
    app.clock.advance(Duration::seconds(10));
    let res = app
        .anon()
        .post(app.url("/api/v1/auth/code"))
        .json(&json!({"email": "asha@acme.test", "purpose": "login"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 429);
    assert_eq!(res.headers()["retry-after"], "20");
    assert_eq!(
        res.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        "RATE_LIMITED"
    );
    app.clock.advance(Duration::seconds(20));
    assert_eq!(request_code(&app, "asha@acme.test", "login").await.0, 202);
}

#[tokio::test]
async fn five_codes_per_hour_per_email_known_or_not() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    for email in ["asha@acme.test", "ghost@acme.test"] {
        for _ in 0..5 {
            assert_eq!(request_code(&app, email, "login").await.0, 202);
            app.clock.advance(Duration::seconds(31));
        }
        assert_eq!(
            request_code(&app, email, "login").await.0,
            429,
            "{email}: same limit whether or not it exists"
        );
    }
    app.clock.advance(Duration::hours(1));
    assert_eq!(request_code(&app, "asha@acme.test", "login").await.0, 202);
}

#[tokio::test]
async fn twenty_code_requests_per_hour_per_ip() {
    let app = TestApp::spawn().await;
    for i in 0..20 {
        assert_eq!(
            request_code(&app, &format!("user{i}@acme.test"), "login")
                .await
                .0,
            202
        );
    }
    assert_eq!(request_code(&app, "user21@acme.test", "login").await.0, 429);
}

#[tokio::test]
async fn new_code_invalidates_the_previous_one() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    request_code(&app, "asha@acme.test", "login").await;
    app.clock.advance(Duration::seconds(31));
    request_code(&app, "asha@acme.test", "login").await;
    let codes = codes_sent_to(&app, "asha@acme.test").await;
    assert_eq!(codes.len(), 2);
    let old = post_json(
        &app.anon(),
        app.url("/api/v1/auth/verify"),
        json!({"email": "asha@acme.test", "code": codes[0], "purpose": "login"}),
    )
    .await;
    assert_eq!(old.0, 422);
    let new = post_json(
        &app.anon(),
        app.url("/api/v1/auth/verify"),
        json!({"email": "asha@acme.test", "code": codes[1], "purpose": "login"}),
    )
    .await;
    assert_eq!(new.0, 200);
}

#[tokio::test]
async fn codes_are_stored_hashed_never_in_plain_text() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    request_code(&app, "asha@acme.test", "login").await;
    let code = codes_sent_to(&app, "asha@acme.test").await.pop().unwrap();
    let hash: Vec<u8> = sqlx::query_scalar("SELECT code_hash FROM login_codes")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    assert_eq!(hash.len(), 32);
    assert!(!String::from_utf8_lossy(&hash).contains(&code));
}
