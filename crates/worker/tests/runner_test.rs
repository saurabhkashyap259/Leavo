//! T020 — job runner: SKIP LOCKED, backoff, dead-lettering, dedupe, outbox semantics (FR-047).
use async_trait::async_trait;
use chrono::{Duration, TimeZone, Utc};
use leavo_core::clock::{Clock, FixedClock};
use leavo_db::jobs::{enqueue, Job, JobStatus, NewJob};
use leavo_db::testkit::TestDb;
use leavo_worker::runner::{JobHandler, JobResult, Runner};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Default)]
struct Counting(Mutex<HashMap<Uuid, u32>>);

#[async_trait]
impl JobHandler for Counting {
    async fn handle(&self, job: &Job) -> JobResult {
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        *self.0.lock().unwrap().entry(job.id).or_default() += 1;
        Ok(())
    }
}

struct AlwaysFails;

#[async_trait]
impl JobHandler for AlwaysFails {
    async fn handle(&self, _job: &Job) -> JobResult {
        Err("smtp unreachable".into())
    }
}

fn clock() -> Arc<FixedClock> {
    Arc::new(FixedClock::new(
        Utc.with_ymd_and_hms(2026, 11, 24, 9, 0, 0).unwrap(),
    ))
}

fn job(kind: &str, dedupe: Option<&str>, run_at: chrono::DateTime<Utc>) -> NewJob {
    NewJob {
        kind: kind.into(),
        org_id: None,
        payload: json!({}),
        run_at,
        dedupe_key: dedupe.map(String::from),
    }
}

async fn status_of(
    db: &TestDb,
    kind: &str,
) -> (JobStatus, i32, chrono::DateTime<Utc>, Option<String>) {
    sqlx::query_as("SELECT status, attempts, run_at, last_error FROM jobs WHERE kind = $1")
        .bind(kind)
        .fetch_one(&db.app)
        .await
        .unwrap()
}

#[tokio::test]
async fn two_runners_never_process_the_same_job() {
    let db = TestDb::new().await;
    let c = clock();
    for _ in 0..60 {
        enqueue(&db.app, &job("count", None, c.now()))
            .await
            .unwrap();
    }
    let handler = Arc::new(Counting::default());
    let mk = |id: &str| {
        let mut r = Runner::new(db.app.clone(), c.clone(), id);
        r.register("count", handler.clone());
        r
    };
    let (r1, r2) = (mk("w1"), mk("w2"));
    let drain = |r: Runner| async move {
        let mut n = 0;
        loop {
            let done = r.run_once(5).await.unwrap();
            if done == 0 {
                break n;
            }
            n += done;
        }
    };
    let (a, b) = tokio::join!(drain(r1), drain(r2));
    assert_eq!(a + b, 60);
    let seen = handler.0.lock().unwrap();
    assert_eq!(seen.len(), 60);
    assert!(seen.values().all(|n| *n == 1), "a job ran twice");
}

#[tokio::test]
async fn failures_back_off_exponentially_then_go_dead() {
    let db = TestDb::new().await;
    let c = clock();
    enqueue(&db.app, &job("flaky", None, c.now()))
        .await
        .unwrap();
    let mut r = Runner::new(db.app.clone(), c.clone(), "w1");
    r.register("flaky", Arc::new(AlwaysFails));

    assert_eq!(r.run_once(10).await.unwrap(), 1);
    let (st, attempts, run_at, err) = status_of(&db, "flaky").await;
    assert_eq!(st, JobStatus::Queued);
    assert_eq!(attempts, 1);
    assert_eq!(run_at, c.now() + Duration::seconds(30));
    assert_eq!(err.as_deref(), Some("smtp unreachable"));

    // Not due yet: nothing runs.
    assert_eq!(r.run_once(10).await.unwrap(), 0);

    c.advance(Duration::seconds(30));
    r.run_once(10).await.unwrap();
    let (_, attempts, run_at, _) = status_of(&db, "flaky").await;
    assert_eq!(attempts, 2);
    assert_eq!(run_at, c.now() + Duration::seconds(60));

    for _ in 0..3 {
        c.advance(Duration::hours(1));
        r.run_once(10).await.unwrap();
    }
    let (st, attempts, _, _) = status_of(&db, "flaky").await;
    assert_eq!(attempts, 5);
    assert_eq!(st, JobStatus::Dead);

    c.advance(Duration::hours(1));
    assert_eq!(
        r.run_once(10).await.unwrap(),
        0,
        "dead jobs are never retried"
    );
}

#[tokio::test]
async fn unknown_job_kinds_fail_instead_of_vanishing() {
    let db = TestDb::new().await;
    let c = clock();
    enqueue(&db.app, &job("mystery", None, c.now()))
        .await
        .unwrap();
    let r = Runner::new(db.app.clone(), c.clone(), "w1");
    r.run_once(10).await.unwrap();
    let (st, _, _, err) = status_of(&db, "mystery").await;
    assert_eq!(st, JobStatus::Queued);
    assert!(err.unwrap().contains("no handler"));
}

#[tokio::test]
async fn dedupe_key_enqueues_once() {
    let db = TestDb::new().await;
    let c = clock();
    let first = enqueue(
        &db.app,
        &job("rollover", Some("rollover:org:2027"), c.now()),
    )
    .await
    .unwrap();
    let second = enqueue(
        &db.app,
        &job("rollover", Some("rollover:org:2027"), c.now()),
    )
    .await
    .unwrap();
    assert!(first.is_some());
    assert!(second.is_none());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs")
        .fetch_one(&db.app)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn jobs_from_a_rolled_back_transaction_never_run() {
    let db = TestDb::new().await;
    let c = clock();
    let mut tx = db.app.begin().await.unwrap();
    enqueue(&mut *tx, &job("count", None, c.now()))
        .await
        .unwrap();
    tx.rollback().await.unwrap();

    let handler = Arc::new(Counting::default());
    let mut r = Runner::new(db.app.clone(), c.clone(), "w1");
    r.register("count", handler.clone());
    assert_eq!(r.run_once(10).await.unwrap(), 0);
    assert!(handler.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn successful_jobs_are_marked_done() {
    let db = TestDb::new().await;
    let c = clock();
    enqueue(&db.app, &job("count", None, c.now()))
        .await
        .unwrap();
    let mut r = Runner::new(db.app.clone(), c.clone(), "w1");
    r.register("count", Arc::new(Counting::default()));
    r.run_once(10).await.unwrap();
    let (st, attempts, _, _) = status_of(&db, "count").await;
    assert_eq!(st, JobStatus::Done);
    assert_eq!(attempts, 1);
}
