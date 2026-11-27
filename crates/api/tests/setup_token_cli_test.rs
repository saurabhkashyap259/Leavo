//! T078 — `leavo setup-token` regenerates the token before setup and refuses afterwards.
mod common;

use common::*;
use serde_json::json;
use std::process::Command;

fn setup_token_cli(db_url: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_leavo"))
        .arg("setup-token")
        .env_clear()
        .env("DATABASE_URL", db_url)
        .env("LEAVO_BASE_URL", "http://leavo.test")
        .env("LEAVO_SECRET_KEY", SECRET)
        .env("SMTP_URL", "smtp://localhost:1025")
        .env("EMAIL_FROM", "leave@leavo.test")
        .output()
        .unwrap()
}

#[tokio::test]
async fn prints_a_working_token_until_setup_completes() {
    let app = TestApp::spawn().await;
    let out = setup_token_cli(&app.db.url);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(token.len(), 32, "{token:?}");

    let (status, _) = post_json(
        &app.anon(),
        app.url("/api/v1/setup/verify-token"),
        json!({ "setup_token": token }),
    )
    .await;
    assert_eq!(status, 204);

    // A second run replaces the first token.
    let newer = String::from_utf8_lossy(&setup_token_cli(&app.db.url).stdout)
        .trim()
        .to_string();
    assert_ne!(newer, token);
    let (status, _) = post_json(
        &app.anon(),
        app.url("/api/v1/setup/verify-token"),
        json!({ "setup_token": token }),
    )
    .await;
    assert_eq!(status, 403);

    common::factories::create_org(&app, "Acme", "acme.test").await;
    let out = setup_token_cli(&app.db.url);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("already complete"));
}
