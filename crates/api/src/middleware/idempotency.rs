//! `Idempotency-Key` handling for create endpoints (FR-019, research R22).
//!
//! The handler claims the key inside its own transaction. The claim row is locked until that
//! transaction ends, so a concurrent retry with the same key waits, then replays the stored
//! response. A failed attempt rolls back and frees the key. Keys older than 24 h are ignored
//! (and deleted by the `cleanup` job).
use crate::error::{field_error, ApiError, ApiResult};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use leavo_core::error::ErrorCode;
use leavo_core::tokens::hash_token;
use sqlx::PgConnection;
use uuid::Uuid;

pub const HEADER: &str = "Idempotency-Key";
pub const TTL_HOURS: i64 = 24;

/// The request's `Idempotency-Key` (a UUID). Missing or malformed → `422 VALIDATION`.
pub struct IdempotencyKey(pub String);

impl<S: Send + Sync> FromRequestParts<S> for IdempotencyKey {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        parts
            .headers
            .get(HEADER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| Uuid::parse_str(v.trim()).ok())
            .map(|k| IdempotencyKey(k.to_string()))
            .ok_or_else(|| field_error(HEADER))
    }
}

pub enum Claim {
    /// First use: run the handler, then [`store`] its response.
    Fresh,
    /// Seen before with the same body: return this response unchanged.
    Replay(StatusCode, serde_json::Value),
}

/// Stable hash of the request body (serde_json sorts object keys).
pub fn body_hash(body: &serde_json::Value) -> [u8; 32] {
    hash_token(&body.to_string())
}

/// Claim `key` for `endpoint` in the current org-scoped transaction.
pub async fn claim(
    conn: &mut PgConnection,
    org: Uuid,
    user: Uuid,
    key: &IdempotencyKey,
    endpoint: &str,
    hash: &[u8; 32],
    now: DateTime<Utc>,
) -> ApiResult<Claim> {
    // An expired key is free again.
    sqlx::query("DELETE FROM idempotency_keys WHERE user_id = $1 AND key = $2 AND created_at < $3")
        .bind(user)
        .bind(&key.0)
        .bind(now - Duration::hours(TTL_HOURS))
        .execute(&mut *conn)
        .await?;
    let inserted = sqlx::query(
        "INSERT INTO idempotency_keys (org_id, user_id, key, endpoint, request_hash, status, response, created_at)
         VALUES ($1, $2, $3, $4, $5, 0, 'null', $6)
         ON CONFLICT (user_id, key) DO NOTHING",
    )
    .bind(org)
    .bind(user)
    .bind(&key.0)
    .bind(endpoint)
    .bind(hash.as_slice())
    .bind(now)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if inserted == 1 {
        return Ok(Claim::Fresh);
    }
    let (stored_endpoint, stored_hash, status, response): (
        String,
        Vec<u8>,
        i16,
        serde_json::Value,
    ) = sqlx::query_as(
        "SELECT endpoint, request_hash, status, response FROM idempotency_keys
             WHERE user_id = $1 AND key = $2",
    )
    .bind(user)
    .bind(&key.0)
    .fetch_one(&mut *conn)
    .await?;
    if stored_endpoint != endpoint || stored_hash != hash.as_slice() {
        return Err(ErrorCode::IdempotencyMismatch.into());
    }
    let status = StatusCode::from_u16(status as u16).unwrap_or(StatusCode::OK);
    Ok(Claim::Replay(status, response))
}

/// Record the response of a [`Claim::Fresh`] key; commit with the handler's transaction.
pub async fn store(
    conn: &mut PgConnection,
    user: Uuid,
    key: &IdempotencyKey,
    status: StatusCode,
    response: &serde_json::Value,
) -> ApiResult<()> {
    sqlx::query(
        "UPDATE idempotency_keys SET status = $3, response = $4 WHERE user_id = $1 AND key = $2",
    )
    .bind(user)
    .bind(&key.0)
    .bind(status.as_u16() as i16)
    .bind(response)
    .execute(conn)
    .await?;
    Ok(())
}
