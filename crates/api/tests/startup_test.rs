//! T202 (start-up half) — the instance never serves a partly migrated database (FR-069).
mod common;

use common::*;
use leavo_db::testkit::TestDb;
use std::process::Command;
use std::time::Duration;

#[tokio::test]
async fn readyz_fails_while_a_migration_is_pending() {
    let app = TestApp::spawn().await;
    let res = reqwest::get(app.url("/readyz")).await.unwrap();
    assert_eq!(res.status(), 200);
    sqlx::query(
        "DELETE FROM _sqlx_migrations WHERE version = (SELECT max(version) FROM _sqlx_migrations)",
    )
    .execute(&app.db.owner)
    .await
    .unwrap();
    let res = reqwest::get(app.url("/readyz")).await.unwrap();
    assert_eq!(res.status(), 503);
}

#[tokio::test]
async fn serve_exits_without_binding_when_migrations_fail() {
    let db = TestDb::new().await;
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
         VALUES (99999, 'from a newer release', true, '\\x00', 0)",
    )
    .execute(&db.owner)
    .await
    .unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let bind = format!("127.0.0.1:{port}");
    let out = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_leavo"))
            .arg("serve")
            .env_clear()
            .env("DATABASE_URL", &db.url)
            .env("LEAVO_BASE_URL", "http://leavo.test")
            .env("LEAVO_SECRET_KEY", SECRET)
            .env("SMTP_URL", "smtp://localhost:1025")
            .env("EMAIL_FROM", "Leavo <leave@leavo.test>")
            .env("LEAVO_UPDATE_CHECK", "false")
            .env("LEAVO_BIND", &bind)
            .output()
            .unwrap()
    });
    let out = tokio::time::timeout(Duration::from_secs(60), out)
        .await
        .expect("serve must exit, not keep running")
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("migration 99999"), "{stderr}");
    assert!(stderr.contains("Downgrades are not supported"), "{stderr}");
    assert!(
        !stderr.contains("listening")
            && !String::from_utf8_lossy(&out.stdout).contains("listening")
    );
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", port)).is_err(),
        "nothing bound the port"
    );
}
