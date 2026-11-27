//! Append-only balance ledger (FR-040, constitution V).
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "ledger_reason", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum LedgerReason {
    Allotment,
    Prorate,
    QuotaChange,
    CarryForward,
    NegativeCarry,
    Lapse,
    LeaveTaken,
    LeaveReversed,
    CompOffCredit,
    CompOffExpired,
    CompOffRevoked,
    ManualAdjustment,
    Import,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LedgerEntry {
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub leave_type_id: Uuid,
    pub leave_year: i32,
    pub delta: Decimal,
    pub reason: LedgerReason,
    pub ref_kind: Option<String>,
    pub ref_id: Option<Uuid>,
    pub note: Option<String>,
    pub created_by: Option<Uuid>,
}

/// Append one entry. Must run inside an org-scoped transaction.
pub async fn insert_entry(conn: &mut PgConnection, e: &LedgerEntry) -> sqlx::Result<Uuid> {
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO balance_ledger
             (id, org_id, user_id, leave_type_id, leave_year, delta, reason, ref_kind, ref_id, note, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
    .bind(e.org_id)
    .bind(e.user_id)
    .bind(e.leave_type_id)
    .bind(e.leave_year)
    .bind(e.delta)
    .bind(e.reason)
    .bind(&e.ref_kind)
    .bind(e.ref_id)
    .bind(&e.note)
    .bind(e.created_by)
    .execute(conn)
    .await?;
    Ok(id)
}

/// Ledger-only balance figures (no pending requests). Comp-off rows have `leave_year = None`.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow, Serialize)]
pub struct LedgerBalance {
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub leave_type_id: Uuid,
    pub leave_year: Option<i32>,
    pub allotted: Decimal,
    pub carried: Decimal,
    pub taken: Decimal,
    pub adjusted: Decimal,
    pub balance: Decimal,
}

pub async fn ledger_balances(
    conn: &mut PgConnection,
    user: Uuid,
) -> sqlx::Result<Vec<LedgerBalance>> {
    sqlx::query_as::<_, LedgerBalance>(
        "SELECT org_id, user_id, leave_type_id, leave_year, allotted, carried, taken, adjusted, balance
         FROM v_ledger_balances WHERE user_id = $1
         ORDER BY leave_type_id, leave_year NULLS FIRST",
    )
    .bind(user)
    .fetch_all(conn)
    .await
}

/// Ledger balance of one type in the given leave years (missing years → zero).
pub async fn balance_by_year(
    conn: &mut PgConnection,
    user: Uuid,
    leave_type: Uuid,
    years: &[i32],
) -> sqlx::Result<Vec<(i32, Decimal)>> {
    let rows: Vec<(i32, Decimal)> = sqlx::query_as(
        "SELECT y, COALESCE(SUM(l.delta), 0)
         FROM unnest($3::int[]) AS y
         LEFT JOIN balance_ledger l
                ON l.user_id = $1 AND l.leave_type_id = $2 AND l.leave_year = y
         GROUP BY y ORDER BY y",
    )
    .bind(user)
    .bind(leave_type)
    .bind(years)
    .fetch_all(conn)
    .await?;
    Ok(rows)
}
