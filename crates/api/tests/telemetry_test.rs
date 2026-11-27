//! T206 — no telemetry and an opt-out update check (FR-052), and Prometheus metrics with an
//! optional bearer token (FR-065, contracts/configuration.md).
mod common;

use common::*;
use std::sync::atomic::Ordering;

async fn update_jobs(app: &TestApp) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'update_check'")
        .fetch_one(&app.db.owner)
        .await
        .unwrap()
}

#[tokio::test]
async fn the_update_check_never_runs_when_disabled() {
    let app = TestApp::spawn().await; // LEAVO_UPDATE_CHECK=false
    org_with_admin(&app, "acme.test").await;
    app.tick().await;
    assert_eq!(update_jobs(&app).await, 0);
    assert_eq!(
        app.releases.calls.load(Ordering::SeqCst),
        0,
        "no outbound request"
    );
}

#[tokio::test]
async fn the_update_check_runs_daily_when_enabled() {
    let app = TestApp::spawn_with(&[("LEAVO_UPDATE_CHECK", "true")]).await;
    let (_, _, admin) = org_with_admin(&app, "acme.test").await;
    app.tick().await;
    app.tick().await;
    assert_eq!(update_jobs(&app).await, 1, "once a day");
    assert_eq!(app.releases.calls.load(Ordering::SeqCst), 1);
    let (_, out) = get_json(&admin, app.url("/api/v1/admin/instance")).await;
    assert_eq!(out["update_check"]["enabled"], true);
    assert_eq!(out["update_check"]["latest_version"], "99.1.0");
    assert_eq!(out["update_check"]["update_available"], true);
    app.clock.set(t0() + chrono::Duration::days(1));
    app.tick().await;
    assert_eq!(update_jobs(&app).await, 2);
}

const METRICS: [&str; 8] = [
    "leavo_http_requests_total",
    "leavo_http_request_duration_seconds_bucket",
    "leavo_login_codes_sent_total",
    "leavo_login_codes_verified_total",
    "leavo_email_send_failures_total",
    "leavo_jobs_queued",
    "leavo_job_oldest_queued_seconds",
    "leavo_migrations_pending",
];

#[tokio::test]
async fn metrics_are_open_without_a_token() {
    let app = TestApp::spawn().await;
    // Some traffic to count.
    reqwest::get(app.url("/healthz")).await.unwrap();
    let res = reqwest::get(app.url("/metrics")).await.unwrap();
    assert_eq!(res.status(), 200);
    assert!(res.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
    let body = res.text().await.unwrap();
    for m in METRICS {
        assert!(body.contains(m), "missing {m} in:\n{body}");
    }
    assert!(body.contains("leavo_migrations_pending 0"), "{body}");
    assert!(
        body.contains("leavo_http_requests_total{status=\"2xx\"}"),
        "{body}"
    );
}

#[tokio::test]
async fn metrics_need_the_bearer_token_when_set() {
    let app = TestApp::spawn_with(&[("LEAVO_METRICS_TOKEN", "s3cret")]).await;
    let client = reqwest::Client::new();
    let res = client.get(app.url("/metrics")).send().await.unwrap();
    assert_eq!(res.status(), 401);
    let res = client
        .get(app.url("/metrics"))
        .bearer_auth("wrong")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);
    let res = client
        .get(app.url("/metrics"))
        .bearer_auth("s3cret")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
}
