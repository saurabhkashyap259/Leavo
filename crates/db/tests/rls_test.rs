//! T018 — tenant isolation through Row-Level Security (FR-048, constitution IV).
use leavo_db::testkit::TestDb;
use leavo_db::tx::begin_org_tx;
use sqlx::Row;
use uuid::Uuid;

/// Tables that carry `org_id` but are read before the tenant is known, on purpose.
/// Each must be accessed only by an unguessable key or by the worker.
const RLS_EXEMPT: &[(&str, &str)] = &[
    (
        "sessions",
        "looked up by token hash before the org is known",
    ),
    (
        "action_tokens",
        "looked up by single-use nonce from an email link",
    ),
    (
        "jobs",
        "claimed by the worker across all orgs; handlers re-scope to the job's org",
    ),
];

#[tokio::test]
async fn every_tenant_table_has_rls_enabled_with_a_policy() {
    let db = TestDb::new().await;
    let rows = sqlx::query(
        r#"
        SELECT c.relname AS table,
               c.relrowsecurity AS rls,
               EXISTS (SELECT 1 FROM pg_policies p WHERE p.tablename = c.relname) AS has_policy
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public'
        WHERE c.relkind = 'r'
          AND (c.relname = 'organizations'
               OR EXISTS (SELECT 1 FROM information_schema.columns col
                          WHERE col.table_schema = 'public' AND col.table_name = c.relname
                            AND col.column_name = 'org_id'))
        "#,
    )
    .fetch_all(&db.owner)
    .await
    .unwrap();
    assert!(
        rows.len() >= 10,
        "expected tenant tables, found {}",
        rows.len()
    );
    for r in rows {
        let table: String = r.get("table");
        if RLS_EXEMPT.iter().any(|(t, _)| *t == table) {
            continue;
        }
        assert!(r.get::<bool, _>("rls"), "RLS not enabled on {table}");
        assert!(r.get::<bool, _>("has_policy"), "no policy on {table}");
    }
}

async fn seed_org(db: &TestDb, name: &str) -> Uuid {
    let org = Uuid::now_v7();
    let user = Uuid::now_v7();
    let group = Uuid::now_v7();
    let lt = Uuid::now_v7();
    let owner = &db.owner;
    sqlx::query("INSERT INTO organizations (id, name) VALUES ($1, $2)")
        .bind(org)
        .bind(name)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO org_domains (id, org_id, domain) VALUES ($1, $2, $3)")
        .bind(Uuid::now_v7())
        .bind(org)
        .bind(format!("{name}.test"))
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, 'Eng')")
        .bind(group)
        .bind(org)
        .execute(owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users (id, org_id, email, full_name, role, approver_group_id) VALUES ($1, $2, $3, 'U', 'approver', $4)")
        .bind(user).bind(org).bind(format!("u@{name}.test")).bind(group).execute(owner).await.unwrap();
    sqlx::query(
        "INSERT INTO approver_group_members (group_id, user_id, org_id) VALUES ($1, $2, $3)",
    )
    .bind(group)
    .bind(user)
    .bind(org)
    .execute(owner)
    .await
    .unwrap();
    sqlx::query("INSERT INTO leave_types (id, org_id, code, name, color, yearly_quota) VALUES ($1, $2, 'CL', 'Casual', '#3B82F6', 12)")
        .bind(lt).bind(org).execute(owner).await.unwrap();
    sqlx::query(
        "INSERT INTO holidays (id, org_id, date, name) VALUES ($1, $2, '2026-12-25', 'Christmas')",
    )
    .bind(Uuid::now_v7())
    .bind(org)
    .execute(owner)
    .await
    .unwrap();
    sqlx::query("INSERT INTO balance_ledger (id, org_id, user_id, leave_type_id, leave_year, delta, reason) VALUES ($1, $2, $3, $4, 2026, 12, 'allotment')")
        .bind(Uuid::now_v7()).bind(org).bind(user).bind(lt).execute(owner).await.unwrap();
    sqlx::query("INSERT INTO audit_log (id, org_id, actor_id, action, entity_kind) VALUES ($1, $2, $3, 'test.seed', 'org')")
        .bind(Uuid::now_v7()).bind(org).bind(user).execute(owner).await.unwrap();
    sqlx::query("INSERT INTO rollover_runs (org_id, leave_year) VALUES ($1, 2025)")
        .bind(org)
        .execute(owner)
        .await
        .unwrap();
    org
}

const TENANT_TABLES: &[&str] = &[
    "organizations",
    "org_domains",
    "approver_groups",
    "approver_group_members",
    "users",
    "leave_types",
    "holidays",
    "balance_ledger",
    "audit_log",
    "rollover_runs",
    "v_ledger_balances",
];

#[tokio::test]
async fn app_role_sees_only_its_own_org() {
    let db = TestDb::new().await;
    let a = seed_org(&db, "alpha").await;
    let _b = seed_org(&db, "beta").await;

    let mut tx = begin_org_tx(&db.app, a).await.unwrap();
    for table in TENANT_TABLES {
        let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let col = if *table == "organizations" {
            "id"
        } else {
            "org_id"
        };
        let foreign: i64 =
            sqlx::query_scalar(&format!("SELECT count(*) FROM {table} WHERE {col} <> $1"))
                .bind(a)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert!(total >= 1, "{table}: org A should see its own rows");
        assert_eq!(foreign, 0, "{table}: org A must not see org B rows");
    }
}

#[tokio::test]
async fn app_role_without_tenant_sees_nothing() {
    let db = TestDb::new().await;
    seed_org(&db, "alpha").await;
    for table in TENANT_TABLES {
        let n: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&db.app)
            .await
            .unwrap();
        assert_eq!(n, 0, "{table} leaked rows without app.org_id");
    }
}

#[tokio::test]
async fn app_role_cannot_write_into_another_org() {
    let db = TestDb::new().await;
    let a = seed_org(&db, "alpha").await;
    let b = seed_org(&db, "beta").await;
    let mut tx = begin_org_tx(&db.app, a).await.unwrap();
    let res = sqlx::query(
        "INSERT INTO holidays (id, org_id, date, name) VALUES ($1, $2, '2026-01-26', 'X')",
    )
    .bind(Uuid::now_v7())
    .bind(b)
    .execute(&mut *tx)
    .await;
    assert!(
        res.is_err(),
        "insert into org B must be rejected by the policy"
    );
}

#[tokio::test]
async fn tenant_setting_does_not_leak_past_the_transaction() {
    let db = TestDb::new().await;
    let a = seed_org(&db, "alpha").await;
    let mut conn = db.app.acquire().await.unwrap();
    {
        let mut tx = leavo_db::tx::begin_org_tx_on(&mut conn, a).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(n, 1);
        tx.commit().await.unwrap();
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    assert_eq!(n, 0, "SET LOCAL must end with the transaction");
}
