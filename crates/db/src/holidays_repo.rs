//! Holidays (T162, FR-033–FR-035): CRUD and the side effects of a change on booked leave.
//!
//! Callers record which dates were mandatory holidays before a change
//! ([`mandatory_dates`]), make the change, then call [`apply_holiday_change`]. A date that
//! became a mandatory holiday frees the leave booked on it; a date that stopped being one is
//! booked again for every pending or approved request covering it. Runs in org-scoped
//! transactions.
use crate::comp_off_repo;
use crate::leaves_repo::org_calendar;
use crate::ledger::{insert_entry, LedgerEntry, LedgerReason};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use leavo_core::calendar::leave_year_of;
use rust_decimal::Decimal;
use sqlx::PgConnection;
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct HolidayRow {
    pub id: Uuid,
    pub date: NaiveDate,
    pub name: String,
    /// `mandatory` or `optional`.
    pub kind: String,
    pub location: Option<String>,
}

const SELECT: &str = "SELECT id, date, name, kind::text AS kind, location FROM holidays";

pub async fn between(
    conn: &mut PgConnection,
    from: NaiveDate,
    to: NaiveDate,
) -> sqlx::Result<Vec<HolidayRow>> {
    sqlx::query_as(&format!(
        "{SELECT} WHERE date BETWEEN $1 AND $2 ORDER BY date, name"
    ))
    .bind(from)
    .bind(to)
    .fetch_all(conn)
    .await
}

pub async fn get(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<HolidayRow>> {
    sqlx::query_as(&format!("{SELECT} WHERE id = $1 FOR UPDATE"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// Is there already a holiday with this date and name (ignoring case), other than `except`?
pub async fn exists(
    conn: &mut PgConnection,
    date: NaiveDate,
    name: &str,
    except: Option<Uuid>,
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM holidays
                        WHERE date = $1 AND lower(name) = lower($2) AND id IS DISTINCT FROM $3)",
    )
    .bind(date)
    .bind(name)
    .bind(except)
    .fetch_one(conn)
    .await
}

pub struct NewHoliday<'a> {
    pub date: NaiveDate,
    pub name: &'a str,
    pub kind: &'a str,
    pub location: Option<&'a str>,
    pub source: &'a str,
}

pub async fn insert(
    conn: &mut PgConnection,
    org: Uuid,
    h: &NewHoliday<'_>,
) -> sqlx::Result<HolidayRow> {
    sqlx::query_as(
        "INSERT INTO holidays (id, org_id, date, name, kind, location, source)
         VALUES ($1, $2, $3, $4, $5::holiday_kind, $6, $7)
         RETURNING id, date, name, kind::text AS kind, location",
    )
    .bind(Uuid::now_v7())
    .bind(org)
    .bind(h.date)
    .bind(h.name)
    .bind(h.kind)
    .bind(h.location)
    .bind(h.source)
    .fetch_one(conn)
    .await
}

pub async fn update(conn: &mut PgConnection, h: &HolidayRow) -> sqlx::Result<HolidayRow> {
    sqlx::query_as(
        "UPDATE holidays SET date = $2, name = $3, kind = $4::holiday_kind, location = $5
         WHERE id = $1 RETURNING id, date, name, kind::text AS kind, location",
    )
    .bind(h.id)
    .bind(h.date)
    .bind(&h.name)
    .bind(&h.kind)
    .bind(&h.location)
    .fetch_one(conn)
    .await
}

pub async fn delete(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<()> {
    sqlx::query("DELETE FROM holidays WHERE id = $1")
        .bind(id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Which of `dates` are mandatory holidays now.
pub async fn mandatory_dates(
    conn: &mut PgConnection,
    dates: &[NaiveDate],
) -> sqlx::Result<BTreeSet<NaiveDate>> {
    let rows: Vec<NaiveDate> = sqlx::query_scalar(
        "SELECT DISTINCT date FROM holidays WHERE kind = 'mandatory' AND date = ANY($1)",
    )
    .bind(dates)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().collect())
}

/// What a holiday change did to one request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Affected {
    pub request: Uuid,
    pub user_id: Uuid,
    pub leave_type_id: Uuid,
    /// The request's status (`pending`, `approved` or `cancel_requested`).
    pub status: String,
    pub day: NaiveDate,
    /// Days freed (positive) or booked again (negative).
    pub delta: Decimal,
}

const OPEN: &str = "r.status IN ('pending', 'approved', 'cancel_requested')";

/// Apply FR-035 for `dates`, given which of them were mandatory holidays before the change.
/// Approved (and cancel-requested) leave gets ledger rows; pending leave is only recounted.
/// `days` on each request follows its booked days.
pub async fn apply_holiday_change(
    conn: &mut PgConnection,
    org: Uuid,
    actor: Uuid,
    dates: &[NaiveDate],
    before: &BTreeSet<NaiveDate>,
    now: DateTime<Utc>,
) -> sqlx::Result<Vec<Affected>> {
    let after = mandatory_dates(conn, dates).await?;
    let cal = org_calendar(conn, org, now).await?;
    let mut out = Vec::new();
    let unique: BTreeSet<NaiveDate> = dates.iter().copied().collect();
    for day in unique {
        match (before.contains(&day), after.contains(&day)) {
            (false, true) => out.extend(release(conn, org, actor, day, now, cal.today).await?),
            (true, false) if !cal.weekend.contains(day.weekday()) => {
                out.extend(book(conn, org, actor, day, cal.start_month, cal.today).await?)
            }
            _ => {}
        }
    }
    Ok(out)
}

async fn set_days(conn: &mut PgConnection, request: Uuid, delta: Decimal) -> sqlx::Result<()> {
    // `days` must stay positive: a request whose only day became a holiday keeps its count.
    sqlx::query(
        "UPDATE leave_requests SET days = days + $2, version = version + 1
         WHERE id = $1 AND days + $2 > 0",
    )
    .bind(request)
    .bind(delta)
    .execute(conn)
    .await?;
    Ok(())
}

/// The day became a mandatory holiday: free it.
async fn release(
    conn: &mut PgConnection,
    org: Uuid,
    actor: Uuid,
    day: NaiveDate,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> sqlx::Result<Vec<Affected>> {
    let rows: Vec<(Uuid, Uuid, Uuid, String, Decimal, i32)> = sqlx::query_as(&format!(
        "UPDATE leave_request_days d SET active = false
         FROM leave_requests r
         WHERE d.leave_request_id = r.id AND d.active AND d.day = $1 AND {OPEN}
         RETURNING r.id, r.user_id, r.leave_type_id, r.status::text, d.portion, d.leave_year"
    ))
    .bind(day)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for (request, user, leave_type, status, portion, year) in rows {
        set_days(conn, request, -portion).await?;
        if status == "pending" {
            comp_off_repo::release_part(conn, request, portion, Some(actor), now, today).await?;
        } else {
            insert_entry(
                conn,
                &LedgerEntry {
                    org_id: org,
                    user_id: user,
                    leave_type_id: leave_type,
                    leave_year: year,
                    delta: portion,
                    reason: LedgerReason::LeaveReversed,
                    ref_kind: Some("leave_request".into()),
                    ref_id: Some(request),
                    note: Some(format!("Holiday added on {day}")),
                    created_by: Some(actor),
                },
            )
            .await?;
            comp_off_repo::give_back(conn, request, portion, Some(actor), now, today).await?;
        }
        out.push(Affected {
            request,
            user_id: user,
            leave_type_id: leave_type,
            status,
            day,
            delta: portion,
        });
    }
    Ok(out)
}

/// The day is a working day again: book it for every open request covering it, as the half
/// the request asked for on its first or last day.
async fn book(
    conn: &mut PgConnection,
    org: Uuid,
    actor: Uuid,
    day: NaiveDate,
    start_month: u32,
    today: NaiveDate,
) -> sqlx::Result<Vec<Affected>> {
    type Open = (
        Uuid,
        Uuid,
        Uuid,
        String,
        NaiveDate,
        NaiveDate,
        Option<String>,
        Option<String>,
    );
    let open: Vec<Open> = sqlx::query_as(&format!(
        "SELECT r.id, r.user_id, r.leave_type_id, r.status::text, r.from_date, r.to_date,
                r.start_half::text, r.end_half::text
         FROM leave_requests r
         WHERE {OPEN} AND r.from_date <= $1 AND r.to_date >= $1
           AND NOT EXISTS (SELECT 1 FROM leave_request_days d
                           WHERE d.user_id = r.user_id AND d.day = $1 AND d.active)
         ORDER BY r.created_at, r.id"
    ))
    .bind(day)
    .fetch_all(&mut *conn)
    .await?;
    let year = leave_year_of(day, start_month);
    let mut out = Vec::new();
    for (request, user, leave_type, status, from, to, start_half, end_half) in open {
        let half = if day == from && start_half.is_some() {
            start_half
        } else if day == to && end_half.is_some() {
            end_half
        } else {
            None
        };
        let portion = if half.is_some() {
            Decimal::new(5, 1)
        } else {
            Decimal::ONE
        };
        let inserted = sqlx::query(
            "INSERT INTO leave_request_days
                (id, leave_request_id, org_id, user_id, day, leave_year, portion, half)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::day_half)
             ON CONFLICT DO NOTHING",
        )
        .bind(Uuid::now_v7())
        .bind(request)
        .bind(org)
        .bind(user)
        .bind(day)
        .bind(year)
        .bind(portion)
        .bind(&half)
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if inserted == 0 {
            continue;
        }
        set_days(conn, request, portion).await?;
        // Comp-off leave: hold (pending) or use (approved) credit for the day too.
        if status == "pending" {
            comp_off_repo::reserve(conn, org, user, request, portion, today).await?;
        } else {
            comp_off_repo::consume(conn, org, user, request, portion, today).await?;
        }
        if status != "pending" {
            // Charged even if the balance goes negative (FR-035).
            insert_entry(
                conn,
                &LedgerEntry {
                    org_id: org,
                    user_id: user,
                    leave_type_id: leave_type,
                    leave_year: year,
                    delta: -portion,
                    reason: LedgerReason::LeaveTaken,
                    ref_kind: Some("leave_request".into()),
                    ref_id: Some(request),
                    note: Some(format!("Holiday removed on {day}")),
                    created_by: Some(actor),
                },
            )
            .await?;
        }
        out.push(Affected {
            request,
            user_id: user,
            leave_type_id: leave_type,
            status,
            day,
            delta: -portion,
        });
    }
    Ok(out)
}
