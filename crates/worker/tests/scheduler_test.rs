//! T045 — scheduler skeleton: periodic jobs are enqueued once per period; tick time recorded.
use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Timelike, Utc};
use leavo_core::clock::{Clock, FixedClock};
use leavo_db::jobs::NewJob;
use leavo_db::testkit::TestDb;
use leavo_worker::scheduler::{Periodic, Scheduler};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;

struct Hourly;

#[async_trait]
impl Periodic for Hourly {
    fn kind(&self) -> &'static str {
        "hourly_probe"
    }
    async fn due(&self, _pool: &PgPool, now: DateTime<Utc>) -> sqlx::Result<Vec<NewJob>> {
        let hour = now.with_minute(0).unwrap().with_second(0).unwrap();
        Ok(vec![NewJob {
            kind: self.kind().into(),
            org_id: None,
            payload: json!({ "hour": hour }),
            run_at: now,
            dedupe_key: Some(format!("{}:{}", self.kind(), hour.format("%Y%m%d%H"))),
        }])
    }
}

async fn count(db: &TestDb) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = 'hourly_probe'")
        .fetch_one(&db.app)
        .await
        .unwrap()
}

#[tokio::test]
async fn enqueues_once_per_period_and_records_tick() {
    let db = TestDb::new().await;
    let clock = Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 11, 24, 9, 5, 0).unwrap(),
    ));
    let mut s = Scheduler::new(db.app.clone(), clock.clone());
    s.register(Arc::new(Hourly));

    assert_eq!(s.tick().await.unwrap(), 1);
    clock.advance(Duration::minutes(1));
    assert_eq!(s.tick().await.unwrap(), 0, "same hour: deduplicated");
    assert_eq!(count(&db).await, 1);

    clock.advance(Duration::hours(1));
    assert_eq!(s.tick().await.unwrap(), 1);
    assert_eq!(count(&db).await, 2);

    let last: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT last_scheduler_tick_at FROM instance_settings")
            .fetch_one(&db.app)
            .await
            .unwrap();
    assert_eq!(last, Some(clock.now()));
}
