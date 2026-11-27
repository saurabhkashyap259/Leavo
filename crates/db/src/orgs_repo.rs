//! Organizations, whitelisted domains and holiday rows (US2).
use chrono::NaiveDate;
use sqlx::PgConnection;
use uuid::Uuid;

/// True when `e` is a unique-constraint violation (SQLSTATE 23505).
pub fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.code().as_deref() == Some("23505"))
}

#[derive(Debug, Clone)]
pub struct NewOrg<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub timezone: &'a str,
    pub weekend_mask: i16,
    pub leave_year_start_month: i16,
}

/// Insert an organization. The transaction must already be scoped to `o.id` (`tx::set_org`).
pub async fn insert_org(conn: &mut PgConnection, o: &NewOrg<'_>) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO organizations (id, name, timezone, weekend_mask, leave_year_start_month)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(o.id)
    .bind(o.name)
    .bind(o.timezone)
    .bind(o.weekend_mask)
    .bind(o.leave_year_start_month)
    .execute(conn)
    .await?;
    Ok(())
}

/// Add a whitelisted domain. A domain used by any organization fails with a unique violation.
pub async fn add_domain(conn: &mut PgConnection, org: Uuid, domain: &str) -> sqlx::Result<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO org_domains (id, org_id, domain) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(org)
    .bind(domain)
    .fetch_one(conn)
    .await
}

pub async fn list_domains(conn: &mut PgConnection) -> sqlx::Result<Vec<(Uuid, String)>> {
    sqlx::query_as("SELECT id, domain::text FROM org_domains ORDER BY domain")
        .fetch_all(conn)
        .await
}

pub async fn remove_domain(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<bool> {
    let r = sqlx::query("DELETE FROM org_domains WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(r.rows_affected() == 1)
}

/// Insert holidays, skipping ones already present (same date and name). Returns rows added.
pub async fn insert_holidays(
    conn: &mut PgConnection,
    org: Uuid,
    holidays: &[(NaiveDate, String, String)],
    source: &str,
) -> sqlx::Result<u64> {
    let mut added = 0;
    for (date, name, kind) in holidays {
        added += sqlx::query(
            "INSERT INTO holidays (id, org_id, date, name, kind, source)
             VALUES ($1, $2, $3, $4, $5::holiday_kind, $6)
             ON CONFLICT (org_id, date, name) DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(org)
        .bind(date)
        .bind(name)
        .bind(kind)
        .bind(source)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    }
    Ok(added)
}
