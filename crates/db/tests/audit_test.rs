//! T040 — audit helper writes who/what/when with before/after values (FR-044).
use leavo_db::audit::{write_audit, AuditEntry};
use leavo_db::testkit::TestDb;
use leavo_db::tx::begin_org_tx;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn writes_an_org_scoped_audit_row() {
    let db = TestDb::new().await;
    let (org, actor) = (Uuid::now_v7(), Uuid::now_v7());
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, 'Acme')")
        .bind(org)
        .execute(&db.owner)
        .await
        .unwrap();
    let mut tx = begin_org_tx(&db.app, org).await.unwrap();
    let entity = Uuid::now_v7();
    write_audit(
        &mut tx,
        &AuditEntry {
            org_id: org,
            actor_id: Some(actor),
            action: "policy.update",
            entity_kind: "leave_type",
            entity_id: Some(entity),
            before: Some(json!({"yearly_quota": "12"})),
            after: Some(json!({"yearly_quota": "14"})),
            ip: Some("10.0.0.1".parse().unwrap()),
        },
    )
    .await
    .unwrap();
    let (action, before, after, ip): (
        String,
        serde_json::Value,
        serde_json::Value,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT action, before, after, host(ip) FROM audit_log WHERE entity_id = $1",
    )
    .bind(entity)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(action, "policy.update");
    assert_eq!(before["yearly_quota"], "12");
    assert_eq!(after["yearly_quota"], "14");
    assert_eq!(ip.as_deref(), Some("10.0.0.1"));
}
