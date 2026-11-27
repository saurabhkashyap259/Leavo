//! Decisions on leave requests (T118): optimistic status update, ledger rows per leave year,
//! released days on rejection, and the inbox/team queries. Runs in org-scoped transactions.
use crate::ledger::{insert_entry, LedgerEntry, LedgerReason};
use chrono::{DateTime, NaiveDate, Utc};
use leavo_core::approval::days_to_return;
use leavo_core::calendar::{BookedDay, Half};
use rust_decimal::Decimal;
use sqlx::PgConnection;
use uuid::Uuid;

/// The fields a decision needs.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RequestRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub leave_type_id: Uuid,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub start_half: Option<String>,
    pub end_half: Option<String>,
    pub days: Decimal,
    pub reason: String,
    pub status: String,
    pub version: i32,
    pub routed_group_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<String>,
}

const SELECT: &str = "SELECT id, org_id, user_id, leave_type_id, from_date, to_date,
        start_half::text AS start_half, end_half::text AS end_half, days, reason,
        status::text AS status, version, routed_group_id, created_at, cancel_requested_at,
        cancel_reason
    FROM leave_requests";

pub async fn load(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<RequestRow>> {
    sqlx::query_as(&format!("{SELECT} WHERE id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Move a request out of `pending` if it is still at `version`. False when someone else
/// decided first (or the version is stale).
pub async fn apply_decision(
    conn: &mut PgConnection,
    id: Uuid,
    version: i32,
    status: &str,
    decider: Uuid,
    comment: Option<&str>,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let done = sqlx::query(
        "UPDATE leave_requests
         SET status = $3::request_status, decided_by = $4, decided_at = $5,
             decision_comment = $6, version = version + 1, updated_at = $5
         WHERE id = $1 AND version = $2 AND status = 'pending'",
    )
    .bind(id)
    .bind(version)
    .bind(status)
    .bind(decider)
    .bind(now)
    .bind(comment)
    .execute(conn)
    .await?
    .rows_affected();
    Ok(done == 1)
}

/// Approved: one `leave_taken` row per leave year of the request's active days (FR-055).
pub async fn charge(
    conn: &mut PgConnection,
    r: &RequestRow,
    decider: Uuid,
) -> sqlx::Result<Vec<(i32, Decimal)>> {
    let years: Vec<(i32, Decimal)> = sqlx::query_as(
        "SELECT leave_year, SUM(portion) FROM leave_request_days
         WHERE leave_request_id = $1 AND active GROUP BY leave_year ORDER BY leave_year",
    )
    .bind(r.id)
    .fetch_all(&mut *conn)
    .await?;
    for (year, days) in &years {
        insert_entry(
            conn,
            &LedgerEntry {
                org_id: r.org_id,
                user_id: r.user_id,
                leave_type_id: r.leave_type_id,
                leave_year: *year,
                delta: -*days,
                reason: LedgerReason::LeaveTaken,
                ref_kind: Some("leave_request".into()),
                ref_id: Some(r.id),
                note: None,
                created_by: Some(decider),
            },
        )
        .await?;
    }
    Ok(years)
}

/// Rejected or withdrawn: the days are free again.
pub async fn release_days(conn: &mut PgConnection, request: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE leave_request_days SET active = false WHERE leave_request_id = $1")
        .bind(request)
        .execute(conn)
        .await?;
    Ok(())
}

/// Move a request from `from` to `to` (cancellation steps, FR-020), recording the reason and
/// time when it becomes `cancel_requested`. `version` is checked when given. False when the
/// request is no longer in `from`.
pub async fn move_status(
    conn: &mut PgConnection,
    id: Uuid,
    from: &str,
    to: &str,
    version: Option<i32>,
    cancel_reason: Option<&str>,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let requesting = to == "cancel_requested";
    let done = sqlx::query(
        "UPDATE leave_requests
         SET status = $3::request_status, version = version + 1, updated_at = $5,
             cancel_requested_at = CASE WHEN $6 THEN $5 ELSE cancel_requested_at END,
             cancel_reason = CASE WHEN $6 THEN $7 ELSE cancel_reason END
         WHERE id = $1 AND status = $2::request_status AND ($4::int IS NULL OR version = $4)",
    )
    .bind(id)
    .bind(from)
    .bind(to)
    .bind(version)
    .bind(now)
    .bind(requesting)
    .bind(cancel_reason)
    .execute(conn)
    .await?
    .rows_affected();
    Ok(done == 1)
}

/// The request's active days.
pub async fn active_days(conn: &mut PgConnection, request: Uuid) -> sqlx::Result<Vec<BookedDay>> {
    let rows: Vec<(NaiveDate, Decimal, Option<String>, i32)> = sqlx::query_as(
        "SELECT day, portion, half::text, leave_year FROM leave_request_days
         WHERE leave_request_id = $1 AND active ORDER BY day, half NULLS FIRST",
    )
    .bind(request)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(day, portion, half, leave_year)| BookedDay {
            day,
            portion,
            half: match half.as_deref() {
                Some("first") => Some(Half::First),
                Some("second") => Some(Half::Second),
                _ => None,
            },
            leave_year,
        })
        .collect())
}

/// Cancelled approved leave: give back the days dated on or after `from_day` with one
/// `leave_reversed` row per leave year, and free those days. Earlier days stay taken (FR-020).
pub async fn reverse_from(
    conn: &mut PgConnection,
    r: &RequestRow,
    actor: Uuid,
    from_day: NaiveDate,
) -> sqlx::Result<Vec<(i32, Decimal)>> {
    let days = active_days(conn, r.id).await?;
    let years = days_to_return(&days, from_day);
    sqlx::query(
        "UPDATE leave_request_days SET active = false WHERE leave_request_id = $1 AND day >= $2",
    )
    .bind(r.id)
    .bind(from_day)
    .execute(&mut *conn)
    .await?;
    for (year, days) in &years {
        insert_entry(
            conn,
            &LedgerEntry {
                org_id: r.org_id,
                user_id: r.user_id,
                leave_type_id: r.leave_type_id,
                leave_year: *year,
                delta: *days,
                reason: LedgerReason::LeaveReversed,
                ref_kind: Some("leave_request".into()),
                ref_id: Some(r.id),
                note: None,
                created_by: Some(actor),
            },
        )
        .await?;
    }
    Ok(years)
}

/// Every action link of the request stops working once it is decided.
pub async fn retire_action_tokens(
    conn: &mut PgConnection,
    request: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<()> {
    sqlx::query("UPDATE action_tokens SET used_at = $2 WHERE request_id = $1 AND used_at IS NULL")
        .bind(request)
        .bind(now)
        .execute(conn)
        .await?;
    Ok(())
}

/// Who decided, when and the resulting status (for `409 ALREADY_DECIDED`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Decided {
    pub status: String,
    pub decided_by: Option<Uuid>,
    pub decided_by_name: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
}

pub async fn decided(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Decided> {
    sqlx::query_as(
        "SELECT r.status::text AS status, r.decided_by, u.full_name AS decided_by_name, r.decided_at
         FROM leave_requests r LEFT JOIN users u ON u.id = r.decided_by WHERE r.id = $1",
    )
    .bind(id)
    .fetch_one(conn)
    .await
}

/// Pending requests `viewer` may decide, oldest first (FR-022): admins see all; approvers see
/// requests routed to a group they belong to, or to no group. Never their own.
pub async fn pending_for(
    conn: &mut PgConnection,
    viewer: Uuid,
    is_admin: bool,
) -> sqlx::Result<Vec<RequestRow>> {
    sqlx::query_as(&format!(
        "{SELECT} r WHERE r.status = 'pending' AND r.user_id <> $1 AND {ELIGIBLE}
         ORDER BY r.created_at, r.id"
    ))
    .bind(viewer)
    .bind(is_admin)
    .fetch_all(conn)
    .await
}

/// Who may decide a request for viewer `$1` (admin flag `$2`), as in [`pending_for`].
const ELIGIBLE: &str = "($2 OR r.routed_group_id IS NULL OR EXISTS (
        SELECT 1 FROM approver_group_members m
        WHERE m.group_id = r.routed_group_id AND m.user_id = $1))";

/// Cancellations of started leave waiting for `viewer`, oldest request first (FR-020).
pub async fn cancel_requests_for(
    conn: &mut PgConnection,
    viewer: Uuid,
    is_admin: bool,
) -> sqlx::Result<Vec<RequestRow>> {
    sqlx::query_as(&format!(
        "{SELECT} r WHERE r.status = 'cancel_requested' AND r.user_id <> $1 AND {ELIGIBLE}
         ORDER BY r.cancel_requested_at, r.id"
    ))
    .bind(viewer)
    .bind(is_admin)
    .fetch_all(conn)
    .await
}

/// A teammate's approved leave overlapping a request.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TeamAway {
    pub user_id: Uuid,
    pub full_name: String,
    pub group_id: Option<Uuid>,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub leave_type_id: Uuid,
    pub code: String,
    pub name: String,
    pub color: String,
    pub reason: String,
}

/// Approved leave of the requester's team (same current approver group or department)
/// overlapping `from..=to`, excluding the requester.
pub async fn team_away(
    conn: &mut PgConnection,
    requester: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> sqlx::Result<Vec<TeamAway>> {
    sqlx::query_as(
        "SELECT u.id AS user_id, u.full_name, u.approver_group_id AS group_id,
                r.from_date, r.to_date, t.id AS leave_type_id, t.code, t.name, t.color, r.reason
         FROM leave_requests r
         JOIN users u ON u.id = r.user_id
         JOIN leave_types t ON t.id = r.leave_type_id
         JOIN users me ON me.id = $1
         WHERE r.status IN ('approved', 'cancel_requested') AND r.user_id <> $1
           AND r.from_date <= $3 AND r.to_date >= $2
           AND ((me.approver_group_id IS NOT NULL AND u.approver_group_id = me.approver_group_id)
             OR (me.department IS NOT NULL AND u.department = me.department))
         ORDER BY r.from_date, u.full_name",
    )
    .bind(requester)
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await
}

/// Groups `user` is an approving member of.
pub async fn groups_of(conn: &mut PgConnection, user: Uuid) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar("SELECT group_id FROM approver_group_members WHERE user_id = $1")
        .bind(user)
        .fetch_all(conn)
        .await
}

/// Notified members of a request: `(email, user_id)`.
pub async fn notified(
    conn: &mut PgConnection,
    request: Uuid,
) -> sqlx::Result<Vec<(String, Option<Uuid>)>> {
    sqlx::query_as(
        "SELECT email::text, user_id FROM request_notifications
         WHERE request_kind = 'leave' AND request_id = $1 ORDER BY email",
    )
    .bind(request)
    .fetch_all(conn)
    .await
}
