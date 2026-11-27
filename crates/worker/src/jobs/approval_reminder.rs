//! Reminders for requests pending too long (T122, FR-025).
//!
//! [`ApprovalReminders`] (scheduled hourly) enqueues one `approval_reminder` job per pending
//! request and 24-hour window once it has waited `reminder_after_hours` (default 48), keyed so
//! a window is never reminded twice. [`ApprovalReminder`] re-checks the request is still
//! pending and emails its eligible approvers.
use crate::runner::{JobHandler, JobResult};
use crate::scheduler::Periodic;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use leavo_core::clock::Clock;
use leavo_db::jobs::{Job, NewJob};
use leavo_db::outbox::{notify, OutgoingEmail};
use leavo_db::tx::begin_org_tx;
use leavo_db::{approvals_repo, leaves_repo};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

pub const KIND: &str = "approval_reminder";

pub struct ApprovalReminders;

#[async_trait]
impl Periodic for ApprovalReminders {
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
            // (request, completed 24 h windows since it became overdue)
            let due: Vec<(Uuid, i64)> = sqlx::query_as(
                "SELECT r.id,
                        floor(extract(epoch FROM ($1 - r.created_at - make_interval(hours => o.reminder_after_hours))) / 86400)::bigint
                 FROM leave_requests r JOIN organizations o ON o.id = r.org_id
                 WHERE r.status = 'pending'
                   AND r.created_at + make_interval(hours => o.reminder_after_hours) <= $1",
            )
            .bind(now)
            .fetch_all(&mut *tx)
            .await?;
            tx.commit().await?;
            jobs.extend(due.into_iter().map(|(request, window)| NewJob {
                kind: KIND.into(),
                org_id: Some(org),
                payload: json!({ "request_id": request }),
                run_at: now,
                dedupe_key: Some(format!("{KIND}:{org}:{request}:{window}")),
            }));
        }
        Ok(jobs)
    }
}

pub struct ApprovalReminder {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    base_url: String,
}

impl ApprovalReminder {
    pub const KIND: &'static str = KIND;

    pub fn new(pool: PgPool, clock: Arc<dyn Clock>, base_url: String) -> Self {
        Self {
            pool,
            clock,
            base_url,
        }
    }
}

fn day(d: NaiveDate, h: Option<&str>) -> String {
    let mut s = d.format("%-d %b %Y").to_string();
    match h {
        Some("first") => s.push_str(" (first half)"),
        Some("second") => s.push_str(" (second half)"),
        _ => {}
    }
    s
}

fn days_text(days: Decimal) -> String {
    let n = days.normalize();
    if n <= Decimal::ONE {
        format!("{n} day")
    } else {
        format!("{n} days")
    }
}

fn waited(hours: i64) -> String {
    if hours >= 48 {
        format!("{} days", hours / 24)
    } else {
        format!("{hours} hours")
    }
}

#[async_trait]
impl JobHandler for ApprovalReminder {
    async fn handle(&self, job: &Job) -> JobResult {
        let org = job.org_id.ok_or("approval_reminder without org")?;
        let request: Uuid = serde_json::from_value(job.payload["request_id"].clone())?;
        let now = self.clock.now();
        let mut tx = begin_org_tx(&self.pool, org).await?;
        let Some(r) = approvals_repo::load(&mut tx, request).await? else {
            return Ok(());
        };
        // Decided since it was scheduled: nothing to remind (FR-025).
        if r.status != "pending" {
            return Ok(());
        }
        let Ok(approvers) =
            leaves_repo::eligible_approvers(&mut tx, r.routed_group_id, r.user_id).await?
        else {
            return Ok(());
        };
        let (requester, type_name, org_name): (String, String, String) = sqlx::query_as(
            "SELECT u.full_name, t.name, o.name FROM users u, leave_types t, organizations o
             WHERE u.id = $1 AND t.id = $2 AND o.id = $3",
        )
        .bind(r.user_id)
        .bind(r.leave_type_id)
        .bind(org)
        .fetch_one(&mut *tx)
        .await?;
        let dates = if r.from_date == r.to_date {
            day(
                r.from_date,
                r.start_half.as_deref().or(r.end_half.as_deref()),
            )
        } else {
            format!(
                "{} – {}",
                day(r.from_date, r.start_half.as_deref()),
                day(r.to_date, r.end_half.as_deref())
            )
        };
        let emails: Vec<String> =
            sqlx::query_scalar("SELECT email::text FROM users WHERE id = ANY($1) ORDER BY email")
                .bind(&approvers)
                .fetch_all(&mut *tx)
                .await?;
        for to in emails {
            notify(
                &mut tx,
                &OutgoingEmail {
                    org_id: Some(org),
                    template: "approval_reminder".into(),
                    to,
                    context: json!({
                        "org_name": org_name, "base_url": self.base_url,
                        "requester": requester, "leave_type": type_name, "reason": r.reason,
                        "dates": dates, "days_text": days_text(r.days),
                        "waiting": waited((now - r.created_at).num_hours()),
                        "inbox_url": format!("{}/approvals?request={}", self.base_url, r.id),
                    }),
                    attachments: vec![],
                },
                now,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
