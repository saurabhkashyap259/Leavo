//! Year-end rollover (T170/T171, FR-037–FR-039): the plan's input from the database, the
//! once-per-year run guarded by `rollover_runs`, and the carry-forward report used by the admin
//! preview and the summary email. Runs in org-scoped transactions.
use crate::ledger::{insert_entry, LedgerEntry, LedgerReason};
use chrono::{DateTime, NaiveDate, Utc};
use leavo_core::rollover::{
    plan_rollover, OldBalance, Planned, PlannedKind, RolloverInput, TypePolicy, UserInfo,
};
use rust_decimal::Decimal;
use sqlx::PgConnection;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Everything the plan needs to close `closing_year`.
pub async fn input(
    conn: &mut PgConnection,
    org: Uuid,
    closing_year: i32,
) -> sqlx::Result<RolloverInput> {
    let (cap, start_month): (Decimal, i32) = sqlx::query_as(
        "SELECT carry_forward_cap, leave_year_start_month::int FROM organizations WHERE id = $1",
    )
    .bind(org)
    .fetch_one(&mut *conn)
    .await?;
    type TypeRow = (
        Uuid,
        Option<Decimal>,
        Option<Decimal>,
        bool,
        bool,
        String,
        bool,
    );
    let types: Vec<TypeRow> = sqlx::query_as(
        "SELECT id, yearly_quota, pending_yearly_quota, is_comp_off, receives_carry_forward,
                availability::text, active
         FROM leave_types ORDER BY sort_order, code, id",
    )
    .fetch_all(&mut *conn)
    .await?;
    let users: Vec<(Uuid, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT id, date_of_joining FROM users WHERE status = 'active' ORDER BY email, id",
    )
    .fetch_all(&mut *conn)
    .await?;
    let grants: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT user_id, leave_type_id FROM leave_type_grants WHERE revoked_at IS NULL",
    )
    .fetch_all(&mut *conn)
    .await?;
    let balances: Vec<(Uuid, Uuid, Decimal)> = sqlx::query_as(
        "SELECT user_id, leave_type_id, SUM(delta) FROM balance_ledger
         WHERE leave_year = $1 GROUP BY user_id, leave_type_id",
    )
    .bind(closing_year)
    .fetch_all(&mut *conn)
    .await?;
    let allotted: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT DISTINCT user_id, leave_type_id FROM balance_ledger
         WHERE leave_year = $1 AND reason IN ('allotment', 'prorate')",
    )
    .bind(closing_year + 1)
    .fetch_all(&mut *conn)
    .await?;
    Ok(RolloverInput {
        closing_year,
        start_month: start_month as u32,
        cap,
        types: types
            .into_iter()
            .map(
                |(id, quota, pending, is_comp_off, carry, availability, active)| TypePolicy {
                    id,
                    quota: pending.or(quota),
                    is_comp_off,
                    receives_carry_forward: carry,
                    restricted: availability == "restricted",
                    active,
                },
            )
            .collect(),
        users: users
            .into_iter()
            .map(|(id, doj)| UserInfo {
                id,
                date_of_joining: doj,
                granted: grants.iter().filter(|g| g.0 == id).map(|g| g.1).collect(),
            })
            .collect(),
        balances: balances
            .into_iter()
            .map(|(user, leave_type, balance)| OldBalance {
                user,
                leave_type,
                balance,
            })
            .collect(),
        already_allotted: allotted.into_iter().collect::<BTreeSet<_>>(),
    })
}

pub async fn has_run(conn: &mut PgConnection, org: Uuid, new_year: i32) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM rollover_runs WHERE org_id = $1 AND leave_year = $2)",
    )
    .bind(org)
    .bind(new_year)
    .fetch_one(conn)
    .await
}

fn reason(kind: PlannedKind) -> LedgerReason {
    match kind {
        PlannedKind::Allotment => LedgerReason::Allotment,
        PlannedKind::Prorate => LedgerReason::Prorate,
        PlannedKind::CarryForward => LedgerReason::CarryForward,
        PlannedKind::NegativeCarry => LedgerReason::NegativeCarry,
        PlannedKind::Lapse => LedgerReason::Lapse,
    }
}

/// Open `new_year` for the org: write the plan, apply scheduled quotas and record the run.
/// `None` when it has already run (FR-038). Serialised per org with an advisory lock.
pub async fn run(
    conn: &mut PgConnection,
    org: Uuid,
    new_year: i32,
    now: DateTime<Utc>,
) -> sqlx::Result<Option<Vec<Planned>>> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("rollover:{org}"))
        .execute(&mut *conn)
        .await?;
    if has_run(conn, org, new_year).await? {
        return Ok(None);
    }
    sqlx::query("INSERT INTO rollover_runs (org_id, leave_year, started_at) VALUES ($1, $2, $3)")
        .bind(org)
        .bind(new_year)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    let plan = plan_rollover(&input(conn, org, new_year - 1).await?);
    let note = format!("Year end {} → {new_year}", new_year - 1);
    for p in &plan {
        insert_entry(
            conn,
            &LedgerEntry {
                org_id: org,
                user_id: p.user,
                leave_type_id: p.leave_type,
                leave_year: p.leave_year,
                delta: p.delta,
                reason: reason(p.kind),
                ref_kind: Some("rollover".into()),
                ref_id: None,
                note: Some(note.clone()),
                created_by: None,
            },
        )
        .await?;
    }
    // A quota scheduled for "next leave year" is now the quota (FR-056).
    sqlx::query(
        "UPDATE leave_types SET yearly_quota = pending_yearly_quota, pending_yearly_quota = NULL
         WHERE pending_yearly_quota IS NOT NULL",
    )
    .execute(&mut *conn)
    .await?;
    let sum =
        |k: PlannedKind| -> Decimal { plan.iter().filter(|p| p.kind == k).map(|p| p.delta).sum() };
    let lapsed: Decimal = plan
        .iter()
        .filter(|p| p.kind == PlannedKind::Lapse && p.delta < Decimal::ZERO)
        .map(|p| -p.delta)
        .sum();
    sqlx::query(
        "UPDATE rollover_runs SET finished_at = $3, summary = $4
         WHERE org_id = $1 AND leave_year = $2",
    )
    .bind(org)
    .bind(new_year)
    .bind(now)
    .bind(serde_json::json!({
        "entries": plan.len(),
        "allotted": (sum(PlannedKind::Allotment) + sum(PlannedKind::Prorate)).to_string(),
        "carried": sum(PlannedKind::CarryForward).to_string(),
        "negative_carried": sum(PlannedKind::NegativeCarry).to_string(),
        "lapsed": lapsed.to_string(),
    }))
    .execute(&mut *conn)
    .await?;
    Ok(Some(plan))
}

/// One line of the carry-forward report.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct ReportRow {
    pub user_id: Uuid,
    pub email: String,
    pub full_name: String,
    pub leave_type_id: Uuid,
    pub code: String,
    pub name: String,
    pub color: String,
    /// The closing year's balance before year end.
    pub unused: Decimal,
    /// Carried into the new year (negative for a carried deficit).
    pub carry: Decimal,
    /// Positive days lost.
    pub lapse: Decimal,
}

/// Carry-forward results for closing `closing_year` (FR-039), per active user and carry-forward
/// type with a non-zero balance: what the plan will write, or what was written once the year
/// has been rolled over. Both give the same numbers.
pub async fn report(
    conn: &mut PgConnection,
    org: Uuid,
    closing_year: i32,
) -> sqlx::Result<Vec<ReportRow>> {
    let people: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, email::text, full_name FROM users WHERE status = 'active' ORDER BY email, id",
    )
    .fetch_all(&mut *conn)
    .await?;
    let types: Vec<(Uuid, String, String, String)> = sqlx::query_as(
        "SELECT id, code, name, color FROM leave_types
         WHERE receives_carry_forward AND NOT is_comp_off ORDER BY sort_order, code, id",
    )
    .fetch_all(&mut *conn)
    .await?;
    // (user, type) → (unused, carry, lapse)
    let mut figures: BTreeMap<(Uuid, Uuid), (Decimal, Decimal, Decimal)> = BTreeMap::new();
    if has_run(conn, org, closing_year + 1).await? {
        let rows: Vec<(Uuid, Uuid, Decimal, Decimal, Decimal, Decimal)> = sqlx::query_as(
            "SELECT user_id, leave_type_id,
                    COALESCE(SUM(delta) FILTER (WHERE leave_year = $1), 0),
                    COALESCE(SUM(delta) FILTER (WHERE leave_year = $1 AND reason = 'lapse'), 0),
                    COALESCE(-SUM(delta) FILTER (WHERE leave_year = $1 AND reason = 'lapse' AND delta < 0), 0),
                    COALESCE(SUM(delta) FILTER (WHERE leave_year = $1 + 1
                                                  AND reason IN ('carry_forward', 'negative_carry')), 0)
             FROM balance_ledger WHERE leave_year IN ($1, $1 + 1)
             GROUP BY user_id, leave_type_id",
        )
        .bind(closing_year)
        .fetch_all(&mut *conn)
        .await?;
        for (user, t, balance, lapse_rows, lapsed, carry) in rows {
            figures.insert((user, t), (balance - lapse_rows, carry, lapsed));
        }
    } else {
        let input = input(conn, org, closing_year).await?;
        for b in &input.balances {
            figures.entry((b.user, b.leave_type)).or_default().0 += b.balance;
        }
        for p in plan_rollover(&input) {
            let f = figures.entry((p.user, p.leave_type)).or_default();
            match p.kind {
                PlannedKind::CarryForward | PlannedKind::NegativeCarry => f.1 += p.delta,
                PlannedKind::Lapse if p.delta < Decimal::ZERO => f.2 -= p.delta,
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for (user, email, full_name) in &people {
        for (t, code, name, color) in &types {
            let (unused, carry, lapse) = figures.get(&(*user, *t)).copied().unwrap_or_default();
            if unused.is_zero() {
                continue;
            }
            out.push(ReportRow {
                user_id: *user,
                email: email.clone(),
                full_name: full_name.clone(),
                leave_type_id: *t,
                code: code.clone(),
                name: name.clone(),
                color: color.clone(),
                unused,
                carry,
                lapse,
            });
        }
    }
    Ok(out)
}

/// Neutralise spreadsheet formulas and quote when needed (as the history export does).
fn csv_field(value: &str) -> String {
    let safe = if value.starts_with(['=', '+', '-', '@', '\t', '\r'])
        && value.parse::<Decimal>().is_err()
    {
        format!("'{value}")
    } else {
        value.to_string()
    };
    if safe.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

/// `email,full_name,leave_type,unused,carry,lapse` with CRLF line endings
/// (contracts/csv-formats.md §7).
pub fn report_csv(rows: &[ReportRow]) -> String {
    let mut out = String::from("email,full_name,leave_type,unused,carry,lapse\r\n");
    for r in rows {
        let line = [
            csv_field(&r.email),
            csv_field(&r.full_name),
            csv_field(&r.code),
            r.unused.normalize().to_string(),
            r.carry.normalize().to_string(),
            r.lapse.normalize().to_string(),
        ]
        .join(",");
        out.push_str(&line);
        out.push_str("\r\n");
    }
    out
}
