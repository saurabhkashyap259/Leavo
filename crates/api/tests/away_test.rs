//! T126 — who's away and the team calendar (FR-027, FR-058, research R10).
//!
//! The clock starts on Tue 24 Nov 2026, 14:30 in Asia/Kolkata (the org's time zone). "This
//! week" is Mon 23 – Sun 29 Nov; "this month" is November.
mod common;

use chrono::{TimeZone, Utc};
use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::{json, Value};

struct World {
    app: TestApp,
    admin: reqwest::Client,
    priya: reqwest::Client,
    asha: reqwest::Client,
}

/// Eng (approver Priya): Asha, Ravi. Sales (approver Vik): Sam, in Asha's department.
/// Meena has no group. Approved leave:
/// - Ravi Tue 24 – Wed 25 Nov; Sam Tue 24 Nov (both away today)
/// - Meena Thu 26 – Fri 27 Nov (this week)
/// - Vik Mon 30 Nov (this month)
/// - Ravi Mon 14 Dec (next month)
///
/// Asha's own request on 24 Nov is still pending, so it is never listed.
async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, _, admin) = org_with_admin(&app, "acme.test").await;
    let cl = create_leave_type(&app, &org, NewLeaveType::all("CL", dec!(12))).await;
    let priya_u = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    let vik_u = create_user(
        &app,
        &org,
        NewUser::employee("vik@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    let sales = create_group(&app, &org, "Sales", &[&vik_u]).await;
    let asha_u = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    let ravi = create_user(&app, &org, NewUser::employee("ravi@acme.test").group(eng)).await;
    let sam = create_user(&app, &org, NewUser::employee("sam@acme.test").group(sales)).await;
    let meena = create_user(&app, &org, NewUser::employee("meena@acme.test")).await;
    set_department(&app, &asha_u, "Platform").await;
    set_department(&app, &sam, "Platform").await;
    for u in [&asha_u, &ravi, &sam, &meena, &vik_u] {
        add_ledger_row(&app, u, cl, 2026, dec!(12), "allotment").await;
    }
    for (user, from, to) in [
        (&ravi, "2026-11-24", "2026-11-25"),
        (&sam, "2026-11-24", "2026-11-24"),
        (&meena, "2026-11-26", "2026-11-27"),
        (&vik_u, "2026-11-30", "2026-11-30"),
        (&ravi, "2026-12-14", "2026-12-14"),
    ] {
        let mut body = leave_body(cl, from, to);
        body["reason"] = json!(format!("{} private reason", user.email));
        let id = submit(&app, &app.login_as(user).await, &body).await;
        let (status, out) = decide(&app, &admin, &id, approve()).await;
        assert_eq!(status, 200, "{out}");
    }
    let asha = app.login_as(&asha_u).await;
    submit(&app, &asha, &leave_body(cl, "2026-11-24", "2026-11-24")).await;
    let priya = app.login_as(&priya_u).await;
    World {
        app,
        admin,
        priya,
        asha,
    }
}

async fn away(w: &World, client: &reqwest::Client, query: &str) -> Value {
    let (status, out) = get_json(client, w.app.url(&format!("/api/v1/away?{query}"))).await;
    assert_eq!(status, 200, "{out}");
    out
}

fn names(out: &Value) -> Vec<String> {
    out["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["user"]["full_name"].as_str().unwrap().to_string())
        .collect()
}

fn item<'a>(out: &'a Value, name: &str) -> &'a Value {
    out["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["user"]["full_name"] == name)
        .unwrap_or_else(|| panic!("{name} not in {out}"))
}

#[tokio::test]
async fn today_week_and_month_list_approved_leave_sorted_by_start() {
    let w = world().await;
    let today = away(&w, &w.asha, "range=today").await;
    assert_eq!(names(&today), ["ravi", "sam"]);
    assert_eq!(today["count_today"], 2);
    let ravi = item(&today, "ravi");
    assert_eq!(ravi["from"], "2026-11-24");
    assert_eq!(ravi["to"], "2026-11-25");
    assert_eq!(ravi["days"], 2.0);

    let week = away(&w, &w.asha, "range=week").await;
    assert_eq!(names(&week), ["ravi", "sam", "meena"]);
    assert_eq!(week["count_today"], 2);

    let month = away(&w, &w.asha, "range=month&scope=org").await;
    assert_eq!(names(&month), ["ravi", "sam", "meena", "vik"]);

    // Default range is today.
    assert_eq!(names(&away(&w, &w.asha, "").await), ["ravi", "sam"]);
}

#[tokio::test]
async fn dates_follow_the_org_time_zone() {
    let w = world().await;
    // 19:00 UTC on 24 Nov is 00:30 on Wed 25 Nov in Asia/Kolkata: Sam's day off is over.
    w.app
        .clock
        .set(Utc.with_ymd_and_hms(2026, 11, 24, 19, 0, 0).unwrap());
    let today = away(&w, &w.asha, "range=today").await;
    assert_eq!(names(&today), ["ravi"]);
    assert_eq!(today["count_today"], 1);
}

#[tokio::test]
async fn type_and_reason_are_absent_unless_the_viewer_is_eligible() {
    let w = world().await;
    // Employees: name, dates and days only — the keys are absent, not null.
    let month = away(&w, &w.asha, "range=month").await;
    for i in month["items"].as_array().unwrap() {
        assert!(i.get("leave_type").is_none(), "{i}");
        assert!(i.get("reason").is_none(), "{i}");
        assert!(i["user"]["id"].is_string());
    }

    // Priya approves Eng (Ravi) and, like every approver, people without a group (Meena, Vik).
    let month = away(&w, &w.priya, "range=month").await;
    let ravi = item(&month, "ravi");
    assert_eq!(ravi["leave_type"]["code"], "CL");
    assert!(ravi["leave_type"]["color"].is_string());
    assert_eq!(ravi["reason"], "ravi@acme.test private reason");
    assert!(item(&month, "meena").get("reason").is_some());
    assert!(item(&month, "vik").get("reason").is_some());
    let sam = item(&month, "sam");
    assert!(sam.get("leave_type").is_none() && sam.get("reason").is_none());

    // Admins see everything.
    let month = away(&w, &w.admin, "range=month").await;
    assert!(month["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|i| i.get("reason").is_some()));
}

#[tokio::test]
async fn team_scope_is_the_same_group_or_department() {
    let w = world().await;
    // Asha: Ravi shares her group, Sam her department; Meena and Vik share neither.
    let team = away(&w, &w.asha, "range=month&scope=team").await;
    assert_eq!(names(&team), ["ravi", "sam"]);
    assert_eq!(team["count_today"], 2);
    // An approver's team is the group they approve.
    let team = away(&w, &w.priya, "range=month&scope=team").await;
    assert_eq!(names(&team), ["ravi"]);
    assert_eq!(team["count_today"], 1);
}

#[tokio::test]
async fn bad_parameters_are_rejected() {
    let w = world().await;
    for (q, field) in [("range=year", "range"), ("scope=everyone", "scope")] {
        let (status, out) = get_json(&w.asha, w.app.url(&format!("/api/v1/away?{q}"))).await;
        assert_eq!(status, 422, "{q}: {out}");
        assert_eq!(out["error"]["details"]["field"], field);
    }
    let (status, _) = get_json(&w.app.anon(), w.app.url("/api/v1/away")).await;
    assert_eq!(status, 401);
}

async fn calendar(w: &World, client: &reqwest::Client, query: &str) -> Value {
    let (status, out) =
        get_json(client, w.app.url(&format!("/api/v1/team/calendar?{query}"))).await;
    assert_eq!(status, 200, "{out}");
    out
}

fn people_on(out: &Value, date: &str) -> Vec<Value> {
    out["days"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["date"] == date)
        .map(|d| d["people"].as_array().unwrap().clone())
        .unwrap_or_default()
}

#[tokio::test]
async fn team_calendar_uses_the_same_projection_per_day() {
    let w = world().await;
    let cal = calendar(&w, &w.asha, "month=2026-11").await;
    assert_eq!(cal["month"], "2026-11");
    let on_24: Vec<_> = people_on(&cal, "2026-11-24")
        .iter()
        .map(|p| p["user"]["full_name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(on_24, ["ravi", "sam"]);
    assert_eq!(people_on(&cal, "2026-11-26").len(), 1);
    // Booked days only: the weekend and December are not listed.
    assert!(people_on(&cal, "2026-11-28").is_empty());
    assert!(people_on(&cal, "2026-12-14").is_empty());
    for d in cal["days"].as_array().unwrap() {
        for p in d["people"].as_array().unwrap() {
            assert!(
                p.get("leave_type").is_none() && p.get("reason").is_none(),
                "{p}"
            );
            assert!(p.get("half").is_some());
        }
    }

    // Approvers see details for their people; team scope filters as in /away.
    let cal = calendar(&w, &w.priya, "month=2026-11&scope=team").await;
    let on_24 = people_on(&cal, "2026-11-24");
    assert_eq!(on_24.len(), 1);
    assert_eq!(on_24[0]["leave_type"]["code"], "CL");
    assert!(people_on(&cal, "2026-11-26").is_empty());

    // Defaults to the current month; a bad month is a validation error.
    assert_eq!(calendar(&w, &w.asha, "").await["month"], "2026-11");
    let (status, out) = get_json(&w.asha, w.app.url("/api/v1/team/calendar?month=2026-13")).await;
    assert_eq!(status, 422);
    assert_eq!(out["error"]["details"]["field"], "month");
}
