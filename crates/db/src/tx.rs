//! Org-scoped transactions and balance locks.
//!
//! Every tenant query runs inside a transaction that has set `app.org_id` with
//! `SET LOCAL` semantics, so the RLS policies only expose that organization's rows and the
//! setting disappears when the transaction ends.
use sqlx::{Acquire, PgConnection, Postgres, Transaction};
use uuid::Uuid;

/// Begin a transaction scoped to `org` on a pool, pool connection or connection.
pub async fn begin_org_tx<'c, A>(conn: A, org: Uuid) -> sqlx::Result<Transaction<'c, Postgres>>
where
    A: Acquire<'c, Database = Postgres>,
{
    let mut tx = conn.begin().await?;
    set_org(&mut tx, org).await?;
    Ok(tx)
}

/// Same as [`begin_org_tx`]; reads better when passing a borrowed connection.
pub async fn begin_org_tx_on<'c, A>(conn: A, org: Uuid) -> sqlx::Result<Transaction<'c, Postgres>>
where
    A: Acquire<'c, Database = Postgres>,
{
    begin_org_tx(conn, org).await
}

/// Scope the current transaction to `org` (transaction-local).
pub async fn set_org(conn: &mut PgConnection, org: Uuid) -> sqlx::Result<()> {
    sqlx::query("SELECT set_config('app.org_id', $1, true)")
        .bind(org.to_string())
        .execute(conn)
        .await?;
    Ok(())
}

/// Serialise every balance change for one employee and leave type (FR-019).
pub async fn lock_balance(
    conn: &mut PgConnection,
    user: Uuid,
    leave_type: Uuid,
) -> sqlx::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("balance:{user}:{leave_type}"))
        .execute(conn)
        .await?;
    Ok(())
}

/// Serialise org-wide invariants such as "at least one active admin" or year-end rollover.
pub async fn lock_org(conn: &mut PgConnection, org: Uuid) -> sqlx::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("org:{org}"))
        .execute(conn)
        .await?;
    Ok(())
}
