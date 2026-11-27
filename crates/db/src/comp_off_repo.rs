//! Comp-off (T151, FR-030–FR-032, FR-060, FR-061): requests and claimed dates, credits with
//! expiry, reservations by pending comp-off leave, usage by approved leave, expiry and
//! revocation. Every ledger row is written against the org's comp-off leave type; balances
//! ignore its leave year (one pool). Runs in org-scoped transactions.
use crate::ledger::{insert_entry, LedgerEntry, LedgerReason};
use chrono::{DateTime, Days, NaiveDate, Utc};
use leavo_core::calendar::leave_year_of;
use leavo_core::comp_off::{allocate, expirable, CreditLeft};
use rust_decimal::Decimal;
use sqlx::PgConnection;
use uuid::Uuid;

/// The org's comp-off leave type (exactly one per org), if it is active.
pub async fn comp_off_type(conn: &mut PgConnection) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM leave_types WHERE is_comp_off AND active")
        .fetch_optional(conn)
        .await
}

async fn type_id(conn: &mut PgConnection) -> sqlx::Result<Uuid> {
    sqlx::query_scalar("SELECT id FROM leave_types WHERE is_comp_off")
        .fetch_one(conn)
        .await
}

/// The org's `(comp_off_expiry_days, leave_year_start_month)`.
async fn org_settings(conn: &mut PgConnection, org: Uuid) -> sqlx::Result<(i32, i32)> {
    sqlx::query_as(
        "SELECT comp_off_expiry_days, leave_year_start_month::int FROM organizations WHERE id = $1",
    )
    .bind(org)
    .fetch_one(conn)
    .await
}

// ---------------------------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------------------------

pub struct NewCompOffRequest<'a> {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub reason: &'a str,
    pub routed_group_id: Option<Uuid>,
    pub now: DateTime<Utc>,
}

/// Dates among `dates` that `user` has already claimed in a pending or approved request.
pub async fn claimed(
    conn: &mut PgConnection,
    user: Uuid,
    dates: &[NaiveDate],
) -> sqlx::Result<Vec<NaiveDate>> {
    sqlx::query_scalar(
        "SELECT worked_date FROM comp_off_request_dates
         WHERE user_id = $1 AND active AND worked_date = ANY($2) ORDER BY worked_date",
    )
    .bind(user)
    .bind(dates)
    .fetch_all(conn)
    .await
}

/// Insert a pending request with its dates. A date claimed concurrently fails with a unique
/// violation on `comp_off_request_dates_claim_idx`.
pub async fn insert_request(
    conn: &mut PgConnection,
    r: &NewCompOffRequest<'_>,
    dates: &[(NaiveDate, Decimal)],
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO comp_off_requests (id, org_id, user_id, reason, routed_group_id, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $6)",
    )
    .bind(r.id)
    .bind(r.org_id)
    .bind(r.user_id)
    .bind(r.reason)
    .bind(r.routed_group_id)
    .bind(r.now)
    .execute(&mut *conn)
    .await?;
    for (date, portion) in dates {
        sqlx::query(
            "INSERT INTO comp_off_request_dates
                (id, comp_off_request_id, org_id, user_id, worked_date, portion)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(r.id)
        .bind(r.org_id)
        .bind(r.user_id)
        .bind(date)
        .bind(portion)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CompOffRow {
    pub id: Uuid,
    pub org_id: Uuid,
    pub user_id: Uuid,
    pub reason: String,
    pub status: String,
    pub routed_group_id: Option<Uuid>,
    pub decided_by: Option<Uuid>,
    pub decided_by_name: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_comment: Option<String>,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    /// Earliest and latest worked date, and the total days.
    pub first_date: NaiveDate,
    pub last_date: NaiveDate,
    pub days: Decimal,
}

const SELECT: &str = "SELECT r.id, r.org_id, r.user_id, r.reason, r.status::text AS status,
        r.routed_group_id, r.decided_by, u.full_name AS decided_by_name, r.decided_at,
        r.decision_comment, r.version, r.created_at,
        (SELECT min(worked_date) FROM comp_off_request_dates d WHERE d.comp_off_request_id = r.id) AS first_date,
        (SELECT max(worked_date) FROM comp_off_request_dates d WHERE d.comp_off_request_id = r.id) AS last_date,
        (SELECT sum(portion) FROM comp_off_request_dates d WHERE d.comp_off_request_id = r.id) AS days
    FROM comp_off_requests r LEFT JOIN users u ON u.id = r.decided_by";

pub async fn load(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<CompOffRow>> {
    sqlx::query_as(&format!("{SELECT} WHERE r.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// `(worked_date, portion)` of a request, in date order.
pub async fn dates_of(
    conn: &mut PgConnection,
    request: Uuid,
) -> sqlx::Result<Vec<(NaiveDate, Decimal)>> {
    sqlx::query_as(
        "SELECT worked_date, portion FROM comp_off_request_dates
         WHERE comp_off_request_id = $1 ORDER BY worked_date",
    )
    .bind(request)
    .fetch_all(conn)
    .await
}

/// The user's requests, newest first.
pub async fn requests_of(conn: &mut PgConnection, user: Uuid) -> sqlx::Result<Vec<CompOffRow>> {
    sqlx::query_as(&format!(
        "{SELECT} WHERE r.user_id = $1 ORDER BY r.created_at DESC, r.id DESC"
    ))
    .bind(user)
    .fetch_all(conn)
    .await
}

/// Pending requests `viewer` may decide, oldest first (same eligibility as leave, FR-022).
pub async fn pending_for(
    conn: &mut PgConnection,
    viewer: Uuid,
    is_admin: bool,
) -> sqlx::Result<Vec<CompOffRow>> {
    sqlx::query_as(&format!(
        "{SELECT} WHERE r.status = 'pending' AND r.user_id <> $1
           AND ($2 OR r.routed_group_id IS NULL OR EXISTS (
                SELECT 1 FROM approver_group_members m
                WHERE m.group_id = r.routed_group_id AND m.user_id = $1))
         ORDER BY r.created_at, r.id"
    ))
    .bind(viewer)
    .bind(is_admin)
    .fetch_all(conn)
    .await
}

/// Move a pending request to `to` if still at `version` (when given). Decisions record who and
/// when; a withdrawal does not. False when it is no longer pending.
pub async fn move_status(
    conn: &mut PgConnection,
    id: Uuid,
    to: &str,
    version: Option<i32>,
    decider: Option<Uuid>,
    comment: Option<&str>,
    now: DateTime<Utc>,
) -> sqlx::Result<bool> {
    let done = sqlx::query(
        "UPDATE comp_off_requests
         SET status = $2::request_status, version = version + 1, updated_at = $5,
             decided_by = COALESCE($4, decided_by),
             decided_at = CASE WHEN $4 IS NULL THEN decided_at ELSE $5 END,
             decision_comment = CASE WHEN $4 IS NULL THEN decision_comment ELSE $6 END
         WHERE id = $1 AND status = 'pending' AND ($3::int IS NULL OR version = $3)",
    )
    .bind(id)
    .bind(to)
    .bind(version)
    .bind(decider)
    .bind(now)
    .bind(comment)
    .execute(conn)
    .await?
    .rows_affected();
    Ok(done == 1)
}

/// Rejected or withdrawn: the dates can be claimed again.
pub async fn free_dates(conn: &mut PgConnection, request: Uuid) -> sqlx::Result<()> {
    sqlx::query("UPDATE comp_off_request_dates SET active = false WHERE comp_off_request_id = $1")
        .bind(request)
        .execute(conn)
        .await?;
    Ok(())
}

/// Approved: one credit per date, expiring `comp_off_expiry_days` after the worked date, and a
/// `comp_off_credit` ledger row each (FR-032). Returns the total days credited.
pub async fn create_credits(
    conn: &mut PgConnection,
    r: &CompOffRow,
    approver: Uuid,
    now: DateTime<Utc>,
) -> sqlx::Result<Decimal> {
    let (expiry_days, start_month) = org_settings(conn, r.org_id).await?;
    let co = type_id(conn).await?;
    let dates: Vec<(Uuid, NaiveDate, Decimal)> = sqlx::query_as(
        "SELECT id, worked_date, portion FROM comp_off_request_dates
         WHERE comp_off_request_id = $1 AND active ORDER BY worked_date",
    )
    .bind(r.id)
    .fetch_all(&mut *conn)
    .await?;
    let mut total = Decimal::ZERO;
    for (date_id, worked, portion) in dates {
        let id = Uuid::now_v7();
        let expires = worked
            .checked_add_days(Days::new(expiry_days as u64))
            .unwrap_or(worked);
        sqlx::query(
            "INSERT INTO comp_off_credits
                (id, org_id, user_id, comp_off_request_date_id, worked_date, days, expires_on, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(id)
        .bind(r.org_id)
        .bind(r.user_id)
        .bind(date_id)
        .bind(worked)
        .bind(portion)
        .bind(expires)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        insert_entry(
            conn,
            &LedgerEntry {
                org_id: r.org_id,
                user_id: r.user_id,
                leave_type_id: co,
                leave_year: leave_year_of(worked, start_month as u32),
                delta: portion,
                reason: LedgerReason::CompOffCredit,
                ref_kind: Some("comp_off_credit".into()),
                ref_id: Some(id),
                note: None,
                created_by: Some(approver),
            },
        )
        .await?;
        total += portion;
    }
    Ok(total)
}

// ---------------------------------------------------------------------------------------------
// Credits
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CreditRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub worked_date: NaiveDate,
    pub days: Decimal,
    pub days_used: Decimal,
    pub days_reserved: Decimal,
    pub days_expired: Decimal,
    pub expires_on: NaiveDate,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoke_reason: Option<String>,
    pub approved_by: Option<Uuid>,
    pub approved_by_name: Option<String>,
}

impl CreditRow {
    /// Days still spendable.
    pub fn left(&self) -> Decimal {
        if self.revoked_at.is_some() {
            return Decimal::ZERO;
        }
        expirable(
            self.days,
            self.days_used,
            self.days_reserved,
            self.days_expired,
        )
    }

    /// `revoked`, `active` (days left or held by pending leave), `expired`, or `used`.
    pub fn status(&self) -> &'static str {
        if self.revoked_at.is_some() {
            "revoked"
        } else if self.left() > Decimal::ZERO || self.days_reserved > Decimal::ZERO {
            "active"
        } else if self.days_expired > Decimal::ZERO {
            "expired"
        } else {
            "used"
        }
    }
}

const CREDITS: &str = "SELECT c.id, c.user_id, c.worked_date, c.days, c.days_used,
        c.days_reserved, c.days_expired, c.expires_on, c.revoked_at, c.revoke_reason,
        r.decided_by AS approved_by, u.full_name AS approved_by_name
    FROM comp_off_credits c
    JOIN comp_off_request_dates d ON d.id = c.comp_off_request_date_id
    JOIN comp_off_requests r ON r.id = d.comp_off_request_id
    LEFT JOIN users u ON u.id = r.decided_by";

/// The user's credits, soonest expiry first.
pub async fn credits_of(conn: &mut PgConnection, user: Uuid) -> sqlx::Result<Vec<CreditRow>> {
    sqlx::query_as(&format!(
        "{CREDITS} WHERE c.user_id = $1 ORDER BY c.expires_on, c.worked_date, c.id"
    ))
    .bind(user)
    .fetch_all(conn)
    .await
}

pub async fn credit(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<CreditRow>> {
    sqlx::query_as(&format!("{CREDITS} WHERE c.id = $1 FOR UPDATE OF c"))
        .bind(id)
        .fetch_optional(conn)
        .await
}

/// The approved comp-off leave a credit was spent on: `(credit, request, from, to, days)`.
pub async fn used_on(
    conn: &mut PgConnection,
    user: Uuid,
) -> sqlx::Result<Vec<(Uuid, Uuid, NaiveDate, NaiveDate, Decimal)>> {
    sqlx::query_as(
        "SELECT g.comp_off_credit_id, r.id, r.from_date, r.to_date, g.days_used
         FROM comp_off_usages g
         JOIN leave_requests r ON r.id = g.leave_request_id
         WHERE r.user_id = $1 AND g.days_used > 0
         ORDER BY r.from_date, r.id",
    )
    .bind(user)
    .fetch_all(conn)
    .await
}

/// Is this leave request of the comp-off type?
async fn is_comp_off_leave(conn: &mut PgConnection, request: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT t.is_comp_off FROM leave_requests r JOIN leave_types t ON t.id = r.leave_type_id
         WHERE r.id = $1",
    )
    .bind(request)
    .fetch_one(conn)
    .await
}

/// Spendable credits of `user` today, locked for the caller's transaction.
async fn spendable(
    conn: &mut PgConnection,
    user: Uuid,
    today: NaiveDate,
) -> sqlx::Result<Vec<CreditLeft>> {
    let rows: Vec<(Uuid, NaiveDate, Decimal)> = sqlx::query_as(
        "SELECT id, expires_on, days - days_used - days_reserved - days_expired
         FROM comp_off_credits
         WHERE user_id = $1 AND revoked_at IS NULL AND expires_on > $2
           AND days - days_used - days_reserved - days_expired > 0
         ORDER BY expires_on, id FOR UPDATE",
    )
    .bind(user)
    .bind(today)
    .fetch_all(conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, expires_on, left)| CreditLeft {
            id,
            expires_on,
            left,
        })
        .collect())
}

/// Submitted comp-off leave: hold `days` of the user's credits, soonest expiry first (FR-032).
/// A no-op for other leave types.
pub async fn reserve(
    conn: &mut PgConnection,
    org: Uuid,
    user: Uuid,
    request: Uuid,
    days: Decimal,
    today: NaiveDate,
) -> sqlx::Result<()> {
    if !is_comp_off_leave(conn, request).await? {
        return Ok(());
    }
    let credits = spendable(conn, user, today).await?;
    for (credit, take) in allocate(&credits, days) {
        sqlx::query("UPDATE comp_off_credits SET days_reserved = days_reserved + $2 WHERE id = $1")
            .bind(credit)
            .bind(take)
            .execute(&mut *conn)
            .await?;
        sqlx::query(
            "INSERT INTO comp_off_usages (comp_off_credit_id, leave_request_id, org_id, days_reserved)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (comp_off_credit_id, leave_request_id)
             DO UPDATE SET days_reserved = comp_off_usages.days_reserved + EXCLUDED.days_reserved",
        )
        .bind(credit)
        .bind(request)
        .bind(org)
        .bind(take)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Approved comp-off leave: the credits it held become used. Days the reservation did not cover
/// (credit added by manual adjustment) are drawn from spendable credits. A no-op for other types.
pub async fn consume(
    conn: &mut PgConnection,
    org: Uuid,
    user: Uuid,
    request: Uuid,
    days: Decimal,
    today: NaiveDate,
) -> sqlx::Result<()> {
    if !is_comp_off_leave(conn, request).await? {
        return Ok(());
    }
    let held: Option<Decimal> = sqlx::query_scalar(
        "SELECT sum(days_reserved) FROM comp_off_usages WHERE leave_request_id = $1",
    )
    .bind(request)
    .fetch_one(&mut *conn)
    .await?;
    let short = days - held.unwrap_or_default();
    if short > Decimal::ZERO {
        reserve(conn, org, user, request, short, today).await?;
    }
    sqlx::query(
        "UPDATE comp_off_credits c SET days_reserved = c.days_reserved - g.days_reserved,
                days_used = c.days_used + g.days_reserved
         FROM comp_off_usages g
         WHERE g.comp_off_credit_id = c.id AND g.leave_request_id = $1 AND g.days_reserved > 0",
    )
    .bind(request)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE comp_off_usages SET days_used = days_used + days_reserved, days_reserved = 0
         WHERE leave_request_id = $1",
    )
    .bind(request)
    .execute(conn)
    .await?;
    Ok(())
}

/// Rejected or withdrawn comp-off leave: the held days are free again; any that are past their
/// credit's expiry date expire now (FR-060).
pub async fn release(
    conn: &mut PgConnection,
    request: Uuid,
    actor: Option<Uuid>,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> sqlx::Result<()> {
    let credits: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE comp_off_credits c SET days_reserved = c.days_reserved - g.days_reserved
         FROM comp_off_usages g
         WHERE g.comp_off_credit_id = c.id AND g.leave_request_id = $1 AND g.days_reserved > 0
         RETURNING c.id",
    )
    .bind(request)
    .fetch_all(&mut *conn)
    .await?;
    sqlx::query("UPDATE comp_off_usages SET days_reserved = 0 WHERE leave_request_id = $1")
        .bind(request)
        .execute(&mut *conn)
        .await?;
    expire_due(conn, &credits, actor, now, today).await?;
    Ok(())
}

/// Pending comp-off leave lost `days` (a holiday was added inside it): release that much of
/// its reservation, latest expiry first; released days past their expiry date expire now.
pub async fn release_part(
    conn: &mut PgConnection,
    request: Uuid,
    days: Decimal,
    actor: Option<Uuid>,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> sqlx::Result<()> {
    let held: Vec<(Uuid, Decimal)> = sqlx::query_as(
        "SELECT g.comp_off_credit_id, g.days_reserved FROM comp_off_usages g
         JOIN comp_off_credits c ON c.id = g.comp_off_credit_id
         WHERE g.leave_request_id = $1 AND g.days_reserved > 0
         ORDER BY c.expires_on DESC, c.id DESC FOR UPDATE OF c",
    )
    .bind(request)
    .fetch_all(&mut *conn)
    .await?;
    let mut left = days;
    let mut touched = Vec::new();
    for (credit, reserved) in held {
        if left <= Decimal::ZERO {
            break;
        }
        let back = reserved.min(left);
        sqlx::query("UPDATE comp_off_credits SET days_reserved = days_reserved - $2 WHERE id = $1")
            .bind(credit)
            .bind(back)
            .execute(&mut *conn)
            .await?;
        sqlx::query(
            "UPDATE comp_off_usages SET days_reserved = days_reserved - $3
             WHERE comp_off_credit_id = $1 AND leave_request_id = $2",
        )
        .bind(credit)
        .bind(request)
        .bind(back)
        .execute(&mut *conn)
        .await?;
        touched.push(credit);
        left -= back;
    }
    expire_due(conn, &touched, actor, now, today).await?;
    Ok(())
}

/// Cancelled approved comp-off leave: `days` go back to the credits it used, latest expiry first
/// (the soonest-expiring credit keeps the days already taken). Returned days past their credit's
/// expiry date expire now.
pub async fn give_back(
    conn: &mut PgConnection,
    request: Uuid,
    days: Decimal,
    actor: Option<Uuid>,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> sqlx::Result<()> {
    let usages: Vec<(Uuid, Decimal)> = sqlx::query_as(
        "SELECT g.comp_off_credit_id, g.days_used FROM comp_off_usages g
         JOIN comp_off_credits c ON c.id = g.comp_off_credit_id
         WHERE g.leave_request_id = $1 AND g.days_used > 0
         ORDER BY c.expires_on DESC, c.id DESC FOR UPDATE OF c",
    )
    .bind(request)
    .fetch_all(&mut *conn)
    .await?;
    let mut left = days;
    let mut touched = Vec::new();
    for (credit, used) in usages {
        if left <= Decimal::ZERO {
            break;
        }
        let back = used.min(left);
        sqlx::query("UPDATE comp_off_credits SET days_used = days_used - $2 WHERE id = $1")
            .bind(credit)
            .bind(back)
            .execute(&mut *conn)
            .await?;
        sqlx::query(
            "UPDATE comp_off_usages SET days_used = days_used - $3
             WHERE comp_off_credit_id = $1 AND leave_request_id = $2",
        )
        .bind(credit)
        .bind(request)
        .bind(back)
        .execute(&mut *conn)
        .await?;
        touched.push(credit);
        left -= back;
    }
    expire_due(conn, &touched, actor, now, today).await?;
    Ok(())
}

/// Expire what may expire on `credits` that are past their expiry date (FR-032, FR-060): one
/// `comp_off_expired` ledger row per credit. Returns `(credit, user, days)` expired.
pub async fn expire_due(
    conn: &mut PgConnection,
    credits: &[Uuid],
    actor: Option<Uuid>,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> sqlx::Result<Vec<(Uuid, Uuid, Decimal)>> {
    let due: Vec<(Uuid, Uuid, Uuid, Decimal)> = sqlx::query_as(
        "SELECT id, org_id, user_id, days - days_used - days_reserved - days_expired
         FROM comp_off_credits
         WHERE id = ANY($1) AND revoked_at IS NULL AND expires_on <= $2
           AND days - days_used - days_reserved - days_expired > 0
         ORDER BY expires_on, id FOR UPDATE",
    )
    .bind(credits)
    .bind(today)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::with_capacity(due.len());
    for (id, org, user, days) in due {
        sqlx::query(
            "UPDATE comp_off_credits SET days_expired = days_expired + $2, expired_at = $3
             WHERE id = $1",
        )
        .bind(id)
        .bind(days)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        let (_, start_month) = org_settings(conn, org).await?;
        let co = type_id(conn).await?;
        insert_entry(
            conn,
            &LedgerEntry {
                org_id: org,
                user_id: user,
                leave_type_id: co,
                leave_year: leave_year_of(today, start_month as u32),
                delta: -days,
                reason: LedgerReason::CompOffExpired,
                ref_kind: Some("comp_off_credit".into()),
                ref_id: Some(id),
                note: None,
                created_by: actor,
            },
        )
        .await?;
        out.push((id, user, days));
    }
    Ok(out)
}

/// Every credit in the org past its expiry date with days that may expire.
pub async fn expiry_candidates(
    conn: &mut PgConnection,
    today: NaiveDate,
) -> sqlx::Result<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT id FROM comp_off_credits
         WHERE revoked_at IS NULL AND expires_on <= $1
           AND days - days_used - days_reserved - days_expired > 0
         ORDER BY expires_on, id",
    )
    .bind(today)
    .fetch_all(conn)
    .await
}

/// A credit to warn about: `(credit, user, worked_date, expires_on, days left)`.
pub type ExpiringCredit = (Uuid, Uuid, NaiveDate, NaiveDate, Decimal);

/// Credits expiring within 7 days with spendable days and no warning yet (FR-032). Marks them
/// warned; the caller emails in the same transaction.
pub async fn take_warnings(
    conn: &mut PgConnection,
    today: NaiveDate,
    now: DateTime<Utc>,
) -> sqlx::Result<Vec<ExpiringCredit>> {
    sqlx::query_as(
        "UPDATE comp_off_credits SET warned_at = $2
         WHERE warned_at IS NULL AND revoked_at IS NULL
           AND expires_on > $1 AND expires_on <= $1 + 7
           AND days - days_used - days_reserved - days_expired > 0
         RETURNING id, user_id, worked_date, expires_on,
                   days - days_used - days_reserved - days_expired",
    )
    .bind(today)
    .bind(now)
    .fetch_all(conn)
    .await
}

/// Why a credit cannot be revoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeRefused {
    NotFound,
    AlreadyRevoked,
    InUse,
}

/// Revoke an unused, unreserved credit (FR-061): `comp_off_revoked` for what has not expired.
pub async fn revoke(
    conn: &mut PgConnection,
    org: Uuid,
    id: Uuid,
    admin: Uuid,
    reason: &str,
    now: DateTime<Utc>,
    today: NaiveDate,
) -> sqlx::Result<Result<CreditRow, RevokeRefused>> {
    let Some(c) = credit(conn, id).await? else {
        return Ok(Err(RevokeRefused::NotFound));
    };
    if c.revoked_at.is_some() {
        return Ok(Err(RevokeRefused::AlreadyRevoked));
    }
    if c.days_used > Decimal::ZERO || c.days_reserved > Decimal::ZERO {
        return Ok(Err(RevokeRefused::InUse));
    }
    let amount = c.days - c.days_expired;
    sqlx::query(
        "UPDATE comp_off_credits SET revoked_at = $2, revoked_by = $3, revoke_reason = $4
         WHERE id = $1",
    )
    .bind(id)
    .bind(now)
    .bind(admin)
    .bind(reason)
    .execute(&mut *conn)
    .await?;
    if amount > Decimal::ZERO {
        let (_, start_month) = org_settings(conn, org).await?;
        let co = type_id(conn).await?;
        insert_entry(
            conn,
            &LedgerEntry {
                org_id: org,
                user_id: c.user_id,
                leave_type_id: co,
                leave_year: leave_year_of(today, start_month as u32),
                delta: -amount,
                reason: LedgerReason::CompOffRevoked,
                ref_kind: Some("comp_off_credit".into()),
                ref_id: Some(id),
                note: Some(reason.to_string()),
                created_by: Some(admin),
            },
        )
        .await?;
    }
    Ok(credit(conn, id).await?.ok_or(RevokeRefused::NotFound))
}
