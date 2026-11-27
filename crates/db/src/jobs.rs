//! Job queue / transactional outbox (FR-047, research R11).
//!
//! Jobs are inserted in the same transaction as the change that causes them, so a rolled-back
//! change never sends email. Workers claim jobs with `FOR UPDATE SKIP LOCKED`.
use chrono::{DateTime, Duration, Utc};
use sqlx::{FromRow, PgExecutor, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, serde::Serialize)]
#[sqlx(type_name = "job_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Dead,
}

#[derive(Debug, Clone)]
pub struct NewJob {
    pub kind: String,
    pub org_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub run_at: DateTime<Utc>,
    /// Unique key; a second enqueue with the same key is ignored (idempotent scheduling).
    pub dedupe_key: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
pub struct Job {
    pub id: Uuid,
    pub kind: String,
    pub org_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub attempts: i32,
    pub max_attempts: i32,
}

/// Insert a job. Returns `None` when `dedupe_key` already exists.
pub async fn enqueue<'e, E: PgExecutor<'e>>(ex: E, job: &NewJob) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar(
        "INSERT INTO jobs (id, kind, org_id, payload, run_at, dedupe_key)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (dedupe_key) DO NOTHING
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(&job.kind)
    .bind(job.org_id)
    .bind(&job.payload)
    .bind(job.run_at)
    .bind(&job.dedupe_key)
    .fetch_optional(ex)
    .await
}

/// Claim up to `limit` due jobs for `worker`. Concurrent claimers never get the same job.
pub async fn claim(
    pool: &PgPool,
    worker: &str,
    now: DateTime<Utc>,
    limit: i64,
) -> sqlx::Result<Vec<Job>> {
    sqlx::query_as::<_, Job>(
        "UPDATE jobs SET status = 'running', locked_by = $1, locked_at = $2, attempts = attempts + 1
         WHERE id IN (
             SELECT id FROM jobs
             WHERE status = 'queued' AND run_at <= $2
             ORDER BY run_at, id
             LIMIT $3
             FOR UPDATE SKIP LOCKED
         )
         RETURNING id, kind, org_id, payload, attempts, max_attempts",
    )
    .bind(worker)
    .bind(now)
    .bind(limit)
    .fetch_all(pool)
    .await
}

pub async fn complete(pool: &PgPool, id: Uuid, now: DateTime<Utc>) -> sqlx::Result<()> {
    sqlx::query("UPDATE jobs SET status = 'done', finished_at = $2, locked_by = NULL, last_error = NULL WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

/// Retry delay after the n-th failed attempt: 30 s, 60 s, 120 s … capped at one hour.
pub fn backoff(attempts: i32) -> Duration {
    let exp = attempts.clamp(1, 20) - 1;
    Duration::seconds((30_i64 << exp).min(3600))
}

/// Record a failure: requeue with backoff, or mark dead after `max_attempts`.
pub async fn fail(
    pool: &PgPool,
    job: &Job,
    error: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<JobStatus> {
    let dead = job.attempts >= job.max_attempts;
    let status = if dead {
        JobStatus::Dead
    } else {
        JobStatus::Queued
    };
    sqlx::query(
        "UPDATE jobs SET status = $2, run_at = $3, last_error = $4, locked_by = NULL,
                         finished_at = CASE WHEN $2 = 'dead'::job_status THEN $5 ELSE NULL END
         WHERE id = $1",
    )
    .bind(job.id)
    .bind(status)
    .bind(now + backoff(job.attempts))
    .bind(truncate(error, 2000))
    .bind(now)
    .execute(pool)
    .await?;
    Ok(status)
}

fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Counts for the Instance page.
pub async fn counts(pool: &PgPool, org: Option<Uuid>) -> sqlx::Result<(i64, i64)> {
    sqlx::query_as(
        "SELECT count(*) FILTER (WHERE status = 'queued'), count(*) FILTER (WHERE status = 'dead')
         FROM jobs WHERE $1::uuid IS NULL OR org_id = $1",
    )
    .bind(org)
    .fetch_one(pool)
    .await
}
