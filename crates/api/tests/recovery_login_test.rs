//! T204 — `leavo recovery-login <email>` (FR-067): for an active Org Admin it prints a sign-in
//! link valid 15 minutes and usable once; using it starts a session and is recorded as
//! `auth.recovery_login`. Anyone else is refused.
mod common;

use chrono::{Duration, Utc};
use common::factories::*;
use common::*;
use std::process::{Command, Output};

async fn recovery_login(app: &TestApp, email: &str) -> Output {
    let (url, email) = (app.db.url.clone(), email.to_string());
    tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_leavo"))
            .args(["recovery-login", &email])
            .env_clear()
            .env("DATABASE_URL", url)
            .env("LEAVO_BASE_URL", "http://leavo.test")
            .env("LEAVO_SECRET_KEY", SECRET)
            .env("SMTP_URL", "smtp://localhost:1025")
            .env("EMAIL_FROM", "Leavo <leave@leavo.test>")
            .env("LEAVO_UPDATE_CHECK", "false")
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}

/// The printed link, pointed at the test server.
fn link(app: &TestApp, out: &Output) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let url = stdout
        .split_whitespace()
        .find(|w| w.starts_with("http://leavo.test/"))
        .unwrap_or_else(|| panic!("no link in: {stdout}"));
    app.url(url.trim_start_matches("http://leavo.test"))
}

async fn open(app: &TestApp, url: &str) -> reqwest::Response {
    app.anon().get(url).send().await.unwrap()
}

#[tokio::test]
async fn an_active_admin_gets_a_single_use_link() {
    let app = TestApp::spawn().await;
    app.clock.set(Utc::now());
    let (_, admin, _) = org_with_admin(&app, "acme.test").await;
    let out = recovery_login(&app, "admin@acme.test").await;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("15 minutes"));
    let url = link(&app, &out);

    let res = open(&app, &url).await;
    assert_eq!(res.status(), 303);
    assert_eq!(res.headers()["location"], "/admin");
    let token = session_cookie(&res).expect("a session cookie");
    let (status, me) = get_json(&app.with_token(&token), app.url("/api/v1/me")).await;
    assert_eq!(status, 200);
    assert_eq!(me["email"], "admin@acme.test");
    let logged: (i64, Option<uuid::Uuid>) = sqlx::query_as(
        "SELECT count(*), max(actor_id::text)::uuid FROM audit_log WHERE action = 'auth.recovery_login'",
    )
    .fetch_one(&app.db.owner)
    .await
    .unwrap();
    assert_eq!(logged, (1, Some(admin.id)));

    // Used once.
    let res = open(&app, &url).await;
    assert_eq!(res.status(), 303);
    assert_eq!(res.headers()["location"], "/login?link=used");
    assert!(session_cookie(&res).is_none());

    // Expires after 15 minutes.
    let out = recovery_login(&app, "admin@acme.test").await;
    let url = link(&app, &out);
    app.clock.set(Utc::now() + Duration::minutes(16));
    let res = open(&app, &url).await;
    assert_eq!(res.headers()["location"], "/login?link=expired");

    // A made-up link.
    let res = open(&app, &app.url("/api/v1/auth/recovery/nonsense")).await;
    assert_eq!(res.headers()["location"], "/login?link=invalid");
}

#[tokio::test]
async fn only_active_admins() {
    let app = TestApp::spawn().await;
    let (org, _, _) = org_with_admin(&app, "acme.test").await;
    create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    create_user(
        &app,
        &org,
        NewUser::employee("old@acme.test")
            .role("admin")
            .status("deactivated"),
    )
    .await;
    for email in ["asha@acme.test", "old@acme.test", "nobody@acme.test"] {
        let out = recovery_login(&app, email).await;
        assert!(!out.status.success(), "{email}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("not an active Org Admin"),
            "{email}"
        );
        assert!(out.stdout.is_empty());
    }
    let codes: i64 =
        sqlx::query_scalar("SELECT count(*) FROM login_codes WHERE purpose = 'recovery'")
            .fetch_one(&app.db.owner)
            .await
            .unwrap();
    assert_eq!(codes, 0);
}
