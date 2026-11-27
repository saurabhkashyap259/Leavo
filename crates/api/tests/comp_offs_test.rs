//! T145 — requesting comp-off credit and deciding it (FR-030, FR-031, FR-032).
//!
//! The clock starts on Tue 24 Nov 2026, 14:30 in Asia/Kolkata. Diwali (Mon 9 Nov) is a
//! mandatory holiday and Mon 16 Nov an optional one. The org's comp-off expiry is 90 days.
mod common;

use chrono::{NaiveDate, TimeZone, Utc};
use common::factories::*;
use common::*;
use rust_decimal_macros::dec;
use serde_json::{json, Value};
use uuid::Uuid;

pub struct World {
    pub app: TestApp,
    pub priya: reqwest::Client,
    pub vik: reqwest::Client,
    pub asha_user: User,
    pub asha: reqwest::Client,
    pub ravi: reqwest::Client,
    pub co: Uuid,
}

async fn long_session(app: &TestApp, user: &User) -> reqwest::Client {
    app.with_token(&create_session(app, user, true).await)
}

fn d(s: &str) -> NaiveDate {
    s.parse().unwrap()
}

async fn world() -> World {
    let app = TestApp::spawn().await;
    let (org, _, _) = org_with_admin(&app, "acme.test").await;
    let co = create_leave_type(&app, &org, NewLeaveType::comp_off()).await;
    sqlx::query("UPDATE leave_types SET name = 'Comp-off' WHERE id = $1")
        .bind(co)
        .execute(&app.db.owner)
        .await
        .unwrap();
    add_holiday(&app, &org, d("2026-11-09"), "Diwali", "mandatory").await;
    add_holiday(&app, &org, d("2026-11-16"), "Regional", "optional").await;
    let priya_u = create_user(
        &app,
        &org,
        NewUser::employee("priya@acme.test").role("approver"),
    )
    .await;
    set_full_name(&app, &priya_u, "Priya Shah").await;
    let vik_u = create_user(
        &app,
        &org,
        NewUser::employee("vik@acme.test").role("approver"),
    )
    .await;
    let eng = create_group(&app, &org, "Eng", &[&priya_u]).await;
    create_group(&app, &org, "Sales", &[&vik_u]).await;
    let asha_user = create_user(&app, &org, NewUser::employee("asha@acme.test").group(eng)).await;
    set_full_name(&app, &asha_user, "Asha Rao").await;
    let ravi_u = create_user(&app, &org, NewUser::employee("ravi@acme.test")).await;
    World {
        priya: long_session(&app, &priya_u).await,
        vik: long_session(&app, &vik_u).await,
        asha: long_session(&app, &asha_user).await,
        ravi: long_session(&app, &ravi_u).await,
        asha_user,
        co,
        app,
    }
}

async fn mail(w: &World) -> Vec<(String, String, String)> {
    w.app.run_jobs().await;
    w.app
        .email
        .take()
        .into_iter()
        .map(|m| (m.to, m.subject, m.text))
        .collect()
}

fn to<'a>(sent: &'a [(String, String, String)], who: &str) -> &'a (String, String, String) {
    sent.iter()
        .find(|m| m.0 == who)
        .unwrap_or_else(|| panic!("no email to {who} in {sent:?}"))
}

async fn request(w: &World, dates: &[(&str, f64)]) -> String {
    let mut body = comp_off_body(dates);
    body["notify"] = json!([{ "email": "ravi@acme.test" }]);
    let (status, out) = post_comp_off(&w.app, &w.asha, &body).await;
    assert_eq!(status, 201, "{out}");
    out["id"].as_str().unwrap().to_string()
}

async fn rejected_with(w: &World, body: Value) -> Value {
    let (status, out) = post_comp_off(&w.app, &w.asha, &body).await;
    assert_eq!(status, 422, "{body} → {out}");
    out["error"].clone()
}

#[tokio::test]
async fn request_several_dates_with_half_days() {
    let w = world().await;
    mail(&w).await;
    let mut body = comp_off_body(&[
        ("2026-11-21", 1.0),
        ("2026-11-22", 0.5),
        ("2026-11-09", 1.0),
    ]);
    body["notify"] = json!([{ "email": "ravi@acme.test" }]);
    let (status, out) = post_comp_off(&w.app, &w.asha, &body).await;
    assert_eq!(status, 201, "{out}");
    assert_eq!(out["status"], "pending");
    assert_eq!(out["days"], 2.5);
    let id = out["id"].as_str().unwrap().to_string();

    let mine = comp_offs(&w.app, &w.asha).await;
    assert_eq!(mine["available"], 0.0);
    assert_eq!(mine["credits"], json!([]));
    let req = &mine["requests"][0];
    assert_eq!(req["id"], id);
    assert_eq!(req["status"], "pending");
    assert_eq!(req["days"], 2.5);
    assert_eq!(req["reason"], "Release weekend");
    assert_eq!(
        req["dates"],
        json!([
            { "date": "2026-11-09", "portion": 1.0 },
            { "date": "2026-11-21", "portion": 1.0 },
            { "date": "2026-11-22", "portion": 0.5 },
        ])
    );

    // The approver's inbox, kept apart from leave requests.
    let (status, inbox) = get_json(&w.priya, w.app.url("/api/v1/approvals?kind=comp_off")).await;
    assert_eq!(status, 200, "{inbox}");
    let item = &inbox[0];
    assert_eq!(item["id"], id);
    assert_eq!(item["kind"], "comp_off");
    assert_eq!(item["requester"]["full_name"], "Asha Rao");
    assert_eq!(item["leave_type"]["name"], "Comp-off");
    assert_eq!(item["from"], "2026-11-09");
    assert_eq!(item["to"], "2026-11-22");
    assert_eq!(item["days"], 2.5);
    assert_eq!(item["version"], 1);
    assert_eq!(item["dates"].as_array().unwrap().len(), 3);
    let (_, leave_inbox) = get_json(&w.priya, w.app.url("/api/v1/approvals")).await;
    assert_eq!(leave_inbox, json!([]));
    let (_, vik_inbox) = get_json(&w.vik, w.app.url("/api/v1/approvals?kind=comp_off")).await;
    assert_eq!(vik_inbox, json!([]));

    let sent = mail(&w).await;
    let action = to(&sent, "priya@acme.test");
    assert!(action.1.contains("Asha Rao"), "{}", action.1);
    assert!(action.1.to_lowercase().contains("comp-off"), "{}", action.1);
    assert!(action.2.contains("Release weekend"), "{}", action.2);
    assert!(
        action.2.contains("/api/v1/approvals/action/"),
        "{}",
        action.2
    );
    let fyi = to(&sent, "ravi@acme.test");
    assert!(!fyi.2.contains("Release weekend"), "{}", fyi.2);
    assert!(!fyi.2.contains("http"), "{}", fyi.2);
}

#[tokio::test]
async fn invalid_and_already_claimed_dates_are_rejected() {
    let w = world().await;
    for (date, why) in [
        ("2026-11-18", "a working Wednesday"),
        ("2026-11-16", "an optional holiday"),
        ("2026-11-24", "today"),
        ("2026-11-28", "a future Saturday"),
    ] {
        let err = rejected_with(&w, comp_off_body(&[("2026-11-21", 1.0), (date, 1.0)])).await;
        assert_eq!(err["code"], "COMP_OFF_DATE_INVALID", "{why}");
        assert_eq!(err["details"]["date"], date, "{why}");
    }
    for (body, field) in [
        (comp_off_body(&[]), "dates"),
        (
            comp_off_body(&[("2026-11-21", 1.0), ("2026-11-21", 0.5)]),
            "dates",
        ),
        (comp_off_body(&[("2026-11-21", 0.25)]), "dates"),
        (
            json!({ "dates": [{ "date": "2026-11-21", "portion": 1 }], "reason": " ", "notify": [] }),
            "reason",
        ),
    ] {
        let err = rejected_with(&w, body).await;
        assert_eq!(err["code"], "VALIDATION");
        assert_eq!(err["details"]["field"], field);
    }

    // A date claimed by a pending (or approved) request cannot be claimed again …
    let first = request(&w, &[("2026-11-21", 0.5)]).await;
    let err = rejected_with(
        &w,
        comp_off_body(&[("2026-11-22", 1.0), ("2026-11-21", 0.5)]),
    )
    .await;
    assert_eq!(err["code"], "COMP_OFF_DATE_INVALID");
    assert_eq!(err["details"]["date"], "2026-11-21");
    // … until that request is rejected.
    let (status, _) = decide_kind(
        &w.app,
        &w.priya,
        "comp_off",
        &first,
        json!({ "decision": "reject", "version": 1, "comment": "Not on the rota" }),
    )
    .await;
    assert_eq!(status, 200);
    request(&w, &[("2026-11-21", 1.0)]).await;
}

#[tokio::test]
async fn a_pending_request_can_be_withdrawn() {
    let w = world().await;
    let id = request(&w, &[("2026-11-21", 1.0)]).await;
    let url = w.app.url(&format!("/api/v1/comp-offs/{id}/cancel"));
    let (status, out) = post_json(&w.priya, url.clone(), json!({})).await;
    assert_eq!(status, 403, "{out}");
    let (status, _) = post_json(&w.ravi, url.clone(), json!({})).await;
    assert_eq!(status, 404);

    let (status, out) = post_json(&w.asha, url.clone(), json!({})).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "cancelled");
    assert_eq!(
        comp_offs(&w.app, &w.asha).await["requests"][0]["status"],
        "cancelled"
    );
    let (_, inbox) = get_json(&w.priya, w.app.url("/api/v1/approvals?kind=comp_off")).await;
    assert_eq!(inbox, json!([]));
    let (status, out) = post_json(&w.asha, url, json!({})).await;
    assert_eq!(status, 409, "{out}");
    assert_eq!(out["error"]["code"], "ALREADY_DECIDED");
    // The date is free again.
    request(&w, &[("2026-11-21", 1.0)]).await;
}

#[tokio::test]
async fn approval_creates_credits_that_expire_after_the_org_period() {
    let w = world().await;
    let id = request(
        &w,
        &[
            ("2026-11-21", 1.0),
            ("2026-11-22", 0.5),
            ("2026-11-09", 1.0),
        ],
    )
    .await;
    mail(&w).await;
    let (status, out) = decide_kind(&w.app, &w.priya, "comp_off", &id, approve()).await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "approved");
    assert_eq!(out["decided_by"]["full_name"], "Priya Shah");

    let mine = comp_offs(&w.app, &w.asha).await;
    assert_eq!(mine["available"], 2.5);
    assert_eq!(
        mine["next_expiry"],
        json!({ "date": "2027-02-07", "days": 1.0 })
    );
    let credits: Vec<(String, f64, String, String)> = mine["credits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            assert_eq!(c["approved_by"]["full_name"], "Priya Shah");
            assert_eq!(c["days_used"], 0.0);
            (
                c["worked_date"].as_str().unwrap().into(),
                c["days"].as_f64().unwrap(),
                c["expires_on"].as_str().unwrap().into(),
                c["status"].as_str().unwrap().into(),
            )
        })
        .collect();
    assert_eq!(
        credits,
        [
            (
                "2026-11-09".into(),
                1.0,
                "2027-02-07".into(),
                "active".into()
            ),
            (
                "2026-11-21".into(),
                1.0,
                "2027-02-19".into(),
                "active".into()
            ),
            (
                "2026-11-22".into(),
                0.5,
                "2027-02-20".into(),
                "active".into()
            ),
        ]
    );
    let ledger: Vec<(String, rust_decimal::Decimal)> = ledger_rows(&w.app, w.asha_user.id, w.co)
        .await
        .into_iter()
        .map(|(r, d, _, _)| (r, d))
        .collect();
    assert_eq!(ledger.len(), 3);
    assert!(ledger.iter().all(|(r, _)| r == "comp_off_credit"));
    assert_eq!(
        ledger
            .iter()
            .map(|(_, d)| *d)
            .sum::<rust_decimal::Decimal>(),
        dec!(2.5)
    );
    let card = balance_card(&w.app, &w.asha, "CO", None).await;
    assert_eq!(card["available"], 2.5);
    assert_eq!(
        card["next_expiry"],
        json!({ "date": "2027-02-07", "days": 1.0 })
    );
    assert_eq!(mine["requests"][0]["status"], "approved");

    let sent = mail(&w).await;
    let decided = to(&sent, "asha@acme.test");
    assert!(decided.1.contains("approved"), "{}", decided.1);
    assert!(decided.2.contains("2.5 days"), "{}", decided.2);
    let fyi = to(&sent, "ravi@acme.test");
    assert!(fyi.2.contains("approved"), "{}", fyi.2);

    // Deciding again loses the race.
    let (status, _) = decide_kind(&w.app, &w.priya, "comp_off", &id, approve()).await;
    assert_eq!(status, 409);
}

#[tokio::test]
async fn rejection_needs_a_comment_and_creates_no_credit() {
    let w = world().await;
    let id = request(&w, &[("2026-11-21", 1.0)]).await;
    let reject = json!({ "decision": "reject", "version": 1 });
    let (status, out) = decide_kind(&w.app, &w.priya, "comp_off", &id, reject).await;
    assert_eq!(status, 422);
    assert_eq!(out["error"]["code"], "COMMENT_REQUIRED");
    let (status, out) = decide_kind(
        &w.app,
        &w.vik,
        "comp_off",
        &id,
        json!({ "decision": "approve", "version": 1 }),
    )
    .await;
    assert_eq!(status, 403);
    assert_eq!(out["error"]["code"], "NOT_ELIGIBLE_APPROVER");
    let (status, _) = decide_kind(&w.app, &w.asha, "comp_off", &id, approve()).await;
    assert_eq!(status, 403);
    mail(&w).await;
    let (status, out) = decide_kind(
        &w.app,
        &w.priya,
        "comp_off",
        &id,
        json!({ "decision": "reject", "version": 1, "comment": "Not on the rota" }),
    )
    .await;
    assert_eq!(status, 200, "{out}");
    assert_eq!(out["status"], "rejected");
    let mine = comp_offs(&w.app, &w.asha).await;
    assert_eq!(mine["credits"], json!([]));
    assert_eq!(mine["available"], 0.0);
    assert_eq!(mine["requests"][0]["decision_comment"], "Not on the rota");
    let sent = mail(&w).await;
    assert!(to(&sent, "asha@acme.test").2.contains("Not on the rota"));
}

#[tokio::test]
async fn credit_is_not_tied_to_a_leave_year() {
    let w = world().await;
    // Worked Sat 12 Dec 2026, approved in December.
    w.app
        .clock
        .set(Utc.with_ymd_and_hms(2026, 12, 15, 4, 30, 0).unwrap());
    earn_comp_off(&w.app, &w.asha, &w.priya, &[("2026-12-12", 1.0)]).await;
    // In January the new leave year has started; the credit is still there and spendable.
    w.app
        .clock
        .set(Utc.with_ymd_and_hms(2027, 1, 5, 4, 30, 0).unwrap());
    let asha = long_session(&w.app, &w.asha_user).await;
    let card = balance_card(&w.app, &asha, "CO", None).await;
    assert_eq!(card["leave_year"], 2027);
    assert_eq!(card["available"], 1.0);
    submit(&w.app, &asha, &leave_body(w.co, "2027-01-11", "2027-01-11")).await;
}
