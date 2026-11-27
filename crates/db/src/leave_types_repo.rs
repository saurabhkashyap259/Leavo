//! Leave types and restricted-type grants (T084, FR-011, FR-012, FR-056).
//! Every function runs inside an org-scoped transaction.
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{FromRow, PgConnection};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, FromRow)]
pub struct LeaveTypeRow {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub color: String,
    pub yearly_quota: Option<Decimal>,
    pub pending_yearly_quota: Option<Decimal>,
    pub is_comp_off: bool,
    pub receives_carry_forward: bool,
    pub allow_half_day: bool,
    pub allow_negative: bool,
    pub negative_limit: Decimal,
    pub max_days_per_request: Option<Decimal>,
    pub min_notice_days: i32,
    pub max_backdate_days: i32,
    pub attachment_required_above_days: Option<Decimal>,
    pub availability: String,
    pub active: bool,
    pub sort_order: i32,
    /// Users with an active grant (restricted types).
    pub granted_count: i64,
}

const SELECT: &str = "SELECT t.id, t.code, t.name, t.color, t.yearly_quota, t.pending_yearly_quota,
        t.is_comp_off, t.receives_carry_forward, t.allow_half_day, t.allow_negative, t.negative_limit,
        t.max_days_per_request, t.min_notice_days, t.max_backdate_days,
        t.attachment_required_above_days, t.availability::text AS availability, t.active, t.sort_order,
        (SELECT count(*) FROM leave_type_grants g
          WHERE g.leave_type_id = t.id AND g.revoked_at IS NULL) AS granted_count
     FROM leave_types t";

pub async fn list(conn: &mut PgConnection) -> sqlx::Result<Vec<LeaveTypeRow>> {
    sqlx::query_as(&format!("{SELECT} ORDER BY t.sort_order, t.code"))
        .fetch_all(conn)
        .await
}

pub async fn get(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<LeaveTypeRow>> {
    sqlx::query_as(&format!("{SELECT} WHERE t.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Lock a type row for the rest of the transaction (quota changes, grants).
pub async fn get_for_update(
    conn: &mut PgConnection,
    id: Uuid,
) -> sqlx::Result<Option<LeaveTypeRow>> {
    let found: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM leave_types WHERE id = $1 FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
    match found {
        Some(id) => get(conn, id).await,
        None => Ok(None),
    }
}

/// Insert (`id` new) or update every editable column of a type.
pub async fn upsert(conn: &mut PgConnection, org: Uuid, t: &LeaveTypeRow) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO leave_types
             (id, org_id, code, name, color, yearly_quota, pending_yearly_quota, is_comp_off,
              receives_carry_forward, allow_half_day, allow_negative, negative_limit,
              max_days_per_request, min_notice_days, max_backdate_days,
              attachment_required_above_days, availability, active, sort_order)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                 $17::leave_availability, $18, $19)
         ON CONFLICT (id) DO UPDATE SET
             code = EXCLUDED.code, name = EXCLUDED.name, color = EXCLUDED.color,
             yearly_quota = EXCLUDED.yearly_quota, pending_yearly_quota = EXCLUDED.pending_yearly_quota,
             receives_carry_forward = EXCLUDED.receives_carry_forward,
             allow_half_day = EXCLUDED.allow_half_day, allow_negative = EXCLUDED.allow_negative,
             negative_limit = EXCLUDED.negative_limit, max_days_per_request = EXCLUDED.max_days_per_request,
             min_notice_days = EXCLUDED.min_notice_days, max_backdate_days = EXCLUDED.max_backdate_days,
             attachment_required_above_days = EXCLUDED.attachment_required_above_days,
             availability = EXCLUDED.availability, active = EXCLUDED.active,
             sort_order = EXCLUDED.sort_order",
    )
    .bind(t.id)
    .bind(org)
    .bind(&t.code)
    .bind(&t.name)
    .bind(&t.color)
    .bind(t.yearly_quota)
    .bind(t.pending_yearly_quota)
    .bind(t.is_comp_off)
    .bind(t.receives_carry_forward)
    .bind(t.allow_half_day)
    .bind(t.allow_negative)
    .bind(t.negative_limit)
    .bind(t.max_days_per_request)
    .bind(t.min_notice_days)
    .bind(t.max_backdate_days)
    .bind(t.attachment_required_above_days)
    .bind(&t.availability)
    .bind(t.active)
    .bind(t.sort_order)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn code_taken(
    conn: &mut PgConnection,
    code: &str,
    except: Option<Uuid>,
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM leave_types WHERE code = $1 AND id IS DISTINCT FROM $2)",
    )
    .bind(code)
    .bind(except)
    .fetch_one(conn)
    .await
}

pub async fn next_sort_order(conn: &mut PgConnection) -> sqlx::Result<i32> {
    sqlx::query_scalar("SELECT COALESCE(max(sort_order), 0) + 1 FROM leave_types")
        .fetch_one(conn)
        .await
}

/// Users a quota change applies to: active or invited users, and for restricted types only
/// those holding an active grant.
pub async fn affected_users(conn: &mut PgConnection, t: &LeaveTypeRow) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT u.id FROM users u
         WHERE u.status IN ('active', 'invited')
           AND ($2 = 'all' OR EXISTS (
                SELECT 1 FROM leave_type_grants g
                WHERE g.leave_type_id = $1 AND g.user_id = u.id AND g.revoked_at IS NULL))
         ORDER BY u.id",
    )
    .bind(t.id)
    .bind(&t.availability)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, PartialEq, FromRow)]
pub struct GrantRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub email: String,
    pub full_name: String,
    pub granted_by: Option<Uuid>,
    pub granted_at: DateTime<Utc>,
    pub revoked_by: Option<Uuid>,
    pub revoked_at: Option<DateTime<Utc>>,
}

pub async fn list_grants(
    conn: &mut PgConnection,
    leave_type: Uuid,
    include_revoked: bool,
) -> sqlx::Result<Vec<GrantRow>> {
    sqlx::query_as(
        "SELECT g.id, g.user_id, u.email::text AS email, u.full_name, g.granted_by, g.granted_at,
                g.revoked_by, g.revoked_at
         FROM leave_type_grants g JOIN users u ON u.id = g.user_id
         WHERE g.leave_type_id = $1 AND ($2 OR g.revoked_at IS NULL)
         ORDER BY g.granted_at DESC, g.id DESC",
    )
    .bind(leave_type)
    .bind(include_revoked)
    .fetch_all(conn)
    .await
}

pub async fn active_grant(
    conn: &mut PgConnection,
    leave_type: Uuid,
    user: Uuid,
) -> sqlx::Result<Option<GrantRow>> {
    Ok(list_grants(conn, leave_type, false)
        .await?
        .into_iter()
        .find(|g| g.user_id == user))
}

pub async fn insert_grant(
    conn: &mut PgConnection,
    org: Uuid,
    leave_type: Uuid,
    user: Uuid,
    by: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO leave_type_grants (id, org_id, leave_type_id, user_id, granted_by, granted_at)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(org)
    .bind(leave_type)
    .bind(user)
    .bind(by)
    .bind(now)
    .fetch_one(conn)
    .await
}

pub async fn revoke_grant(
    conn: &mut PgConnection,
    leave_type: Uuid,
    user: Uuid,
    by: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar(
        "UPDATE leave_type_grants SET revoked_at = $4, revoked_by = $3
         WHERE leave_type_id = $1 AND user_id = $2 AND revoked_at IS NULL RETURNING id",
    )
    .bind(leave_type)
    .bind(user)
    .bind(by)
    .bind(now)
    .fetch_optional(conn)
    .await
}

/// Whether a grant allotment was already written for this user, type and leave year.
pub async fn grant_allotted(
    conn: &mut PgConnection,
    user: Uuid,
    leave_type: Uuid,
    leave_year: i32,
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM balance_ledger
         WHERE user_id = $1 AND leave_type_id = $2 AND leave_year = $3
           AND reason = 'allotment' AND ref_kind = 'grant')",
    )
    .bind(user)
    .bind(leave_type)
    .bind(leave_year)
    .fetch_one(conn)
    .await
}
