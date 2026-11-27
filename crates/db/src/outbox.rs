//! Transactional outbox for email (FR-046, FR-047): queue an email in the same transaction as
//! the change that triggers it. If the transaction rolls back, nothing is sent.
use crate::jobs::{enqueue, NewJob};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

pub const SEND_EMAIL: &str = "send_email";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxAttachment {
    pub filename: String,
    pub content_type: String,
    /// Text content (calendar invites, CSV summaries).
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutgoingEmail {
    pub org_id: Option<Uuid>,
    /// Template name, e.g. `login_code` (see `leavo-email` templates).
    pub template: String,
    pub to: String,
    pub context: serde_json::Value,
    #[serde(default)]
    pub attachments: Vec<OutboxAttachment>,
}

/// Queue one email, due at `now` (from the caller's clock). Call inside the transaction that
/// makes the change.
pub async fn notify(
    conn: &mut PgConnection,
    email: &OutgoingEmail,
    now: DateTime<Utc>,
) -> sqlx::Result<Uuid> {
    let payload = serde_json::to_value(email).expect("email payload serialises");
    let id = enqueue(
        conn,
        &NewJob {
            kind: SEND_EMAIL.into(),
            org_id: email.org_id,
            payload,
            run_at: now,
            dedupe_key: None,
        },
    )
    .await?;
    Ok(id.expect("no dedupe key, always inserted"))
}
