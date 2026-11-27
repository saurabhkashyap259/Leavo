//! T115 — reminders for requests pending over 48 h (FR-025): once per 24 h, none after the
//! decision, to the request's eligible approvers.
use chrono::{DateTime, Duration, TimeZone, Utc};
use leavo_core::clock::FixedClock;
use leavo_db::testkit::TestDb;
use leavo_email::{MemoryEmailSender, Templates};
use leavo_worker::jobs::approval_reminder::{ApprovalReminder, ApprovalReminders};
use leavo_worker::jobs::send_email::SendEmail;
use leavo_worker::runner::Runner;
use leavo_worker::scheduler::Scheduler;
use std::sync::Arc;
use uuid::Uuid;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 24, 9, 0, 0).unwrap()
}

struct World {
    db: TestDb,
    clock: Arc<FixedClock>,
    email: Arc<MemoryEmailSender>,
    scheduler: Scheduler,
    runner: Runner,
    request: Uuid,
}

async fn exec(db: &TestDb, sql: &str, binds: &[Uuid]) {
    let mut q = sqlx::query(sql);
    for b in binds {
        q = q.bind(*b);
    }
    q.execute(&db.owner).await.unwrap();
}

/// Org with an admin, an Eng group (approver Priya), a Sales approver (Vik), and Asha's
/// pending request submitted 47 hours before `t0`.
async fn world() -> World {
    let db = TestDb::new().await;
    let (org, admin, priya, vik, asha, eng, cl, request) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    exec(
        &db,
        "INSERT INTO organizations (id, name) VALUES ($1, 'Acme')",
        &[org],
    )
    .await;
    exec(
        &db,
        "INSERT INTO approver_groups (id, org_id, name) VALUES ($1, $2, 'Eng')",
        &[eng, org],
    )
    .await;
    for (id, email, role, group) in [
        (admin, "admin@acme.test", "admin", None),
        (priya, "priya@acme.test", "approver", None),
        (vik, "vik@acme.test", "approver", None),
        (asha, "asha@acme.test", "employee", Some(eng)),
    ] {
        sqlx::query(
            "INSERT INTO users (id, org_id, email, full_name, role, approver_group_id)
             VALUES ($1, $2, $3, $3, $4::user_role, $5)",
        )
        .bind(id)
        .bind(org)
        .bind(email)
        .bind(role)
        .bind(group)
        .execute(&db.owner)
        .await
        .unwrap();
    }
    exec(
        &db,
        "INSERT INTO approver_group_members (group_id, user_id, org_id) VALUES ($1, $2, $3)",
        &[eng, priya, org],
    )
    .await;
    exec(
        &db,
        "INSERT INTO leave_types (id, org_id, code, name, color, yearly_quota)
         VALUES ($1, $2, 'CL', 'Casual Leave', '#3B82F6', 12)",
        &[cl, org],
    )
    .await;
    sqlx::query(
        "INSERT INTO leave_requests (id, org_id, user_id, leave_type_id, from_date, to_date, days,
                                     reason, routed_group_id, created_at)
         VALUES ($1, $2, $3, $4, '2026-12-01', '2026-12-01', 1, 'Trip', $5, $6)",
    )
    .bind(request)
    .bind(org)
    .bind(asha)
    .bind(cl)
    .bind(eng)
    .bind(t0() - Duration::hours(47))
    .execute(&db.owner)
    .await
    .unwrap();
    let _ = vik;

    let clock = Arc::new(FixedClock::new(t0()));
    let email = Arc::new(MemoryEmailSender::default());
    let mut scheduler = Scheduler::new(db.app.clone(), clock.clone());
    scheduler.register(Arc::new(ApprovalReminders));
    let mut runner = Runner::new(db.app.clone(), clock.clone(), "test");
    runner.register(
        ApprovalReminder::KIND,
        Arc::new(ApprovalReminder::new(
            db.app.clone(),
            clock.clone(),
            "http://leavo.test".into(),
        )),
    );
    runner.register(
        SendEmail::KIND,
        Arc::new(SendEmail::new(Arc::new(Templates::new()), email.clone())),
    );
    World {
        db,
        clock,
        email,
        scheduler,
        runner,
        request,
    }
}

impl World {
    /// Tick the scheduler, run every job, and return who was reminded.
    async fn tick(&self) -> Vec<String> {
        self.scheduler.tick().await.unwrap();
        while self.runner.run_once(50).await.unwrap() > 0 {}
        let mut to: Vec<String> = self
            .email
            .take()
            .into_iter()
            .filter(|m| m.subject.contains("waiting"))
            .map(|m| m.to)
            .collect();
        to.sort();
        to
    }
}

#[tokio::test]
async fn reminds_after_48_hours_then_once_a_day_until_decided() {
    let w = world().await;
    assert!(w.tick().await.is_empty(), "47 h: too early");

    w.clock.advance(Duration::hours(1));
    assert_eq!(
        w.tick().await,
        ["admin@acme.test", "priya@acme.test"],
        "48 h"
    );

    for _ in 0..23 {
        w.clock.advance(Duration::hours(1));
        assert!(w.tick().await.is_empty(), "not again within 24 h");
    }
    w.clock.advance(Duration::hours(1));
    assert_eq!(
        w.tick().await,
        ["admin@acme.test", "priya@acme.test"],
        "72 h"
    );

    sqlx::query("UPDATE leave_requests SET status = 'approved' WHERE id = $1")
        .bind(w.request)
        .execute(&w.db.owner)
        .await
        .unwrap();
    w.clock.advance(Duration::hours(24));
    assert!(w.tick().await.is_empty(), "none after the decision");
}

#[tokio::test]
async fn a_decision_between_scheduling_and_sending_cancels_the_reminder() {
    let w = world().await;
    w.clock.advance(Duration::hours(2));
    w.scheduler.tick().await.unwrap();
    sqlx::query("UPDATE leave_requests SET status = 'rejected' WHERE id = $1")
        .bind(w.request)
        .execute(&w.db.owner)
        .await
        .unwrap();
    while w.runner.run_once(50).await.unwrap() > 0 {}
    assert!(w.email.take().is_empty());
}

#[tokio::test]
async fn the_reminder_names_the_request_and_links_to_the_inbox() {
    let w = world().await;
    w.clock.advance(Duration::hours(1));
    w.scheduler.tick().await.unwrap();
    while w.runner.run_once(50).await.unwrap() > 0 {}
    let mail = w
        .email
        .take()
        .into_iter()
        .find(|m| m.to == "priya@acme.test")
        .unwrap();
    assert!(mail.subject.contains("asha@acme.test"), "{}", mail.subject);
    assert!(mail.text.contains("Casual Leave"));
    assert!(mail.text.contains("1 Dec 2026"));
    assert!(
        mail.text.contains(&format!(
            "http://leavo.test/approvals?request={}",
            w.request
        )),
        "{}",
        mail.text
    );
}
