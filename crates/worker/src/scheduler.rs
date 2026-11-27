//! Scheduler: every tick asks each [`Periodic`] which jobs are due and enqueues them.
//! Jobs carry a `dedupe_key` of the form `kind:org:period`, so ticking more than once in a
//! period (or running several instances) never enqueues duplicates.
//!
//! Catch-up of periods missed while the instance was stopped is added in T209 (FR-070).
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use leavo_core::clock::Clock;
use leavo_db::jobs::{enqueue, NewJob};
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration as StdDuration;

#[async_trait]
pub trait Periodic: Send + Sync {
    fn kind(&self) -> &'static str;
    /// Jobs due at `now`, each with a dedupe key identifying its period.
    async fn due(&self, pool: &PgPool, now: DateTime<Utc>) -> sqlx::Result<Vec<NewJob>>;
}

pub struct Scheduler {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    periodic: Vec<Arc<dyn Periodic>>,
}

impl Scheduler {
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self {
            pool,
            clock,
            periodic: Vec::new(),
        }
    }

    pub fn register(&mut self, p: Arc<dyn Periodic>) {
        self.periodic.push(p);
    }

    /// Enqueue everything due now. Returns the number of newly enqueued jobs.
    pub async fn tick(&self) -> sqlx::Result<usize> {
        let now = self.clock.now();
        let mut inserted = 0;
        for p in &self.periodic {
            for job in p.due(&self.pool, now).await? {
                debug_assert!(
                    job.dedupe_key.is_some(),
                    "{} jobs need a dedupe key",
                    p.kind()
                );
                if enqueue(&self.pool, &job).await?.is_some() {
                    inserted += 1;
                }
            }
        }
        sqlx::query("UPDATE instance_settings SET last_scheduler_tick_at = $1 WHERE id = 1")
            .bind(now)
            .execute(&self.pool)
            .await?;
        Ok(inserted)
    }

    /// Tick once a minute until `shutdown` resolves.
    pub async fn run_until<F: std::future::Future<Output = ()>>(&self, shutdown: F) {
        tokio::pin!(shutdown);
        loop {
            if let Err(e) = self.tick().await {
                tracing::error!(error = %e, "scheduler tick failed");
            }
            tokio::select! {
                _ = &mut shutdown => break,
                _ = tokio::time::sleep(StdDuration::from_secs(60)) => {}
            }
        }
    }
}
