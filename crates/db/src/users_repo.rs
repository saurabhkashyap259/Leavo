//! Users and joining allotments (T062). Runs inside an org-scoped transaction.
use crate::ledger::{insert_entry, LedgerEntry, LedgerReason};
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use leavo_core::balance::prorate;
use leavo_core::calendar::{leave_year_bounds, leave_year_of, today_in};
use rust_decimal::Decimal;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct NewUserRow<'a> {
    pub org_id: Uuid,
    pub email: &'a str,
    pub full_name: &'a str,
    pub role: &'a str,
    pub status: &'a str,
    pub date_of_joining: Option<NaiveDate>,
}

pub async fn insert_user(
    conn: &mut PgConnection,
    u: &NewUserRow<'_>,
    now: DateTime<Utc>,
) -> sqlx::Result<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO users (id, org_id, email, full_name, role, status, date_of_joining, created_at)
         VALUES ($1, $2, $3, $4, $5::user_role, $6::user_status, $7, $8) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(u.org_id)
    .bind(u.email)
    .bind(u.full_name)
    .bind(u.role)
    .bind(u.status)
    .bind(u.date_of_joining)
    .bind(now)
    .fetch_one(conn)
    .await
}

pub async fn activate(conn: &mut PgConnection, user: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE users SET status = 'active' WHERE id = $1 AND status = 'invited'")
        .bind(user)
        .execute(conn)
        .await?;
    Ok(())
}

/// The organization's current date and leave year at `now` (FR-049).
pub async fn org_today(
    conn: &mut PgConnection,
    org: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<(NaiveDate, u32)> {
    let (tz, start): (String, i16) =
        sqlx::query_as("SELECT timezone, leave_year_start_month FROM organizations WHERE id = $1")
            .bind(org)
            .fetch_one(&mut *conn)
            .await?;
    let tz: Tz = tz.parse().unwrap_or(chrono_tz::Asia::Kolkata);
    Ok((today_in(tz, now), start as u32))
}

/// Allot this leave year's quota of every active, non-restricted type, pro-rated by the
/// joining date (FR-013). Returns the entries written.
pub async fn allot_on_join(
    conn: &mut PgConnection,
    org: Uuid,
    user: Uuid,
    doj: Option<NaiveDate>,
    now: DateTime<Utc>,
) -> sqlx::Result<Vec<LedgerEntry>> {
    let (today, start_month) = org_today(conn, org, now).await?;
    let leave_year = leave_year_of(today, start_month);
    let (year_start, _) = leave_year_bounds(leave_year, start_month);
    let types: Vec<(Uuid, Decimal)> = sqlx::query_as(
        "SELECT id, yearly_quota FROM leave_types
         WHERE active AND NOT is_comp_off AND availability = 'all' AND yearly_quota > 0
         ORDER BY sort_order, code",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut written = Vec::new();
    for (leave_type_id, quota) in types {
        let amount = prorate(quota, doj, year_start, start_month);
        if amount.is_zero() {
            continue;
        }
        let entry = LedgerEntry {
            org_id: org,
            user_id: user,
            leave_type_id,
            leave_year,
            delta: amount,
            reason: if amount == quota {
                LedgerReason::Allotment
            } else {
                LedgerReason::Prorate
            },
            ref_kind: Some("user".into()),
            ref_id: Some(user),
            note: None,
            created_by: None,
        };
        insert_entry(conn, &entry).await?;
        written.push(entry);
    }
    Ok(written)
}
