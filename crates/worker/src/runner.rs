//! Background job runner: claims due jobs and dispatches them by `kind`.
use async_trait::async_trait;
use leavo_core::clock::Clock;
use leavo_db::jobs::{self, Job};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration as StdDuration;

pub type JobError = Box<dyn std::error::Error + Send + Sync>;
pub type JobResult = Result<(), JobError>;

#[async_trait]
pub trait JobHandler: Send + Sync {
    async fn handle(&self, job: &Job) -> JobResult;
}

pub struct Runner {
    pool: PgPool,
    clock: Arc<dyn Clock>,
    worker_id: String,
    handlers: HashMap<String, Arc<dyn JobHandler>>,
}

impl Runner {
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>, worker_id: impl Into<String>) -> Self {
        Self {
            pool,
            clock,
            worker_id: worker_id.into(),
            handlers: HashMap::new(),
        }
    }

    pub fn register(&mut self, kind: impl Into<String>, handler: Arc<dyn JobHandler>) {
        self.handlers.insert(kind.into(), handler);
    }

    /// Claim and process up to `limit` due jobs. Returns how many were processed.
    pub async fn run_once(&self, limit: i64) -> sqlx::Result<usize> {
        let claimed = jobs::claim(&self.pool, &self.worker_id, self.clock.now(), limit).await?;
        for job in &claimed {
            let result = match self.handlers.get(&job.kind) {
                Some(h) => h.handle(job).await,
                None => Err(format!("no handler registered for job kind '{}'", job.kind).into()),
            };
            match result {
                Ok(()) => jobs::complete(&self.pool, job.id, self.clock.now()).await?,
                Err(e) => {
                    let status =
                        jobs::fail(&self.pool, job, &e.to_string(), self.clock.now()).await?;
                    tracing::warn!(job_id = %job.id, kind = %job.kind, attempts = job.attempts, ?status, error = %e, "job failed");
                }
            }
        }
        Ok(claimed.len())
    }

    /// Poll until `shutdown` resolves.
    pub async fn run_until<F: std::future::Future<Output = ()>>(&self, shutdown: F) {
        tokio::pin!(shutdown);
        loop {
            let processed = match self.run_once(10).await {
                Ok(n) => n,
                Err(e) => {
                    tracing::error!(error = %e, "job runner error");
                    0
                }
            };
            let idle = if processed == 0 {
                StdDuration::from_secs(1)
            } else {
                StdDuration::ZERO
            };
            tokio::select! {
                _ = &mut shutdown => break,
                _ = tokio::time::sleep(idle) => {}
            }
        }
    }
}
