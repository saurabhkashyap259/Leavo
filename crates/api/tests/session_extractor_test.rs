//! T023 — session extractor: expiry, sliding window, 90-day cap, revocation, deactivation
//! (FR-005, FR-006).
mod common;
use chrono::Duration;
use common::factories::{create_org, create_session, create_user, NewUser};
use common::TestApp;
use leavo_core::clock::Clock;

async fn me_status(client: &reqwest::Client, app: &TestApp) -> u16 {
    client
        .get(app.url("/api/v1/me"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

async fn expires_at(app: &TestApp, user: uuid::Uuid) -> chrono::DateTime<chrono::Utc> {
    sqlx::query_scalar("SELECT expires_at FROM sessions WHERE user_id = $1")
        .bind(user)
        .fetch_one(&app.db.owner)
        .await
        .unwrap()
}

#[tokio::test]
async fn missing_or_unknown_cookie_is_unauthenticated() {
    let app = TestApp::spawn().await;
    let res = app.anon().get(app.url("/api/v1/me")).send().await.unwrap();
    assert_eq!(res.status(), 401);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "UNAUTHENTICATED");
    assert_eq!(
        me_status(&app.with_token("not-a-real-token"), &app).await,
        401
    );
}

#[tokio::test]
async fn valid_session_identifies_the_user() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let user = create_user(
        &app,
        &org,
        NewUser::employee("asha@acme.test").role("approver"),
    )
    .await;
    let res = app
        .login_as(&user)
        .await
        .get(app.url("/api/v1/me"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["id"], user.id.to_string());
    assert_eq!(body["role"], "approver");
    assert_eq!(body["org"]["id"], org.id.to_string());
}

#[tokio::test]
async fn session_expires_after_seven_days_without_use() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let user = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let client = app.login_as(&user).await;
    app.clock.advance(Duration::days(7) + Duration::seconds(1));
    assert_eq!(me_status(&client, &app).await, 401);
}

#[tokio::test]
async fn use_in_the_last_day_slides_the_expiry() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let user = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let client = app.login_as(&user).await;
    let before = expires_at(&app, user.id).await;

    app.clock.advance(Duration::days(3)); // not yet in the last 24 h: unchanged
    assert_eq!(me_status(&client, &app).await, 200);
    assert_eq!(expires_at(&app, user.id).await, before);

    app.clock.advance(Duration::days(3) + Duration::hours(12)); // inside last 24 h
    assert_eq!(me_status(&client, &app).await, 200);
    assert_eq!(
        expires_at(&app, user.id).await,
        app.clock.now() + Duration::days(7)
    );
}

#[tokio::test]
async fn sliding_never_passes_the_90_day_cap() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let user = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let start = app.clock.now();
    let token = create_session(&app, &user, true).await; // keep signed in: 30-day lifetime
    let client = app.with_token(&token);
    for _ in 0..4 {
        app.clock.advance(Duration::days(29) + Duration::hours(12));
        if app.clock.now() < start + Duration::days(90) {
            assert_eq!(me_status(&client, &app).await, 200);
        }
    }
    assert!(expires_at(&app, user.id).await <= start + Duration::days(90));
    app.clock
        .set(start + Duration::days(90) + Duration::seconds(1));
    assert_eq!(me_status(&client, &app).await, 401);
}

#[tokio::test]
async fn revoked_session_is_rejected() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let user = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let client = app.login_as(&user).await;
    sqlx::query("UPDATE sessions SET revoked_at = now()")
        .execute(&app.db.owner)
        .await
        .unwrap();
    assert_eq!(me_status(&client, &app).await, 401);
}

#[tokio::test]
async fn deactivated_user_is_rejected() {
    let app = TestApp::spawn().await;
    let org = create_org(&app, "Acme", "acme.test").await;
    let user = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let client = app.login_as(&user).await;
    sqlx::query("UPDATE users SET status = 'deactivated' WHERE id = $1")
        .bind(user.id)
        .execute(&app.db.owner)
        .await
        .unwrap();
    assert_eq!(me_status(&client, &app).await, 401);
}
