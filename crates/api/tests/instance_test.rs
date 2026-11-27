//! T205 — the Instance page (FR-051): version, update check, email health, storage, migrations
//! and job figures; a test email. In multi-organization mode, instance-wide details are hidden
//! and figures cover only the caller's organization.
mod common;

use common::factories::*;
use common::*;
use serde_json::{json, Value};
use uuid::Uuid;

async fn add_job(
    app: &TestApp,
    org: Option<Uuid>,
    kind: &str,
    status: &str,
    error: Option<&str>,
    age_minutes: i64,
) {
    sqlx::query(
        "INSERT INTO jobs (id, kind, org_id, status, last_error, run_at, created_at, finished_at)
         VALUES ($1, $2, $3, $4::job_status, $5, $6, $6,
                 CASE WHEN $4 IN ('done', 'dead') THEN $6 END)",
    )
    .bind(Uuid::now_v7())
    .bind(kind)
    .bind(org)
    .bind(status)
    .bind(error)
    .bind(t0() - chrono::Duration::minutes(age_minutes))
    .execute(&app.db.owner)
    .await
    .unwrap();
}

async fn instance(app: &TestApp, client: &reqwest::Client) -> Value {
    let (status, out) = get_json(client, app.url("/api/v1/admin/instance")).await;
    assert_eq!(status, 200, "{out}");
    out
}

#[tokio::test]
async fn shows_the_health_of_the_instance() {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    add_job(&app, Some(org.id), "send_email", "done", None, 30).await;
    add_job(
        &app,
        Some(org.id),
        "send_email",
        "dead",
        Some("550 mailbox unavailable"),
        20,
    )
    .await;
    add_job(&app, Some(org.id), "send_email", "queued", None, 12).await;
    add_job(&app, None, "approval_reminder", "dead", Some("boom"), 5).await;
    sqlx::query(
        "UPDATE instance_settings SET latest_version_seen = '99.0.0', last_update_check_at = $1",
    )
    .bind(t0())
    .execute(&app.db.owner)
    .await
    .unwrap();

    let out = instance(&app, &admin).await;
    assert_eq!(out["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(out["mode"], "single");
    assert_eq!(out["update_check"]["enabled"], false);
    assert_eq!(out["update_check"]["latest_version"], "99.0.0");
    assert_eq!(out["update_check"]["update_available"], true);
    assert_eq!(out["email"]["sent_24h"], 1);
    assert_eq!(out["email"]["failed_24h"], 1);
    assert_eq!(out["email"]["last_error"], "550 mailbox unavailable");
    assert_eq!(out["storage"], "memory");
    let pending = out["migrations"]["pending"].as_u64().unwrap();
    assert_eq!(pending, 0);
    assert!(out["migrations"]["applied"].as_u64().unwrap() >= 9);
    assert_eq!(out["jobs"]["queued"], 1);
    assert_eq!(out["jobs"]["dead"], 2);
    assert_eq!(out["jobs"]["oldest_queued_seconds"], 12 * 60);

    let emp = create_user(&app, &org, NewUser::employee("asha@acme.test")).await;
    let (status, _) = get_json(&app.login_as(&emp).await, app.url("/api/v1/admin/instance")).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn a_test_email_goes_to_the_admin() {
    let app = TestApp::spawn().await;
    let (_, _, admin) = org_with_admin(&app, "acme.test").await;
    app.email.take();
    let (status, out) = post_json(
        &admin,
        app.url("/api/v1/admin/instance/test-email"),
        json!({}),
    )
    .await;
    assert_eq!(status, 202, "{out}");
    assert_eq!(out["to"], "admin@acme.test");
    app.run_jobs().await;
    let sent = app.email.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "admin@acme.test");
}

#[tokio::test]
async fn multi_mode_hides_instance_details_and_scopes_figures() {
    let app = TestApp::spawn_with(&[("LEAVO_MODE", "multi")]).await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let (other, _, _) = org_with_admin(&app, "other.test").await;
    add_job(&app, Some(org.id), "send_email", "done", None, 30).await;
    add_job(
        &app,
        Some(other.id),
        "send_email",
        "dead",
        Some("theirs"),
        20,
    )
    .await;
    add_job(&app, Some(other.id), "send_email", "queued", None, 10).await;
    add_job(
        &app,
        None,
        "approval_reminder",
        "dead",
        Some("instance-wide"),
        5,
    )
    .await;

    let out = instance(&app, &admin).await;
    assert_eq!(out["mode"], "multi");
    assert!(out.get("storage").is_none(), "{out}");
    assert!(out.get("migrations").is_none(), "{out}");
    assert_eq!(out["email"]["sent_24h"], 1);
    assert_eq!(out["email"]["failed_24h"], 0);
    assert!(out["email"]["last_error"].is_null());
    assert_eq!(out["jobs"]["queued"], 0);
    assert_eq!(out["jobs"]["dead"], 0);
}
