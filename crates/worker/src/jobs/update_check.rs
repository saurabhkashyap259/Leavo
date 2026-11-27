//! Update check (T211, FR-052): once a day, ask where releases are published for the latest
//! version and remember it for the Instance page. Nothing about the instance is sent. Disabled
//! entirely with `LEAVO_UPDATE_CHECK=false` (the scheduler then never enqueues it).
use crate::runner::{JobHandler, JobResult};
use crate::scheduler::Periodic;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use leavo_core::clock::Clock;
use leavo_db::jobs::{Job, NewJob};
use serde_json::json;
use sqlx::PgPool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

pub const KIND: &str = "update_check";

/// Where the latest released version comes from.
#[async_trait]
pub trait ReleaseSource: Send + Sync {
    /// The latest version, e.g. `1.4.0`.
    async fn latest_version(&self) -> Result<String, String>;
}

/// A fixed answer, counting how often it was asked (tests and offline builds).
pub struct StaticRelease {
    pub version: String,
    pub calls: AtomicUsize,
}

impl StaticRelease {
    pub fn new(version: &str) -> Self {
        Self {
            version: version.into(),
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl ReleaseSource for StaticRelease {
    async fn latest_version(&self) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.version.clone())
    }
}

pub struct UpdateChecks;

#[async_trait]
impl Periodic for UpdateChecks {
    fn kind(&self) -> &'static str {
        KIND
    }

    async fn due(&self, _pool: &PgPool, now: DateTime<Utc>) -> sqlx::Result<Vec<NewJob>> {
        Ok(vec![NewJob {
            kind: KIND.into(),
            org_id: None,
            payload: json!({}),
            run_at: now,
            dedupe_key: Some(format!("{KIND}:{}", now.date_naive())),
        }])
    }
}

pub struct UpdateCheck {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    source: Arc<dyn ReleaseSource>,
}

impl UpdateCheck {
    pub const KIND: &'static str = KIND;

    pub fn new(pool: PgPool, clock: Arc<dyn Clock>, source: Arc<dyn ReleaseSource>) -> Self {
        Self {
            pool,
            clock,
            source,
        }
    }
}

#[async_trait]
impl JobHandler for UpdateCheck {
    async fn handle(&self, _job: &Job) -> JobResult {
        let latest = self.source.latest_version().await?;
        let latest = latest.trim().trim_start_matches('v').to_string();
        sqlx::query(
            "UPDATE instance_settings SET latest_version_seen = $1, last_update_check_at = $2
             WHERE id = 1",
        )
        .bind(&latest)
        .bind(self.clock.now())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
