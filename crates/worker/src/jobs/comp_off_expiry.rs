//! Comp-off expiry (T154, FR-032, FR-060).
//!
//! [`CompOffExpiries`] enqueues one `comp_off_expiry` job per org and local day once it is
//! 00:15 in the org's time zone. [`CompOffExpiry`] expires the days of credits past their expiry
//! date that are neither used nor held by pending comp-off leave, and emails a warning, once,
//! for credits expiring within 7 days.
use crate::runner::{JobHandler, JobResult};
use crate::scheduler::Periodic;
use async_trait::async_trait;
use chrono::{DateTime, NaiveTime, Utc};
use leavo_core::clock::Clock;
use leavo_db::comp_off_repo;
use leavo_db::jobs::{Job, NewJob};
use leavo_db::leaves_repo;
use leavo_db::outbox::{notify, OutgoingEmail};
use leavo_db::tx::begin_org_tx;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;
use uuid::Uuid;

pub const KIND: &str = "comp_off_expiry";

/// Local time from which the day's run is due.
fn run_from() -> NaiveTime {
    NaiveTime::from_hms_opt(0, 15, 0).expect("valid time")
}

pub struct CompOffExpiries;

#[async_trait]
impl Periodic for CompOffExpiries {
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
            tx.commit().await?;
            let local = now.with_timezone(&tz);
            if local.time() < run_from() {
                continue;
            }
            let day = local.date_naive();
            jobs.push(NewJob {
                kind: KIND.into(),
                org_id: Some(org),
                payload: json!({ "day": day }),
                run_at: now,
                dedupe_key: Some(format!("{KIND}:{org}:{day}")),
            });
        }
        Ok(jobs)
    }
}

pub struct CompOffExpiry {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    base_url: String,
}

impl CompOffExpiry {
    pub const KIND: &'static str = KIND;

    pub fn new(pool: PgPool, clock: Arc<dyn Clock>, base_url: String) -> Self {
        Self {
            pool,
            clock,
            base_url,
        }
    }
}

fn days_text(days: Decimal) -> String {
    let n = days.normalize();
    if n <= Decimal::ONE {
        format!("{n} day")
    } else {
        format!("{n} days")
    }
}

#[async_trait]
impl JobHandler for CompOffExpiry {
    async fn handle(&self, job: &Job) -> JobResult {
        let org = job.org_id.ok_or("comp_off_expiry without org")?;
        let now = self.clock.now();
        let mut tx = begin_org_tx(&self.pool, org).await?;
        let today = leaves_repo::org_calendar(&mut tx, org, now).await?.today;
        let due = comp_off_repo::expiry_candidates(&mut tx, today).await?;
        let expired = comp_off_repo::expire_due(&mut tx, &due, None, now, today).await?;
        if !expired.is_empty() {
            tracing::info!(org = %org, credits = expired.len(), "comp-off days expired");
        }
        let org_name: String = sqlx::query_scalar("SELECT name FROM organizations WHERE id = $1")
            .bind(org)
            .fetch_one(&mut *tx)
            .await?;
        for (_, user, worked, expires, days) in
            comp_off_repo::take_warnings(&mut tx, today, now).await?
        {
            let (name, email): (String, String) = sqlx::query_as(
                "SELECT full_name, email::text FROM users WHERE id = $1 AND status = 'active'",
            )
            .bind(user)
            .fetch_optional(&mut *tx)
            .await?
            .unwrap_or_default();
            if email.is_empty() {
                continue;
            }
            notify(
                &mut tx,
                &OutgoingEmail {
                    org_id: Some(org),
                    template: "comp_off_expiring".into(),
                    to: email,
                    context: json!({
                        "org_name": org_name, "base_url": self.base_url, "requester": name,
                        "days_text": days_text(days),
                        "worked_date": worked.format("%-d %b %Y").to_string(),
                        "expires_on": expires.format("%-d %b %Y").to_string(),
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
