//! Audit trail: who did what, when, with before/after values (FR-044).
use sqlx::PgConnection;
use std::net::IpAddr;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AuditEntry<'a> {
    pub org_id: Uuid,
    pub actor_id: Option<Uuid>,
    /// Dotted verb, e.g. `leave.approve`, `policy.update`, `auth.recovery_login`.
    pub action: &'a str,
    pub entity_kind: &'a str,
    pub entity_id: Option<Uuid>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
    pub ip: Option<IpAddr>,
}

/// Write one audit row inside the caller's org-scoped transaction.
pub async fn write_audit(conn: &mut PgConnection, e: &AuditEntry<'_>) -> sqlx::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO audit_log (id, org_id, actor_id, action, entity_kind, entity_id, before, after, ip)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::inet)",
    )
    .bind(id)
    .bind(e.org_id)
    .bind(e.actor_id)
    .bind(e.action)
    .bind(e.entity_kind)
    .bind(e.entity_id)
    .bind(&e.before)
    .bind(&e.after)
    .bind(e.ip.map(|ip| ip.to_string()))
    .execute(conn)
    .await?;
    Ok(id)
}
