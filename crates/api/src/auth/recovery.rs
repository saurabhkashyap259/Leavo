//! Recovery sign-in for when email is down (T210, FR-067): `leavo recovery-login <email>` prints a
//! link for an active Org Admin, valid 15 minutes and usable once. Using it starts a normal
//! session and is recorded as `auth.recovery_login`.
//!
//! The link is `/api/v1/auth/recovery/<code id>.<token>`; the database keeps only an HMAC of the
//! token (keyed with `LEAVO_SECRET_KEY`), so rotating the key also voids unused links.
use crate::app::AppState;
use crate::routes::auth::{start_session, with_session_cookie};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Duration, Utc};
use leavo_core::otp::hash_code;
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::{auth_repo, tx};
use rand::RngCore;
use sqlx::PgPool;
use std::net::SocketAddr;
use uuid::Uuid;

pub const VALID_MINUTES: i64 = 15;

/// `(email, code_hash, expires_at, consumed_at)` of a recovery code.
type CodeRow = (String, Vec<u8>, DateTime<Utc>, Option<DateTime<Utc>>);

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("{0} is not an active Org Admin")]
    NotAdmin(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

/// Create a recovery link for an active Org Admin.
pub async fn issue(
    pool: &PgPool,
    secret: &[u8],
    base_url: &str,
    email: &str,
    now: DateTime<Utc>,
) -> Result<String, RecoveryError> {
    let email = email.trim().to_lowercase();
    let mut conn = pool.begin().await?;
    let user = auth_repo::find_user(&mut conn, &email)
        .await?
        .filter(|u| u.role == "admin" && u.status == "active")
        .ok_or_else(|| RecoveryError::NotAdmin(email.clone()))?;
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO login_codes (id, email, purpose, code_hash, payload, expires_at, created_at)
         VALUES ($1, $2, 'recovery', $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(&email)
    .bind(hash_code(secret, &email, &token).to_vec())
    .bind(serde_json::json!({ "user_id": user.id }))
    .bind(now + Duration::minutes(VALID_MINUTES))
    .bind(now)
    .execute(&mut *conn)
    .await?;
    tx::set_org(&mut conn, user.org_id).await?;
    write_audit(
        &mut conn,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: None,
            action: "auth.recovery_link",
            entity_kind: "user",
            entity_id: Some(user.id),
            before: None,
            after: Some(serde_json::json!({ "valid_minutes": VALID_MINUTES })),
            ip: None,
        },
    )
    .await?;
    conn.commit().await?;
    Ok(format!(
        "{}/api/v1/auth/recovery/{id}.{token}",
        base_url.trim_end_matches('/')
    ))
}

fn to(location: &str) -> Response {
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, location.to_string()),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
    )
        .into_response()
}

#[utoipa::path(get, path = "/api/v1/auth/recovery/{link}", tag = "auth",
    params(("link" = String, Path, description = "From `leavo recovery-login`")),
    responses(
        (status = 303, description = "Valid: session cookie and `/admin`. Otherwise `/login?link=used|expired|invalid`"),
    ))]
pub async fn redeem(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(link): Path<String>,
) -> Result<Response, crate::error::ApiError> {
    let now = state.clock.now();
    let Some((id, token)) = link
        .split_once('.')
        .and_then(|(id, t)| id.parse::<Uuid>().ok().map(|id| (id, t.to_string())))
    else {
        return Ok(to("/login?link=invalid"));
    };
    let mut conn = state.db.begin().await?;
    let row: Option<CodeRow> = sqlx::query_as(
        "SELECT email::text, code_hash, expires_at, consumed_at FROM login_codes
         WHERE id = $1 AND purpose = 'recovery' FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((email, hash, expires, consumed)) = row else {
        return Ok(to("/login?link=invalid"));
    };
    if hash != hash_code(state.config.secret_key.as_bytes(), &email, &token) {
        return Ok(to("/login?link=invalid"));
    }
    if consumed.is_some() {
        return Ok(to("/login?link=used"));
    }
    if expires <= now {
        return Ok(to("/login?link=expired"));
    }
    sqlx::query("UPDATE login_codes SET consumed_at = $2 WHERE id = $1")
        .bind(id)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    // Still an active admin now?
    let Some(user) = auth_repo::find_user(&mut conn, &email)
        .await?
        .filter(|u| u.role == "admin" && u.status == "active")
    else {
        conn.commit().await?;
        return Ok(to("/login?link=invalid"));
    };
    tx::set_org(&mut conn, user.org_id).await?;
    let session = start_session(
        &mut conn,
        user.id,
        user.org_id,
        false,
        &headers,
        peer.ip(),
        now,
    )
    .await?;
    write_audit(
        &mut conn,
        &AuditEntry {
            org_id: user.org_id,
            actor_id: Some(user.id),
            action: "auth.recovery_login",
            entity_kind: "user",
            entity_id: Some(user.id),
            before: None,
            after: None,
            ip: Some(peer.ip()),
        },
    )
    .await?;
    conn.commit().await?;
    Ok(with_session_cookie(to("/admin"), &session))
}
