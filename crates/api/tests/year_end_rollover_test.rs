//! T167/T168 — year-end rollover through the real scheduler and runner (FR-037–FR-039,
//! FR-056, FR-057, FR-070), and the admin preview that must match what the job writes.
//! Kept with the API tests (not `crates/worker/tests`) so balances come from real requests.
//!
//! Org: leave year starts in January, Asia/Kolkata, carry-forward cap 5, created in Jan 2026.
//! Asha: CL 12, 4 days taken → 8 unused; SL 6 unused (SL's quota becomes 8 next year).
//! Ravi: CL 12 with a −14 adjustment → −2.
mod common;

use chrono::{DateTime, Duration, TimeZone, Utc};
use common::factories::*;
use common::*;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use uuid::Uuid;

struct World {
    app: TestApp,
    admin_user: User,
    admin: reqwest::Client,
    asha_user: User,
    ravi_user: User,
    cl: Uuid,
    sl: Uuid,
}

/// `hh:mm` in Kolkata on `y-m-d`.
fn ist(y: i32, m: u32, d: u32, hh: u32, mm: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(y, m, d, hh, mm, 0).unwrap() - Duration::minutes(330)
}

async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, admin_u, _) = org_with_admin(&app, "acme.test").await;
    sqlx::query(
        "UPDATE organizations SET carry_forward_cap = 5, created_at = '2026-01-05T00:00:00Z'
         WHERE id = $1",
    )
    .bind(org.id)
    .execute(&app.db.owner)
    .await
    .unwrap();
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let sl = create_leave_type(&app, &org, NewLeaveType::all("SL", dec!(6))).await;
    sqlx::query("UPDATE leave_types SET receives_carry_forward = true WHERE id = $1")
        .bind(cl)
        .execute(&app.db.owner)
        .await
        .unwrap();
    set_type_rule(&app, sl, "pending_yearly_quota", "8").await;
    sqlx::query("UPDATE leave_types SET name = 'Casual Leave' WHERE id = $1")
        .bind(cl)
        .execute(&app.db.owner)
        .await
        .unwrap();
    let priya_u = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    let asha_user = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    set_full_name(&app, &asha_user, "Asha Rao").await;
    let ravi_user = create_user(&app, &org, NewUser::employee("ravi@acme.test").group(eng)).await;
    for u in [&asha_user, &ravi_user] {
        add_ledger_row(&app, u, cl, 2026, dec!(12), "allotment").await;
    }
    add_ledger_row(&app, &asha_user, sl, 2026, dec!(6), "allotment").await;
    add_ledger_row(&app, &ravi_user, cl, 2026, dec!(-14), "manual_adjustment").await;
    let asha = app.login_as(&asha_user).await;
    let priya = app.login_as(&priya_u).await;
    let id = submit(&app, &asha, &leave_body(cl, "2026-11-30", "2026-12-03")).await;
    let (status, out) = decide(&app, &priya, &id, approve()).await;
    assert_eq!(status, 200, "{out}");
    // "Keep me signed in": survives the clock moving into January.
    let admin = app.with_token(&create_session(&app, &admin_u, true).await);
    World {
        app,
        admin_user: admin_u,
        admin,
        asha_user,
        ravi_user,
        cl,
        sl,
    }
}

/// `(reason, delta, leave_year)` of one user's rows for a type.
async fn rows(w: &World, user: &User, leave_type: Uuid) -> Vec<(String, Decimal, i32)> {
    ledger_rows(&w.app, user.id, leave_type)
        .await
        .into_iter()
        .map(|(r, d, y, _)| (r, d, y))
        .collect()
}

fn row(reason: &str, delta: Decimal, year: i32) -> (String, Decimal, i32) {
    (reason.into(), delta, year)
}

async fn ledger_count(w: &World) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM balance_ledger")
        .fetch_one(&w.app.db.owner)
        .await
        .unwrap()
}

async fn preview(w: &World) -> Value {
    let (status, out) = get_json(
        &w.admin,
        w.app.url("/api/v1/admin/carry-forward/preview?year=2026"),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    out
}

#[tokio::test]
async fn runs_once_on_the_first_day_of_the_leave_year() {
    let w = world().await;
    let before = ledger_count(&w).await;
    // New Year's Eve, and just after midnight but before 00:30: nothing yet.
    for at in [ist(2026, 12, 31, 23, 0), ist(2027, 1, 1, 0, 20)] {
        w.app.clock.set(at);
        w.app.tick().await;
        assert_eq!(ledger_count(&w).await, before, "{at}");
    }
    w.app.email.take();

    w.app.clock.set(ist(2027, 1, 1, 0, 40));
    w.app.tick().await;
    assert_eq!(
        rows(&w, &w.asha_user, w.cl).await,
        [
            row("allotment", dec!(12), 2026),
            row("leave_taken", dec!(-4), 2026),
            row("lapse", dec!(-3), 2026),
            row("carry_forward", dec!(5), 2027),
            row("allotment", dec!(12), 2027),
        ]
    );
    assert_eq!(
        rows(&w, &w.asha_user, w.sl).await,
        [
            row("allotment", dec!(6), 2026),
            row("lapse", dec!(-6), 2026),
            row("allotment", dec!(8), 2027),
        ]
    );
    assert_eq!(
        rows(&w, &w.ravi_user, w.cl).await[2..],
        [
            row("lapse", dec!(2), 2026),
            row("negative_carry", dec!(-2), 2027),
            row("allotment", dec!(12), 2027),
        ]
    );
    // The scheduled quota is now the quota.
    let (quota, pending): (Decimal, Option<Decimal>) =
        sqlx::query_as("SELECT yearly_quota, pending_yearly_quota FROM leave_types WHERE id = $1")
            .bind(w.sl)
            .fetch_one(&w.app.db.owner)
            .await
            .unwrap();
    assert_eq!((quota, pending), (dec!(8), None));
    let run: (i32, bool) =
        sqlx::query_as("SELECT leave_year, finished_at IS NOT NULL FROM rollover_runs")
            .fetch_one(&w.app.db.owner)
            .await
            .unwrap();
    assert_eq!(run, (2027, true));

    // Employees get their new balances; admins a summary with the CSV.
    let sent = w.app.email.take();
    let asha = sent
        .iter()
        .find(|m| m.to == "asha@acme.test")
        .expect("employee email");
    assert!(asha.subject.contains("2027"), "{}", asha.subject);
    assert!(asha.text.contains("Casual Leave: 17 days"), "{}", asha.text);
    assert!(asha.text.contains("5 carried"), "{}", asha.text);
    assert!(asha.text.contains("3 lapsed"), "{}", asha.text);
    let admin = sent
        .iter()
        .find(|m| m.to == "admin@acme.test" && m.subject.starts_with("Year end"))
        .expect("admin summary");
    assert_eq!(admin.attachments.len(), 1);
    assert_eq!(admin.attachments[0].filename, "carry-forward-2026.csv");
    let csv = String::from_utf8(admin.attachments[0].bytes.clone()).unwrap();
    assert!(
        csv.starts_with("email,full_name,leave_type,unused,carry,lapse\r\n"),
        "{csv}"
    );
    assert!(csv.contains("asha@acme.test,Asha Rao,CL,8,5,3"), "{csv}");

    // Running again — the same day, the next day — changes nothing and sends nothing.
    let after = ledger_count(&w).await;
    for at in [ist(2027, 1, 1, 9, 0), ist(2027, 1, 2, 9, 0)] {
        w.app.clock.set(at);
        w.app.tick().await;
        assert_eq!(ledger_count(&w).await, after);
    }
    assert!(w.app.email.take().is_empty());
}

#[tokio::test]
async fn a_missed_year_end_runs_when_the_instance_comes_back() {
    let w = world().await;
    // Down over New Year: the first tick on 3 Jan does the work (FR-070).
    w.app.clock.set(ist(2027, 1, 3, 10, 0));
    w.app.tick().await;
    assert!(rows(&w, &w.asha_user, w.cl)
        .await
        .contains(&row("carry_forward", dec!(5), 2027)));
}

#[tokio::test]
async fn an_org_created_during_the_year_gets_no_rollover_for_it() {
    let w = world().await;
    sqlx::query("UPDATE organizations SET created_at = '2027-01-01T05:00:00Z'")
        .execute(&w.app.db.owner)
        .await
        .unwrap();
    let before = ledger_count(&w).await;
    w.app.clock.set(ist(2027, 1, 2, 10, 0));
    w.app.tick().await;
    assert_eq!(ledger_count(&w).await, before);
}

#[tokio::test]
async fn the_preview_matches_what_the_job_writes() {
    let w = world().await;
    let out = preview(&w).await;
    let items = out.as_array().unwrap();
    let pick = |email: &str| {
        items
            .iter()
            .find(|i| i["user"]["email"] == email)
            .cloned()
            .unwrap_or_else(|| panic!("{email} not in {out}"))
    };
    let asha = pick("asha@acme.test");
    assert_eq!(asha["user"]["full_name"], "Asha Rao");
    assert_eq!(asha["leave_type"]["code"], "CL");
    assert_eq!(
        (
            asha["unused"].clone(),
            asha["carry"].clone(),
            asha["lapse"].clone()
        ),
        (json!(8.0), json!(5.0), json!(3.0))
    );
    let ravi = pick("ravi@acme.test");
    assert_eq!(
        (
            ravi["unused"].clone(),
            ravi["carry"].clone(),
            ravi["lapse"].clone()
        ),
        (json!(-2.0), json!(-2.0), json!(0.0))
    );
    // Only carry-forward types are listed.
    assert!(items.iter().all(|i| i["leave_type"]["code"] == "CL"));

    let res = w
        .admin
        .get(
            w.app
                .url("/api/v1/admin/carry-forward/preview.csv?year=2026"),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert!(res.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/csv"));
    let csv = res.text().await.unwrap();
    assert!(
        csv.starts_with("email,full_name,leave_type,unused,carry,lapse\r\n"),
        "{csv}"
    );
    assert!(csv.contains("ravi@acme.test,ravi,CL,-2,-2,0\r\n"), "{csv}");

    // After the run, the report shows what was written: the same numbers.
    w.app.clock.set(ist(2027, 1, 1, 0, 40));
    w.app.tick().await;
    let admin = w
        .app
        .with_token(&create_session(&w.app, &w.admin_user, true).await);
    let (status, after) = get_json(
        &admin,
        w.app.url("/api/v1/admin/carry-forward/preview?year=2026"),
    )
    .await;
    assert_eq!(status, 200, "{after}");
    assert_eq!(after, out);

    let employee = w.app.login_as(&w.asha_user).await;
    let (status, _) = get_json(&employee, w.app.url("/api/v1/admin/carry-forward/preview")).await;
    assert_eq!(status, 403);
}
