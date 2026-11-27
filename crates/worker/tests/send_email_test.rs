//! T043/T044 — outbox notify inside a transaction → send_email job renders and sends.
use chrono::{DateTime, TimeZone, Utc};
use leavo_core::clock::FixedClock;

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 24, 9, 0, 0).unwrap()
}
use leavo_db::outbox::{notify, OutboxAttachment, OutgoingEmail};
use leavo_db::testkit::TestDb;
use leavo_email::{EmailSender, MemoryEmailSender, Templates};
use leavo_worker::jobs::send_email::SendEmail;
use leavo_worker::runner::Runner;
use serde_json::json;
use std::sync::Arc;

fn setup(db: &TestDb, sender: Arc<MemoryEmailSender>) -> Runner {
    let clock = Arc::new(FixedClock::new(t0()));
    let mut r = Runner::new(db.app.clone(), clock, "w1");
    let sender: Arc<dyn EmailSender> = sender;
    r.register(
        SendEmail::KIND,
        Arc::new(SendEmail::new(Arc::new(Templates::new()), sender)),
    );
    r
}

fn email(to: &str) -> OutgoingEmail {
    OutgoingEmail {
        org_id: None,
        template: "test_email".into(),
        to: to.into(),
        context: json!({"org_name": "Acme"}),
        attachments: vec![OutboxAttachment {
            filename: "leave.ics".into(),
            content_type: "text/calendar; charset=utf-8".into(),
            content: "BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n".into(),
        }],
    }
}

#[tokio::test]
async fn committed_notification_is_rendered_and_sent() {
    let db = TestDb::new().await;
    let sender = Arc::new(MemoryEmailSender::default());
    let mut tx = db.app.begin().await.unwrap();
    notify(&mut tx, &email("asha@acme.test"), t0())
        .await
        .unwrap();
    tx.commit().await.unwrap();

    setup(&db, sender.clone()).run_once(10).await.unwrap();
    let sent = sender.take();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "asha@acme.test");
    assert_eq!(sent[0].subject, "Leavo test email");
    assert!(sent[0].html.contains("Acme"));
    assert_eq!(sent[0].attachments[0].filename, "leave.ics");
    assert!(String::from_utf8_lossy(&sent[0].attachments[0].bytes).starts_with("BEGIN:VCALENDAR"));
}

#[tokio::test]
async fn rolled_back_notification_is_never_sent() {
    let db = TestDb::new().await;
    let sender = Arc::new(MemoryEmailSender::default());
    let mut tx = db.app.begin().await.unwrap();
    notify(&mut tx, &email("asha@acme.test"), t0())
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    setup(&db, sender.clone()).run_once(10).await.unwrap();
    assert!(sender.take().is_empty());
}

#[tokio::test]
async fn failed_send_is_retried() {
    let db = TestDb::new().await;
    let sender = Arc::new(MemoryEmailSender::default());
    notify(
        &mut db.app.acquire().await.unwrap(),
        &email("asha@acme.test"),
        t0(),
    )
    .await
    .unwrap();
    sender.fail_next("421 try again later");
    setup(&db, sender.clone()).run_once(10).await.unwrap();
    let (status, attempts): (String, i32) =
        sqlx::query_as("SELECT status::text, attempts FROM jobs")
            .fetch_one(&db.app)
            .await
            .unwrap();
    assert_eq!(status, "queued");
    assert_eq!(attempts, 1);
    assert!(sender.take().is_empty());
}

#[tokio::test]
async fn unknown_template_fails_the_job() {
    let db = TestDb::new().await;
    let sender = Arc::new(MemoryEmailSender::default());
    let mut e = email("asha@acme.test");
    e.template = "does_not_exist".into();
    notify(&mut db.app.acquire().await.unwrap(), &e, t0())
        .await
        .unwrap();
    setup(&db, sender.clone()).run_once(10).await.unwrap();
    let err: Option<String> = sqlx::query_scalar("SELECT last_error FROM jobs")
        .fetch_one(&db.app)
        .await
        .unwrap();
    assert!(err.unwrap().contains("unknown email template"));
}
