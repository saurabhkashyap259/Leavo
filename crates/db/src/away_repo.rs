//! Who's away and the team calendar (T131, FR-027). Rows carry the leave type and reason; the
//! API drops them for viewers who may not see them (FR-058). Runs in an org-scoped transaction.
use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgConnection;
use uuid::Uuid;

/// Approved leave counts as "away" until it is cancelled, including while a cancellation is
/// being decided.
const AWAY: &str = "r.status IN ('approved', 'cancel_requested') AND u.status = 'active'";

/// `scope=team` for viewer `$1`: the same approver group, the groups the viewer approves, or
/// the same department (research R10).
const TEAM: &str = "($1::uuid IS NULL OR EXISTS (
        SELECT 1 FROM users me WHERE me.id = $1 AND (
            (me.approver_group_id IS NOT NULL AND u.approver_group_id = me.approver_group_id)
         OR (me.department IS NOT NULL AND u.department = me.department)
         OR u.approver_group_id IN (SELECT group_id FROM approver_group_members WHERE user_id = me.id))))";

/// One approved request overlapping the range.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AwayRow {
    pub user_id: Uuid,
    pub full_name: String,
    pub group_id: Option<Uuid>,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub days: Decimal,
    pub leave_type_id: Uuid,
    pub code: String,
    pub name: String,
    pub color: String,
    pub reason: String,
}

/// Approved requests overlapping `from..=to`, sorted by start date then name. `team_of`
/// limits the list to that viewer's team.
pub async fn requests_between(
    conn: &mut PgConnection,
    team_of: Option<Uuid>,
    from: NaiveDate,
    to: NaiveDate,
) -> sqlx::Result<Vec<AwayRow>> {
    sqlx::query_as(&format!(
        "SELECT u.id AS user_id, u.full_name, u.approver_group_id AS group_id,
                r.from_date, r.to_date, r.days, t.id AS leave_type_id, t.code, t.name, t.color,
                r.reason
         FROM leave_requests r
         JOIN users u ON u.id = r.user_id
         JOIN leave_types t ON t.id = r.leave_type_id
         WHERE {AWAY} AND {TEAM} AND r.from_date <= $3 AND r.to_date >= $2
         ORDER BY r.from_date, u.full_name, r.id"
    ))
    .bind(team_of)
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await
}

/// People with an approved booked day (or half day) on `day`.
pub async fn count_on(
    conn: &mut PgConnection,
    team_of: Option<Uuid>,
    day: NaiveDate,
) -> sqlx::Result<i64> {
    sqlx::query_scalar(&format!(
        "SELECT COUNT(DISTINCT u.id)
         FROM leave_request_days d
         JOIN leave_requests r ON r.id = d.leave_request_id
         JOIN users u ON u.id = d.user_id
         WHERE {AWAY} AND {TEAM} AND d.active AND d.day = $2"
    ))
    .bind(team_of)
    .bind(day)
    .fetch_one(conn)
    .await
}

/// One booked day of approved leave.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AwayDay {
    pub day: NaiveDate,
    /// `first`/`second`, or `None` for the full day.
    pub half: Option<String>,
    pub user_id: Uuid,
    pub full_name: String,
    pub group_id: Option<Uuid>,
    pub leave_type_id: Uuid,
    pub code: String,
    pub name: String,
    pub color: String,
    pub reason: String,
}

/// Booked days of approved leave in `from..=to` (weekends and holidays are never booked),
/// sorted by day then name.
pub async fn days_between(
    conn: &mut PgConnection,
    team_of: Option<Uuid>,
    from: NaiveDate,
    to: NaiveDate,
) -> sqlx::Result<Vec<AwayDay>> {
    sqlx::query_as(&format!(
        "SELECT d.day, d.half::text AS half, u.id AS user_id, u.full_name,
                u.approver_group_id AS group_id, t.id AS leave_type_id, t.code, t.name, t.color,
                r.reason
         FROM leave_request_days d
         JOIN leave_requests r ON r.id = d.leave_request_id
         JOIN users u ON u.id = d.user_id
         JOIN leave_types t ON t.id = r.leave_type_id
         WHERE {AWAY} AND {TEAM} AND d.active AND d.day BETWEEN $2 AND $3
         ORDER BY d.day, u.full_name, d.half NULLS FIRST"
    ))
    .bind(team_of)
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await
}
