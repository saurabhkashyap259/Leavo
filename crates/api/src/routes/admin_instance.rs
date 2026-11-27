//! Instance page (T211, FR-051): version and update check, email health, storage, migrations
//! and the job queue, plus a test email. In multi-organization mode (`LEAVO_MODE=multi`) an org
//! admin is not the operator: storage and migrations are left out and email and job figures
//! cover only their organization.
use crate::app::AppState;
use crate::auth::CurrentUser;
use crate::config::InstanceMode;
use crate::error::ApiResult;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use leavo_core::permissions::Action;
use leavo_db::outbox::{notify, OutgoingEmail};
use leavo_db::tx::begin_org_tx;
use serde::Serialize;
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct UpdateCheckStatus {
    /// `LEAVO_UPDATE_CHECK`.
    pub enabled: bool,
    pub latest_version: Option<String>,
    pub last_checked_at: Option<DateTime<Utc>>,
    /// A newer release than this one is published.
    pub update_available: bool,
}

#[derive(Serialize, ToSchema)]
pub struct EmailStatus {
    /// Delivered in the last 24 hours.
    pub sent_24h: i64,
    /// Given up after every retry in the last 24 hours.
    pub failed_24h: i64,
    pub last_error: Option<String>,
    pub last_error_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, ToSchema)]
pub struct JobStatus {
    /// Due and waiting to run.
    pub queued: i64,
    /// Failed for good.
    pub dead: i64,
    pub oldest_queued_seconds: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct MigrationStatus {
    pub applied: usize,
    pub pending: usize,
}

#[derive(Serialize, ToSchema)]
pub struct InstanceStatus {
    pub version: String,
    /// `single` or `multi`.
    pub mode: String,
    pub update_check: UpdateCheckStatus,
    pub email: EmailStatus,
    pub jobs: JobStatus,
    /// Single mode only: `local`, `s3` (or `memory` in tests).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<String>,
    /// Single mode only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub migrations: Option<MigrationStatus>,
}

/// `a > b` for dotted versions; anything unparseable compares as not newer.
fn newer(a: &str, b: &str) -> bool {
    let parse = |v: &str| -> Option<Vec<u64>> {
        v.split(['-', '+'])
            .next()?
            .split('.')
            .map(|p| p.parse().ok())
            .collect()
    };
    matches!((parse(a), parse(b)), (Some(x), Some(y)) if x > y)
}

#[utoipa::path(get, path = "/api/v1/admin/instance", tag = "admin",
    responses(
        (status = 200, description = "Instance health", body = InstanceStatus),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
    ))]
pub async fn status(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<Json<InstanceStatus>> {
    user.require(Action::ManagePolicy)?;
    let now = state.clock.now();
    let multi = state.config.mode == InstanceMode::Multi;
    // Job rows are instance-wide; in multi mode only the caller's org counts.
    let org: Option<Uuid> = multi.then_some(user.org_id);
    let (latest, checked): (Option<String>, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT latest_version_seen, last_update_check_at FROM instance_settings WHERE id = 1",
    )
    .fetch_one(&state.owner)
    .await?;
    let day_ago = now - Duration::hours(24);
    let (sent, failed): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'done'), count(*) FILTER (WHERE status = 'dead')
         FROM jobs WHERE kind = 'send_email' AND finished_at >= $1
           AND ($2::uuid IS NULL OR org_id = $2)",
    )
    .bind(day_ago)
    .bind(org)
    .fetch_one(&state.owner)
    .await?;
    let last_error: Option<(String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT last_error, COALESCE(finished_at, locked_at, run_at) FROM jobs
         WHERE kind = 'send_email' AND last_error IS NOT NULL
           AND ($1::uuid IS NULL OR org_id = $1)
         ORDER BY COALESCE(finished_at, locked_at, run_at) DESC LIMIT 1",
    )
    .bind(org)
    .fetch_optional(&state.owner)
    .await?;
    let (queued, dead, oldest): (i64, i64, Option<f64>) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'queued' AND run_at <= $1),
                count(*) FILTER (WHERE status = 'dead'),
                extract(epoch FROM ($1 - min(run_at) FILTER (WHERE status = 'queued' AND run_at <= $1)))::float8
         FROM jobs WHERE ($2::uuid IS NULL OR org_id = $2)",
    )
    .bind(now)
    .bind(org)
    .fetch_one(&state.owner)
    .await?;
    let migrations = if multi {
        None
    } else {
        let (applied, pending) = leavo_db::migrate::status(&state.owner).await?;
        Some(MigrationStatus { applied, pending })
    };
    let version = env!("CARGO_PKG_VERSION").to_string();
    Ok(Json(InstanceStatus {
        update_check: UpdateCheckStatus {
            enabled: state.config.update_check,
            update_available: latest.as_deref().is_some_and(|l| newer(l, &version)),
            latest_version: latest,
            last_checked_at: checked,
        },
        version,
        mode: if multi { "multi" } else { "single" }.into(),
        email: EmailStatus {
            sent_24h: sent,
            failed_24h: failed,
            last_error: last_error.as_ref().map(|e| e.0.clone()),
            last_error_at: last_error.map(|e| e.1),
        },
        jobs: JobStatus {
            queued,
            dead,
            oldest_queued_seconds: oldest.map(|s| s.max(0.0) as i64),
        },
        storage: (!multi).then(|| state.storage.kind().to_string()),
        migrations,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct TestEmailQueued {
    pub to: String,
}

#[utoipa::path(post, path = "/api/v1/admin/instance/test-email", tag = "admin",
    responses(
        (status = 202, description = "A test email is queued to the signed-in admin; its delivery shows in the email status", body = TestEmailQueued),
        (status = 403, description = "Not an admin", body = crate::openapi::ErrorBody),
    ))]
pub async fn test_email(
    State(state): State<AppState>,
    user: CurrentUser,
) -> ApiResult<(StatusCode, Json<TestEmailQueued>)> {
    user.require(Action::ManagePolicy)?;
    let now = state.clock.now();
    let mut tx = begin_org_tx(&state.db, user.org_id).await?;
    let (to, org_name): (String, String) = sqlx::query_as(
        "SELECT u.email::text, o.name FROM users u JOIN organizations o ON o.id = u.org_id
         WHERE u.id = $1",
    )
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await?;
    notify(
        &mut tx,
        &OutgoingEmail {
            org_id: Some(user.org_id),
            template: "test_email".into(),
            to: to.clone(),
            context: json!({ "org_name": org_name, "base_url": state.config.base_url }),
            attachments: vec![],
        },
        now,
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(TestEmailQueued { to })))
}
