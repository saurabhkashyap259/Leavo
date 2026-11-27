//! Leave requests (T103): the context the engine needs (calendar, holidays, balances, booked
//! days), inserting a request with its days under the requester's locks, and eligibility.
//! Every function runs inside an org-scoped transaction.
use crate::leave_types_repo::LeaveTypeRow;
use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use leavo_core::approval::{route_approvers, Routing};
use leavo_core::balance::prorate;
use leavo_core::calendar::{
    leave_year_bounds, leave_year_of, today_in, BookedDay, Half, Holiday, HolidayKind, WeekendMask,
};
use leavo_core::error::ErrorCode;
use leavo_core::policy::{Availability, ExistingDay};
use rust_decimal::Decimal;
use sqlx::PgConnection;
use std::collections::BTreeMap;
use uuid::Uuid;

/// The organization's calendar settings and today's date in its time zone.
#[derive(Debug, Clone, Copy)]
pub struct OrgCalendar {
    pub today: NaiveDate,
    pub weekend: WeekendMask,
    pub start_month: u32,
}

impl OrgCalendar {
    pub fn current_leave_year(&self) -> i32 {
        leave_year_of(self.today, self.start_month)
    }
}

pub async fn org_calendar(
    conn: &mut PgConnection,
    org: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<OrgCalendar> {
    let (tz, mask, start): (String, i16, i16) = sqlx::query_as(
        "SELECT timezone, weekend_mask, leave_year_start_month FROM organizations WHERE id = $1",
    )
    .bind(org)
    .fetch_one(conn)
    .await?;
    let tz: Tz = tz.parse().unwrap_or(chrono_tz::Asia::Kolkata);
    Ok(OrgCalendar {
        today: today_in(tz, now),
        weekend: WeekendMask::from_bits(mask).unwrap_or_default(),
        start_month: start as u32,
    })
}

fn parse_half(v: Option<String>) -> Option<Half> {
    match v.as_deref() {
        Some("first") => Some(Half::First),
        Some("second") => Some(Half::Second),
        _ => None,
    }
}

pub async fn holidays_between(
    conn: &mut PgConnection,
    from: NaiveDate,
    to: NaiveDate,
) -> sqlx::Result<Vec<Holiday>> {
    let rows: Vec<(NaiveDate, String, String)> = sqlx::query_as(
        "SELECT date, name, kind::text FROM holidays WHERE date BETWEEN $1 AND $2 ORDER BY date, name",
    )
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(date, name, kind)| Holiday {
            date,
            name,
            kind: if kind == "mandatory" {
                HolidayKind::Mandatory
            } else {
                HolidayKind::Optional
            },
        })
        .collect())
}

/// The user's booked days (pending, approved or awaiting cancellation) in a date range.
pub async fn existing_days(
    conn: &mut PgConnection,
    user: Uuid,
    from: NaiveDate,
    to: NaiveDate,
) -> sqlx::Result<Vec<ExistingDay>> {
    let rows: Vec<(NaiveDate, Option<String>)> = sqlx::query_as(
        "SELECT d.day, d.half::text FROM leave_request_days d
         JOIN leave_requests r ON r.id = d.leave_request_id
         WHERE d.user_id = $1 AND d.active AND d.day BETWEEN $2 AND $3
           AND r.status IN ('pending', 'approved', 'cancel_requested')",
    )
    .bind(user)
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(day, half)| ExistingDay {
            day,
            half: parse_half(half),
        })
        .collect())
}

pub async fn has_active_grant(
    conn: &mut PgConnection,
    leave_type: Uuid,
    user: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM leave_type_grants
                        WHERE leave_type_id = $1 AND user_id = $2 AND revoked_at IS NULL)",
    )
    .bind(leave_type)
    .bind(user)
    .fetch_one(conn)
    .await
}

/// Whether the type can be requested by `user`: active, and granted when restricted.
pub async fn requestable(
    conn: &mut PgConnection,
    t: &LeaveTypeRow,
    user: Uuid,
) -> sqlx::Result<bool> {
    if !t.active {
        return Ok(false);
    }
    if t.availability == "restricted" {
        return has_active_grant(conn, t.id, user).await;
    }
    Ok(true)
}

/// Allotment a leave year that has not started yet will bring (FR-055): the quota (the
/// pending quota if one is scheduled), pro-rated by joining date, or the full quota for a
/// granted restricted type. Zero for the current or past years, for comp-off, and once
/// allotment rows exist for that year (the rollover has written them).
pub async fn projected_allotment(
    conn: &mut PgConnection,
    user: Uuid,
    t: &LeaveTypeRow,
    leave_year: i32,
    cal: &OrgCalendar,
) -> sqlx::Result<Decimal> {
    if leave_year <= cal.current_leave_year() || t.is_comp_off || !t.active {
        return Ok(Decimal::ZERO);
    }
    let Some(quota) = t.pending_yearly_quota.or(t.yearly_quota) else {
        return Ok(Decimal::ZERO);
    };
    let allotted: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM balance_ledger
                        WHERE user_id = $1 AND leave_type_id = $2 AND leave_year = $3
                          AND reason IN ('allotment', 'prorate'))",
    )
    .bind(user)
    .bind(t.id)
    .bind(leave_year)
    .fetch_one(&mut *conn)
    .await?;
    if allotted {
        return Ok(Decimal::ZERO);
    }
    if t.availability == "restricted" {
        let granted = has_active_grant(conn, t.id, user).await?;
        return Ok(if granted { quota } else { Decimal::ZERO });
    }
    let doj: Option<NaiveDate> =
        sqlx::query_scalar("SELECT date_of_joining FROM users WHERE id = $1")
            .bind(user)
            .fetch_one(&mut *conn)
            .await?;
    let (year_start, _) = leave_year_bounds(leave_year, cal.start_month);
    Ok(prorate(quota, doj, year_start, cal.start_month))
}

/// `v_balances` figures for one user, type and leave year (`None` for comp-off).
#[derive(Debug, Clone, Default, PartialEq, sqlx::FromRow)]
pub struct BalanceRow {
    pub allotted: Decimal,
    pub carried: Decimal,
    pub taken: Decimal,
    pub adjusted: Decimal,
    pub balance: Decimal,
    pub pending: Decimal,
    pub available: Decimal,
}

pub async fn balance_row(
    conn: &mut PgConnection,
    user: Uuid,
    leave_type: Uuid,
    leave_year: Option<i32>,
) -> sqlx::Result<BalanceRow> {
    let row: Option<BalanceRow> = sqlx::query_as(
        "SELECT allotted, carried, taken, adjusted, balance, pending, available FROM v_balances
         WHERE user_id = $1 AND leave_type_id = $2 AND leave_year IS NOT DISTINCT FROM $3",
    )
    .bind(user)
    .bind(leave_type)
    .bind(leave_year)
    .fetch_optional(conn)
    .await?;
    Ok(row.unwrap_or_default())
}

/// Balance including a future year's projected allotment (added to `allotted`, `balance` and
/// `available`). Comp-off is one pool, whatever `leave_year` says.
pub async fn balance_with_projection(
    conn: &mut PgConnection,
    user: Uuid,
    t: &LeaveTypeRow,
    leave_year: i32,
    cal: &OrgCalendar,
) -> sqlx::Result<BalanceRow> {
    let year = (!t.is_comp_off).then_some(leave_year);
    let mut row = balance_row(conn, user, t.id, year).await?;
    let projected = projected_allotment(conn, user, t, leave_year, cal).await?;
    row.allotted += projected;
    row.balance += projected;
    row.available += projected;
    Ok(row)
}

/// What the user can still spend, per leave year touched by the request.
pub async fn availability(
    conn: &mut PgConnection,
    user: Uuid,
    t: &LeaveTypeRow,
    years: &[i32],
    cal: &OrgCalendar,
) -> sqlx::Result<Availability> {
    if t.is_comp_off {
        let row = balance_row(conn, user, t.id, None).await?;
        return Ok(Availability::Pool(row.available));
    }
    let mut map = BTreeMap::new();
    for &year in years {
        let row = balance_with_projection(conn, user, t, year, cal).await?;
        map.insert(year, row.available);
    }
    Ok(Availability::PerYear(map))
}

/// Serialise request submission for one employee across all leave types, so overlap checks
/// (a full day against a half day of another type) cannot race (FR-019).
pub async fn lock_requester(conn: &mut PgConnection, user: Uuid) -> sqlx::Result<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("requests:{user}"))
        .execute(conn)
        .await?;
    Ok(())
}

/// Active users who can decide a request: the routed group's approvers and admins (or every
/// approver when there is no group) plus every admin, never the requester (FR-022).
pub async fn eligible_approvers(
    conn: &mut PgConnection,
    routed_group: Option<Uuid>,
    requester: Uuid,
) -> sqlx::Result<Result<Vec<Uuid>, ErrorCode>> {
    let people: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id, role::text FROM users
         WHERE status = 'active' AND role IN ('approver', 'admin')",
    )
    .fetch_all(&mut *conn)
    .await?;
    let approvers: Vec<Uuid> = people
        .iter()
        .filter(|(_, r)| r == "approver")
        .map(|(id, _)| *id)
        .collect();
    let admins: Vec<Uuid> = people
        .iter()
        .filter(|(_, r)| r == "admin")
        .map(|(id, _)| *id)
        .collect();
    let members: Option<Vec<Uuid>> = match routed_group {
        None => None,
        Some(g) => Some(
            sqlx::query_scalar(
                "SELECT m.user_id FROM approver_group_members m
                 JOIN users u ON u.id = m.user_id
                 WHERE m.group_id = $1 AND u.status = 'active' AND u.role IN ('approver', 'admin')",
            )
            .bind(g)
            .fetch_all(&mut *conn)
            .await?,
        ),
    };
    Ok(route_approvers(&Routing {
        requester,
        group_members: members.as_deref(),
        approvers: &approvers,
        admins: &admins,
    }))
}

pub struct NewLeaveRequest<'a> {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub leave_type_id: Uuid,
    pub from: NaiveDate,
    pub to: NaiveDate,
    pub start_half: Option<Half>,
    pub end_half: Option<Half>,
    pub days: Decimal,
    pub reason: &'a str,
    pub routed_group_id: Option<Uuid>,
    pub now: DateTime<Utc>,
}

/// Insert a pending request and its booked days. Call under [`lock_requester`] and
/// `lock_balance`; the partial unique index still guards against a double booking.
pub async fn insert_request(
    conn: &mut PgConnection,
    r: &NewLeaveRequest<'_>,
    booked: &[BookedDay],
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO leave_requests
             (id, org_id, user_id, leave_type_id, from_date, to_date, start_half, end_half,
              days, reason, routed_group_id, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7::day_half, $8::day_half, $9, $10, $11, $12, $12)",
    )
    .bind(r.id)
    .bind(r.org_id)
    .bind(r.user_id)
    .bind(r.leave_type_id)
    .bind(r.from)
    .bind(r.to)
    .bind(r.start_half.map(Half::as_str))
    .bind(r.end_half.map(Half::as_str))
    .bind(r.days)
    .bind(r.reason)
    .bind(r.routed_group_id)
    .bind(r.now)
    .execute(&mut *conn)
    .await?;
    for b in booked {
        sqlx::query(
            "INSERT INTO leave_request_days
                 (id, leave_request_id, org_id, user_id, day, leave_year, portion, half)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::day_half)",
        )
        .bind(Uuid::now_v7())
        .bind(r.id)
        .bind(r.org_id)
        .bind(r.user_id)
        .bind(b.day)
        .bind(b.leave_year)
        .bind(b.portion)
        .bind(b.half.map(Half::as_str))
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// A notified member of a request: a colleague (`user_id`) or an external address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notified {
    pub user_id: Option<Uuid>,
    pub email: String,
}

pub async fn insert_notifications(
    conn: &mut PgConnection,
    org: Uuid,
    request: Uuid,
    kind: &str,
    members: &[Notified],
) -> sqlx::Result<()> {
    for m in members {
        sqlx::query(
            "INSERT INTO request_notifications (id, org_id, request_id, request_kind, user_id, email)
             VALUES ($1, $2, $3, $4::request_kind, $5, $6)
             ON CONFLICT DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(org)
        .bind(request)
        .bind(kind)
        .bind(m.user_id)
        .bind(&m.email)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Pending days per leave year for a user and type (from `v_balances`).
pub async fn pending_by_year(
    conn: &mut PgConnection,
    user: Uuid,
    leave_type: Uuid,
) -> sqlx::Result<Vec<(Option<i32>, Decimal)>> {
    sqlx::query_as(
        "SELECT leave_year, pending FROM v_balances
         WHERE user_id = $1 AND leave_type_id = $2 AND pending <> 0
         ORDER BY leave_year NULLS FIRST",
    )
    .bind(user)
    .bind(leave_type)
    .fetch_all(conn)
    .await
}

/// Whether the organization has any leave request (LEAVE_YEAR_LOCKED, FR-062).
pub async fn org_has_requests(conn: &mut PgConnection) -> sqlx::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM leave_requests)")
        .fetch_one(conn)
        .await
}

/// Filters of `/me/leaves` (FR-028). `year` is a leave year: a request belongs to every leave
/// year one of its days falls in.
#[derive(Debug, Clone, Copy, Default)]
pub struct HistoryFilter<'a> {
    pub year: Option<i32>,
    pub leave_type: Option<Uuid>,
    pub status: Option<&'a str>,
    /// Keyset cursor: rows strictly after `(from_date, id)` in newest-first order.
    pub after: Option<(NaiveDate, Uuid)>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HistoryRow {
    pub id: Uuid,
    pub type_id: Uuid,
    pub code: String,
    pub type_name: String,
    pub color: String,
    pub from_date: NaiveDate,
    pub to_date: NaiveDate,
    pub start_half: Option<String>,
    pub end_half: Option<String>,
    pub days: Decimal,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub decided_by: Option<Uuid>,
    pub decided_name: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub reason: String,
    pub decision_comment: Option<String>,
    pub attachment_id: Option<Uuid>,
    pub attachment_name: Option<String>,
}

/// `user`'s leave requests, newest first (by start date, then id).
pub async fn history(
    conn: &mut PgConnection,
    user: Uuid,
    f: &HistoryFilter<'_>,
) -> sqlx::Result<Vec<HistoryRow>> {
    sqlx::query_as(
        "SELECT r.id, t.id AS type_id, t.code, t.name AS type_name, t.color,
                r.from_date, r.to_date, r.start_half::text AS start_half,
                r.end_half::text AS end_half, r.days, r.status::text AS status, r.created_at,
                r.decided_by, d.full_name AS decided_name, r.decided_at, r.reason,
                r.decision_comment, a.id AS attachment_id, a.file_name AS attachment_name
         FROM leave_requests r
         JOIN leave_types t ON t.id = r.leave_type_id
         LEFT JOIN users d ON d.id = r.decided_by
         LEFT JOIN attachments a ON a.request_id = r.id
         WHERE r.user_id = $1
           AND ($2::int IS NULL OR EXISTS (
                SELECT 1 FROM leave_request_days x
                WHERE x.leave_request_id = r.id AND x.leave_year = $2))
           AND ($3::uuid IS NULL OR r.leave_type_id = $3)
           AND ($4::text IS NULL OR r.status::text = $4)
           AND ($5::date IS NULL OR (r.from_date, r.id) < ($5, $6))
         ORDER BY r.from_date DESC, r.id DESC
         LIMIT $7",
    )
    .bind(user)
    .bind(f.year)
    .bind(f.leave_type)
    .bind(f.status)
    .bind(f.after.map(|a| a.0))
    .bind(f.after.map(|a| a.1))
    .bind(f.limit)
    .fetch_all(conn)
    .await
}

/// Notified people per request: a member's current name, otherwise the email address.
pub async fn notified_names(
    conn: &mut PgConnection,
    requests: &[Uuid],
) -> sqlx::Result<BTreeMap<Uuid, Vec<String>>> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT n.request_id, COALESCE(u.full_name, n.email::text)
         FROM request_notifications n
         LEFT JOIN users u ON u.id = n.user_id
         WHERE n.request_kind = 'leave' AND n.request_id = ANY($1)
         ORDER BY n.request_id, n.email",
    )
    .bind(requests)
    .fetch_all(conn)
    .await?;
    let mut out: BTreeMap<Uuid, Vec<String>> = BTreeMap::new();
    for (id, name) in rows {
        out.entry(id).or_default().push(name);
    }
    Ok(out)
}

/// The organization's time zone.
pub async fn org_timezone(conn: &mut PgConnection, org: Uuid) -> sqlx::Result<Tz> {
    let tz: String = sqlx::query_scalar("SELECT timezone FROM organizations WHERE id = $1")
        .bind(org)
        .fetch_one(conn)
        .await?;
    Ok(tz.parse().unwrap_or(chrono_tz::Asia::Kolkata))
}
