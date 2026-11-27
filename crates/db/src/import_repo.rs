//! Applying an import (T198, FR-043): the organization context for the dry-run plan, and the
//! all-or-nothing apply in the caller's single transaction.
//!
//! - New people are created `invited` without an allotment: the imported balance is their
//!   balance.
//! - A balance row writes one `import` entry so that the user's ledger balance for that type and
//!   leave year (the whole pool for comp-off) equals the file. On re-import that entry is the
//!   compensating difference; nothing is written when it already matches.
//! - History rows become approved requests with booked working days (so they show in history and
//!   block overlaps) and write no `leave_taken`: the opening balance already reflects them.
use crate::leaves_repo::{self, NewLeaveRequest, OrgCalendar};
use crate::ledger::{insert_entry, LedgerEntry, LedgerReason};
use crate::users_repo::{self, NewUserRow};
use chrono::{DateTime, NaiveDate, Utc};
use leavo_core::calendar::{working_days, Half};
use leavo_importers::csv::{ExistingUser, OrgContext, Plan};
use rust_decimal::Decimal;
use sqlx::PgConnection;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub async fn context(conn: &mut PgConnection, cal: &OrgCalendar) -> sqlx::Result<OrgContext> {
    let domains: Vec<String> = sqlx::query_scalar("SELECT lower(domain::text) FROM org_domains")
        .fetch_all(&mut *conn)
        .await?;
    let types: Vec<(String, Uuid)> = sqlx::query_as("SELECT upper(code), id FROM leave_types")
        .fetch_all(&mut *conn)
        .await?;
    let users: Vec<(String, Uuid)> = sqlx::query_as("SELECT lower(email::text), id FROM users")
        .fetch_all(&mut *conn)
        .await?;
    let imported: Vec<(Uuid, String, i32)> = sqlx::query_as(
        "SELECT DISTINCT l.user_id, upper(t.code), l.leave_year FROM balance_ledger l
         JOIN leave_types t ON t.id = l.leave_type_id WHERE l.reason = 'import'",
    )
    .fetch_all(&mut *conn)
    .await?;
    let booked: Vec<(Uuid, NaiveDate)> = sqlx::query_as(
        "SELECT d.user_id, d.day FROM leave_request_days d
         JOIN leave_requests r ON r.id = d.leave_request_id
         WHERE d.active AND r.status IN ('pending', 'approved', 'cancel_requested')",
    )
    .fetch_all(&mut *conn)
    .await?;
    let groups: Vec<String> = sqlx::query_scalar("SELECT lower(name) FROM approver_groups")
        .fetch_all(&mut *conn)
        .await?;
    Ok(OrgContext {
        domains: domains.into_iter().collect(),
        leave_types: types.into_iter().collect(),
        users: users
            .into_iter()
            .map(|(email, id)| {
                (
                    email,
                    ExistingUser {
                        id,
                        imported: imported
                            .iter()
                            .filter(|i| i.0 == id)
                            .map(|i| (i.1.clone(), i.2))
                            .collect(),
                        booked: booked.iter().filter(|b| b.0 == id).map(|b| b.1).collect(),
                    },
                )
            })
            .collect(),
        groups: groups.into_iter().collect(),
        current_leave_year: cal.current_leave_year(),
        start_month: cal.start_month,
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub users_created: usize,
    pub groups_created: usize,
    pub entries: usize,
    pub requests: usize,
}

/// Write the plan. The caller checked that it has no errors and owns the transaction.
pub async fn apply(
    conn: &mut PgConnection,
    org: Uuid,
    actor: Uuid,
    plan: &Plan,
    ctx: &OrgContext,
    cal: &OrgCalendar,
    now: DateTime<Utc>,
) -> sqlx::Result<Applied> {
    let mut out = Applied::default();
    let mut groups: BTreeMap<String, Uuid> =
        sqlx::query_as::<_, (String, Uuid)>("SELECT lower(name), id FROM approver_groups")
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .collect();
    for name in &plan.groups_to_create {
        let id = Uuid::now_v7();
        sqlx::query("INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, $3)")
            .bind(id)
            .bind(org)
            .bind(name)
            .execute(&mut *conn)
            .await?;
        groups.insert(name.to_lowercase(), id);
        out.groups_created += 1;
    }

    let mut ids: BTreeMap<String, Uuid> =
        ctx.users.iter().map(|(e, u)| (e.clone(), u.id)).collect();
    for u in &plan.users_to_create {
        let id = users_repo::insert_user(
            conn,
            &NewUserRow {
                org_id: org,
                email: &u.email,
                full_name: &u.full_name,
                role: "employee",
                status: "invited",
                date_of_joining: u.date_of_joining,
            },
            now,
        )
        .await?;
        let group = u
            .approver_group
            .as_ref()
            .and_then(|g| groups.get(&g.to_lowercase()).copied());
        sqlx::query(
            "UPDATE users SET employee_code = $2, department = $3, approver_group_id = $4 WHERE id = $1",
        )
        .bind(id)
        .bind(&u.employee_code)
        .bind(&u.department)
        .bind(group)
        .execute(&mut *conn)
        .await?;
        ids.insert(u.email.clone(), id);
        out.users_created += 1;
    }

    let comp_off: BTreeSet<Uuid> =
        sqlx::query_scalar::<_, Uuid>("SELECT id FROM leave_types WHERE is_comp_off")
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .collect();
    for b in &plan.balances {
        let (Some(&user), Some(&leave_type)) =
            (ids.get(&b.email), ctx.leave_types.get(&b.leave_type_code))
        else {
            continue;
        };
        crate::tx::lock_balance(conn, user, leave_type).await?;
        let pooled = comp_off.contains(&leave_type);
        let current: Decimal = sqlx::query_scalar(
            "SELECT COALESCE(SUM(delta), 0) FROM balance_ledger
             WHERE user_id = $1 AND leave_type_id = $2 AND ($3 OR leave_year = $4)",
        )
        .bind(user)
        .bind(leave_type)
        .bind(pooled)
        .bind(b.leave_year)
        .fetch_one(&mut *conn)
        .await?;
        let delta = b.available_days - current;
        if delta.is_zero() {
            continue;
        }
        insert_entry(
            conn,
            &LedgerEntry {
                org_id: org,
                user_id: user,
                leave_type_id: leave_type,
                leave_year: b.leave_year,
                delta,
                reason: LedgerReason::Import,
                ref_kind: Some("import".into()),
                ref_id: None,
                note: Some(format!(
                    "Opening balance {} (row {})",
                    b.available_days.normalize(),
                    b.row
                )),
                created_by: Some(actor),
            },
        )
        .await?;
        out.entries += 1;
    }

    for h in &plan.history {
        let (Some(&user), Some(&leave_type)) =
            (ids.get(&h.email), ctx.leave_types.get(&h.leave_type_code))
        else {
            continue;
        };
        let holidays = leaves_repo::holidays_between(conn, h.from_date, h.to_date).await?;
        let half = !h.days.fract().is_zero();
        let (start_half, end_half) = match (half, h.from_date == h.to_date) {
            (true, true) => (Some(Half::First), None),
            (true, false) => (None, Some(Half::First)),
            _ => (None, None),
        };
        let count = working_days(
            h.from_date,
            h.to_date,
            start_half,
            end_half,
            cal.weekend,
            &holidays,
            cal.start_month,
        );
        let group: Option<Uuid> =
            sqlx::query_scalar("SELECT approver_group_id FROM users WHERE id = $1")
                .bind(user)
                .fetch_one(&mut *conn)
                .await?;
        let id = Uuid::now_v7();
        leaves_repo::insert_request(
            conn,
            &NewLeaveRequest {
                id,
                org_id: org,
                user_id: user,
                leave_type_id: leave_type,
                from: h.from_date,
                to: h.to_date,
                start_half,
                end_half,
                days: h.days,
                reason: &h.reason,
                routed_group_id: group,
                now,
            },
            &count.days,
        )
        .await?;
        let approver = h
            .approved_by_email
            .as_ref()
            .and_then(|e| ids.get(e))
            .copied();
        sqlx::query(
            "UPDATE leave_requests SET status = 'approved', decided_by = $2, decided_at = $3,
                    decision_comment = 'Imported', version = version + 1
             WHERE id = $1",
        )
        .bind(id)
        .bind(approver)
        .bind(now)
        .execute(&mut *conn)
        .await?;
        out.requests += 1;
    }
    Ok(out)
}
