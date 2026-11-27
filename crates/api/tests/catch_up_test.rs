//! T203 — catch-up after downtime (FR-070): the instance is down from Wed 30 Dec 2026 to
//! Sat 2 Jan 2027. On the first tick back, the missed reminder, comp-off expiry and year-end
//! rollover each happen exactly once; a restarted scheduler adds nothing and no email repeats.
//! Kept with the API tests so the data comes from real requests.
mod common;

use chrono::{DateTime, Duration, TimeZone, Utc};
use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use std::collections::BTreeMap;

fn ist(y: i32, m: u32, d: u32, hh: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, hh, 0, 0).unwrap() - Duration::minutes(330)
}

async fn long_session(app: &TestApp, user: &User) -> reqwest::Client {
    app.with_token(&create_session(app, user, true).await)
}

/// Jobs enqueued per kind.
async fn jobs(app: &TestApp) -> BTreeMap<String, i64> {
    sqlx::query_as::<_, (String, i64)>("SELECT kind, count(*) FROM jobs GROUP BY kind")
        .fetch_all(&app.db.owner)
        .await
        .unwrap()
        .into_iter()
        .collect()
}

#[tokio::test]
async fn missed_work_runs_once_when_the_instance_comes_back() {
    let app = TestApp::spawn().await;
    app.clock.set(ist(2026, 12, 30, 10));
    let (org, _, _) = org_with_admin(&app, "acme.test").await;
    sqlx::query(
        "UPDATE organizations SET created_at = '2026-01-05T00:00:00Z', comp_off_expiry_days = 5,
                carry_forward_cap = 5 WHERE id = $1",
    )
    .bind(org.id)
    .execute(&app.db.owner)
    .await
    .unwrap();
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    create_leave_type(&app, &org, NewLeaveType::comp_off()).await;
    let priya_u = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    let asha_u = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    add_ledger_row(&app, &asha_u, cl, 2026, dec!(12), "allotment").await;
    let asha = long_session(&app, &asha_u).await;
    let priya = long_session(&app, &priya_u).await;

    // Wed 30 Dec: a request that will be overdue on 1 Jan, and a comp-off credit for Sat 26 Dec
    // that expires on 31 Dec.
    submit(&app, &asha, &leave_body(cl, "2027-01-11", "2027-01-11")).await;
    earn_comp_off(&app, &asha, &priya, &[("2026-12-26", 1.0)]).await;
    app.run_jobs().await;
    app.email.take();
    let before = jobs(&app).await;

    // Down until Sat 2 Jan, 10:00. The first tick back does everything that was missed.
    app.clock.set(ist(2027, 1, 2, 10));
    app.tick().await;
    let after = jobs(&app).await;
    let added =
        |kind: &str| after.get(kind).copied().unwrap_or(0) - before.get(kind).copied().unwrap_or(0);
    assert_eq!(added("approval_reminder"), 1);
    assert_eq!(added("comp_off_expiry"), 1);
    assert_eq!(added("year_end_rollover"), 1);

    let expired: i64 =
        sqlx::query_scalar("SELECT count(*) FROM balance_ledger WHERE reason = 'comp_off_expired'")
            .fetch_one(&app.db.owner)
            .await
            .unwrap();
    assert_eq!(expired, 1);
    let runs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM rollover_runs WHERE leave_year = 2027")
            .fetch_one(&app.db.owner)
            .await
            .unwrap();
    assert_eq!(runs, 1);
    let sent = app.email.take();
    let to_priya_reminders = sent
        .iter()
        .filter(|m| m.to == "priya@acme.test" && m.subject.to_lowercase().contains("waiting"))
        .count();
    assert_eq!(
        to_priya_reminders,
        1,
        "{:?}",
        sent.iter().map(|m| &m.subject).collect::<Vec<_>>()
    );
    assert_eq!(
        sent.iter()
            .filter(|m| m.to == "asha@acme.test" && m.subject.contains("2027"))
            .count(),
        1,
        "one year-end email"
    );

    // A restarted instance (a new scheduler) finds nothing new to do.
    let ledger: i64 = sqlx::query_scalar("SELECT count(*) FROM balance_ledger")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    leavo_api::app::build_scheduler(&app.state)
        .tick()
        .await
        .unwrap();
    app.run_jobs().await;
    assert_eq!(jobs(&app).await, after);
    assert!(app.email.take().is_empty());
    let ledger_again: i64 = sqlx::query_scalar("SELECT count(*) FROM balance_ledger")
        .fetch_one(&app.db.owner)
        .await
        .unwrap();
    assert_eq!(ledger_again, ledger);
}
