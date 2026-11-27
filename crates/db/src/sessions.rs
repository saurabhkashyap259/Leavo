//! Sessions (not RLS-scoped: looked up by the SHA-256 of an unguessable token).
use chrono::{DateTime, Utc};
use leavo_core::tokens::session_expiry;
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NewSession {
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub token_hash: Vec<u8>,
    pub keep_signed_in: bool,
    pub user_agent: Option<String>,
    pub ip: Option<std::net::IpAddr>,
}

#[derive(Debug, Clone, FromRow)]
pub struct SessionRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub keep_signed_in: bool,
    pub expires_at: DateTime<Utc>,
    pub absolute_expires_at: DateTime<Utc>,
}

pub async fn create<'e, E: PgExecutor<'e>>(
    ex: E,
    s: &NewSession,
    now: DateTime<Utc>,
) -> sqlx::Result<SessionRow> {
    let (expires_at, absolute) = session_expiry(s.keep_signed_in, now);
    sqlx::query_as(
        "INSERT INTO sessions (id, user_id, org_id, token_hash, keep_signed_in, expires_at,
                               absolute_expires_at, user_agent, ip, created_at, last_seen_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::inet, $10, $10)
         RETURNING id, user_id, org_id, keep_signed_in, expires_at, absolute_expires_at",
    )
    .bind(Uuid::now_v7())
    .bind(s.user_id)
    .bind(s.org_id)
    .bind(&s.token_hash)
    .bind(s.keep_signed_in)
    .bind(expires_at)
    .bind(absolute)
    .bind(&s.user_agent)
    .bind(s.ip.map(|ip| ip.to_string()))
    .bind(now)
    .fetch_one(ex)
    .await
}

/// The live session for this token hash at `now`, if any.
pub async fn find_active<'e, E: PgExecutor<'e>>(
    ex: E,
    token_hash: &[u8],
    now: DateTime<Utc>,
) -> sqlx::Result<Option<SessionRow>> {
    sqlx::query_as(
        "SELECT id, user_id, org_id, keep_signed_in, expires_at, absolute_expires_at
         FROM sessions
         WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > $2 AND absolute_expires_at > $2",
    )
    .bind(token_hash)
    .bind(now)
    .fetch_optional(ex)
    .await
}

pub async fn extend<'e, E: PgExecutor<'e>>(
    ex: E,
    id: Uuid,
    expires_at: DateTime<Utc>,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE sessions SET expires_at = $2, last_seen_at = $3 WHERE id = $1")
        .bind(id)
        .bind(expires_at)
        .bind(now)
        .execute(ex)
        .await?;
    Ok(())
}

pub async fn revoke<'e, E: PgExecutor<'e>>(
    ex: E,
    id: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE sessions SET revoked_at = $2 WHERE id = $1 AND revoked_at IS NULL")
        .bind(id)
        .bind(now)
        .execute(ex)
        .await?;
    Ok(())
}

/// "Sign out everywhere" and deactivation (FR-005, FR-006).
pub async fn revoke_all_for_user<'e, E: PgExecutor<'e>>(
    ex: E,
    user: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<u64> {
    Ok(
        sqlx::query(
            "UPDATE sessions SET revoked_at = $2 WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(user)
        .bind(now)
        .execute(ex)
        .await?
        .rows_affected(),
    )
}

/// "Sign out everyone" for an organization (FR-063).
pub async fn revoke_all_for_org<'e, E: PgExecutor<'e>>(
    ex: E,
    org: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<u64> {
    Ok(
        sqlx::query("UPDATE sessions SET revoked_at = $2 WHERE org_id = $1 AND revoked_at IS NULL")
            .bind(org)
            .bind(now)
            .execute(ex)
            .await?
            .rows_affected(),
    )
}
