//! One-click Approve/Reject links in approver emails (T119, FR-024, research R5).
//!
//! Token = `<nonce>.<expiry>.<hmac>`; the nonce is the `action_tokens` row, which binds it to
//! the organization, request, approver and decision. Opening the link only pre-fills the
//! decision (link scanners may open it); deciding needs a session, and a decision retires every
//! link of the request.
use chrono::{DateTime, Duration, Utc};
use leavo_core::tokens::{mac_b64, mac_matches};
use sqlx::PgConnection;
use uuid::Uuid;

pub const TTL_HOURS: i64 = 72;

fn message(row: &TokenRow, expires: i64) -> String {
    format!(
        "action\n{}\n{}\n{}\n{}\n{}\n{}\n{expires}",
        row.id, row.org_id, row.request_id, row.request_kind, row.approver_id, row.decision
    )
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TokenRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub request_id: Uuid,
    pub request_kind: String,
    pub approver_id: Uuid,
    pub decision: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

/// Create a link token for one approver and one decision.
pub async fn mint(
    conn: &mut PgConnection,
    secret: &[u8],
    org: Uuid,
    request: Uuid,
    kind: &str,
    approver: Uuid,
    decision: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<String> {
    let row = TokenRow {
        id: Uuid::now_v7(),
        org_id: org,
        request_id: request,
        request_kind: kind.into(),
        approver_id: approver,
        decision: decision.into(),
        expires_at: now + Duration::hours(TTL_HOURS),
        used_at: None,
    };
    sqlx::query(
        "INSERT INTO action_tokens (id, org_id, request_id, request_kind, approver_id, decision, expires_at, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(row.id)
    .bind(row.org_id)
    .bind(row.request_id)
    .bind(&row.request_kind)
    .bind(row.approver_id)
    .bind(&row.decision)
    .bind(row.expires_at)
    .bind(now)
    .execute(conn)
    .await?;
    let exp = row.expires_at.timestamp();
    Ok(format!(
        "{}.{exp}.{}",
        row.id.simple(),
        mac_b64(secret, &message(&row, exp))
    ))
}

#[derive(Debug)]
pub enum Check {
    Valid(TokenRow),
    Used,
    Expired,
    Invalid,
}

/// Look a token up and check its signature, expiry and use.
pub async fn check(
    conn: &mut PgConnection,
    secret: &[u8],
    token: &str,
    now: DateTime<Utc>,
) -> sqlx::Result<Check> {
    let mut parts = token.split('.');
    let (Some(nonce), Some(exp), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Ok(Check::Invalid);
    };
    let (Ok(nonce), Ok(exp)) = (Uuid::parse_str(nonce), exp.parse::<i64>()) else {
        return Ok(Check::Invalid);
    };
    let row: Option<TokenRow> = sqlx::query_as(
        "SELECT id, org_id, request_id, request_kind, approver_id, decision, expires_at, used_at
         FROM action_tokens WHERE id = $1",
    )
    .bind(nonce)
    .fetch_optional(conn)
    .await?;
    let Some(row) = row else {
        return Ok(Check::Invalid);
    };
    if exp != row.expires_at.timestamp() || !mac_matches(secret, &message(&row, exp), sig) {
        return Ok(Check::Invalid);
    }
    Ok(if row.used_at.is_some() {
        Check::Used
    } else if now >= row.expires_at {
        Check::Expired
    } else {
        Check::Valid(row)
    })
}
