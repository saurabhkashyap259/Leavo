//! The one-time setup token (T078, FR-007).
//!
//! * Generated on the first `serve` while no organization exists and printed once to the log;
//!   only its SHA-256 is stored (`instance_settings.setup_token_hash`).
//! * `leavo setup-token` replaces it (the operator lost the log line).
//! * 5 wrong tries lock it for 15 minutes; it is cleared when setup completes.
use crate::error::ApiError;
use chrono::{DateTime, Duration, Utc};
use leavo_core::error::ErrorCode;
use leavo_core::tokens::hash_token;
use rand::RngCore;
use sqlx::{PgConnection, PgPool};
use subtle::ConstantTimeEq;

pub const MAX_WRONG: i32 = 5;
pub const LOCK_MINUTES: i64 = 15;

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("setup is already complete")]
    SetupComplete,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

fn new_token() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn setup_open(conn: &mut PgConnection) -> sqlx::Result<bool> {
    let has_org: bool = sqlx::query_scalar("SELECT instance_has_org()")
        .fetch_one(&mut *conn)
        .await?;
    let completed: bool = sqlx::query_scalar(
        "SELECT setup_completed_at IS NOT NULL FROM instance_settings WHERE id = 1",
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(!has_org && !completed)
}

/// Replace the token (CLI `leavo setup-token`, and tests). Refused once an organization exists.
pub async fn regenerate(db: &PgPool) -> Result<String, TokenError> {
    let mut tx = db.begin().await?;
    sqlx::query("SELECT 1 FROM instance_settings WHERE id = 1 FOR UPDATE")
        .execute(&mut *tx)
        .await?;
    if !setup_open(&mut tx).await? {
        return Err(TokenError::SetupComplete);
    }
    let token = new_token();
    sqlx::query(
        "UPDATE instance_settings SET setup_token_hash = $1, setup_token_failed_attempts = 0,
                setup_token_locked_until = NULL WHERE id = 1",
    )
    .bind(hash_token(&token).to_vec())
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(token)
}

/// On `serve`: create a token if setup is pending and none exists yet. Returns the new token
/// (to log once); `None` when setup is done or a token already exists.
pub async fn ensure(db: &PgPool) -> Result<Option<String>, TokenError> {
    let exists: bool = sqlx::query_scalar(
        "SELECT setup_token_hash IS NOT NULL FROM instance_settings WHERE id = 1",
    )
    .fetch_one(db)
    .await?;
    if exists {
        return Ok(None);
    }
    match regenerate(db).await {
        Ok(t) => Ok(Some(t)),
        Err(TokenError::SetupComplete) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Check a presented token, counting wrong tries in its own transaction (so they stick even
/// when the caller fails later). Wrong → `403 SETUP_TOKEN_INVALID`; locked → `429 LOCKED`.
pub async fn check(db: &PgPool, presented: &str, now: DateTime<Utc>) -> Result<(), ApiError> {
    let mut tx = db.begin().await?;
    let (hash, failed, locked_until): (Option<Vec<u8>>, i32, Option<DateTime<Utc>>) =
        sqlx::query_as(
            "SELECT setup_token_hash, setup_token_failed_attempts, setup_token_locked_until
         FROM instance_settings WHERE id = 1 FOR UPDATE",
        )
        .fetch_one(&mut *tx)
        .await?;
    if let Some(until) = locked_until.filter(|u| *u > now) {
        let wait = (until - now).num_seconds().max(1) as u64;
        return Err(ApiError::new(ErrorCode::Locked).retry_after(wait));
    }
    let Some(hash) = hash else {
        return Err(ApiError::new(ErrorCode::SetupTokenInvalid));
    };
    if bool::from(hash_token(presented.trim()).as_slice().ct_eq(&hash)) {
        if failed > 0 {
            sqlx::query(
                "UPDATE instance_settings SET setup_token_failed_attempts = 0 WHERE id = 1",
            )
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        return Ok(());
    }
    let failed = failed + 1;
    let (failed, lock) = if failed >= MAX_WRONG {
        (0, Some(now + Duration::minutes(LOCK_MINUTES)))
    } else {
        (failed, None)
    };
    sqlx::query(
        "UPDATE instance_settings SET setup_token_failed_attempts = $1, setup_token_locked_until = $2
         WHERE id = 1",
    )
    .bind(failed)
    .bind(lock)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Err(ApiError::new(ErrorCode::SetupTokenInvalid))
}

/// Retire the token when setup completes (inside the setup transaction).
pub async fn retire(conn: &mut PgConnection, now: DateTime<Utc>) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE instance_settings SET setup_token_hash = NULL, setup_token_failed_attempts = 0,
                setup_token_locked_until = NULL, setup_completed_at = $1 WHERE id = 1",
    )
    .bind(now)
    .execute(conn)
    .await?;
    Ok(())
}

/// Whether the first-run wizard is still needed.
pub async fn setup_required(db: &PgPool) -> sqlx::Result<bool> {
    let mut conn = db.acquire().await?;
    setup_open(&mut conn).await
}
