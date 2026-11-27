//! Pre-tenant authentication queries (T060).
//!
//! Sign-in happens before we know the user's organization, so these queries cannot run inside
//! an org-scoped transaction. They touch only:
//! * `login_codes` and `rate_limits`, which are not tenant data and are keyed by email;
//! * two `SECURITY DEFINER` functions (`auth_find_user`, `auth_find_org_by_domain`) that return
//!   the minimum needed (ids, role, status) — never names or leave data.
//!
//! Everything after the tenant is known (creating the user, ledger rows, audit) runs after
//! `tx::set_org` in the same transaction, under Row-Level Security.
use chrono::{DateTime, Duration, Utc};
use leavo_core::otp::LOCK_MINUTES;
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct FoundUser {
    pub id: Uuid,
    pub org_id: Uuid,
    pub role: String,
    pub status: String,
}

pub async fn find_user(conn: &mut PgConnection, email: &str) -> sqlx::Result<Option<FoundUser>> {
    sqlx::query_as("SELECT id, org_id, role::text AS role, status::text AS status FROM auth_find_user($1::citext)")
        .bind(email)
        .fetch_optional(conn)
        .await
}

pub async fn find_org_by_domain(
    conn: &mut PgConnection,
    domain: &str,
) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT auth_find_org_by_domain($1::citext)")
        .bind(domain)
        .fetch_one(conn)
        .await
}

#[derive(Debug, Clone, FromRow)]
pub struct CodeRow {
    pub id: Uuid,
    pub code_hash: Vec<u8>,
    pub payload: serde_json::Value,
    pub attempts: i16,
}

/// Insert a new code and invalidate every earlier unused code for the email (FR-002).
pub async fn replace_code(
    conn: &mut PgConnection,
    email: &str,
    purpose: &str,
    code_hash: &[u8],
    payload: &serde_json::Value,
    expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> sqlx::Result<Uuid> {
    sqlx::query("UPDATE login_codes SET consumed_at = $2 WHERE email = $1 AND consumed_at IS NULL")
        .bind(email)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    sqlx::query_scalar(
        "INSERT INTO login_codes (id, email, purpose, code_hash, payload, expires_at, created_at)
         VALUES ($1, $2, $3::code_purpose, $4, $5, $6, $7) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(email)
    .bind(purpose)
    .bind(code_hash)
    .bind(payload)
    .bind(expires_at)
    .bind(now)
    .fetch_one(conn)
    .await
}

/// Newest unused, unexpired code for this email and purpose, locked for update.
pub async fn latest_live_code(
    conn: &mut PgConnection,
    email: &str,
    purpose: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<Option<CodeRow>> {
    sqlx::query_as(
        "SELECT id, code_hash, payload, attempts FROM login_codes
         WHERE email = $1 AND purpose = $2::code_purpose AND consumed_at IS NULL AND expires_at > $3
         ORDER BY created_at DESC LIMIT 1
         FOR UPDATE",
    )
    .bind(email)
    .bind(purpose)
    .bind(now)
    .fetch_optional(conn)
    .await
}

/// Count one verification attempt; returns the new total.
pub async fn record_attempt(conn: &mut PgConnection, code_id: Uuid) -> sqlx::Result<i16> {
    sqlx::query_scalar(
        "UPDATE login_codes SET attempts = attempts + 1 WHERE id = $1 RETURNING attempts",
    )
    .bind(code_id)
    .fetch_one(conn)
    .await
}

pub async fn consume(
    conn: &mut PgConnection,
    code_id: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE login_codes SET consumed_at = $2 WHERE id = $1")
        .bind(code_id)
        .bind(now)
        .execute(conn)
        .await?;
    Ok(())
}

const LOCK_BUCKET: &str = "verify_lock";

/// End of the current sign-in lock for this email, if locked (FR-002: 15 minutes).
pub async fn locked_until(
    conn: &mut PgConnection,
    email: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<Option<DateTime<Utc>>> {
    let lock = Duration::minutes(LOCK_MINUTES);
    let started: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT max(window_start) FROM rate_limits WHERE bucket = $1 AND key = $2 AND window_start > $3",
    )
    .bind(LOCK_BUCKET)
    .bind(email)
    .bind(now - lock)
    .fetch_one(conn)
    .await?;
    Ok(started.map(|s| s + lock))
}

pub async fn lock(conn: &mut PgConnection, email: &str, now: DateTime<Utc>) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO rate_limits (bucket, key, window_start, count) VALUES ($1, $2, $3, 1)
         ON CONFLICT (bucket, key, window_start) DO NOTHING",
    )
    .bind(LOCK_BUCKET)
    .bind(email)
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}
