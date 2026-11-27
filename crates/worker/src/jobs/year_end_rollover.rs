//! Year-end rollover (T170, FR-037, FR-038, FR-070).
//!
//! [`YearEndRollovers`] enqueues one `year_end_rollover` job per org and leave year from 00:30
//! (org time zone) on the leave year's first day, or on the first tick after it when the
//! instance was down. Orgs created after a leave year started are not rolled into it. The job
//! writes the plan once (`rollover_runs`), then emails each employee their new balances and the
//! admins a summary with the carry-forward CSV.
use crate::runner::{JobHandler, JobResult};
use crate::scheduler::Periodic;
use async_trait::async_trait;
use chrono::{DateTime, NaiveTime, Utc};
use leavo_core::calendar::{leave_year_bounds, leave_year_of};
use leavo_core::clock::Clock;
use leavo_core::rollover::{Planned, PlannedKind};
use leavo_db::jobs::{Job, NewJob};
use leavo_db::outbox::{notify, OutboxAttachment, OutgoingEmail};
use leavo_db::tx::begin_org_tx;
use leavo_db::{leaves_repo, rollover_repo};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use std::collections::BTreeMap;
use std::sync::Arc;
use uuid::Uuid;

pub const KIND: &str = "year_end_rollover";

fn run_from() -> NaiveTime {
    NaiveTime::from_hms_opt(0, 30, 0).expect("valid time")
}

pub struct YearEndRollovers;

#[async_trait]
impl Periodic for YearEndRollovers {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn due(&self, pool: &PgPool, now: DateTime<Utc>) -> sqlx::Result<Vec<NewJob>> {
        let orgs: Vec<Uuid> = sqlx::query_scalar("SELECT job_org_ids()")
            .fetch_all(pool)
            .await?;
        let mut jobs = Vec::new();
        for org in orgs {
            let mut tx = begin_org_tx(pool, org).await?;
            let tz = leaves_repo::org_timezone(&mut tx, org).await?;
            let (start_month, created): (i32, DateTime<Utc>) = sqlx::query_as(
                "SELECT leave_year_start_month::int, created_at FROM organizations WHERE id = $1",
            )
            .bind(org)
            .fetch_one(&mut *tx)
            .await?;
            let local = now.with_timezone(&tz);
            let year = leave_year_of(local.date_naive(), start_month as u32);
            let (start, _) = leave_year_bounds(year, start_month as u32);
            let done = rollover_repo::has_run(&mut tx, org, year).await?;
            tx.commit().await?;
            let started = local.date_naive() > start || local.time() >= run_from();
            let existed = created.with_timezone(&tz).date_naive() < start;
            if done || !started || !existed {
                continue;
            }
            jobs.push(NewJob {
                kind: KIND.into(),
                org_id: Some(org),
                payload: json!({ "leave_year": year }),
                run_at: now,
                dedupe_key: Some(format!("rollover:{org}:{year}")),
            });
        }
        Ok(jobs)
    }
}

pub struct YearEndRollover {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    base_url: String,
}

impl YearEndRollover {
    pub const KIND: &'static str = KIND;

    pub fn new(pool: PgPool, clock: Arc<dyn Clock>, base_url: String) -> Self {
        Self {
            pool,
            clock,
            base_url,
        }
    }
}

fn num(d: Decimal) -> String {
    d.normalize().to_string()
}

fn days(d: Decimal) -> String {
    let n = d.normalize();
    if n.abs() <= Decimal::ONE {
        format!("{n} day")
    } else {
        format!("{n} days")
    }
}

/// "Casual Leave: 17 days (12 allotted, 5 carried, 3 lapsed)" for each type the plan touched.
fn employee_lines(
    plan: &[Planned],
    user: Uuid,
    names: &BTreeMap<Uuid, String>,
    balances: &BTreeMap<Uuid, Decimal>,
) -> Vec<String> {
    let mut by_type: BTreeMap<&str, (Uuid, Decimal, Decimal, Decimal)> = BTreeMap::new();
    for p in plan.iter().filter(|p| p.user == user) {
        let name = names
            .get(&p.leave_type)
            .map(String::as_str)
            .unwrap_or("Leave");
        let e = by_type.entry(name).or_insert((
            p.leave_type,
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
        ));
        match p.kind {
            PlannedKind::Allotment | PlannedKind::Prorate => e.1 += p.delta,
            PlannedKind::CarryForward | PlannedKind::NegativeCarry => e.2 += p.delta,
            PlannedKind::Lapse if p.delta < Decimal::ZERO => e.3 -= p.delta,
            PlannedKind::Lapse => {}
        }
    }
    by_type
        .into_iter()
        .map(|(name, (t, allotted, carried, lapsed))| {
            let mut parts = Vec::new();
            if !allotted.is_zero() {
                parts.push(format!("{} allotted", num(allotted)));
            }
            if !carried.is_zero() {
                parts.push(format!("{} carried", num(carried)));
            }
            if !lapsed.is_zero() {
                parts.push(format!("{} lapsed", num(lapsed)));
            }
            let total = balances.get(&t).copied().unwrap_or_default();
            format!("{name}: {} ({})", days(total), parts.join(", "))
        })
        .collect()
}

#[async_trait]
impl JobHandler for YearEndRollover {
    async fn handle(&self, job: &Job) -> JobResult {
        let org = job.org_id.ok_or("year_end_rollover without org")?;
        let year: i32 = serde_json::from_value(job.payload["leave_year"].clone())?;
        let now = self.clock.now();
        let mut tx = begin_org_tx(&self.pool, org).await?;
        let Some(plan) = rollover_repo::run(&mut tx, org, year, now).await? else {
            return Ok(());
        };
        let org_name: String = sqlx::query_scalar("SELECT name FROM organizations WHERE id = $1")
            .bind(org)
            .fetch_one(&mut *tx)
            .await?;
        let names: BTreeMap<Uuid, String> =
            sqlx::query_as::<_, (Uuid, String)>("SELECT id, name FROM leave_types")
                .fetch_all(&mut *tx)
                .await?
                .into_iter()
                .collect();
        let people: Vec<(Uuid, String, String, String)> = sqlx::query_as(
            "SELECT id, email::text, full_name, role::text FROM users
             WHERE status = 'active' ORDER BY email",
        )
        .fetch_all(&mut *tx)
        .await?;
        for (user, email, name, _) in &people {
            let balances: BTreeMap<Uuid, Decimal> = sqlx::query_as::<_, (Uuid, Decimal)>(
                "SELECT leave_type_id, SUM(delta) FROM balance_ledger
                 WHERE user_id = $1 AND leave_year = $2 GROUP BY leave_type_id",
            )
            .bind(user)
            .bind(year)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .collect();
            let lines = employee_lines(&plan, *user, &names, &balances);
            if lines.is_empty() {
                continue;
            }
            notify(
                &mut tx,
                &OutgoingEmail {
                    org_id: Some(org),
                    template: "rollover_employee".into(),
                    to: email.clone(),
                    context: json!({
                        "org_name": org_name, "base_url": self.base_url, "requester": name,
                        "year": year, "lines": lines,
                    }),
                    attachments: vec![],
                },
                now,
            )
            .await?;
        }
        let report = rollover_repo::report(&mut tx, org, year - 1).await?;
        let csv = rollover_repo::report_csv(&report);
        let carried: Decimal = report
            .iter()
            .map(|r| r.carry)
            .filter(|c| *c > Decimal::ZERO)
            .sum();
        let lapsed: Decimal = report.iter().map(|r| r.lapse).sum();
        for (_, email, _, _) in people.iter().filter(|p| p.3 == "admin") {
            notify(
                &mut tx,
                &OutgoingEmail {
                    org_id: Some(org),
                    template: "rollover_admin_summary".into(),
                    to: email.clone(),
                    context: json!({
                        "org_name": org_name, "base_url": self.base_url, "year": year,
                        "closing_year": year - 1, "employees": report.len(),
                        "carried": num(carried), "lapsed": num(lapsed),
                    }),
                    attachments: vec![OutboxAttachment {
                        filename: format!("carry-forward-{}.csv", year - 1),
                        content_type: "text/csv; charset=utf-8".into(),
                        content: csv.clone(),
                    }],
                },
                now,
            )
            .await?;
        }
        tx.commit().await?;
        tracing::info!(org = %org, leave_year = year, entries = plan.len(), "year-end rollover done");
        Ok(())
    }
}
